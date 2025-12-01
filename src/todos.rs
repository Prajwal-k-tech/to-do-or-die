//! Todo item management and persistence
//!
//! Todos are stored in JSON format at ~/.local/share/to-do-or-die/todos.json
//!
//! Escalation is based on PERCENTAGE of time elapsed from creation to deadline:
//! - Stage 0: < 50% elapsed (no effects)
//! - Stage 1: 50% elapsed → notification
//! - Stage 2: 75% elapsed → notification + audio
//! - Stage 3: 90% elapsed → notification + TTS
//! - Stage 4: 95% elapsed → wallpaper change
//! - Stage 5+: 100%+ (overdue) → deletions begin and escalate

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use thiserror::Error;
use uuid::Uuid;

use crate::paths;

#[derive(Error, Debug)]
pub enum TodoError {
    #[error("Failed to read todos file: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Failed to parse todos file: {0}")]
    ParseError(#[from] serde_json::Error),
    #[error("Todo not found: {0}")]
    NotFound(String),
    #[error("Ambiguous todo ID: {0} matches multiple todos")]
    AmbiguousId(String),
}

/// A single todo item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Unique identifier
    pub id: Uuid,
    /// Description of the task
    pub description: String,
    /// When the todo was created
    pub created_at: DateTime<Utc>,
    /// When the todo is due
    pub due_at: DateTime<Utc>,
    /// When the todo was completed (None if still active)
    pub completed_at: Option<DateTime<Utc>>,
    /// Current escalation stage (0 = no effects yet)
    #[serde(default)]
    pub stage: u32,
    /// Total number of files deleted for this todo
    #[serde(default)]
    pub deletions_count: u32,
}

/// Container for all todos
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TodoList {
    pub todos: Vec<TodoItem>,
}

impl TodoItem {
    /// Create a new todo with the given description and deadline
    pub fn new(description: String, due_at: DateTime<Utc>) -> Self {
        TodoItem {
            id: Uuid::new_v4(),
            description,
            created_at: Utc::now(),
            due_at,
            completed_at: None,
            stage: 0,
            deletions_count: 0,
        }
    }

    /// Check if this todo is overdue (past deadline)
    pub fn is_overdue(&self) -> bool {
        self.completed_at.is_none() && Utc::now() > self.due_at
    }

    /// Check if this todo is completed
    pub fn is_completed(&self) -> bool {
        self.completed_at.is_some()
    }

    /// Check if this todo is active (not completed)
    pub fn is_active(&self) -> bool {
        self.completed_at.is_none()
    }

    /// Calculate percentage of time elapsed from creation to deadline
    /// Returns value from 0.0 to 100.0+ (can exceed 100 if overdue)
    pub fn percent_elapsed(&self) -> f64 {
        if self.completed_at.is_some() {
            return 0.0;
        }
        
        let now = Utc::now();
        let total_duration = (self.due_at - self.created_at).num_seconds() as f64;
        
        if total_duration <= 0.0 {
            // Edge case: deadline is at or before creation time
            return if now >= self.due_at { 100.0 } else { 0.0 };
        }
        
        let elapsed = (now - self.created_at).num_seconds() as f64;
        (elapsed / total_duration) * 100.0
    }

    /// Get minutes overdue (0 if not overdue or completed)
    pub fn minutes_overdue(&self) -> i64 {
        if self.completed_at.is_some() {
            return 0;
        }
        let now = Utc::now();
        if now <= self.due_at {
            return 0;
        }
        (now - self.due_at).num_minutes()
    }

    /// Calculate the escalation stage based on percentage elapsed
    /// 
    /// BEFORE deadline (percentage-based warnings):
    /// - Stage 0: < 50% elapsed (no effects)
    /// - Stage 1: 50% elapsed → notification
    /// - Stage 2: 75% elapsed → notification + audio  
    /// - Stage 3: 90% elapsed → notification + TTS
    /// - Stage 4: 95% elapsed → wallpaper change
    /// 
    /// AFTER deadline (deletion escalation):
    /// - Stage 5: 100-110% (just overdue) → delete 1 file
    /// - Stage 6: 110-120% → delete 2 files
    /// - Stage 7+: continues escalating every 10%
    pub fn calculate_stage(&self) -> u32 {
        if self.completed_at.is_some() {
            return 0;
        }
        
        let pct = self.percent_elapsed();
        
        if pct < 50.0 {
            0
        } else if pct < 75.0 {
            1  // 50% - notification
        } else if pct < 90.0 {
            2  // 75% - notification + audio
        } else if pct < 95.0 {
            3  // 90% - notification + TTS
        } else if pct < 100.0 {
            4  // 95% - wallpaper change
        } else {
            // Overdue! Deletion stages begin
            // Stage 5 at 100%, then +1 stage for every 10% more overdue
            let overdue_pct = pct - 100.0;
            5 + (overdue_pct / 10.0) as u32
        }
    }

