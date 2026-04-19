---
title: "02 · Claude Code conversation fragmentation — and cross-tool context extraction"
description: Conversation history is fragmented across WSL/Windows path encodings, and the content itself contains OS-specific absolute paths that no file-sync can fix. Pivoted from "sync/bridge" to "paste-ready extractor" — owned by portagenty via pluggable per-tool adapters. This challenge drives the remaining investigation.
date created: 2026-04-19
date modified: 2026-04-19
tags:
  - /meta
  - research
  - challenges
  - claude-code
  - opencode
  - wsl
  - portability
  - conversation-history
  - adapters
status: design-in-progress
priority: high
source: 2026-04-19 direct encounter while testing workflow on cynario project
related:
  - "[[../../tools/claudecode-project-sync/README|claudecode-project-sync (the approach we are stepping away from)]]"
  - "[[../../../../portagenty/PROPOSAL-claude-code-history-bridge.md|portagenty PROPOSAL — pa convos extractor]]"
---

# 02 · Claude Code conversation fragmentation — and cross-tool context extraction

## TL;DR

What looked like a **sync problem** (two OS encodings writing to two
`~/.claude/projects/` buckets) turned out to be a **content problem**
(conversation JSONLs contain OS-specific absolute paths baked into
`cwd`, `file_path`, tool calls, and prose). File-level sync cannot fix
content-layer divergence.

**We are pivoting** to a paste-ready **extractor** approach, owned by
portagenty as `pa convos` with pluggable adapters per agentic tool
(Claude Code first, then OpenCode, Cursor, Aider, continue.dev). See
the companion [portagenty proposal](../../../../portagenty/PROPOSAL-claude-code-history-bridge.md)
for the design.

This challenge remains open to drive the remaining design and
research questions.

---

## The Assignment

Investigate and settle **three** open design questions in support of
the pivot to `pa convos` as a cross-tool conversation extractor:

1. **Adapter interface shape.** What's the minimum `ConvoAdapter` trait
   that supports Claude Code, OpenCode, Cursor, Aider, and
   continue.dev without leaking tool-specifics into the shared model?

2. **Rendering defaults and budget.** How verbose should tool calls
   be in paste output by default? What's the right default
   `--max-tokens` cap? What's the shape of the "paste-ready markdown"
   format agentic tools handle best when fed back as context?

3. **Migration story.** What happens to existing users of
   `tools/claudecode-project-sync/`? Deprecate? Keep as
   "aggressive-merge" option? How do we walk users back from the
   silently-diverged state without data loss?

The answers feed the portagenty proposal into its implementation
phase.

---

## Why This Matters

`/resume` is one of the most-used Claude Code commands. When it shows
a fragmented or wrong list, users:

- Lose confidence that the tool remembers their work.
- Accidentally start over instead of continuing.
- Waste tokens re-loading context that was already resolved.
- Can't audit what work has actually been done.

The fragmentation is also **not Claude-specific**. Every agentic
coding tool stores conversation history somewhere, each with its own
format. As users use multiple tools (Claude Code + OpenCode, or
Cursor + Aider, or any mix), the "where's my context from yesterday?"
problem generalizes. A cross-tool extractor is broadly useful, not a
one-off WSL patch.

---

## Concrete evidence (2026-04-19)

One real project on one real machine:

| Encoding | Encoded directory name | Size | Owner | Latest activity |
|----------|------------------------|------|-------|-----------------|
| WSL | `-mnt-c-...-mcp-workflow-and-tech-stack` | 54.1 MB | `cybersader` | 2026-04-19 12:01 |
| Windows | `C--...-mcp-workflow-and-tech-stack` | 54.1 MB | `root` | 2026-04-19 12:00 |
| WSL | `-mnt-c-...-tools-terminal-workspaces` | 8.4 MB | — | 2026-03-25 |
| Windows | `C--...-tools-terminal-workspaces` | 8.4 MB | — | 2026-03-25 |
| WSL | `-mnt-c-...-ultimate-workflow` | empty | — | — |
| Windows | `C--...-ultimate-workflow` | empty | — | — |

Both live buckets hold the same session UUID (`97d7b58b-...`) as
separate files that diverged by ~20 KB within the same day. `/resume`
from WSL was showing "2 months ago" entries instead of today's
session.

