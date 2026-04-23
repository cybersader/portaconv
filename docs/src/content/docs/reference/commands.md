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

# Explicit backing-file override — manually pick which duplicate
pconv dump <id> --file <path>                    # bypass corpus walk, load this exact JSONL
pconv dump --file <path>                         # if file has a single session, id is optional
pconv dump --latest --file <path>                # newest session within this file

# Machine format
pconv dump <id> --format json
```

### Manually selecting among duplicate sessionIds

When a workspace has been opened from both WSL and Windows (or the folder has
moved), the same `sessionId` can exist in two physical JSONLs — one per
encoded-path bucket under `~/.claude/projects/`. `pconv dump <id>` picks one
automatically (basename-stem match first, then largest file), which is usually
right but occasionally not. Use `--file <path>` to override:

```sh
# Discover the paths for a duplicated id
pconv list --show-duplicates --format json | jq '.[] | select(.id == "<id>") | .source_path'

# Load the one you actually want
pconv dump <id> --file "/home/you/.claude/projects/C--your-project/<id>.jsonl"
```

`--file` conflicts with `--workspace-toml` (workspace scope applies to the
corpus walk, which `--file` bypasses by design). If the file contains multiple
sessions (common after `/compact`), pair `--file` with a positional `<id>` or
`--latest` to pick one; otherwise an error lists the ids available in the file.

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

## `pconv doctor`

Diagnose stale `sessions-index.json` files across `~/.claude/projects/`.
Read-only. The `/resume` picker reads this index, and it goes stale when
Claude Code is closed ungracefully (WSL force-close, `wsl --shutdown`,
machine suspend) — upstream [#25032](https://github.com/anthropics/claude-code/issues/25032)
and siblings. Doctor flags projects where the index is missing or lagging
the actual `.jsonl`s so you know which ones to repair.

```sh
pconv doctor                                # table of stale projects, newest first
pconv doctor --format json                  # machine-readable, good for scripting + MCP
pconv doctor --project <absolute-path>      # scope to one project dir
pconv doctor --stale-threshold-hours 24     # default — missing index is always stale
```

**What "stale" means:** `sessions-index.json` is either absent, OR its
mtime lags the newest non-subagent `.jsonl` in the same project dir by
more than `--stale-threshold-hours` (default 24). Missing index is a
special case — it's always reported, threshold ignored.

**Output columns (table):** `project · lag · newest session · size`. Lag
is shown as `MISSING` if the index is absent, otherwise `Nh` (up to
48 hours) or `Nd` (beyond). "Newest session" is the first `sessionId`
found in the newest jsonl — copy it to `claude -r <uuid>` for instant
picker-bypass recovery.

### Paste-ready handoff for stale projects

```sh
pconv doctor --dump-stale
```

Prints the staleness table, then for each stale project: an HTML-comment
divider naming the project, followed by a paste-ready markdown dump of
the newest session (default renderer options — thinking hidden, tool
results truncated). Projects are separated by `---` dividers. Copy the
block you care about into a fresh `claude` session to recover the
context without waiting on the picker.

This is the **escape hatch** — use it when rebuild doesn't apply
(cross-OS case) or when the jsonl is so large that continuing in place
is more painful than starting fresh.

### MCP surface

`doctor { project?, stale_threshold_hours? }` — same rule as the CLI.
Returns a JSON array of stale-project records with fields
`project_dir`, `index_mtime_ms`, `newest_jsonl`, `newest_jsonl_mtime_ms`,
`newest_jsonl_size_bytes`, `lag_hours` (`null` when missing), `missing`,
`newest_session_id`. Agents can use this to self-heal: call `doctor`
before `get_conversation` to pick the right session explicitly.

## `pconv rebuild-index`

Rewrite `sessions-index.json` from the actual `.jsonl` content. This
is what you reach for when you want the native `/resume` picker
working correctly again, rather than switching to paste-based recovery.

```sh
# Rebuild one project
pconv rebuild-index --project /home/you/.claude/projects/-mnt-c-Users-…-proj

# Rebuild everything under ~/.claude/projects/
pconv rebuild-index --all

