//! Deadline checking logic
//!
//! Called by the systemd timer every N minutes to check todos and trigger effects.
//!
//! Effects are triggered based on PERCENTAGE of time elapsed:
//! - Stage 1 (50%): Notification
//! - Stage 2 (75%): Notification + Audio
//! - Stage 3 (90%): Notification + TTS
//! - Stage 4 (95%): Wallpaper change
//! - Stage 5+ (100%+): File deletions begin

use crate::config::Config;
use crate::deleter;
use crate::effects;
use crate::safety;
use crate::todos::{TodoItem, TodoList};

/// Result of checking a single todo
#[derive(Debug)]
pub struct CheckResult {
    pub todo_id: String,
    pub description: String,
    pub percent_elapsed: f64,
    pub stage: u32,
    pub previous_stage: u32,
    pub effects_triggered: Vec<String>,
    pub files_deleted: u32,
}

/// Check all active todos and trigger appropriate effects
pub fn run_check(dry_run: bool) -> anyhow::Result<Vec<CheckResult>> {
    let config = Config::load()?;
    let mut todos = TodoList::load()?;
    let mut results = Vec::new();

    // Get ALL active todos that need attention (stage >= 1, meaning 50%+ elapsed)
    let attention_ids: Vec<_> = todos
        .active()
        .iter()
        .filter(|t| t.calculate_stage() >= 1)
        .map(|t| t.id.to_string())
        .collect();

    if attention_ids.is_empty() {
        if dry_run {
            println!("No todos need attention yet. All clear! 🎉");
        }
        return Ok(results);
    }

    for todo_id in attention_ids {
        let todo = todos.find_by_id(&todo_id)?;
        let previous_stage = todo.stage;
        let current_stage = todo.calculate_stage();
        let percent = todo.percent_elapsed();

        let mut effects_triggered = Vec::new();
        let mut files_deleted = 0u32;

        // Only trigger effects if stage has increased
        if current_stage > previous_stage {
            // Trigger effects for this stage
            let triggered = trigger_effects_for_stage(todo, current_stage, percent, &config, dry_run)?;
            effects_triggered = triggered;

            // Handle deletion if we're at stage 5+ (100%+ elapsed = overdue)
            if current_stage >= 5 && config.deletion.enabled {
                let deletion_count =
                    calculate_deletion_count(current_stage, config.general.escalation_cap);

                if deletion_count > 0 {
                    files_deleted = execute_deletions(todo, deletion_count, &config, dry_run)?;
                }
            }
        }

        results.push(CheckResult {
            todo_id: todo.short_id(),
            description: todo.description.clone(),
            percent_elapsed: percent,
            stage: current_stage,
            previous_stage,
            effects_triggered,
            files_deleted,
        });

        // Update the todo's stage and deletion count
        if !dry_run {
            let todo_mut = todos.find_by_id_mut(&todo_id)?;
            todo_mut.stage = current_stage;
            todo_mut.deletions_count += files_deleted;
        }
    }

    // Save updated todos
    if !dry_run {
        todos.save()?;
    }

    // Print summary
    print_check_summary(&results, dry_run);

    Ok(results)
}

/// Calculate how many files to delete at this stage
/// Stage 5 = 1 file, Stage 6 = 2 files, etc. (capped at escalation_cap)
fn calculate_deletion_count(stage: u32, cap: u32) -> u32 {
    if stage < 5 {
        0
    } else {
        std::cmp::min(stage - 4, cap)
    }
}

/// Get the notification title based on stage
fn get_notification_title(stage: u32, percent: f64) -> String {
    if stage >= 5 {
        "🔥 TODO OVERDUE - DELETIONS ACTIVE!".to_string()
    } else if stage == 4 {
        "⚠️ CRITICAL: 95% time elapsed!".to_string()
    } else if stage == 3 {
        "⏰ WARNING: 90% time elapsed!".to_string()
    } else if stage == 2 {
        "📢 ALERT: 75% time elapsed!".to_string()
    } else {
        format!("📋 Reminder: {:.0}% time elapsed", percent)
    }
}

