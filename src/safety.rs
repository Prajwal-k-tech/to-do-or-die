//! File safety assessment and candidate gathering
//!
//! Determines which files are safe to delete based on:
//! - Tier 1: Built-in cache, cookie, trash, and old-download candidates (review before enabling)
//! - Tier 2: User-configured directories
//! - Blocklist: Never delete these paths
//!
//! Safety is the highest priority in this module. The blocklist uses
//! path-component matching (not substring) to avoid false positives and
//! false negatives. All Tier 2 paths are canonicalized before walking
//! to prevent symlink escape attacks.

use crate::config::Config;
use crate::paths;
use chrono::{Duration, Utc};
use glob::glob;
use rand::seq::SliceRandom;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

/// Built-in protected directory names (checked as path components, not substrings).
/// If any path component matches one of these, the file is protected.
const PROTECTED_DIR_COMPONENTS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".password-store",
    ".git",
    ".config/to-do-or-die",
    ".local/share/to-do-or-die",
    ".local/state/to-do-or-die",
    ".config/systemd",
];

/// Shell config files that should never be deleted
const PROTECTED_SHELL_CONFIGS: &[&str] = &[
    ".bashrc",
    ".zshrc",
    ".profile",
    ".bash_profile",
    ".zprofile",
    ".config/fish/config.fish",
    ".config/nushell/config.nu",
    ".config/elvish/rc.elv",
    ".xonshrc",
];

/// File extensions that should never be deleted (cryptographic material)
const PROTECTED_EXTENSIONS: &[&str] = &["key", "pem", "crt", "cer", "p12", "pfx", "gpg", "asc"];

/// System root directories whose descendants must never be used as deletion targets
const FORBIDDEN_TARGET_ROOTS: &[&str] = &[
    "/", "/boot", "/dev", "/etc", "/proc", "/run", "/sys", "/usr", "/var", "/bin", "/sbin", "/lib",
    "/lib64", "/root",
];

/// Gather all deletion candidates based on config
pub fn gather_candidates(config: &Config) -> anyhow::Result<Vec<PathBuf>> {
    let blocklist = build_blocklist(config)?;

    let mut candidates = Vec::new();

    if config.deletion.tier1_enabled {
        candidates.extend(gather_tier1_candidates(&blocklist, config)?);
    }

    if config.deletion.tier2_enabled {
        candidates.extend(gather_tier2_candidates(&blocklist, config)?);
    }

    // Final safety filter: remove any blocklisted items that slipped through
    candidates.retain(|p| !is_protected(p, &blocklist));

    Ok(candidates)
}

/// Build the complete blocklist from built-in patterns plus user config.
/// Returns a set of canonical paths that should never be deleted.
fn build_blocklist(config: &Config) -> anyhow::Result<HashSet<PathBuf>> {
    let mut blocklist = HashSet::new();
    let home = paths::home_dir();

    // Add our own directories (canonicalized)
    for dir in &[
        home.join(".config/to-do-or-die"),
        home.join(".local/share/to-do-or-die"),
        home.join(".local/state/to-do-or-die"),
    ] {
        if let Ok(canonical) = dir.canonicalize() {
            blocklist.insert(canonical);
        } else {
            blocklist.insert(dir.clone());
        }
    }

    // Add credential directories
    for dir in &[".ssh", ".gnupg", ".password-store", ".config/systemd"] {
        let path = home.join(dir);
        if let Ok(canonical) = path.canonicalize() {
            blocklist.insert(canonical);
        } else {
            blocklist.insert(path);
        }
    }

    // Add shell config files
    for file in PROTECTED_SHELL_CONFIGS {
        let path = paths::expand_tilde(file);
        if path.exists() {
            if let Ok(canonical) = path.canonicalize() {
                blocklist.insert(canonical);
            } else {
                blocklist.insert(path);
            }
        }
    }

    // Add credential glob patterns (~/.config/*/credentials*)
    let cred_pattern = home.join(".config").join("*/credentials*");
    if let Ok(entries) = glob(&cred_pattern.to_string_lossy()) {
        for entry in entries.flatten() {
            if let Ok(canonical) = entry.canonicalize() {
                blocklist.insert(canonical);
            }
        }
    }

    // Add user-configured blocklist paths
    for path in &config.blocklist.additional_paths {
        let expanded = paths::expand_tilde(path);
        if let Ok(canonical) = expanded.canonicalize() {
            blocklist.insert(canonical);
        } else {
            blocklist.insert(expanded);
        }
    }

    Ok(blocklist)
}

