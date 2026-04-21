//! Paste-ready markdown renderer.
//!
//! Goal: when the output is pasted into a fresh agent session, the
//! agent picks up the substantive context. Tool-call *state* doesn't
//! survive the paste — that's by design per the design-decisions doc
//! (the tool-call args are re-runnable against the current filesystem
//! anyway, so we preserve the intent, not the replay).
//!
//! Defaults: thinking blocks collapsed, tool results truncated to a
//! short preview. Both are overridable via `MarkdownOptions` so a
//! user pasting between long-context agents can keep the lot.

use std::fmt::Write;

use crate::model::{ContentBlock, Conversation, Message, Role};

/// Knobs for the renderer. Defaults lean paste-ergonomic — small
/// output that still reads like a conversation.
#[derive(Debug, Clone)]
pub struct MarkdownOptions {
    /// Include `ContentBlock::Thinking` blocks verbatim. Default off —
    /// thinking is internal reasoning, rarely worth pasting back.
    pub include_thinking: bool,
    /// Emit full tool-result bodies. Default off — results are capped
    /// at `tool_result_preview_chars` and get a truncation marker.
    pub full_results: bool,
    /// Preview cap for `tool_result` when `full_results` is false.
    pub tool_result_preview_chars: usize,
    /// Include `system_events` from conversation-level extensions as
    /// a trailing block. Default off — it's noise for paste workflows.
    pub include_system_events: bool,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            include_thinking: false,
            full_results: false,
            tool_result_preview_chars: 600,
            include_system_events: false,
        }
    }
}

