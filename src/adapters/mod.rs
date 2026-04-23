//! Adapter trait — one implementation per agent CLI whose storage we
//! normalize. v0.1 ships only the Claude Code adapter; OpenCode / Cursor
//! / Aider / continue.dev are separate PRs that land after this trait
//! has survived contact with reality.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::model::Conversation;

pub mod cache;
pub mod claude_code;

pub use claude_code::{
    build_index_for_project, detect_staleness, list_project_dirs, write_index_atomic, ClaudeCode,
    IndexEntry, SessionIndex, StaleReport,
};

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

/// Sort key for `sort_sessions`. Default across the app is `Updated`
/// descending — matches what humans expect when glancing at a "recent
/// sessions" list.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SortKey {
    Updated,
    Started,
    Msgs,
    Title,
    Id,
}

pub fn sort_sessions(metas: &mut [SessionMeta], key: SortKey, reverse: bool) {
    // Stable sort so equal keys keep their relative order from the
    // caller (typically still walk-order, which preserves predictability).
    match key {
        SortKey::Updated => metas.sort_by_key(|m| m.updated_at),
        SortKey::Started => metas.sort_by_key(|m| m.started_at),
        SortKey::Msgs => metas.sort_by_key(|m| m.message_count),
        SortKey::Title => metas.sort_by(|a, b| a.title.as_deref().cmp(&b.title.as_deref())),
        SortKey::Id => metas.sort_by(|a, b| a.id.cmp(&b.id)),
    }
    // Default semantics:
    //   updated/started/msgs → newest/biggest first (reverse-of-ascending)
    //   title/id             → alphabetic ascending
    // `reverse` flips whichever default applies.
    let default_descending = matches!(key, SortKey::Updated | SortKey::Started | SortKey::Msgs);
    if default_descending ^ reverse {
        metas.reverse();
    }
}

/// Cap a session list after filtering/sorting. `0` means no cap.
pub fn limit_sessions(metas: Vec<SessionMeta>, limit: usize) -> Vec<SessionMeta> {
    if limit == 0 || metas.len() <= limit {
        metas
    } else {
        metas.into_iter().take(limit).collect()
    }
}

/// Case-insensitive substring match on title + cwd. Intentionally NOT
/// full-content search — that's a separate verb we'd add later with
/// proper streaming and no index. This is the cheap discovery filter
/// humans and agents reach for first: "the react refactor one",
/// "anything under /mnt/c/work/api".
pub fn grep_sessions(metas: Vec<SessionMeta>, needle: &str) -> Vec<SessionMeta> {
    if needle.is_empty() {
        return metas;
    }
    let n = needle.to_lowercase();
    metas
        .into_iter()
        .filter(|m| {
            let title_hit = m
                .title
                .as_deref()
                .map(|t| t.to_lowercase().contains(&n))
                .unwrap_or(false);
            let cwd_hit = m
                .cwd
                .as_ref()
                .and_then(|p| p.to_str())
                .map(|p| p.to_lowercase().contains(&n))
                .unwrap_or(false);
            title_hit || cwd_hit
        })
        .collect()
}

