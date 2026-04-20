//! OS-path rewriting inside conversation content.
//!
//! Scope (per adapter notes §7 Q#3):
//! - `ContentBlock::Text.text` — prose written by user/assistant
//! - `ContentBlock::ToolUse.input` — tool-call argument strings inside the JSON
//! - `ContentBlock::ToolResult.output` — result bodies may carry paths
//!
//! NOT touched:
//! - `Conversation.cwd` — metadata about the authoring environment
//! - `Message.extensions` — adapter-preserved raw fields
//! - `ContentBlock::ToolUse.id` / `tool_use_id` — opaque handles
//! - `ContentBlock::Thinking` — reasoning trace, left verbatim
//!
//! Patterns are deliberately conservative: they match well-formed
//! absolute paths and leave ambiguous cases alone. A user relying on
//! the transform for correctness (vs convenience) should review the
//! diff before pasting.

use std::sync::OnceLock;

use regex::{Captures, Regex};
use serde_json::Value;

use crate::model::{ContentBlock, Conversation};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PathRewrite {
    /// `/mnt/c/…` → `C:\…`. Only the WSL `/mnt/<letter>/…` form is
    /// rewritten; bare `/home/user/…` stays untouched.
    WslToWin,
    /// `C:\…` or `C:/…` → `/mnt/c/…`. Drive letters get lowercased.
    WinToWsl,
    /// Replace any absolute path (WSL-style OR Windows-style) with the
    /// literal placeholder `<path>`. Useful when the paths shouldn't
    /// just be translated but erased — e.g. when pasting into a
    /// context that has no local filesystem to resolve against.
    Strip,
}

pub fn apply_path_rewrite(conv: &mut Conversation, mode: PathRewrite) {
    for msg in &mut conv.messages {
        for block in &mut msg.content {
            match block {
                ContentBlock::Text { text } => *text = rewrite_str(text, mode),
                ContentBlock::ToolUse { input, .. } => rewrite_json(input, mode),
                ContentBlock::ToolResult { output, .. } => {
                    *output = rewrite_str(output, mode);
                }
                ContentBlock::Thinking { .. } | ContentBlock::Unknown { .. } => {}
            }
        }
    }
}

fn rewrite_json(value: &mut Value, mode: PathRewrite) {
    match value {
        Value::String(s) => *s = rewrite_str(s, mode),
        Value::Array(a) => a.iter_mut().for_each(|v| rewrite_json(v, mode)),
        Value::Object(o) => o.values_mut().for_each(|v| rewrite_json(v, mode)),
        _ => {}
    }
}

fn wsl_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // /mnt/<letter>/<rest-until-whitespace-or-quote-or-backtick-or-paren>
    //
    // Matches anywhere in the string. Paths with spaces inside get
    // truncated at the space — users who need space-paths should review
    // the diff. The letter class is strictly [a-z] so `Cargo/mnt/foo`
    // (no drive-letter meaning) won't match; only /mnt/ followed by a
    // single lowercase letter followed by / or end-of-boundary does.
    RE.get_or_init(|| {
        Regex::new(r#"/mnt/(?P<letter>[a-z])(?P<rest>(?:/[^\s\)\]\}"'`]*)*)"#).unwrap()
    })
}

fn win_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // <drive>:\<rest> or <drive>:/<non-slash-rest>. The forward-slash
    // branch excludes a second `/` to avoid matching URL schemes like
    // `https://` (where `s:` otherwise looks like a drive letter).
    RE.get_or_init(|| {
        Regex::new(
            r#"\b(?P<drive>[A-Za-z]):(?:\\(?P<bsrest>[^\s\)\]\}"'`]*)|/(?P<fsrest>[^/\s\)\]\}"'`][^\s\)\]\}"'`]*))"#,
        )
        .unwrap()
    })
}

