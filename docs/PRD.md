# to-do-or-die: Product Requirements Document

## Overview

**to-do-or-die** is a Rust CLI that escalates deadline reminders. At 95%, it attempts a GNOME background-color setting or applies KDE's BreezeDark color scheme; these changes are not automatically restored and do not replace wallpaper images. After the deadline, it can delete selected files, but deletion is disabled by default.

### Core Philosophy
- **Simple CLI**: User only interacts to add todos and mark them complete
- **Invisible enforcement**: Background monitoring via systemd user timer
- **Escalating consequences**: Effects intensify as deadline overage grows
- **Safe by default**: Dry-run starts enabled and live deletion starts disabled
- **Educational**: Built for learning Rust, systemd, and system programming

## User Stories

1. **As a user**, I want to add a todo with a deadline so I'm held accountable
2. **As a user**, I want the system to automatically start enforcing after install
3. **As a user**, I want escalating annoyances when I miss deadlines
4. **As a user**, I want file deletion disabled by default and limited to configured candidates when enabled
5. **As a user**, I want to see what files would be deleted before it happens (dry-run for testing)
6. **As a user**, I want to enable extreme mode for maximum stakes

## Architecture

### System Components

```
┌─────────────────────────────────────────────────────────────────┐
│                         User CLI                                │
│  add, list, complete, install, uninstall, status, check, config │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                      systemd User Timer                         │
│           OnBootSec=1min, OnUnitActiveSec=5min                  │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Deadline Checker                           │
│     Runs `to-do-or-die check` every 5 minutes                   │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                       Effects Chain                             │
│  Stage 1: Notification                                          │
│  Stage 2: Audio alert                                           │
│  Stage 3: TTS announcement                                      │
│  Stage 4: Desktop appearance setting                            │
│  Stage 5+: File deletion (escalating count)                     │
└─────────────────────────────────────────────────────────────────┘
```

### Module Structure

```
src/
├── main.rs          # CLI entry point, clap setup
├── cli.rs           # Command definitions and handlers
├── todos.rs         # Todo struct, JSON persistence
├── config.rs        # Config struct, TOML persistence
├── installer.rs     # systemd service/timer setup
├── checker.rs       # Deadline checking logic
├── effects.rs       # Notifications, audio, TTS, desktop appearance effect
├── safety.rs        # File candidate gathering, blocklist
├── deleter.rs       # Deletion execution + audit logging
└── paths.rs         # XDG path helpers
```

## CLI Commands

### Todo Management

```bash
# Add a todo with deadline
to-do-or-die add "Finish project report" --due "2 hours"
to-do-or-die add "Call mom" --due "30 minutes"

# List todos
to-do-or-die list           # Active todos only
to-do-or-die list --all     # Include completed

# Complete a todo
to-do-or-die complete <id>  # UUID or partial match
```

### System Management

```bash
# Install systemd timer (enables lingering automatically)
to-do-or-die install

# Uninstall systemd timer
to-do-or-die uninstall

# Check timer status
to-do-or-die status

# Manual check (for testing)
to-do-or-die check              # Real execution
to-do-or-die check --dry-run    # Preview only
```

### Configuration

```bash
# Show current config
to-do-or-die config show

# Set config values
to-do-or-die config set escalation_cap 15
to-do-or-die config set check_interval_minutes 10

# Reset to defaults
to-do-or-die config reset
```

## Escalation Timeline

Effects escalate based on **percentage of time elapsed** BEFORE the deadline:

| Stage | % Elapsed | Effects |
|-------|-----------|---------|
| 0 | 0-49% | No effects (working time) |
| 1 | 50-74% | Desktop notification |
| 2 | 75-89% | Notification + audio alert |
| 3 | 90-94% | Notification + TTS announcement |
| 4 | 95-99% | Notification + desktop appearance effect on GNOME/KDE (notification fallback elsewhere) |
| 5 | 100%+ (overdue) | Delete 1 file |
| 6 | 110%+ | Delete 2 files |
| 7 | 120%+ | Delete 3 files |
| 8+ | 130%+ | Delete N files (capped at escalation_cap) |

**Example**: A 2-hour deadline triggers:
- At 1 hour (50%): notification
- At 1.5 hours (75%): notification + audio
- At 1h 48m (90%): notification + TTS
- At 1h 54m (95%): desktop appearance effect on GNOME/KDE, or notification fallback
- At 2 hours (100%): if live deletion is enabled, eligible deletions begin

**Deletion formula**: `min(stage - 4, escalation_cap)` files per check

## Deletion Tiers

### Tier 1: Cache & Cookies (Default)

Built-in candidates (not guaranteed to be disposable):

```
~/.cache/*                                    # Application caches
~/.local/share/Trash/*                        # Already-deleted files
~/Downloads/*.{tmp,part,crdownload}           # Incomplete downloads
~/Downloads/* (older than 30 days)            # Old downloads

# Browser cookies (removal can sign the user out)
~/.config/google-chrome/Default/Cookies
~/.config/google-chrome/Default/Cookies-journal
~/.config/chromium/Default/Cookies
~/.config/chromium/Default/Cookies-journal
~/.mozilla/firefox/*.default*/cookies.sqlite
~/.mozilla/firefox/*.default*/cookies.sqlite-wal
```

### Tier 2: User-Configured Directories (Default)

User can add custom safe directories in config:

```toml
[deletion]
tier2_paths = [
    "~/Downloads/temp",
    "~/Documents/scratch",
]
```

### Tier 3: Extreme Mode (Requires --extreme)

When enabled with `--extreme` flag:
- Can delete from any user-writable location
- Requires explicit typed confirmation
- May require root for some operations
- **NOT IMPLEMENTED IN V1**

