//! Adapter trait — one implementation per agent CLI whose storage we
//! normalize. v0.1 ships only the Claude Code adapter; OpenCode / Cursor
//! / Aider / continue.dev are separate PRs that land after this trait
//! has survived contact with reality.

use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::Conversation;

pub mod claude_code;

pub use claude_code::ClaudeCode;

/// Lightweight session entry returned by `list()`. Cheap to produce —
/// the adapter does not parse the full message stream for listings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub tool: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    pub message_count: usize,
    /// Path to the on-disk backing file. Kept so `load(id)` can find it
    /// without re-walking the corpus.
    pub source_path: PathBuf,
}

/// Scope for `list()` calls. v0.1 doesn't wire portagenty integration —
/// the field is present so the trait shape is stable when the pa shim
/// lands in a later PR.
#[derive(Debug, Default, Clone)]
pub struct WorkspaceScope {
    pub project_paths: Vec<PathBuf>,
    pub since: Option<DateTime<Utc>>,
}

pub trait ConvoAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn detect(&self) -> bool;
    fn list(&self, scope: Option<&WorkspaceScope>) -> Result<Vec<SessionMeta>>;
    fn load(&self, id: &str) -> Result<Conversation>;
}

/// Collapse duplicate `SessionMeta` entries with the same id, keeping the
/// one with the highest `message_count` (tie-break: most recent
/// `updated_at`). Same sessionId shows up multiple times when a project
/// has been launched from both WSL and Windows — the two encoded-dir
/// buckets each carry a copy. Callers that want the raw multi-entry
/// view skip this step.
pub fn dedup_sessions(mut metas: Vec<SessionMeta>) -> Vec<SessionMeta> {
    use std::collections::HashMap;
    let mut best: HashMap<String, SessionMeta> = HashMap::new();
    for m in metas.drain(..) {
        match best.get(&m.id) {
            Some(existing)
                if (existing.message_count, existing.updated_at)
                    >= (m.message_count, m.updated_at) => {}
            _ => {
                best.insert(m.id.clone(), m);
            }
        }
    }
    let mut out: Vec<SessionMeta> = best.into_values().collect();
    out.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
    out
}
