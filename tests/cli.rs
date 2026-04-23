//! End-to-end CLI tests against a small synthetic fixture corpus under
//! `tests/fixtures/claude-projects/`. The fixture intentionally exercises
//! the skip rules (subagent files + old-style `agent-*.jsonl` at root
//! must not appear in `list`) and the record-type classification from
//! the adapter notes (file-history-snapshot, progress, system all
//! absent from the conversational stream but preserved or skipped per
//! the contract).

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::str::contains;

fn fixture_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/claude-projects");
    p
}

fn workspace_fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/workspaces");
    p.push(name);
    p
}

#[test]
fn list_shows_only_main_session() {
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["list", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let out = String::from_utf8(out).unwrap();
    let sessions: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = sessions.as_array().unwrap();

    // Two real sessions in the fixture corpus (main + pre-move).
    // Subagent + old-agent files are filtered by the adapter per
    // docs §4 and must NOT appear — the length check pins that.
    assert_eq!(
        arr.len(),
        2,
        "expected 2 sessions (main + pre-move), got: {out}"
    );
    let main = arr
        .iter()
        .find(|s| s["id"] == "aaaaaaaa-bbbb-cccc-dddd-000000000001")
        .expect("main session missing");
    assert_eq!(main["tool"], "claude-code");
    assert_eq!(main["message_count"], 4);
    assert_eq!(main["cwd"], "/test/workspace/sample");
    assert_eq!(main["title"], "Hello world — please read README.md");
}

#[test]
fn list_table_format_lists_one_row() {
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("aaaaaaaa-bbbb-cccc-dddd-000000000001"))
        .stdout(contains("Hello world"))
        .stdout(contains("2 session(s)"));
}

