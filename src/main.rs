//! to-do-or-die: A todo CLI that enforces deadlines through escalating consequences
//!
//! When you miss a deadline, consequences escalate:
//! 1. Desktop notifications
//! 2. Audio alerts
//! 3. Text-to-speech announcements
//! 4. Desktop appearance setting
//! 5. File deletion (starting with cache/cookies, moved to trash by default)

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
use cli::{BlocklistAction, Cli, Commands, ConfigAction, TargetsAction};
use config::Config;
use owo_colors::OwoColorize;
use todos::{TodoItem, TodoList};

fn main() -> Result<()> {
    paths::ensure_dirs().context("Failed to create application directories")?;

    let cli = Cli::parse_args();

    // Determine if color should be used
    let use_color = should_use_color(cli.no_color);

    match cli.command {
        Commands::Add { description, due } => cmd_add(description, due, use_color),
        Commands::List { all, json } => cmd_list(all, json, use_color),
        Commands::Complete { id } => cmd_complete(id, use_color),
        Commands::Install { linger } => cmd_install(linger, use_color),
        Commands::Uninstall => cmd_uninstall(),
        Commands::Status => cmd_status(),
        Commands::Check {
            dry_run,
            no_dry_run,
            yes,
        } => cmd_check(dry_run, no_dry_run, yes, use_color),
        Commands::Config { action } => cmd_config(action, use_color),
        Commands::Log {
            limit,
            deletions,
            full,
        } => cmd_log(limit, deletions, full),
        Commands::Candidates { full, limit, tier } => cmd_candidates(full, limit, tier),
        Commands::Targets { action } => cmd_targets(action, use_color),
        Commands::Blocklist { action } => cmd_blocklist(action, use_color),
        Commands::Doctor => cmd_doctor(use_color),
    }
}

/// Determine if colored output should be used.
/// Disabled if: --no-color flag, NO_COLOR env var, or not a TTY.
fn should_use_color(no_color_flag: bool) -> bool {
    if no_color_flag {
        return false;
    }
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    // Check config setting
    if let Ok(config) = Config::load()
        && !config.general.color
    {
        return false;
    }
    true
}

/// Add a new todo
fn cmd_add(description: String, due: String, use_color: bool) -> Result<()> {
    if description.trim().is_empty() {
        anyhow::bail!("Description cannot be empty");
    }

    let duration = parse_duration(&due).with_context(|| {
        format!(
            "Invalid duration: '{}'. Supported formats:\n  \
             Word-based: 2 hours, 30 minutes, 1 day, 1 hour 30 minutes\n  \
             Shorthand:  2h, 30m, 1d, 1h30m\n  \
             Example: to-do-or-die add \"Task\" --due \"2 hours\"",
            due
        )
    })?;

    let due_at = Utc::now() + duration;
    let todo = TodoItem::new(description.clone(), due_at);
    let short_id = todo.short_id();

    let mut todos = TodoList::load()?;
    todos.add(todo);
    todos.save()?;

    if use_color {
        println!(
            "{} Added todo: {}",
            "[ok]".green().bold(),
            description.bold()
        );
    } else {
        println!("[ok] Added todo: {}", description);
    }
    println!("   ID:  {}", short_id);
    println!("   Due: {} ({})", due_at.format("%Y-%m-%d %H:%M"), due);

    // Show onboarding hint for first todo
    if todos.todos.len() == 1 {
        println!("\n   Next steps:");
        println!("     to-do-or-die list          (view your todos)");
        println!("     to-do-or-die install        (enable background monitoring)");
        println!("     to-do-or-die config show    (review settings)");
    }

    Ok(())
}

