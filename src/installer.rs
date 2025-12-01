//! systemd service and timer installation
//!
//! Installs user-level systemd units for background todo monitoring

use std::fs;
use std::process::Command;
use thiserror::Error;

use crate::paths;

#[derive(Error, Debug)]
pub enum InstallerError {
    #[error("Failed to write systemd unit: {0}")]
    WriteError(#[from] std::io::Error),
    #[error("systemctl command failed: {0}")]
    SystemctlError(String),
    #[error("Failed to enable lingering: {0}")]
    LingerError(String),
}

/// systemd timer unit content
fn timer_unit(interval_minutes: u32) -> String {
    format!(
        r#"[Unit]
Description=Check todos for deadline enforcement

[Timer]
OnBootSec=1min
OnUnitActiveSec={interval_minutes}min
Persistent=true

[Install]
WantedBy=timers.target
"#
    )
}

/// systemd service unit content
fn service_unit() -> String {
    // Get the path to our binary
    let binary_path = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "to-do-or-die".to_string());

    format!(
        r#"[Unit]
Description=Todo deadline checker

[Service]
Type=oneshot
ExecStart={binary_path} check
Environment=DISPLAY=:0
Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/%U/bus
"#
    )
}

/// Install systemd user timer and service
pub fn install(interval_minutes: u32, enable_linger: bool) -> Result<(), InstallerError> {
    let systemd_dir = paths::systemd_user_dir();
    fs::create_dir_all(&systemd_dir)?;

    // Write timer unit
    let timer_path = systemd_dir.join("to-do-or-die.timer");
    fs::write(&timer_path, timer_unit(interval_minutes))?;
    println!("✓ Created {}", timer_path.display());

    // Write service unit
    let service_path = systemd_dir.join("to-do-or-die.service");
    fs::write(&service_path, service_unit())?;
    println!("✓ Created {}", service_path.display());

    // Reload systemd daemon
    run_systemctl(&["--user", "daemon-reload"])?;
    println!("✓ Reloaded systemd daemon");

    // Enable and start timer
    run_systemctl(&["--user", "enable", "--now", "to-do-or-die.timer"])?;
    println!("✓ Enabled and started timer");

    // Enable lingering so timer runs when logged out
    if enable_linger {
        enable_lingering()?;
        println!("✓ Enabled lingering (timer will run when logged out)");
    }

    println!(
        "\n🎯 Installation complete! The timer will check todos every {} minutes.",
        interval_minutes
    );
    println!("   Run 'to-do-or-die status' to verify it's running.");

    Ok(())
}

/// Uninstall systemd user timer and service
pub fn uninstall() -> Result<(), InstallerError> {
    // Stop and disable timer
    let _ = run_systemctl(&["--user", "stop", "to-do-or-die.timer"]);
    let _ = run_systemctl(&["--user", "disable", "to-do-or-die.timer"]);
    println!("✓ Stopped and disabled timer");

    // Remove unit files
    let systemd_dir = paths::systemd_user_dir();
    let timer_path = systemd_dir.join("to-do-or-die.timer");
    let service_path = systemd_dir.join("to-do-or-die.service");

    if timer_path.exists() {
        fs::remove_file(&timer_path)?;
        println!("✓ Removed {}", timer_path.display());
    }

    if service_path.exists() {
        fs::remove_file(&service_path)?;
        println!("✓ Removed {}", service_path.display());
    }

    // Reload daemon
    run_systemctl(&["--user", "daemon-reload"])?;
    println!("✓ Reloaded systemd daemon");

    println!("\n✅ Uninstallation complete. Background monitoring stopped.");

    Ok(())
}

/// Show timer and service status
pub fn status() -> Result<(), InstallerError> {
    println!("=== Timer Status ===\n");

    let timer_output = Command::new("systemctl")
        .args(["--user", "status", "to-do-or-die.timer"])
        .output();

    match timer_output {
        Ok(output) => {
            if output.status.success() {
                println!("{}", String::from_utf8_lossy(&output.stdout));
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("could not be found") {
                    println!("Timer not installed. Run 'to-do-or-die install' to set up.");
                } else {
                    println!("Timer status:\n{}", String::from_utf8_lossy(&output.stdout));
                    if !stderr.is_empty() {
                        println!("{}", stderr);
                    }
                }
            }
        }
        Err(e) => {
            println!("Failed to check timer status: {}", e);
            println!("Make sure systemd is available on your system.");
        }
    }

    println!("\n=== Next Trigger ===\n");

    let list_output = Command::new("systemctl")
        .args(["--user", "list-timers", "to-do-or-die.timer"])
        .output();

    if let Ok(output) = list_output {
        println!("{}", String::from_utf8_lossy(&output.stdout));
    }

    // Check lingering status
    println!("=== Lingering Status ===\n");
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let linger_output = Command::new("loginctl")
        .args(["show-user", &user, "--property=Linger"])
        .output();

    if let Ok(output) = linger_output {
        let status = String::from_utf8_lossy(&output.stdout);
        if status.contains("yes") {
            println!("Lingering: enabled (timer runs when logged out)");
        } else {
            println!("Lingering: disabled (timer stops when you log out)");
            println!("Run 'to-do-or-die install' to enable lingering.");
        }
    }

    Ok(())
}

/// Run a systemctl command
fn run_systemctl(args: &[&str]) -> Result<(), InstallerError> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .map_err(|e| InstallerError::SystemctlError(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(InstallerError::SystemctlError(stderr.to_string()));
    }

    Ok(())
}

/// Enable lingering for the current user
fn enable_lingering() -> Result<(), InstallerError> {
    let output = Command::new("loginctl")
        .args(["enable-linger"])
        .output()
        .map_err(|e| InstallerError::LingerError(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(InstallerError::LingerError(stderr.to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_unit_generation() {
        let unit = timer_unit(5);
        assert!(unit.contains("OnUnitActiveSec=5min"));
        assert!(unit.contains("Persistent=true"));
    }

    #[test]
    fn test_service_unit_generation() {
        let unit = service_unit();
        assert!(unit.contains("Type=oneshot"));
        // The unit contains the full path to the binary + "check" command
        assert!(unit.contains("check"));
        assert!(unit.contains("DISPLAY=:0"));
    }
}