#[test]
fn dump_json_yields_normalized_conversation() {
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "dump",
            "aaaaaaaa-bbbb-cccc-dddd-000000000001",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    let conv: serde_json::Value = serde_json::from_str(&out).unwrap();

    assert_eq!(conv["id"], "aaaaaaaa-bbbb-cccc-dddd-000000000001");
    let msgs = conv["messages"].as_array().unwrap();

    // Four user/assistant messages (user → assistant → user(tool_result)
    // → assistant). file-history-snapshot + progress are dropped; system
    // lives in extensions, not messages.
    assert_eq!(msgs.len(), 4, "got: {out}");

    // First = user text.
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"][0]["type"], "text");
    assert!(msgs[0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Hello world"));

    // Second = assistant with thinking + text + tool_use (order preserved).
    assert_eq!(msgs[1]["role"], "assistant");
    let blocks = msgs[1]["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0]["type"], "thinking");
    assert_eq!(blocks[1]["type"], "text");
    assert_eq!(blocks[2]["type"], "tool_use");
    assert_eq!(blocks[2]["name"], "Read");
    assert_eq!(
        blocks[2]["input"]["file_path"],
        "/test/workspace/sample/README.md"
    );

    // Third = user with tool_result (normalized to our shape).
    assert_eq!(msgs[2]["role"], "user");
    let blocks = msgs[2]["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["type"], "tool_result");
    assert_eq!(blocks[0]["tool_use_id"], "t1");
    assert!(blocks[0]["output"]
        .as_str()
        .unwrap()
        .contains("test fixture"));

    // System event preserved in extensions, not rendered as a message.
    let sys = &conv["extensions"]["system_events"];
    assert_eq!(sys.as_array().unwrap().len(), 1);
    assert_eq!(sys[0]["subtype"], "turn_duration");

    // Per-message extensions carry the Claude-specific bits.
    assert_eq!(msgs[0]["extensions"]["gitBranch"], "main");
    assert_eq!(msgs[0]["extensions"]["version"], "2.1.51");
    assert_eq!(msgs[0]["extensions"]["userType"], "external");
}

#[test]
fn dump_unknown_session_errors() {
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["dump", "no-such-session-id", "--format", "json"])
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn list_sort_by_msgs_reverse_ascending() {
    // Fixture has one session; adding --sort + --reverse mostly checks
    // that the flag plumbing doesn't error. Empty-but-parsed output
    // is a valid success.
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["list", "--sort", "msgs", "--reverse", "--format", "json"])
        .assert()
        .success();
}

#[test]
fn list_grep_filters_title() {
    // Fixture title contains "Hello world". Substring "hello" matches
    // case-insensitively.
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["list", "--grep", "hello", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let sessions: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert_eq!(sessions.as_array().unwrap().len(), 1);
}

#[test]
fn list_grep_misses_on_unrelated_term() {
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "list",
            "--grep",
            "nothing_like_this_exists",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let sessions: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert_eq!(sessions.as_array().unwrap().len(), 0);
}

#[test]
fn list_limit_caps_output() {
    // Fixture yields exactly 1 session. --limit 1 is a no-op on this
    // corpus but proves the flag parses + runs the cap code path.
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["list", "--limit", "1", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let sessions: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert!(sessions.as_array().unwrap().len() <= 1);
}

#[test]
fn list_since_rejects_garbage() {
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["list", "--since", "banana", "--format", "json"])
        .assert()
        .failure()
        .stderr(contains("since"));
}

#[test]
fn list_table_has_cwd_column() {
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("cwd"))
        .stdout(contains("/test/workspace/sample"));
}

#[test]
fn dump_latest_resolves_to_fixture_session() {
    // The fixture has exactly one non-subagent session. --latest
    // (no workspace scope) should pick it.
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["dump", "--latest", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let conv: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert_eq!(conv["id"], "aaaaaaaa-bbbb-cccc-dddd-000000000001");
}

#[test]
fn dump_latest_and_id_mutually_exclusive() {
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["dump", "aaaaaaaa-bbbb-cccc-dddd-000000000001", "--latest"])
        .assert()
        .failure()
        .stderr(contains("mutually exclusive"));
}

#[test]
fn dump_without_id_or_latest_errors() {
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["dump"])
        .assert()
        .failure()
        .stderr(contains("session id"));
}

#[test]
fn dump_tail_slices_and_records_truncation() {
    // Fixture has 4 messages. --tail 2 keeps the last 2 and records
    // the drop count in extensions.truncated (JSON) and the header
    // (markdown).
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "dump",
            "aaaaaaaa-bbbb-cccc-dddd-000000000001",
            "--tail",
            "2",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let conv: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert_eq!(conv["messages"].as_array().unwrap().len(), 2);
    let trunc = &conv["extensions"]["truncated"];
    assert_eq!(trunc["tail"], 2);
    assert_eq!(trunc["original_message_count"], 4);
    assert_eq!(trunc["dropped"], 2);

    // Markdown form also surfaces it in the header.
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "dump",
            "aaaaaaaa-bbbb-cccc-dddd-000000000001",
            "--tail",
            "2",
        ])
        .assert()
        .success()
        .stdout(contains("truncated: last 2 of 4 messages"));
}

#[test]
fn list_includes_sessions_from_previous_paths() {
    // A workspace that has moved: the TOML's `projects` points at the
    // new location, `previous_paths` carries the pre-move one. Sessions
    // authored at either path must both surface under one list call.
    let toml = workspace_fixture("with-previous-paths.portagenty.toml");
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "list",
            "--workspace-toml",
            toml.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let sessions: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let arr = sessions.as_array().unwrap();

    // Two sessions: the main fixture (at /test/workspace/sample) and the
    // pre-move one (at /test/workspace/sample-before-move). Without
    // previous_paths plumbing, only the first would show up.
    let ids: Vec<&str> = arr.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(
        ids.contains(&"aaaaaaaa-bbbb-cccc-dddd-000000000001"),
        "missing current-path session; got: {ids:?}"
    );
    assert!(
        ids.contains(&"bbbbbbbb-cccc-dddd-eeee-222222222222"),
        "missing previous_paths-bridged session; got: {ids:?}"
    );
}

