#![cfg(target_os = "linux")]

use chrono::{Duration, Utc};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    home: PathBuf,
    target: PathBuf,
    gsettings_log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let target = home.join("safe-target");
        let bin = dir.path().join("bin");
        let gsettings_log = dir.path().join("gsettings.log");

        fs::create_dir_all(home.join(".config/to-do-or-die")).unwrap();
        fs::create_dir_all(home.join(".local/share/to-do-or-die")).unwrap();
        fs::create_dir_all(home.join(".local/state/to-do-or-die")).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&bin).unwrap();

        let gsettings = bin.join("gsettings");
        fs::write(
            &gsettings,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TODO_OR_DIE_TEST_GSETTINGS_LOG\"\nexit 0\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&gsettings, fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            _dir: dir,
            home,
            target,
            gsettings_log,
        }
    }

    fn config(&self, appearance_enabled: bool, deletion_enabled: bool) {
        let text = format!(
            "[general]\ndry_run = false\ncolor = false\n\n[notifications]\nenabled = false\nsound_enabled = false\ntts_enabled = false\nappearance_enabled = {appearance_enabled}\n\n[deletion]\nenabled = {deletion_enabled}\ntier1_enabled = false\ntier2_enabled = true\ntier2_paths = [\"~/safe-target\"]\ncookies_enabled = false\npermanent_delete = true\n"
        );
        fs::write(self.home.join(".config/to-do-or-die/config.toml"), text).unwrap();
    }

    fn todo(&self, created_at: chrono::DateTime<Utc>, due_at: chrono::DateTime<Utc>, stage: u32) {
        let data = json!({
            "todos": [{
                "id": "f0aee1c7-1b8e-4e31-8ec8-5aeef5f03a3a",
                "description": "integration fixture",
                "created_at": created_at,
                "due_at": due_at,
                "completed_at": null,
                "stage": stage,
                "deletions_count": 0
            }]
        });
        fs::write(
            self.home.join(".local/share/to-do-or-die/todos.json"),
            serde_json::to_vec_pretty(&data).unwrap(),
        )
        .unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        let bin_dir = self.gsettings_log.parent().unwrap().join("bin");
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path =
            std::env::join_paths(std::iter::once(bin_dir).chain(std::env::split_paths(&path)))
                .unwrap();

        Command::new(env!("CARGO_BIN_EXE_to-do-or-die"))
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("TODO_OR_DIE_TEST_GSETTINGS_LOG", &self.gsettings_log)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/nonexistent/todo-test-bus",
            )
            .env("PATH", path)
            .output()
            .unwrap()
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn audit_entries(home: &Path) -> Vec<Value> {
    let audit_path = home.join(".local/state/to-do-or-die/audit.jsonl");
    fs::read_to_string(audit_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn cli_check_transitions_effects_previews_and_deletes_only_fixture_files() {
    let fixture = Fixture::new();
    fixture.config(true, true);
    let created_at = Utc::now() - Duration::days(10);
    let due_at = Utc::now() + Duration::hours(5);
    fixture.todo(created_at, due_at, 0);

    let candidate = fixture.target.join("fixture.txt");
    fs::write(&candidate, "temporary fixture").unwrap();

    let first_check = fixture.run(&["check", "--no-dry-run", "--yes"]);
    assert_success(&first_check);
    let first_stdout = String::from_utf8_lossy(&first_check.stdout);
    assert!(first_stdout.contains("Stage: 0 -> 4"));
    assert!(first_stdout.contains("Effects: appearance"));
    assert!(candidate.exists(), "stage 4 must not delete files");

    let second_check = fixture.run(&["check", "--no-dry-run", "--yes"]);
    assert_success(&second_check);
    assert!(
        second_check.stdout.is_empty()
            || !String::from_utf8_lossy(&second_check.stdout).contains("Effects: appearance"),
        "appearance should only fire on a stage transition"
    );
    let effect_calls = fs::read_to_string(&fixture.gsettings_log).unwrap();
    assert_eq!(effect_calls.lines().count(), 2);

    fixture.config(false, true);
    fixture.todo(created_at, Utc::now() - Duration::days(1), 4);

    let dry_run = fixture.run(&["check", "--dry-run"]);
    assert_success(&dry_run);
    assert!(String::from_utf8_lossy(&dry_run.stdout).contains("Would delete 1 files"));
    assert!(
        candidate.exists(),
        "dry-run must leave the fixture file intact"
    );
    assert!(
        !fixture
            .home
            .join(".local/state/to-do-or-die/audit.jsonl")
            .exists()
    );

    let live_check = fixture.run(&["check", "--no-dry-run", "--yes"]);
    assert_success(&live_check);
    assert!(
        !candidate.exists(),
        "live deletion should remove only the fixture file"
    );

    let entries = audit_entries(&fixture.home);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["action"], "delete");
    assert_eq!(entries[0]["path"], candidate.to_string_lossy().as_ref());
    assert_eq!(entries[0]["dry_run"], false);
    assert_eq!(entries[0]["success"], true);
    assert_eq!(entries[0]["permanent"], true);

    let saved_todos: Value = serde_json::from_slice(
        &fs::read(fixture.home.join(".local/share/to-do-or-die/todos.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved_todos["todos"][0]["stage"], 6);
    assert_eq!(saved_todos["todos"][0]["deletions_count"], 1);
}

#[test]
fn fresh_install_keeps_deletion_disabled_even_when_live_check_is_forced() {
    let fixture = Fixture::new();
    let cache = fixture.home.join(".cache");
    fs::create_dir_all(&cache).unwrap();
    let candidate = cache.join("fixture-cache.txt");
    fs::write(&candidate, "temporary fixture").unwrap();
    fixture.todo(
        Utc::now() - Duration::days(10),
        Utc::now() - Duration::hours(1),
        5,
    );

    let live_check = fixture.run(&["check", "--no-dry-run", "--yes"]);
    assert_success(&live_check);
    assert!(
        candidate.exists(),
        "fresh-install defaults must keep deletion disabled"
    );

    let config = fs::read_to_string(fixture.home.join(".config/to-do-or-die/config.toml")).unwrap();
    assert!(config.contains("enabled = false"));
}
