---
title: Quickstart
description: Sixty seconds from install to paste-ready context.
sidebar:
  order: 2
---

Four steps. Each is one command. Skip any you don't need.

## 1. Install

```sh
git clone https://github.com/cybersader/portaconv
cd portaconv
cargo install --path .
```

`pconv` now lives on your `$PATH`. See [Installation](/portaconv/getting-started/installation/)
if you want more detail (requirements, platform notes).

## 2. See your sessions

```sh
pconv list

# Often more useful — only this workspace, last 2 days
pconv list --workspace-toml auto --since 2d

# Or just grab the latest and skip the id-copying step entirely:
pconv dump --latest --workspace-toml auto | clip.exe   # WSL
```

The one-liner at the end is the "I just want yesterday's context back"
shortcut — no need to eyeball a list and copy a UUID. Read on for the
step-by-step view.

Output looks like this (table format, newest first):

```
session-id                             msgs  updated           cwd                                       title
----------------------------------------------------------------------------------------------------------------------------------
2d23322b-40ea-41a3-99ed-9c14569a44b8    453  2026-04-20 13:51  …Workspaces/portaconv                     porting docs from portagenty
97d7b58b-09f5-41ea-a59f-a12f230083b0   8152  2026-04-20 13:25  …/mcp-workflow-and-tech-stack             trying to develop a good workflow…
b1f7edc9-9a75-4c21-9067-4de665cb3d7c   7461  2026-04-20 12:37  …Workspaces/cyberbaser                    need to really initialize this…

75 session(s)
```

Columns: **session-id · msgs · updated · cwd · title**. cwd is
truncated with a leading `…` — projects are usually more recognizable
by their tail segments than their home-dir prefix.

Pick the id you want. It's the first column. (Tip:
`pconv list --workspace-toml auto` scopes to the current portagenty
workspace if you're in one — much shorter list.)

## 3. Dump one as paste-ready markdown

```sh
pconv dump 2d23322b-40ea-41a3-99ed-9c14569a44b8
```

You get markdown like this on stdout:

```markdown
# porting docs from portagenty

- session: `2d23322b-40ea-41a3-99ed-9c14569a44b8`
- cwd: `/mnt/c/Users/.../portaconv`
- started: 2026-04-20 00:13 UTC
- git branch: `main`

## User

let me plan the docs restructure before I touch anything

## Assistant

Sounds good. Let me see what's there first.

**tool call:** `Read`

\`\`\`json
{
  "file_path": "/path/to/docs/src/content/docs/index.mdx"
}
\`\`\`

…
```

That's what an agent wants pasted back as context.

## 4. Pipe it somewhere

```sh
# WSL → Windows clipboard
pconv dump <id> | clip.exe

# macOS
pconv dump <id> | pbcopy

# Linux with Wayland
pconv dump <id> | wl-copy

# Save to the repo
pconv dump <id> > docs/agent-context/2026-04-20-redesign.md
```

Paste into Claude Code (new chat), claude.ai, opencode, or anywhere
you want to continue.

## Crossing the OS boundary

If the session was authored on the other OS, translate paths on the
way out:

```sh
# WSL-authored session → heading into Windows
pconv dump <id> --rewrite wsl-to-win | clip.exe

# Windows-authored session → heading into WSL
pconv dump <id> --rewrite win-to-wsl

# Pasting somewhere with no filesystem at all (claude.ai, API)
pconv dump <id> --rewrite strip
```

See [path rewriting on the homepage](/portaconv/#path-rewriting-by-example)
for concrete before/after pairs.

## Controlling length

Long sessions don't always fit — or shouldn't — in a fresh agent's
context. `--tail N` keeps only the most-recent N messages:

```sh
pconv dump <id> --tail 50                           # last 50 msgs only
pconv dump --latest --workspace-toml auto --tail 30 # latest in workspace, last 30
```

The output is self-documenting about what got dropped:

- **Markdown** gets a header line: `- truncated: last N of T messages (D earlier dropped)`
- **JSON** gets `extensions.truncated = { tail, original_message_count, dropped }` so agents can detect the slice programmatically

Stacks cleanly with the rewrite and thinking/results flags.

## When `/resume` shows the wrong sessions

If Claude Code's own picker is lying — wrong summaries, missing your
active session, showing months-old entries for projects you were in
yesterday — that's usually a stale `sessions-index.json` (upstream
[#25032](https://github.com/anthropics/claude-code/issues/25032)).
`pconv doctor` diagnoses; `pconv rebuild-index` repairs.

```sh
# What's stale on this machine?
pconv doctor

# Fix the project I'm currently in
pconv rebuild-index --project ~/.claude/projects/-mnt-c-Users-…-your-project

# Fix every project whose index is >24h behind its jsonls
pconv rebuild-index --all --lag-threshold-hours 24

# Don't want to wait on a picker rebuild? Dump the stale session as
# paste-ready markdown and continue in a fresh claude session.
pconv doctor --dump-stale
```

Rebuild writes atomically (tempfile + rename) with a dated
`.bak-YYYY-MM-DD` backup by default. The only file touched is
`sessions-index.json` — your `.jsonl` session content is never
modified. See [Commands → `pconv doctor`](/portaconv/reference/commands/#pconv-doctor)
and [`pconv rebuild-index`](/portaconv/reference/commands/#pconv-rebuild-index)
for the full flag surface.

## Let an agent do it for you (MCP)

Instead of shelling out, let an MCP-capable agent call portaconv
directly. Add this to your agent's MCP config (e.g. `~/.claude/mcp.json`):

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

The agent now has three tools: `list_conversations`,
`get_conversation`, and `doctor` (for self-healing when it notices
the picker disagrees with reality). It can pull in prior context on
its own, no copy-paste needed. Full details on the
[agents + portagenty page](/portaconv/concepts/agents-and-portagenty/).

## What's next

- [Commands reference](/portaconv/reference/commands/) — every flag,
  every output format.
- [Concepts](/portaconv/concepts/) — how portaconv thinks about your
  conversations (subagent filter, multi-session files, what survives
  the paste).
- [Agent wiring + usage patterns](/portaconv/concepts/agents-and-portagenty/)
  — four real-world recovery patterns: post-`/compact`, cross-tool
  handoff, committed artifacts, agent-as-curator.
