//! On-disk cache of `list()` scan results.
//!
//! A full walk of `~/.claude/projects/` with line-scans per JSONL takes
//! ~50 s on a ~5 GB / 2607-file corpus. Most of that work is repeated:
//! for unchanged files, every run re-parses the same bytes. This module
//! memoizes per-file scan output, keyed by the tuple that guarantees
//! validity: `(path, mtime_ns, size)`.
//!
//! Design rules the cache respects:
//! - **Convenience, not source of truth.** A missing/corrupt/stale
//!   cache falls back to a full scan. portaconv's extract-on-demand
//!   philosophy still holds — the cache is just memoized work.
//! - **Atomic write.** Serialize to a temp file, then `rename()`.
//! - **Per-adapter namespace.** Future opencode / Cursor adapters get
//!   their own cache file under the same root; no sharing.
//! - **JSON, not binary.** Debuggability beats byte-shaving for a
//!   file that's ~100 KB at worst.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::SessionMeta;

const CACHE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct FileCacheEntry {
    pub mtime_ns: i128,
    pub size: u64,
    pub sessions: Vec<CachedSession>,
}

/// Cached per-session data. Mirrors `SessionMeta` but omits
/// `source_path` (the cache is keyed by path, so storing it per-session
/// would be redundant).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSession {
    pub id: String,
    pub title: Option<String>,
    pub cwd: Option<PathBuf>,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub message_count: usize,
}

impl CachedSession {
    pub fn into_session_meta(self, source_path: PathBuf) -> SessionMeta {
        SessionMeta {
            id: self.id,
            tool: "claude-code",
            title: self.title,
            cwd: self.cwd,
            started_at: self.started_at,
            updated_at: self.updated_at,
            message_count: self.message_count,
            source_path,
        }
    }
}

