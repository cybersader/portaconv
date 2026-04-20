//! Opt-in content transforms applied to a loaded `Conversation`.
//!
//! v0.1 ships one: OS-path rewriting. This exists because Claude Code
//! conversation JSONLs carry absolute filesystem paths from the
//! authoring OS inside `cwd` fields, tool-call args, and prose — which
//! means a WSL-authored session pastes badly into a Windows-launched
//! agent (and vice versa). The spike measured 464/489 lines of one
//! Windows-encoded session still carrying `/mnt/c/…` references.

pub mod paths;

pub use paths::{apply_path_rewrite, PathRewrite};