/// List todos (sorted by urgency)
fn cmd_list(show_all: bool, json: bool, use_color: bool) -> Result<()> {
    let todos = TodoList::load()?;

    // First-run onboarding: no todos at all
    if todos.todos.is_empty() {
        println!("\nWelcome to to-do-or-die!\n");
        println!("  A todo list with actual consequences. Set a deadline,");
        println!("  complete it on time, or face escalating chaos.\n");
        println!("  Get started:");
        println!("    to-do-or-die add \"Buy groceries\" --due \"2 hours\"");
        println!("    to-do-or-die add \"Finish report\" --due \"1 day\"");
        println!("\n  Then:");
        println!("    to-do-or-die list           (view todos)");
        println!("    to-do-or-die install         (enable background monitoring)");
        println!("    to-do-or-die config show     (review settings)");
        println!("\n  Full docs: to-do-or-die --help");
        return Ok(());
    }

    let mut items: Vec<&TodoItem> = if show_all {
        todos.todos.iter().collect()
    } else {
        todos.active()
    };

    // Sort by urgency: overdue (highest stage) first, then by percent elapsed
    items.sort_by(|a, b| {
        let a_stage = a.calculate_stage();
        let b_stage = b.calculate_stage();
        b_stage.cmp(&a_stage).then_with(|| {
            b.percent_elapsed()
                .partial_cmp(&a.percent_elapsed())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    // JSON output for scripting
    if json {
        let json_items: Vec<serde_json::Value> = items
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.short_id(),
                    "description": t.description,
                    "stage": t.calculate_stage(),
                    "stage_label": t.stage_label(),
                    "percent_elapsed": t.percent_elapsed(),
                    "due_at": t.due_at,
                    "completed": t.is_completed(),
                    "time_status": t.time_status(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_items)?);
        return Ok(());
    }

    // Table output
    println!(
        "\n {:<4}  {:<12}  {:<40}  {:<20}  {:<8}  {:<10}",
        "#", "ID", "Description", "Status", "Elapsed", "Stage"
    );
    println!(" {}", "-".repeat(100));

    for (i, todo) in items.iter().enumerate() {
        let index = i + 1;
        let desc = truncate_str(&todo.description, 38);

        let status = todo.time_status();
        let elapsed = if todo.is_completed() {
            "-".to_string()
        } else {
            format!("{:.0}%", todo.percent_elapsed())
        };
        let stage = if todo.is_completed() {
            "done".to_string()
        } else {
            format!("{} ({})", todo.calculate_stage(), todo.stage_label())
        };

        // Color based on urgency
        if use_color {
            let status_colored = if todo.is_completed() {
                status.green().to_string()
            } else if todo.is_overdue() {
                status.red().bold().to_string()
            } else if todo.calculate_stage() >= 1 {
                status.yellow().to_string()
            } else {
                status.clone()
            };

            let stage_colored = if todo.is_completed() {
                stage.green().to_string()
            } else if todo.is_overdue() {
                stage.red().bold().to_string()
            } else if todo.calculate_stage() >= 1 {
                stage.yellow().to_string()
            } else {
                stage.clone()
            };

            let id_str = todo.short_id().dimmed().to_string();
            let desc_str = if todo.is_overdue() {
                desc.bold().to_string()
            } else {
                desc.clone()
            };

            println!(
                " {:<4}  {:<12}  {:<40}  {:<20}  {:<8}  {}",
                index, id_str, desc_str, status_colored, elapsed, stage_colored
            );
        } else {
            println!(
                " {:<4}  {:<12}  {:<40}  {:<20}  {:<8}  {}",
                index,
                todo.short_id(),
                desc,
                status,
                elapsed,
                stage
            );
        }
    }

    // Summary
    let active_count = todos.active().len();
    let overdue_count = todos.overdue().len();
    let warning_count = todos
        .active()
        .iter()
        .filter(|t| t.calculate_stage() >= 1 && !t.is_overdue())
        .count();

    println!();
    if overdue_count > 0 {
        if use_color {
            println!(
                "  {} {} overdue, {} warning, {} active",
                "[!]".red().bold(),
                overdue_count,
                warning_count,
                active_count
            );
        } else {
            println!(
                "  [!] {} overdue, {} warning, {} active",
                overdue_count, warning_count, active_count
            );
        }
    } else if active_count > 0 {
        if use_color {
            println!(
                "  {} {} active todos, none overdue",
                "[ok]".green(),
                active_count
            );
        } else {
            println!("  [ok] {} active todos, none overdue", active_count);
        }
    }

    Ok(())
}

/// Complete a todo (by index, UUID prefix, or description match)
fn cmd_complete(id: String, use_color: bool) -> Result<()> {
    let mut todos = TodoList::load()?;

    // Use smart find: tries index, then UUID prefix, then description match
    let description = todos.find_smart(&id)?.description.clone();
    todos.find_smart_mut(&id)?.complete();
    todos.save()?;

    if use_color {
        println!(
            "{} Completed: {}",
            "[ok]".green().bold(),
            description.bold()
        );
    } else {
        println!("[ok] Completed: {}", description);
    }

    Ok(())
}

/// Install systemd timer
fn cmd_install(linger: bool, _use_color: bool) -> Result<()> {
    let config = Config::load()?;
    installer::install(config.general.check_interval_minutes, linger)?;
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
fn cmd_check(dry_run: bool, no_dry_run: bool, yes: bool, use_color: bool) -> Result<()> {
    let config = Config::load()?;

    // Determine effective dry-run mode
    let effective_dry_run = if no_dry_run {
        false
    } else {
        dry_run || config.general.dry_run
    };

    if effective_dry_run {
        println!("Running check in DRY RUN mode (no actual changes)...\n");
    } else {
        // Check if deletions would happen and prompt for confirmation
        if !yes && checker::would_delete_files()? {
            if use_color {
                println!(
                    "{} Files will be {} in this check.",
                    "[!]".red().bold(),
                    if config.deletion.permanent_delete {
                        "permanently deleted"
                    } else {
                        "moved to trash"
                    }
                    .red()
                );
            } else {
                println!(
                    "[!] Files will be {} in this check.",
                    if config.deletion.permanent_delete {
                        "permanently deleted"
                    } else {
                        "moved to trash"
                    }
                );
            }
            println!("    Pass --yes to skip this prompt, or --dry-run to preview.\n");

            print!("Continue with live check? [y/N] ");
            use std::io::{self, Write};
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Check cancelled. No changes made.");
                return Ok(());
            }
            println!();
        }

        if use_color {
            println!("{}\n", "Running deadline check (LIVE)...".bold());
        } else {
            println!("Running deadline check (LIVE)...\n");
        }
    }

    checker::run_check(effective_dry_run)?;

    Ok(())
}

/// Manage configuration
fn cmd_config(action: ConfigAction, use_color: bool) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let config = Config::load()?;
            println!("Current configuration:\n");
            println!("  [general]");
            println!(
                "    check_interval_minutes = {}   # How often the timer checks (minutes)",
                config.general.check_interval_minutes
            );
            println!(
                "    escalation_cap         = {}   # Max files deleted per check",
                config.general.escalation_cap
            );
            println!(
                "    dry_run                = {}   # If true, no actual changes are made",
                config.general.dry_run
            );
            println!(
                "    color                  = {}   # Enable colored terminal output",
                config.general.color
            );
            println!();
            println!("  [notifications]");
            println!(
                "    enabled         = {}   # Desktop notifications",
                config.notifications.enabled
            );
            println!(
                "    sound_enabled   = {}   # Audio alerts",
                config.notifications.sound_enabled
            );
            println!(
                "    tts_enabled     = {}   # Text-to-speech warnings",
                config.notifications.tts_enabled
            );
            println!(
                "    appearance_enabled = {} # Try desktop color/theme setting",
                config.notifications.appearance_enabled
            );
            println!();
            println!("  [deletion]");
            println!(
                "    enabled          = {}  # Master switch for file deletion",
                config.deletion.enabled
            );
            println!(
                "    tier1_enabled    = {}  # Cache, cookies, trash",
                config.deletion.tier1_enabled
            );
            println!(
                "    tier2_enabled    = {}  # User-configured directories",
                config.deletion.tier2_enabled
            );
            println!(
                "    cookies_enabled  = {}  # Browser cookie deletion",
                config.deletion.cookies_enabled
            );
            println!(
                "    permanent_delete = {}  # If false, files go to trash (recoverable)",
                config.deletion.permanent_delete
            );
            println!(
                "    download_age_days = {} # Age threshold for old downloads",
                config.deletion.download_age_days
            );
            if !config.deletion.tier2_paths.is_empty() {
                println!("    tier2_paths      = {:?}", config.deletion.tier2_paths);
            } else {
                println!("    tier2_paths      = []   # Add with: to-do-or-die targets add <path>");
            }
            println!();
            println!("  [audio]");
            println!(
                "    alert_sound = \"{}\"   # \"default\" or path to .wav/.mp3",
                config.audio.alert_sound
            );
            println!();
            println!("  [blocklist]");
            if config.blocklist.additional_paths.is_empty() {
                println!(
                    "    additional_paths = []   # Add with: to-do-or-die blocklist add <path>"
                );
            } else {
                println!(
                    "    additional_paths = {:?}",
                    config.blocklist.additional_paths
                );
            }
            println!();
            println!("  To change a setting: to-do-or-die config set <key> <value>");
            println!("  Example: to-do-or-die config set dry_run false");
        }
        ConfigAction::Set { key, value } => {
            let mut config = Config::load()?;
            set_config_value(&mut config, &key, &value)?;
            config.save()?;
            if use_color {
                println!("{} Set {} = {}", "[ok]".green().bold(), key.bold(), value);
            } else {
                println!("[ok] Set {} = {}", key, value);
            }
        }
        ConfigAction::Reset => {
            Config::reset()?;
            if use_color {
                println!("{} Configuration reset to defaults", "[ok]".green().bold());
            } else {
                println!("[ok] Configuration reset to defaults");
            }
            println!("    Note: dry_run=true and deletion.enabled=false by default (safe mode).");
        }
        ConfigAction::Path => {
            println!("Config file: {}", paths::config_file().display());
            println!("Todos file:  {}", paths::todos_file().display());
            println!("Audit log:   {}", paths::audit_log_file().display());
            println!("Lock file:   {}", paths::lock_file().display());
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

    println!(
        "\n Audit Log (showing last {} of {} entries)\n",
        display.len(),
        filtered.len()
    );

    let stats = deleter::audit_stats()?;
    println!(
        " Stats: {} deletions, {} effects, {} failures\n",
        stats.total_deletions, stats.total_effects, stats.failed_deletions
    );

    println!(
        "{:<20}  {:<10}  {:<8}  Details",
        "Timestamp", "Action", "Status"
    );
    println!("{}", "-".repeat(80));

    for entry in display.iter().rev() {
        let time = entry.timestamp.format("%Y-%m-%d %H:%M:%S");
        let status = if entry.success { "ok" } else { "FAIL" };
        let dry = if entry.dry_run { " (dry)" } else { "" };
        let perm = if entry.permanent && entry.action == "delete" {
            " (perm)"
        } else {
            ""
        };

        let details = if let Some(ref path) = entry.path {
            if full_paths {
                path.clone()
            } else {
                truncate_path(path, 50)
            }
        } else if let Some(ref msg) = entry.message {
            msg.clone()
        } else {
            "-".to_string()
        };

        println!(
            "{:<20}  {:<10}  {:<8}  {}{}{}",
            time, entry.action, status, details, dry, perm
        );
    }

    println!("\n  Use 'to-do-or-die log --full' to see complete paths");
    println!("  Use 'to-do-or-die log --deletions' to see only deletions");

    Ok(())
}

/// Show deletion candidates
fn cmd_candidates(full_paths: bool, limit: usize, tier_filter: Option<u8>) -> Result<()> {
    let config = Config::load()?;

    let (tier1_candidates, tier2_candidates) = safety::gather_candidates_by_tier(&config)?;

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
        println!("\n  Add directories with: to-do-or-die targets add ~/Downloads/temp");
        return Ok(());
    }

    let tier_label = match tier_filter {
        Some(1) => " (Tier 1 only)",
        Some(2) => " (Tier 2 only)",
        _ => "",
    };

    // Apply the limit
    let display_count = candidates.len().min(limit);

    println!(
        "\n Deletion Candidates{} - {} files (showing {})\n",
        tier_label,
        candidates.len(),
        display_count
    );

    if tier_filter.is_none() {
        println!("  Tier 1 (cache/cookies/trash): {} files", t1_count);
        println!("  Tier 2 (user directories):    {} files", t2_count);
        println!();
    }

    // Group by parent directory for cleaner display
    let mut by_dir: std::collections::BTreeMap<String, Vec<&std::path::PathBuf>> =
        std::collections::BTreeMap::new();
    for path in candidates.iter().take(limit) {
        let dir = path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        by_dir.entry(dir).or_default().push(path);
    }

    // Sort by file count (most files first)
    let mut dirs: Vec<_> = by_dir.into_iter().collect();
    dirs.sort_by_key(|a| std::cmp::Reverse(a.1.len()));

    let max_dirs = 20;

    for (shown_dirs, (dir, files)) in dirs.iter().enumerate() {
        if shown_dirs >= max_dirs {
            let remaining: usize = dirs.iter().skip(max_dirs).map(|(_, f)| f.len()).sum();
            println!(
                "\n   ... and {} more directories ({} files)",
                dirs.len() - max_dirs,
                remaining
            );
            break;
        }

        let display_dir = if full_paths {
            dir.clone()
        } else {
            let home = paths::home_dir();
            let home_str = home.to_string_lossy();
            if dir.starts_with(home_str.as_ref()) {
                format!("~{}", &dir[home_str.len()..])
            } else if dir.len() > 50 {
                truncate_path(dir, 50)
            } else {
                dir.clone()
            }
        };

        println!("  {} ({} files)", display_dir, files.len());

        for file in files.iter().take(3) {
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "?".to_string());
            println!("     - {}", name);
        }

        if files.len() > 3 {
            println!("     ... and {} more", files.len() - 3);
        }
    }

    // Show deletion rate explanation
    println!("\n  At stage 5 (just overdue): 1 file deleted per check");
    println!("  At stage 6: 2 files per check");
    println!(
        "  At stage 10+: up to {} files per check (capped by escalation_cap)",
        config.general.escalation_cap
    );
    println!("  Use 'to-do-or-die check --dry-run' to see exactly what would be deleted");

    Ok(())
}

