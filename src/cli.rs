//! CLI command definitions using clap
//!
//! Commands:
//! - add: Add a new todo with deadline
//! - list: List todos (active by default, --all for all)
//! - complete: Mark a todo as done
//! - install: Set up systemd timer
//! - uninstall: Remove systemd timer
//! - status: Show timer status
//! - check: Run deadline check manually
//! - config: Manage configuration

use clap::{Parser, Subcommand}; //clap for nice cli 

#[derive(Parser, Debug)]
#[command(name = "to-do-or-die")]
#[command(
    author,
    version,
    about = "A todo CLI that enforces deadlines through escalating chaos"
)]
#[command(long_about = "
Set a todo, complete it on time, or face the consequences lmao.
")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Add a new todo with a deadline
    #[command(alias = "a")]
    Add {
        /// Description of the todo
        description: String,

        /// Deadline duration (e.g., "2 hours", "30 minutes", "1 day")
        #[arg(short, long)]
        due: String,
    },

    /// List todos
    #[command(alias = "ls")]
    List {
        /// Show all todos including completed ones
        #[arg(short, long)]
        all: bool,
    },

    /// Mark a todo as completed
    #[command(alias = "c", alias = "done")]
    Complete {
        /// Todo ID (full UUID or partial match)
        id: String,
    },

    /// Install systemd timer for background monitoring
    Install {
        /// Don't enable lingering (timer won't run when logged out)
        #[arg(long)]
        no_linger: bool,
    },

    /// Uninstall systemd timer
    Uninstall,

    /// Show systemd timer status
    Status,

    /// Run deadline check manually (normally run by timer)
    Check {
        /// Preview what would happen without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show current configuration
    Show,

    /// Set a configuration value
    Set {
        /// Configuration key (e.g., "escalation_cap", "check_interval_minutes")
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
        // Verify that the CLI definition is valid
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
            Commands::List { all } => assert!(!all),
            _ => panic!("Expected List command"),
        }

        let cli = Cli::parse_from(["to-do-or-die", "list", "--all"]);
        match cli.command {
            Commands::List { all } => assert!(all),
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
            Commands::Check { dry_run } => assert!(!dry_run),
            _ => panic!("Expected Check command"),
        }

        let cli = Cli::parse_from(["to-do-or-die", "check", "--dry-run"]);
        match cli.command {
            Commands::Check { dry_run } => assert!(dry_run),
            _ => panic!("Expected Check command"),
        }
    }
}
