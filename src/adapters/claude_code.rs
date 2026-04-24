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
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::cache::{self, ListCache};
use super::{ConvoAdapter, SessionMeta, WorkspaceScope};
use crate::model::{ContentBlock, Conversation, Message, Role};

/// Storage root for Claude Code. Overridable via env for tests.
pub(crate) fn projects_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PORTACONV_CLAUDE_ROOT") {
        return Some(PathBuf::from(p));
    }
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

/// Flag + stats captured per list() call. The CLI (and MCP server,
/// if it grows a debug mode) can read these back to expose cache
/// behavior without the trait having to surface it.
#[derive(Debug, Default, Clone)]
pub struct ListRunStats {
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub cache_enabled: bool,
    pub cache_path: Option<PathBuf>,
}

thread_local! {
    // Tiny thread-local switch for the --no-cache flag. Avoids threading
    // an option through the ConvoAdapter trait (which is tool-agnostic
    // and shouldn't know about caching). Set via the helper below.
    static NO_CACHE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static LAST_STATS: std::cell::RefCell<ListRunStats> =
        std::cell::RefCell::new(ListRunStats::default());
}

pub fn set_no_cache(no: bool) {
    NO_CACHE.with(|c| c.set(no));
}

pub fn take_last_stats() -> ListRunStats {
    LAST_STATS.with(|c| std::mem::take(&mut *c.borrow_mut()))
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

        let use_cache = !NO_CACHE.with(|c| c.get());
        let mut cache = if use_cache {
            cache::load_or_empty()
        } else {
            ListCache::default()
        };
        let mut hits = 0usize;
        let mut misses = 0usize;

        let mut out: Vec<SessionMeta> = Vec::new();
        for file in walk_jsonls(&root)? {
            if is_subagent_file(&file) {
                continue;
            }
            // Per the research spike a single file can hold multiple
            // sessionIds (/compact rewrites the continuation under a new
            // id but appends to the same file). list() surfaces every
            // distinct sessionId — matches Claude's /resume mental model.
            let metas = if use_cache {
                match cache::lookup(&cache, &file) {
                    Some(cached) => {
                        hits += 1;
                        cached
                            .into_iter()
                            .map(|c| c.into_session_meta(file.clone()))
                            .collect()
                    }
                    None => {
                        misses += 1;
                        let fresh = scan_file(&file)?;
                        cache::record(&mut cache, &file, &fresh);
                        fresh
                    }
                }
            } else {
                scan_file(&file)?
            };
            for meta in metas.into_iter() {
                if let Some(s) = scope {
                    if !scope_matches(&meta, s) {
                        continue;
                    }
                }
                out.push(meta);
            }
        }

        if use_cache {
            cache::prune_missing(&mut cache);
            if let Err(e) = cache::save(&cache) {
                // Cache write failure isn't fatal — fresh data already
                // in `out`. Note and carry on.
                eprintln!("pconv: warning: failed to write list cache: {e:#}");
            }
        }

        LAST_STATS.with(|c| {
            *c.borrow_mut() = ListRunStats {
                cache_hits: hits,
                cache_misses: misses,
                cache_enabled: use_cache,
                cache_path: if use_cache { cache::cache_path() } else { None },
            };
        });

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

impl ClaudeCode {
    /// Bypass the corpus walk and load a specific file. The escape hatch
    /// for `dump --file <path>`: when a sessionId lives in more than one
    /// JSONL (typically WSL- and Windows-encoded project dirs after
    /// cross-OS work), `load()`'s automatic pick may pick the wrong one.
    /// This honors the user's explicit choice.
    pub fn load_from_file(&self, path: &Path, id: &str) -> Result<Conversation> {
        if !path.is_file() {
            return Err(anyhow!("--file: {} is not a readable file", path.display()));
        }
        parse_session(path, id)
    }

    /// List the sessions present in a single JSONL. Used to resolve
    /// `--file --latest` (newest within that file) and to produce a
    /// useful error message when `--file <path> <id>` targets an id
    /// that's not in the file.
    pub fn list_sessions_in_file(&self, path: &Path) -> Result<Vec<SessionMeta>> {
        if !path.is_file() {
            return Err(anyhow!("--file: {} is not a readable file", path.display()));
        }
        scan_file(path)
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

// ---------------------------------------------------------------------
// Index diagnostics + rebuild (sessions-index.json).
//
// Claude Code writes a `sessions-index.json` summary file in each
// encoded project dir (`~/.claude/projects/<encoded-cwd>/`). The picker
// for `/resume` reads it to show session lists. The file is known to go
// stale relative to the actual `.jsonl` content (upstream issue #25032
// and siblings). This block provides:
//
//   - `detect_staleness`  — compare index mtime to newest jsonl mtime
//   - `build_index_for_project` — reconstruct a fresh SessionIndex from jsonls
//   - `write_index_atomic` — tempfile+rename write with optional dated backup
//
// All three share the `scan_file` / `walk` plumbing above. The adapter
// deliberately does NOT rebuild on `load()` calls — the write path is a
// side effect a user explicitly asks for via `pconv rebuild-index`.
// ---------------------------------------------------------------------

/// One entry in a Claude Code `sessions-index.json`. Field names match
/// the on-disk JSON (camelCase) via serde rename. Fields that are
/// sometimes absent upstream (summary, customTitle) are `Option` so the
/// reader doesn't error on older or fresh files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub session_id: String,
    pub full_path: String,
    /// Milliseconds since UNIX epoch. Upstream uses a JS Date.now()
    /// value, which is `u64` for any realistic date.
    pub file_mtime: u64,
    #[serde(default)]
    pub first_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub message_count: usize,
    /// ISO 8601 timestamp of the first message (or empty if unknown).
    #[serde(default)]
    pub created: String,
    /// ISO 8601 timestamp of the last message (or empty if unknown).
    #[serde(default)]
    pub modified: String,
    /// Always empty in our reconstruction — the adapter doesn't resolve
    /// a git branch without re-running `git` against the project, which
    /// isn't always safe (the project dir may not exist on this host).
    /// Upstream populates this; we leave it blank.
    #[serde(default)]
    pub git_branch: String,
    /// The project's absolute cwd, taken from the first record in the
    /// first jsonl. Best-effort — the encoded dir name is lossy.
    #[serde(default)]
    pub project_path: String,
    #[serde(default)]
    pub is_sidechain: bool,
}

/// Top-level `sessions-index.json` document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionIndex {
    /// Observed upstream value is 1. Preserved as-is on rebuild.
    pub version: u32,
    pub entries: Vec<IndexEntry>,
    /// Optional — upstream includes this for the encoded dir's nominal
    /// project path. We populate it from the first scanned jsonl's cwd.
    #[serde(rename = "originalPath", skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
}

/// A project that looks stale by the `detect_staleness` rule. The
/// picker-facing summary fields (`newest_session_id`, `newest_jsonl`)
/// are what the caller wants to show or dump.
#[derive(Debug, Clone)]
pub struct StaleReport {
    pub project_dir: PathBuf,
    /// The mtime observed on `sessions-index.json`, if the file exists.
    pub index_mtime: Option<SystemTime>,
    /// The mtime of the newest non-subagent `.jsonl` in the project dir.
    pub newest_jsonl_mtime: SystemTime,
    pub newest_jsonl: PathBuf,
    /// `newest_jsonl_mtime - index_mtime` in whole hours. `i64::MAX` if
    /// the index is missing entirely.
    pub lag_hours: i64,
    /// True when the index file is absent (distinct from "old").
    pub missing: bool,
    /// First sessionId found in the newest jsonl. Useful to surface in
    /// the diagnostic table so the user can `claude -r <id>` directly.
    pub newest_session_id: Option<String>,
    /// Bytes on disk for the newest jsonl. Orientation only — massive
    /// files may be too big to practically resume.
    pub newest_jsonl_size_bytes: u64,
}

/// List the top-level project directories under `~/.claude/projects/`.
/// One per encoded cwd. Callers iterate these to scan for staleness or
/// rebuild.
pub fn list_project_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let rd = fs::read_dir(root).with_context(|| format!("read_dir {}", root.display()))?;
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Return the set of non-subagent `.jsonl` files directly inside a
/// project dir. The rebuild only tracks these — nested session dirs
/// (`<uuid>/subagents/*`) and old-style `agent-*.jsonl` at the root are
/// excluded per the same rules `list()` uses.
fn project_top_level_jsonls(project_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let rd = match fs::read_dir(project_dir) {
        Ok(r) => r,
        Err(_) => return Ok(Vec::new()),
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if is_subagent_file(&p) {
            continue;
        }
        out.push(p);
    }
    out.sort();
    Ok(out)
}

/// Compare the `sessions-index.json` mtime in `project_dir` against the
/// newest jsonl in the same dir. Returns `Some(StaleReport)` when the
/// index is missing OR lags by more than `threshold_hours`. Returns
/// `None` for fresh projects and for empty dirs (no jsonls = nothing to
/// be stale about).
pub fn detect_staleness(project_dir: &Path, threshold_hours: i64) -> Result<Option<StaleReport>> {
    let jsonls = project_top_level_jsonls(project_dir)?;
    if jsonls.is_empty() {
        return Ok(None);
    }

    // Find the newest jsonl by mtime.
    let mut newest: Option<(PathBuf, SystemTime, u64)> = None;
    for p in &jsonls {
        let Ok(md) = fs::metadata(p) else { continue };
        let Ok(mt) = md.modified() else { continue };
        let size = md.len();
        match &newest {
            Some((_, cur, _)) if *cur >= mt => {}
            _ => newest = Some((p.clone(), mt, size)),
        }
    }
    let (newest_path, newest_mtime, newest_size) = match newest {
        Some(t) => t,
        None => return Ok(None),
    };

    let index_path = project_dir.join("sessions-index.json");
    let index_mtime = fs::metadata(&index_path).and_then(|m| m.modified()).ok();

    let (lag_hours, missing) = match index_mtime {
        Some(idx) => {
            let lag = newest_mtime
                .duration_since(idx)
                .ok()
                .map(|d| (d.as_secs() / 3600) as i64)
                .unwrap_or(0);
            (lag, false)
        }
        None => (i64::MAX, true),
    };

    if !missing && lag_hours <= threshold_hours {
        return Ok(None);
    }

    // Pluck a sessionId from the newest jsonl so callers can point the
    // user at `claude -r <id>` directly. scan_file returns per-sessionId
    // entries; pick the first (order stable across runs).
    let newest_session_id = scan_file(&newest_path)
        .ok()
        .and_then(|metas| metas.into_iter().next().map(|m| m.id));

    Ok(Some(StaleReport {
        project_dir: project_dir.to_path_buf(),
        index_mtime,
        newest_jsonl_mtime: newest_mtime,
        newest_jsonl: newest_path,
        lag_hours,
        missing,
        newest_session_id,
        newest_jsonl_size_bytes: newest_size,
    }))
}

/// Reconstruct a `SessionIndex` by scanning every top-level jsonl in
/// the project dir. One `IndexEntry` per distinct sessionId observed
/// across the files.
pub fn build_index_for_project(project_dir: &Path) -> Result<SessionIndex> {
    let jsonls = project_top_level_jsonls(project_dir)?;
    let mut entries: Vec<IndexEntry> = Vec::new();
    let mut first_cwd: Option<String> = None;

    for path in &jsonls {
        let metas = match scan_file(path) {
            Ok(v) => v,
            Err(e) => {
                // A single bad jsonl shouldn't kill the rebuild — warn
                // and carry on. Upstream's picker is just as resilient.
                eprintln!("pconv: warning: scan failed for {}: {e:#}", path.display());
                continue;
            }
        };
        let file_mtime = fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        for meta in metas {
            if first_cwd.is_none() {
                first_cwd = meta.cwd.as_ref().map(|p| p.display().to_string());
            }
            let project_path = meta
                .cwd
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            entries.push(IndexEntry {
                session_id: meta.id,
                full_path: path.display().to_string(),
                file_mtime,
                first_prompt: meta.title.unwrap_or_default(),
                custom_title: None,
                summary: None,
                message_count: meta.message_count,
                created: meta
                    .started_at
                    .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                    .unwrap_or_default(),
                modified: meta
                    .updated_at
                    .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                    .unwrap_or_default(),
                git_branch: String::new(),
                project_path,
                is_sidechain: false,
            });
        }
    }

    // Deterministic ordering: newest modified first. Upstream doesn't
    // guarantee order but this is what the picker effectively presents.
    entries.sort_by(|a, b| b.modified.cmp(&a.modified));

    Ok(SessionIndex {
        version: 1,
        entries,
        original_path: first_cwd,
    })
}

/// Atomic write of `sessions-index.json`. Writes to `<path>.tmp` then
/// renames over the target. Creates a `<filename>.bak-YYYY-MM-DD`
/// alongside first unless `backup` is false. Safe under crash —
/// either the new file is in place or the old one is, never both
/// partially written (modulo the filesystem's rename atomicity).
pub fn write_index_atomic(
    index_path: &Path,
    idx: &SessionIndex,
    backup: bool,
) -> Result<Option<PathBuf>> {
    let mut backup_path: Option<PathBuf> = None;
    if backup && index_path.exists() {
        let date = Utc::now().format("%Y-%m-%d");
        let name = index_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("sessions-index.json");
        let bak = index_path.with_file_name(format!("{name}.bak-{date}"));
        fs::copy(index_path, &bak).with_context(|| format!("backup {}", bak.display()))?;
        backup_path = Some(bak);
    }

    // Write to a sibling temp file then rename. Avoid `.tmp` as an
    // extension suffix directly — some filesystems interpret double
    // extensions oddly. Use a hidden dotfile prefix instead.
    let parent = index_path
        .parent()
        .ok_or_else(|| anyhow!("index path has no parent: {}", index_path.display()))?;
    let name = index_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sessions-index.json");
    let tmp_path = parent.join(format!(".{name}.tmp"));

    let json = serde_json::to_string_pretty(idx).context("serialize SessionIndex")?;
    fs::write(&tmp_path, json.as_bytes())
        .with_context(|| format!("write tmp {}", tmp_path.display()))?;
    fs::rename(&tmp_path, index_path)
        .with_context(|| format!("rename {} → {}", tmp_path.display(), index_path.display()))?;

    Ok(backup_path)
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