/// Manage deletion targets
fn cmd_targets(action: TargetsAction, use_color: bool) -> Result<()> {
    match action {
        TargetsAction::List => {
            let config = Config::load()?;

            println!("\n Deletion Target Directories\n");

            println!("  Tier 1 (Built-in candidates; inspect before enabling):");
            println!(
                "    Cache, browser cookies, trash, and old downloads can still contain useful data."
            );
            let tier1_dirs = [
                ("~/.cache/*", "Application caches"),
                ("~/.local/share/Trash/*", "Trash bin"),
                (
                    "~/Downloads (old files)",
                    &format!(
                        "Files older than {} days",
                        config.deletion.download_age_days
                    ),
                ),
            ];
            for (path, desc) in tier1_dirs {
                let status = if config.deletion.tier1_enabled {
                    "[+]"
                } else {
                    "[-]"
                };
                println!("    {} {} - {}", status, path, desc);
            }
            if config.deletion.cookies_enabled && config.deletion.tier1_enabled {
                println!(
                    "    [+] Browser cookies - Chrome, Firefox, Chromium, Brave, Edge, Opera, Vivaldi"
                );
            }

            println!("\n  Tier 2 (User-configured):");
            if config.deletion.tier2_paths.is_empty() {
                println!("    (none configured)");
                println!("    Add directories with: to-do-or-die targets add ~/Downloads/temp");
            } else {
                for path in &config.deletion.tier2_paths {
                    let expanded = paths::expand_tilde(path);
                    let exists = expanded.exists();
                    let status = if config.deletion.tier2_enabled && exists {
                        "[+]"
                    } else if !exists {
                        "[?]"
                    } else {
                        "[-]"
                    };
                    let file_count = if exists && expanded.is_dir() {
                        walkdir::WalkDir::new(&expanded)
                            .min_depth(1)
                            .max_depth(3)
                            .into_iter()
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().is_file())
                            .count()
                    } else {
                        0
                    };
                    println!("    {} {} ({} files)", status, path, file_count);
                }
            }

            println!("\n  Enable/disable tiers: to-do-or-die config set tier1 false");
        }

        TargetsAction::Add { path } => {
            let mut config = Config::load()?;
            let expanded = paths::expand_tilde(&path);

            // Validate path safety (rejects root, home, system dirs, protected paths)
            safety::validate_target_path(&expanded)?;
            if expanded.exists() {
                if !expanded.is_dir() {
                    anyhow::bail!("Path must be a directory: {}", expanded.display());
                }
            } else {
                println!("[!] Warning: Path does not exist: {}", expanded.display());
                println!("    It will be added anyway and checked when it exists.");
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

            config.deletion.tier2_paths.push(normalized.clone());
            config.save()?;

            if use_color {
                println!(
                    "{} Added to Tier 2 targets: {}",
                    "[ok]".green().bold(),
                    normalized
                );
            } else {
                println!("[ok] Added to Tier 2 targets: {}", normalized);
            }

            if expanded.exists() && expanded.is_dir() {
                let file_count = walkdir::WalkDir::new(&expanded)
                    .min_depth(1)
                    .max_depth(3)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_file())
                    .count();
                println!("    Contains {} files that may be deleted", file_count);
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

            let original_len = config.deletion.tier2_paths.len();
            config.deletion.tier2_paths.retain(|p| {
                p != &normalized && paths::expand_tilde(p) != paths::expand_tilde(&path)
            });

            if config.deletion.tier2_paths.len() < original_len {
                config.save()?;
                println!("[ok] Removed from Tier 2 targets: {}", path);
            } else {
                println!("Path not found in Tier 2 targets: {}", path);
                println!("\nCurrent targets:");
                for p in &config.deletion.tier2_paths {
                    println!("  - {}", p);
                }
            }
        }

        TargetsAction::Suggest => {
            println!("\n Suggested directories for Tier 2:\n");

            let home = paths::home_dir();
            let suggestions = [
                ("~/Downloads", "Old downloads (already in Tier 1 by age)"),
                ("~/Downloads/temp", "Temporary downloads folder"),
                ("~/Documents/scratch", "Scratch/temporary documents"),
                ("~/tmp", "Personal temp folder"),
                ("~/.thumbnails", "Old thumbnail cache location"),
            ];

            for (path, desc) in suggestions {
                let expanded = paths::expand_tilde(path);
                let exists = if expanded.exists() { "[+]" } else { "[-]" };
                println!("  {} {} - {}", exists, path, desc);
            }

            println!("\n  Detected large directories in home:");

            let safe_patterns = ["temp", "tmp", "scratch", "cache", "old", "backup"];
            for entry in std::fs::read_dir(&home)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_lowercase())
                        .unwrap_or_default();

                    if name.starts_with('.')
                        || [
                            "documents",
                            "downloads",
                            "pictures",
                            "videos",
                            "music",
                            "desktop",
                        ]
                        .contains(&name.as_str())
                    {
                        continue;
                    }

                    if safe_patterns.iter().any(|p| name.contains(p)) {
                        let size = dir_size(&path);
                        if size > 10_000_000 {
                            println!(
                                "    ~/{} ({:.1} MB)",
                                entry.file_name().to_string_lossy(),
                                size as f64 / 1_000_000.0
                            );
                        }
                    }
                }
            }

            println!("\n  Add with: to-do-or-die targets add <path>");
        }
    }

    Ok(())
}

