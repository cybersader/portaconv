//! portaconv — library crate for the terminal-native conversation
//! extractor. Public API is intentionally empty in v0.0.1 (bootstrap
//! only). Modules will land incrementally per the approved plan.
//!
//! See `knowledgebase/03-design-decisions.md` for the locked shape.
//!
//! Planned module layout:
//!   src/model.rs                 — Conversation / Message / ContentBlock
//!   src/schema/openai.rs         — OpenAI Chat Completions (de)serializer
//!   src/adapters/claude_code.rs  — P0 adapter reading ~/.claude/projects/
//!   src/render/markdown.rs       — paste-ready markdown output
//!   src/transform/paths.rs       — WSL ↔ Windows path rewriting
//!   src/mcp/mod.rs               — stdio MCP server
//!   src/cli.rs                   — clap command parsing
