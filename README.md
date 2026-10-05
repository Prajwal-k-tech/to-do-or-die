# to-do-or-die

A command-line todo application that escalates deadline reminders from notifications to audio and text-to-speech. At 95%, it attempts a GNOME background-color setting or applies KDE's BreezeDark color scheme. These appearance changes are not restored automatically, and the program does not change wallpaper images. Overdue file deletion is available but disabled by default.

Built in Rust as a systems programming project demonstrating CLI design, systemd integration, file system safety, and progressive escalation patterns.

## How It Works

Set a todo with a deadline. A systemd user timer checks every 5 minutes. As the deadline approaches, effects escalate based on the percentage of time elapsed:

| Stage | Time Elapsed | Effect |
|-------|-------------|--------|
| 0 | 0 to 49% | No effects (working time) |
| 1 | 50% | Desktop notification |
| 2 | 75% | Notification + audio alert |
| 3 | 90% | Notification + text-to-speech |
| 4 | 95% | Notification + GNOME background-color or KDE color-scheme setting; notification fallback elsewhere |
| 5+ | 100%+ (overdue) | File deletion (escalating count) |

When a todo goes overdue, live enforcement deletes eligible files on every check cycle only if deletion has been explicitly enabled and dry-run is off. The count escalates from 1 file per check at stage 5, up to a configurable cap.

**Example**: A 2-hour deadline triggers:
- At 1 hour (50%): notification
- At 1.5 hours (75%): notification + audio
- At 1h 48m (90%): notification + TTS
- At 1h 54m (95%): desktop appearance effect on GNOME/KDE, or notification fallback
- At 2 hours (100%): if live deletion is enabled, 1 eligible file per check
- At 2h 12m (110%): 2 files per check
- At 2h 24m (120%): 3 files per check

## Safety First

This tool deletes files, so safety is the top priority:

- **Safe defaults**: New users start in dry-run mode with deletion disabled. You must explicitly enable real enforcement.
- **Trash by default**: Files are moved to the system trash (recoverable), not permanently deleted. Permanent deletion requires explicit opt-in.
- **Deletion candidates**: The built-in list includes caches, browser cookies, trash, and old downloads. Cookies can sign you out and old downloads may still matter, so inspect candidates before enabling live deletion.
- **Blocklist protection**: SSH keys, GPG keys, passwords, git repositories, shell configs, and cryptographic files are never deleted.
- **Path validation**: Root, home directory, and system directories cannot be added as deletion targets.
- **Symlink protection**: Tier 2 paths are canonicalized before walking. Symlinks are never followed during directory traversal.
- **Confirmation prompts**: Manual `check` commands prompt before deleting files. Use `--yes` to skip.
- **Audit log**: Every deletion is logged with timestamp, file path, and todo ID in JSONL format.
- **Atomic writes**: Config and todo files are written atomically (write to temp, then rename) to prevent corruption from crashes.
- **File locking**: A lock file prevents concurrent checker processes from interfering.

## Current Scope

The current version supports built-in candidates and user-configured Tier 2 targets. The broader Tier 3 `--extreme` mode described as a future idea in the PRD is not implemented. Automated Linux integration tests cover stage transitions, the fresh-install deletion default, dry-run behavior, explicitly enabled deletion in an isolated temporary home, and its audit record.

## Installation

### Prerequisites

- Rust toolchain (stable, edition 2024)
- Linux with systemd (for background monitoring)
- User units are written to the configuration directory used by the systemd user manager (usually `~/.config/systemd/user/`). This keeps installation aligned with the manager even when the installer shell has a different `XDG_CONFIG_HOME`.
- Optional system packages for full effect chain:
  - `espeak-ng` for text-to-speech
  - `pulseaudio-utils` (paplay) or `alsa-utils` (aplay) for audio alerts
  - `gsettings` (GNOME) or `plasma-apply-colorscheme` (KDE) for the desktop appearance effect

### Build and Install

