# CLAUDE.md — portaconv

## What this project is

A **terminal-native conversation extractor + MCP server** for
agent CLIs (Claude Code first). Reads each tool's native
conversation storage, normalizes to OpenAI Chat Completions
format, and emits paste-ready output — optionally with OS-path
rewriting so WSL-authored content can resume on Windows and
vice versa.

Since v0.1.0, also **diagnoses and repairs stale
`sessions-index.json`** files — the metadata file Claude Code's
`/resume` picker reads, which goes stale on ungraceful shutdowns
(upstream [#25032](https://github.com/anthropics/claude-code/issues/25032)).
Repair sits naturally alongside extract because the jsonl walker
is already there.

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
5. **`docs/src/content/docs/reference/commands.md`** — the
   current CLI surface as documented to users. Keep in sync
   with `src/cli.rs`.

## Current stage

**v0.1.0 — four commands live.** Core model, Claude Code adapter,
markdown renderer, path-rewrite transforms, stdio MCP server, and
sessions-index diagnostics + rebuild all shipped. 84 tests pass
(29 unit / 37 CLI integration / 18 MCP).

## Hard constraints (locked)

These came out of the design Q&A. Don't reopen without user OK.

- **Standalone binary.** `pconv` installs via `cargo install
  --git https://github.com/cybersader/portaconv`. Not a pa
  subcommand.
- **CLI-first, terminal-native.** No GUI, no TUI viewer. Unix-pipe
  semantics.
- **OpenAI Chat Completions** as the canonical schema, with
  Anthropic-style `ContentBlock` variants for tool calls, plus a
  `extensions: serde_json::Value` escape hatch for tool-specific
  fields.
- **Adapter trait per tool.** One small trait; each tool normalizes
  its format to the shared `Conversation` model.
- **Claude Code adapter only through v0.1.** OpenCode / Cursor /
  Aider / continue.dev = separate follow-up PRs.
- **Read-only by default; writes are explicit and rare.**
  `rebuild-index` is the only writing subcommand. It writes
  atomically (tempfile + rename) with a dated `.bak-YYYY-MM-DD`
  backup by default. The "safe to run, can't break your Claude
  state" contract still holds — it only touches
  `sessions-index.json`, never the `.jsonl` session content.
- **`rebuild-index` is CLI-only.** Not exposed via MCP in v0.1 —
  write ops through agent-callable tools risk unintended fan-out.
- **No daemon, no file watching, no auto-sync.** On-demand reads.
- **No path-rewrite by default.** `--rewrite wsl-to-win` etc. is
  opt-in.

## v0.1 command surface

```
pconv list                     # list conversations (Claude Code)
pconv dump <session-id>        # paste-ready markdown to stdout
pconv doctor                   # detect stale sessions-index.json
pconv doctor --dump-stale      # + paste-ready markdown for stale projects
pconv rebuild-index --all      # rewrite sessions-index.json from the .jsonls
pconv mcp serve                # stdio MCP server
```

**MCP tools exposed:** `list_conversations`, `get_conversation`,
`doctor`. Each conversation also exposed as a resource at
`convos://conversation/<id>`. `rebuild-index` deliberately not
exposed via MCP.

## Failure modes addressed

Three distinct `/resume` failure modes; one tool covers all three:

| Failure | Primitive |
|---|---|
| Cross-OS content poisoning (paths baked into jsonl) | `pconv dump --rewrite {wsl-to-win\|win-to-wsl}` |
| Folder moved (portagenty workspace) | `pconv list --workspace-toml auto` (reads `previous_paths`) |
| Stale `sessions-index.json` (ungraceful shutdown skipped the rewrite) | `pconv doctor` + `pconv rebuild-index` |

The first two use the extractor; the third uses the same jsonl
walker to repair the native index. Zero new infrastructure per
failure mode after the jsonl walker existed.

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
  renderer output. `assert_fs::TempDir` + `filetime` for
  fixture-copy-and-mutate tests (see `tests/cli.rs` for the
  `fresh_fixture_clone()` pattern used by every rebuild test).
- **Never commit without the user asking.** Keep `.claude/` and
  `.mcp.json` scaffolding in the tree (they help anyone cloning
  the repo wire up the MCP server + agent hooks).
- **No AI attribution in commits, PRs, tags, or docs.** Never add
  `Co-Authored-By: Claude`, "Generated by/with Claude", or any
  equivalent for other AI tools. Enforced by global `commit-msg`
  hook on this machine.
- When in doubt about a design direction, surface a tradeoff
  framed in the user's stated principles: **first principles,
  resilient, elegant, future accounting**.

## Sibling repos (don't touch from here)

- `../portagenty/` — launcher. Workspace `id` field we'll use.
- `../mcp-workflow-and-tech-stack/tools/claudecode-project-sync/`
  — the Docker sync tool we pivoted away from. Already has a
  "superseded" banner + "Related tools" table pointing at
  portaconv; don't modify it from here.
- `../mcp-workflow-and-tech-stack/02-stack/patterns/claude-code-session-recovery.md`
  — user-facing practitioner pattern with the decision tree across
  `claude -r <uuid>` / `pconv rebuild-index` / `pconv doctor`. If
  portaconv's UX changes, this doc's cross-refs need a look.
