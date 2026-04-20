---
title: Commands
description: The pconv CLI surface.
sidebar:
  order: 1
---

:::note
All three v0.1 commands are implemented: `pconv list`, `pconv dump`
(markdown + json, path rewriting, thinking/result flags), and
`pconv mcp serve` (stdio JSON-RPC 2.0, two tools + one resource
template). See the [agents + portagenty page](/portaconv/concepts/agents-and-portagenty/)
for wiring it into Claude Code / opencode.
:::

## `pconv list`

List conversations discoverable on this machine. Surfaces each
distinct sessionId — a single JSONL may hold several when `/compact`
writes a continuation to the same file.

```sh
pconv list                          # table by default
pconv list --format json
pconv list --min-messages 5         # filter chatty-short placeholder sessions
```

Output columns (table format): `session-id · msgs · updated · title`.
Title is derived from the first user message. Subagent JSONLs are
filtered by path pattern and do not appear in the list — see the
[concepts page](/portaconv/concepts/#subagent-filter) for the rule.

## `pconv dump <session-id>`

Render one session to stdout. Markdown is the default (paste-ready);
use `--format json` for the raw normalized model.

```sh
pconv dump 01234567-89ab-cdef-0123-456789abcdef

# Markdown knobs
pconv dump <id> --include-thinking        # show assistant reasoning blocks
pconv dump <id> --full-results            # emit full tool-result bodies
pconv dump <id> --include-system-events   # append the system_events section

# Path rewriting — content only, not cwd metadata
pconv dump <id> --rewrite wsl-to-win      # /mnt/c/… → C:\…
pconv dump <id> --rewrite win-to-wsl      # C:\… → /mnt/c/…
pconv dump <id> --rewrite strip           # replace absolute paths with <path>

# Machine format
pconv dump <id> --format json
```

### Markdown output shape

Each message becomes a heading (`## User` / `## Assistant`) followed by:

- plain text (rendered verbatim)
- tool calls as **tool call:** label + a fenced `json` block with pretty-printed input
- tool results as **tool result:** label + a fenced code block (truncated to 600 chars by default; `--full-results` to expand)
- thinking blocks hidden by default (`--include-thinking` to show)
- unknown content blocks surface an HTML comment + inline note rather than silently dropping

### Rewrite scope

| Rewritten | Left alone |
|---|---|
| `ContentBlock::Text.text` (prose) | `Conversation.cwd` (authoring env metadata) |
| `ContentBlock::ToolUse.input` (all strings inside the JSON, recursively) | `Message.extensions` (adapter-preserved raw bits) |
| `ContentBlock::ToolResult.output` (result bodies) | `ContentBlock::ToolUse.id` / `tool_use_id` (opaque handles) |
|  | `ContentBlock::Thinking.text` (internal reasoning) |

Windows → WSL regex is bounded to avoid URL-scheme false positives
(`https://` doesn't match `s://`). WSL → Windows only matches the
`/mnt/<letter>/…` form, so bare Linux paths like `/home/alice/…`
stay untouched.

## `pconv mcp serve`

Stdio MCP server speaking JSON-RPC 2.0 (protocol version `2024-11-05`).
Line-delimited framing — one JSON object per line on stdin/stdout.

**Tools:**

- `list_conversations { min_messages?, show_duplicates?, workspace_toml? }` — same surface as `pconv list`.
- `get_conversation { id, format?, rewrite?, include_thinking?, full_results? }` — same as `pconv dump`.

**Resources:** one URI template `convos://conversation/{id}`.
`resources/read` returns the session rendered as markdown with default options.

Wiring into an MCP client (`~/.claude/mcp.json` or equivalent):

```json
{
  "mcpServers": {
    "portaconv": {
      "command": "pconv",
      "args": ["mcp", "serve"]
    }
  }
}
```

See [agents + portagenty](/portaconv/concepts/agents-and-portagenty/) for
usage patterns (post-compact recovery, cross-tool handoff, committed
recovery artifacts).

## Non-goals for v0.1

- No `pconv copy` (clipboard integration — v0.2). Pipe to `clip.exe` / `pbcopy` / `wl-copy` yourself.
- No `pconv export --to <file>` (committable handoff docs — v0.2). Redirect stdout.
- No search / FTS / embeddings — get-by-id only.
- No writes back to any tool's storage. Ever. Read-only by design.
