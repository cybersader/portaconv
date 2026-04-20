//! CLI argument parsing. v0.1 surface: `list`, `dump`, `mcp serve`.
//!
//! Markdown rendering + --rewrite transforms are stubbed here but land
//! as a follow-up commit — this file's scope is the argument shape and
//! the `list` / `dump --format json` execution paths.

use std::io::{self, Write};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use crate::adapters::{ClaudeCode, ConvoAdapter, WorkspaceScope};

#[derive(Parser, Debug)]
#[command(name = "pconv", version, about = "Terminal-native conversation extractor for agent CLIs")]
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
    #[arg(long, value_enum, default_value_t = DumpFormat::Json)]
    pub format: DumpFormat,
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
    Json,
    // Markdown — lands with the renderer commit.
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::List(args) => run_list(args),
        Command::Dump(args) => run_dump(args),
        Command::Mcp { sub: McpCommand::Serve } => Err(anyhow!(
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
                "{:<36}  {:>5}  {:<20}  {}",
                "session-id", "msgs", "updated", "title"
            )?;
            writeln!(out, "{}", "-".repeat(100))?;
            for s in &sessions {
                let updated = s
                    .updated_at
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "-".into());
                let title = s
                    .title
                    .as_deref()
                    .unwrap_or("(untitled)");
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
    let conv = adapter.load(&args.session_id)?;
    let out = io::stdout();
    let mut out = out.lock();
    match args.format {
        DumpFormat::Json => {
            serde_json::to_writer_pretty(&mut out, &conv)?;
            writeln!(out)?;
        }
    }
    Ok(())
}
