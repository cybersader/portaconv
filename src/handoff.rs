//! Bounded, recency-biased recovery handoffs.
//!
//! Motivation (2026-07-19): a session that crosses the model context
//! window can no longer `/compact` — compaction itself submits the
//! oversized conversation to the model and 400s. Recovery has to happen
//! locally. `pconv dump --tail N` alone is not safe because one
//! "message" can be a ~150 KB injected Skill body, so message-count
//! limits do not bound bytes.
//!
//! `pconv handoff` builds a deterministic, exactly-byte-bounded markdown
//! document from the persisted JSONL:
//!   - pins the first substantive user task (orientation),
//!   - keeps a contiguous newest-first suffix of interaction units,
//!   - omits thinking, abbreviates giant injected skill bodies and
//!     system reminders, head/tail-truncates every remaining block,
//!   - enforces the total UTF-8 byte budget on the rendered output.
//!
//! Nothing here reads or writes the source transcript beyond the normal
//! adapter load; the transform is pure `Conversation` → `String`.

use crate::model::{ContentBlock, Conversation, Message, Role};

/// Tuning knobs. Defaults chosen for a 200k-window rescue: 48 KiB of
/// markdown is roughly 10–14k tokens of English/code — enough to carry
/// the task + recent state, small enough to leave the new session room.
#[derive(Debug, Clone)]
pub struct HandoffOptions {
    /// Hard cap on the rendered document, in UTF-8 bytes.
    pub max_bytes: usize,
    /// Only the newest N messages are candidates for the recent suffix
    /// (the pinned initial task is found independently). 0 = all.
    pub candidate_messages: usize,
    /// Per-block byte cap (text; tool inputs/results use fractions of it).
    pub max_block_bytes: usize,
}

impl Default for HandoffOptions {
    fn default() -> Self {
        Self {
            max_bytes: 48 * 1024,
            candidate_messages: 120,
            max_block_bytes: 8 * 1024,
        }
    }
}

/// Markers that identify an injected skill body (the harness pastes the
/// whole SKILL.md into a user turn after a `Skill` tool call). The
/// first is near-unique to skill bodies; the rest are corroborating.
const SKILL_STRONG_MARKERS: &[&str] = &["Base directory for this skill:"];
const SKILL_WEAK_MARKERS: &[&str] = &["## User Request", "ARGUMENTS:", "<command-name>"];

const REMINDER_OPEN: &str = "<system-reminder>";
const REMINDER_CLOSE: &str = "</system-reminder>";