/// Parse a `--since` value into an absolute `DateTime<Utc>` threshold.
///
/// Accepted shapes, tried in order:
///   - relative:   `30m`, `6h`, `2d`, `4w`
///   - RFC 3339:   `2026-04-01T12:00:00Z`
///   - ISO date:   `2026-04-01` (interpreted as 00:00 UTC on that day)
///
/// Zero new deps — `chrono::Duration` + `NaiveDate::parse_from_str`
/// cover everything.
pub fn parse_since(s: &str) -> Result<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return Err(anyhow!("--since: empty value"));
    }

    // Relative form: trailing m / h / d / w, rest is a positive integer.
    if let Some(last) = s.chars().last() {
        if matches!(last, 'm' | 'h' | 'd' | 'w') {
            let body = &s[..s.len() - 1];
            if let Ok(n) = body.parse::<i64>() {
                if n <= 0 {
                    return Err(anyhow!("--since: duration must be positive (got {s})"));
                }
                let delta = match last {
                    'm' => chrono::Duration::minutes(n),
                    'h' => chrono::Duration::hours(n),
                    'd' => chrono::Duration::days(n),
                    'w' => chrono::Duration::weeks(n),
                    _ => unreachable!(),
                };
                return Ok(Utc::now() - delta);
            }
        }
    }

    // Absolute: RFC 3339 first (carries timezone), then date-only.
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = d
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow!("--since: invalid date {s}"))?;
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc));
    }

    Err(anyhow!(
        "--since: can't parse {s:?} — expected a duration like `2d` or a date like `2026-04-01`"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str, msgs: usize, updated_iso: Option<&str>) -> SessionMeta {
        SessionMeta {
            id: id.into(),
            tool: "claude-code",
            title: None,
            cwd: None,
            started_at: None,
            updated_at: updated_iso
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc)),
            message_count: msgs,
            source_path: std::path::PathBuf::new(),
        }
    }

    #[test]
    fn parse_since_relative_shapes() {
        let now = Utc::now();
        let two_days = parse_since("2d").unwrap();
        assert!(now - two_days > chrono::Duration::hours(47));
        assert!(now - two_days < chrono::Duration::hours(49));

        assert!(parse_since("30m").is_ok());
        assert!(parse_since("6h").is_ok());
        assert!(parse_since("4w").is_ok());
    }

    #[test]
    fn parse_since_absolute_date() {
        let got = parse_since("2026-04-01").unwrap();
        assert_eq!(got.format("%Y-%m-%d").to_string(), "2026-04-01");
        assert_eq!(got.format("%H:%M:%S").to_string(), "00:00:00");
    }

    #[test]
    fn parse_since_rfc3339() {
        assert!(parse_since("2026-04-01T12:00:00Z").is_ok());
    }

    #[test]
    fn parse_since_rejects_garbage() {
        assert!(parse_since("").is_err());
        assert!(parse_since("banana").is_err());
        assert!(parse_since("0d").is_err());
        assert!(parse_since("-1d").is_err());
    }

    #[test]
    fn sort_by_msgs_desc_by_default() {
        let mut v = vec![meta("a", 1, None), meta("b", 10, None), meta("c", 5, None)];
        sort_sessions(&mut v, SortKey::Msgs, false);
        assert_eq!(v[0].id, "b");
        assert_eq!(v[1].id, "c");
        assert_eq!(v[2].id, "a");
    }

    #[test]
    fn sort_reverse_flips() {
        let mut v = vec![meta("a", 1, None), meta("b", 10, None), meta("c", 5, None)];
        sort_sessions(&mut v, SortKey::Msgs, true);
        assert_eq!(v[0].id, "a");
    }

    #[test]
    fn sort_title_ascending_by_default() {
        let mut v = vec![
            SessionMeta {
                title: Some("charlie".into()),
                ..meta("1", 0, None)
            },
            SessionMeta {
                title: Some("alpha".into()),
                ..meta("2", 0, None)
            },
            SessionMeta {
                title: Some("bravo".into()),
                ..meta("3", 0, None)
            },
        ];
        sort_sessions(&mut v, SortKey::Title, false);
        assert_eq!(v[0].title.as_deref(), Some("alpha"));
        assert_eq!(v[2].title.as_deref(), Some("charlie"));
    }

    #[test]
    fn limit_caps_output() {
        let v = vec![meta("a", 0, None), meta("b", 0, None), meta("c", 0, None)];
        assert_eq!(limit_sessions(v.clone(), 0).len(), 3); // 0 = unlimited
        assert_eq!(limit_sessions(v.clone(), 2).len(), 2);
        assert_eq!(limit_sessions(v, 99).len(), 3);
    }

    #[test]
    fn grep_matches_title_or_cwd_case_insensitive() {
        let v = vec![
            SessionMeta {
                title: Some("React refactor".into()),
                cwd: Some("/work/api".into()),
                ..meta("a", 0, None)
            },
            SessionMeta {
                title: Some("misc notes".into()),
                cwd: Some("/work/frontend".into()),
                ..meta("b", 0, None)
            },
            SessionMeta {
                title: None,
                cwd: Some("/home/x".into()),
                ..meta("c", 0, None)
            },
        ];
        let hit = grep_sessions(v.clone(), "react");
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].id, "a");

        let hit = grep_sessions(v.clone(), "work");
        assert_eq!(hit.len(), 2); // matched via cwd

        let hit = grep_sessions(v.clone(), "");
        assert_eq!(hit.len(), 3); // empty needle = passthrough
    }
}