/// Check if a path is protected from deletion.
///
/// This is the single, consolidated protection check used everywhere.
/// It checks:
/// 1. Direct match or prefix match against the blocklist set
/// 2. Protected path components (.ssh, .gnupg, .git, etc.)
/// 3. Protected file extensions (.key, .pem, .crt, etc.)
/// 4. Hidden files directly under home (except .cache)
fn is_protected(path: &Path, blocklist: &HashSet<PathBuf>) -> bool {
    // 1. Check against blocklist set (canonical prefix match)
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if forbidden_root_for(&canonical).is_some() {
        return true;
    }

    for blocked in blocklist {
        if canonical == *blocked || canonical.starts_with(blocked) {
            return true;
        }
    }

    // 2. Check for protected path components (.ssh, .gnupg, .git, etc.)
    for component in path.components() {
        if let Component::Normal(name) = component {
            let name_str = name.to_string_lossy();
            if PROTECTED_DIR_COMPONENTS.contains(&name_str.as_ref()) {
                return true;
            }
            // Also check multi-segment components like ".config/to-do-or-die"
            for protected in PROTECTED_DIR_COMPONENTS {
                if protected.contains('/') && name_str.as_ref() == *protected {
                    return true;
                }
            }
        }
    }

    // Also check the full path string for multi-segment protected dirs
    let path_str = path.to_string_lossy();
    for protected in PROTECTED_DIR_COMPONENTS {
        if protected.contains('/') {
            // For multi-segment paths like ".config/to-do-or-die", check if the
            // path contains this as a directory prefix
            let protected_path = format!("/{}/", protected);
            let home_protected = format!("{}/", protected);
            if path_str.contains(&protected_path) || path_str.contains(&home_protected) {
                return true;
            }
        }
    }

    // 3. Check protected file extensions
    if let Some(ext) = path.extension() {
        let ext_lower = ext.to_string_lossy().to_lowercase();
        if PROTECTED_EXTENSIONS.contains(&ext_lower.as_str()) {
            return true;
        }
    }

    // 4. Never delete hidden config files directly under home (except .cache)
    if let Some(home) = dirs::home_dir()
        && path.parent() == Some(home.as_path())
        && let Some(name) = path.file_name()
    {
        let name = name.to_string_lossy();
        if name.starts_with('.') && name != ".cache" {
            return true;
        }
    }

    false
}

/// Public API: check if a path would be protected from deletion.
/// Used by the `blocklist check` command and `targets add` validation.
pub fn is_path_protected(path: &Path) -> bool {
    if let Ok(config) = Config::load() {
        let blocklist = build_blocklist(&config).unwrap_or_default();
        is_protected(path, &blocklist)
    } else {
        // Fallback: check components and extensions only
        let empty_set = HashSet::new();
        is_protected(path, &empty_set)
    }
}

/// Validate that a path is safe to add as a Tier 2 deletion target.
/// Returns Ok(()) if safe, Err with a message if dangerous.
pub fn validate_target_path(path: &Path) -> anyhow::Result<()> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    // Reject system roots and every path beneath them, not just the root itself.
    if let Some(forbidden) = forbidden_root_for(&canonical) {
        anyhow::bail!(
            "Cannot add '{}' because it is inside forbidden system directory '{}'. \
             This would be extremely dangerous.",
            canonical.display(),
            forbidden
        );
    }

    // Reject the home directory itself
    let home = paths::home_dir();
    if canonical == home {
        anyhow::bail!(
            "Cannot add your home directory '{}' as a deletion target. \
             This would expose all your personal files to deletion.",
            canonical.display()
        );
    }

    // Reject if the path is already protected
    if is_path_protected(&canonical) {
        anyhow::bail!(
            "Cannot add protected path: {}. \
             This path is in the blocklist and cannot be used as a deletion target.",
            canonical.display()
        );
    }

    // Warn if the path is outside the home directory
    if !canonical.starts_with(&home) {
        eprintln!(
            "Warning: Path '{}' is outside your home directory. \
             Are you sure you want to add this as a deletion target?",
            canonical.display()
        );
    }

    Ok(())
}