/// Manage blocklist
fn cmd_blocklist(action: BlocklistAction, use_color: bool) -> Result<()> {
    match action {
        BlocklistAction::List => {
            let config = Config::load()?;

            println!("\n Blocklist (Protected Paths)\n");

            println!("  Built-in (cannot be removed):");
            let builtins = [
                "~/.ssh/",
                "~/.gnupg/",
                "~/.password-store/",
                "~/.config/to-do-or-die/",
                "~/.local/share/to-do-or-die/",
                "~/.local/state/to-do-or-die/",
                "~/.config/systemd/",
                "~/.bashrc, ~/.zshrc, ~/.profile, ~/.bash_profile",
                "~/.config/fish/config.fish, ~/.config/nushell/config.nu",
                "**/.git/ (path component check)",
                "*.key, *.pem, *.crt, *.cer, *.p12, *.pfx, *.gpg, *.asc",
            ];
            for path in builtins {
                println!("    [+] {}", path);
            }

            println!("\n  User-added:");
            if config.blocklist.additional_paths.is_empty() {
                println!("    (none)");
            } else {
                for path in &config.blocklist.additional_paths {
                    println!("    [+] {}", path);
                }
            }

            println!("\n  Add protection: to-do-or-die blocklist add ~/important/");
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

            if use_color {
                println!(
                    "{} Added to blocklist: {}",
                    "[ok]".green().bold(),
                    normalized
                );
            } else {
                println!("[ok] Added to blocklist: {}", normalized);
            }
            println!("    This path will never be deleted.");
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
                println!("[ok] Removed from blocklist: {}", path);
                println!("    [!] This path is no longer protected from deletion!");
            } else {
                println!("Path not found in user blocklist: {}", path);
            }
        }

        BlocklistAction::Check { path } => {
            let config = Config::load()?;
            let expanded = paths::expand_tilde(&path);
            let canonical = expanded.canonicalize().unwrap_or(expanded.clone());
            let is_protected = safety::is_path_protected(&canonical);

            let display_path = path.replace(&std::env::var("HOME").unwrap_or_default(), "~");

            if is_protected {
                if use_color {
                    println!("{} {} is PROTECTED", "[+]".green().bold(), display_path);
                } else {
                    println!("[+] {} is PROTECTED", display_path);
                }
                println!("    This path will never be deleted.");

                let path_str = canonical.to_string_lossy();
                if path_str.contains("/.ssh") {
                    println!("    Reason: SSH directory (built-in protection)");
                } else if path_str.contains("/.gnupg") {
                    println!("    Reason: GPG directory (built-in protection)");
                } else if path_str.contains("/.git") {
                    println!("    Reason: Git repository (built-in protection)");
                } else if path_str.ends_with(".key")
                    || path_str.ends_with(".pem")
                    || path_str.ends_with(".crt")
                {
                    println!("    Reason: Cryptographic file extension (built-in protection)");
                } else if config
                    .blocklist
                    .additional_paths
                    .iter()
                    .any(|p| canonical.starts_with(paths::expand_tilde(p)))
                {
                    println!("    Reason: User-added blocklist entry");
                } else {
                    println!("    Reason: Built-in protection rule");
                }
            } else {
                if use_color {
                    println!(
                        "{} {} is NOT PROTECTED",
                        "[!]".yellow().bold(),
                        display_path
                    );
                } else {
                    println!("[!] {} is NOT PROTECTED", display_path);
                }
                println!("    This path could be deleted if in a target directory.");
                println!("\n    To protect it: to-do-or-die blocklist add {}", path);
            }
        }
    }

    Ok(())
}

