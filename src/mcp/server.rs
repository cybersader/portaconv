//! JSON-RPC 2.0 stdio server for MCP.

use std::io::{self, BufRead, Write};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::adapters::{
    dedup_sessions, grep_sessions, limit_sessions, parse_since, sort_sessions, ClaudeCode,
    ConvoAdapter, SortKey, WorkspaceScope,
};
use crate::render::{render_markdown, MarkdownOptions};
use crate::transform::{apply_path_rewrite, PathRewrite};

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "portaconv";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run_stdio_server() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                write_response(
                    &mut stdout,
                    &error_response(Value::Null, -32700, &format!("parse error: {e}")),
                )?;
                continue;
            }
        };

        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(Value::Null);

        // Notifications (no `id`) never get a response body — but the
        // server still dispatches side-effects if relevant. v0.1 has
        // no side-effect notifications to honor, so we just drop them.
        let is_notification = req.get("id").is_none();

        let response = dispatch(method, params);
        if is_notification {
            continue;
        }

        let envelope = match response {
            Ok(result) => success_response(id, result),
            Err((code, msg)) => error_response(id, code, &msg),
        };
        write_response(&mut stdout, &envelope)?;
    }

    Ok(())
}

fn write_response(out: &mut impl Write, v: &Value) -> io::Result<()> {
    let s = serde_json::to_string(v).unwrap_or_else(|_| "{}".into());
    writeln!(out, "{s}")?;
    out.flush()
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

fn dispatch(method: &str, params: Value) -> Result<Value, (i32, String)> {
    match method {
        "initialize" => Ok(initialize_result()),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list()),
        "tools/call" => tools_call(params),
        "resources/list" => Ok(resources_list()),
        "resources/read" => resources_read(params),
        // Notifications MCP clients send but don't require response:
        "notifications/initialized"
        | "notifications/cancelled"
        | "notifications/roots/list_changed" => Ok(Value::Null),
        other => Err((-32601, format!("method not found: {other}"))),
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION,
        },
        "capabilities": {
            "tools": {},
            "resources": { "listChanged": false, "subscribe": false },
        },
    })
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "list_conversations",
                "description": "List Claude Code conversations discoverable on this machine. Returns SessionMeta entries with id, cwd, started_at, updated_at, message_count, source_path.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "min_messages": {
                            "type": "integer",
                            "description": "Hide sessions with fewer than this many messages (default 1)."
                        },
                        "show_duplicates": {
                            "type": "boolean",
                            "description": "Return every physical-file entry; default false collapses WSL/Windows-encoded duplicates."
                        },
                        "workspace_toml": {
                            "type": "string",
                            "description": "Scope by a portagenty workspace TOML. Accepts an explicit path or the literal string 'auto' to walk up from cwd."
                        },
                        "since": {
                            "type": "string",
                            "description": "Only include sessions updated after this point. Relative duration (e.g. '2d', '6h', '30m', '4w') or absolute date ('2026-04-01' / RFC 3339)."
                        },
                        "sort": {
                            "type": "string",
                            "enum": ["updated", "started", "msgs", "title", "id"],
                            "description": "Sort key (default: updated). Time/count keys default to newest/biggest first; title/id default ascending."
                        },
                        "reverse": {
                            "type": "boolean",
                            "description": "Flip the default sort direction for whichever column is active."
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Cap output at N entries after filter + sort (0 = no cap)."
                        },
                        "grep": {
                            "type": "string",
                            "description": "Case-insensitive substring match on title + cwd. NOT full-content search."
                        }
                    }
                }
            },
            {
                "name": "get_conversation",
                "description": "Load and render one session. Default format is paste-ready markdown. Path-rewrite transforms are opt-in. Pass `latest: true` (optionally with `workspace_toml`) to resolve to the most recent session in scope without a second call.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Session UUID. Optional when `latest: true`." },
                        "latest": {
                            "type": "boolean",
                            "description": "Resolve to the most recent session in scope (after dedup). Mutually exclusive with `id`."
                        },
                        "workspace_toml": {
                            "type": "string",
                            "description": "Scope `latest` lookup to a portagenty workspace. Explicit path or 'auto' to walk up."
                        },
                        "format": {
                            "type": "string",
                            "enum": ["markdown", "json"],
                            "description": "Output format (default: markdown)."
                        },
                        "rewrite": {
                            "type": "string",
                            "enum": ["wsl-to-win", "win-to-wsl", "strip"],
                            "description": "Opt-in OS-path rewriting on content (NOT cwd metadata)."
                        },
                        "include_thinking": {
                            "type": "boolean",
                            "description": "Include assistant thinking blocks in markdown output (default false)."
                        },
                        "full_results": {
                            "type": "boolean",
                            "description": "Emit full tool-result bodies (default false — truncated at 600 chars)."
                        },
                        "tail": {
                            "type": "integer",
                            "description": "Keep only the last N messages. Response records the drop count in extensions.truncated (JSON) or the markdown header."
                        }
                    }
                }
            }
        ]
    })
}