impl From<&SessionMeta> for CachedSession {
    fn from(m: &SessionMeta) -> Self {
        Self {
            id: m.id.clone(),
            title: m.title.clone(),
            cwd: m.cwd.clone(),
            started_at: m.started_at,
            updated_at: m.updated_at,
            message_count: m.message_count,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListCache {
    pub version: u32,
    /// Path → entry. Using BTreeMap for deterministic on-disk order,
    /// which keeps diffs of `cat list-cache.json` readable during
    /// debugging.
    pub entries: BTreeMap<String, FileCacheEntry>,
}

impl Default for ListCache {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

pub fn cache_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PORTACONV_CACHE_ROOT") {
        return Some(PathBuf::from(p).join("claude-code").join("list-cache.json"));
    }
    // state_dir is Linux-native ($XDG_STATE_HOME). data_local_dir is
    // the closest analog on macOS / Windows where XDG_STATE doesn't
    // exist. Either way the cache is machine-local, never synced.
    let root = dirs::state_dir().or_else(dirs::data_local_dir)?;
    Some(
        root.join("portaconv")
            .join("claude-code")
            .join("list-cache.json"),
    )
}

pub fn load_or_empty() -> ListCache {
    let Some(path) = cache_path() else {
        return ListCache::default();
    };
    let Ok(body) = std::fs::read_to_string(&path) else {
        return ListCache::default();
    };
    match serde_json::from_str::<ListCache>(&body) {
        Ok(c) if c.version == CACHE_VERSION => c,
        _ => {
            // Version mismatch or corrupt → start fresh. Don't error
            // out; the cache is convenience, not truth.
            ListCache::default()
        }
    }
}

pub fn save(cache: &ListCache) -> Result<()> {
    let Some(path) = cache_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create cache dir {}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(cache)?;
    // Atomic write: temp file + rename. Prevents a crashed write from
    // leaving a truncated JSON file that we'd interpret as corrupt on
    // the next run. On WSL/Linux the rename is atomic within a
    // filesystem; cross-filesystem we accept the same best-effort
    // guarantees rename gives.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Try to satisfy a scan from cache. Returns `Some(sessions)` only
/// when the cache entry's `(mtime_ns, size)` matches the file's
/// current metadata; any drift invalidates.
pub fn lookup(cache: &ListCache, path: &Path) -> Option<Vec<CachedSession>> {
    let key = path.to_str()?;
    let entry = cache.entries.get(key)?;
    let meta = std::fs::metadata(path).ok()?;
    let size = meta.len();
    let mtime_ns = mtime_ns_of(&meta)?;
    if entry.size == size && entry.mtime_ns == mtime_ns {
        Some(entry.sessions.clone())
    } else {
        None
    }
}

/// Record a fresh scan result. Stored under the path's string form.
pub fn record(cache: &mut ListCache, path: &Path, sessions: &[SessionMeta]) {
    let Some(key) = path.to_str() else {
        return;
    };
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let Some(mtime_ns) = mtime_ns_of(&meta) else {
        return;
    };
    let entry = FileCacheEntry {
        mtime_ns,
        size: meta.len(),
        sessions: sessions.iter().map(CachedSession::from).collect(),
    };
    cache.entries.insert(key.to_string(), entry);
}

/// Prune entries for files that no longer exist — otherwise the cache
/// grows unboundedly over time as users delete sessions.
pub fn prune_missing(cache: &mut ListCache) {
    cache
        .entries
        .retain(|path, _| std::path::Path::new(path).is_file());
}

fn mtime_ns_of(meta: &std::fs::Metadata) -> Option<i128> {
    let t = meta.modified().ok()?;
    let d = t.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(d.as_nanos() as i128)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpfile(content: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f
    }

    #[test]
    fn lookup_miss_on_nonexistent_key() {
        let cache = ListCache::default();
        let p = std::path::Path::new("/nope/does-not-exist");
        assert!(lookup(&cache, p).is_none());
    }

    #[test]
    fn record_then_lookup_hits() {
        let f = tmpfile(b"{}\n");
        let mut cache = ListCache::default();
        let meta = SessionMeta {
            id: "abc".into(),
            tool: "claude-code",
            title: None,
            cwd: None,
            started_at: None,
            updated_at: None,
            message_count: 1,
            source_path: f.path().to_path_buf(),
        };
        record(&mut cache, f.path(), std::slice::from_ref(&meta));
        let hit = lookup(&cache, f.path()).expect("hit");
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].id, "abc");
    }

    #[test]
    fn size_change_invalidates() {
        let f = tmpfile(b"{}\n");
        let mut cache = ListCache::default();
        let meta = SessionMeta {
            id: "abc".into(),
            tool: "claude-code",
            title: None,
            cwd: None,
            started_at: None,
            updated_at: None,
            message_count: 1,
            source_path: f.path().to_path_buf(),
        };
        record(&mut cache, f.path(), std::slice::from_ref(&meta));
        // Re-open, append: size changes → cache must miss.
        std::fs::write(f.path(), b"{}\n{}\n").unwrap();
        assert!(lookup(&cache, f.path()).is_none());
    }

    #[test]
    fn prune_drops_missing_files() {
        let f = tmpfile(b"{}\n");
        let mut cache = ListCache::default();
        let meta = SessionMeta {
            id: "abc".into(),
            tool: "claude-code",
            title: None,
            cwd: None,
            started_at: None,
            updated_at: None,
            message_count: 1,
            source_path: f.path().to_path_buf(),
        };
        record(&mut cache, f.path(), std::slice::from_ref(&meta));
        // Inject a ghost entry directly — record() correctly refuses
        // to store unreadable paths, so we bypass it here to set up
        // the prune target.
        cache.entries.insert(
            "/tmp/definitely-not-a-real-cache-ghost-path.jsonl".into(),
            FileCacheEntry {
                mtime_ns: 0,
                size: 0,
                sessions: vec![],
            },
        );
        assert_eq!(cache.entries.len(), 2);
        prune_missing(&mut cache);
        assert_eq!(cache.entries.len(), 1);
    }
}
