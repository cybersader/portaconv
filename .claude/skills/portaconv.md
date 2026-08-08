---
name: portaconv
description: This workspace is wired to portaconv (`pconv`) — a terminal-native conversation extractor + MCP server for Claude Code history. Use it whenever the user asks to read, re-paste, or bridge prior-session context.
---

# portaconv

Portaconv (binary `pconv`) normalizes Claude Code's on-disk JSONL
conversation history into paste-ready markdown or MCP resources, and
rewrites WSL/Windows absolute paths baked into content so pasted
context lands coherently on whatever host is replying.

## Two interfaces, same data

- **MCP (preferred)** — `.mcp.json` registers the `portaconv` server.
  Tools: `list_conversations(since, workspace_id?, limit)` and
  `get_conversation(id, format, rewrite?)`. Each conversation is
  also exposed as a resource at `convos://conversation/<id>`.
- **CLI** — shell out via `pa convos ...` (a workspace-aware shim
  over `pconv`):
  - `pa convos list` — list conversations scoped to this workspace
  - `pa convos dump <id>` — emit paste-ready markdown
  - `pa convos dump <id> --rewrite wsl-to-win` — normalize paths

## Workspace scoping

`pa convos` injects `--workspace-toml <path>` so pconv sees only
conversations whose `cwd` prefix matches this workspace's
`projects` (plus any `previous_paths` — see the
`portagenty-workspace` skill).

## When to use

- User asks "what did we do before"/"show me the last session".
- User wants a transcript to paste into a new session / another
  agent CLI.
- Agent needs prior context to avoid re-asking the human.
- Folder was moved and old sessions aren't showing — check that
  `previous_paths` is populated in the workspace TOML (portagenty
  maintains this automatically on walk-up re-registration).
