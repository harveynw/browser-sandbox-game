//! Manages local guest identity, one-use join tickets, active sessions and committed profile data.
//! `control` uses it to create or reuse guests, issue tickets and record transport readiness; `world`
//! consumes those tickets, waits for profile admission and tracks active sessions. `jobs` and `repairs`
//! use it to commit rewards, repair charges and refunds without applying an event twice. `config`
//! selects in-memory storage or the SQLite worker in `persistence`, whose commands and results are
//! queued and polled through this module; shared profile data uses the types defined in `protocol`.
use crate::persistence::{Command, SqliteWorker, StoreError};
use protocol::{Credits, DisplayName, OpaqueId, ProfileView};
use std::collections::{BTreeMap, BTreeSet};
/// Generate a prefixed hexadecimal ID from the requested number of OS-random bytes.
pub fn random_id(prefix: &str, bytes: usize) -> Result<String, String> {
    let mut secret = vec![0; bytes];
    getrandom::fill(&mut secret).map_err(|_| "OS randomness unavailable")?;
    Ok(format!(
        "{prefix}{}",
        secret
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
}
/// Hash a guest cookie or join-ticket secret with SHA-256 before indexing or storage.
fn digest(secret: &str) -> String {
    content::canonical::sha256(secret.as_bytes())
}
struct Profile {
    view: ProfileView,
    cookie_hash: String,
}
struct JoiningTicket {
    profile_id: String,
    expires: u64,
}
struct PendingEconomic {
    profile: String,
    refund: bool,
    ready: u32,
}
#[derive(Debug, PartialEq, Eq)]
pub enum EconomicError {
    InsufficientFunds,
    Unavailable,
}
pub struct EconomicResult {
    pub event: String,
    pub result: Result<ProfileView, EconomicError>,
}
#[derive(Default)]
pub struct Profiles {
    // Optional worker owning the SQLite connection; absence selects in-memory storage.
    worker: Option<SqliteWorker>,
    // Submitted persistence commands keyed by operation ID, retained until completion.
    outstanding: BTreeMap<String, Command>,
    // Completed persistence results keyed by operation ID, waiting for their callers.
    completed: BTreeMap<String, Result<ProfileView, StoreError>>,
    // Earliest simulation tick at which each failed persistence operation can be retried.
    retry_after: BTreeMap<String, u32>,
    // Latest simulation tick supplied when polling persistence results.
    store_tick: u32,
    // Tracks an observed storage-unavailable result until a later operation succeeds.
    store_unavailable: bool,
    // Repair event IDs mapped to profile IDs, awaiting durable debit acknowledgement.
    finalizations: BTreeMap<String, String>,
    // Committed profile records cached by profile ID.
    profiles: BTreeMap<String, Profile>,
    // Guest cookie hashes mapped to their cached profile IDs.
    cookies: BTreeMap<String, String>,
    // Hashed one-use join tickets mapped to their profile and expiry time.
    tickets: BTreeMap<String, JoiningTicket>,
    // Profile IDs mapped to their currently active gameplay session IDs.
    active: BTreeMap<String, String>,
    // In-memory event ledger mapping IDs to profile, credit delta and completed-job flag.
    economic_events: BTreeMap<String, (String, i64, bool)>,
    // In-memory repair debits and refunds waiting for their simulated completion tick.
    pending_economic: BTreeMap<String, PendingEconomic>,
    // In-memory ledger capacity reserved for refunds of unacknowledged repair debits.
    reserved_refunds: BTreeSet<String>,
    /// The memory adapter exposes delayed completion like the SQLite worker.
    pub repair_delay_ticks: u32,
    /// Monotonic expiry time in milliseconds of the last accepted WS/RTC probe report.
    pub probe_until_ms: u64,
}
impl Profiles {
    /// Reuse or create an in-memory guest profile, returning a cookie secret only for a new guest.
    pub fn session(
        &mut self,
        cookie: Option<&str>,
    ) -> Result<(ProfileView, Option<String>), String> {
        if self.worker.is_some() {
            return Err("use asynchronous profile preparation".into());
        }
        if let Some(profile) = self.profile(cookie) {
            return Ok((profile, None));
        }
        if self.profiles.len() >= 256 {
            return Err("local profile capacity reached; restart memory server".into());
        }
        let secret = random_id("", 32)?;
        let cookie_hash = digest(&secret);
        let id = random_id("profile-", 16)?;
        let view = ProfileView {
            tshirt_colour: Default::default(),
            profile_id: OpaqueId(id.clone()),
            display_name: DisplayName("Local Guest".into()),
            credits: Credits(0),
            completed_jobs: 0,
        };
        self.cookies.insert(cookie_hash.clone(), id.clone());
        self.profiles.insert(
            id,
            Profile {
                view: view.clone(),
                cookie_hash,
            },
        );
        Ok((view, Some(secret)))
    }
    /// Look up a committed cached profile using its guest cookie secret.
    pub fn profile(&self, cookie: Option<&str>) -> Option<ProfileView> {
        let hash = digest(cookie?);
        let id = self.cookies.get(&hash)?;
        let profile = self.profiles.get(id)?;
        (profile.cookie_hash == hash).then(|| profile.view.clone())
    }
    /// Apply a mission reward once, returning its committed profile or queuing a durable commit.
    pub fn apply_mission_reward(
        &mut self,
        profile_id: &str,
        instance_id: &str,
        amount: u32,
    ) -> Result<ProfileView, String> {
        if self.worker.is_some() {
            let event = format!("mission:{instance_id}");
            if let Some(result) = self.completed.remove(&event) {
                return result.map_err(|_| "persistent reward temporarily unavailable".into());
            }
            self.queue_store(
                &event,
                Command::Economic {
                    profile: profile_id.into(),
                    event: event.clone(),
                    delta: i64::from(amount),
                    job: true,
                },
            )?;
            return Err("reward commit pending".into());
        }
        self.apply_economic(
            profile_id,
            &format!("mission:{instance_id}"),
            i64::from(amount),
            true,
            0,
        )
    }
    /// Apply an in-memory credit change once, checking event conflicts, balance bounds and ledger capacity.
    fn apply_economic(
        &mut self,
        profile_id: &str,
        event: &str,
        delta: i64,
        job: bool,
        extra_slots: usize,
    ) -> Result<ProfileView, String> {
        let profile = self
            .profiles
            .get(profile_id)
            .ok_or("unknown economic profile")?;
        if let Some((owner, previous, previous_job)) = self.economic_events.get(event) {
            if owner != profile_id || *previous != delta || *previous_job != job {
                return Err("economic event conflict".into());
            }
            return Ok(profile.view.clone());
        }
        if self.economic_events.len() + self.reserved_refunds.len() + 1 + extra_slots > 4096 {
            return Err("memory economic event capacity reached; restart required".into());
        }
        let credits = i64::from(profile.view.credits.0) + delta;
        if !(0..=1_000_000_000).contains(&credits) {
            return Err("credit bound exceeded".into());
        }
        let mut view = profile.view.clone();
        view.credits = Credits(credits as u32);
        if job {
            view.completed_jobs = view
                .completed_jobs
                .checked_add(1)
                .ok_or("job count exhausted")?;
        }
        self.economic_events
            .insert(event.into(), (profile_id.into(), delta, job));
        self.profiles
            .get_mut(profile_id)
            .ok_or("unknown profile")?
            .view = view.clone();
        Ok(view)
    }
    /// Return the latest committed cached profile by profile ID.
    pub fn committed(&self, profile: &str) -> Option<ProfileView> {
        self.profiles.get(profile).map(|p| p.view.clone())
    }
    /// Queue a 50-credit repair debit or refund through SQLite or delayed in-memory completion.
    pub fn submit_repair_event(
        &mut self,
        profile: &str,
        event: &str,
        refund: bool,
        tick: u32,
    ) -> Result<(), String> {
        if self.worker.is_some() {
            return self.queue_store(
                event,
                Command::Economic {
                    profile: profile.into(),
                    event: event.into(),
                    delta: if refund { 50 } else { -50 },
                    job: false,
                },
            );
        }
        if let Some(p) = self.pending_economic.get(event) {
            return if p.profile == profile && p.refund == refund {
                Ok(())
            } else {
                Err("queued economic conflict".into())
            };
        }
        if self.pending_economic.len() >= 32 {
            return Err("memory worker full".into());
        }
        self.pending_economic.insert(
            event.into(),
            PendingEconomic {
                profile: profile.into(),
                refund,
                ready: tick.saturating_add(self.repair_delay_ticks.max(1)),
            },
        );
        Ok(())
    }
    /// Acknowledge an applied repair debit so it no longer requires automatic refund protection.
    pub fn finish_repair_debit(&mut self, profile: &str, event: &str) {
        if self.worker.is_some() {
            // Queue failure is retained for retry; crash before acknowledgement is compensated at boot.
            self.finalizations.insert(event.into(), profile.into());
        } else {
            self.reserved_refunds.remove(&format!("refund:{event}"));
        }
    }
    /// Collect completed repair debits and refunds, advancing delayed in-memory work when due.
    pub fn poll_economic(&mut self, tick: u32) -> Vec<EconomicResult> {
        self.poll_store(tick);
        if self.worker.is_some() {
            let ids: Vec<_> = self
                .completed
                .keys()
                .filter(|id| id.starts_with("repair:") || id.starts_with("refund:"))
                .cloned()
                .collect();
            return ids
                .into_iter()
                .map(|event| EconomicResult {
                    result: self
                        .completed
                        .remove(&event)
                        .expect("completion")
                        .map_err(|e| {
                            if e == StoreError::InsufficientFunds {
                                EconomicError::InsufficientFunds
                            } else {
                                EconomicError::Unavailable
                            }
                        }),
                    event,
                })
                .collect();
        }
        let ready: Vec<_> = self
            .pending_economic
            .iter()
            .filter(|(_, p)| p.ready <= tick)
            .take(4)
            .map(|(id, _)| id.clone())
            .collect();
        let mut results = Vec::new();
        for event in ready {
            let pending = self
                .pending_economic
                .remove(&event)
                .expect("queued economic event");
            let result = if pending.refund {
                let debit = event
                    .strip_prefix("refund:")
                    .and_then(|id| self.economic_events.get(id));
                if debit.is_none_or(|(owner, delta, job)| {
                    owner != &pending.profile || *delta != -50 || *job
                }) || (!self.reserved_refunds.contains(&event)
                    && !self.economic_events.contains_key(&event))
                {
                    Err(EconomicError::Unavailable)
                } else {
                    // Replace the reserved refund slot atomically with its committed record.
                    let reserved = self.reserved_refunds.remove(&event);
                    let result = self
                        .apply_economic(&pending.profile, &event, 50, false, 0)
                        .map_err(|_| EconomicError::Unavailable);
                    if result.is_err() && reserved {
                        self.reserved_refunds.insert(event.clone());
                    }
                    result
                }
            } else if self
                .committed(&pending.profile)
                .is_some_and(|p| p.credits.0 < 50)
                && !self.economic_events.contains_key(&event)
            {
                Err(EconomicError::InsufficientFunds)
            } else {
                let cached = self.economic_events.contains_key(&event);
                let result = self
                    .apply_economic(&pending.profile, &event, -50, false, usize::from(!cached))
                    .map_err(|_| EconomicError::Unavailable);
                if result.is_ok() && !cached {
                    self.reserved_refunds.insert(format!("refund:{event}"));
                }
                result
            };
            results.push(EconomicResult { event, result });
        }
        results
    }
    /// Issue a join ticket valid for 30 seconds, replacing earlier tickets for the same guest.
    pub fn issue(&mut self, cookie: Option<&str>, now: u64) -> Result<String, String> {
        let profile = self.profile(cookie).ok_or("guest cookie required")?;
        self.tickets
            .retain(|_, t| t.expires > now && t.profile_id != profile.profile_id.0);
        if self.tickets.len() >= 64 {
            return Err("ticket capacity reached".into());
        }
        let ticket = random_id("", 32)?;
        self.tickets.insert(
            digest(&ticket),
            JoiningTicket {
                profile_id: profile.profile_id.0,
                expires: now.saturating_add(30_000),
            },
        );
        Ok(ticket)
    }
    /// Consume an unexpired in-memory join ticket and update the guest display name.
    pub fn consume(&mut self, ticket: &str, now: u64, name: DisplayName) -> Option<ProfileView> {
        if self.worker.is_some() {
            return None;
        }
        let ticket = self.tickets.remove(&digest(ticket))?;
        if ticket.expires <= now {
            return None;
        }
        let profile = self.profiles.get_mut(&ticket.profile_id)?;
        profile.view.display_name = name;
        Some(profile.view.clone())
    }
    /// Record the gameplay session currently active for a profile.
    pub fn activate(&mut self, profile: &str, session: &str) {
        self.active.insert(profile.into(), session.into());
    }
    /// Clear a profile's active session only if it still matches the session being removed.
    pub fn deactivate(&mut self, profile: &str, session: &str) {
        if self.active.get(profile).is_some_and(|s| s == session) {
            self.active.remove(profile);
        }
    }
    /// Trusted localhost browser reports actual WS/RTC pong receipt, not constructor success.
    pub fn report_probe(&mut self, cookie: Option<&str>, session: &str, now: u64) -> bool {
        let Some(profile) = self.profile(cookie) else {
            return false;
        };
        if self
            .active
            .get(&profile.profile_id.0)
            .is_none_or(|s| s != session)
        {
            return false;
        }
        self.probe_until_ms = now.saturating_add(5000);
        true
    }
}

/// Result of consuming one ticket. SQLite admission waits without allocating a game entity.
pub enum Admission {
    Ready(ProfileView),
    Pending(String),
}
impl Profiles {
    /// Create a profile manager backed by a dedicated SQLite worker.
    pub fn sqlite(path: &std::path::Path) -> Result<Self, String> {
        Ok(Self {
            worker: Some(SqliteWorker::open(path)?),
            ..Self::default()
        })
    }
    /// Check whether a profile has an outstanding economic commit or repair acknowledgement.
    pub fn economic_pending(&self, profile: &str) -> bool {
        self.finalizations.values().any(|p| p == profile)
            || self
                .outstanding
                .values()
                .any(|c| matches!(c,Command::Economic {profile:p,..} if p==profile))
    }
    /// Report whether this profile manager uses SQLite persistence.
    pub fn durable(&self) -> bool {
        self.worker.is_some()
    }
    /// Report whether the SQLite worker is alive, or true for in-memory storage.
    pub fn healthy(&self) -> bool {
        self.worker.as_ref().is_none_or(SqliteWorker::healthy)
    }
    /// Report whether storage is healthy and has no unresolved unavailable result.
    pub fn available(&self) -> bool {
        self.healthy() && !self.store_unavailable
    }
    /// Submit a deduplicated persistence command while enforcing retry, capacity and per-profile limits.
    fn queue_store(&mut self, id: &str, command: Command) -> Result<(), String> {
        if let Some(old) = self.outstanding.get(id) {
            return if old == &command {
                Ok(())
            } else {
                Err("persistence operation conflict".into())
            };
        }
        if self.outstanding.len() >= 64
            || self
                .retry_after
                .get(id)
                .is_some_and(|t| *t > self.store_tick)
        {
            return Err("persistence busy; retry later".into());
        }
        if let Command::Economic { profile, .. } = &command
            && self
                .outstanding
                .values()
                .any(|c| matches!(c, Command::Economic {profile: p,..} if p == profile))
        {
            return Err("profile transaction pending".into());
        }
        self.worker
            .as_ref()
            .ok_or("SQLite disabled")?
            .submit(id.into(), command.clone())?;
        self.retry_after.remove(id);
        self.outstanding.insert(id.into(), command);
        Ok(())
    }
    /// Return a cached guest profile or queue its load/create, supplying a new cookie secret when needed.
    pub fn prepare_session(
        &mut self,
        cookie: Option<&str>,
    ) -> Result<(Option<ProfileView>, Option<String>), String> {
        if self.worker.is_none() {
            return self.session(cookie).map(|(p, s)| (Some(p), s));
        }
        if let Some(profile) = self.profile(cookie) {
            return Ok((Some(profile), None));
        }
        if self.profiles.len()
            + self
                .outstanding
                .values()
                .filter(|c| matches!(c, Command::Load { .. }))
                .count()
            >= 256
        {
            return Err("local profile cache capacity reached".into());
        }
        let valid = cookie.filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()));
        let secret = match valid {
            Some(s) => s.to_owned(),
            None => random_id("", 32)?,
        };
        let hash = digest(&secret);
        let op = format!("load:{hash}");
        if let Some(Err(_)) = self.completed.remove(&op) {
            return Err("profile storage temporarily unavailable".into());
        }
        self.queue_store(
            &op,
            Command::Load {
                hash,
                new_id: self
                    .outstanding
                    .get(&op)
                    .and_then(|c| match c {
                        Command::Load { new_id, .. } => Some(new_id.clone()),
                        _ => None,
                    })
                    .unwrap_or(random_id("profile-", 16)?),
            },
        )?;
        Ok((None, valid.is_none().then_some(secret)))
    }
    /// Consume a valid ticket and return the profile immediately or await its durable name update.
    pub fn begin_admission(
        &mut self,
        ticket: &str,
        now: u64,
        name: DisplayName,
        tshirt_colour: protocol::TshirtColour,
    ) -> Result<Admission, protocol::RejectCode> {
        if self.worker.is_none() {
            return self
                .consume(ticket, now, name)
                .map(|mut view| {
                    view.tshirt_colour = tshirt_colour;
                    self.profiles
                        .get_mut(&view.profile_id.0)
                        .expect("consumed profile")
                        .view = view.clone();
                    Admission::Ready(view)
                })
                .ok_or(protocol::RejectCode::InvalidTicket);
        }
        let id = format!("name:{}", digest(ticket));
        let ticket = self
            .tickets
            .remove(&digest(ticket))
            .ok_or(protocol::RejectCode::InvalidTicket)?;
        if ticket.expires <= now {
            return Err(protocol::RejectCode::InvalidTicket);
        }
        self.queue_store(
            &id,
            Command::Name {
                profile: ticket.profile_id,
                name: name.0,
                tshirt_colour: tshirt_colour.0,
            },
        )
        .map_err(|_| protocol::RejectCode::Unavailable)?;
        Ok(Admission::Pending(id))
    }
    /// Commit a shirt change without touching the name or balance; callers poll pending writes.
    pub fn update_tshirt_colour(
        &mut self,
        cookie: Option<&str>,
        colour: protocol::TshirtColour,
    ) -> Result<Option<ProfileView>, String> {
        let view = self.profile(cookie).ok_or("guest session required")?;
        if self.worker.is_none() {
            let p = self
                .profiles
                .get_mut(&view.profile_id.0)
                .expect("cached guest");
            p.view.tshirt_colour = colour;
            return Ok(Some(p.view.clone()));
        }
        let op = format!("colour:{}", view.profile_id.0);
        if let Some(result) = self.completed.remove(&op) {
            result.map_err(|_| "appearance storage unavailable")?;
        }
        // Serialize writes per profile, including requests from multiple tabs.
        if self.outstanding.contains_key(&op) {
            return Ok(None);
        }
        if view.tshirt_colour == colour {
            return Ok(Some(view));
        }
        self.queue_store(
            &op,
            Command::TshirtColour {
                profile: view.profile_id.0,
                colour: colour.0,
            },
        )?;
        Ok(None)
    }
    /// Take the completed profile or storage error for a pending admission operation.
    pub fn admission_result(&mut self, id: &str) -> Option<Result<ProfileView, StoreError>> {
        self.completed.remove(id)
    }
    /// Collect worker results, update committed caches and retry bookkeeping, and submit repair acknowledgements.
    pub fn poll_store(&mut self, tick: u32) {
        self.store_tick = tick;
        let results = self
            .worker
            .as_ref()
            .map(SqliteWorker::poll)
            .unwrap_or_default();
        for done in results {
            self.outstanding.remove(&done.id);
            if done.result == Err(StoreError::Unavailable) {
                self.store_unavailable = true;
            } else if done.result.is_ok() {
                self.store_unavailable = false;
            }
            if let Ok(view) = &done.result {
                if let Command::Load { hash, .. } = &done.command {
                    self.cookies.insert(hash.clone(), view.profile_id.0.clone());
                    self.profiles.insert(
                        view.profile_id.0.clone(),
                        Profile {
                            view: view.clone(),
                            cookie_hash: hash.clone(),
                        },
                    );
                } else if let Some(p) = self.profiles.get_mut(&view.profile_id.0) {
                    p.view = view.clone();
                }
            }
            if done.result == Err(StoreError::Unavailable) {
                self.retry_after
                    .insert(done.id.clone(), tick.saturating_add(30));
            }
            if let Command::FinishRepair { event } = &done.command {
                if done.result.is_ok() {
                    self.finalizations.remove(event);
                }
            } else if !matches!(done.command, Command::Load { .. }) || done.result.is_err() {
                self.completed.insert(done.id, done.result);
            }
        }
        while self.completed.len() > 128 {
            self.completed.pop_first();
        }
        self.retry_after.retain(|_, end| *end > tick);
        for (event, _) in self.finalizations.clone() {
            let _ = self.queue_store(&format!("finish:{event}"), Command::FinishRepair { event });
        }
    }
    /// Bounded shutdown drains committed worker outcomes; unresolved repair debits recover on boot.
    pub fn shutdown(&mut self, timeout: std::time::Duration) -> bool {
        let until = std::time::Instant::now() + timeout;
        while (!self.outstanding.is_empty() || !self.finalizations.is_empty())
            && std::time::Instant::now() < until
        {
            self.poll_store(self.store_tick.saturating_add(1));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let drained = self.outstanding.is_empty() && self.finalizations.is_empty();
        self.worker.take();
        drained
    }
}
