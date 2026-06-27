//! Effect triggers: notifications, audio, TTS, wallpaper
//!
//! All effects are designed to be annoying but not destructive.
//! Audio playback via rodio is optional (behind the "audio" feature flag).
//! When the audio feature is not enabled, system commands (paplay/aplay)
//! are used as fallbacks.

use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum EffectsError {
    #[error("Notification failed: {0}")]
    NotificationError(String),
    #[error("Audio playback failed: {0}")]
    AudioError(String),
    #[error("TTS failed: {0}")]
    TtsError(String),
    #[error("Wallpaper change failed: {0}")]
    #[allow(dead_code)]
    WallpaperError(String),
}

/// Send a desktop notification
pub fn send_notification(title: &str, body: &str) -> Result<(), EffectsError> {
    notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .urgency(notify_rust::Urgency::Critical)
        .timeout(notify_rust::Timeout::Milliseconds(10000))
        .show()
        .map_err(|e| EffectsError::NotificationError(e.to_string()))?;

    Ok(())
}

/// Play an alert sound
pub fn play_alert_sound(sound_path: &str) -> Result<(), EffectsError> {
    if sound_path == "default" {
        play_system_bell()
    } else {
        play_audio_file(sound_path)
    }
}

/// Play system bell/beep using available system commands
fn play_system_bell() -> Result<(), EffectsError> {
    // Try paplay first (PulseAudio)
    let result = Command::new("paplay")
        .arg("/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga")
        .output();

    if let Ok(output) = result
        && output.status.success()
    {
        return Ok(());
    }

    // Fallback: try aplay with a beep
    let result = Command::new("aplay")
        .args(["-q", "/usr/share/sounds/alsa/Front_Center.wav"])
        .output();

    if let Ok(output) = result
        && output.status.success()
    {
        return Ok(());
    }

    // Last resort: terminal bell
    print!("\x07");
    Ok(())
}

/// Play an audio file using rodio (if the audio feature is enabled)
/// or fall back to system commands.
fn play_audio_file(path: &str) -> Result<(), EffectsError> {
    #[cfg(feature = "audio")]
    {
        use rodio::{Decoder, OutputStream, Sink};
        use std::fs::File;
        use std::io::BufReader;

        let file = File::open(path)
            .map_err(|e| EffectsError::AudioError(format!("Cannot open {}: {}", path, e)))?;

        let (_stream, stream_handle) = OutputStream::try_default()
            .map_err(|e| EffectsError::AudioError(format!("No audio output: {}", e)))?;

        let sink = Sink::try_new(&stream_handle)
            .map_err(|e| EffectsError::AudioError(format!("Cannot create sink: {}", e)))?;

        let source = Decoder::new(BufReader::new(file))
            .map_err(|e| EffectsError::AudioError(format!("Cannot decode audio: {}", e)))?;

        sink.append(source);
        // Use detach instead of sleep_until_end to avoid blocking the timer thread.
        // The sound will play in the background while the check continues.
        sink.detach();

        return Ok(());
    }

    #[cfg(not(feature = "audio"))]
    {
        // Fall back to system commands when rodio is not available
        let result = Command::new("paplay").arg(path).output();
        if let Ok(output) = result
            && output.status.success()
        {
            return Ok(());
        }

        let result = Command::new("aplay").args(["-q", path]).output();
        if let Ok(output) = result
            && output.status.success()
        {
            return Ok(());
        }

        Err(EffectsError::AudioError(format!(
            "Cannot play audio file '{}'. \
             Install paplay (PulseAudio) or aplay (ALSA), \
             or rebuild with --features audio for native rodio playback.",
            path
        )))
    }
}

/// Speak text using espeak-ng or fallback TTS engines
pub fn speak_text(text: &str) -> Result<(), EffectsError> {
    // Try espeak-ng first
    let result = Command::new("espeak-ng").args(["-v", "en", text]).output();

    if let Ok(output) = result
        && output.status.success()
    {
        return Ok(());
    }

    // Fallback to espeak
    let result = Command::new("espeak").args(["-v", "en", text]).output();

    if let Ok(output) = result
        && output.status.success()
    {
        return Ok(());
    }

    // Fallback to spd-say
    let result = Command::new("spd-say").arg(text).output();

    if let Ok(output) = result
        && output.status.success()
    {
        return Ok(());
    }

    Err(EffectsError::TtsError(
        "No TTS engine available (tried espeak-ng, espeak, spd-say)".to_string(),
    ))
}

/// Set a warning wallpaper.
/// Tries GNOME gsettings first, then KDE Plasma, then falls back to a notification.
pub fn set_warning_wallpaper(todo_description: &str) -> Result<(), EffectsError> {
    let warning_message = format!("TODO OVERDUE: {}", todo_description);

    // Try GNOME gsettings
    let result = Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.background",
            "primary-color",
            "#FF0000",
        ])
        .output();

    if let Ok(output) = result
        && output.status.success()
    {
        let _ = Command::new("gsettings")
            .args([
                "set",
                "org.gnome.desktop.background",
                "color-shading-type",
                "solid",
            ])
            .output();

        let _ = send_notification("WALLPAPER WARNING", &warning_message);
        return Ok(());
    }

    // Try KDE Plasma
    let result = Command::new("plasma-apply-colorscheme")
        .arg("BreezeDark")
        .output();

    if let Ok(output) = result
        && output.status.success()
    {
        let _ = send_notification("WALLPAPER WARNING", &warning_message);
        return Ok(());
    }

    // If we can't change wallpaper, at least send a notification
    send_notification("WALLPAPER WARNING", &warning_message)?;

    Ok(())
}

/// Check if a command exists in the system PATH
pub fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_creation() {
        // This test just verifies the function doesn't panic.
        // Actual notification display depends on desktop environment.
        let result = send_notification("Test", "Test body");
        let _ = result;
    }

    #[test]
    fn test_command_exists() {
        // 'ls' should exist on any Unix system
        assert!(command_exists("ls"));
        // This command should not exist
        assert!(!command_exists("nonexistent-command-xyz123"));
    }
}