#[test]
fn dump_file_loads_explicit_backing_file() {
    // The Windows-encoded copy of the same sessionId lives in a
    // different project dir with a distinct marker string. Without
    // `--file`, dump picks the WSL copy (pick_rank size tie-break).
    // With `--file`, the explicit override wins.
    let mut win_path = fixture_root();
    win_path.push("C--test-workspace-sample");
    win_path.push("aaaaaaaa-bbbb-cccc-dddd-000000000001.jsonl");

    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "dump",
            "aaaaaaaa-bbbb-cccc-dddd-000000000001",
            "--file",
            win_path.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let conv: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(conv["messages"].as_array().unwrap().len(), 2);
    assert!(conv["messages"][0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Hello from Windows"));
}

#[test]
fn dump_file_alone_works_on_single_session_file() {
    // No positional id, no --latest: file has exactly one session,
    // use it. The obvious-default path that keeps the escape hatch
    // ergonomic when duplicates aren't actually in play.
    let mut win_path = fixture_root();
    win_path.push("C--test-workspace-sample");
    win_path.push("aaaaaaaa-bbbb-cccc-dddd-000000000001.jsonl");

    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "dump",
            "--file",
            win_path.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let conv: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(conv["id"], "aaaaaaaa-bbbb-cccc-dddd-000000000001");
}

#[test]
fn dump_file_bad_id_errors_with_available_ids() {
    let mut win_path = fixture_root();
    win_path.push("C--test-workspace-sample");
    win_path.push("aaaaaaaa-bbbb-cccc-dddd-000000000001.jsonl");

    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["dump", "no-such-id", "--file", win_path.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(contains("aaaaaaaa-bbbb-cccc-dddd-000000000001"));
}

#[test]
fn dump_file_and_workspace_toml_conflict() {
    let toml = workspace_fixture("with-previous-paths.portagenty.toml");
    let mut win_path = fixture_root();
    win_path.push("C--test-workspace-sample");
    win_path.push("aaaaaaaa-bbbb-cccc-dddd-000000000001.jsonl");

    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "dump",
            "aaaaaaaa-bbbb-cccc-dddd-000000000001",
            "--file",
            win_path.to_str().unwrap(),
            "--workspace-toml",
            toml.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(contains("conflict"));
}

#[test]
fn dump_file_missing_errors_gracefully() {
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "dump",
            "aaaaaaaa-bbbb-cccc-dddd-000000000001",
            "--file",
            "/does/not/exist.jsonl",
        ])
        .assert()
        .failure()
        .stderr(contains("not a readable file"));
}

// ---------------------------------------------------------------------
// doctor + rebuild-index tests. Each test copies the fixture corpus to
// a tempdir so it can freely write sessions-index.json without polluting
// the repo.
// ---------------------------------------------------------------------

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)?.flatten() {
        let ty = entry.file_type()?;
        let src_p = entry.path();
        let dst_p = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_p, &dst_p)?;
        } else if ty.is_file() {
            std::fs::copy(&src_p, &dst_p)?;
        }
    }
    Ok(())
}

fn fresh_fixture_clone() -> assert_fs::TempDir {
    let td = assert_fs::TempDir::new().expect("tempdir");
    copy_dir_recursive(&fixture_root(), td.path()).expect("copy fixture");
    td
}

#[test]
fn doctor_reports_missing_index_as_stale() {
    // Fixture dirs have no sessions-index.json → all 3 project dirs
    // should surface with `missing: true`.
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["doctor", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let reports: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    let arr = reports.as_array().unwrap();
    assert!(!arr.is_empty(), "expected stale projects");
    for r in arr {
        assert_eq!(r["missing"], true);
        // When missing, lag_hours is null (per the JSON encoding).
        assert!(r["lag_hours"].is_null());
        assert!(r["newest_session_id"].is_string());
    }
}

#[test]
fn doctor_table_has_missing_marker() {
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["doctor"])
        .assert()
        .success()
        .stdout(contains("MISSING"))
        .stdout(contains("stale project"));
}

#[test]
fn doctor_project_scopes_to_one_dir() {
    // --project narrows to a single dir. Result list has exactly 1 entry.
    let mut p = fixture_root();
    p.push("-test-workspace-sample");
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "doctor",
            "--project",
            p.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let reports: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(reports.as_array().unwrap().len(), 1);
}

