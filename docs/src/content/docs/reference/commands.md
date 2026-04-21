---
title: Commands
description: The pconv CLI surface.
sidebar:
  order: 1
---

:::tip
New here? The [60-second quickstart](/portaconv/getting-started/quickstart/)
walks through the whole loop (install → list → dump → paste) before
you dig into flag-by-flag detail.
:::

## `pconv list`

List conversations discoverable on this machine. Surfaces each
distinct sessionId — a single JSONL may hold several when `/compact`
writes a continuation to the same file.

```sh
pconv list                                  # table (default), newest first
pconv list --format json
pconv list --min-messages 5                 # hide short placeholder sessions
pconv list --workspace-toml auto            # scope to current portagenty workspace
pconv list --show-duplicates                # keep WSL- + Windows-encoded copies
```

`--workspace-toml` reads both `projects` and `previous_paths` from the
TOML; the latter is how a moved workspace keeps surfacing sessions from
its pre-move cwd without any out-of-band state.

### Filtering, sorting, limiting

```sh
# Time window — relative or absolute
pconv list --since 2d                       # last 2 days
pconv list --since 6h
pconv list --since 2026-04-01               # from a date

# Substring search on title + cwd (NOT full content)
pconv list --grep react
pconv list --grep /work/api

# Sort by column; use --reverse to flip direction
pconv list --sort msgs                      # chattiest first
pconv list --sort title                     # A → Z
pconv list --sort started --reverse         # oldest first

# Cap output
pconv list --limit 10

# Compose: last week, containing "refactor", in the current workspace, top 5
pconv list --workspace-toml auto --since 7d --grep refactor --limit 5
```

**Sort keys:** `updated` (default), `started`, `msgs`, `title`, `id`.
Time and count keys default to newest/biggest first; `title` and `id`
default to ascending alphabetic. `--reverse` flips whichever default
applies.

**Output columns (table):** `session-id · msgs · updated · cwd · title`.
Title is derived from the first user message; cwd is truncated with a
leading `…` when long (the project's tail is usually more
recognizable than its home-dir prefix).

Subagent JSONLs are filtered by path pattern and do not appear in
the list — see the
[concepts page](/portaconv/concepts/#subagent-sessions-are-filtered)
for the rule.

## `pconv dump [<session-id>]`

Render one session to stdout. Markdown is the default (paste-ready);
use `--format json` for the raw normalized model.

```sh
pconv dump 01234567-89ab-cdef-0123-456789abcdef

# Skip the list + copy step entirely
pconv dump --latest                              # most recent session on this machine
pconv dump --latest --workspace-toml auto        # most recent in this workspace
pconv dump --latest --rewrite wsl-to-win         # compose with rewrite for cross-OS paste

# Markdown knobs
pconv dump <id> --include-thinking               # show assistant reasoning blocks
pconv dump <id> --full-results                   # emit full tool-result bodies
pconv dump <id> --include-system-events          # append the system_events section

# Path rewriting — content only, not cwd metadata
pconv dump <id> --rewrite wsl-to-win             # /mnt/c/… → C:\…
pconv dump <id> --rewrite win-to-wsl             # C:\… → /mnt/c/…
pconv dump <id> --rewrite strip                  # replace absolute paths with <path>

# Length control — keep only the most-recent N messages
pconv dump <id> --tail 50                        # last 50 msgs; earlier ones dropped
pconv dump --latest --workspace-toml auto --tail 30
# (the output records how many were dropped: markdown header line,
# or JSON extensions.truncated = { tail, original_message_count, dropped })

# Machine format
pconv dump <id> --format json
```

`--latest` and a positional `<session-id>` are mutually exclusive.
`--latest` alone surfaces the most recent session on the whole
machine; combined with `--workspace-toml` it scopes to that
workspace. The portagenty one-liner is:

```sh
pconv dump --latest --workspace-toml auto | clip.exe
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

- `list_conversations { min_messages?, show_duplicates?, workspace_toml?, since?, sort?, reverse?, limit?, grep? }` — same surface as `pconv list`.
- `get_conversation { id?, latest?, workspace_toml?, format?, rewrite?, include_thinking?, full_results?, tail? }` — same surface as `pconv dump`. Pass `latest: true` (optionally with `workspace_toml`) to resolve the most recent session in scope without a prior `list_conversations` call. `tail: N` keeps only the most-recent N messages and records the drop count.

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
- No full-content search — `--grep` is substring-over-title-and-cwd only. Full-content FTS stays deferred (separate verb if / when it lands).
- No writes back to any tool's storage. Ever. Read-only by design.
