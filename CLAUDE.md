# CLAUDE.md — portaconv

## What this project is

A **terminal-native conversation extractor + MCP server** for
agent CLIs (Claude Code first). Reads each tool's native
conversation storage, normalizes to OpenAI Chat Completions
format, and emits paste-ready output — optionally with OS-path
rewriting so WSL-authored content can resume on Windows and
vice versa.

Sibling project to [portagenty](https://github.com/cybersader/portagenty)
(workspace launcher) and
[mcp-workflow-and-tech-stack](https://github.com/cybersader/mcp-workflow-and-tech-stack)
(agent-ready scaffold).

## Read before acting

1. **`knowledgebase/04-handoff-context.md`** — your orientation.
   Explains what's already done, what's next, and the constraints
   you must respect. Read this first, every session.
2. **`knowledgebase/03-design-decisions.md`** — the locked
   decisions from the founding Q&A. Don't re-open these without
   explicit user OK.
3. **`knowledgebase/00-proposal-source.md`** + **`01-challenge-source.md`**
   — the original design proposal + the content-layer evidence
   that drove the pivot from sync/bridge to extract/paste.
4. **`knowledgebase/02-agent-research.md`** — competitive landscape
   + schema recommendations from parallel Explore agents.

## Current stage

**v0.0.1 bootstrap.** Repo exists, Cargo skeleton compiles,
workspace TOML is registered with portagenty. **No commands
implemented yet.** Next task: Phase 1 research spike on Claude
Code JSONLs.

## Hard constraints (locked)

These came out of the design Q&A. Don't reopen without user OK.

- **Standalone binary.** `pconv` installs via `cargo install
  portaconv`. Not a pa subcommand.
- **CLI-first, terminal-native.** No GUI, no TUI viewer. Unix-pipe
  semantics.
- **OpenAI Chat Completions** as the canonical schema, with
  Anthropic-style `ContentBlock` variants for tool calls, plus a
  `extensions: serde_json::Value` escape hatch for tool-specific
  fields.
- **Adapter trait per tool.** One small trait; each tool normalizes
  its format to the shared `Conversation` model.
- **Claude Code adapter only in v0.1.** OpenCode / Cursor / Aider
  / continue.dev = separate follow-up PRs.
- **Read-only.** portaconv never writes to any tool's storage.
- **No daemon, no file watching, no auto-sync.** On-demand reads.
- **No path-rewrite by default.** `--rewrite wsl-to-win` etc. is
  opt-in.

## v0.1 command surface

```
pconv list                     # list conversations (Claude Code)
pconv dump <session-id>        # paste-ready markdown to stdout
pconv mcp serve                # stdio MCP server
```

MCP tools: `list_conversations`, `get_conversation`. Each
conversation also exposed as a resource at
`convos://conversation/<id>`.

## Portagenty awareness (future — not v0.1)

Two use cases the user flagged:

1. **Context retrieval for the current session.** Running agent
   can query pconv via MCP to pull relevant prior context.
2. **Folder-move recovery.** pa detects a workspace moved (its
   auto-re-register hook already fires), prompts the user to run
   pconv for bridging conversations tied to the old path.

Both happen as separate PRs on portagenty's side. pconv stays
standalone.

## Working style for this repo

- Mirror [portagenty's](https://github.com/cybersader/portagenty)
  code style: `anyhow::Result`-everywhere, tight `// why` comments
  (never `// what`), no docstring sprawl.
- Tests alongside features. Snapshot tests (via `insta`) for
  renderer output.
- Never commit without the user asking.
- When in doubt about a design direction, surface a tradeoff
  framed in the user's stated principles: **first principles,
  resilient, elegant, future accounting**.

## Where to find the plan

`~/.claude/plans/piped-sauteeing-breeze.md` — the approved
implementation plan. Read-only reference; don't edit.

## Sibling repos (don't touch from here)

- `../portagenty/` — launcher. Workspace `id` field we'll use.
- `../mcp-workflow-and-tech-stack/tools/claudecode-project-sync/`
  — the Docker sync tool we pivoted away from. Gets a banner
  later pointing users at pconv; don't modify it from here.
