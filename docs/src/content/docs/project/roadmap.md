---
title: Roadmap
description: Ideas and potential features for portaconv — not commitments, explicit tiers of likelihood, and a record of what's been explicitly decided against.
sidebar:
  order: 1
---

:::note
This isn't a commitment list. It's a snapshot of where ideas sit —
some likely, some speculative, some explicitly parked. The goal is
to keep design decisions visible alongside the "what if we…" pile
so neither gets forgotten. Actionable items live on the
[GitHub issue tracker](https://github.com/cybersader/portaconv/issues);
anything here without an issue link is still idea-shaped.
:::

## Likely next

Things that probably make sense once there's user signal or the
next 1-2 sessions of focused work.

- **Additional adapters.** Aider first (plain markdown in
  `.aider.chat.history.md`, cheapest adapter we could land);
  opencode next (similar storage shape to Claude Code);
  continue.dev after that. Cursor's SQLite storage is meaningfully
  harder and waits until demand justifies it.
- **crates.io publish** once the surface feels stable. Tag v0.1.0,
  `cargo publish`, users install with `cargo install portaconv`
  instead of cloning.
- **Schema promotion for production-observed record types.** The
  spike's initial six record types were joined by five more
  surfaced on real corpus data (`permission-mode`, `attachment`,
  `last-prompt`, `custom-title`, `agent-name`). Most should stay in
  the extensions bag, but `custom-title` in particular deserves
  promotion to `Conversation.title` when present, overriding the
  first-user-message heuristic.

## Maybe (ideas worth recording, not prioritized)

- **`pconv search <query>`** — a separate verb for full-content
  search, streaming through JSONLs with no index. Deliberately
  distinct from `list --grep` (which stays substring-over-title+cwd)
  so the scope creep of FTS doesn't bleed into the discovery filter.
- **`pconv copy <id>`** — bake in clipboard support with OS
  auto-detection (`clip.exe`, `pbcopy`, `wl-copy`, `xclip`). Removes
  the `| clip.exe` pipe dance. Small win; currently piping works.
- **`pconv export --to <file>`** — shorthand for `pconv dump <id> >
  <file>`. Arguably sugar; `>` already works.
- **Fuzzy-matched `list --find`** — nucleo-ranked (same crate
  portagenty uses) for "kinda remember the project name" flows. The
  dep is real weight though; punt unless someone asks.
- **Snapshot tests for markdown renderer** — `insta` is already a
  dev-dep and unused. Lock down the rendered shape so format
  regressions surface on PR diff.
- **Property tests for path-rewrite edge cases** — paths with
  spaces, UNC paths, paths adjacent to URLs. Catch the subtle
  regex cases my hand-rolled tests miss.
- **Cache warm-up on first install** — the first `pconv list` on a
  big corpus is 40s; subsequent runs are 3s. A `pconv cache warm`
  subcommand that pre-fills in the background would make the first
  interactive call feel instant.
- **Shell completions** — `pconv completions bash|zsh|fish` emits
  a completion script. clap makes this trivial; hold until someone
  hits a dead-end tab-completing `--sort`.

## Portagenty-side (lives in the other repo)

These ideas **aren't portaconv's to build** — they'd ship as PRs
on [portagenty](https://github.com/cybersader/portagenty). The
receiving-side contract (what flags pconv accepts, what output
format it emits) is settled and documented here; the consumer is
upstream.

- **`pa init --with-agent-hooks`** scaffolds a workspace with pconv
  pre-wired: `.mcp.json` (portaconv MCP entry), `.claude/commands/*.md`
  (slash commands like `/pull-context` that call pconv), and
  `.claude/skills/*.md` (capability manifest so the agent
  self-discovers what's available). Optional flag; skipped if
  pconv isn't on PATH. The in-session friction killer — agent
  inside a pa-launched session reaches its own workspace history
  without copy-paste or leaving the terminal.
- **`pa convos` shim.** Small wrapper that shells out to `pconv`
  with workspace context pre-filled. `pa convos list` →
  `pconv list --workspace-toml <current>`. `pa convos last` →
  `pconv dump --latest --workspace-toml <current>`. If pconv isn't
  installed, print an install hint and exit. ~100 LOC + an install
  check.
- **Auto-maintain `previous_paths`** in the workspace TOML. When
  pa's auto-re-register-on-walk-up detects that a workspace `id`
  has shown up at a new path, append the old cwd to the TOML's
  `previous_paths` array. portaconv **already reads** this field,
  so the bridging works the moment pa starts writing it — no
  portaconv version bump needed. Keeps the entire story inside
  the committable TOML; no cross-tool registry coupling.
- **`pa://convos/<workspace-id>`** protocol-handler route. pa
  already registers a `pa://` URL scheme for cross-device deep
  links; this slot would resolve to `pconv list` filtered to that
  workspace. Nice-to-have, low demand.

## Won't do (design decisions, recorded so nobody re-argues them)

These are parked permanently unless the foundation shifts. The
answers below came from the original design Q&A and live in
[`knowledgebase/03-design-decisions.md`](https://github.com/cybersader/portaconv/blob/main/knowledgebase/03-design-decisions.md).

- **Interactive TUI viewer.** Different niche. jhlee0409's
  [claude-code-history-viewer](https://github.com/jhlee0409/claude-code-history-viewer)
  covers browsing well; portaconv stays CLI-pipe-friendly.
- **Daemon / file watcher / auto-sync.** The whole thesis is
  extract-on-demand, not continuous-sync. A daemon brings back
  every problem the research doc catalogued (race windows,
  divergence, content-layer poisoning). Non-starter.
- **Writes to any tool's storage.** Read-only by construction.
  Every feature is additive; not one byte goes back to
  `~/.claude/projects/`.
- **FTS bolted into `--grep`.** `--grep` is substring-over-title-
  and-cwd and stays that way. Full-content search is a **separate**
  verb (see "Maybe") if / when it lands.
- **`pconv import`** or any flow that writes markdown back into a
  tool's native format. If you want to commit context, use
  `pconv dump <id> > docs/agent-context/whatever.md` and git-add
  it. Git is the store.

## What's already shipped

For context — the following are done and live:

- Core schema + Claude Code adapter
- `list` + `dump` (markdown/json) + `mcp serve`
- Path-rewrite transforms (`wsl-to-win`, `win-to-wsl`, `strip`)
- Tier A list filters (`--since` / `--sort` / `--reverse` /
  `--limit` / `--grep`)
- `dump --latest --workspace-toml auto` one-liner
- `dump --tail N` length control with self-documenting truncation
- Full MCP schema parity (every CLI flag mirrored as a tool argument)
- Per-file list cache (~13× speedup on 2607-JSONL corpus)
- v0.2 issues #1 #2 #3 all closed
- Defensive `previous_paths` read — portaconv bridges old-path
  sessions the moment portagenty starts writing the field; no
  portaconv version bump required when that lands upstream

If something here looks like it should be on a different tier,
open an [issue](https://github.com/cybersader/portaconv/issues) and
argue for it.
