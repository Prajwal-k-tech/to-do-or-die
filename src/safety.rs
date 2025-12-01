//! File safety assessment and candidate gathering
//!
//! Determines which files are safe to delete based on:
//! - Tier 1: Cache, cookies, trash (always safe)
//! - Tier 2: User-configured directories
//! - Blocklist: Never delete these paths

use crate::config::Config;
use crate::paths;
use chrono::{Duration, Utc};
use glob::glob;
use rand::seq::SliceRandom;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Built-in blocklist - NEVER delete these
const BLOCKLIST_PATTERNS: &[&str] = &[
    // Our own files
    "~/.config/to-do-or-die",
    "~/.local/share/to-do-or-die",
    "~/.local/state/to-do-or-die",
    // Credentials and keys
    "~/.ssh",
    "~/.gnupg",
    "~/.password-store",
    "~/.config/*/credentials*",
    // Git
    "**/.git",
    // Important configs
    "~/.bashrc",
    "~/.zshrc",
    "~/.profile",
    "~/.config/systemd",
];

/// File extensions that should never be deleted
const PROTECTED_EXTENSIONS: &[&str] = &["key", "pem", "crt", "cer", "p12", "pfx", "gpg", "asc"];

/// Gather all deletion candidates based on config
pub fn gather_candidates(config: &Config) -> anyhow::Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    let blocklist = build_blocklist(config)?;

    // Tier 1: Cache and system-safe files
    if config.deletion.tier1_enabled {
        candidates.extend(gather_tier1_candidates(&blocklist, config)?);
    }

    // Tier 2: User-configured paths
    if config.deletion.tier2_enabled {
        candidates.extend(gather_tier2_candidates(&blocklist, config)?);
    }

    // Filter out any remaining blocklisted items
    candidates.retain(|p| !is_blocklisted(p, &blocklist));

    Ok(candidates)
}

/// Build the complete blocklist from built-in + user config
fn build_blocklist(config: &Config) -> anyhow::Result<HashSet<PathBuf>> {
    let mut blocklist = HashSet::new();

    // Add built-in blocklist patterns
    for pattern in BLOCKLIST_PATTERNS {
        let expanded = paths::expand_tilde(pattern);
        if let Ok(entries) = glob(&expanded.to_string_lossy()) {
            for entry in entries.flatten() {
                if let Ok(canonical) = entry.canonicalize() {
                    blocklist.insert(canonical);
                } else {
                    blocklist.insert(entry);
                }
            }
        }
        // Also add the pattern path itself
        blocklist.insert(expanded);
    }

    // Add user-configured blocklist
    for path in &config.blocklist.additional_paths {
        let expanded = paths::expand_tilde(path);
        blocklist.insert(expanded);
    }

    Ok(blocklist)
}

/// Check if a path is blocklisted
fn is_blocklisted(path: &Path, blocklist: &HashSet<PathBuf>) -> bool {
    // Check direct match
    if blocklist.contains(path) {
        return true;
    }

    // Check if path is under any blocklisted directory
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    for blocked in blocklist {
        if canonical.starts_with(blocked) {
            return true;
        }
    }

    // Check protected extensions
    if let Some(ext) = path.extension() {
        if PROTECTED_EXTENSIONS.contains(&ext.to_string_lossy().to_lowercase().as_str()) {
            return true;
        }
    }

    // Never delete hidden config files directly under home
    if let Some(home) = dirs::home_dir() {
        if path.parent() == Some(&home) {
            if let Some(name) = path.file_name() {
                let name = name.to_string_lossy();
                if name.starts_with('.') && !name.starts_with(".cache") {
                    return true;
                }
            }
        }
    }

    false
}

/// Gather Tier 1 candidates (cache, cookies, trash, old downloads)
fn gather_tier1_candidates(
    blocklist: &HashSet<PathBuf>,
    config: &Config,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    let home = paths::home_dir();

    // ~/.cache/* (excluding important subdirs)
    let cache_dir = home.join(".cache");
    if cache_dir.exists() {
        for entry in WalkDir::new(&cache_dir)
            .min_depth(1)
            .max_depth(3)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path().to_path_buf();
            if path.is_file() && !is_blocklisted(&path, blocklist) {
                candidates.push(path);
            }
        }
    }

    // Browser cookies
    if config.deletion.cookies_enabled {
        candidates.extend(gather_cookie_files(blocklist)?);
    }

    // ~/.local/share/Trash/*
    let trash_dir = home.join(".local/share/Trash/files");
    if trash_dir.exists() {
        for entry in WalkDir::new(&trash_dir)
            .min_depth(1)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path().to_path_buf();
            if path.is_file() && !is_blocklisted(&path, blocklist) {
                candidates.push(path);
            }
        }
    }

    // ~/Downloads old files
    let downloads_dir = home.join("Downloads");
    if downloads_dir.exists() {
        let age_threshold = Utc::now() - Duration::days(config.deletion.download_age_days as i64);

        for entry in WalkDir::new(&downloads_dir)
            .min_depth(1)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path().to_path_buf();
            if path.is_file() && !is_blocklisted(&path, blocklist) {
                // Check if file is old enough
                if let Ok(metadata) = path.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        let modified_time: chrono::DateTime<Utc> = modified.into();
                        if modified_time < age_threshold {
                            candidates.push(path);
                        }
                    }
                }
            }
        }

        // Also include temp/partial downloads regardless of age
        for pattern in &["*.tmp", "*.part", "*.crdownload"] {
            let full_pattern = downloads_dir.join(pattern);
            if let Ok(entries) = glob(&full_pattern.to_string_lossy()) {
                for entry in entries.flatten() {
                    if !is_blocklisted(&entry, blocklist) {
                        candidates.push(entry);
                    }
                }
            }
        }
    }

    Ok(candidates)
}

