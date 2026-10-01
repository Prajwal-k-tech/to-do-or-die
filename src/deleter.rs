//! File deletion and audit logging
//!
//! By default, files are moved to the system trash (recoverable).
//! When `permanent_delete` is enabled in config, files are permanently
//! deleted with `fs::remove_file`.
//!
//! Every deletion is logged to the audit log in JSONL format.

use crate::paths;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum DeleteError {
    #[error("Failed to delete file {path}: {reason}")]
    #[allow(dead_code)]
    DeletionFailed { path: String, reason: String },
    #[error("Failed to write audit log: {0}")]
    AuditError(#[from] std::io::Error),
}

/// Audit log entry
#[derive(Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: chrono::DateTime<Utc>,
    pub todo_id: String,
    pub action: String,
    pub path: Option<String>,
    pub message: Option<String>,
    pub stage: Option<u32>,
    pub dry_run: bool,
    pub success: bool,
    pub error: Option<String>,
    /// Whether the file was permanently deleted or moved to trash
    #[serde(default)]
    pub permanent: bool,
}

impl AuditEntry {
    /// Create a new deletion audit entry
    pub fn deletion(todo_id: &str, path: &Path, dry_run: bool, permanent: bool) -> Self {
        AuditEntry {
            timestamp: Utc::now(),
            todo_id: todo_id.to_string(),
            action: "delete".to_string(),
            path: Some(path.display().to_string()),
            message: None,
            stage: None,
            dry_run,
            success: true,
            error: None,
            permanent,
        }
    }

    /// Create an effect audit entry (notification, audio, tts, appearance)
    #[allow(dead_code)]
    pub fn effect(todo_id: &str, action: &str, message: &str, stage: u32) -> Self {
        AuditEntry {
            timestamp: Utc::now(),
            todo_id: todo_id.to_string(),
            action: action.to_string(),
            path: None,
            message: Some(message.to_string()),
            stage: Some(stage),
            dry_run: false,
            success: true,
            error: None,
            permanent: false,
        }
    }

    /// Mark as failed with error
    pub fn with_error(mut self, error: &str) -> Self {
        self.success = false;
        self.error = Some(error.to_string());
        self
    }
}

/// Delete a list of files and log each deletion.
///
/// If `permanent` is true, files are permanently deleted with `fs::remove_file`.
/// If false (default), files are moved to the system trash and are recoverable.
pub fn delete_files(
    files: &[std::path::PathBuf],
    todo_id: &str,
    permanent: bool,
) -> Result<usize, DeleteError> {
    let mut deleted_count = 0;

    for path in files {
        let entry = AuditEntry::deletion(todo_id, path, false, permanent);

        let result = if permanent {
            fs::remove_file(path)
        } else {
            trash::delete(path).map_err(std::io::Error::other)
        };

        match result {
            Ok(_) => {
                write_audit_entry(&entry)?;
                deleted_count += 1;
                let action = if permanent { "Deleted" } else { "Trashed" };
                println!("  [x] {}: {}", action, path.display());
            }
            Err(e) => {
                let entry = entry.with_error(&e.to_string());
                write_audit_entry(&entry)?;
                eprintln!("  [!] Failed to delete {}: {}", path.display(), e);
            }
        }
    }

    Ok(deleted_count)
}

/// Write an audit entry to the log file
fn write_audit_entry(entry: &AuditEntry) -> Result<(), DeleteError> {
    let log_path = paths::audit_log_file();

    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let json = serde_json::to_string(entry).map_err(std::io::Error::other)?;

    writeln!(file, "{}", json)?;
    file.sync_data()?;

    Ok(())
}

/// Read the audit log
pub fn read_audit_log() -> Result<Vec<AuditEntry>, DeleteError> {
    let log_path = paths::audit_log_file();

    if !log_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&log_path)?;
    let entries: Vec<AuditEntry> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    Ok(entries)
}

/// Get statistics from the audit log
pub fn audit_stats() -> Result<AuditStats, DeleteError> {
    let entries = read_audit_log()?;

    let total_deletions = entries
        .iter()
        .filter(|e| e.action == "delete" && e.success && !e.dry_run)
        .count();

    let total_effects = entries
        .iter()
        .filter(|e| e.action != "delete" && e.success)
        .count();

    let failed_deletions = entries
        .iter()
        .filter(|e| e.action == "delete" && !e.success)
        .count();

    Ok(AuditStats {
        total_deletions,
        total_effects,
        failed_deletions,
        total_entries: entries.len(),
    })
}

#[derive(Debug)]
pub struct AuditStats {
    pub total_deletions: usize,
    pub total_effects: usize,
    pub failed_deletions: usize,
    #[allow(dead_code)]
    pub total_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_audit_entry_serialization() {
        let entry = AuditEntry::deletion("test-123", Path::new("/tmp/test.txt"), false, false);
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: AuditEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.todo_id, "test-123");
        assert_eq!(parsed.action, "delete");
        assert!(parsed.success);
        assert!(!parsed.permanent);
    }

    #[test]
    fn test_delete_files_permanent() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("test1.txt");
        let file2 = dir.path().join("test2.txt");

        File::create(&file1).unwrap();
        File::create(&file2).unwrap();

        assert!(file1.exists());
        assert!(file2.exists());

        let files = vec![file1.clone(), file2.clone()];
        let deleted = delete_files(&files, "test-todo", true).unwrap();

        assert_eq!(deleted, 2);
        assert!(!file1.exists());
        assert!(!file2.exists());
    }
}
