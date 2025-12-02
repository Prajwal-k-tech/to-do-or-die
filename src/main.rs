//! to-do-or-die: A todo CLI that enforces deadlines through escalating chaos
//!
//! When you miss a deadline, consequences escalate:
//! 1. Desktop notifications
//! 2. Audio alerts  
//! 3. Text-to-speech announcements
//! 4. Wallpaper changes
//! 5. File deletion (starting with cache/cookies)

mod checker;
mod cli;
mod config;
mod deleter;
mod effects;
mod installer;
mod paths;
mod safety;
mod todos;

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use cli::{Cli, Commands, ConfigAction, TargetsAction, BlocklistAction};
use config::Config;
use todos::{TodoItem, TodoList};

fn main() -> Result<()> {
    // Ensure our directories exist
    paths::ensure_dirs().context("Failed to create application directories")?;

    let cli = Cli::parse_args();

    match cli.command {
        Commands::Add { description, due } => cmd_add(description, due),
        Commands::List { all } => cmd_list(all),
        Commands::Complete { id } => cmd_complete(id),
        Commands::Install { no_linger } => cmd_install(!no_linger),
        Commands::Uninstall => cmd_uninstall(),
        Commands::Status => cmd_status(),
        Commands::Check { dry_run } => cmd_check(dry_run),
        Commands::Config { action } => cmd_config(action),
        Commands::Log { limit, deletions, full } => cmd_log(limit, deletions, full),
        Commands::Candidates { full, limit, tier } => cmd_candidates(full, limit, tier),
        Commands::Targets { action } => cmd_targets(action),
        Commands::Blocklist { action } => cmd_blocklist(action),
    }
}

/// Add a new todo
fn cmd_add(description: String, due: String) -> Result<()> {
    // Parse the duration string
    let duration = parse_duration(&due).with_context(|| {
        format!(
            "Invalid duration: '{}'. Try '2 hours', '30 minutes', '1 day'",
            due
        )
    })?;

    let due_at = Utc::now() + duration;
    let todo = TodoItem::new(description.clone(), due_at);
    let short_id = todo.short_id();

    let mut todos = TodoList::load()?;
    todos.add(todo);
    todos.save()?;

    println!("✅ Added todo: {}", description);
    println!("   ID: {}", short_id);
    println!("   Due: {} ({})", due_at.format("%Y-%m-%d %H:%M"), due);

    Ok(())
}

/// List todos
fn cmd_list(show_all: bool) -> Result<()> {
    let todos = TodoList::load()?;

    let items: Vec<_> = if show_all {
        todos.todos.iter().collect()
    } else {
        todos.active()
    };

    if items.is_empty() {
        if show_all {
            println!("No todos found.");
        } else {
            println!("No active todos. Use 'list --all' to see completed ones.");
        }
        return Ok(());
    }

    println!(
        "\n{:<10} {:<35} {:<18} {:<8} {}",
        "ID", "Description", "Status", "Elapsed", "Stage"
    );
    println!("{}", "-".repeat(85));

    for todo in items {
        let status_icon = if todo.is_completed() {
            "✓"
        } else if todo.is_overdue() {
            "🔥"
        } else if todo.calculate_stage() >= 1 {
            "⚠️"
        } else {
            "○"
        };

        let status = todo.time_status();
        let elapsed = if todo.is_completed() {
            "-".to_string()
        } else {
            format!("{:.0}%", todo.percent_elapsed())
        };
        let stage = if todo.is_completed() {
            "-".to_string()
        } else {
            format!("{}", todo.calculate_stage())
        };

        // Truncate description if too long
        let desc = if todo.description.len() > 33 {
            format!("{}...", &todo.description[..30])
        } else {
            todo.description.clone()
        };

        println!(
            "{:<10} {} {:<35} {:<18} {:<8} {}",
            todo.short_id(),
            status_icon,
            desc,
            status,
            elapsed,
            stage
        );
    }

    // Summary
    let active_count = todos.active().len();
    let overdue_count = todos.overdue().len();
    let warning_count = todos.active().iter().filter(|t| t.calculate_stage() >= 1 && !t.is_overdue()).count();

    println!();
    if overdue_count > 0 {
        println!(
            "⚠️  {} overdue out of {} active todos",
            overdue_count, active_count
        );
    } else if active_count > 0 {
        println!("📋 {} active todos, none overdue", active_count);
    }

    Ok(())
}

