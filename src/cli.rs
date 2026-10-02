//! CLI command definitions using clap
//!
//! Commands:
//! - add: Add a new todo with deadline
//! - list: List todos (active by default, --all for all)
//! - complete: Mark a todo as done (by index, UUID, or description)
//! - install: Set up systemd timer
//! - uninstall: Remove systemd timer
//! - status: Show timer status
//! - check: Run deadline check manually
//! - config: Manage configuration
//! - log: View audit log
//! - candidates: Preview deletion candidates
//! - targets: Manage deletion target directories
//! - blocklist: Manage protected paths
//! - doctor: Check system dependencies

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "to-do-or-die")]
#[command(
    author,
    version,
    about = "A todo CLI that enforces deadlines through escalating consequences"
)]
#[command(long_about = "\
A todo list with actual consequences. Set a deadline, complete it on time,
or watch the chaos escalate: notifications, audio alerts, TTS warnings,
desktop appearance changes where supported, and file deletion.

By default, the tool runs in dry-run mode with deletion disabled.
Use 'to-do-or-die config set dry_run false' and
'to-do-or-die config set deletion true' to enable real enforcement.

Quick start:
  to-do-or-die add \"Finish report\" --due \"2 hours\"
  to-do-or-die list
  to-do-or-die install
")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Add a new todo with a deadline
    #[command(visible_alias = "a")]
    #[command(
        after_help = "EXAMPLES:\n  to-do-or-die add \"Buy groceries\" --due \"2 hours\"\n  to-do-or-die add \"Call mom\" --due \"30m\"\n  to-do-or-die add \"File taxes\" --due \"3 days\"\n  to-do-or-die add \"Write report\" --due \"1 day 6 hours\""
    )]
    Add {
        /// Description of the todo
        description: String,

        /// Deadline duration (e.g., "2 hours", "30 minutes", "1 day", "2h", "30m", "1d")
        #[arg(short, long)]
        due: String,
    },

    /// List todos (sorted by urgency)
    #[command(visible_alias = "ls")]
    List {
        /// Show all todos including completed ones
        #[arg(short, long)]
        all: bool,

        /// Output as JSON (for scripting and status bars)
        #[arg(long)]
        json: bool,
    },

    /// Mark a todo as completed (accepts index, UUID prefix, or description match)
    #[command(visible_alias = "c", visible_alias = "done")]
    #[command(
        after_help = "EXAMPLES:\n  to-do-or-die complete 1           (by list index)\n  to-do-or-die complete a1b2c3d4    (by UUID prefix)\n  to-do-or-die complete \"buy milk\"  (by description match)"
    )]
    Complete {
        /// Todo identifier: list index (1-based), UUID prefix, or description text
        id: String,
    },

    /// Install systemd timer for background monitoring
    #[command(
        after_help = "This creates systemd user units at ~/.config/systemd/user/\nand enables a timer to run 'check' at the configured interval."
    )]
    Install {
        /// Enable lingering so the timer runs even when you are logged out
        #[arg(long)]
        linger: bool,
    },

    /// Uninstall systemd timer
    Uninstall,

    /// Show systemd timer status
    Status,

    /// Run deadline check manually (normally run by the timer)
    #[command(
        after_help = "By default, this runs in dry-run mode if configured.\nUse --no-dry-run to force live execution (may delete files).\nUse --yes to skip the confirmation prompt for live execution."
    )]
    Check {
        /// Preview what would happen without making changes
        #[arg(long)]
        dry_run: bool,

        /// Force live execution (overrides dry_run config setting)
        #[arg(long)]
        no_dry_run: bool,

        /// Skip confirmation prompt for live execution
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// View audit log of all actions
    #[command(visible_alias = "audit")]
    Log {
        /// Show only the last N entries
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,

        /// Show only deletions
        #[arg(short, long)]
        deletions: bool,

        /// Show full paths (don't truncate)
        #[arg(short, long)]
        full: bool,
    },

    /// Show deletion candidates (what could be deleted)
    #[command(
        after_help = "Shows the pool of files that could be deleted when\ntodos go overdue. Use --tier to filter by tier."
    )]
    Candidates {
        /// Show full paths
        #[arg(short, long)]
        full: bool,

        /// Limit number of candidates shown
        #[arg(short = 'n', long, default_value = "50")]
        limit: usize,

        /// Show by tier (1, 2, or all)
        #[arg(short, long)]
        tier: Option<u8>,
    },

    /// Manage deletion targets (directories to delete from)
    #[command(visible_alias = "target")]
    Targets {
        #[command(subcommand)]
        action: TargetsAction,
    },

    /// Manage blocklist (paths that are never deleted)
    #[command(visible_alias = "block")]
    Blocklist {
        #[command(subcommand)]
        action: BlocklistAction,
    },

    /// Check system dependencies and configuration
    #[command(
        after_help = "Checks available system tools:\nsystemd, notify-rust (D-Bus), espeak-ng (TTS), GNOME/KDE appearance commands,\nand audio playback support."
    )]
    Doctor,
}