fn tools_call(params: Value) -> Result<Value, (i32, String)> {
    #[derive(Deserialize)]
    struct Envelope {
        name: String,
        #[serde(default)]
        arguments: Value,
    }
    let env: Envelope = serde_json::from_value(params)
        .map_err(|e| (-32602, format!("invalid tools/call params: {e}")))?;

    match env.name.as_str() {
        "list_conversations" => list_conversations(env.arguments),
        "get_conversation" => get_conversation(env.arguments),
        other => Err((-32601, format!("tool not found: {other}"))),
    }
}

#[derive(Deserialize, Default)]
struct ListArgs {
    min_messages: Option<usize>,
    show_duplicates: Option<bool>,
    workspace_toml: Option<String>,
    since: Option<String>,
    sort: Option<String>,
    reverse: Option<bool>,
    limit: Option<usize>,
    grep: Option<String>,
}

fn list_conversations(args: Value) -> Result<Value, (i32, String)> {
    let args: ListArgs = serde_json::from_value(args)
        .map_err(|e| (-32602, format!("invalid list_conversations args: {e}")))?;
    let adapter = ClaudeCode;
    if !adapter.detect() {
        return Err((-32603, "Claude Code storage not detected".into()));
    }
    let mut scope = build_scope(args.workspace_toml.as_deref())
        .map_err(|e| (-32603, format!("workspace scope: {e:#}")))?;
    if let Some(s) = args.since.as_deref() {
        scope.since = Some(parse_since(s).map_err(|e| (-32602, format!("invalid since: {e:#}")))?);
    }
    let mut metas = adapter
        .list(Some(&scope))
        .map_err(|e| (-32603, format!("list failed: {e:#}")))?;
    let min = args.min_messages.unwrap_or(1);
    metas.retain(|m| m.message_count >= min);
    if !args.show_duplicates.unwrap_or(false) {
        metas = dedup_sessions(metas);
    }
    if let Some(needle) = args.grep.as_deref() {
        metas = grep_sessions(metas, needle);
    }
    let key =
        parse_sort_key(args.sort.as_deref()).map_err(|e| (-32602, format!("invalid sort: {e}")))?;
    sort_sessions(&mut metas, key, args.reverse.unwrap_or(false));
    metas = limit_sessions(metas, args.limit.unwrap_or(0));

    // MCP tools return a `content` array of content items; a JSON
    // payload lands as a text block containing its JSON string. Clients
    // parse this themselves — the schema here matches the convention
    // used by other MCP servers.
    let payload = serde_json::to_string(&metas).map_err(|e| (-32603, format!("serialize: {e}")))?;
    Ok(json!({
        "content": [ { "type": "text", "text": payload } ]
    }))
}

fn parse_sort_key(s: Option<&str>) -> Result<SortKey, String> {
    match s.unwrap_or("updated") {
        "updated" => Ok(SortKey::Updated),
        "started" => Ok(SortKey::Started),
        "msgs" => Ok(SortKey::Msgs),
        "title" => Ok(SortKey::Title),
        "id" => Ok(SortKey::Id),
        other => Err(format!("unknown sort key: {other}")),
    }
}

/// Mirror of `cli::resolve_dump_target`. Kept in this module so the
/// MCP handler doesn't have to cross back into cli.rs — the CLI layer
/// owns argv parsing, and this layer owns JSON-RPC params parsing.
fn resolve_mcp_target(adapter: &ClaudeCode, args: &GetArgs) -> Result<String, (i32, String)> {
    let latest = args.latest.unwrap_or(false);
    if let Some(id) = args.id.as_deref() {
        if latest {
            return Err((
                -32602,
                "get_conversation: `id` and `latest` are mutually exclusive".into(),
            ));
        }
        return Ok(id.to_string());
    }
    if !latest {
        return Err((
            -32602,
            "get_conversation: pass either `id` or `latest: true` (optionally with workspace_toml)"
                .into(),
        ));
    }
    let scope = build_scope(args.workspace_toml.as_deref())
        .map_err(|e| (-32603, format!("workspace scope: {e:#}")))?;
    let metas = adapter
        .list(Some(&scope))
        .map_err(|e| (-32603, format!("list failed: {e:#}")))?;
    let metas = dedup_sessions(metas);
    metas
        .into_iter()
        .next()
        .map(|m| m.id)
        .ok_or_else(|| (-32603, "no sessions found in scope for latest".into()))
}