/// Check system dependencies and configuration
fn cmd_doctor(use_color: bool) -> Result<()> {
    println!("\n System Check\n");
    println!("{}\n", "=".repeat(50));

    let mut all_ok = true;
    let mut warnings = Vec::new();

    // systemd
    if effects::command_exists("systemctl") {
        if use_color {
            println!(
                "  {} systemd          - user timer support available",
                "[ok]".green()
            );
        } else {
            println!("  [ok] systemd          - user timer support available");
        }
    } else {
        all_ok = false;
        if use_color {
            println!(
                "  {} systemd          - NOT FOUND (timer installation will fail)",
                "[FAIL]".red()
            );
        } else {
            println!("  [FAIL] systemd          - NOT FOUND (timer installation will fail)");
        }
        println!("       Install: systemd (usually pre-installed on Linux)");
    }

    // loginctl (for lingering)
    if effects::command_exists("loginctl") {
        if use_color {
            println!(
                "  {} loginctl        - lingering support available",
                "[ok]".green()
            );
        } else {
            println!("  [ok] loginctl        - lingering support available");
        }
    } else {
        warnings.push("loginctl not found (cannot enable lingering for background timer)");
        if use_color {
            println!(
                "  {} loginctl        - NOT FOUND (lingering unavailable)",
                "[warn]".yellow()
            );
        } else {
            println!("  [warn] loginctl        - NOT FOUND (lingering unavailable)");
        }
    }

    // D-Bus / notifications
    if effects::command_exists("dbus-send") || effects::command_exists("busctl") {
        if use_color {
            println!(
                "  {} D-Bus           - desktop notification support available",
                "[ok]".green()
            );
        } else {
            println!("  [ok] D-Bus           - desktop notification support available");
        }
    } else {
        warnings.push("D-Bus not found (desktop notifications may not work)");
        if use_color {
            println!(
                "  {} D-Bus           - NOT FOUND (notifications may fail)",
                "[warn]".yellow()
            );
        } else {
            println!("  [warn] D-Bus           - NOT FOUND (notifications may fail)");
        }
    }

    // TTS: espeak-ng
    if effects::command_exists("espeak-ng") {
        if use_color {
            println!(
                "  {} espeak-ng       - TTS warnings available",
                "[ok]".green()
            );
        } else {
            println!("  [ok] espeak-ng       - TTS warnings available");
        }
    } else if effects::command_exists("espeak") {
        if use_color {
            println!(
                "  {} espeak          - TTS warnings available (fallback)",
                "[ok]".green()
            );
        } else {
            println!("  [ok] espeak          - TTS warnings available (fallback)");
        }
    } else if effects::command_exists("spd-say") {
        if use_color {
            println!(
                "  {} spd-say         - TTS warnings available (fallback)",
                "[ok]".green()
            );
        } else {
            println!("  [ok] spd-say         - TTS warnings available (fallback)");
        }
    } else {
        warnings.push("No TTS engine found (speech warnings will be unavailable)");
        if use_color {
            println!(
                "  {} TTS engine      - NOT FOUND (speech warnings unavailable)",
                "[warn]".yellow()
            );
        } else {
            println!("  [warn] TTS engine      - NOT FOUND (speech warnings unavailable)");
        }
        println!("       Install: sudo apt install espeak-ng");
    }

    // Audio: paplay or aplay
    if effects::command_exists("paplay") {
        if use_color {
            println!(
                "  {} paplay          - audio playback available (PulseAudio)",
                "[ok]".green()
            );
        } else {
            println!("  [ok] paplay          - audio playback available (PulseAudio)");
        }
    } else if effects::command_exists("aplay") {
        if use_color {
            println!(
                "  {} aplay           - audio playback available (ALSA)",
                "[ok]".green()
            );
        } else {
            println!("  [ok] aplay           - audio playback available (ALSA)");
        }
    } else {
        warnings.push("No audio playback command found (audio alerts will be unavailable)");
        if use_color {
            println!(
                "  {} audio           - NOT FOUND (audio alerts unavailable)",
                "[warn]".yellow()
            );
        } else {
            println!("  [warn] audio           - NOT FOUND (audio alerts unavailable)");
        }
        println!("       Install: pulseaudio-utils or alsa-utils");
    }

    // Native audio (rodio feature)
    #[cfg(feature = "audio")]
    {
        if use_color {
            println!(
                "  {} rodio (native)  - compiled with audio feature",
                "[ok]".green()
            );
        } else {
            println!("  [ok] rodio (native)  - compiled with audio feature");
        }
    }
    #[cfg(not(feature = "audio"))]
    {
        println!("  [info] rodio (native) - not compiled (using system commands for audio)");
        println!("         Rebuild with: cargo build --features audio");
    }

    // Desktop appearance setting: GNOME background color or KDE color scheme.
    if effects::command_exists("gsettings") {
        if use_color {
            println!(
                "  {} gsettings       - command found; GNOME setting will be tried",
                "[ok]".green()
            );
        } else {
            println!("  [ok] gsettings       - command found; GNOME setting will be tried");
        }
    } else if effects::command_exists("plasma-apply-colorscheme") {
        if use_color {
            println!(
                "  {} plasma-apply    - command found; KDE color scheme will be tried",
                "[ok]".green()
            );
        } else {
            println!("  [ok] plasma-apply    - command found; KDE color scheme will be tried");
        }
    } else {
        warnings.push("No desktop appearance command found (stage 4 will use notifications only)");
        if use_color {
            println!(
                "  {} appearance      - not available (stage 4 will use notifications only)",
                "[warn]".yellow()
            );
        } else {
            println!(
                "  [warn] appearance      - not available (stage 4 will use notifications only)"
            );
        }
    }

    // trash (for safe deletion)
    if use_color {
        println!(
            "  {} trash           - safe deletion (move to trash) built-in",
            "[ok]".green()
        );
    } else {
        println!("  [ok] trash           - safe deletion (move to trash) built-in");
    }

    // Config check
    println!("\n{}\n", "=".repeat(50));
    println!(" Configuration:\n");

    let config = Config::load()?;
    if config.general.dry_run {
        if use_color {
            println!(
                "  {} dry_run = true    (safe mode: no actual changes)",
                "[ok]".green()
            );
        } else {
            println!("  [ok] dry_run = true    (safe mode: no actual changes)");
        }
    } else {
        if use_color {
            println!(
                "  {} dry_run = false   (LIVE mode: changes are real)",
                "[!]".yellow().bold()
            );
        } else {
            println!("  [!] dry_run = false   (LIVE mode: changes are real)");
        }
    }

    if config.deletion.enabled {
        if use_color {
            println!(
                "  {} deletion = true   (file deletion is ENABLED)",
                "[!]".yellow().bold()
            );
        } else {
            println!("  [!] deletion = true   (file deletion is ENABLED)");
        }
        if config.deletion.permanent_delete {
            if use_color {
                println!(
                    "  {} permanent = true  (files are PERMANENTLY deleted, not trashed)",
                    "[!]".red().bold()
                );
            } else {
                println!("  [!] permanent = true  (files are PERMANENTLY deleted, not trashed)");
            }
        } else {
            println!("  permanent = false  (files go to trash, recoverable)");
        }
    } else {
        if use_color {
            println!(
                "  {} deletion = false  (file deletion is disabled)",
                "[ok]".green()
            );
        } else {
            println!("  [ok] deletion = false  (file deletion is disabled)");
        }
    }

    // Timer status
    println!("\n{}\n", "=".repeat(50));
    println!(" Timer Status:\n");
    let timer_output = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "to-do-or-die.timer"])
        .output();
    match timer_output {
        Ok(o) if o.status.success() => {
            if use_color {
                println!("  {} Timer is active and running", "[ok]".green());
            } else {
                println!("  [ok] Timer is active and running");
            }
        }
        _ => {
            println!("  [!] Timer is not running");
            println!("       Run 'to-do-or-die install' to set up background monitoring");
        }
    }

    // Summary
    println!("\n{}\n", "=".repeat(50));
    if all_ok && warnings.is_empty() {
        if use_color {
            println!("  {} Your system is fully ready.", "[ok]".green().bold());
        } else {
            println!("  [ok] Your system is fully ready.");
        }
        println!("       Run 'to-do-or-die install' to set up background monitoring.");
    } else if all_ok {
        if use_color {
            println!(
                "  {} Your system is ready with some optional features unavailable.",
                "[ok]".green()
            );
        } else {
            println!("  [ok] Your system is ready with some optional features unavailable.");
        }
        for w in &warnings {
            println!("       - {}", w);
        }
    } else {
        if use_color {
            println!(
                "  {} Some required components are missing.",
                "[FAIL]".red().bold()
            );
        } else {
            println!("  [FAIL] Some required components are missing.");
        }
        println!("       Install the missing components above before using this tool.");
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

/// Truncate a string to a max length, respecting char boundaries.
/// Adds "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_len - 3).collect();
    format!("{}...", truncated)
}

