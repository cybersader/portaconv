//! CLI argument parsing. v0.1 surface: `list`, `dump`, `mcp serve`.
//!
//! Markdown rendering + --rewrite transforms are stubbed here but land
//! as a follow-up commit — this file's scope is the argument shape and
//! the `list` / `dump --format json` execution paths.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use crate::adapters::{
    build_index_for_project, claude_code, dedup_sessions, detect_staleness, grep_sessions,
    limit_sessions, list_project_dirs, parse_since, sort_sessions, write_index_atomic, ClaudeCode,
    ConvoAdapter, SortKey, StaleReport, WorkspaceScope,
};
use crate::model::Conversation;
use crate::render::{render_markdown, MarkdownOptions};
use crate::transform::{apply_path_rewrite, PathRewrite};

#[derive(Parser, Debug)]
#[command(
    name = "pconv",
    version,
    about = "Terminal-native conversation extractor for agent CLIs"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List conversations discoverable on this machine.
    List(ListArgs),
    /// Dump one session to stdout.
    Dump(DumpArgs),
    /// Diagnose stale `sessions-index.json` files under
    /// `~/.claude/projects/`. Read-only. Pass `--dump-stale` to
    /// additionally emit paste-ready markdown for the newest session in
    /// each stale project so you can recover into a fresh `claude`
    /// session without waiting on the picker.
    Doctor(DoctorArgs),
    /// Rewrite `sessions-index.json` from the actual `.jsonl` content.
    /// The `/resume` picker reads the index; rebuilding restores it to
    /// match reality after ungraceful shutdowns (WSL kill, suspend,
    /// etc.) have left it behind. Atomic write; keeps a dated `.bak`
    /// unless `--no-backup`.
    RebuildIndex(RebuildIndexArgs),
    /// MCP-related subcommands.
    Mcp {
        #[command(subcommand)]
        sub: McpCommand,
    },
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Output format.
    #[arg(long, value_enum, default_value_t = ListFormat::Table)]
    pub format: ListFormat,
    /// Hide sessions with fewer than this many messages.
    #[arg(long, default_value_t = 1)]
    pub min_messages: usize,
    /// Show every physical-file entry, including WSL- and Windows-encoded
    /// duplicates that dedup would normally collapse.
    #[arg(long)]
    pub show_duplicates: bool,
    /// Scope by a portagenty workspace TOML. Accepts an explicit path or
    /// `auto` to walk up from cwd for the nearest `*.portagenty.toml`.
    #[arg(long)]
    pub workspace_toml: Option<String>,
    /// Bypass the per-file list cache. Use for debugging or after
    /// suspecting the cache is stale (it normally invalidates per-file
    /// by mtime+size, but this is the manual override).
    #[arg(long)]
    pub no_cache: bool,
    /// Print a trailing line with cache hit/miss counts and the
    /// on-disk cache path. Useful for benchmarking.
    #[arg(long)]
    pub cache_stats: bool,
    /// Only list sessions updated after this point. Accepts a relative
    /// duration (`2d`, `6h`, `30m`, `4w`) or an absolute date
    /// (`2026-04-01`, `2026-04-01T12:00:00Z`).
    #[arg(long)]
    pub since: Option<String>,
    /// Sort column. Defaults: updated/started/msgs descend newest-first,
    /// title/id ascend alphabetic. Use --reverse to flip.
    #[arg(long, value_enum, default_value_t = SortKeyFlag::Updated)]
    pub sort: SortKeyFlag,
    /// Flip the default sort direction for whichever column is active.
    #[arg(long)]
    pub reverse: bool,
    /// Cap output at N entries after filtering and sorting. 0 = no cap.
    #[arg(long, default_value_t = 0)]
    pub limit: usize,
    /// Case-insensitive substring match on title + cwd. NOT full-content
    /// search — use this for "the react refactor one" / "anything in
    /// /work/api". Full-content search is tracked separately.
    #[arg(long)]
    pub grep: Option<String>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum SortKeyFlag {
    Updated,
    Started,
    Msgs,
    Title,
    Id,
}

