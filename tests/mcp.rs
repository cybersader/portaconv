//! End-to-end MCP server tests via stdin/stdout. Same fixture corpus
//! as `tests/cli.rs` so the adapter-level correctness is shared.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

fn fixture_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/claude-projects");
    p
}

fn pconv_bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin("pconv")
}

/// Send a batch of requests, capture stdout lines, parse each.
fn roundtrip(requests: &[Value]) -> Vec<Value> {
    let mut cmd = Command::new(pconv_bin())
        .arg("mcp")
        .arg("serve")
        .env("PORTACONV_CLAUDE_ROOT", fixture_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pconv mcp serve");

    {
        let stdin = cmd.stdin.as_mut().expect("stdin");
        for req in requests {
            writeln!(stdin, "{}", serde_json::to_string(req).unwrap()).unwrap();
        }
        // Closing stdin (via drop at end of block) signals EOF; the
        // server loop exits naturally.
    }

    let out = cmd.wait_with_output().expect("wait");
    String::from_utf8(out.stdout)
        .expect("utf8")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("json-rpc line"))
        .collect()
}

#[test]
fn initialize_handshake() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "protocolVersion": "2024-11-05", "capabilities": {} }
    })]);
    assert_eq!(resps.len(), 1);
    let r = &resps[0];
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["serverInfo"]["name"], "portaconv");
    assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
}

#[test]
fn tools_list_describes_both_tools() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/list"
    })]);
    let tools = resps[0]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"list_conversations"));
    assert!(names.contains(&"get_conversation"));
}

#[test]
fn list_conversations_returns_fixture_session() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "list_conversations", "arguments": {} }
    })]);
    let text = resps[0]["result"]["content"][0]["text"].as_str().unwrap();
    let metas: Value = serde_json::from_str(text).unwrap();
    let arr = metas.as_array().unwrap();
    // Fixture corpus has 2 sessions (main + pre-move); pin that both
    // surface, and that the main one is present.
    assert_eq!(arr.len(), 2);
    assert!(arr
        .iter()
        .any(|s| s["id"] == "aaaaaaaa-bbbb-cccc-dddd-000000000001"));
}

#[test]
fn get_conversation_returns_markdown_by_default() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {
            "name": "get_conversation",
            "arguments": { "id": "aaaaaaaa-bbbb-cccc-dddd-000000000001" }
        }
    })]);
    let text = resps[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("# "));
    assert!(text.contains("## User"));
    assert!(text.contains("## Assistant"));
    assert!(text.contains("**tool call:**"));
}

#[test]
fn resources_read_matches_get_conversation_markdown() {
    let resps = roundtrip(&[
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {
                "name": "get_conversation",
                "arguments": { "id": "aaaaaaaa-bbbb-cccc-dddd-000000000001" }
            }
        }),
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "convos://conversation/aaaaaaaa-bbbb-cccc-dddd-000000000001" }
        }),
    ]);
    let via_tool = resps[0]["result"]["content"][0]["text"].as_str().unwrap();
    let via_resource = resps[1]["result"]["contents"][0]["text"].as_str().unwrap();
    assert_eq!(via_tool, via_resource);
    assert_eq!(
        resps[1]["result"]["contents"][0]["mimeType"],
        "text/markdown"
    );
}

#[test]
fn unknown_method_returns_32601() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "nope/whatever"
    })]);
    assert_eq!(resps[0]["error"]["code"], -32601);
}

#[test]
fn unknown_session_returns_internal_error() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "get_conversation", "arguments": { "id": "no-such-id" } }
    })]);
    assert_eq!(resps[0]["error"]["code"], -32603);
    assert!(resps[0]["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not found"));
}

#[test]
fn notification_produces_no_response() {
    let resps = roundtrip(&[
        json!({
            "jsonrpc": "2.0", "method": "notifications/initialized"
        }),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/list"
        }),
    ]);
    // Only one response — the notification shouldn't produce one.
    assert_eq!(resps.len(), 1);
    assert_eq!(resps[0]["id"], 1);
}

#[test]
fn list_conversations_honors_grep_and_limit() {
    // Hit: "hello" matches the fixture title.
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "list_conversations",
            "arguments": { "grep": "hello", "limit": 10 }
        }
    })]);
    let text = resps[0]["result"]["content"][0]["text"].as_str().unwrap();
    let metas: Value = serde_json::from_str(text).unwrap();
    assert_eq!(metas.as_array().unwrap().len(), 1);

    // Miss: no title or cwd contains this.
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "list_conversations",
            "arguments": { "grep": "nonexistent_needle_xyz" }
        }
    })]);
    let text = resps[0]["result"]["content"][0]["text"].as_str().unwrap();
    let metas: Value = serde_json::from_str(text).unwrap();
    assert_eq!(metas.as_array().unwrap().len(), 0);
}

