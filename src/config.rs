use anyhow::{Context, Result};
use serde::Deserialize;
use std::fmt;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub eleven_labs_key: Option<String>,
    pub ical_url: String,
}

/// Log-safe rendering: the API key is truncated to its first 5 characters so
/// pasting a log never leaks the full secret. Prefer this over `Debug` in logs.
impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ical_url: {}, eleven_labs_key: ", self.ical_url)?;
        match &self.eleven_labs_key {
            Some(key) => write!(f, "{}…", key.chars().take(5).collect::<String>()),
            None => write!(f, "unset"),
        }
    }
}

pub fn home() -> Result<String> {
    std::env::var("HOME").context("Failed to get HOME environment variable")
}

/// Returns the path to the config file (nextcall.toml)
/// Checks current working directory first, then home directory
fn get_config_path() -> Result<Option<PathBuf>> {
    // Check current working directory first
    let cwd_config = PathBuf::from("nextcall.toml");
    if cwd_config.exists() {
        return Ok(Some(cwd_config));
    }

    let home_config = PathBuf::from(home()?).join("nextcall.toml");
    if home_config.exists() {
        return Ok(Some(home_config));
    }

    Ok(None)
}

/// Loads the configuration from nextcall.toml
/// Returns None if the config file doesn't exist
pub fn get_config() -> Result<Option<Config>> {
    let config_path = match get_config_path()? {
        Some(path) => path,
        None => return Ok(None),
    };

    let contents = fs::read_to_string(&config_path).context("Failed to read config file")?;

    let config: Config = toml::from_str(&contents).context("Failed to parse config file")?;

    Ok(Some(config))
}