impl From<SortKeyFlag> for SortKey {
    fn from(f: SortKeyFlag) -> Self {
        match f {
            SortKeyFlag::Updated => SortKey::Updated,
            SortKeyFlag::Started => SortKey::Started,
            SortKeyFlag::Msgs => SortKey::Msgs,
            SortKeyFlag::Title => SortKey::Title,
            SortKeyFlag::Id => SortKey::Id,
        }
    }
}

#[derive(clap::Args, Debug)]
pub struct DumpArgs {
    /// Session ID (UUID). Optional when `--latest` is set.
    pub session_id: Option<String>,
    /// Resolve to the most recent session discoverable (after workspace
    /// scope + dedup). Composes with `--workspace-toml` for the
    /// "dump the most recent session in this portagenty workspace"
    /// one-liner.
    #[arg(long)]
    pub latest: bool,
    /// Scope latest-session lookup by a portagenty workspace TOML.
    /// Accepts an explicit path or `auto` to walk up from cwd.
    #[arg(long)]
    pub workspace_toml: Option<String>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = DumpFormat::Markdown)]
    pub format: DumpFormat,
    /// Include assistant `thinking` blocks in markdown output.
    #[arg(long)]
    pub include_thinking: bool,
    /// Emit full tool-result bodies instead of the short preview.
    #[arg(long)]
    pub full_results: bool,
    /// Append conversation-level system events as a trailing section.
    #[arg(long)]
    pub include_system_events: bool,
    /// Rewrite OS-specific absolute paths inside conversation content.
    /// Touches Text blocks + tool-call inputs + tool results. Leaves
    /// `cwd` metadata alone.
    #[arg(long, value_enum)]
    pub rewrite: Option<PathRewriteFlag>,
    /// Keep only the last N messages. The output records how many
    /// earlier messages were dropped (markdown header + extensions.truncated
    /// in JSON). Pair with `--include-thinking` / `--full-results` to
    /// trade off depth vs length.
    #[arg(long)]
    pub tail: Option<usize>,
    /// Load from this specific JSONL, bypassing the corpus walk. Escape
    /// hatch for when a session id lives in multiple files (WSL- and
    /// Windows-encoded project dirs after cross-OS work) and the
    /// automatic pick isn't the one you want. Discover paths via
    /// `pconv list --show-duplicates --format json` and the
    /// `source_path` field.
    #[arg(long)]
    pub file: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum PathRewriteFlag {
    /// `/mnt/c/…` → `C:\…`
    WslToWin,
    /// `C:\…` → `/mnt/c/…`
    WinToWsl,
    /// Replace any absolute path with the literal `<path>` placeholder.
    Strip,
}

impl From<PathRewriteFlag> for PathRewrite {
    fn from(f: PathRewriteFlag) -> Self {
        match f {
            PathRewriteFlag::WslToWin => PathRewrite::WslToWin,
            PathRewriteFlag::WinToWsl => PathRewrite::WinToWsl,
            PathRewriteFlag::Strip => PathRewrite::Strip,
        }
    }
}