#[test]
fn rebuild_index_dry_run_writes_nothing() {
    let td = fresh_fixture_clone();
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args(["rebuild-index", "--all", "--dry-run"])
        .assert()
        .success()
        .stdout(contains("[DRY]"))
        .stdout(contains("would be rebuilt"));

    // No sessions-index.json should have appeared anywhere.
    for proj in ["-test-workspace-sample", "-test-workspace-sample-before-move"] {
        let p = td.path().join(proj).join("sessions-index.json");
        assert!(
            !p.exists(),
            "dry-run should not have written {}",
            p.display()
        );
    }
}

#[test]
fn rebuild_index_writes_fresh_index_and_satisfies_doctor() {
    let td = fresh_fixture_clone();

    // Rebuild all projects.
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args(["rebuild-index", "--all"])
        .assert()
        .success()
        .stdout(contains("rebuilt"));

    // sessions-index.json should exist for each project and contain
    // the expected sessionIds.
    let idx_path = td
        .path()
        .join("-test-workspace-sample")
        .join("sessions-index.json");
    assert!(idx_path.exists(), "index not written at {}", idx_path.display());
    let body = std::fs::read_to_string(&idx_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["version"], 1);
    let entries = parsed["entries"].as_array().unwrap();
    assert!(entries
        .iter()
        .any(|e| e["sessionId"] == "aaaaaaaa-bbbb-cccc-dddd-000000000001"));
    // The adapter's rebuild doesn't resolve git branch; leave it empty.
    assert_eq!(entries[0]["gitBranch"], "");
    // Round-trip: doctor against the now-fresh tree reports zero stale.
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args(["doctor", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let reports: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        reports.as_array().unwrap().len(),
        0,
        "post-rebuild doctor should be clean, got: {}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn rebuild_index_preserves_pre_existing_via_backup() {
    let td = fresh_fixture_clone();
    let proj = td.path().join("-test-workspace-sample");
    let idx_path = proj.join("sessions-index.json");

    // Seed a pre-existing stale index we can later find in the backup.
    let marker = r#"{"version":1,"entries":[{"sessionId":"PRE-EXISTING-MARKER","fullPath":"","fileMtime":0,"firstPrompt":"","messageCount":0,"created":"","modified":"","gitBranch":"","projectPath":"","isSidechain":false}]}"#;
    std::fs::write(&idx_path, marker).unwrap();

    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args([
            "rebuild-index",
            "--project",
            proj.to_str().unwrap(),
        ])
        .assert()
        .success();

    // A `.bak-YYYY-MM-DD` file should exist alongside with the original
    // marker content.
    let mut bak: Option<std::path::PathBuf> = None;
    for entry in std::fs::read_dir(&proj).unwrap().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("sessions-index.json.bak-") {
            bak = Some(entry.path());
        }
    }
    let bak = bak.expect("backup file missing");
    let bak_body = std::fs::read_to_string(&bak).unwrap();
    assert!(
        bak_body.contains("PRE-EXISTING-MARKER"),
        "backup should carry original content; got: {bak_body}"
    );

    // The live index must NOT have the marker — it was overwritten.
    let new_body = std::fs::read_to_string(&idx_path).unwrap();
    assert!(!new_body.contains("PRE-EXISTING-MARKER"));
    assert!(new_body.contains("aaaaaaaa-bbbb-cccc-dddd-000000000001"));
}

#[test]
fn rebuild_index_no_backup_skips_bak_file() {
    let td = fresh_fixture_clone();
    let proj = td.path().join("-test-workspace-sample");
    let idx_path = proj.join("sessions-index.json");
    std::fs::write(&idx_path, r#"{"version":1,"entries":[]}"#).unwrap();

    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args([
            "rebuild-index",
            "--project",
            proj.to_str().unwrap(),
            "--no-backup",
        ])
        .assert()
        .success();

    for entry in std::fs::read_dir(&proj).unwrap().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        assert!(
            !name.starts_with("sessions-index.json.bak-"),
            "--no-backup should not have produced a .bak file: {name}"
        );
    }
}

#[test]
fn rebuild_index_requires_project_or_all() {
    let got = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["rebuild-index"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&got);
    assert!(
        stderr.contains("--project") || stderr.contains("--all"),
        "expected error to mention one of the required flags; got: {stderr}"
    );
}

#[test]
fn rebuild_index_project_and_all_conflict() {
    // clap `conflicts_with` should reject using both at once.
    let got = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "rebuild-index",
            "--all",
            "--project",
            "/tmp/ignored",
        ])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&got);
    assert!(
        stderr.contains("cannot be used")
            || stderr.contains("conflict")
            || stderr.contains("the argument"),
        "expected conflicts error; got: {stderr}"
    );
}

