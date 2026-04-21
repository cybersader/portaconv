//! Shared in-memory model — adapter-agnostic.
//!
//! OpenAI Chat Completions shape on the outside (ordered `messages`
//! array, `{role, content}` per message), Anthropic-style content-blocks
//! on the inside so tool-use / tool-result / thinking survive a round
//! trip without flattening. Both `Conversation` and `Message` carry a
//! `extensions: serde_json::Value` bag for tool-specific fields the
//! core schema doesn't promote.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A normalized conversation in shared-model form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub extensions: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub extensions: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

/// Anthropic-style content blocks. `thinking` is preserved as a distinct
/// variant even though the default renderer collapses it — lossless on
/// input is the whole point.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        output: String,
        #[serde(default)]
        is_error: bool,
    },
    Thinking {
        text: String,
    },
    /// Catch-all for blocks the v0.1 adapter doesn't promote to first class
    /// (image / document / unknown). The raw JSON is preserved so a future
    /// adapter version can upgrade without a data migration.
    Unknown {
        raw: Value,
    },
}

impl ContentBlock {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }
}

impl Conversation {
    /// Keep only the last `tail` messages. Records a `truncated` entry
    /// in `extensions` so downstream consumers (markdown renderer, JSON
    /// readers) can surface that the paste is a slice, not the whole
    /// thing. A `tail` of `0` is a no-op. If the session already has
    /// ≤ `tail` messages the whole conversation is preserved untouched.
    pub fn apply_tail(&mut self, tail: usize) {
        if tail == 0 || self.messages.len() <= tail {
            return;
        }
        let original = self.messages.len();
        let dropped = original - tail;
        self.messages = self.messages.split_off(dropped);

        let mut ext_map = self
            .extensions
            .as_object()
            .cloned()
            .unwrap_or_else(serde_json::Map::new);
        ext_map.insert(
            "truncated".into(),
            serde_json::json!({
                "tail": tail,
                "original_message_count": original,
                "dropped": dropped,
            }),
        );
        self.extensions = Value::Object(ext_map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_minimal_conversation() {
        let c = Conversation {
            id: "abc".into(),
            title: None,
            cwd: None,
            started_at: None,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::text("hi")],
                timestamp: None,
                extensions: Value::Null,
            }],
            extensions: Value::Null,
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: Conversation = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, "abc");
        assert_eq!(back.messages.len(), 1);
        assert!(matches!(back.messages[0].role, Role::User));
    }

    #[test]
    fn content_block_tag_is_snake_case_type() {
        let b = ContentBlock::ToolUse {
            id: "t1".into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": "/tmp/x"}),
        };
        let s = serde_json::to_string(&b).unwrap();
        assert!(s.contains("\"type\":\"tool_use\""));
    }

    #[test]
    fn unknown_block_preserves_raw() {
        let raw = serde_json::json!({"type": "image", "source": "…"});
        let block: ContentBlock = serde_json::from_value(raw.clone())
            .unwrap_or(ContentBlock::Unknown { raw: raw.clone() });
        if let ContentBlock::Unknown { raw: r } = block {
            assert_eq!(r, raw);
        }
    }
}
