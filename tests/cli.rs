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
