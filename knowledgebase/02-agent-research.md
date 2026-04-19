# 02 · Research synthesis — existing tools + schemas + formats

Three Explore agents ran in parallel on 2026-04-19 to survey the
landscape before committing to portaconv's design. This file is
the distilled findings. Full agent outputs are in the parent
conversation (`04-handoff-context.md`).

## Key findings

### 1. Terminal-native is the gap

The viewing/extraction niche has several solid players:

| Tool | Shape | Active | Notes |
|---|---|---|---|
| `d-kimuson/claude-code-viewer` | Web GUI | Yes | Real-time log tailing. Claude-only. |
| `jhlee0409/claude-code-history-viewer` | Desktop GUI + headless server | Yes | **Multi-tool viewer** — reads Claude, Cursor, Cline, Aider, Codex, Gemini, OpenCode. Closest competitor. |
| `raine/claude-history` | TUI (fuzzy search) | Yes | Rust. Field-aware indexing. Closest-in-feel to our CLI plans. |
| `kvsankar/claude-history` | Python CLI | Stable | Multi-env (WSL, SSH) workspace aliasing. Strips UNC prefixes on read. |
| `cclog` | JSONL → MD converter | Recent | Optional TUI mode. |
| `claude-mem` | Context compression | Active | RAG-style injection. Schemas its own memory format. |
| `claude-code-soul` | Identity persistence (SOUL.md) | Lightweight | 200 LOC bash. Not an extractor. |
| `ContextPool` | Insight extraction + MCP server | Emerging | Extracts decisions/bugs from JSONL. |
| `agsoft` VS Code ext | Sidebar | Unclear | VS Code only. |

**The unclaimed territory**: CLI-first, Unix-pipe-friendly,
path-content-aware extractor + MCP server. No existing tool does
content-level path rewriting. That's our wedge.

### 2. OpenAI Chat Completions is the de facto schema

Schema landscape confirmed:

| Schema | Shape | Tool calls |
|---|---|---|
| **OpenAI Chat Completions** | `{role, content}[]` | `tool_calls` nested in assistant msg |
| **Anthropic Messages** | `{role, content: Block[]}` | `tool_use` as a content-block type |
| **LangChain BaseMessage** | `{type, content, additional_kwargs, metadata}` | in `additional_kwargs` |
| **Claude Code JSONL** | Event stream `{type, message, parentUuid, timestamp}` | nested in `message.content` |

Convergence: `(role, content)` tuple is universal. OpenAI format is
widely read/written. Anthropic's content-blocks preserve tool calls
cleanly. IETF vCon (draft-ietf-vcon-vcon-core) is the only real
standards-body effort — too broad (phone/video/chat) for agent use.

**Decision**: adopt OpenAI Chat Completions shape with Anthropic-style
`ContentBlock` variants for tool_use / tool_result. Carry
tool-specific oddities in `extensions: serde_json::Value`.

### 3. Cross-tool conversation storage layouts

| Tool | Storage | Format | Binary? |
|---|---|---|---|
| **Claude Code** | `~/.claude/projects/<encoded-cwd>/*.jsonl` | JSONL append-only event stream | Text |
| **Cursor** | `~/.config/Cursor/User/globalStorage/*.vscdb` | SQLite | Binary (needs `rusqlite` etc.) |
| **Aider** | `.aider.chat.history.md` (project root) | Markdown | Text |
| **continue.dev** | `.continue/dev_data/*` (unclear) | Unknown | Unknown |
| **OpenCode** | `~/.local/share/opencode/storage/` | SQLite + JSON | Mixed |

**Common denominators across all 5**: message content, role/type,
timestamp, session ID, message ID. Optional-but-frequent: cwd, tool
invocations.

**Hardest adapter** (for future): Cursor — binary SQLite, keys
reverse-engineered from community projects, composite-bubble nesting.
If we ever implement it, the adapter trait will be honest.

**Easiest after Claude**: Aider (plain Markdown, single file per
project). Good second adapter for when we generalize.

### 4. MCP memory server landscape

Existing servers:
- **Pieces LTM MCP** (production): 39 tools, SQLite FTS5 + semantic
  vectors. Heavy.
- **claude-memory-mcp, claude-mem**: Claude-specific memory wrappers.

**No shared MCP schema** for "conversation" as a resource. We get
to invent ours. Minimal is `list_conversations` + `get_conversation`
tools plus an `convos://conversation/<id>` resource URI template.

### 5. Conversation-as-commit-artifact has prior art

- **Aider** already writes `.aider.chat.history.md` in the project
  root — prior art for committable convo records.
- **Cloudflare Artifacts**, **Playbase**, **GitAgent** — commit-as-
  agent-state is an emerging pattern.
- GitHub's `agents.md` convention is surfacing.

Means: `pconv dump --to docs/agent-context/X.md && git add` isn't
inventing anything new. We're normalizing an existing pattern.

## Recommendations adopted

| Question | Recommendation | Status |
|---|---|---|
| Internal schema | OpenAI Chat Completions + extensions bag | ✅ Adopted |
| MCP server in v0.1? | Yes, minimal (`list_conversations` + `get_conversation`) | ✅ Adopted |
| OpenCode/Cursor/Aider adapters in v0.1? | No. Claude Code only for v0.1. | ✅ Adopted |
| GUI viewer scope | No. jhlee0409's viewer covers that niche well. | ✅ Adopted |
| Path rewriting | Our unique value. Opt-in transform. | ✅ Adopted |
| Search / FTS / embeddings | v0.2+. Not in v0.1. | ✅ Adopted |

## Sources (full links)

### Claude history tools
- https://github.com/d-kimuson/claude-code-viewer
- https://github.com/jhlee0409/claude-code-history-viewer
- https://github.com/raine/claude-history
- https://github.com/kvsankar/claude-history
- https://github.com/thedotmack/claude-mem
- https://github.com/israelmirsky/claude-code-soul

### Schema references
- https://platform.openai.com/docs/api-reference/chat/create
- https://docs.anthropic.com/en/api/messages
- https://datatracker.ietf.org/doc/draft-ietf-vcon-vcon-core/

### Cross-tool / memory
- https://github.com/MemPalace/mempalace
- https://pieces.app/blog/mcp-memory
- https://blog.cloudflare.com/artifacts-git-for-agents-beta/

### Claude Code upstream issues (tracking)
- https://github.com/anthropics/claude-code/issues/17682 — cross-env sync
- https://github.com/anthropics/claude-code/issues/9668 — WSL duplicate titles
- https://github.com/anthropics/claude-code/issues/9306 — project-local storage