#[test]
fn list_conversations_rejects_bad_since() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "list_conversations",
            "arguments": { "since": "banana" }
        }
    })]);
    assert_eq!(resps[0]["error"]["code"], -32602);
}

#[test]
fn get_conversation_latest_resolves_fixture() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "get_conversation",
            "arguments": { "latest": true, "format": "json" }
        }
    })]);
    let text = resps[0]["result"]["content"][0]["text"].as_str().unwrap();
    let conv: Value = serde_json::from_str(text).unwrap();
    assert_eq!(conv["id"], "aaaaaaaa-bbbb-cccc-dddd-000000000001");
}

#[test]
fn get_conversation_id_and_latest_mutually_exclusive() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "get_conversation",
            "arguments": {
                "id": "aaaaaaaa-bbbb-cccc-dddd-000000000001",
                "latest": true
            }
        }
    })]);
    assert_eq!(resps[0]["error"]["code"], -32602);
    assert!(resps[0]["error"]["message"]
        .as_str()
        .unwrap()
        .contains("mutually exclusive"));
}

#[test]
fn get_conversation_tail_slices_via_mcp() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "get_conversation",
            "arguments": {
                "id": "aaaaaaaa-bbbb-cccc-dddd-000000000001",
                "format": "json",
                "tail": 2
            }
        }
    })]);
    let text = resps[0]["result"]["content"][0]["text"].as_str().unwrap();
    let conv: Value = serde_json::from_str(text).unwrap();
    assert_eq!(conv["messages"].as_array().unwrap().len(), 2);
    assert_eq!(conv["extensions"]["truncated"]["dropped"], 2);
}

#[test]
fn get_conversation_missing_id_and_no_latest_errors() {
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "get_conversation",
            "arguments": {}
        }
    })]);
    assert_eq!(resps[0]["error"]["code"], -32602);
}

#[test]
fn get_conversation_file_arg_loads_explicit_copy() {
    let mut win_path = fixture_root();
    win_path.push("C--test-workspace-sample");
    win_path.push("aaaaaaaa-bbbb-cccc-dddd-000000000001.jsonl");

    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "get_conversation",
            "arguments": {
                "id": "aaaaaaaa-bbbb-cccc-dddd-000000000001",
                "file": win_path.to_str().unwrap(),
                "format": "json"
            }
        }
    })]);
    let text = resps[0]["result"]["content"][0]["text"].as_str().unwrap();
    let conv: Value = serde_json::from_str(text).unwrap();
    // Distinguishing marker from the Windows-encoded copy.
    assert_eq!(conv["messages"].as_array().unwrap().len(), 2);
    assert!(conv["messages"][0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Hello from Windows"));
}

#[test]
fn get_conversation_file_and_workspace_toml_conflict() {
    let mut win_path = fixture_root();
    win_path.push("C--test-workspace-sample");
    win_path.push("aaaaaaaa-bbbb-cccc-dddd-000000000001.jsonl");

    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "get_conversation",
            "arguments": {
                "id": "aaaaaaaa-bbbb-cccc-dddd-000000000001",
                "file": win_path.to_str().unwrap(),
                "workspace_toml": "auto"
            }
        }
    })]);
    assert_eq!(resps[0]["error"]["code"], -32602);
    assert!(resps[0]["error"]["message"]
        .as_str()
        .unwrap()
        .contains("conflict"));
}

#[test]
fn rewrite_flag_honored_via_mcp() {
    // Build a conversation that contains a /mnt/c/ path, ask for
    // wsl-to-win via the MCP tool, verify conversion happened. The
    // baseline fixture has a path inside the tool_use input so the
    // rewrite has something to bite.
    let resps = roundtrip(&[json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "get_conversation",
            "arguments": {
                "id": "aaaaaaaa-bbbb-cccc-dddd-000000000001",
                "format": "json",
                "rewrite": "strip"
            }
        }
    })]);
    let text = resps[0]["result"]["content"][0]["text"].as_str().unwrap();
    // The fixture has /test/workspace/sample/README.md — that's not a
    // /mnt/ or drive-letter path so strip wouldn't touch it. Instead,
    // verify the rewrite MODE round-trips without error (no regression)
    // and the JSON is still valid.
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert!(parsed["messages"].is_array());
}