/// Return the forbidden system root containing this path.
/// The filesystem root is handled by equality so it does not match every absolute path.
fn forbidden_root_for(path: &Path) -> Option<&'static str> {
    FORBIDDEN_TARGET_ROOTS.iter().copied().find(|root| {
        let root_path = Path::new(root);
        if root_path == Path::new("/") {
            path == root_path
        } else {
            path.starts_with(root_path)
        }
    })
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
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path().to_path_buf();
            if path.is_file() && !is_protected(&path, blocklist) {
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
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path().to_path_buf();
            if path.is_file() && !is_protected(&path, blocklist) {
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
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path().to_path_buf();
            if path.is_file()
                && !is_protected(&path, blocklist)
                && let Ok(metadata) = path.metadata()
                && let Ok(modified) = metadata.modified()
            {
                let modified_time: chrono::DateTime<Utc> = modified.into();
                if modified_time < age_threshold {
                    candidates.push(path);
                }
            }
        }

        // Also include temp/partial downloads regardless of age
        for pattern in &["*.tmp", "*.part", "*.crdownload"] {
            let full_pattern = downloads_dir.join(pattern);
            if let Ok(entries) = glob(&full_pattern.to_string_lossy()) {
                for entry in entries.flatten() {
                    if !is_protected(&entry, blocklist) {
                        candidates.push(entry);
                    }
                }
            }
        }
    }

    Ok(candidates)
}

/// Gather browser cookie files from all supported browsers
fn gather_cookie_files(blocklist: &HashSet<PathBuf>) -> anyhow::Result<Vec<PathBuf>> {
    let mut cookies = Vec::new();
    let home = paths::home_dir();

    // Chrome and Chromium cookies
    let chrome_paths = [
        home.join(".config/google-chrome/Default/Cookies"),
        home.join(".config/google-chrome/Default/Cookies-journal"),
        home.join(".config/chromium/Default/Cookies"),
        home.join(".config/chromium/Default/Cookies-journal"),
        // Brave
        home.join(".config/BraveSoftware/Brave-Browser/Default/Cookies"),
        home.join(".config/BraveSoftware/Brave-Browser/Default/Cookies-journal"),
        // Microsoft Edge
        home.join(".config/microsoft-edge/Default/Cookies"),
        home.join(".config/microsoft-edge/Default/Cookies-journal"),
        // Opera
        home.join(".config/opera/Default/Cookies"),
        home.join(".config/opera/Default/Cookies-journal"),
        // Vivaldi
        home.join(".config/vivaldi/Default/Cookies"),
        home.join(".config/vivaldi/Default/Cookies-journal"),
    ];

    for path in chrome_paths {
        if path.exists() && !is_protected(&path, blocklist) {
            cookies.push(path);
        }
    }

    // Firefox cookies (need to find profile directories)
    let firefox_dir = home.join(".mozilla/firefox");
    if firefox_dir.exists() {
        let pattern = firefox_dir.join("*.default*/cookies.sqlite*");
        if let Ok(entries) = glob(&pattern.to_string_lossy()) {
            for entry in entries.flatten() {
                if !is_protected(&entry, blocklist) {
                    cookies.push(entry);
                }
            }
        }
    }

    Ok(cookies)
}

