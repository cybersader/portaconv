//! Claude Code adapter.
//!
//! Reads `~/.claude/projects/*/*.jsonl` (and nested session dirs). Each
//! line is one JSON record; records with `type ∈ {user, assistant}` carry
//! conversational content, the rest are metadata. See
//! `docs/src/content/docs/reference/adapter-claude-code.md` for the
//! full record-type contract — this file is the implementation of it.

use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{ConvoAdapter, SessionMeta, WorkspaceScope};
use crate::model::{ContentBlock, Conversation, Message, Role};

/// Storage root for Claude Code. Overridable via env for tests.
fn projects_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PORTACONV_CLAUDE_ROOT") {
        return Some(PathBuf::from(p));
    }
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

pub struct ClaudeCode;

impl ConvoAdapter for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn detect(&self) -> bool {
        projects_root().map(|p| p.is_dir()).unwrap_or(false)
    }

    fn list(&self, scope: Option<&WorkspaceScope>) -> Result<Vec<SessionMeta>> {
        let root = projects_root().ok_or_else(|| anyhow!("no home dir"))?;
        if !root.is_dir() {
            return Ok(Vec::new());
        }

        let mut out: Vec<SessionMeta> = Vec::new();
        for file in walk_jsonls(&root)? {
            if is_subagent_file(&file) {
                continue;
            }
            // Per the research spike a single file can hold multiple
            // sessionIds (/compact rewrites the continuation under a new
            // id but appends to the same file). list() surfaces every
            // distinct sessionId — matches Claude's /resume mental model.
            for meta in scan_file(&file)?.into_iter() {
                if let Some(s) = scope {
                    if !scope_matches(&meta, s) {
                        continue;
                    }
                }
                out.push(meta);
            }
        }
        out.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
        Ok(out)
    }

    fn load(&self, id: &str) -> Result<Conversation> {
        let root = projects_root().ok_or_else(|| anyhow!("no home dir"))?;
        // A sessionId can appear in multiple physical files: its home file,
        // WSL/Windows-encoded duplicates, and sibling files when /compact
        // writes a continuation under a new sessionId but appends to the
        // parent's file. Prefer the canonical home file (basename stem ==
        // sessionId); tie-break by size (larger = fuller history).
        let mut candidates: Vec<PathBuf> = Vec::new();
        for file in walk_jsonls(&root)? {
            if is_subagent_file(&file) {
                continue;
            }
            if file_contains_session(&file, id)? {
                candidates.push(file);
            }
        }
        if candidates.is_empty() {
            return Err(anyhow!("session {id} not found under {}", root.display()));
        }
        candidates.sort_by_key(|p| pick_rank(p, id));
        parse_session(&candidates[0], id)
    }
}

/// Lower rank = higher preference. Tuple ordering does the work:
///   1. 0 if basename stem matches id, else 1 (home file wins)
///   2. size in bytes, negated so larger wins on tie
fn pick_rank(p: &Path, id: &str) -> (u8, i64) {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let tier = if stem == id { 0 } else { 1 };
    let size = std::fs::metadata(p).map(|m| m.len() as i64).unwrap_or(0);
    (tier, -size)
}

fn walk_jsonls(root: &Path) -> Result<Vec<PathBuf>> {
    let mut acc = Vec::new();
    walk(root, &mut acc)?;
    Ok(acc)
}

fn walk(dir: &Path, acc: &mut Vec<PathBuf>) -> Result<()> {
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Ok(()), // unreadable subtree isn't fatal
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk(&p, acc)?;
        } else if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            acc.push(p);
        }
    }
    Ok(())
}

fn is_subagent_file(p: &Path) -> bool {
    // New shape: any ancestor dir named `subagents`.
    let in_subagents_dir = p.components().any(|c| c.as_os_str() == "subagents");
    if in_subagents_dir {
        return true;
    }
    // Old shape: project-root `agent-<hash>.jsonl` files (Claude ≤ 2.0.x
    // wrote subagent sessions here). Per the adapter notes §4.
    p.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with("agent-") && n.ends_with(".jsonl"))
        .unwrap_or(false)
}

fn scope_matches(meta: &SessionMeta, scope: &WorkspaceScope) -> bool {
    if let Some(since) = scope.since {
        let last = meta.updated_at.or(meta.started_at);
        if last.map(|t| t < since).unwrap_or(true) {
            return false;
        }
    }
    if !scope.project_paths.is_empty() {
        let Some(cwd) = meta.cwd.as_ref() else {
            return false;
        };
        let matched = scope
            .project_paths
            .iter()
            .any(|p| cwd.starts_with(p) || cwd == p);
        if !matched {
            return false;
        }
    }
    true
}