#[derive(clap::Args, Debug)]
pub struct DoctorArgs {
    /// Scope to one project dir (absolute path to the encoded dir under
    /// `~/.claude/projects/`). Omit to scan all.
    #[arg(long)]
    pub project: Option<PathBuf>,
    /// Consider a project stale when its `sessions-index.json` lags the
    /// newest `.jsonl` by more than this many hours. Missing index is
    /// always stale. Default: 24 — most sessions idle longer than the
    /// write cadence is measured in minutes, so anything beyond a day
    /// is drift rather than race.
    #[arg(long, default_value_t = 24)]
    pub stale_threshold_hours: i64,
    /// Also emit paste-ready markdown for the newest session in each
    /// stale project, separated by `---` dividers. Paste into a fresh
    /// `claude` session to recover context. Respects the renderer
    /// defaults (collapsed thinking, truncated tool results).
    #[arg(long)]
    pub dump_stale: bool,
    /// Output format.
    #[arg(long, value_enum, default_value_t = DoctorFormat::Table)]
    pub format: DoctorFormat,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum DoctorFormat {
    /// Human-readable summary table.
    Table,
    /// Machine-readable list of stale projects. Useful for scripting.
    Json,
}

#[derive(clap::Args, Debug)]
pub struct RebuildIndexArgs {
    /// Rebuild a single project's index. Mutually exclusive with
    /// `--all`.
    #[arg(long)]
    pub project: Option<PathBuf>,
    /// Rebuild every project's index under `~/.claude/projects/`.
    /// Combine with `--lag-threshold-hours` to skip fresh projects.
    #[arg(long, conflicts_with = "project")]
    pub all: bool,
    /// When `--all`, only rebuild projects whose index lags by more
    /// than this many hours (matches `doctor --stale-threshold-hours`
    /// semantics). Default 0 = rebuild every project unconditionally.
    #[arg(long, default_value_t = 0)]
    pub lag_threshold_hours: i64,
    /// Report what would change without writing. Exit 0 whether or not
    /// anything is stale.
    #[arg(long)]
    pub dry_run: bool,
    /// Skip the `.bak-YYYY-MM-DD` backup of the pre-rebuild index.
    /// Default: keep the backup so a bad rebuild is recoverable.
    #[arg(long)]
    pub no_backup: bool,
}

#[derive(Subcommand, Debug)]
pub enum McpCommand {
    /// Start the stdio MCP server (not implemented in v0.0.2).
    Serve,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ListFormat {
    Table,
    Json,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum DumpFormat {
    /// Paste-ready markdown (default) — User / Assistant blocks, tool
    /// calls as fenced JSON, tool results truncated by default.
    Markdown,
    /// Raw normalized model. Stable across adapters.
    Json,
}

/// Public re-export for the MCP module. Shares the same walk-up +
/// TOML-parse logic so the two surfaces behave identically.
pub fn build_workspace_scope_public(flag: Option<&str>) -> Result<WorkspaceScope> {
    build_workspace_scope(flag)
}

/// Resolve which session id `dump` should load. Either the explicit
/// positional arg or the most-recent session in the scope when
/// `--latest` is given. Errors if neither is usable.
fn resolve_dump_target(adapter: &ClaudeCode, args: &DumpArgs) -> Result<String> {
    if let Some(id) = args.session_id.as_deref() {
        if args.latest {
            return Err(anyhow!(
                "--latest and a positional session id are mutually exclusive — pick one"
            ));
        }
        return Ok(id.to_string());
    }
    if !args.latest {
        return Err(anyhow!(
            "pconv dump: give a session id, or pass --latest (optionally with --workspace-toml auto)"
        ));
    }
    let scope = build_workspace_scope(args.workspace_toml.as_deref())?;
    let metas = adapter
        .list(Some(&scope))
        .context("listing sessions to resolve --latest")?;
    let metas = dedup_sessions(metas);
    // adapter.list already sorts updated_at desc; after dedup the
    // first entry is the freshest surviving session.
    let pick = metas.into_iter().next().ok_or_else(|| {
        anyhow!(
            "no sessions found{}",
            if args.workspace_toml.is_some() {
                " in this workspace"
            } else {
                ""
            }
        )
    })?;
    Ok(pick.id)
}

/// Truncate a path-ish string to a visual width, keeping the useful
/// tail (projects are recognizable by their last segments, not their
/// `/home/cybersader/…` prefix).
fn truncate_middle(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    // "…" + last (width - 1) chars.
    let tail: String = s.chars().rev().take(width - 1).collect();
    let tail: String = tail.chars().rev().collect();
    format!("…{tail}")
}

/// Resolve a --workspace-toml flag into a `WorkspaceScope` with the
/// declared project paths populated. `None` → empty scope. Returns an
/// error on `auto` if no TOML is found by walking up from cwd, or if
/// the declared path doesn't exist or can't be parsed.
fn build_workspace_scope(flag: Option<&str>) -> Result<WorkspaceScope> {
    let Some(value) = flag else {
        return Ok(WorkspaceScope::default());
    };
    let toml_path = if value == "auto" {
        find_workspace_toml_upwards()?
            .ok_or_else(|| anyhow!("no *.portagenty.toml found by walking up from cwd"))?
    } else {
        std::path::PathBuf::from(value)
    };
    parse_workspace_toml(&toml_path)
}

fn find_workspace_toml_upwards() -> Result<Option<std::path::PathBuf>> {
    let mut dir = std::env::current_dir()?;
    loop {
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            // `*.portagenty.toml` (workspace file) — distinct from bare
            // `portagenty.toml` (per-project file).
            if name.ends_with(".portagenty.toml") && name != ".portagenty.toml" {
                return Ok(Some(p));
            }
        }
        if !dir.pop() {
            return Ok(None);
        }
    }
}

fn parse_workspace_toml(path: &std::path::Path) -> Result<WorkspaceScope> {
    // Minimal parse — we only care about two arrays:
    //   projects       : authoritative workspace project paths
    //   previous_paths : historical paths the workspace lived at before
    //                    (appended by portagenty when it re-registers a
    //                    moved folder). Bridges sessions tied to the
    //                    old on-disk encoded-dir key in
    //                    `~/.claude/projects/` without requiring any
    //                    pconv-side state.
    //
    // A full toml dep for two array reads is overkill — line-scan it.
    let body = std::fs::read_to_string(path)
        .with_context(|| format!("read workspace toml {}", path.display()))?;
    let base = path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let mut declared_projects: Option<Vec<std::path::PathBuf>> = None;
    let mut previous: Vec<std::path::PathBuf> = Vec::new();

    for line in body.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("previous_paths") {
            if let Some(items) = extract_toml_string_array(rest) {
                for item in items {
                    previous.push(expand_path(&item, &base));
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("projects") {
            if let Some(items) = extract_toml_string_array(rest) {
                declared_projects = Some(items.iter().map(|s| expand_path(s, &base)).collect());
            }
            continue;
        }
    }

    // `projects` wins authoritatively when declared; otherwise fall back
    // to the TOML's own directory (matches portagenty's convention).
    // `previous_paths` is purely additive — always appended — so moved
    // workspaces can reach their pre-move sessions without any
    // portagenty-side state lookup.
    let mut scope = WorkspaceScope::default();
    match declared_projects {
        Some(ps) => scope.project_paths.extend(ps),
        None => scope.project_paths.push(base.clone()),
    }
    scope.project_paths.extend(previous);
    Ok(scope)
}

/// Parse the RHS of a `key = [ "a", "b", … ]` assignment from a single
/// TOML line. Tolerates trailing comments and inline whitespace.
/// Returns `None` if the line isn't in the expected shape.
fn extract_toml_string_array(rest: &str) -> Option<Vec<String>> {
    let rhs = rest.trim_start().strip_prefix('=')?.trim();
    // Strip an inline `# comment` tail if present; safe because TOML
    // strings can't contain unescaped `#`-with-leading-space without
    // being quoted, and we only match quoted items below.
    let rhs = rhs.split_once(" #").map(|(a, _)| a).unwrap_or(rhs).trim();
    let inner = rhs.strip_prefix('[').and_then(|s| s.strip_suffix(']'))?;
    let items: Vec<String> = inner
        .split(',')
        .map(|s| {
            s.trim()
                .trim_matches(|c: char| c == '"' || c == '\'')
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    Some(items)
}

fn expand_path(input: &str, base: &std::path::Path) -> std::path::PathBuf {
    let with_home = if let Some(rest) = input.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            home.join(rest.trim_start_matches('/'))
        } else {
            std::path::PathBuf::from(input)
        }
    } else {
        std::path::PathBuf::from(input)
    };
    if with_home.is_absolute() {
        with_home
    } else {
        base.join(with_home)
    }
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::List(args) => run_list(args),
        Command::Dump(args) => run_dump(args),
        Command::Doctor(args) => run_doctor(args),
        Command::RebuildIndex(args) => run_rebuild_index(args),
        Command::Mcp {
            sub: McpCommand::Serve,
        } => crate::mcp::run_stdio_server(),
    }
}

fn run_list(args: ListArgs) -> Result<()> {
    let adapter = ClaudeCode;
    if !adapter.detect() {
        return Err(anyhow!(
            "Claude Code storage not found. Expected ~/.claude/projects/ (or set PORTACONV_CLAUDE_ROOT)"
        ));
    }
    crate::adapters::claude_code::set_no_cache(args.no_cache);
    let mut scope = build_workspace_scope(args.workspace_toml.as_deref())?;
    if let Some(s) = args.since.as_deref() {
        scope.since = Some(parse_since(s)?);
    }
    let mut sessions = adapter
        .list(Some(&scope))
        .context("listing Claude Code sessions")?;
    sessions.retain(|s| s.message_count >= args.min_messages);
    if !args.show_duplicates {
        sessions = dedup_sessions(sessions);
    }
    if let Some(needle) = args.grep.as_deref() {
        sessions = grep_sessions(sessions, needle);
    }
    sort_sessions(&mut sessions, args.sort.into(), args.reverse);
    sessions = limit_sessions(sessions, args.limit);

    let out = io::stdout();
    let mut out = out.lock();
    match args.format {
        ListFormat::Json => {
            serde_json::to_writer_pretty(&mut out, &sessions)?;
            writeln!(out)?;
        }
        ListFormat::Table => {
            // Columns: id (36) · msgs (5) · updated (16) · cwd (40) · title
            writeln!(
                out,
                "{:<36}  {:>5}  {:<16}  {:<40}  title",
                "session-id", "msgs", "updated", "cwd"
            )?;
            writeln!(out, "{}", "-".repeat(130))?;
            for s in &sessions {
                let updated = s
                    .updated_at
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "-".into());
                let cwd = s.cwd.as_ref().and_then(|p| p.to_str()).unwrap_or("-");
                let cwd_short: String = truncate_middle(cwd, 40);
                let title = s.title.as_deref().unwrap_or("(untitled)");
                let title_short: String = title.chars().take(60).collect();
                writeln!(
                    out,
                    "{:<36}  {:>5}  {:<16}  {:<40}  {}",
                    s.id, s.message_count, updated, cwd_short, title_short
                )?;
            }
            writeln!(out, "\n{} session(s)", sessions.len())?;
        }
    }
    if args.cache_stats {
        let stats = crate::adapters::claude_code::take_last_stats();
        writeln!(
            out,
            "cache: enabled={}, hits={}, misses={}, path={}",
            stats.cache_enabled,
            stats.cache_hits,
            stats.cache_misses,
            stats
                .cache_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(unavailable)".into())
        )?;
    }
    Ok(())
}

fn run_dump(args: DumpArgs) -> Result<()> {
    let adapter = ClaudeCode;
    if !adapter.detect() {
        return Err(anyhow!(
            "Claude Code storage not found. Expected ~/.claude/projects/ (or set PORTACONV_CLAUDE_ROOT)"
        ));
    }
    let mut conv = match args.file.as_deref() {
        Some(path) => load_from_file_dispatch(&adapter, path, &args)?,
        None => {
            let target_id = resolve_dump_target(&adapter, &args)?;
            adapter.load(&target_id)?
        }
    };
    if let Some(mode) = args.rewrite {
        apply_path_rewrite(&mut conv, mode.into());
    }
    if let Some(n) = args.tail {
        conv.apply_tail(n);
    }
    let out = io::stdout();
    let mut out = out.lock();
    match args.format {
        DumpFormat::Markdown => {
            let opts = MarkdownOptions {
                include_thinking: args.include_thinking,
                full_results: args.full_results,
                include_system_events: args.include_system_events,
                ..MarkdownOptions::default()
            };
            let md = render_markdown(&conv, &opts);
            out.write_all(md.as_bytes())?;
        }
        DumpFormat::Json => {
            serde_json::to_writer_pretty(&mut out, &conv)?;
            writeln!(out)?;
        }
    }
    Ok(())
}

fn run_doctor(args: DoctorArgs) -> Result<()> {
    let root = claude_code::projects_root()
        .ok_or_else(|| anyhow!("no home dir — cannot locate ~/.claude/projects/"))?;
    if !root.is_dir() {
        return Err(anyhow!(
            "Claude Code storage not found. Expected {} (or set PORTACONV_CLAUDE_ROOT)",
            root.display()
        ));
    }

    let project_dirs = match args.project.as_deref() {
        Some(p) => vec![p.to_path_buf()],
        None => list_project_dirs(&root)?,
    };

    let mut stale: Vec<StaleReport> = Vec::new();
    for dir in project_dirs {
        if let Some(rep) = detect_staleness(&dir, args.stale_threshold_hours)? {
            stale.push(rep);
        }
    }
    // Worst-first ordering — missing indexes sort to the top, then
    // largest lag. The top row is the one most likely to bite you.
    stale.sort_by(|a, b| b.lag_hours.cmp(&a.lag_hours));

    let out = io::stdout();
    let mut out = out.lock();
    match args.format {
        DoctorFormat::Json => {
            let payload: Vec<serde_json::Value> = stale
                .iter()
                .map(|r| stale_report_to_json(r))
                .collect();
            serde_json::to_writer_pretty(&mut out, &payload)?;
            writeln!(out)?;
        }
        DoctorFormat::Table => {
            writeln!(
                out,
                "{:<60}  {:>8}  {:<36}  {:>10}",
                "project", "lag", "newest session", "size"
            )?;
            writeln!(out, "{}", "-".repeat(124))?;
            for r in &stale {
                let lag = if r.missing {
                    "MISSING".to_string()
                } else if r.lag_hours >= 48 {
                    format!("{}d", r.lag_hours / 24)
                } else {
                    format!("{}h", r.lag_hours)
                };
                let name = r
                    .project_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                let name = truncate_middle(name, 60);
                let sid = r.newest_session_id.as_deref().unwrap_or("-");
                let size = format_size(r.newest_jsonl_size_bytes);
                writeln!(out, "{name:<60}  {lag:>8}  {sid:<36}  {size:>10}")?;
            }
            writeln!(out, "\n{} stale project(s)", stale.len())?;
        }
    }

    if args.dump_stale && !stale.is_empty() {
        let adapter = ClaudeCode;
        for (i, r) in stale.iter().enumerate() {
            if i == 0 {
                writeln!(out)?;
            } else {
                writeln!(out, "\n---\n")?;
            }
            writeln!(
                out,
                "<!-- portaconv doctor: stale project {} lag={}h -->",
                r.project_dir.display(),
                r.lag_hours
            )?;
            writeln!(out)?;
            let Some(sid) = r.newest_session_id.as_deref() else {
                writeln!(out, "(no session id; skipping dump)")?;
                continue;
            };
            match adapter.load_from_file(&r.newest_jsonl, sid) {
                Ok(conv) => {
                    let md = render_markdown(&conv, &MarkdownOptions::default());
                    out.write_all(md.as_bytes())?;
                }
                Err(e) => {
                    writeln!(out, "(load failed: {e:#})")?;
                }
            }
        }
    }

    Ok(())
}

fn stale_report_to_json(r: &StaleReport) -> serde_json::Value {
    use std::time::UNIX_EPOCH;
    serde_json::json!({
        "project_dir": r.project_dir.display().to_string(),
        "index_mtime_ms": r.index_mtime
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64),
        "newest_jsonl": r.newest_jsonl.display().to_string(),
        "newest_jsonl_mtime_ms": r.newest_jsonl_mtime
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as u64),
        "newest_jsonl_size_bytes": r.newest_jsonl_size_bytes,
        "lag_hours": if r.missing { serde_json::Value::Null } else { serde_json::json!(r.lag_hours) },
        "missing": r.missing,
        "newest_session_id": r.newest_session_id,
    })
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}K", bytes as f64 / KB as f64)
    } else {
        format!("{bytes}B")
    }
}