/// Complete a todo
fn cmd_complete(id: String) -> Result<()> {
    let mut todos = TodoList::load()?;

    // Find the todo first to get its description
    let description = todos.find_by_id(&id)?.description.clone();

    todos.complete(&id)?;
    todos.save()?;

    println!("✅ Completed: {}", description);

    Ok(())
}

/// Install systemd timer
fn cmd_install(enable_linger: bool) -> Result<()> {
    let config = Config::load()?;
    installer::install(config.general.check_interval_minutes, enable_linger)?;
    Ok(())
}

/// Uninstall systemd timer
fn cmd_uninstall() -> Result<()> {
    installer::uninstall()?;
    Ok(())
}

/// Show timer status
fn cmd_status() -> Result<()> {
    installer::status()?;
    Ok(())
}

/// Run deadline check
fn cmd_check(dry_run: bool) -> Result<()> {
    let config = Config::load()?;
    let effective_dry_run = dry_run || config.general.dry_run;

    if effective_dry_run {
        println!("🔍 Running check in DRY RUN mode (no actual changes)...\n");
    } else {
        println!("🔍 Running deadline check...\n");
    }

    checker::run_check(effective_dry_run)?;

    Ok(())
}

/// Manage configuration
fn cmd_config(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let config = Config::load()?;
            let toml_str = toml::to_string_pretty(&config)?;
            println!("Current configuration:\n");
            println!("{}", toml_str);
        }
        ConfigAction::Set { key, value } => {
            let mut config = Config::load()?;
            set_config_value(&mut config, &key, &value)?;
            config.save()?;
            println!("✅ Set {} = {}", key, value);
        }
        ConfigAction::Reset => {
            Config::reset()?;
            println!("✅ Configuration reset to defaults");
        }
        ConfigAction::Path => {
            println!("Config file: {}", paths::config_file().display());
            println!("Todos file:  {}", paths::todos_file().display());
            println!("Audit log:   {}", paths::audit_log_file().display());
        }
    }
    Ok(())
}

/// View audit log
fn cmd_log(limit: usize, deletions_only: bool, full_paths: bool) -> Result<()> {
    let entries = deleter::read_audit_log()?;

    if entries.is_empty() {
        println!("No audit log entries yet.");
        println!("The log will be populated when deadlines are enforced.");
        return Ok(());
    }

    let filtered: Vec<_> = if deletions_only {
        entries.iter().filter(|e| e.action == "delete").collect()
    } else {
        entries.iter().collect()
    };

    let display: Vec<_> = filtered.iter().rev().take(limit).collect();

    println!("\n📋 Audit Log (showing last {} of {} entries)\n", display.len(), filtered.len());

    // Get stats
    let stats = deleter::audit_stats()?;
    println!("📊 Stats: {} deletions, {} effects, {} failures\n",
        stats.total_deletions, stats.total_effects, stats.failed_deletions);

    println!("{:<20} {:<10} {:<8} {}", "Timestamp", "Action", "Status", "Details");
    println!("{}", "-".repeat(80));

    for entry in display.iter().rev() {
        let time = entry.timestamp.format("%Y-%m-%d %H:%M:%S");
        let status = if entry.success { "✓" } else { "✗" };
        let dry = if entry.dry_run { " (dry)" } else { "" };

        let details = if let Some(ref path) = entry.path {
            if full_paths {
                path.clone()
            } else {
                // Truncate long paths
                if path.len() > 45 {
                    format!("...{}", &path[path.len()-42..])
                } else {
                    path.clone()
                }
            }
        } else if let Some(ref msg) = entry.message {
            msg.clone()
        } else {
            "-".to_string()
        };

        println!("{:<20} {:<10} {:<8} {}{}",
            time, entry.action, status, details, dry);
    }

    println!("\n💡 Use 'to-do-or-die log --full' to see complete paths");
    println!("   Use 'to-do-or-die log --deletions' to see only deletions");

    Ok(())
}