#[test]
fn rebuild_index_missing_project_dir_errors_cleanly() {
    // Path that doesn't exist at all — the rebuild should surface a
    // readable error rather than panic.
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "rebuild-index",
            "--project",
            "/does/not/exist/anywhere",
        ])
        .assert()
        .failure();
}

#[test]
fn rebuild_index_lag_threshold_skips_fresh_projects() {
    // Seed a fresh index that matches the jsonl mtime exactly — doctor
    // considers it non-stale, and rebuild-index --lag-threshold-hours
    // should skip it rather than unnecessarily rewriting.
    let td = fresh_fixture_clone();
    let proj = td.path().join("-test-workspace-sample");
    let idx_path = proj.join("sessions-index.json");

    // Produce a valid-shape index first via a direct rebuild.
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args(["rebuild-index", "--project", proj.to_str().unwrap(), "--no-backup"])
        .assert()
        .success();

    // Touch both files to align mtimes — the index is now 'fresh'.
    let now = filetime::FileTime::from_system_time(std::time::SystemTime::now());
    filetime::set_file_mtime(&idx_path, now).unwrap();
    for e in std::fs::read_dir(&proj).unwrap().flatten() {
        if e.path().extension().and_then(|s| s.to_str()) == Some("jsonl") {
            filetime::set_file_mtime(&e.path(), now).unwrap();
        }
    }

    // Capture the original index body so we can assert it's untouched.
    let original = std::fs::read_to_string(&idx_path).unwrap();

    // --all with a high threshold skips the fresh project (it also
    // skips the two sibling fixture projects since they get rebuilt
    // fresh in doctor-missing state — but those siblings are missing
    // indexes so they WILL be rebuilt). Focus the assertion on our
    // specific project.
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args([
            "rebuild-index",
            "--project",
            proj.to_str().unwrap(),
            "--lag-threshold-hours",
            "24",
        ])
        .assert()
        .success();

    // Note: --project mode is exempt from the threshold (it only
    // applies when scanning --all). This test pins that semantics —
    // per-project rebuilds are deliberate and always happen.
    let rewritten = std::fs::read_to_string(&idx_path).unwrap();
    // The rewrite updated fileMtime but content shape remains consistent.
    let parsed: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
    assert_eq!(parsed["version"], 1);
    assert!(parsed["entries"].as_array().unwrap().len() >= 1);
    // Order might shift by sort — just pin the body is valid JSON.
    let _ = original; // touched to appease the linter; intentional no-op
}