### The content-layer finding that killed the sync approach

Spot-check of the 54 MB session file in the C-- bucket:

- **9999+ occurrences of `/mnt/c/...` paths** (WSL-authored content)
- **72 occurrences of `C:\...` paths** (Windows-authored content)
- Embedded inside `cwd` fields, `file_path` tool-call args, and prose

Symlinking/copying/bind-mounting the two encoded directories does
not touch any of this. It merges *storage* but leaves *content*
poisoned with the other OS's paths. A Windows-launched Claude Code
resuming a WSL-authored session fails the first time it tries to
Read `/mnt/c/...`, and vice versa.

Hence the pivot: do not try to resume in place. Extract and paste.

---

## What to Investigate

### Adapter design (P0)

1. **Claude Code adapter.** What's in `~/.claude/projects/*/*.jsonl`?
   Each line is a JSON record with types like `file-history-snapshot`,
   user/assistant messages, tool calls, tool results, metadata. How do
   we normalize these into a tool-agnostic `Conversation` model without
   losing information the renderer might care about later? What
   edge-case record types exist? (Recommend: read 3–5 real sessions of
   varying size before freezing the schema.)

2. **OpenCode adapter.** Where does OpenCode store session history?
   What's the format? Does it share Claude Code's path-encoding pain,
   or a different one? This unblocks the adapter interface — having
   two very different shapes defined forces the interface to be
   honestly tool-agnostic.

3. **Cursor / Aider / continue.dev.** Light research for each. Enough
   to confirm each fits the adapter trait without special-casing.
   Full adapter implementations are later.

### Rendering (P0)

4. **Markdown output shape.** What markdown format do other agentic
   tools parse best when you paste a conversation back in? Does
   Claude prefer `## User:` / `## Assistant:` blocks? Does OpenCode
   handle XML tags better? Test by actually pasting the output into
   each target tool and observing whether they pick up the context.

5. **Tool-call rendering default.** Full tool inputs and results are
   often noisy. Proposed default: tool name + truncated inputs +
   summary of result. Verify by testing real-world pastes.

6. **Path rewriting.** The `--rewrite-paths wsl-to-win` / `win-to-wsl`
   / `strip` transforms are the direct answer to the content-layer
   problem. What's the right regex? Are there edge cases (paths with
   spaces, paths inside prose vs tool args, etc.)?

### Migration and recovery (P1)

7. **Deprecating `tools/claudecode-project-sync/`.** Currently runs a
   Docker container on the user's machine doing bidirectional file
   sync every 15 s. Now that `pa convos` is the planned answer:
   - Keep it as "legacy / aggressive merge" with a README banner
     pointing at `pa convos`?
   - Delete it?
   - Rewrite it as "the portagenty helper that runs only on demand"?

8. **Recovery playbook.** Given a user in the already-diverged state
   (two 54 MB JSONLs, same UUID, different content), what's the safe
   sequence?
   - List divergent UUIDs (`pa convos list --show-forks`?)
   - For each: extract both, diff, pick the canonical one
   - Archive the other with timestamp
   - Document that paste-to-new-chat is the last-resort floor.

