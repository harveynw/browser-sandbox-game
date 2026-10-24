//! Local persistence configuration; networking retains the existing loopback-only bindings.
use serde::Deserialize;
use std::path::{Path, PathBuf};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub persistence: Persistence,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Persistence {
    pub mode: Mode,
    pub path: Option<PathBuf>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Sqlite,
    Memory,
}
impl Config {
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
        Ok(config)
    }
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
