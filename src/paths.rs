//! XDG-compliant path helpers for to-do-or-die
//!
//! All data is stored in standard XDG locations:
//! - Config: ~/.config/to-do-or-die/config.toml
//! - Data: ~/.local/share/to-do-or-die/todos.json
//! - State/Logs: ~/.local/state/to-do-or-die/audit.jsonl

use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

/// Get project directories following XDG Base Directory spec
fn project_dirs() -> ProjectDirs {
    ProjectDirs::from("", "", "to-do-or-die").expect("Could not determine home directory")
}

/// Config directory: ~/.config/to-do-or-die/
pub fn config_dir() -> PathBuf {
    project_dirs().config_dir().to_path_buf()
}

/// Data directory: ~/.local/share/to-do-or-die/
pub fn data_dir() -> PathBuf {
    project_dirs().data_dir().to_path_buf()
}

/// State directory: ~/.local/state/to-do-or-die/
/// Used for logs and runtime state
pub fn state_dir() -> PathBuf {
    // ProjectDirs doesn't have state_dir, so we construct it manually
    let home = dirs::home_dir().expect("Could not determine home directory");
    home.join(".local").join("state").join("to-do-or-die")
}

/// Path to config file: ~/.config/to-do-or-die/config.toml
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

/// Path to todos database: ~/.local/share/to-do-or-die/todos.json
pub fn todos_file() -> PathBuf {
    data_dir().join("todos.json")
}

/// Path to audit log: ~/.local/state/to-do-or-die/audit.jsonl
pub fn audit_log_file() -> PathBuf {
    state_dir().join("audit.jsonl")
}

/// Path to assets directory: ~/.local/share/to-do-or-die/assets/
pub fn assets_dir() -> PathBuf {
    data_dir().join("assets")
}

/// systemd user units directory: ~/.config/systemd/user/
pub fn systemd_user_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Could not determine home directory");
    home.join(".config").join("systemd").join("user")
}

/// Ensure all required directories exist
pub fn ensure_dirs() -> std::io::Result<()> {
    fs::create_dir_all(config_dir())?;
    fs::create_dir_all(data_dir())?;
    fs::create_dir_all(state_dir())?;
    fs::create_dir_all(assets_dir())?;
    Ok(())
}

/// Expand ~ to home directory in a path string
pub fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        let home = dirs::home_dir().expect("Could not determine home directory");
        home.join(&path[2..])
    } else if path == "~" {
        dirs::home_dir().expect("Could not determine home directory")
    } else {
        PathBuf::from(path)
    }
}

/// Get the home directory
pub fn home_dir() -> PathBuf {
    dirs::home_dir().expect("Could not determine home directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~/Downloads"), home.join("Downloads"));
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(expand_tilde("/tmp/test"), PathBuf::from("/tmp/test"));
    }

    #[test]
    fn test_paths_are_under_home() {
        let home = dirs::home_dir().unwrap();
        assert!(config_dir().starts_with(&home));
        assert!(data_dir().starts_with(&home));
        assert!(state_dir().starts_with(&home));
    }
}