fn run_rebuild_index(args: RebuildIndexArgs) -> Result<()> {
    // Validate — exactly one of `--project` or `--all`.
    if args.project.is_none() && !args.all {
        return Err(anyhow!(
            "pconv rebuild-index: pass --project <path> or --all"
        ));
    }

    let root = claude_code::projects_root()
        .ok_or_else(|| anyhow!("no home dir — cannot locate ~/.claude/projects/"))?;
    if !root.is_dir() && args.all {
        return Err(anyhow!(
            "Claude Code storage not found. Expected {} (or set PORTACONV_CLAUDE_ROOT)",
            root.display()
        ));
    }

    let project_dirs = match args.project.as_deref() {
        Some(p) => {
            // User pointed at a specific dir — a missing path is a hard
            // error, not a warning. Otherwise typos like `--project ./x`
            // silently succeed with "0 rebuilt" which is worse than
            // failing loudly.
            if !p.is_dir() {
                return Err(anyhow!(
                    "project dir not found: {} (pass an existing encoded dir under ~/.claude/projects/)",
                    p.display()
                ));
            }
            vec![p.to_path_buf()]
        }
        None => list_project_dirs(&root)?,
    };

    let out = io::stdout();
    let mut out = out.lock();
    let prefix = if args.dry_run { "[DRY] " } else { "" };
    let mut rebuilt = 0usize;
    let mut skipped = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for dir in project_dirs {
        // Gate: when --all + threshold > 0, skip fresh projects.
        if args.all && args.lag_threshold_hours > 0 {
            let staleness = detect_staleness(&dir, args.lag_threshold_hours)?;
            if staleness.is_none() {
                skipped += 1;
                continue;
            }
        }

        let idx = build_index_for_project(&dir)?;
        let entry_count = idx.entries.len();
        let index_path = dir.join("sessions-index.json");
        let dir_name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("(unnamed)");

        if args.dry_run {
            writeln!(
                out,
                "{prefix}would rebuild {dir_name}: {entry_count} entries"
            )?;
            rebuilt += 1;
            continue;
        }

        match write_index_atomic(&index_path, &idx, !args.no_backup) {
            Ok(bak) => {
                match bak {
                    Some(bp) => writeln!(
                        out,
                        "rebuilt {dir_name}: {entry_count} entries (backup: {})",
                        bp.display()
                    )?,
                    None => writeln!(out, "rebuilt {dir_name}: {entry_count} entries")?,
                }
                rebuilt += 1;
            }
            Err(e) => {
                writeln!(out, "FAILED {dir_name}: {e:#}")?;
                errors.push(format!("{dir_name}: {e:#}"));
                skipped += 1;
            }
        }
    }

    writeln!(
        out,
        "\n{prefix}{rebuilt} project(s) {}, {skipped} skipped",
        if args.dry_run { "would be rebuilt" } else { "rebuilt" }
    )?;

    // In --all mode, per-project failures are visible-but-non-fatal so
    // one bad project doesn't kill the batch. But if EVERY rebuild
    // failed, that's worth surfacing as a process-level error.
    if rebuilt == 0 && !errors.is_empty() {
        return Err(anyhow!(
            "all rebuild attempts failed:\n  {}",
            errors.join("\n  ")
        ));
    }
    Ok(())
}