```bash
git clone https://github.com/Prajwal-k-tech/to-do-or-die.git
cd to-do-or-die
cargo build --release
cargo install --path .
```

### With Native Audio Support

By default, audio playback uses system commands (paplay/aplay). For native Rust audio playback via rodio, build with the `audio` feature (requires `libasound2-dev` on Debian/Ubuntu):

```bash
sudo apt install libasound2-dev  # Debian/Ubuntu
cargo build --release --features audio
```

### Verify Your Setup

```bash
to-do-or-die doctor
```

This checks all system dependencies and reports what is available.

## Quick Start

```bash
# Add a todo with a deadline
to-do-or-die add "Finish report" --due "2 hours"
to-do-or-die add "Call mom" --due "30 minutes"

# List your todos (sorted by urgency)
to-do-or-die list

# Complete a todo (by index, UUID prefix, or description)
to-do-or-die complete 1
to-do-or-die complete a1b2c3d4
to-do-or-die complete "finish report"

# Check system dependencies
to-do-or-die doctor

# Install background monitoring (systemd timer)
to-do-or-die install

# Check timer status
to-do-or-die status
```

## Enabling Real Enforcement

By default, the tool runs in safe mode (dry-run, deletion disabled). To enable real enforcement:

```bash
# Disable dry-run mode
to-do-or-die config set dry_run false

# Enable file deletion
to-do-or-die config set deletion true

# Verify your settings
to-do-or-die config show

# Preview what would be deleted
to-do-or-die check --dry-run

# Run a live check (will prompt for confirmation)
to-do-or-die check
```

## Commands

| Command | Description |
|---------|-------------|
| `add <desc> --due <time>` | Add a todo with a deadline |
| `list [--all] [--json]` | List todos sorted by urgency |
| `complete <id>` | Mark todo as done (index, UUID, or description) |
| `install [--linger]` | Set up systemd timer for background monitoring |
| `uninstall` | Remove systemd timer |
| `status` | Show timer status |
| `check [--dry-run] [--no-dry-run] [--yes]` | Run deadline check manually |
| `config show / set / reset / path` | Manage configuration |
| `log [-n N] [--deletions] [--full]` | View audit log |
| `candidates [--full] [-n N] [--tier N]` | Preview deletion candidates |
| `targets list / add / remove / suggest` | Manage deletion target directories |
| `blocklist list / add / remove / check` | Manage protected paths |
| `doctor` | Check system dependencies and configuration |

### Aliases

Short aliases are available: `a` (add), `ls` (list), `c`/`done` (complete), `audit` (log), `target` (targets), `block` (blocklist).

## Duration Formats

All of these work:
- `2 hours`, `2h`
- `30 minutes`, `30m`, `30min`
- `1 day`, `1d`
- `1 hour 30 minutes`, `1h30m`

## What Gets Deleted

### Tier 1: Cache and Cookies (Built-in)

Built-in candidates (not guaranteed to be disposable):
- `~/.cache/*` - Application caches
- `~/.local/share/Trash/*` - Already-deleted files
- `~/Downloads/*.tmp`, `*.part`, `*.crdownload` - Incomplete downloads
- `~/Downloads/*` (older than 30 days) - Old downloads
- Browser cookies (Chrome, Chromium, Firefox, Brave, Edge, Opera, Vivaldi)

### Tier 2: User-Configured Directories

Add your own safe directories:

```bash
to-do-or-die targets add ~/Downloads/temp
to-do-or-die targets add ~/Documents/scratch
to-do-or-die targets list
```

### Never Deleted (Blocklist)