/// Truncate a path string from the middle to preserve context.
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    // Show the last part of the path (most informative)
    let chars: Vec<char> = path.chars().collect();
    let start = chars.len().saturating_sub(max_len - 3);
    let result: String = chars[start..].iter().collect();
    format!("...{}", result)
}

/// Parse a human-readable duration string
fn parse_duration(s: &str) -> Result<Duration> {
    // Try humantime first (handles "2 hours", "30 minutes", "1 day", "1 hour 30 minutes")
    if let Ok(std_duration) = humantime::parse_duration(s) {
        return Ok(Duration::from_std(std_duration)?);
    }

    // Manual parsing for shorthand formats
    let s = s.trim().to_lowercase();

    // Try patterns like "2h", "30m", "1d", "1h30m"
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
        "color" => {
            config.general.color = value.parse()?;
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
        "notifications.appearance_enabled"
        | "notifications.wallpaper_enabled"
        | "appearance"
        | "wallpaper" => {
            config.notifications.appearance_enabled = value.parse()?;
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
        "deletion.permanent_delete" | "permanent_delete" => {
            config.deletion.permanent_delete = value.parse()?;
        }
        "deletion.download_age_days" | "download_age" => {
            config.deletion.download_age_days = value.parse()?;
        }
        "audio.alert_sound" | "alert_sound" => {
            config.audio.alert_sound = value.to_string();
        }
        _ => {
            anyhow::bail!(
                "Unknown config key: '{}'. Use 'to-do-or-die config show' to see available options.",
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

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world this is long", 10), "hello w...");
        // Should not panic on multi-byte chars
        assert_eq!(truncate_str("helloworld", 8), "hello...");
    }

    #[test]
    fn test_truncate_path() {
        assert_eq!(truncate_path("/short", 50), "/short");
        let long = "/home/user/.cache/thumbnails/large/some/very/deep/path/file.png";
        let truncated = truncate_path(long, 30);
        assert!(truncated.starts_with("..."));
        assert!(truncated.len() <= 30);
    }
}