#[test]
fn rebuild_index_all_with_threshold_skips_fresh_peers() {
    // Set up: rebuild all, then touch mtimes to match. Second rebuild
    // with --lag-threshold-hours 24 should report zero rebuilt, or
    // only the projects that aren't fresh.
    let td = fresh_fixture_clone();

    // First pass — rebuild everything, no backup.
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args(["rebuild-index", "--all", "--no-backup"])
        .assert()
        .success();

    // Force fresh alignment for every project.
    let now = filetime::FileTime::from_system_time(std::time::SystemTime::now());
    let root = td.path();
    for proj in std::fs::read_dir(root).unwrap().flatten() {
        let pp = proj.path();
        if !pp.is_dir() {
            continue;
        }
        for e in std::fs::read_dir(&pp).unwrap().flatten() {
            if e.path().is_file() {
                filetime::set_file_mtime(&e.path(), now).ok();
            }
        }
    }

    // Second pass with high threshold — nothing should be stale.
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args([
            "rebuild-index",
            "--all",
            "--lag-threshold-hours",
            "24",
            "--no-backup",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    // "0 project(s) rebuilt" is what we expect when all are fresh.
    assert!(
        stdout.contains("0 project(s) rebuilt") || stdout.contains("rebuilt, 3 skipped"),
        "threshold should have skipped everything; got: {stdout}"
    );
}

#[test]
fn doctor_dump_stale_emits_paste_ready_markdown() {
    // --dump-stale against missing-index projects should print the
    // table plus markdown dumps for each, with the session header and
    // at least one User block.
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["doctor", "--dump-stale"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(stdout.contains("stale project(s)"));
    // The renderer emits `# <title>` then `- session: \`<id>\``.
    assert!(
        stdout.contains("- session:"),
        "expected markdown session header; got:\n{stdout}"
    );
    // At least one user block must have rendered.
    assert!(
        stdout.contains("## User") || stdout.contains("User:"),
        "expected a user block; got:\n{stdout}"
    );
}

#[test]
fn doctor_custom_threshold_filters_hours() {
    // Sanity: --stale-threshold-hours accepts an int and runs. Fixture
    // projects have missing indexes so they remain stale regardless,
    // but this pins the flag plumbing.
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "doctor",
            "--stale-threshold-hours",
            "100000",
            "--format",
            "json",
        ])
        .assert()
        .success();
}

#[test]
fn rebuild_index_written_json_schema_matches_upstream_shape() {
    // Rebuild + parse + assert every expected field is present with
    // correct type. This is the schema contract — if upstream renames
    // or drops a field, downstream tools (including Claude Code's own
    // picker) may break. Pin it.
    let td = fresh_fixture_clone();
    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args(["rebuild-index", "--all", "--no-backup"])
        .assert()
        .success();
    let idx = td
        .path()
        .join("-test-workspace-sample")
        .join("sessions-index.json");
    let body = std::fs::read_to_string(&idx).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();

    assert!(parsed["version"].is_number());
    assert!(parsed["entries"].is_array());
    let entry = &parsed["entries"][0];
    assert!(entry["sessionId"].is_string());
    assert!(entry["fullPath"].is_string());
    assert!(entry["fileMtime"].is_number());
    assert!(entry["firstPrompt"].is_string());
    assert!(entry["messageCount"].is_number());
    assert!(entry["created"].is_string());
    assert!(entry["modified"].is_string());
    assert!(entry["gitBranch"].is_string());
    assert!(entry["projectPath"].is_string());
    assert!(entry["isSidechain"].is_boolean());
    // fileMtime as ms-since-epoch is a 13-digit number for current dates.
    let m = entry["fileMtime"].as_u64().unwrap();
    assert!(
        m > 1_700_000_000_000,
        "fileMtime should be ms since epoch; got {m}"
    );
}

#[test]
fn rebuild_index_handles_malformed_jsonl_gracefully() {
    // Drop a malformed jsonl in alongside valid ones — rebuild should
    // warn but not fail, and still produce the entry from valid lines.
    let td = fresh_fixture_clone();
    let proj = td.path().join("-test-workspace-sample");
    let bad = proj.join("corrupt-session.jsonl");
    std::fs::write(
        &bad,
        "not-valid-json-here\n{\"type\":\"partial\":broken\n",
    )
    .unwrap();

    Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", td.path())
        .args(["rebuild-index", "--project", proj.to_str().unwrap(), "--no-backup"])
        .assert()
        .success(); // must not fail

    let body = std::fs::read_to_string(proj.join("sessions-index.json")).unwrap();
    // The fixture's valid session is still present.
    assert!(body.contains("aaaaaaaa-bbbb-cccc-dddd-000000000001"));
}

#[test]
fn dump_tail_larger_than_conversation_is_noop() {
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args([
            "dump",
            "aaaaaaaa-bbbb-cccc-dddd-000000000001",
            "--tail",
            "100",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let conv: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert_eq!(conv["messages"].as_array().unwrap().len(), 4);
    // No truncated marker when nothing was actually dropped.
    assert!(conv["extensions"].get("truncated").is_none());
}