/// Build the bounded handoff document. Deterministic: same conversation
/// + same options ⇒ byte-identical output.
pub fn render_handoff(conv: &Conversation, opts: &HandoffOptions) -> Result<String, String> {
    let scaffold_floor = 1024;
    if opts.max_bytes < scaffold_floor {
        return Err(format!(
            "max-bytes {} is below the minimum scaffold size {} — raise the budget",
            opts.max_bytes, scaffold_floor
        ));
    }

    let total = conv.messages.len();

    // ---- classify + sanitize every message up front -------------------
    let sanitized: Vec<SanitizedMessage> = conv
        .messages
        .iter()
        .enumerate()
        .map(|(ix, m)| sanitize_message(ix, m, conv, opts))
        .collect();

    // ---- pin the first substantive user task --------------------------
    let task_ix = sanitized.iter().position(|m| m.is_substantive_user_task);
    let task_fallback = task_ix.is_none();
    let task_ix = task_ix.or_else(|| {
        sanitized
            .iter()
            .position(|m| m.role == Role::User && !m.rendered.trim().is_empty())
    });

    // ---- candidate window for the recent suffix -----------------------
    let start = if opts.candidate_messages == 0 || total <= opts.candidate_messages {
        0
    } else {
        total - opts.candidate_messages
    };

    // ---- group candidates into interaction units ----------------------
    // A unit = a run of messages that ends at a user/tool boundary:
    // assistant tool_use + following tool_result(s) + adjacent prose
    // stay together so no orphan output ships without its call.
    let units = build_units(&sanitized[start..]);

    // ---- assemble under budget, newest unit first ---------------------
    let header = render_header(conv, total, opts, task_fallback);
    let task_section = task_ix
        .map(|ix| {
            format!(
                "## Original task orientation\n\n{}\n",
                sanitized[ix].rendered
            )
        })
        .unwrap_or_else(|| "## Original task orientation\n\n(none found)\n".to_string());
    let footer = "\n## Reading guidance\n\nThis is a selective recovery handoff, not a full transcript. The source session JSONL is unchanged. If retained recent context conflicts with the original orientation, prefer the recent context. Verify current repository state before trusting historical tool results.\n";

    let fixed_cost = header.len() + task_section.len() + footer.len() + 64;
    if fixed_cost >= opts.max_bytes {
        // Task section too big even alone — shrink it hard and retry once.
        let squeezed = truncate_head_tail(
            &task_section,
            opts.max_bytes
                .saturating_sub(header.len() + footer.len() + 128),
            0.75,
        );
        let doc = format!("{header}{squeezed}\n{footer}");
        return if doc.len() <= opts.max_bytes {
            Ok(doc)
        } else {
            Err(format!(
                "budget {} too small even for header + minimal task section",
                opts.max_bytes
            ))
        };
    }

    let mut budget = opts.max_bytes - fixed_cost;
    let mut chosen: Vec<&Unit> = Vec::new();
    for unit in units.iter().rev() {
        // Dedup the pinned task ONLY when a unit is exactly that one
        // message — a unit that merely contains it carries other
        // context (tool calls/results) we must not drop.
        if let Some(tix) = task_ix {
            if start <= tix && unit.indices == vec![tix - start] {
                continue;
            }
        }
        let cost = unit.rendered_len + 2;
        if cost > budget {
            if chosen.is_empty() {
                // Always keep the newest unit — the final enforcement
                // pass squeezes it to fit the budget.
                chosen.push(unit);
            }
            break;
        }
        budget -= cost;
        chosen.push(unit);
    }
    chosen.reverse();

    let mut recent = String::from("## Recent retained context\n\n");
    for unit in &chosen {
        recent.push_str(&unit.rendered);
        recent.push('\n');
    }
    let dropped_units = units.len().saturating_sub(chosen.len());
    if dropped_units > 0 {
        recent.push_str(&format!(
            "\n*({dropped_units} earlier interaction unit(s) in the candidate window omitted for budget)*\n"
        ));
    }

    let mut doc = format!("{header}{task_section}\n{recent}{footer}");

    // ---- final hard enforcement ---------------------------------------
    if doc.len() > opts.max_bytes {
        // Shrink the recent section from the front (oldest first) until it fits.
        let overshoot = doc.len() - opts.max_bytes;
        let target_recent = recent.len().saturating_sub(overshoot + 64);
        let squeezed = truncate_head_tail(&recent, target_recent.max(256), 0.25);
        doc = format!("{header}{task_section}\n{squeezed}{footer}");
    }
    if doc.len() > opts.max_bytes {
        return Err(format!(
            "internal: rendered {} bytes > budget {} after squeeze",
            doc.len(),
            opts.max_bytes
        ));
    }
    Ok(doc)
}

// ---------------------------------------------------------------------
// internals
// ---------------------------------------------------------------------

struct SanitizedMessage {
    role: Role,
    rendered: String,
    is_substantive_user_task: bool,
}

struct Unit {
    indices: Vec<usize>,
    rendered: String,
    rendered_len: usize,
}

fn render_header(
    conv: &Conversation,
    total: usize,
    opts: &HandoffOptions,
    task_fallback: bool,
) -> String {
    let mut h = String::from("# Claude Code recovery handoff\n\n");
    h.push_str(&format!("- source session: `{}`\n", conv.id));
    if let Some(t) = &conv.title {
        h.push_str(&format!("- title: {}\n", truncate_head_tail(t, 200, 1.0)));
    }
    if let Some(c) = &conv.cwd {
        h.push_str(&format!("- cwd: `{}`\n", c.display()));
    }
    if let Some(s) = &conv.started_at {
        h.push_str(&format!("- started: {}\n", s.format("%Y-%m-%d %H:%M UTC")));
    }
    h.push_str(&format!(
        "- original messages: {total} · candidate window: {} · budget: {} bytes\n",
        if opts.candidate_messages == 0 {
            "all".to_string()
        } else {
            opts.candidate_messages.to_string()
        },
        opts.max_bytes
    ));
    if task_fallback {
        h.push_str("- note: no clean user task found; using first non-empty user text\n");
    }
    h.push('\n');
    h
}