    /// Check if this todo needs attention (at or above stage 1)
    pub fn needs_attention(&self) -> bool {
        self.is_active() && self.calculate_stage() >= 1
    }

    /// Get short ID (first 8 chars of UUID)
    pub fn short_id(&self) -> String {
        self.id.to_string()[..8].to_string()
    }

    /// Mark this todo as completed
    pub fn complete(&mut self) {
        self.completed_at = Some(Utc::now());
    }

    /// Get human-readable time until/since deadline
    pub fn time_status(&self) -> String {
        let now = Utc::now();
        if let Some(completed) = self.completed_at {
            let ago = now - completed;
            format!("completed {} ago", humanize_duration(ago))
        } else if now > self.due_at {
            let overdue = now - self.due_at;
            format!("overdue by {}", humanize_duration(overdue))
        } else {
            let remaining = self.due_at - now;
            format!("{} remaining", humanize_duration(remaining))
        }
    }
    
    /// Get percentage status string
    pub fn percent_status(&self) -> String {
        let pct = self.percent_elapsed();
        if pct >= 100.0 {
            format!("{:.0}% (OVERDUE)", pct)
        } else {
            format!("{:.0}%", pct)
        }
    }
}

/// Convert a chrono Duration to a human-readable string
fn humanize_duration(duration: chrono::Duration) -> String {
    let total_secs = duration.num_seconds().abs();
    if total_secs < 60 {
        format!("{}s", total_secs)
    } else if total_secs < 3600 {
        format!("{}m", total_secs / 60)
    } else if total_secs < 86400 {
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    } else {
        let days = total_secs / 86400;
        let hours = (total_secs % 86400) / 3600;
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
        }
    }
}

impl TodoList {
    /// Load todos from file, creating empty list if file doesn't exist
    pub fn load() -> Result<Self, TodoError> {
        let path = paths::todos_file();

        if !path.exists() {
            return Ok(TodoList::default());
        }

        let content = fs::read_to_string(&path)?;
        if content.trim().is_empty() {
            return Ok(TodoList::default());
        }

        let list: TodoList = serde_json::from_str(&content)?;
        Ok(list)
    }