/// Handle `dump --file <path>`. The flag is an explicit override — a
/// workspace scope makes no sense alongside it (the corpus walk is the
/// thing being bypassed), so reject that combination loudly.
/// Otherwise, resolve the session in-file per:
///   - `<id>` given: parse that file for that id (or error listing what's there)
///   - `--latest`  : newest session in the file
///   - neither     : single-session file is used; multi-session errors with the id list
fn load_from_file_dispatch(
    adapter: &ClaudeCode,
    path: &Path,
    args: &DumpArgs,
) -> Result<Conversation> {
    if args.workspace_toml.is_some() {
        return Err(anyhow!(
            "--file and --workspace-toml conflict: --file is an explicit backing-file override, workspace scope applies only to the corpus walk"
        ));
    }
    if let Some(id) = args.session_id.as_deref() {
        if args.latest {
            return Err(anyhow!(
                "--latest and a positional session id are mutually exclusive — pick one"
            ));
        }
        return adapter.load_from_file(path, id).with_context(|| {
            let known = adapter
                .list_sessions_in_file(path)
                .ok()
                .map(|ms| {
                    ms.iter()
                        .map(|m| m.id.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            if known.is_empty() {
                format!("load {id} from {}", path.display())
            } else {
                format!(
                    "load {id} from {} (sessions in file: {known})",
                    path.display()
                )
            }
        });
    }
    let metas = adapter.list_sessions_in_file(path)?;
    if metas.is_empty() {
        return Err(anyhow!(
            "no sessions found in {} — is this a Claude Code JSONL?",
            path.display()
        ));
    }
    if args.latest {
        let pick = metas
            .into_iter()
            .max_by_key(|m| m.updated_at)
            .expect("non-empty checked above");
        return adapter.load_from_file(path, &pick.id);
    }
    if metas.len() == 1 {
        return adapter.load_from_file(path, &metas[0].id);
    }
    let ids: Vec<String> = metas.into_iter().map(|m| m.id).collect();
    Err(anyhow!(
        "{} contains {} sessions — pass a positional session id or --latest (available: {})",
        path.display(),
        ids.len(),
        ids.join(", ")
    ))
}
