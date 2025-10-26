use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub eleven_labs_key: Option<String>,
    pub ical_url: String,
}

/// Returns the path to the config file (nextcall.toml)
/// Checks current working directory first, then home directory
fn get_config_path() -> Result<Option<PathBuf>> {
    // Check current working directory first
    let cwd_config = PathBuf::from("nextcall.toml");
    if cwd_config.exists() {
        return Ok(Some(cwd_config));
    }

    // Check home directory
    let home = std::env::var("HOME")
        .context("Failed to get HOME environment variable")?;

    let home_config = PathBuf::from(home).join("nextcall.toml");
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

    let contents = fs::read_to_string(&config_path)
        .context("Failed to read config file")?;

    let config: Config = toml::from_str(&contents)
        .context("Failed to parse config file")?;

    Ok(Some(config))
}
