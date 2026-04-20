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

    // Only the main session should surface — subagent + old-agent files
    // are filtered by the adapter per docs §4.
    assert_eq!(arr.len(), 1, "expected 1 session, got: {out}");
    let s = &arr[0];
    assert_eq!(s["id"], "aaaaaaaa-bbbb-cccc-dddd-000000000001");
    assert_eq!(s["tool"], "claude-code");
    assert_eq!(s["message_count"], 4);
    assert_eq!(s["cwd"], "/test/workspace/sample");
    assert_eq!(
        s["title"],
        "Hello world — please read README.md"
    );
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
        .stdout(contains("1 session(s)"));
}

#[test]
fn dump_json_yields_normalized_conversation() {
    let out = Command::cargo_bin("pconv")
        .unwrap()
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .args(["dump", "aaaaaaaa-bbbb-cccc-dddd-000000000001", "--format", "json"])
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
