use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// Returns the path to the config file (~/.config/nextcall/config.txt)
fn get_config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .context("Failed to get HOME environment variable")?;

    let config_dir = PathBuf::from(home)
        .join(".config")
        .join("nextcall");

    Ok(config_dir.join("config.txt"))
}

/// Saves the ICS URL to the config file
pub fn set_config(ics_url: &str) -> Result<()> {
    let config_path = get_config_path()?;

    // Create parent directories if they don't exist
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .context("Failed to create config directory")?;
    }

    // Write the URL to the file
    fs::write(&config_path, ics_url)
        .context("Failed to write config file")?;

    Ok(())
}

/// Loads the ICS URL from the config file
/// Returns None if the config file doesn't exist
pub fn get_config() -> Result<Option<String>> {
    let config_path = get_config_path()?;

    // Return None if the config file doesn't exist
    if !config_path.exists() {
        return Ok(None);
    }

    let url = fs::read_to_string(&config_path)
        .context("Failed to read config file")?;

    // Trim whitespace from the URL
    Ok(Some(url.trim().to_string()))
}