/// Trigger effects for the given stage
fn trigger_effects_for_stage(
    todo: &TodoItem,
    stage: u32,
    percent: f64,
    config: &Config,
    dry_run: bool,
) -> anyhow::Result<Vec<String>> {
    let mut triggered = Vec::new();

    // Stage 1+ (50%+): Notification
    if stage >= 1 && config.notifications.enabled {
        let title = get_notification_title(stage, percent);
        let body = format!(
            "{}\n{}\n{:.0}% of time elapsed",
            todo.description,
            todo.time_status(),
            percent
        );
        
        if dry_run {
            triggered.push("notification".to_string());
        } else {
            effects::send_notification(&title, &body)?;
            triggered.push("notification".to_string());
        }
    }

    // Stage 2+ (75%+): Audio
    if stage >= 2 && config.notifications.sound_enabled {
        if dry_run {
            triggered.push("audio".to_string());
        } else {
            effects::play_alert_sound(&config.audio.alert_sound)?;
            triggered.push("audio".to_string());
        }
    }

    // Stage 3+ (90%+): TTS
    if stage >= 3 && config.notifications.tts_enabled {
        let urgency = if stage >= 5 { "overdue" } else { "running out of time" };
        let message = format!(
            "Warning! Your todo is {}. {}. Complete it now!",
            urgency, todo.description
        );
        if dry_run {
            triggered.push("tts".to_string());
        } else {
            effects::speak_text(&message)?;
            triggered.push("tts".to_string());
        }
    }

    // Stage 4+ (95%+): Wallpaper
    if stage >= 4 && config.notifications.wallpaper_enabled {
        if dry_run {
            triggered.push("wallpaper".to_string());
        } else {
            effects::set_warning_wallpaper(&todo.description)?;
            triggered.push("wallpaper".to_string());
        }
    }

    Ok(triggered)
}

/// Execute file deletions for this todo
fn execute_deletions(
    todo: &TodoItem,
    count: u32,
    config: &Config,
    dry_run: bool,
) -> anyhow::Result<u32> {
    // Gather deletion candidates
    let candidates = safety::gather_candidates(config)?;

    if candidates.is_empty() {
        if dry_run {
            println!("  No deletion candidates found");
        }
        return Ok(0);
    }

    // Select random files to delete
    let selected = safety::select_random(&candidates, count as usize);

    if dry_run {
        println!("  Would delete {} files:", selected.len());
        for path in &selected {
            println!("    - {}", path.display());
        }
        return Ok(selected.len() as u32);
    }

    // Actually delete the files
    let deleted = deleter::delete_files(&selected, &todo.id.to_string())?;

    Ok(deleted as u32)
}

/// Print a summary of the check results
fn print_check_summary(results: &[CheckResult], dry_run: bool) {
    if results.is_empty() {
        return;
    }

    let prefix = if dry_run { "[DRY RUN] " } else { "" };

    println!("\n{}=== Check Summary ===", prefix);

    for result in results {
        let status = if result.stage >= 5 { "🔥 OVERDUE" } else { "⏳ Warning" };
        println!(
            "\n{} {} ({}) - {:.0}% elapsed",
            status, result.description, result.todo_id, result.percent_elapsed
        );
        println!("   Stage: {} → {}", result.previous_stage, result.stage);

        if !result.effects_triggered.is_empty() {
            println!("   Effects: {}", result.effects_triggered.join(", "));
        }

        if result.files_deleted > 0 {
            println!("   🗑️ Files deleted: {}", result.files_deleted);
        }
    }

    let total_deleted: u32 = results.iter().map(|r| r.files_deleted).sum();
    if total_deleted > 0 {
        println!("\n🔥 Total files deleted this check: {}", total_deleted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deletion_count_calculation() {
        assert_eq!(calculate_deletion_count(0, 10), 0);
        assert_eq!(calculate_deletion_count(4, 10), 0);
        assert_eq!(calculate_deletion_count(5, 10), 1);
        assert_eq!(calculate_deletion_count(6, 10), 2);
        assert_eq!(calculate_deletion_count(14, 10), 10); // Capped
        assert_eq!(calculate_deletion_count(20, 10), 10); // Capped
    }
}