pub fn render_markdown(conv: &Conversation, opts: &MarkdownOptions) -> String {
    let mut out = String::new();

    // Header — keep small; the conversation body is the product.
    let title = conv.title.as_deref().unwrap_or("(untitled)");
    let _ = writeln!(out, "# {title}");
    let _ = writeln!(out);
    let _ = writeln!(out, "- session: `{}`", conv.id);
    if let Some(cwd) = conv.cwd.as_ref() {
        let _ = writeln!(out, "- cwd: `{}`", cwd.display());
    }
    if let Some(t) = conv.started_at {
        let _ = writeln!(out, "- started: {}", t.format("%Y-%m-%d %H:%M UTC"));
    }
    if let Some(branch) = conv
        .messages
        .first()
        .and_then(|m| m.extensions.get("gitBranch"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        let _ = writeln!(out, "- git branch: `{branch}`");
    }
    // Truncation marker — present when apply_tail() slimmed the
    // conversation before render. Paste recipients should see this
    // explicitly so they understand the paste is a window, not the
    // full session.
    if let Some(t) = conv.extensions.get("truncated") {
        let tail = t.get("tail").and_then(|v| v.as_u64()).unwrap_or(0);
        let original = t
            .get("original_message_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let dropped = t.get("dropped").and_then(|v| v.as_u64()).unwrap_or(0);
        let _ = writeln!(
            out,
            "- truncated: last {tail} of {original} messages ({dropped} earlier dropped)"
        );
    }
    let _ = writeln!(out);

    for msg in &conv.messages {
        render_message(&mut out, msg, opts);
    }

    if opts.include_system_events {
        if let Some(events) = conv
            .extensions
            .get("system_events")
            .and_then(|v| v.as_array())
        {
            if !events.is_empty() {
                let _ = writeln!(out, "---");
                let _ = writeln!(out);
                let _ = writeln!(out, "## System events");
                let _ = writeln!(out);
                for ev in events {
                    let _ = writeln!(out, "- `{ev}`");
                }
                let _ = writeln!(out);
            }
        }
    }

    out
}

fn render_message(out: &mut String, msg: &Message, opts: &MarkdownOptions) {
    let heading = match msg.role {
        Role::User => "## User",
        Role::Assistant => "## Assistant",
        Role::System => "## System",
        Role::Tool => "## Tool",
    };
    let _ = writeln!(out, "{heading}");
    let _ = writeln!(out);

    for block in &msg.content {
        render_block(out, block, opts);
    }
}

fn render_block(out: &mut String, block: &ContentBlock, opts: &MarkdownOptions) {
    match block {
        ContentBlock::Text { text } => {
            let _ = writeln!(out, "{}", text.trim_end());
            let _ = writeln!(out);
        }
        ContentBlock::Thinking { text } => {
            if opts.include_thinking {
                let _ = writeln!(out, "> _thinking:_");
                for line in text.lines() {
                    let _ = writeln!(out, "> {line}");
                }
                let _ = writeln!(out);
            }
            // If skipped, emit nothing — a placeholder would clutter
            // more than a missing block signals.
        }
        ContentBlock::ToolUse { id: _, name, input } => {
            let _ = writeln!(out, "**tool call:** `{name}`");
            let _ = writeln!(out);
            let _ = writeln!(out, "```json");
            // Pretty-print the input — makes paste-readable, and agents
            // re-parse JSON reliably either way.
            let rendered =
                serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
            let _ = writeln!(out, "{rendered}");
            let _ = writeln!(out, "```");
            let _ = writeln!(out);
        }
        ContentBlock::ToolResult {
            tool_use_id: _,
            output,
            is_error,
        } => {
            let label = if *is_error {
                "tool result (error)"
            } else {
                "tool result"
            };
            let _ = writeln!(out, "**{label}:**");
            let _ = writeln!(out);
            let body = if opts.full_results
                || output.chars().count() <= opts.tool_result_preview_chars
            {
                output.clone()
            } else {
                let truncated: String = output
                    .chars()
                    .take(opts.tool_result_preview_chars)
                    .collect();
                let dropped = output.chars().count() - opts.tool_result_preview_chars;
                format!("{truncated}\n… ({dropped} more chars truncated; --full-results to expand)")
            };
            let _ = writeln!(out, "```");
            let _ = writeln!(out, "{}", body.trim_end_matches('\n'));
            let _ = writeln!(out, "```");
            let _ = writeln!(out);
        }
        ContentBlock::Unknown { raw } => {
            let _ = writeln!(
                out,
                "<!-- unknown content block preserved in extensions -->"
            );
            let _ = writeln!(
                out,
                "_unknown block:_ `{}`",
                raw.get("type").and_then(|t| t.as_str()).unwrap_or("?")
            );
            let _ = writeln!(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ContentBlock, Conversation, Message, Role};
    use serde_json::json;

    fn sample() -> Conversation {
        Conversation {
            id: "aaaaaaaa-bbbb-cccc-dddd-000000000001".into(),
            title: Some("Hello world — please read README.md".into()),
            cwd: Some(std::path::PathBuf::from("/test/workspace/sample")),
            started_at: None,
            messages: vec![
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::text("Hello world — please read README.md")],
                    timestamp: None,
                    extensions: json!({"gitBranch": "main", "version": "2.1.51"}),
                },
                Message {
                    role: Role::Assistant,
                    content: vec![
                        ContentBlock::Thinking {
                            text: "I should read the file.".into(),
                        },
                        ContentBlock::text("Sure — reading README.md now."),
                        ContentBlock::ToolUse {
                            id: "t1".into(),
                            name: "Read".into(),
                            input: json!({"file_path": "/test/workspace/sample/README.md"}),
                        },
                    ],
                    timestamp: None,
                    extensions: serde_json::Value::Null,
                },
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "t1".into(),
                        output: "# Sample\nThis is a test fixture.\n".into(),
                        is_error: false,
                    }],
                    timestamp: None,
                    extensions: serde_json::Value::Null,
                },
            ],
            extensions: serde_json::Value::Null,
        }
    }

    #[test]
    fn default_renders_no_thinking() {
        let out = render_markdown(&sample(), &MarkdownOptions::default());
        assert!(!out.contains("_thinking:_"));
        assert!(!out.contains("I should read the file"));
        assert!(out.contains("## User"));
        assert!(out.contains("## Assistant"));
        assert!(out.contains("**tool call:** `Read`"));
        assert!(out.contains("file_path"));
        assert!(out.contains("**tool result:**"));
        assert!(out.contains("test fixture"));
    }

    #[test]
    fn include_thinking_opt_in() {
        let opts = MarkdownOptions {
            include_thinking: true,
            ..MarkdownOptions::default()
        };
        let out = render_markdown(&sample(), &opts);
        assert!(out.contains("_thinking:_"));
        assert!(out.contains("I should read the file"));
    }

    #[test]
    fn long_tool_result_truncates() {
        let big: String = "x".repeat(1200);
        let mut conv = sample();
        conv.messages[2].content[0] = ContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            output: big,
            is_error: false,
        };
        let out = render_markdown(&conv, &MarkdownOptions::default());
        assert!(out.contains("truncated"));

        let opts = MarkdownOptions {
            full_results: true,
            ..MarkdownOptions::default()
        };
        let out = render_markdown(&conv, &opts);
        assert!(!out.contains("truncated"));
    }

    #[test]
    fn header_carries_meta() {
        let out = render_markdown(&sample(), &MarkdownOptions::default());
        assert!(out.contains("# Hello world — please read README.md"));
        assert!(out.contains("session: `aaaaaaaa-"));
        assert!(out.contains("cwd: `/test/workspace/sample`"));
        assert!(out.contains("git branch: `main`"));
    }
}
