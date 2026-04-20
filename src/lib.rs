//! portaconv — terminal-native conversation extractor for agent CLIs.
//!
//! Library surface: the shared model types (`Conversation`, `Message`,
//! `ContentBlock`) and the `ConvoAdapter` trait. The Claude Code
//! adapter is the only one shipped in v0.1; new adapters are separate
//! PRs per the locked plan.

pub mod adapters;
pub mod cli;
pub mod model;
pub mod render;
pub mod transform;

pub use adapters::{ClaudeCode, ConvoAdapter, SessionMeta, WorkspaceScope};
pub use model::{ContentBlock, Conversation, Message, Role};
pub use render::{render_markdown, MarkdownOptions};
pub use transform::{apply_path_rewrite, PathRewrite};
