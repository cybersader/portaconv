//! Stdio MCP server.
//!
//! Minimal handcrafted JSON-RPC 2.0 implementation — no rmcp dependency.
//! Stdio MCP is line-delimited JSON-RPC; each message is one JSON object
//! on one line. We read lines from stdin, dispatch, write responses to
//! stdout, and exit on EOF.
//!
//! Exposed surface (protocol version `2024-11-05`):
//!
//!   tools/
//!     list_conversations     — lists sessions (workspace/since/min-messages)
//!     get_conversation       — returns markdown or JSON for one session
//!
//!   resources/
//!     convos://conversation/<id>   — read a session as markdown
//!
//! Errors follow JSON-RPC conventions (-32700 parse, -32600 invalid
//! request, -32601 method not found, -32602 invalid params, -32603
//! internal). Anything outside the known methods returns -32601 —
//! MCP clients MUST treat that as "not supported" and carry on.

pub mod server;

pub use server::run_stdio_server;
