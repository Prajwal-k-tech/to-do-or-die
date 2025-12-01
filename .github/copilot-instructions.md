# Copilot instructions for to-do-or-die

This repository is a small Rust CLI (binary crate) that persists todos to `db.json` and, in later phases, runs timed "nudges" and escalating deletions when todos are missed. The current `src/main.rs` is minimal; use these notes to add staged‑nudge behavior, selection/deletion safety, and systemd scheduling in a safe, testable way.

Quick facts
- Entry point: `src/main.rs`. Current commands implemented: `add`, `complete`.
- Persistence: `db.json` (working dir by default). Consider migrating to `~/.config/to-do-or-die/db.json` with an explicit migration path.
- Build & run: `cargo build` / `cargo run -- <command>`.

Phase‑1 feature overview (what to implement first)
- Time‑percent staged nudges per todo: notifications every 10% elapsed, audio at 50/75/95%.
- Escalating deletion schedule after deadline: delete `deletion_base` files at 0% post‑deadline, then increase deletions every `deletion_slot_percent` (default 10%) until capped.
- Only delete files that pass `assess_delete_safety` (writable parent dir, no immutable flag, not in protected paths). `--extreme` requires root and typed confirmation to override.
- Provide `preview` / `--dry-run` (deterministic with `--seed`) and `--force` semantics for destructive operations.

Recommended new modules (mirror under `src/`)
- `src/cli.rs` — `clap` commands: `add`, `list`, `complete`, `preview`, `punish-now`, `install-timer`, `mark-safe`, `config` helpers.
- `src/todos.rs` — typed `TodoItem`, `Todos` container, `load_db`/`save_db` using `serde`.
- `src/scheduler.rs` — percent calculations, stage recognition, `trigger_todo_stage`.
- `src/select.rs` — `gather_candidates`, seeded `select_random`.
- `src/safety.rs` — `assess_delete_safety(path: &Path) -> Deletability` (see heuristics below).
- `src/actions.rs` — `send_notification`, `play_sound`, `delete_files`, `change_wallpaper` (optional).
- `src/config.rs` — `Config` load/save (`config.toml`) and permission map storage.
- `src/log.rs` — audit JSONL (`~/.local/share/to-do-or-die/audit.log`).

Permission & safety heuristics (concrete, implement these checks)
- Canonicalize candidate paths (`Path::canonicalize`) to avoid symlink tricks.
- Check parent directory write permission (owner/group/other and group membership).
- Respect sticky bit (only owner/dir-owner/root can delete inside sticky dirs like `/tmp` behavior).
- Check immutable attribute (`chattr +i`) where available (optional `ioctl`), and read‑only mounts.
- Classify paths as `safe|caution|dangerous` using config defaults: default safe dirs include `~/Downloads` and `/tmp`; system dirs are `dangerous`.
- Skip `dangerous` unless `--extreme` + running as root + explicit typed confirmation.

Systemd scheduling (systemd‑first UX)
- Prefer a systemd *user* timer for background checks. Implement `install-timer` / `uninstall-timer` helpers that write units to `~/.config/systemd/user/` and call `systemctl --user daemon-reload` + `enable --now`.
- Default installed timer should operate in `--dry-run` until the user explicitly enables destructive automation (`allow_destructive_automation = true` in `config.toml`).
- Always acquire a file lock (`flock`) or similar when the timer runs to avoid concurrent executions.

Testing & developer workflows (project‑specific)
- Add unit tests for: `gather_candidates` filtering, deterministic `select_random`, `assess_delete_safety` heuristics, and scheduler stage transitions. Use `tempfile` / `tempdir` for filesystem fixtures.
- Integration test: simulate todo lifecycle by manipulating `created_at`/`due_at` and verify notifications/audio triggers and deletion counts in dry‑run mode.
- Tools: `cargo build`, `cargo test`, `cargo fmt`, `cargo clippy -- -D warnings`.