/// Gather Tier 2 candidates (user-configured paths).
/// All paths are canonicalized before walking to prevent symlink escape.
fn gather_tier2_candidates(
    blocklist: &HashSet<PathBuf>,
    config: &Config,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();

    for path_str in &config.deletion.tier2_paths {
        let expanded = paths::expand_tilde(path_str);

        // Canonicalize to resolve symlinks and prevent escape
        let canonical = match expanded.canonicalize() {
            Ok(c) => c,
            Err(_) => continue, // Path doesn't exist, skip it
        };

        // Re-validate that the canonical path is not protected
        if is_protected(&canonical, blocklist) {
            continue;
        }

        if canonical.is_dir() {
            for entry in WalkDir::new(&canonical)
                .min_depth(1)
                .max_depth(3)
                .follow_links(false) // Never follow symlinks during walk
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path().to_path_buf();
                if path.is_file() && !is_protected(&path, blocklist) {
                    candidates.push(path);
                }
            }
        } else if canonical.is_file() && !is_protected(&canonical, blocklist) {
            candidates.push(canonical);
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

/// Select random files with a specific seed (for reproducible testing only)
#[cfg(test)]
pub fn select_random_seeded(candidates: &[PathBuf], count: usize, seed: u64) -> Vec<PathBuf> {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut shuffled = candidates.to_vec();
    shuffled.shuffle(&mut rng);
    shuffled.into_iter().take(count).collect()
}

/// Gather candidates separated by tier (for the `candidates` command)
pub fn gather_candidates_by_tier(config: &Config) -> anyhow::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let blocklist = build_blocklist(config)?;

    let tier1 = if config.deletion.tier1_enabled {
        gather_tier1_candidates(&blocklist, config)?
    } else {
        Vec::new()
    };

    let tier2 = if config.deletion.tier2_enabled {
        gather_tier2_candidates(&blocklist, config)?
    } else {
        Vec::new()
    };

    Ok((tier1, tier2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_protected_extensions() {
        let blocklist = HashSet::new();
        assert!(is_protected(
            &PathBuf::from("/home/user/secret.pem"),
            &blocklist
        ));
        assert!(is_protected(
            &PathBuf::from("/home/user/cert.key"),
            &blocklist
        ));
        assert!(!is_protected(
            &PathBuf::from("/home/user/document.txt"),
            &blocklist
        ));
    }

    #[test]
    fn test_protected_dir_components() {
        let blocklist = HashSet::new();
        // .ssh directory should be protected
        assert!(is_protected(
            &PathBuf::from("/home/user/.ssh/id_rsa"),
            &blocklist
        ));
        // .gnupg directory should be protected
        assert!(is_protected(
            &PathBuf::from("/home/user/.gnupg/secring.gpg"),
            &blocklist
        ));
        // .git directory should be protected (component check)
        assert!(is_protected(
            &PathBuf::from("/home/user/project/.git/HEAD"),
            &blocklist
        ));
        // Normal file should not be protected
        assert!(!is_protected(
            &PathBuf::from("/home/user/project/src/main.rs"),
            &blocklist
        ));
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

        assert_eq!(selected1, selected2);
    }

    #[test]
    fn test_blocklist_prevents_our_files() {
        let config = Config::default();
        let blocklist = build_blocklist(&config).unwrap();

        // Our config directory should be in the blocklist
        let our_dir = paths::home_dir().join(".config/to-do-or-die");
        let our_canonical = our_dir.canonicalize().unwrap_or(our_dir);
        assert!(blocklist.contains(&our_canonical));
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
        assert!(candidates.len() >= 2);
    }

    #[test]
    fn test_validate_target_rejects_root() {
        assert!(validate_target_path(&PathBuf::from("/")).is_err());
        assert!(validate_target_path(&PathBuf::from("/etc")).is_err());
        assert!(validate_target_path(&PathBuf::from("/boot")).is_err());
    }

    #[test]
    fn test_validate_target_rejects_paths_under_system_roots() {
        assert!(validate_target_path(&PathBuf::from("/etc/to-do-or-die-test")).is_err());
        assert!(validate_target_path(&PathBuf::from("/var/tmp/to-do-or-die-test")).is_err());
    }

    #[test]
    fn test_system_root_paths_are_always_protected() {
        let blocklist = HashSet::new();
        assert!(is_protected(&PathBuf::from("/"), &blocklist));
        assert!(is_protected(
            &PathBuf::from("/etc/to-do-or-die-test"),
            &blocklist
        ));
        assert!(!is_protected(
            &PathBuf::from("/home/user/scratch/file.txt"),
            &blocklist
        ));
    }

    #[test]
    fn test_validate_target_rejects_home() {
        let home = paths::home_dir();
        assert!(validate_target_path(&home).is_err());
    }

    #[test]
    fn test_validate_target_rejects_protected() {
        let ssh_dir = paths::home_dir().join(".ssh");
        // Create it if it doesn't exist for the test
        let _ = std::fs::create_dir_all(&ssh_dir);
        assert!(validate_target_path(&ssh_dir).is_err());
    }

    #[test]
    fn test_validate_target_accepts_safe_dir() {
        let dir = tempdir().unwrap();
        assert!(validate_target_path(dir.path()).is_ok());
    }

    #[test]
    fn test_is_protected_does_not_false_positive() {
        let blocklist = HashSet::new();
        // A path that contains ".ssh" as part of a filename but not a directory
        // should NOT be protected (substring matching was the old bug)
        assert!(!is_protected(
            &PathBuf::from("/home/user/my-ssh-backup.txt"),
            &blocklist
        ));
        // But a path with .ssh as a directory component SHOULD be protected
        assert!(is_protected(
            &PathBuf::from("/home/user/.ssh/config"),
            &blocklist
        ));
    }
}