fn sanitize_message(
    ix: usize,
    msg: &Message,
    conv: &Conversation,
    opts: &HandoffOptions,
) -> SanitizedMessage {
    let text_cap = opts.max_block_bytes;
    let input_cap = opts.max_block_bytes / 2;
    let result_cap = opts.max_block_bytes / 2;
    let error_cap = opts.max_block_bytes;

    let after_skill_call = preceded_by_skill_call(ix, conv);

    let mut out = String::new();
    let mut has_user_text = false;

    for block in &msg.content {
        match block {
            ContentBlock::Thinking { .. } => { /* always omitted */ }
            ContentBlock::Text { text } => {
                if is_skill_body(text, after_skill_call) {
                    out.push_str(&format!(
                        "*[Injected skill body omitted: {} bytes]*\n",
                        text.len()
                    ));
                } else if let Some(inner) = reminder_payload(text) {
                    if inner.trim().is_empty() {
                        out.push_str(&format!(
                            "*[System reminder omitted: {} bytes]*\n",
                            text.len()
                        ));
                    } else {
                        out.push_str("**Mid-turn user message:** ");
                        out.push_str(&truncate_head_tail(&inner, text_cap, 0.75));
                        out.push('\n');
                    }
                } else {
                    if msg.role == Role::User && !text.trim().is_empty() {
                        has_user_text = true;
                    }
                    out.push_str(&truncate_head_tail(text, text_cap, 0.75));
                    out.push('\n');
                }
            }
            ContentBlock::ToolUse { id: _, name, input } => {
                let compact = serde_json::to_string(&sorted_json(input)).unwrap_or_default();
                out.push_str(&format!(
                    "**tool:** `{}` `{}`\n",
                    name,
                    truncate_head_tail(&compact, input_cap, 0.75)
                ));
            }
            ContentBlock::ToolResult {
                output, is_error, ..
            } => {
                let cap = if *is_error { error_cap } else { result_cap };
                let label = if *is_error {
                    "tool ERROR"
                } else {
                    "tool result"
                };
                out.push_str(&format!(
                    "**{label}:**\n```\n{}\n```\n",
                    truncate_head_tail(output, cap, 0.25)
                ));
            }
            ContentBlock::Unknown { raw } => {
                let size = serde_json::to_string(raw).map(|s| s.len()).unwrap_or(0);
                let kind = raw
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                out.push_str(&format!("*[{kind} block omitted: {size} bytes]*\n"));
            }
        }
    }

    let role_label = match msg.role {
        Role::User => "## User",
        Role::Assistant => "## Assistant",
        Role::System => "## System",
        Role::Tool => "## Tool",
    };
    let rendered = if out.trim().is_empty() {
        String::new()
    } else {
        format!("{role_label}\n\n{out}")
    };

    let is_substantive_user_task = msg.role == Role::User
        && has_user_text
        && !after_skill_call
        && msg.content.iter().any(|b| {
            matches!(b, ContentBlock::Text { text } if !text.trim().is_empty()
                && reminder_payload(text).is_none()
                && !is_skill_body(text, after_skill_call))
        });

    SanitizedMessage {
        role: msg.role,
        rendered,
        is_substantive_user_task,
    }
}

/// True when the nearest preceding assistant tool_use was the `Skill`
/// tool — the structural half of skill-body detection.
fn preceded_by_skill_call(ix: usize, conv: &Conversation) -> bool {
    for m in conv.messages[..ix].iter().rev() {
        for b in m.content.iter().rev() {
            if let ContentBlock::ToolUse { name, .. } = b {
                return name == "Skill";
            }
        }
        // Keep scanning back through tool_result/user messages until we
        // hit an assistant message with prose but no tool call.
        if m.role == Role::Assistant
            && m.content
                .iter()
                .any(|b| matches!(b, ContentBlock::Text { text } if !text.trim().is_empty()))
        {
            return false;
        }
    }
    false
}

fn is_skill_body(text: &str, after_skill_call: bool) -> bool {
    let strong = SKILL_STRONG_MARKERS.iter().any(|m| text.contains(m));
    if strong {
        // The marker alone is near-unique to injected skill bodies.
        return true;
    }
    if !after_skill_call {
        return false;
    }
    let weak_hits = SKILL_WEAK_MARKERS
        .iter()
        .filter(|m| text.contains(*m))
        .count();
    text.len() > 4096 && weak_hits >= 2
}

/// If the whole block is a `<system-reminder>…</system-reminder>`
/// envelope, return the inner payload for selective retention.
fn reminder_payload(text: &str) -> Option<String> {
    let t = text.trim();
    if t.starts_with(REMINDER_OPEN) && t.ends_with(REMINDER_CLOSE) {
        let inner = &t[REMINDER_OPEN.len()..t.len() - REMINDER_CLOSE.len()];
        // Reminders that merely carry harness boilerplate get dropped;
        // ones carrying a real embedded user message keep the payload.
        if inner.contains("The user sent a new message") {
            return Some(inner.trim().to_string());
        }
        return Some(String::new());
    }
    None
}