/// Show deletion candidates
fn cmd_candidates(full_paths: bool, _limit: usize, tier_filter: Option<u8>) -> Result<()> {
    let config = Config::load()?;
    
    // Gather candidates by tier
    let (tier1_candidates, tier2_candidates) = safety::gather_candidates_by_tier(&config)?;
    
    // Get counts before moving
    let t1_count = tier1_candidates.len();
    let t2_count = tier2_candidates.len();
    
    let candidates: Vec<_> = match tier_filter {
        Some(1) => tier1_candidates,
        Some(2) => tier2_candidates,
        _ => {
            let mut all = tier1_candidates;
            all.extend(tier2_candidates);
            all
        }
    };

    if candidates.is_empty() {
        println!("No deletion candidates found.");
        println!("\nThis could mean:");
        println!("  - Your cache is empty");
        println!("  - Tier 1/2 deletion is disabled in config");
        println!("  - All candidate files are blocklisted");
        println!("\n💡 Add directories with: to-do-or-die targets add ~/Downloads/temp");
        return Ok(());
    }

    let tier_label = match tier_filter {
        Some(1) => " (Tier 1 only)",
        Some(2) => " (Tier 2 only)",
        _ => "",
    };
    
    println!("\n🎯 Deletion Candidates{} - {} files\n", tier_label, candidates.len());
    
    // Show tier breakdown if showing all
    if tier_filter.is_none() {
        println!("  📦 Tier 1 (cache/cookies/trash): {} files", t1_count);
        println!("  📁 Tier 2 (user directories):    {} files", t2_count);
        println!();
    }

    // Group by parent directory for cleaner display
    let mut by_dir: std::collections::BTreeMap<String, Vec<&std::path::PathBuf>> = std::collections::BTreeMap::new();
    for path in &candidates {
        let dir = path.parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        by_dir.entry(dir).or_default().push(path);
    }
    
    // Sort by file count (most files first)
    let mut dirs: Vec<_> = by_dir.into_iter().collect();
    dirs.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    let mut shown_dirs = 0;
    let max_dirs = 20;
    
    for (dir, files) in dirs.iter() {
        if shown_dirs >= max_dirs {
            let remaining: usize = dirs.iter().skip(max_dirs).map(|(_, f)| f.len()).sum();
            println!("\n   ... and {} more directories ({} files)", dirs.len() - max_dirs, remaining);
            break;
        }

        let display_dir = if full_paths {
            dir.clone()
        } else {
            // Shorten home path
            let home = paths::home_dir();
            let home_str = home.to_string_lossy();
            if dir.starts_with(home_str.as_ref()) {
                format!("~{}", &dir[home_str.len()..])
            } else if dir.len() > 50 {
                format!("...{}", &dir[dir.len()-47..])
            } else {
                dir.clone()
            }
        };

        println!("📁 {} ({} files)", display_dir, files.len());

        // Show first few files
        for file in files.iter().take(3) {
            let name = file.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "?".to_string());
            println!("   - {}", name);
        }

        if files.len() > 3 {
            println!("   ... and {} more", files.len() - 3);
        }
        
        shown_dirs += 1;
    }

    println!("\n💡 Use 'to-do-or-die candidates --full' for complete paths");
    println!("   Deletions are random from this pool when todos go overdue.");
    println!("   Use 'to-do-or-die targets add <path>' to add directories");

    Ok(())
}