    /// Save todos to file
    pub fn save(&self) -> Result<(), TodoError> {
        let path = paths::todos_file();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// Add a new todo
    pub fn add(&mut self, item: TodoItem) {
        self.todos.push(item);
    }

    /// Find a todo by ID (full or partial match)
    pub fn find_by_id(&self, id: &str) -> Result<&TodoItem, TodoError> {
        let matches: Vec<_> = self
            .todos
            .iter()
            .filter(|t| t.id.to_string().starts_with(id))
            .collect();

        match matches.len() {
            0 => Err(TodoError::NotFound(id.to_string())),
            1 => Ok(matches[0]),
            _ => Err(TodoError::AmbiguousId(id.to_string())),
        }
    }

    /// Find a todo by ID (mutable)
    pub fn find_by_id_mut(&mut self, id: &str) -> Result<&mut TodoItem, TodoError> {
        let matches: Vec<_> = self
            .todos
            .iter()
            .enumerate()
            .filter(|(_, t)| t.id.to_string().starts_with(id))
            .map(|(i, _)| i)
            .collect();

        match matches.len() {
            0 => Err(TodoError::NotFound(id.to_string())),
            1 => Ok(&mut self.todos[matches[0]]),
            _ => Err(TodoError::AmbiguousId(id.to_string())),
        }
    }

    /// Get all active (not completed) todos
    pub fn active(&self) -> Vec<&TodoItem> {
        self.todos.iter().filter(|t| t.is_active()).collect()
    }

    /// Get all overdue todos
    pub fn overdue(&self) -> Vec<&TodoItem> {
        self.todos.iter().filter(|t| t.is_overdue()).collect()
    }

    /// Get all completed todos
    pub fn completed(&self) -> Vec<&TodoItem> {
        self.todos.iter().filter(|t| t.is_completed()).collect()
    }

    /// Complete a todo by ID
    pub fn complete(&mut self, id: &str) -> Result<(), TodoError> {
        let todo = self.find_by_id_mut(id)?;
        todo.complete();
        Ok(())
    }

    /// Update the stage and deletions count for a todo
    pub fn update_stage(&mut self, id: &str, stage: u32, deletions: u32) -> Result<(), TodoError> {
        let todo = self.find_by_id_mut(id)?;
        todo.stage = stage;
        todo.deletions_count += deletions;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_new_todo() {
        let due = Utc::now() + Duration::hours(1);
        let todo = TodoItem::new("Test task".to_string(), due);

        assert!(!todo.is_overdue());
        assert!(!todo.is_completed());
        assert!(todo.is_active());
        assert_eq!(todo.calculate_stage(), 0); // Just created, 0% elapsed
    }

    #[test]
    fn test_overdue_todo() {
        let due = Utc::now() - Duration::minutes(15);
        let todo = TodoItem::new("Late task".to_string(), due);

        assert!(todo.is_overdue());
        assert_eq!(todo.calculate_stage(), 5); // Overdue = stage 5
    }

    #[test]
    fn test_complete_todo() {
        let due = Utc::now() + Duration::hours(1);
        let mut todo = TodoItem::new("Test task".to_string(), due);

        todo.complete();
        assert!(todo.is_completed());
        assert!(!todo.is_active());
        assert!(!todo.is_overdue()); // Completed todos aren't overdue
    }

    #[test]
    fn test_stage_calculation() {
        // New percentage-based system:
        // Stage 0: < 50% elapsed
        // Stage 1: 50-75% elapsed (notification)
        // Stage 2: 75-90% elapsed (audio)
        // Stage 3: 90-95% elapsed (TTS)
        // Stage 4: 95-100% elapsed (wallpaper)
        // Stage 5+: Overdue (deletions)

        let base = Utc::now();

        // Test: 25% elapsed (2 hour deadline, 30 min passed) -> Stage 0
        let todo_25pct = TodoItem {
            id: Uuid::new_v4(),
            description: "Test".to_string(),
            created_at: base - Duration::minutes(30),
            due_at: base + Duration::minutes(90), // 2 hours total, 30 min elapsed
            completed_at: None,
            stage: 0,
            deletions_count: 0,
        };
        assert_eq!(todo_25pct.calculate_stage(), 0);

        // Test: 60% elapsed -> Stage 1
        let todo_60pct = TodoItem {
            created_at: base - Duration::minutes(60),
            due_at: base + Duration::minutes(40), // 100 min total, 60 elapsed
            ..todo_25pct.clone()
        };
        assert_eq!(todo_60pct.calculate_stage(), 1);

        // Test: 80% elapsed -> Stage 2
        let todo_80pct = TodoItem {
            created_at: base - Duration::minutes(80),
            due_at: base + Duration::minutes(20), // 100 min total, 80 elapsed
            ..todo_25pct.clone()
        };
        assert_eq!(todo_80pct.calculate_stage(), 2);

        // Test: Overdue -> Stage 5+ (depends on how much overdue)
        // 30 min overdue on 2 hour deadline = 150/120 = 125%
        // Stage = 5 + (25/10) = 5 + 2 = 7
        let todo_overdue = TodoItem {
            created_at: base - Duration::hours(2),
            due_at: base - Duration::minutes(30), // 30 min overdue
            ..todo_25pct.clone()
        };
        assert!(todo_overdue.calculate_stage() >= 5); // Just check it's overdue stage
    }

    #[test]
    fn test_todo_list_operations() {
        let mut list = TodoList::default();

        let due = Utc::now() + Duration::hours(1);
        let todo = TodoItem::new("Task 1".to_string(), due);
        let id = todo.short_id();

        list.add(todo);
        assert_eq!(list.todos.len(), 1);
        assert_eq!(list.active().len(), 1);

        list.complete(&id).unwrap();
        assert_eq!(list.active().len(), 0);
        assert_eq!(list.completed().len(), 1);
    }

    #[test]
    fn test_humanize_duration() {
        assert_eq!(humanize_duration(Duration::seconds(30)), "30s");
        assert_eq!(humanize_duration(Duration::minutes(5)), "5m");
        assert_eq!(humanize_duration(Duration::hours(2)), "2h");
        assert_eq!(
            humanize_duration(Duration::hours(2) + Duration::minutes(30)),
            "2h 30m"
        );
        assert_eq!(humanize_duration(Duration::days(1)), "1d");
    }
}