/// Gather browser cookie files
fn gather_cookie_files(blocklist: &HashSet<PathBuf>) -> anyhow::Result<Vec<PathBuf>> {
    let mut cookies = Vec::new();
    let home = paths::home_dir();

    // Chrome cookies
    let chrome_cookies = [
        home.join(".config/google-chrome/Default/Cookies"),
        home.join(".config/google-chrome/Default/Cookies-journal"),
        home.join(".config/chromium/Default/Cookies"),
        home.join(".config/chromium/Default/Cookies-journal"),
    ];

    for path in chrome_cookies {
        if path.exists() && !is_blocklisted(&path, blocklist) {
            cookies.push(path);
        }
    }

    // Firefox cookies (need to find profile directories)
    let firefox_dir = home.join(".mozilla/firefox");
    if firefox_dir.exists() {
        let pattern = firefox_dir.join("*.default*/cookies.sqlite*");
        if let Ok(entries) = glob(&pattern.to_string_lossy()) {
            for entry in entries.flatten() {
                if !is_blocklisted(&entry, blocklist) {
                    cookies.push(entry);
                }
            }
        }
    }

    Ok(cookies)
}

/// Gather Tier 2 candidates (user-configured paths)
fn gather_tier2_candidates(
    blocklist: &HashSet<PathBuf>,
    config: &Config,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();

    for path_str in &config.deletion.tier2_paths {
        let expanded = paths::expand_tilde(path_str);

        if expanded.is_dir() {
            // Walk the directory
            for entry in WalkDir::new(&expanded)
                .min_depth(1)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path().to_path_buf();
                if path.is_file() && !is_blocklisted(&path, blocklist) {
                    candidates.push(path);
                }
            }
        } else if expanded.is_file() && !is_blocklisted(&expanded, blocklist) {
            candidates.push(expanded);
        }
    }

    Ok(candidates)
}

/// Select random files from candidates
pub fn select_random(candidates: &[PathBuf], count: usize) -> Vec<PathBuf> {
    let mut rng = rand::thread_rng();
    let mut shuffled = candidates.to_vec();
    shuffled.shuffle(&mut rng);
    shuffled.into_iter().take(count).collect()
}

/// Select random files with a specific seed (for reproducible testing)
pub fn select_random_seeded(candidates: &[PathBuf], count: usize, seed: u64) -> Vec<PathBuf> {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut shuffled = candidates.to_vec();
    shuffled.shuffle(&mut rng);
    shuffled.into_iter().take(count).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_protected_extensions() {
        let path = PathBuf::from("/home/user/.ssh/id_rsa.pem");
        let blocklist = HashSet::new();
        assert!(is_blocklisted(&path, &blocklist));
    }

    #[test]
    fn test_select_random() {
        let candidates: Vec<PathBuf> = (0..10)
            .map(|i| PathBuf::from(format!("/tmp/file{}", i)))
            .collect();

        let selected = select_random(&candidates, 3);
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn test_select_random_seeded() {
        let candidates: Vec<PathBuf> = (0..10)
            .map(|i| PathBuf::from(format!("/tmp/file{}", i)))
            .collect();

        let selected1 = select_random_seeded(&candidates, 3, 12345);
        let selected2 = select_random_seeded(&candidates, 3, 12345);

        assert_eq!(selected1, selected2); // Same seed = same selection
    }

    #[test]
    fn test_blocklist_prevents_our_files() {
        let our_config = PathBuf::from(
            dirs::home_dir()
                .unwrap()
                .join(".config/to-do-or-die/config.toml"),
        );

        // This should be in the blocklist
        let config = Config::default();
        let blocklist = build_blocklist(&config).unwrap();

        // Our config dir should be protected
        let our_dir = dirs::home_dir().unwrap().join(".config/to-do-or-die");
        assert!(
            blocklist
                .iter()
                .any(|p| our_dir.starts_with(p) || p.starts_with(&our_dir))
        );
    }

    #[test]
    fn test_gather_with_temp_files() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("test1.txt");
        let file2 = dir.path().join("test2.txt");

        File::create(&file1).unwrap();
        File::create(&file2).unwrap();

        let mut config = Config::default();
        config.deletion.tier1_enabled = false;
        config.deletion.tier2_enabled = true;
        config.deletion.tier2_paths = vec![dir.path().to_string_lossy().to_string()];

        let candidates = gather_candidates(&config).unwrap();

        // Should find our test files
        assert!(candidates.len() >= 2);
    }
}