/// Manage deletion targets
fn cmd_targets(action: TargetsAction) -> Result<()> {
    match action {
        TargetsAction::List => {
            let config = Config::load()?;
            
            println!("\n🎯 Deletion Target Directories\n");
            
            // Tier 1 (built-in)
            println!("📦 Tier 1 (Built-in, always safe):");
            let tier1_dirs = [
                ("~/.cache/*", "Application caches"),
                ("~/.local/share/Trash/*", "Trash bin"),
                ("~/Downloads (old files)", &format!("Files older than {} days", config.deletion.download_age_days)),
            ];
            for (path, desc) in tier1_dirs {
                let status = if config.deletion.tier1_enabled { "✓" } else { "✗" };
                println!("  {} {} - {}", status, path, desc);
            }
            if config.deletion.cookies_enabled && config.deletion.tier1_enabled {
                println!("  ✓ Browser cookies - Chrome, Firefox, Chromium");
            }
            
            println!("\n📁 Tier 2 (User-configured):");
            if config.deletion.tier2_paths.is_empty() {
                println!("  (none configured)");
                println!("  💡 Add directories with: to-do-or-die targets add ~/Downloads/temp");
            } else {
                for path in &config.deletion.tier2_paths {
                    let expanded = paths::expand_tilde(path);
                    let exists = expanded.exists();
                    let status = if config.deletion.tier2_enabled && exists { "✓" } else if !exists { "?" } else { "✗" };
                    let file_count = if exists && expanded.is_dir() {
                        walkdir::WalkDir::new(&expanded)
                            .min_depth(1)
                            .max_depth(3)
                            .into_iter()
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().is_file())
                            .count()
                    } else { 0 };
                    println!("  {} {} ({} files)", status, path, file_count);
                }
            }
            
            println!("\n💡 Enable/disable tiers: to-do-or-die config set tier1 false");
        }
        
        TargetsAction::Add { path } => {
            let mut config = Config::load()?;
            let expanded = paths::expand_tilde(&path);
            
            // Validate path
            if !expanded.exists() {
                println!("⚠️  Warning: Path does not exist: {}", expanded.display());
                println!("   It will be added anyway and checked when it exists.");
            } else if !expanded.is_dir() {
                anyhow::bail!("Path must be a directory: {}", expanded.display());
            }
            
            // Check if already added
            let normalized = if path.starts_with("~/") {
                path.clone()
            } else {
                expanded.to_string_lossy().to_string()
            };
            
            if config.deletion.tier2_paths.contains(&normalized) {
                println!("Path already in Tier 2 targets: {}", normalized);
                return Ok(());
            }
            
            // Check against blocklist
            if safety::is_path_protected(&expanded) {
                anyhow::bail!("Cannot add protected path: {}", expanded.display());
            }
            
            config.deletion.tier2_paths.push(normalized.clone());
            config.save()?;
            
            println!("✅ Added to Tier 2 targets: {}", normalized);
            
            // Show file count
            if expanded.exists() && expanded.is_dir() {
                let file_count = walkdir::WalkDir::new(&expanded)
                    .min_depth(1)
                    .max_depth(3)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_file())
                    .count();
                println!("   📁 Contains {} files that may be deleted", file_count);
            }
        }
        
        TargetsAction::Remove { path } => {
            let mut config = Config::load()?;
            let normalized = if path.starts_with("~/") {
                path.clone()
            } else {
                let expanded = paths::expand_tilde(&path);
                expanded.to_string_lossy().to_string()
            };
            
            // Try to find and remove
            let original_len = config.deletion.tier2_paths.len();
            config.deletion.tier2_paths.retain(|p| {
                p != &normalized && paths::expand_tilde(p) != paths::expand_tilde(&path)
            });
            
            if config.deletion.tier2_paths.len() < original_len {
                config.save()?;
                println!("✅ Removed from Tier 2 targets: {}", path);
            } else {
                println!("Path not found in Tier 2 targets: {}", path);
                println!("\nCurrent targets:");
                for p in &config.deletion.tier2_paths {
                    println!("  - {}", p);
                }
            }
        }
        
        TargetsAction::Suggest => {
            println!("\n💡 Suggested directories for Tier 2:\n");
            
            let home = paths::home_dir();
            let suggestions = [
                ("~/Downloads", "Old downloads (already in Tier 1 by age)"),
                ("~/Downloads/temp", "Temporary downloads folder"),
                ("~/Documents/scratch", "Scratch/temporary documents"),
                ("~/.local/share/recently-used.xbel", "Recent files list"),
                ("~/tmp", "Personal temp folder"),
                ("~/.thumbnails", "Old thumbnail cache location"),
            ];
            
            for (path, desc) in suggestions {
                let expanded = paths::expand_tilde(path);
                let exists = if expanded.exists() { "✓" } else { "✗" };
                println!("  {} {} - {}", exists, path, desc);
            }
            
            println!("\n📂 Detected large directories in home:");
            
            // Find large dirs that might be safe
            let safe_patterns = ["temp", "tmp", "scratch", "cache", "old", "backup"];
            for entry in std::fs::read_dir(&home)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name()
                        .map(|n| n.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    
                    // Skip hidden dirs and common important ones
                    if name.starts_with('.') || ["documents", "downloads", "pictures", "videos", "music", "desktop"].contains(&name.as_str()) {
                        continue;
                    }
                    
                    if safe_patterns.iter().any(|p| name.contains(p)) {
                        let size = dir_size(&path);
                        if size > 10_000_000 { // > 10MB
                            println!("  📁 ~/{} ({:.1} MB)", entry.file_name().to_string_lossy(), size as f64 / 1_000_000.0);
                        }
                    }
                }
            }
            
            println!("\n💡 Add with: to-do-or-die targets add <path>");
        }
    }
    
    Ok(())
}