Patterns to follow (do not deviate silently)
- Keep changes small and incremental in `src/` (don't rewrite `main.rs` in one patch). Add modules and wire them in `main.rs` progressively.
- Persist only JSON for now; if moving DB path, include an automated migration and preserve old `db.json` as backup.
- All destructive actions must support `--dry-run` and be logged to the audit file.

Files to inspect first when implementing features
- `src/main.rs` — current style/argument parsing approach.
- `Cargo.toml` — add dependencies like `clap`, `serde`, `chrono`, `rand`, `walkdir`, `notify-rust`, `rodio`, `nix` as needed.

If you make a change
- Update unit tests and add integration tests for new behaviors.
- Update `README.md` and this file when major behavior or config locations change.

Questions or unclear areas
- If permission model, default safe dirs, or `db.json` path are unclear, leave a `TODO` comment and open an issue for the owner to confirm.

---
Keep this file up to date as the code evolves; after edits I will run tests and request further clarifications if needed.
# Copilot instructions for to-do-or-die

This repository is a small Rust CLI (binary crate) that persists a simple todo map to `db.json`. The current implementation lives in `src/main.rs` and uses `serde_json` for persistence. Use these notes to help you make safe, focused changes and to find the right places to implement new features.

Key facts (read before editing)
- Entry point: `src/main.rs` (single-file CLI today). Existing commands: `add` and `complete` (args: action, item).
- Persistence: `db.json` in the working directory — written/read with `serde_json::to_writer_pretty` / `from_reader`.
- Build: `cargo build` (Rust edition 2024). Run: `cargo run -- <action> <item>`.
- Cargo manifest: `Cargo.toml` currently lists `serde_json`. Add new crates to `Cargo.toml` as needed.

What the codebase expects from you
- Keep the repo a single binary crate under `src/` (if you split into modules, mirror the `mod` layout under `src/`).
- Preserve the simple JSON DB format unless you intentionally migrate to a different path (e.g. `~/.config/...`) — document migration steps.
- Avoid any change that performs destructive filesystem actions by default. Add `--dry-run` and explicit `--force` flags for destructive commands.

Recommended modules and files to add (Phase‑1 design)
- `src/cli.rs` — `clap` bindings and argument parsing.
- `src/todos.rs` — typed `TodoItem`, load/save helpers (uses `serde::{Serialize, Deserialize}`).
- `src/scheduler.rs` — percent calculations and stage triggering.
- `src/select.rs` or `src/safety.rs` — `assess_delete_safety(path: &Path) -> Deletability` (see notes below).
- `src/actions.rs` — notification, audio, and deletion routines.

Important patterns & conventions to follow
- Use `serde` derives for structs persisted to `db.json` (add `serde = { version = "1", features = ["derive"] }` to `Cargo.toml`).
- Use canonicalized paths (`Path::canonicalize`) before performing deletion or classification to avoid symlink tricks.
- The Unix deletion model: check parent directory write bit, sticky bit, immutable attribute (`chattr +i`), and read‑only mounts — implement `assess_delete_safety` combining these heuristics.
- Always log destructive choices to an audit JSONL file under `~/.local/share/to-do-or-die/audit.log` for transparency.

Build / test / debug workflows
- Build: `cargo build --release` or `cargo build` for dev.
- Run interactive: `cargo run -- add "Buy milk"` or `cargo run -- complete "Buy milk"`.
- Run unit tests: `cargo test` (add tests under `tests/` or `src/` with `#[cfg(test)]`).
- Formatting / linting: run `cargo fmt` and `cargo clippy -- -D warnings` when making changes.

Security & safety guidance for AI changes
- Never enable deletion across system directories by default. Mark `extreme_mode` opt‑in in `config.toml` and require typed confirmation + root to perform system deletions.
- Add `--dry-run` paths for any deletion or cache‑clearing command. Preview and reproducible selection via RNG seed are required.

Integration & ops notes
- Background scheduling: prefer a systemd *user* timer. Provide helper CLI commands `install-timer` / `uninstall-timer` that write units to `~/.config/systemd/user/` and call `systemctl --user` so users don't edit units manually.
- Document unit files and how the helper works in `README.md`.

Where to look for context/examples in this repo
- `src/main.rs` — current implementation and style. Keep small changes incremental.
- `Cargo.toml` — dependency list; update when adding crates such as `clap`, `chrono`, `rand`, `walkdir`, `notify-rust`, `rodio`, `nix`.

If you make a change
- Add or update unit tests that cover critical behavior (selection, permission checks, percent thresholds). Use `tempfile` / `tempdir` for filesystem tests.
- Update `README.md` with any new user-facing commands, and add a short CHANGELOG entry.

Questions or missing info
- If a behavior or config location is unclear, leave a short `TODO` comment in code and open an issue explaining the ambiguity. Ask the repository owner whether `db.json` should move to `~/.config` before migrating data.

If this file is missing or outdated, update it with the main entrypoint and any new module files you add.

---
Feedback: after applying changes I will ask for requested clarifications or preferred conventions (e.g., default config path, destructive defaults).
