//! systemd service and timer installation
//!
//! Installs user-level systemd units for background todo monitoring.
//! The timer runs `to-do-or-die check` at a configurable interval.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum InstallerError {
    #[error("Failed to write systemd unit: {0}")]
    WriteError(#[from] std::io::Error),
    #[error("systemctl command failed: {0}")]
    SystemctlError(String),
    #[error("Failed to enable lingering: {0}")]
    LingerError(String),
}

/// Resolve the user-unit directory using the systemd user manager's environment.
/// The installer shell may have a different XDG_CONFIG_HOME from the long-lived
/// manager, and systemctl activates units from the manager's search path.
fn systemd_user_dir() -> Result<PathBuf, InstallerError> {
    let output = Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
        .map_err(|error| InstallerError::SystemctlError(error.to_string()))?;

    if !output.status.success() {
        return Err(InstallerError::SystemctlError(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    let manager_config_home = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("XDG_CONFIG_HOME="))
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());

    let config_home = manager_config_home.unwrap_or_else(|| {
        dirs::home_dir()
            .expect("Could not determine home directory")
            .join(".config")
    });
    Ok(config_home.join("systemd").join("user"))
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

/// systemd service unit content.
/// Detects DISPLAY and WAYLAND_DISPLAY from the current environment at install time
/// instead of hardcoding DISPLAY=:0 (which fails on Wayland and multi-display setups).
fn service_unit() -> String {
    let binary_path = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "to-do-or-die".to_string());

    let display = std::env::var("DISPLAY").ok();
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let xdg_session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok();

    let mut unit = format!(
        r#"[Unit]
Description=Todo deadline checker

[Service]
Type=oneshot
ExecStart={binary_path} check
"#
    );

    if let Some(display) = display {
        unit.push_str(&format!("Environment=DISPLAY={display}\n"));
    }
    if let Some(wl) = wayland_display {
        unit.push_str(&format!("Environment=WAYLAND_DISPLAY={wl}\n"));
    }
    if let Some(st) = xdg_session_type {
        unit.push_str(&format!("Environment=XDG_SESSION_TYPE={st}\n"));
    }
    if let Some(rd) = xdg_runtime_dir {
        unit.push_str(&format!(
            "Environment=DBUS_SESSION_BUS_ADDRESS=unix:path={rd}/bus\n"
        ));
    } else {
        unit.push_str("Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/%U/bus\n");
    }

    unit
}

/// Install systemd user timer and service
pub fn install(interval_minutes: u32, enable_linger: bool) -> Result<(), InstallerError> {
    let systemd_dir = systemd_user_dir()?;
    fs::create_dir_all(&systemd_dir)?;

    // Write timer unit
    let timer_path = systemd_dir.join("to-do-or-die.timer");
    fs::write(&timer_path, timer_unit(interval_minutes))?;
    println!("[ok] Created {}", timer_path.display());

    // Write service unit
    let service_path = systemd_dir.join("to-do-or-die.service");
    fs::write(&service_path, service_unit())?;
    println!("[ok] Created {}", service_path.display());

    // Reload systemd daemon
    run_systemctl(&["--user", "daemon-reload"])?;
    println!("[ok] Reloaded systemd daemon");

    // Enable and start timer
    run_systemctl(&["--user", "enable", "--now", "to-do-or-die.timer"])?;
    println!("[ok] Enabled and started timer");

    // Enable lingering so timer runs when logged out
    if enable_linger {
        enable_lingering()?;
        println!("[ok] Enabled lingering (timer will run when logged out)");
    }

    println!(
        "\nInstallation complete. The timer will check todos every {} minutes.",
        interval_minutes
    );
    println!("Run 'to-do-or-die status' to verify it's running.");

    Ok(())
}

/// Uninstall systemd user timer and service
pub fn uninstall() -> Result<(), InstallerError> {
    let _ = run_systemctl(&["--user", "stop", "to-do-or-die.timer"]);
    let _ = run_systemctl(&["--user", "disable", "to-do-or-die.timer"]);
    println!("[ok] Stopped and disabled timer");

    let systemd_dir = systemd_user_dir()?;
    let timer_path = systemd_dir.join("to-do-or-die.timer");
    let service_path = systemd_dir.join("to-do-or-die.service");

    if timer_path.exists() {
        fs::remove_file(&timer_path)?;
        println!("[ok] Removed {}", timer_path.display());
    }

    if service_path.exists() {
        fs::remove_file(&service_path)?;
        println!("[ok] Removed {}", service_path.display());
    }

    run_systemctl(&["--user", "daemon-reload"])?;
    println!("[ok] Reloaded systemd daemon");

    println!("\nUninstallation complete. Background monitoring stopped.");

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
        assert!(unit.contains("check"));
        match std::env::var("DISPLAY") {
            Ok(display) => assert!(unit.contains(&format!("Environment=DISPLAY={display}"))),
            Err(_) => assert!(!unit.contains("Environment=DISPLAY=")),
        }
    }
}
