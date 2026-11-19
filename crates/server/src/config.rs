//! Persistence and optional public playtest configuration.
use serde::Deserialize;
use std::path::{Path, PathBuf};
/// Selects profile storage and public network settings.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Profile storage settings.
    pub persistence: Persistence,
    /// Public hosting settings.
    #[serde(default)]
    pub network: NetworkConfig,
}
/// Defines the public origin and advertised WebRTC address.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfig {
    /// Allowed public HTTPS origin, if public hosting is enabled.
    pub public_origin: Option<String>,
    /// Concrete host address advertised to WebRTC clients.
    pub webrtc_host: Option<std::net::IpAddr>,
}
/// Selects the profile storage mode and database path.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Persistence {
    /// Selected profile storage mode.
    pub mode: Mode,
    /// Path to the profile database, if SQLite is selected.
    pub path: Option<PathBuf>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Sqlite,
    Memory,
}
impl Config {
    /// Reads local TOML settings and validates storage and public-hosting requirements.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read local config: {e}"))?;
        let config: Self =
            toml::from_str(&text).map_err(|e| format!("invalid local config: {e}"))?;
        if matches!(config.persistence.mode, Mode::Sqlite)
            && config
                .persistence
                .path
                .as_ref()
                .is_none_or(|p| p.as_os_str().is_empty())
        {
            return Err("SQLite mode requires a database path".into());
        }
        if let Some(origin) = &config.network.public_origin {
            if !origin.starts_with("https://")
                || origin.len() <= 8
                || origin[8..].contains(['/', '?', '#', '@'])
                || origin.chars().any(char::is_whitespace)
                || config
                    .network
                    .webrtc_host
                    .is_none_or(|ip| ip.is_unspecified() || ip.is_multicast())
            {
                return Err("public hosting requires an HTTPS origin without a path and a concrete WebRTC host".into());
            }
        }
        Ok(config)
    }
    /// Opens the profile store selected by this configuration.
    pub fn profiles(&self, repository: &Path) -> Result<crate::admission::Profiles, String> {
        match self.persistence.mode {
            Mode::Memory => Ok(crate::admission::Profiles::default()),
            Mode::Sqlite => crate::admission::Profiles::sqlite(
                &repository.join(
                    self.persistence
                        .path
                        .as_ref()
                        .ok_or("missing database path")?,
                ),
            ),
        }
    }
}