/// Group a sanitized slice into interaction units: each unit ends after
/// the message(s) that carry the results of any tool calls opened in it.
fn build_units(msgs: &[SanitizedMessage]) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_text = String::new();

    for (i, m) in msgs.iter().enumerate() {
        if m.rendered.is_empty() {
            continue;
        }
        cur.push(i);
        cur_text.push_str(&m.rendered);
        cur_text.push('\n');
        // Unit boundary: a user message with actual prose (a new ask)
        // closes the previous unit at the *next* iteration; simplest
        // deterministic rule: close after every assistant message whose
        // successor is a user prose message, or at slice end.
        let next_is_user_prose = msgs
            .get(i + 1)
            .map(|n| n.role == Role::User && n.is_substantive_user_task)
            .unwrap_or(true);
        if next_is_user_prose && m.role == Role::Assistant {
            units.push(Unit {
                indices: std::mem::take(&mut cur),
                rendered_len: cur_text.len(),
                rendered: std::mem::take(&mut cur_text),
            });
        }
    }
    if !cur.is_empty() {
        units.push(Unit {
            indices: cur,
            rendered_len: cur_text.len(),
            rendered: cur_text,
        });
    }
    units
}

/// Head/tail truncation at UTF-8 char boundaries. `head_frac` of the
/// byte budget goes to the head, the rest to the tail, with a marker
/// stating what was dropped. `head_frac == 1.0` = head-only.
fn truncate_head_tail(text: &str, max_bytes: usize, head_frac: f32) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let marker = format!(
        "\n… [{} of {} bytes omitted] …\n",
        text.len() - max_bytes.min(text.len()),
        text.len()
    );
    let usable = max_bytes.saturating_sub(marker.len());
    if usable < 8 {
        return marker.trim().to_string();
    }
    let head_bytes = ((usable as f32) * head_frac) as usize;
    let tail_bytes = usable - head_bytes;

    let head_end = floor_char_boundary(text, head_bytes);
    let tail_start = ceil_char_boundary(text, text.len() - tail_bytes.min(text.len()));

    let mut out = String::with_capacity(max_bytes);
    out.push_str(&text[..head_end]);
    out.push_str(&marker);
    if tail_start < text.len() && tail_bytes > 0 {
        out.push_str(&text[tail_start..]);
    }
    out
}