/// First pass: cheap scan for SessionMeta. Does not parse content
/// bodies — only touches a few fields per line.
fn scan_file(path: &Path) -> Result<Vec<SessionMeta>> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let rdr = BufReader::new(f);

    // One entry per distinct sessionId seen in the file.
    use std::collections::BTreeMap;
    struct Acc {
        cwd: Option<PathBuf>,
        first_ts: Option<DateTime<Utc>>,
        last_ts: Option<DateTime<Utc>>,
        msg_count: usize,
        title: Option<String>,
    }
    let mut per_session: BTreeMap<String, Acc> = BTreeMap::new();

    for line in rdr.lines() {
        let Ok(line) = line else { continue };
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        let Some(sid) = v.get("sessionId").and_then(|s| s.as_str()) else {
            continue;
        };
        let acc = per_session.entry(sid.to_string()).or_insert(Acc {
            cwd: None,
            first_ts: None,
            last_ts: None,
            msg_count: 0,
            title: None,
        });

        if acc.cwd.is_none() {
            if let Some(cwd) = v.get("cwd").and_then(|s| s.as_str()) {
                acc.cwd = Some(PathBuf::from(cwd));
            }
        }

        if let Some(ts) = v.get("timestamp").and_then(|s| s.as_str()) {
            if let Ok(parsed) = DateTime::parse_from_rfc3339(ts) {
                let parsed = parsed.with_timezone(&Utc);
                acc.first_ts = Some(acc.first_ts.map_or(parsed, |e| e.min(parsed)));
                acc.last_ts = Some(acc.last_ts.map_or(parsed, |e| e.max(parsed)));
            }
        }

        let ty = v.get("type").and_then(|s| s.as_str()).unwrap_or("");
        if matches!(ty, "user" | "assistant") {
            acc.msg_count += 1;
            if acc.title.is_none() && ty == "user" {
                // Title derived from the first user message's text. The
                // research spike saw message.content either as an array
                // of blocks or (older Claude) as a bare string.
                acc.title = extract_title(&v);
            }
        }
    }

    Ok(per_session
        .into_iter()
        .map(|(id, acc)| SessionMeta {
            id,
            tool: "claude-code",
            title: acc.title,
            cwd: acc.cwd,
            started_at: acc.first_ts,
            updated_at: acc.last_ts,
            message_count: acc.msg_count,
            source_path: path.to_path_buf(),
        })
        .collect())
}

fn extract_title(v: &Value) -> Option<String> {
    let content = v.pointer("/message/content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .find_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text").and_then(|t| t.as_str()).map(str::to_string)
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        _ => return None,
    };
    let first_line = text.lines().next().unwrap_or_default().trim();
    if first_line.is_empty() {
        None
    } else {
        // Keep it short. The renderer can show more if needed.
        Some(first_line.chars().take(120).collect())
    }
}