/// Manage blocklist
fn cmd_blocklist(action: BlocklistAction) -> Result<()> {
    match action {
        BlocklistAction::List => {
            let config = Config::load()?;
            
            println!("\n🛡️ Blocklist (Protected Paths)\n");
            
            println!("🔒 Built-in (cannot be removed):");
            let builtins = [
                "~/.ssh/",
                "~/.gnupg/",
                "~/.password-store/",
                "~/.config/to-do-or-die/",
                "~/.local/share/to-do-or-die/",
                "~/.bashrc, ~/.zshrc, ~/.profile",
                "**/.git/",
                "*.key, *.pem, *.crt (extensions)",
            ];
            for path in builtins {
                println!("  🔒 {}", path);
            }
            
            println!("\n📝 User-added:");
            if config.blocklist.additional_paths.is_empty() {
                println!("  (none)");
            } else {
                for path in &config.blocklist.additional_paths {
                    println!("  ✓ {}", path);
                }
            }
            
            println!("\n💡 Add protection: to-do-or-die blocklist add ~/important/");
        }
        
        BlocklistAction::Add { path } => {
            let mut config = Config::load()?;
            let normalized = if path.starts_with("~/") {
                path.clone()
            } else {
                let expanded = paths::expand_tilde(&path);
                expanded.to_string_lossy().to_string()
            };
            
            if config.blocklist.additional_paths.contains(&normalized) {
                println!("Path already in blocklist: {}", normalized);
                return Ok(());
            }
            
            config.blocklist.additional_paths.push(normalized.clone());
            config.save()?;
            
            println!("✅ Added to blocklist: {}", normalized);
            println!("   This path will never be deleted.");
        }
        
        BlocklistAction::Remove { path } => {
            let mut config = Config::load()?;
            let normalized = if path.starts_with("~/") {
                path.clone()
            } else {
                let expanded = paths::expand_tilde(&path);
                expanded.to_string_lossy().to_string()
            };
            
            let original_len = config.blocklist.additional_paths.len();
            config.blocklist.additional_paths.retain(|p| {
                p != &normalized && paths::expand_tilde(p) != paths::expand_tilde(&path)
            });
            
            if config.blocklist.additional_paths.len() < original_len {
                config.save()?;
                println!("✅ Removed from blocklist: {}", path);
                println!("   ⚠️  This path is no longer protected from deletion!");
            } else {
                println!("Path not found in user blocklist: {}", path);
            }
        }
        
        BlocklistAction::Check { path } => {
            let config = Config::load()?;
            let expanded = paths::expand_tilde(&path);
            let canonical = expanded.canonicalize().unwrap_or(expanded.clone());
            let is_protected = safety::is_path_protected(&canonical);
            
            let display_path = path.replace(
                &std::env::var("HOME").unwrap_or_default(),
                "~"
            );
            
            if is_protected {
                println!("🔒 {} is PROTECTED", display_path);
                println!("   This path will never be deleted.");
                
                // Show which rule protects it
                let path_str = canonical.to_string_lossy();
                if path_str.contains("/.ssh") {
                    println!("   Reason: SSH directory (built-in protection)");
                } else if path_str.contains("/.gnupg") {
                    println!("   Reason: GPG directory (built-in protection)");
                } else if path_str.contains("/.git") {
                    println!("   Reason: Git repository (built-in protection)");
                } else if path_str.ends_with(".key") || path_str.ends_with(".pem") || path_str.ends_with(".crt") {
                    println!("   Reason: Cryptographic file extension (built-in protection)");
                } else if config.blocklist.additional_paths.iter().any(|p| 
                    canonical.starts_with(paths::expand_tilde(p))
                ) {
                    println!("   Reason: User-added blocklist entry");
                } else {
                    println!("   Reason: Built-in protection rule");
                }
            } else {
                println!("⚠️  {} is NOT PROTECTED", display_path);
                println!("   This path could be deleted if in a target directory.");
                println!("\n   To protect it: to-do-or-die blocklist add {}", path);
            }
        }
    }
    
    Ok(())
}