These are always protected:
- `~/.ssh/` - SSH keys
- `~/.gnupg/` - GPG keys
- `~/.password-store/` - Password store
- `~/.config/to-do-or-die/` - Application config
- `~/.local/share/to-do-or-die/` - Application data
- `~/.local/state/to-do-or-die/` - Audit logs
- `~/.config/systemd/` - systemd units
- `**/.git/` - Git repository internals (path component check)
- Shell configs: `.bashrc`, `.zshrc`, `.profile`, `.bash_profile`, fish, nushell, elvish configs
- File extensions: `.key`, `.pem`, `.crt`, `.cer`, `.p12`, `.pfx`, `.gpg`, `.asc`

Add custom protected paths:

```bash
to-do-or-die blocklist add ~/important-project/
to-do-or-die blocklist check ~/some/path
```

## Configuration

Config file: `~/.config/to-do-or-die/config.toml`

```bash
to-do-or-die config show          # View with explanations
to-do-or-die config set dry_run false
to-do-or-die config set deletion true
to-do-or-die config set escalation_cap 15
to-do-or-die config set permanent_delete true   # Permanently delete instead of using trash
to-do-or-die config reset
```

### Available Settings

| Key | Default | Description |
|-----|---------|-------------|
| `check_interval_minutes` | 5 | Timer check interval (minutes) |
| `escalation_cap` | 10 | Max files deleted per check |
| `dry_run` | true | Preview mode (no actual changes) |
| `color` | true | Colored terminal output |
| `notifications` | true | Desktop notifications |
| `sound` | true | Audio alerts |
| `tts` | true | Text-to-speech warnings |
| `appearance` | true | Try GNOME/KDE appearance setting; changes are not automatically restored |
| `deletion` | false | Master switch for file deletion |
| `tier1` | true | Cache/cookie/trash deletion |
| `tier2` | true | User-configured directory deletion |
| `cookies` | true | Browser cookie deletion |
| `permanent_delete` | false | Permanent delete vs. move to trash |
| `download_age_days` | 30 | Age threshold for old downloads |
| `alert_sound` | "default" | Audio file path or "default" |

## Data Storage

Following XDG Base Directory specification:

| File | Location |
|------|----------|
| Config | `~/.config/to-do-or-die/config.toml` |
| Todos | `~/.local/share/to-do-or-die/todos.json` |
| Audit log | `~/.local/state/to-do-or-die/audit.jsonl` |
| Lock file | `~/.local/state/to-do-or-die/check.lock` |

## Architecture

```
User CLI  -->  systemd User Timer  -->  Deadline Checker  -->  Effects Chain
                   (5 min)                (stage calc)          (notify, audio,
                                                                TTS, appearance,
                                                                file deletion)
```

### Module Structure

| Module | Responsibility |
|--------|---------------|
| `main.rs` | CLI dispatch, command handlers, output formatting |
| `cli.rs` | clap command definitions, help text, aliases |
| `config.rs` | TOML configuration, defaults, atomic save |
| `todos.rs` | Todo items, JSON persistence, stage calculation |
| `checker.rs` | Deadline checking, effect triggering, file locking |
| `effects.rs` | Notifications, audio, TTS, desktop appearance effect |
| `safety.rs` | Candidate gathering, blocklist, path validation |
| `deleter.rs` | File deletion (trash or permanent), audit logging |
| `installer.rs` | systemd timer/service unit generation |
| `paths.rs` | XDG paths, atomic write helper |

## Development

```bash
cargo build              # Build
cargo test               # Run the unit tests
cargo clippy --all-targets  # Lint (zero warnings)
cargo fmt                # Format code
cargo build --features audio  # Build with native audio
```

## Tech Stack

- **Language**: Rust (edition 2024)
- **CLI**: clap v4 with derive macros
- **Serialization**: serde + serde_json (todos), toml (config)
- **Time**: chrono
- **Notifications**: notify-rust (D-Bus)
- **Audio**: rodio (optional, feature-gated) or system commands
- **File deletion**: trash crate (FreeDesktop.org trash spec)
- **File locking**: fs2
- **Terminal colors**: owo-colors with NO_COLOR support
- **Error handling**: thiserror + anyhow
- **Scheduling**: systemd user timers

## License

MIT
