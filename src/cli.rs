//! CLI argument parsing. v0.1 surface: `list`, `dump`, `mcp serve`.
//!
//! Markdown rendering + --rewrite transforms are stubbed here but land
//! as a follow-up commit — this file's scope is the argument shape and
//! the `list` / `dump --format json` execution paths.

use std::io::{self, Write};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use crate::adapters::{ClaudeCode, ConvoAdapter, WorkspaceScope};
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
}

#[derive(clap::Args, Debug)]
pub struct DumpArgs {
    /// Session ID (UUID).
    pub session_id: String,
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

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::List(args) => run_list(args),
        Command::Dump(args) => run_dump(args),
        Command::Mcp {
            sub: McpCommand::Serve,
        } => Err(anyhow!(
            "pconv mcp serve is not implemented yet — tracked for a follow-up commit"
        )),
    }
}

fn run_list(args: ListArgs) -> Result<()> {
    let adapter = ClaudeCode;
    if !adapter.detect() {
        return Err(anyhow!(
            "Claude Code storage not found. Expected ~/.claude/projects/ (or set PORTACONV_CLAUDE_ROOT)"
        ));
    }
    let scope = WorkspaceScope::default();
    let mut sessions = adapter
        .list(Some(&scope))
        .context("listing Claude Code sessions")?;
    sessions.retain(|s| s.message_count >= args.min_messages);

    let out = io::stdout();
    let mut out = out.lock();
    match args.format {
        ListFormat::Json => {
            serde_json::to_writer_pretty(&mut out, &sessions)?;
            writeln!(out)?;
        }
        ListFormat::Table => {
            writeln!(
                out,
                "{:<36}  {:>5}  {:<20}  title",
                "session-id", "msgs", "updated"
            )?;
            writeln!(out, "{}", "-".repeat(100))?;
            for s in &sessions {
                let updated = s
                    .updated_at
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "-".into());
                let title = s.title.as_deref().unwrap_or("(untitled)");
                let truncated: String = title.chars().take(60).collect();
                writeln!(
                    out,
                    "{:<36}  {:>5}  {:<20}  {}",
                    s.id, s.message_count, updated, truncated
                )?;
            }
            writeln!(out, "\n{} session(s)", sessions.len())?;
        }
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
    let mut conv = adapter.load(&args.session_id)?;
    if let Some(mode) = args.rewrite {
        apply_path_rewrite(&mut conv, mode.into());
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
