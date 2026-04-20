//! Output renderers. The v0.1 default is `markdown` (paste-ready); a
//! raw `json` passthrough is provided by serde on the model directly,
//! so it has no renderer module.

pub mod markdown;

pub use markdown::{render_markdown, MarkdownOptions};
