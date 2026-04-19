# 04 · Handoff context — start here, new session

Hello next Claude session. You're picking up from a design
conversation that ran in a different workspace (portagenty).
The session reached a locked plan + a seeded knowledgebase.
Your first job is to read the knowledgebase in order, not to
ask me to re-explain everything.

## Read these files before acting

In order:

1. `00-proposal-source.md` — the original design proposal from
   portagenty. Explains why we pivoted from sync/bridge to
   extract/paste. Authoritative on problem framing.

2. `01-challenge-source.md` — the companion challenge doc from
   the mcp-workflow-and-tech-stack sibling repo. Evidence
   section (§ "Concrete evidence 2026-04-19") is the smoking
   gun that killed the sync approach.

3. `02-agent-research.md` — synthesis of the parallel Explore
   agent runs. Landscape of existing tools, schema convergence,
   format catalog per-tool. The "Recommendations adopted" table
   is what we're building.

4. `03-design-decisions.md` — the Q&A-locked decisions. This is
   the contract for v0.1. Don't deviate without asking.

5. `../CLAUDE.md` (parent dir) — short orientation + conventions
   for this repo.

## What's done (you don't need to redo these)

- Repo created (`git init`, main branch)
- Cargo.toml scaffolded with binary `pconv` + lib `portaconv`
- Bare `src/main.rs` + `src/lib.rs` stubs
- Workspace TOML (`portaconv.portagenty.toml`) with stable `id`
- Sessions declared: `shell`, `claude`, `tests`
- This knowledgebase seeded with the 5 context files
- Plan locked at `~/.claude/plans/piped-sauteeing-breeze.md`
  (read-only reference; copy-paste it if useful)

## What's next (Phase 1 of the plan)

**Research spike before committing to the adapter trait.**

1. Read 3–5 real Claude Code JSONLs of varying size. Find them
   at `~/.claude/projects/*/*.jsonl`. The smallest JSONLs are
   good first reads; save the big ones for later.

2. Enumerate every record `type` seen. Catalog:
   - core message roles (user, assistant, system)
   - tool_use / tool_result blocks
   - metadata records (file-history-snapshot, summary, etc.)
   - any unusual records — log them, don't trust they don't
     exist

3. For each record type, decide:
   - Does it map to a `ContentBlock` in our schema?
   - Is it Conversation-level metadata (goes in `extensions`)?
   - Is it ignorable for v0.1?

4. Write a throwaway 20-line Rust program that parses one JSONL
   and prints `(role, content_chars, tool_use_count,
   tool_result_count)` per message. Confirm the event-stream
   shape matches your mental model before committing types.

5. Output: `docs/adapter-notes-claude-code.md` — freezes what
   the Claude adapter handles in v0.1, explicitly lists what
   it skips, enumerates observed record types.

Once that's done, move to Phase 2 (core model + adapter + CLI).

## Constraints you must respect

- **No GUI / TUI viewer.** CLI-first only. Unix pipe semantics.
- **No daemon, no file watching.** On-demand reads.
- **No path-rewrite by default.** Opt-in transform.
- **Claude Code adapter only in v0.1.** OpenCode/Cursor/Aider
  wait for separate PRs.
- **OpenAI Chat Completions schema.** Don't invent a new format;
  extensions bag captures tool-specific fields.
- **No pa integration in v0.1.** Standalone binary. pa shim is
  a future PR on portagenty's side.
- **`unwrap`/`expect` only at top-level binary — library code
  uses `anyhow::Result` everywhere.**

## Sibling-repo awareness

Three related repos in `1 Projects, Workspaces/`:

- **portagenty** — the launcher that inspired this. Already has
  the workspace `id` (UUIDv4) field that future pa-integration
  will lean on. Don't edit it.

- **mcp-workflow-and-tech-stack** — has
  `tools/claudecode-project-sync/` (Python Docker sync — the
  approach we pivoted away from). Will eventually get a README
  banner pointing users at pconv. Don't touch it yet.

- **portaconv** (us) — sibling of the above. Lives at
  `/mnt/c/Users/Cybersader/Documents/1 Projects, Workspaces/portaconv`.

## How to work in this session

1. Start by reading the five knowledgebase files end-to-end.
2. Then ask me a single targeted question: "Ready to start the
   Phase 1 research spike — want me to enumerate Claude JSONL
   record types first, or start with the throwaway Rust parser?"
3. Don't ask about decisions already locked in 03-design-
   decisions.md. If something there is ambiguous, flag it
   explicitly as a clarification rather than a re-open.
4. Match portagenty's code style — see any `.rs` file in the
   sibling portagenty repo for reference (anyhow-first, tight
   comments explaining the *why*, no docstring bloat).

## Known open items from the plan

These aren't blockers but will come up:

- **MCP crate choice** — rmcp (Anthropic official) vs community
  alternatives. Decide when starting Phase 2d.
- **`--rewrite` regex edge cases** — paths with spaces, UNC paths,
  paths inside code blocks. Spec + snapshot tests.
- **Tool result truncation strategy** — last N / first N / lines
  preserved. Pick during implementation.
- **`cargo search portaconv` / `cargo search pconv`** — confirm
  neither name is taken on crates.io before publishing.

## If you need to escalate to the user

The user values:
1. First principles — no "well we'll just..." hacks
2. Resilience — tool format churn should not require adapter
   rewrites
3. Elegance — minimum surface for maximum utility
4. Future accounting — every decision should survive a 2+ year
   horizon

When surfacing a tradeoff, frame it in those terms.

## Final note

The fact that you're reading this means the paste-first recovery
model works. This file was written in a Claude session A; you
(session B) are now continuing with full context without needing
A to be alive. That's the tool we're building, validated by
existence.

Good luck. Start with the JSONLs.
