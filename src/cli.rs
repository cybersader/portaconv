//! CLI argument parsing. v0.1 surface: `list`, `dump`, `mcp serve`.
//!
//! Markdown rendering + --rewrite transforms are stubbed here but land
//! as a follow-up commit — this file's scope is the argument shape and
//! the `list` / `dump --format json` execution paths.

use std::io::{self, Write};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use crate::adapters::{
    dedup_sessions, grep_sessions, limit_sessions, parse_since, sort_sessions, ClaudeCode,
    ConvoAdapter, SortKey, WorkspaceScope,
};
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
    // Minimal parse — we only care about the `projects` array. Using
    // serde_json's feature-equivalent here would require a full toml
    // dep; a regex-light line scan is fine for the v0.1 contract.
    let body = std::fs::read_to_string(path)
        .with_context(|| format!("read workspace toml {}", path.display()))?;
    let base = path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut scope = WorkspaceScope::default();
    // Projects default to the TOML file's own directory if no
    // `projects = [...]` line is declared — same convention portagenty
    // uses for a workspace with no explicit project list.
    scope.project_paths.push(base.clone());
    for line in body.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("projects") else {
            continue;
        };
        let Some(rhs) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let rhs = rhs.trim();
        let Some(inner) = rhs.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
            continue;
        };
        scope.project_paths.clear();
        for item in inner.split(',') {
            let item = item.trim().trim_matches(|c: char| c == '"' || c == '\'');
            if item.is_empty() {
                continue;
            }
            let expanded = expand_path(item, &base);
            scope.project_paths.push(expanded);
        }
        break;
    }
    Ok(scope)
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
    let target_id = resolve_dump_target(&adapter, &args)?;
    let mut conv = adapter.load(&target_id)?;
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
