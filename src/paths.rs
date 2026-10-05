//! XDG-compliant path helpers for to-do-or-die
//!
//! All data is stored in standard XDG locations:
//! - Config: ~/.config/to-do-or-die/config.toml
//! - Data: ~/.local/share/to-do-or-die/todos.json
//! - State/Logs: ~/.local/state/to-do-or-die/audit.jsonl
//!
//! Atomic writes are used for config and todos to prevent corruption
//! from crashes or concurrent access. A lock file prevents two checker
//! processes from running simultaneously.

use directories::ProjectDirs;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

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

/// Path to lock file: ~/.local/state/to-do-or-die/check.lock
pub fn lock_file() -> PathBuf {
    state_dir().join("check.lock")
}

/// Path to assets directory: ~/.local/share/to-do-or-die/assets/
pub fn assets_dir() -> PathBuf {
    data_dir().join("assets")
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
    if let Some(stripped) = path.strip_prefix("~/") {
        let home = dirs::home_dir().expect("Could not determine home directory");
        home.join(stripped)
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

/// Write data to a file atomically by writing to a temporary file first,
/// then renaming. This prevents corruption if the process is killed
/// mid-write (e.g., power loss, signal, crash).
///
/// The rename syscall is atomic on POSIX systems when both files are on
/// the same filesystem, which is guaranteed here since the temp file is
/// created in the same directory.
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write to a temporary file in the same directory (same filesystem required for atomic rename)
    let tmp_path = path.with_extension("tmp");

    let mut file = fs::File::create(&tmp_path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?; // Flush to disk before rename
    drop(file);

    // Atomic rename (on POSIX, same-filesystem rename is atomic)
    fs::rename(&tmp_path, path)?;

    Ok(())
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

    #[test]
    fn test_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        atomic_write(&path, "{\"hello\": \"world\"}").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "{\"hello\": \"world\"}");
        // Temp file should not exist after rename
        assert!(!path.with_extension("tmp").exists());
    }
}