#[derive(Subcommand, Debug)]
pub enum TargetsAction {
    /// List all target directories (Tier 1 + Tier 2)
    #[command(visible_alias = "ls")]
    List,

    /// Add a directory to Tier 2 targets
    Add {
        /// Path to add (e.g., ~/Downloads/temp, ~/Documents/scratch)
        path: String,
    },

    /// Remove a directory from Tier 2 targets
    #[command(visible_alias = "rm")]
    Remove {
        /// Path to remove
        path: String,
    },

    /// Show suggested directories to add
    Suggest,
}

#[derive(Subcommand, Debug)]
pub enum BlocklistAction {
    /// List all blocklisted paths
    #[command(visible_alias = "ls")]
    List,

    /// Add a path to the blocklist
    Add {
        /// Path to protect from deletion
        path: String,
    },

    /// Remove a path from the blocklist
    #[command(visible_alias = "rm")]
    Remove {
        /// Path to unprotect
        path: String,
    },

    /// Check if a path would be protected from deletion
    Check {
        /// Path to check
        path: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show current configuration with explanations
    Show,

    /// Set a configuration value
    Set {
        /// Configuration key (e.g., "escalation_cap", "check_interval_minutes", "deletion", "dry_run")
        key: String,
        /// Value to set
        value: String,
    },

    /// Reset configuration to defaults
    Reset,

    /// Show configuration file path
    Path,
}

impl Cli {
    pub fn parse_args() -> Self {
        Cli::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_parse_add() {
        let cli = Cli::parse_from(["to-do-or-die", "add", "Test task", "--due", "2 hours"]);
        match cli.command {
            Commands::Add { description, due } => {
                assert_eq!(description, "Test task");
                assert_eq!(due, "2 hours");
            }
            _ => panic!("Expected Add command"),
        }
    }

    #[test]
    fn test_parse_list() {
        let cli = Cli::parse_from(["to-do-or-die", "list"]);
        match cli.command {
            Commands::List { all, json } => {
                assert!(!all);
                assert!(!json);
            }
            _ => panic!("Expected List command"),
        }

        let cli = Cli::parse_from(["to-do-or-die", "list", "--all"]);
        match cli.command {
            Commands::List { all, json } => {
                assert!(all);
                assert!(!json);
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_parse_complete() {
        let cli = Cli::parse_from(["to-do-or-die", "complete", "abc123"]);
        match cli.command {
            Commands::Complete { id } => assert_eq!(id, "abc123"),
            _ => panic!("Expected Complete command"),
        }
    }

    #[test]
    fn test_parse_check() {
        let cli = Cli::parse_from(["to-do-or-die", "check"]);
        match cli.command {
            Commands::Check {
                dry_run,
                no_dry_run,
                yes,
            } => {
                assert!(!dry_run);
                assert!(!no_dry_run);
                assert!(!yes);
            }
            _ => panic!("Expected Check command"),
        }

        let cli = Cli::parse_from(["to-do-or-die", "check", "--dry-run"]);
        match cli.command {
            Commands::Check { dry_run, .. } => assert!(dry_run),
            _ => panic!("Expected Check command"),
        }
    }

    #[test]
    fn test_parse_doctor() {
        let cli = Cli::parse_from(["to-do-or-die", "doctor"]);
        match cli.command {
            Commands::Doctor => {}
            _ => panic!("Expected Doctor command"),
        }
    }
}