# Only rebuild stale projects (pairs with doctor's threshold)
pconv rebuild-index --all --lag-threshold-hours 168   # weekly-cron-friendly

# See what would change, write nothing
pconv rebuild-index --all --dry-run

# Skip the dated backup (default keeps sessions-index.json.bak-YYYY-MM-DD)
pconv rebuild-index --all --no-backup
```

**Write path:** serialized JSON goes to a sibling tempfile (`.sessions-index.json.tmp`);
`std::fs::rename` atomic-replaces the target. If `--no-backup` isn't
set, the pre-rebuild index is first copied to
`sessions-index.json.bak-<YYYY-MM-DD>` so a regrettable rebuild is
recoverable.

**What's reconstructed per session:**

| Field | Source |
|---|---|
| `sessionId` | Scanned from JSONL records |
| `fullPath`, `fileMtime` | Filesystem metadata |
| `firstPrompt` | First 200 chars of the first `"user"` message text |
| `messageCount` | Count of `user` + `assistant` lines |
| `created`, `modified` | First + last JSONL timestamps (fallback file times) |
| `projectPath` | First observed `cwd` in the JSONL |
| `gitBranch` | Left empty — not reconstructed (upstream populates; we don't run `git` against the project) |
| `isSidechain` | `false` (not detected; subagent JSONLs filtered by filename) |
| `customTitle`, `summary` | Omitted unless observed in the source records |

**Gotchas:**

- `--project` expects an absolute path to an existing encoded dir
  under `~/.claude/projects/`. A typo or missing path hard-errors
  rather than silently succeeding with "0 rebuilt" — makes the
  mistake visible.
- `--project` and `--all` conflict.
- `--project` mode ignores `--lag-threshold-hours` — if you
  explicitly name a project, you get a rebuild.
- Subagent JSONLs (`agent-*.jsonl` at the project root, or files
  under any `subagents/` subdir) are filtered, matching upstream
  Claude Code behavior. If the original index tracked N sessions,
  your rebuild will match N.

**Round-trip check:** `pconv rebuild-index --all && pconv doctor`
should report zero stale projects on the same run.

**Not exposed via MCP** (deliberately). Write operations via
agent-callable tools risk unintended fan-out. Keep rebuilds as
human-invoked CLI actions.

### Routine maintenance

Cron snippet for keeping the picker honest:

```crontab
# Monday 09:00 — rebuild any project whose index lags by more than a week
0 9 * * 1  pconv rebuild-index --all --lag-threshold-hours 168
```

## `pconv mcp serve`

Stdio MCP server speaking JSON-RPC 2.0 (protocol version `2024-11-05`).
Line-delimited framing — one JSON object per line on stdin/stdout.

**Tools:**

- `list_conversations { min_messages?, show_duplicates?, workspace_toml?, since?, sort?, reverse?, limit?, grep? }` — same surface as `pconv list`.
- `get_conversation { id?, latest?, workspace_toml?, format?, rewrite?, include_thinking?, full_results?, tail?, file? }` — same surface as `pconv dump`. Pass `latest: true` (optionally with `workspace_toml`) to resolve the most recent session in scope without a prior `list_conversations` call. `tail: N` keeps only the most-recent N messages and records the drop count. `file: "<path>"` bypasses the corpus walk and loads from that specific JSONL — the manual-selection escape hatch for duplicate sessionIds (`file` and `workspace_toml` conflict).
- `doctor { project?, stale_threshold_hours? }` — same surface as `pconv doctor`. Returns a JSON list of stale-project records. Agents can call this for self-healing: detect staleness, pick a session, then call `get_conversation` to pull the paste-ready markdown.

Not exposed (deliberately): `rebuild-index`. Write ops stay CLI-only.

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
- **No writes to `.jsonl` session content.** Ever. `rebuild-index` writes to `sessions-index.json` only (the metadata file), atomically with a dated backup. The conversation `.jsonl`s themselves stay untouched.
- No auto-spawn of a fresh `claude` session after `doctor --dump-stale` — you paste manually into whatever terminal is in front of you.
- `rebuild-index` not exposed via MCP. Write ops through agent-callable tools risk unintended fan-out; kept CLI-only in v0.1.