fn rewrite_str(s: &str, mode: PathRewrite) -> String {
    match mode {
        PathRewrite::WslToWin => wsl_re()
            .replace_all(s, |caps: &Captures| {
                let letter = caps
                    .name("letter")
                    .map_or("", |m| m.as_str())
                    .to_ascii_uppercase();
                let rest = caps
                    .name("rest")
                    .map_or("", |m| m.as_str())
                    .replace('/', "\\");
                // "/mnt/c" followed by nothing → "C:" (valid — drive root).
                format!("{letter}:{rest}")
            })
            .into_owned(),
        PathRewrite::WinToWsl => win_re()
            .replace_all(s, |caps: &Captures| {
                let letter = caps
                    .name("drive")
                    .map_or("", |m| m.as_str())
                    .to_ascii_lowercase();
                let rest = caps
                    .name("bsrest")
                    .or_else(|| caps.name("fsrest"))
                    .map_or("", |m| m.as_str())
                    .replace('\\', "/");
                format!("/mnt/{letter}/{rest}")
            })
            .into_owned(),
        PathRewrite::Strip => {
            let stripped_wsl = wsl_re().replace_all(s, "<path>");
            let stripped_both = win_re().replace_all(&stripped_wsl, "<path>");
            stripped_both.into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wsl_to_win_basic() {
        let out = rewrite_str(
            "open /mnt/c/Users/alice/doc.md please",
            PathRewrite::WslToWin,
        );
        assert_eq!(out, "open C:\\Users\\alice\\doc.md please");
    }

    #[test]
    fn win_to_wsl_basic() {
        let out = rewrite_str(
            "open C:\\Users\\alice\\doc.md please",
            PathRewrite::WinToWsl,
        );
        assert_eq!(out, "open /mnt/c/Users/alice/doc.md please");
    }

    #[test]
    fn win_to_wsl_forward_slashes() {
        let out = rewrite_str("check C:/Temp/logs.txt here", PathRewrite::WinToWsl);
        assert_eq!(out, "check /mnt/c/Temp/logs.txt here");
    }

    #[test]
    fn strip_both_shapes() {
        let out = rewrite_str(
            "wsl=/mnt/d/data.csv and win=E:\\Reports\\q1.xlsx",
            PathRewrite::Strip,
        );
        assert_eq!(out, "wsl=<path> and win=<path>");
    }

    #[test]
    fn leaves_home_paths_alone() {
        // /home/user paths are genuinely Linux-native — no rewrite target.
        let out = rewrite_str("log at /home/alice/app.log", PathRewrite::WslToWin);
        assert_eq!(out, "log at /home/alice/app.log");
    }

    #[test]
    fn rewrites_tool_use_input_recursively() {
        let mut input = json!({
            "file_path": "/mnt/c/Users/alice/README.md",
            "command": "cat /mnt/d/data.csv",
            "nested": { "paths": ["/mnt/c/x", "plain text"] }
        });
        rewrite_json(&mut input, PathRewrite::WslToWin);
        assert_eq!(input["file_path"], "C:\\Users\\alice\\README.md");
        assert_eq!(input["command"], "cat D:\\data.csv");
        assert_eq!(input["nested"]["paths"][0], "C:\\x");
        assert_eq!(input["nested"]["paths"][1], "plain text");
    }

    #[test]
    fn conversation_transform_leaves_cwd_alone() {
        use crate::model::{ContentBlock, Message, Role};
        let mut conv = Conversation {
            id: "x".into(),
            title: None,
            cwd: Some(std::path::PathBuf::from("/mnt/c/work")),
            started_at: None,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::text("please read /mnt/c/work/src/main.rs")],
                timestamp: None,
                extensions: Value::Null,
            }],
            extensions: Value::Null,
        };
        apply_path_rewrite(&mut conv, PathRewrite::WslToWin);
        // Text rewritten:
        let got = match &conv.messages[0].content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!("expected Text"),
        };
        assert!(got.contains("C:\\work\\src\\main.rs"));
        // cwd untouched:
        assert_eq!(conv.cwd.as_ref().unwrap().to_str().unwrap(), "/mnt/c/work");
    }

    #[test]
    fn start_of_line_matches() {
        // The leading-context group has to accept start-of-string, not
        // just whitespace/paren — otherwise a leading `/mnt/c/...` misses.
        let out = rewrite_str("/mnt/c/x.md", PathRewrite::WslToWin);
        assert_eq!(out, "C:\\x.md");
    }
}
