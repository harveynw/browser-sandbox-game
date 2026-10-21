//! Bounded localhost memory profiles/tickets. No disk I/O in the simulation tick.
use protocol::{Credits, DisplayName, OpaqueId, ProfileView};
use std::collections::BTreeMap;
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
#[derive(Default)]
pub struct MemoryProfiles {
    profiles: BTreeMap<String, Profile>,
    cookies: BTreeMap<String, String>,
    tickets: BTreeMap<String, Ticket>,
    active: BTreeMap<String, String>,
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
