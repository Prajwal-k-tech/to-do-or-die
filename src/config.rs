//! Configuration management for to-do-or-die
//!
//! Config is stored in TOML format at ~/.config/to-do-or-die/config.toml
//!
//! Safe defaults: new users start in dry-run mode with deletion disabled.
//! They must explicitly enable deletion after understanding the risks.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

use crate::paths;

#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Failed to parse config file: {0}")]
    ParseError(#[from] toml::de::Error),
    #[error("Failed to serialize config: {0}")]
    SerializeError(#[from] toml::ser::Error),
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    /// Whether to run in dry-run mode by default (default: true for safety)
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// Enable colored terminal output (default: true)
    #[serde(default = "default_true")]
    pub color: bool,
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
    /// Enable file deletion at all (default: false for safety)
    /// New users must explicitly opt in after understanding the risks
    #[serde(default = "default_false")]
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
    /// Permanently delete files instead of moving to trash (default: false)
    /// When false, files are moved to the system trash and are recoverable.
    /// When true, files are permanently deleted with no recovery path.
    #[serde(default = "default_false")]
    pub permanent_delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// Path to alert sound file, or "default" for built-in
    #[serde(default = "default_alert_sound")]
    pub alert_sound: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
fn default_false() -> bool {
    false
}
fn default_download_age_days() -> u32 {
    30
}
fn default_alert_sound() -> String {
    "default".to_string()
}

impl Default for GeneralConfig {
    fn default() -> Self {
        GeneralConfig {
            check_interval_minutes: default_check_interval(),
            escalation_cap: default_escalation_cap(),
            dry_run: true, // Safe default: no real deletions until user opts in
            color: true,
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
            enabled: false, // Safe default: user must explicitly enable
            tier1_enabled: true,
            tier2_enabled: true,
            tier2_paths: Vec::new(),
            cookies_enabled: true,
            download_age_days: default_download_age_days(),
            permanent_delete: false, // Safe default: trash, not permanent delete
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

impl Config {
    /// Load config from file, creating default if it doesn't exist
    pub fn load() -> Result<Self, ConfigError> {
        let path = paths::config_file();

        if !path.exists() {
            let config = Config::default();
            config.save()?;
            return Ok(config);
        }

        let content = fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load config from a specific path (for testing)
    #[allow(dead_code)]
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save config to file atomically
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = paths::config_file();
        let content = toml::to_string_pretty(self)?;
        paths::atomic_write(&path, &content)?;
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
        assert!(config.general.dry_run); // Safe default
        assert!(!config.deletion.enabled); // Safe default
        assert!(!config.deletion.permanent_delete); // Safe default
        assert!(config.deletion.tier1_enabled);
        assert!(config.deletion.cookies_enabled);
        assert!(config.general.color);
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
        config.deletion.enabled = true;
        config.deletion.permanent_delete = true;

        let content = toml::to_string_pretty(&config).unwrap();
        fs::write(&path, &content).unwrap();

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.general.escalation_cap, 15);
        assert_eq!(loaded.deletion.tier2_paths.len(), 1);
        assert!(loaded.deletion.enabled);
        assert!(loaded.deletion.permanent_delete);
    }
}
