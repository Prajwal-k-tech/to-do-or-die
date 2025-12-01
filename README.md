# to-do-or-die 💀

A todo CLI with teeth. Miss a deadline, and consequences escalate from gentle notifications to **file deletion**.

## The Concept

Set a todo → Complete it on time → Or face escalating chaos:

Effects trigger based on **percentage of time elapsed**:

| % Elapsed | Effect |
|-----------|--------|
| 50% | 📢 Desktop notification |
| 75% | 🔊 Audio alert |
| 90% | 🗣️ Text-to-speech warning |
| 95% | 🖼️ Wallpaper change |
| 100%+ | 🗑️ File deletion (escalating!) |

**Example**: Set a 2-hour deadline:
- At 1 hour: notification
- At 1.5 hours: audio alert
- At 1h 48m: TTS warning
- At 1h 54m: wallpaper turns red
- At 2 hours: files start getting deleted!

## Installation

```bash
# Clone and build
git clone https://github.com/Prajwal-k-tech/to-do-or-die.git
cd to-do-or-die
cargo build --release

# Install to PATH
cargo install --path .
```

## Quick Start

```bash
# Add a todo with deadline
to-do-or-die add "Finish report" --due "2 hours"
to-do-or-die add "Call mom" --due "30 minutes"

# List your todos
to-do-or-die list

# Complete a todo (use ID or partial match)
to-do-or-die complete a1b2

# Install background monitoring (systemd timer)
to-do-or-die install

# Check status
to-do-or-die status
```

## Commands

| Command | Description |
|---------|-------------|
| `add <desc> --due <time>` | Add a todo with deadline |
| `list [--all]` | List active (or all) todos |
| `complete <id>` | Mark todo as done |
| `install [--no-linger]` | Set up systemd timer |
| `uninstall` | Remove systemd timer |
| `status` | Show timer status |
| `check [--dry-run]` | Manually check deadlines |
| `config show\|set\|reset\|path` | Manage configuration |
| `log [-n N] [--deletions] [--full]` | View audit log |
| `candidates [--full]` | Preview deletion candidates |

## Duration Formats

All these work:
- `2 hours`, `2h`
- `30 minutes`, `30m`, `30min`
- `1 day`, `1d`
- `1 hour 30 minutes`

## What Gets Deleted?

By default, only **safe, recoverable** files:

### Tier 1 (Default)
- `~/.cache/*` - Application caches
- `~/.local/share/Trash/*` - Already-deleted files
- `~/Downloads/*.tmp` - Incomplete downloads
- Browser cookies (forces re-login - annoying but safe)

### Tier 2 (Configurable)
Add your own safe directories in config:
```toml
[deletion]
tier2_paths = ["~/Downloads/temp", "~/Documents/scratch"]
```

### Never Deleted (Blocklist)
- SSH keys (`~/.ssh/`)
- GPG keys (`~/.gnupg/`)
- Password stores
- Git directories
- Our own config files

## Configuration

Config file: `~/.config/to-do-or-die/config.toml`

```bash
# View current config
to-do-or-die config show

# Change settings
to-do-or-die config set escalation_cap 15
to-do-or-die config set cookies false
to-do-or-die config set tts false

# Reset to defaults
to-do-or-die config reset
```

### Available Settings

| Key | Default | Description |
|-----|---------|-------------|
| `check_interval_minutes` | 5 | Timer check interval |
| `escalation_cap` | 10 | Max deletions per check |
| `dry_run` | false | Preview mode (no actual deletions) |
| `notifications` | true | Enable desktop notifications |
| `sound` | true | Enable audio alerts |
| `tts` | true | Enable text-to-speech |
| `wallpaper` | true | Enable wallpaper changes |
| `deletion` | true | Enable file deletion |
| `tier1` | true | Enable cache/cookie deletion |
| `tier2` | true | Enable custom path deletion |
| `cookies` | true | Enable browser cookie deletion |

## How It Works

1. **You add todos** with deadlines via CLI
2. **systemd timer** runs every 5 minutes (after `install`)
3. **Checker** evaluates overdue todos
4. **Effects escalate** based on time overdue
5. **Audit log** records all actions

### Escalation Timeline

Effects trigger based on **percentage of time elapsed**:

| Stage | % Elapsed | Effects |
|-------|-----------|---------|
| 0 | 0-49% | No effects |
| 1 | 50-74% | Notification |
| 2 | 75-89% | + Audio |
| 3 | 90-94% | + TTS |
| 4 | 95-99% | + Wallpaper |
| 5+ | 100%+ | + Deletions (escalating) |

## Viewing Deletions

```bash
# See what files would be deleted
to-do-or-die candidates

# View audit log of past deletions
to-do-or-die log

# See full paths in log
to-do-or-die log --full

# Show only deletions
to-do-or-die log --deletions
```

## Data Storage

Following XDG Base Directory spec:

| File | Location |
|------|----------|
| Config | `~/.config/to-do-or-die/config.toml` |
| Todos | `~/.local/share/to-do-or-die/todos.json` |
| Audit log | `~/.local/state/to-do-or-die/audit.jsonl` |

## Dependencies

- **notify-rust** - Desktop notifications
- **rodio** - Audio playback
- **espeak-ng** - Text-to-speech (system package)
- **systemd** - Background scheduling

## Development

```bash
# Run tests
cargo test

# Build debug
cargo build

# Run directly
cargo run -- add "Test" --due "1 hour"

# Check for issues
cargo clippy
```

## Safety First

- 🔒 **Blocklist** protects critical files
- 👁️ **Audit log** records every deletion
- 🧪 **Dry-run mode** for testing
- ⚙️ **Configurable** - disable any effect
- 📁 **Tier system** - only safe files by default

## License

MIT

---

*"The best productivity tool is the one that makes procrastination painful."*
