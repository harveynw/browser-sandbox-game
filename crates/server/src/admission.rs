//! Bounded localhost memory profiles/tickets. No disk I/O in the simulation tick.
use protocol::{Credits, DisplayName, OpaqueId, ProfileView};
use std::collections::{BTreeMap, BTreeSet};
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
fn digest(secret: &str) -> String {
    content::canonical::sha256(secret.as_bytes())
}
struct Profile {
    view: ProfileView,
    cookie_hash: String,
}
struct Ticket {
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
pub struct MemoryProfiles {
    profiles: BTreeMap<String, Profile>,
    cookies: BTreeMap<String, String>,
    tickets: BTreeMap<String, Ticket>,
    active: BTreeMap<String, String>,
    economic_events: BTreeMap<String, (String, i64, bool)>,
    pending_economic: BTreeMap<String, PendingEconomic>,
    reserved_refunds: BTreeSet<String>,
    /// The memory worker uses the same delayed completion boundary as a future disk worker.
    pub repair_delay_ticks: u32,
    pub probe_until_ms: u64,
}
impl MemoryProfiles {
    pub fn session(
        &mut self,
        cookie: Option<&str>,
    ) -> Result<(ProfileView, Option<String>), String> {
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
    pub fn profile(&self, cookie: Option<&str>) -> Option<ProfileView> {
        let hash = digest(cookie?);
        let id = self.cookies.get(&hash)?;
        let profile = self.profiles.get(id)?;
        (profile.cookie_hash == hash).then(|| profile.view.clone())
    }
    /// Commit the reward and event atomically in memory; retries never award twice.
    pub fn apply_mission_reward(
        &mut self,
        profile_id: &str,
        instance_id: &str,
        amount: u32,
    ) -> Result<ProfileView, String> {
        self.apply_economic(
            profile_id,
            &format!("mission:{instance_id}"),
            i64::from(amount),
            true,
            0,
        )
    }
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
    pub fn committed(&self, profile: &str) -> Option<ProfileView> {
        self.profiles.get(profile).map(|p| p.view.clone())
    }
    pub fn submit_repair_event(
        &mut self,
        profile: &str,
        event: &str,
        refund: bool,
        tick: u32,
    ) -> Result<(), String> {
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
    pub fn finish_repair_debit(&mut self, event: &str) {
        self.reserved_refunds.remove(&format!("refund:{event}"));
    }
    pub fn poll_economic(&mut self, tick: u32) -> Vec<EconomicResult> {
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
            Ticket {
                profile_id: profile.profile_id.0,
                expires: now.saturating_add(30_000),
            },
        );
        Ok(ticket)
    }
    pub fn consume(&mut self, ticket: &str, now: u64, name: DisplayName) -> Option<ProfileView> {
        let ticket = self.tickets.remove(&digest(ticket))?;
        if ticket.expires <= now {
            return None;
        }
        let profile = self.profiles.get_mut(&ticket.profile_id)?;
        profile.view.display_name = name;
        Some(profile.view.clone())
    }
    pub fn activate(&mut self, profile: &str, session: &str) {
        self.active.insert(profile.into(), session.into());
    }
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