fn floor_char_boundary(s: &str, ix: usize) -> usize {
    let mut i = ix.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, ix: usize) -> usize {
    let mut i = ix.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Deterministic JSON: objects with sorted keys, recursively.
fn sorted_json(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => {
            let mut sorted: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            sorted.sort_by_key(|(k, _)| k.as_str().to_string());
            let mut out = serde_json::Map::new();
            for (k, val) in sorted {
                out.insert(k.clone(), sorted_json(val));
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(a) => {
            serde_json::Value::Array(a.iter().map(sorted_json).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn msg(role: Role, blocks: Vec<ContentBlock>) -> Message {
        Message {
            role,
            content: blocks,
            timestamp: None,
            extensions: Value::Null,
        }
    }

    fn conv(messages: Vec<Message>) -> Conversation {
        Conversation {
            id: "test-session".into(),
            title: Some("Test".into()),
            cwd: None,
            started_at: None,
            messages,
            extensions: Value::Null,
        }
    }

    #[test]
    fn respects_total_budget_exactly() {
        let big = "x".repeat(200_000);
        let c = conv(vec![
            msg(Role::User, vec![ContentBlock::text("Build the widget")]),
            msg(Role::Assistant, vec![ContentBlock::text(big.clone())]),
            msg(Role::User, vec![ContentBlock::text(big.clone())]),
            msg(Role::Assistant, vec![ContentBlock::text("done")]),
        ]);
        let opts = HandoffOptions::default();
        let out = render_handoff(&c, &opts).unwrap();
        assert!(out.len() <= opts.max_bytes, "over budget: {}", out.len());
        assert!(out.contains("Build the widget"));
    }

    #[test]
    fn deterministic_output() {
        let c = conv(vec![
            msg(Role::User, vec![ContentBlock::text("task")]),
            msg(
                Role::Assistant,
                vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "Bash".into(),
                    input: serde_json::json!({"b": 2, "a": 1}),
                }],
            ),
            msg(
                Role::User,
                vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    output: "ok".into(),
                    is_error: false,
                }],
            ),
            msg(Role::Assistant, vec![ContentBlock::text("finished")]),
        ]);
        let opts = HandoffOptions::default();
        let a = render_handoff(&c, &opts).unwrap();
        let b = render_handoff(&c, &opts).unwrap();
        assert_eq!(a, b);
        // sorted keys in tool input
        assert!(a.contains(r#"{"a":1,"b":2}"#));
    }

    #[test]
    fn skill_body_omitted_but_plain_paste_kept() {
        let skill_body = format!(
            "# Some Skill\n\nBase directory for this skill: /tmp/x\n\n{}",
            "instructions ".repeat(20_000)
        );
        let plain_doc = format!("# My design doc\n\n{}", "content ".repeat(1_000));
        let c = conv(vec![
            msg(Role::User, vec![ContentBlock::text("please configure")]),
            msg(
                Role::Assistant,
                vec![ContentBlock::ToolUse {
                    id: "s1".into(),
                    name: "Skill".into(),
                    input: serde_json::json!({"skill": "update-config"}),
                }],
            ),
            msg(Role::User, vec![ContentBlock::text(skill_body)]),
            msg(Role::Assistant, vec![ContentBlock::text("configured")]),
            msg(Role::User, vec![ContentBlock::text(plain_doc)]),
            msg(Role::Assistant, vec![ContentBlock::text("read it")]),
        ]);
        let out = render_handoff(&c, &HandoffOptions::default()).unwrap();
        assert!(out.contains("Injected skill body omitted"));
        assert!(!out.contains("instructions instructions instructions instructions"));
        assert!(out.contains("My design doc"));
    }

    #[test]
    fn thinking_always_omitted_errors_kept() {
        let c = conv(vec![
            msg(Role::User, vec![ContentBlock::text("run tests")]),
            msg(
                Role::Assistant,
                vec![
                    ContentBlock::Thinking {
                        text: "secret reasoning".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "Bash".into(),
                        input: serde_json::json!({"command": "cargo test"}),
                    },
                ],
            ),
            msg(
                Role::User,
                vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    output: format!("{}\nerror[E0308]: mismatched types", "noise\n".repeat(2000)),
                    is_error: true,
                }],
            ),
            msg(Role::Assistant, vec![ContentBlock::text("fixing")]),
        ]);
        let out = render_handoff(&c, &HandoffOptions::default()).unwrap();
        assert!(!out.contains("secret reasoning"));
        assert!(out.contains("tool ERROR"));
        // tail-biased result truncation keeps the error line
        assert!(out.contains("mismatched types"));
    }

    #[test]
    fn utf8_boundaries_survive_truncation() {
        let emoji_text = "🦀".repeat(10_000);
        let c = conv(vec![
            msg(Role::User, vec![ContentBlock::text("unicode task")]),
            msg(Role::Assistant, vec![ContentBlock::text(emoji_text)]),
        ]);
        let out = render_handoff(&c, &HandoffOptions::default()).unwrap();
        // Would panic on invalid boundary; also assert budget.
        assert!(out.len() <= HandoffOptions::default().max_bytes);
    }

    #[test]
    fn reminder_boilerplate_dropped_midturn_message_kept() {
        let reminder = "<system-reminder>\nThe user sent a new message while you were working:\nfix the login bug first\n</system-reminder>";
        let noise = format!(
            "<system-reminder>\n{}\n</system-reminder>",
            "available skills listing ".repeat(500)
        );
        let c = conv(vec![
            msg(Role::User, vec![ContentBlock::text("main task")]),
            msg(Role::User, vec![ContentBlock::text(noise)]),
            msg(Role::User, vec![ContentBlock::text(reminder)]),
            msg(Role::Assistant, vec![ContentBlock::text("on it")]),
        ]);
        let out = render_handoff(&c, &HandoffOptions::default()).unwrap();
        assert!(out.contains("fix the login bug first"));
        assert!(!out.contains("available skills listing available skills"));
    }

    #[test]
    fn tiny_budget_errors_cleanly() {
        let c = conv(vec![msg(Role::User, vec![ContentBlock::text("hi")])]);
        let opts = HandoffOptions {
            max_bytes: 100,
            ..Default::default()
        };
        assert!(render_handoff(&c, &opts).is_err());
    }

    #[test]
    fn recency_bias_keeps_newest() {
        let mut messages = vec![msg(
            Role::User,
            vec![ContentBlock::text("original task: build the parser")],
        )];
        for i in 0..200 {
            messages.push(msg(
                Role::User,
                vec![ContentBlock::text(format!(
                    "iteration {i}: {}",
                    "filler ".repeat(500)
                ))],
            ));
            messages.push(msg(
                Role::Assistant,
                vec![ContentBlock::text(format!("done with iteration {i}"))],
            ));
        }
        let c = conv(messages);
        let out = render_handoff(&c, &HandoffOptions::default()).unwrap();
        assert!(out.contains("original task: build the parser"));
        assert!(out.contains("done with iteration 199"));
        assert!(!out.contains("iteration 5:"));
    }
}