9. **Upstream posture.** Do we comment on
   [#17682](https://github.com/anthropics/claude-code/issues/17682),
   [#9668](https://github.com/anthropics/claude-code/issues/9668),
   [#9306](https://github.com/anthropics/claude-code/issues/9306) with
   the content-layer finding? Our observation is likely news to some
   readers. A well-written public post might pressure an upstream fix.

### Scope (P2)

10. **Does `pa convos` also replace the third-party viewers?** Or do
    we deliberately *not* try to be
    [claude-code-viewer](https://github.com/d-kimuson/claude-code-viewer)
    and instead recommend it for browsing while we own the "extract
    and paste" niche? Current recommendation: own one niche well.

11. **Workspace scoping semantics.** `pa` knows which paths belong to
    the current workspace. Default `pa convos list` shows "conversations
    whose CWD falls under this workspace" — verify this is always the
    right default or if it needs per-workspace config.

---

## Context to Read First

### Internal to the workflow repo
- [`tools/claudecode-project-sync/README.md`](../../tools/claudecode-project-sync/README.md) — current container + symlink script (the approach we're stepping away from)
- [`tools/claudecode-project-sync/claude-projects-bidirectional-sync.py`](../../tools/claudecode-project-sync/claude-projects-bidirectional-sync.py)
- [`tools/claudecode-project-sync/claude-code-wsl-complete-guide.md`](../../tools/claudecode-project-sync/claude-code-wsl-complete-guide.md)

### In the portagenty project
- [`PROPOSAL-claude-code-history-bridge.md`](../../../../portagenty/PROPOSAL-claude-code-history-bridge.md) — the full design for `pa convos` with adapter sketch
- [`DESIGN.md` §11](../../../../portagenty/DESIGN.md) — existing scoped design for workspace `id` anchor
- [`src/export/mod.rs`](../../../../portagenty/src/export/mod.rs) — sibling module (tmux/zellij artifact rendering) that `src/convos/` would parallel

### Upstream (tracking only, not dependencies)
- [anthropics/claude-code#17682](https://github.com/anthropics/claude-code/issues/17682)
- [anthropics/claude-code#9668](https://github.com/anthropics/claude-code/issues/9668)
- [anthropics/claude-code#9306](https://github.com/anthropics/claude-code/issues/9306)

### Related existing tools
- [d-kimuson/claude-code-viewer](https://github.com/d-kimuson/claude-code-viewer) — best browsing UX; we recommend alongside
- [jhlee0409/claude-code-history-viewer](https://github.com/jhlee0409/claude-code-history-viewer)
- [raine/claude-history](https://github.com/raine/claude-history)
- [kvsankar/claude-history](https://github.com/kvsankar/claude-history)
- [agsoft VS Code extension](https://marketplace.visualstudio.com/items?itemName=agsoft.claude-history-viewer)

### The two GitHub repos that own this problem
- [cybersader/portagenty](https://github.com/cybersader/portagenty) — where `pa convos` will ship
- [cybersader/agentic-workflow-and-tech-stack](https://github.com/cybersader/agentic-workflow-and-tech-stack) (also [cybersader-agentic-workflow-and-tech-stack](https://github.com/cybersader/cybersader-agentic-workflow-and-tech-stack)) — where the sync container lives today

---

## What Success Looks Like

A short design+research document that delivers:

- **A frozen adapter trait** — minimal, covers at least 2 concrete tools
  (Claude Code P0, OpenCode P1) without special-casing.
- **A renderer contract** — markdown format validated by real pasting
  into Claude and OpenCode targets.
- **A migration note** for `tools/claudecode-project-sync/` — keep as
  legacy, deprecate, or rewrite; with a concrete recommendation.
- **A recovery playbook** for users currently in the fragmented state.
- **A decision on upstream engagement** — comment on #17682? Post a
  writeup? Nothing? Argued either way.
- **Validity window:** "This design holds until Anthropic ships native
  project-ID resolution (tracked in #17682). Even then, the cross-tool
  and paste-ready aspects remain useful."

---

## What This Does NOT Decide

- Portagenty's roadmap and release cadence (they own the
  [PROPOSAL](../../../../portagenty/PROPOSAL-claude-code-history-bridge.md)).
- Which multiplexer to use, which LLM, etc. — unrelated.
- How Claude Code indexes conversations internally — we're working
  around it, not redesigning it.
- Cross-machine sync (different problem, existing tools when paths
  match).

---

## Open Threads

- **The `-ultimate-workflow` empty-dir footgun.** Evidence suggests
  Claude Code auto-creates encoded dirs on launch even when CWD is
  misidentified. Worth filing upstream with a minimal repro.
- **`wsl.conf automount metadata`.** Would changing WSL mount options
  change the ownership (`root` vs `cybersader`) pattern we observed?
  Orthogonal to the main design but could reduce confusion.
- **Moving the project off `/mnt/c/` into WSL-native FS.** Sidesteps
  the dual-encoding entirely at the cost of losing native Windows
  editor access. Tradeoff worth a short writeup.
- **Agents extracting their own context.** If an agent can invoke
  `pa convos dump <uuid>`, it can bootstrap from a prior session on
  its own. Nice meta-property of the paste-first design.
- **Committing context to the repo.** `pa convos export --to
  docs/agent-context/...` makes conversations first-class
  repo-citizens. Could become the primary knowledge-persistence
  mechanism for agent-assisted projects.
