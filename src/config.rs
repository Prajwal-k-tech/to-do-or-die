//! Configuration management for to-do-or-die
//!
//! Config is stored in TOML format at ~/.config/to-do-or-die/config.toml

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

use crate::paths;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Failed to parse config file: {0}")]
    ParseError(#[from] toml::de::Error),
    #[error("Failed to serialize config: {0}")]
    SerializeError(#[from] toml::ser::Error),
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub deletion: DeletionConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub blocklist: BlocklistConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Timer interval in minutes (default: 5)
    #[serde(default = "default_check_interval")]
    pub check_interval_minutes: u32,
    /// Maximum files to delete per check (default: 10)
    #[serde(default = "default_escalation_cap")]
    pub escalation_cap: u32,
    /// Whether to run in dry-run mode by default (default: false)
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub sound_enabled: bool,
    #[serde(default = "default_true")]
    pub tts_enabled: bool,
    #[serde(default = "default_true")]
    pub wallpaper_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Enable Tier 1: cache, cookies, trash (default: true)
    #[serde(default = "default_true")]
    pub tier1_enabled: bool,
    /// Enable Tier 2: user-configured paths (default: true)
    #[serde(default = "default_true")]
    pub tier2_enabled: bool,
    /// User-configured safe directories for Tier 2
    #[serde(default)]
    pub tier2_paths: Vec<String>,
    /// Enable browser cookie deletion specifically (default: true)
    #[serde(default = "default_true")]
    pub cookies_enabled: bool,
    /// Age in days for old downloads to be considered deletable (default: 30)
    #[serde(default = "default_download_age_days")]
    pub download_age_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// Path to alert sound file, or "default" for built-in
    #[serde(default = "default_alert_sound")]
    pub alert_sound: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlocklistConfig {
    /// Additional paths to never delete (beyond built-in blocklist)
    #[serde(default)]
    pub additional_paths: Vec<String>,
}

// Default value functions
fn default_check_interval() -> u32 {
    5
}
fn default_escalation_cap() -> u32 {
    10
}
fn default_true() -> bool {
    true
}
fn default_download_age_days() -> u32 {
    30
}
fn default_alert_sound() -> String {
    "default".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            general: GeneralConfig::default(),
            notifications: NotificationConfig::default(),
            deletion: DeletionConfig::default(),
            audio: AudioConfig::default(),
            blocklist: BlocklistConfig::default(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        GeneralConfig {
            check_interval_minutes: default_check_interval(),
            escalation_cap: default_escalation_cap(),
            dry_run: false,
        }
    }
}

impl Default for NotificationConfig {
    fn default() -> Self {
        NotificationConfig {
            enabled: true,
            sound_enabled: true,
            tts_enabled: true,
            wallpaper_enabled: true,
        }
    }
}

impl Default for DeletionConfig {
    fn default() -> Self {
        DeletionConfig {
            enabled: true,
            tier1_enabled: true,
            tier2_enabled: true,
            tier2_paths: Vec::new(),
            cookies_enabled: true,
            download_age_days: default_download_age_days(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig {
            alert_sound: default_alert_sound(),
        }
    }
}

impl Default for BlocklistConfig {
    fn default() -> Self {
        BlocklistConfig {
            additional_paths: Vec::new(),
        }
    }
}

impl Config {
    /// Load config from file, creating default if it doesn't exist
    pub fn load() -> Result<Self, ConfigError> {
        let path = paths::config_file();

        if !path.exists() {
            // Create default config
            let config = Config::default();
            config.save()?;
            return Ok(config);
        }

        let content = fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load config from a specific path
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save config to file
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = paths::config_file();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// Reset config to defaults
    pub fn reset() -> Result<Self, ConfigError> {
        let config = Config::default();
        config.save()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.general.check_interval_minutes, 5);
        assert_eq!(config.general.escalation_cap, 10);
        assert!(!config.general.dry_run);
        assert!(config.deletion.tier1_enabled);
        assert!(config.deletion.cookies_enabled);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.general.escalation_cap, parsed.general.escalation_cap);
    }

    #[test]
    fn test_config_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut config = Config::default();
        config.general.escalation_cap = 15;
        config.deletion.tier2_paths = vec!["~/Downloads/temp".to_string()];

        let content = toml::to_string_pretty(&config).unwrap();
        fs::write(&path, &content).unwrap();

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.general.escalation_cap, 15);
        assert_eq!(loaded.deletion.tier2_paths.len(), 1);
    }
}
