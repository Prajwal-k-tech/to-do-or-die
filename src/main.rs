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
use cli::{Cli, Commands, ConfigAction};
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