/// Calculate directory size (quick estimate)
fn dir_size(path: &std::path::Path) -> u64 {
    walkdir::WalkDir::new(path)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Parse a human-readable duration string
fn parse_duration(s: &str) -> Result<Duration> {
    // Try humantime first
    if let Ok(std_duration) = humantime::parse_duration(s) {
        return Ok(Duration::from_std(std_duration)?);
    }

    // Manual parsing for common formats
    let s = s.trim().to_lowercase();

    // Try patterns like "2h", "30m", "1d"
    if let Some(hours) = s.strip_suffix('h') {
        let n: i64 = hours.trim().parse()?;
        return Ok(Duration::hours(n));
    }
    if let Some(mins) = s.strip_suffix('m') {
        let n: i64 = mins.trim().parse()?;
        return Ok(Duration::minutes(n));
    }
    if let Some(days) = s.strip_suffix('d') {
        let n: i64 = days.trim().parse()?;
        return Ok(Duration::days(n));
    }

    anyhow::bail!("Cannot parse duration: {}", s)
}

/// Set a configuration value by key
fn set_config_value(config: &mut Config, key: &str, value: &str) -> Result<()> {
    match key {
        "check_interval_minutes" | "check_interval" => {
            config.general.check_interval_minutes = value.parse()?;
        }
        "escalation_cap" => {
            config.general.escalation_cap = value.parse()?;
        }
        "dry_run" => {
            config.general.dry_run = value.parse()?;
        }
        "notifications.enabled" | "notifications" => {
            config.notifications.enabled = value.parse()?;
        }
        "notifications.sound_enabled" | "sound" => {
            config.notifications.sound_enabled = value.parse()?;
        }
        "notifications.tts_enabled" | "tts" => {
            config.notifications.tts_enabled = value.parse()?;
        }
        "notifications.wallpaper_enabled" | "wallpaper" => {
            config.notifications.wallpaper_enabled = value.parse()?;
        }
        "deletion.enabled" | "deletion" => {
            config.deletion.enabled = value.parse()?;
        }
        "deletion.tier1_enabled" | "tier1" => {
            config.deletion.tier1_enabled = value.parse()?;
        }
        "deletion.tier2_enabled" | "tier2" => {
            config.deletion.tier2_enabled = value.parse()?;
        }
        "deletion.cookies_enabled" | "cookies" => {
            config.deletion.cookies_enabled = value.parse()?;
        }
        "deletion.download_age_days" | "download_age" => {
            config.deletion.download_age_days = value.parse()?;
        }
        "audio.alert_sound" | "alert_sound" => {
            config.audio.alert_sound = value.to_string();
        }
        _ => {
            anyhow::bail!(
                "Unknown config key: {}. Use 'config show' to see available options.",
                key
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("2 hours").unwrap(), Duration::hours(2));
        assert_eq!(parse_duration("30 minutes").unwrap(), Duration::minutes(30));
        assert_eq!(parse_duration("1 day").unwrap(), Duration::days(1));
        assert_eq!(parse_duration("2h").unwrap(), Duration::hours(2));
        assert_eq!(parse_duration("30m").unwrap(), Duration::minutes(30));
        assert_eq!(parse_duration("1d").unwrap(), Duration::days(1));
    }
}