fn file_contains_session(path: &Path, id: &str) -> Result<bool> {
    let f = File::open(path)?;
    let rdr = BufReader::new(f);
    let needle = format!("\"sessionId\":\"{id}\"");
    for line in rdr.lines() {
        let Ok(line) = line else { continue };
        // Substring check is sufficient — sessionId values are 36-char
        // UUIDs so collisions against unrelated substrings are not a
        // practical concern. Correct-parse happens in parse_session().
        if line.contains(&needle) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Second pass: full parse, filtered to the requested sessionId.
fn parse_session(path: &Path, session_id: &str) -> Result<Conversation> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let rdr = BufReader::new(f);

    let mut messages: Vec<Message> = Vec::new();
    let mut cwd: Option<PathBuf> = None;
    let mut started_at: Option<DateTime<Utc>> = None;
    let mut title: Option<String> = None;
    let mut system_events: Vec<Value> = Vec::new();
    let mut unknown_records: Vec<Value> = Vec::new();

    for line in rdr.lines() {
        let Ok(line) = line else { continue };
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v.get("sessionId").and_then(|s| s.as_str()) != Some(session_id) {
            continue;
        }

        if cwd.is_none() {
            if let Some(c) = v.get("cwd").and_then(|s| s.as_str()) {
                cwd = Some(PathBuf::from(c));
            }
        }

        let ty = v.get("type").and_then(|s| s.as_str()).unwrap_or("");
        let ts = v.get("timestamp").and_then(|s| s.as_str()).and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|t| t.with_timezone(&Utc))
        });
        if started_at.is_none() {
            started_at = ts;
        }

        match ty {
            "user" | "assistant" => {
                let role = if ty == "user" {
                    Role::User
                } else {
                    Role::Assistant
                };
                let content = decode_content(&v);
                if role == Role::User && title.is_none() {
                    title = extract_title(&v);
                }
                messages.push(Message {
                    role,
                    content,
                    timestamp: ts,
                    extensions: keep_message_extensions(&v),
                });
            }
            "system" => {
                system_events.push(v.clone());
            }
            // Skipped per adapter notes §3.
            "file-history-snapshot" | "progress" | "queue-operation" => {}
            _ => {
                // Unknown — surface in extensions per the resilience rule.
                unknown_records.push(v.clone());
            }
        }
    }

    if messages.is_empty() && system_events.is_empty() {
        return Err(anyhow!(
            "no records in {} matched sessionId {session_id}",
            path.display()
        ));
    }

    let mut ext = serde_json::Map::new();
    if !system_events.is_empty() {
        ext.insert("system_events".into(), Value::Array(system_events));
    }
    if !unknown_records.is_empty() {
        ext.insert("unknown_records".into(), Value::Array(unknown_records));
    }
    let extensions = if ext.is_empty() {
        Value::Null
    } else {
        Value::Object(ext)
    };

    Ok(Conversation {
        id: session_id.to_string(),
        title,
        cwd,
        started_at,
        messages,
        extensions,
    })
}

/// Pull the in-record fields the adapter promotes to per-message
/// extensions. Keep it small — anything not used downstream stays out
/// of the shared model's surface.
fn keep_message_extensions(v: &Value) -> Value {
    let Some(obj) = v.as_object() else {
        return Value::Null;
    };
    let mut out = serde_json::Map::new();
    for k in [
        "uuid",
        "parentUuid",
        "requestId",
        "isSidechain",
        "isMeta",
        "isApiErrorMessage",
        "isCompactSummary",
        "isVisibleInTranscriptOnly",
        "slug",
        "userType",
        "gitBranch",
        "version",
        "permissionMode",
        "thinkingMetadata",
        "compactMetadata",
        "logicalParentUuid",
    ] {
        if let Some(val) = obj.get(k) {
            out.insert(k.to_string(), val.clone());
        }
    }
    if out.is_empty() {
        Value::Null
    } else {
        Value::Object(out)
    }
}

fn decode_content(v: &Value) -> Vec<ContentBlock> {
    let content = match v.pointer("/message/content") {
        Some(c) => c,
        None => return Vec::new(),
    };
    match content {
        Value::String(s) => vec![ContentBlock::text(s.clone())],
        Value::Array(arr) => arr.iter().map(decode_block).collect(),
        _ => Vec::new(),
    }
}

fn decode_block(b: &Value) -> ContentBlock {
    let ty = b.get("type").and_then(|s| s.as_str()).unwrap_or("");
    match ty {
        "text" => ContentBlock::Text {
            text: b.get("text").and_then(|t| t.as_str()).unwrap_or("").into(),
        },
        "thinking" => ContentBlock::Thinking {
            text: b
                .get("thinking")
                .and_then(|t| t.as_str())
                .or_else(|| b.get("text").and_then(|t| t.as_str()))
                .unwrap_or("")
                .into(),
        },
        "tool_use" => ContentBlock::ToolUse {
            id: b.get("id").and_then(|s| s.as_str()).unwrap_or("").into(),
            name: b.get("name").and_then(|s| s.as_str()).unwrap_or("").into(),
            input: b.get("input").cloned().unwrap_or(Value::Null),
        },
        "tool_result" => ContentBlock::ToolResult {
            tool_use_id: b
                .get("tool_use_id")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .into(),
            output: match b.get("content") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|x| x.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            },
            is_error: b.get("is_error").and_then(|x| x.as_bool()).unwrap_or(false),
        },
        _ => ContentBlock::Unknown { raw: b.clone() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_detection() {
        assert!(is_subagent_file(Path::new(
            "/x/-mnt-…-proj/abc/subagents/agent-xyz.jsonl"
        )));
        assert!(is_subagent_file(Path::new(
            "/x/-mnt-…-proj/agent-a1234.jsonl"
        )));
        assert!(!is_subagent_file(Path::new(
            "/x/-mnt-…-proj/01234567-89ab-cdef-0123-456789abcdef.jsonl"
        )));
    }
}