#[derive(Deserialize, Default)]
struct GetArgs {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    latest: Option<bool>,
    #[serde(default)]
    workspace_toml: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    rewrite: Option<String>,
    #[serde(default)]
    include_thinking: Option<bool>,
    #[serde(default)]
    full_results: Option<bool>,
    #[serde(default)]
    tail: Option<usize>,
}

fn get_conversation(args: Value) -> Result<Value, (i32, String)> {
    let args: GetArgs = serde_json::from_value(args)
        .map_err(|e| (-32602, format!("invalid get_conversation args: {e}")))?;
    let adapter = ClaudeCode;
    if !adapter.detect() {
        return Err((-32603, "Claude Code storage not detected".into()));
    }
    let target_id = resolve_mcp_target(&adapter, &args)?;
    let mut conv = adapter
        .load(&target_id)
        .map_err(|e| (-32603, format!("load failed: {e:#}")))?;
    if let Some(kind) = args.rewrite.as_deref() {
        let mode = parse_rewrite(kind).map_err(|e| (-32602, format!("invalid rewrite: {e}")))?;
        apply_path_rewrite(&mut conv, mode);
    }
    if let Some(n) = args.tail {
        conv.apply_tail(n);
    }
    let body = match args.format.as_deref().unwrap_or("markdown") {
        "markdown" => {
            let opts = MarkdownOptions {
                include_thinking: args.include_thinking.unwrap_or(false),
                full_results: args.full_results.unwrap_or(false),
                ..MarkdownOptions::default()
            };
            render_markdown(&conv, &opts)
        }
        "json" => {
            serde_json::to_string_pretty(&conv).map_err(|e| (-32603, format!("serialize: {e}")))?
        }
        other => return Err((-32602, format!("unknown format: {other}"))),
    };
    Ok(json!({
        "content": [ { "type": "text", "text": body } ]
    }))
}

fn parse_rewrite(kind: &str) -> Result<PathRewrite, String> {
    match kind {
        "wsl-to-win" => Ok(PathRewrite::WslToWin),
        "win-to-wsl" => Ok(PathRewrite::WinToWsl),
        "strip" => Ok(PathRewrite::Strip),
        other => Err(format!("unknown rewrite mode: {other}")),
    }
}

/// Advertise one resource-template — the concrete resource list is
/// `resources/list` and would require enumerating every session, which
/// is expensive. MCP's `resources/templates/list` is the idiomatic home
/// for a URI template; until we wire that separately we surface the
/// template here and return an empty flat list.
fn resources_list() -> Value {
    json!({
        "resources": [],
        "resourceTemplates": [
            {
                "uriTemplate": "convos://conversation/{id}",
                "name": "Conversation",
                "description": "A Claude Code conversation rendered as paste-ready markdown.",
                "mimeType": "text/markdown"
            }
        ]
    })
}

#[derive(Deserialize)]
struct ResourceReadArgs {
    uri: String,
}

fn resources_read(params: Value) -> Result<Value, (i32, String)> {
    let args: ResourceReadArgs = serde_json::from_value(params)
        .map_err(|e| (-32602, format!("invalid resources/read params: {e}")))?;
    let id = args
        .uri
        .strip_prefix("convos://conversation/")
        .ok_or_else(|| {
            (
                -32602,
                format!(
                    "unsupported resource URI: {} (expected convos://conversation/<id>)",
                    args.uri
                ),
            )
        })?;
    let adapter = ClaudeCode;
    if !adapter.detect() {
        return Err((-32603, "Claude Code storage not detected".into()));
    }
    let conv = adapter
        .load(id)
        .map_err(|e| (-32603, format!("load failed: {e:#}")))?;
    let md = render_markdown(&conv, &MarkdownOptions::default());
    Ok(json!({
        "contents": [
            {
                "uri": args.uri,
                "mimeType": "text/markdown",
                "text": md
            }
        ]
    }))
}

fn build_scope(flag: Option<&str>) -> Result<WorkspaceScope> {
    crate::cli::build_workspace_scope_public(flag)
}

/// Forwarding re-export for the MCP module. The CLI owns the workspace
/// resolver; this avoids duplicating the walk-up logic.
#[allow(dead_code)]
#[derive(Serialize)]
struct _Marker;
