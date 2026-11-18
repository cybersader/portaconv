# 04 · Handoff context — start here, new session

You are picking up a working, shipped tool. This file is the current
state; the other knowledgebase files are the history that explains
why it looks the way it does.

## What portaconv is

`pconv` recovers Claude Code conversations that the native
`/resume` picker and `claude -r <uuid>` can't reach — moved folders,
cross-OS (WSL ↔ Windows) sessions, stale `sessions-index.json`, and
sessions too large to `/compact`. It reads Claude Code's storage
read-only and emits paste-ready markdown, and it runs as a stdio MCP
server so other agents can pull prior context themselves.

## Who depends on it

- **Agents via MCP.** `list_conversations`, `get_conversation`,
  `doctor`, and the `convos://conversation/<id>` resource. Treat tool
  names, argument names, and output shapes as a public contract:
  renaming or reshaping them breaks callers silently.
- **portagenty.** Workspace scoping reads its `*.portagenty.toml`
  (`id`, `previous_paths`). Future integration lands on portagenty's
  side; pconv stays standalone.
- **Users of the CLI**, documented at
  <https://cybersader.github.io/portaconv/>.

## Current state

- Commands: `list`, `dump`, `doctor`, `rebuild-index`, `handoff`,
  `mcp serve`. `rebuild-index` and `handoff` are CLI-only.
- Claude Code is the only adapter. Others (OpenCode, Cursor, Aider,
  continue.dev) are deliberate follow-up PRs.
- Version `0.1.0` in `Cargo.toml`; no git tag, GitHub release, or
  crates.io publish yet. Install is `cargo install --git`.
- MIT-licensed. `CONTRIBUTING.md` documents the fork → one command →
  PR loop.

## How to verify a change

```sh
sh scripts/check.sh   # fmt, clippy, tests, release build — same as CI
```

Docs site (Astro/Starlight under `docs/`):

```sh
cd docs && bun install --frozen-lockfile && bun run build && bun run smoke:no-build
```

CI runs both on GitHub-hosted runners (Linux + macOS for Rust).
Nothing should depend on a particular developer machine; tests use
fixtures under `tests/fixtures/`, never a real `~/.claude`.

## Read next, in order

1. `03-design-decisions.md` — the locked v0.1 contract. Don't reopen
   without explicit owner OK.
2. `../CLAUDE.md` — hard constraints and working style.
3. `docs/src/content/docs/reference/commands.md` — user-facing CLI
   surface; keep it in sync with `src/cli.rs`.
4. `docs/src/content/docs/reference/adapter-claude-code.md` — the
   record-type contract for the Claude adapter.
5. History, only if you need the why: `00-proposal-source.md`
   (sync → extract pivot), `01-challenge-source.md` (the evidence),
   `02-agent-research.md` (landscape + schema choice).

## When surfacing a tradeoff

Frame it in the owner's principles: first principles (no "we'll just
…" hacks), resilience (format churn shouldn't force adapter
rewrites), elegance (minimum surface, maximum utility), and future
accounting (survives a 2+ year horizon).