## Blocklist (Never Delete)

These paths are always protected:

```
~/.config/to-do-or-die/          # Our own config
~/.local/share/to-do-or-die/     # Our data
~/.local/state/to-do-or-die/     # Our logs
~/.ssh/                          # SSH keys
~/.gnupg/                        # GPG keys
~/.password-store/               # Pass passwords
~/.config/*/credentials*         # Any credentials files
*.key, *.pem, *.crt              # Certificates
.git/                            # Git internals
```

## Configuration Schema

**Location**: `~/.config/to-do-or-die/config.toml`

```toml
[general]
check_interval_minutes = 5       # Timer interval
escalation_cap = 10              # Max deletions per check
dry_run = true                   # Default: preview only

[notifications]
enabled = true
sound_enabled = true
tts_enabled = true
appearance_enabled = true        # GNOME/KDE setting; notification fallback elsewhere

[deletion]
enabled = false                  # Default: deletion disabled
tier1_enabled = true             # Cache, cookies, trash
tier2_enabled = true             # User-configured paths
tier2_paths = []                 # Custom safe directories
cookies_enabled = true           # Browser cookies specifically

[audio]
alert_sound = "default"          # or path to .wav/.mp3

[blocklist]
additional_paths = []            # Extra paths to never delete
```

## Data Schemas

### Todo Item (`todos.json`)

```json
{
  "todos": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "description": "Finish project report",
      "created_at": "2024-01-15T10:30:00Z",
      "due_at": "2024-01-15T12:30:00Z",
      "completed_at": null,
      "stage": 0,
      "deletions_count": 0
    }
  ]
}
```

### Audit Log (`audit.jsonl`)

```json
{"timestamp":"2024-01-15T13:00:00Z","todo_id":"550e8400...","action":"delete","path":"/home/user/.cache/thumbnails/normal/abc.png","stage":5,"dry_run":false}
{"timestamp":"2024-01-15T13:05:00Z","todo_id":"550e8400...","action":"notify","message":"Todo overdue: Finish project report","stage":5}
```

## systemd Units

### Timer (`~/.config/systemd/user/to-do-or-die.timer`)

```ini
[Unit]
Description=Check todos for deadline enforcement

[Timer]
OnBootSec=1min
OnUnitActiveSec=5min
Persistent=true

[Install]
WantedBy=timers.target
```

### Service (`~/.config/systemd/user/to-do-or-die.service`)

```ini
[Unit]
Description=Todo deadline checker

[Service]
Type=oneshot
ExecStart=%h/.cargo/bin/to-do-or-die check
Environment=DISPLAY=:0
Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/%U/bus
```

## Dependencies

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
directories = "5"
chrono = { version = "0.4", features = ["serde"] }
humantime = "2"
uuid = { version = "1", features = ["v4", "serde"] }
notify-rust = "4"
rodio = "0.19"
walkdir = "2"
rand = "0.8"
thiserror = "2"
```

## Implementation Phases

### Phase 1: Core Infrastructure
- [x] PRD document
- [x] Update Cargo.toml with dependencies
- [x] `paths.rs` - XDG path helpers
- [x] `config.rs` - Config struct with defaults
- [x] `todos.rs` - Todo struct with CRUD operations
- [x] Basic CLI skeleton (`add`, `list`, `complete`)

### Phase 2: Timer Integration
- [x] `installer.rs` - Write systemd units, enable lingering
- [x] `checker.rs` - Deadline checking logic skeleton
- [x] `install`, `uninstall`, `status` commands
- [x] Manual `check` command

### Phase 3: Effects Chain
- [x] `effects.rs` - Desktop notifications
- [x] `effects.rs` - Audio alerts (rodio)
- [x] `effects.rs` - TTS via espeak-ng
- [x] `effects.rs` - Desktop appearance effect (GNOME/KDE; notification fallback)

### Phase 4: Deletion System
- [x] `safety.rs` - File candidate gathering
- [x] `safety.rs` - Blocklist enforcement
- [x] `deleter.rs` - Deletion execution
- [x] `deleter.rs` - Audit logging
- [x] Dry-run support for testing

### Phase 5: Polish
- [x] Error handling with thiserror
- [x] Unit tests for safety module
- [ ] Integration tests
- [x] README documentation
- [ ] --extreme mode (Tier 3)

## Testing Strategy

### Unit Tests
- `paths.rs`: Verify XDG path resolution
- `config.rs`: Default values, TOML serialization
- `todos.rs`: CRUD operations, deadline calculations
- `safety.rs`: Blocklist enforcement, candidate filtering

### Integration Tests
- Full CLI workflow: add → check → effects triggered
- Dry-run verification: no actual deletions
- Timer simulation: stage progression

### Manual Testing
- Install/uninstall timer
- Verify notifications appear
- Test audio playback
- Confirm cookie deletion causes re-login

## Success Criteria

1. ✅ Adding todos with human-readable durations works
2. ✅ Timer runs every 5 minutes automatically
3. ✅ Notifications appear when deadlines pass
4. ✅ Effects escalate through all stages
5. ✅ When explicitly enabled, deletion candidates come from Tier 1/2 only by default
6. ✅ Blocklisted paths are never touched
7. ✅ Audit log captures all deletions
8. ✅ Dry-run mode works for testing

## Future Enhancements

- [ ] Extreme mode (Tier 3) with typed confirmation
- [ ] Web dashboard for todo management
- [ ] Mobile notifications via ntfy.sh
- [ ] Customizable escalation curves
- [ ] Multi-device sync
- [ ] Undo deletion within grace period
