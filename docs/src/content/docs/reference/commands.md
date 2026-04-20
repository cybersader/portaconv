---
title: Commands
description: The pconv CLI surface.
sidebar:
  order: 1
---

:::caution
v0.0.1 bootstrap — none of the commands below are implemented yet.
This page is the intended shape for v0.1. Check the
[repo](https://github.com/cybersader/portaconv) for progress.
:::

## `pconv list`

List conversations discoverable on this machine.

```sh
pconv list
pconv list --tool claude-code
pconv list --since 2d
pconv list --workspace <uuid>    # scope by portagenty workspace id (future)
```

In v0.1 `--tool` always defaults to `claude-code`; other adapters
arrive in later releases.

## `pconv dump <session-id>`

Render one session to stdout.

```sh
pconv dump 01234567-89ab-cdef-0123-456789abcdef
pconv dump <id> --format json
pconv dump <id> --rewrite wsl-to-win     # /mnt/c/… → C:\…
pconv dump <id> --rewrite win-to-wsl     # C:\… → /mnt/c/…
pconv dump <id> --rewrite strip          # remove absolute paths
```

Defaults:

- `--format markdown` — `## User` / `## Assistant` blocks, tool
  calls as fenced code, tool results truncated.
- No path rewriting. Opt in explicitly.
- No trimming. `--tail <N>` and `--max-tokens <N>` planned.

## `pconv mcp serve`

Start a stdio MCP server exposing:

- **Tool** `list_conversations(since?, workspace_id?, limit?)`
- **Tool** `get_conversation(id, format="markdown"|"json", rewrite?)`
- **Resource** template `convos://conversation/<id>`

Add to your agent's `mcp.json`:

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

## Non-goals for v0.1

- No `pconv copy` (clipboard integration — v0.2).
- No `pconv export --to <file>` (committable handoff docs — v0.2).
- No search / FTS / embeddings — get-by-id only.
- No writes back to any tool's storage. Ever. Read-only by design.
