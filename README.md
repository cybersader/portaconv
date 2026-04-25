<div align="center">

# portaconv

**Terminal-native conversation extractor + MCP server for agent CLIs.**

Paste-ready recovery for the cases `claude -r <uuid>` can't reach —
cross-OS content drift, sessions whose original cwd is moved or
different, broken `/resume` pickers, and conversations you want to
commit into the repo.

Sibling to [portagenty](https://github.com/cybersader/portagenty).

</div>

---

> **Status: v0.1 surface feature-complete; not yet on crates.io.**
> Claude Code adapter, `list` / `dump` / `doctor` / `rebuild-index` /
> `mcp serve`, path-rewrite transforms, per-file list cache, and
> explicit backing-file override all shipped. Full guide at
> [cybersader.github.io/portaconv](https://cybersader.github.io/portaconv/).

## The problem

Claude Code (and every other agent CLI) stores conversation
history keyed to the **absolute filesystem path** of the working
directory at launch. This breaks in three ways:

1. **Storage fragmentation + cwd-strict resume.** The same project
   accessed via WSL and Windows produces two separate encoded
   directories under `~/.claude/projects/`. `/resume` from either
   only sees half the history. Worse, even `claude -r <uuid>`
   doesn't bypass this — it only looks in the encoded-dir of your
   *current* cwd, not by sessionId across all encoded dirs. Move
   the project, cd to a sibling, run from `/tmp`: *"No conversation
   found with session ID."* Empirically reproduced; tracked
   upstream as [#5768][s5768] (open) and [#28745][s28745] (open
   feature request for the missing `--cwd` / `--ignore-directory`
   flag).

[s5768]: https://github.com/anthropics/claude-code/issues/5768
[s28745]: https://github.com/anthropics/claude-code/issues/28745

2. **Content poisoning.** Spot-check of one 54 MB session: 9999+
   `/mnt/c/…` and 72 `C:\…` path references baked into
   conversation content (`cwd`, `file_path`, prose). Merging the
   storage layer with symlinks doesn't fix this — a
   Windows-launched Claude that resumes a WSL-authored session
   fails the first time it tries to `Read /mnt/c/…`.

3. **Stale index.** Claude Code caches session summaries in
   `sessions-index.json` alongside the `.jsonl`s. The picker for
   `/resume` reads this index — but the write path only runs on
   graceful shutdown, and ungraceful WSL closures (`wsl
   --shutdown`, window close, machine suspend) skip it. On one
   machine: 14 projects with the index lagging the actual jsonls
   by up to 93 days. Upstream canonical issue: [#25032][s25032].

[s25032]: https://github.com/anthropics/claude-code/issues/25032

File-level sync is folly. The content carries the OS it was
authored on, and the index can't be trusted to reflect reality.

## The pivot

portaconv reads agent-CLI conversation storage (read-only) and
emits **paste-ready** text — optionally with `/mnt/c/` ↔ `C:\`
path rewriting so the output works on the other OS.

You don't try to resume in place. You extract what you said, paste
it into a new session on whatever machine is in front of you, and
the new session picks up where the old one left off.

Also ships as an MCP server, so any MCP-aware agent can query past
conversations directly.

## What's in v0.1

```
pconv list                     # list Claude Code conversations
pconv dump <session-id>        # paste-ready markdown to stdout
pconv doctor                   # detect stale sessions-index.json
pconv doctor --dump-stale      # also dump the newest session per stale project
pconv rebuild-index --all      # rewrite sessions-index.json from the jsonls
pconv mcp serve                # stdio MCP server
```

Claude Code adapter only for v0.1. OpenCode / Cursor / Aider /
continue.dev adapters are separate PRs after the adapter trait
survives contact with reality.

### Failure modes ↔ primitives

| Failure | Primitive | What it does |
|---|---|---|
| Cross-OS content poisoning (paths inside session won't resolve on the other OS) | `pconv dump --rewrite wsl-to-win\|win-to-wsl` | Extracts + rewrites absolute paths; paste into a session on the other OS. |
| Folder moved / different cwd / `claude -r <uuid>` says "not found" | `pconv list --workspace-toml auto` + `pconv dump <id>` | Finds sessions authored at the pre-move path via `previous_paths`; dumps to paste-ready markdown so you can resume from any cwd. |
| Stale `sessions-index.json` (picker shows wrong/missing sessions) | `pconv doctor` + `pconv rebuild-index` | Detects lag; rebuilds the index from the `.jsonl`s with a dated `.bak` backup. |
| Want to commit the conversation as a repo artifact | `pconv dump <id> > docs/agent-context/…md` | Standard stdout redirect; the git repo is the store. |

## Install

```sh
cargo install --git https://github.com/cybersader/portaconv
```

(Published to crates.io once v0.1 stabilizes.)

## When `claude -r` is enough (and when it isn't)

`claude -r <uuid>` is the cheap move when **all** of the following hold:

- you know the session UUID,
- you're on the same OS the session was authored on,
- file paths inside the session still resolve from your current shell,
- **and your current cwd matches the session's original cwd** (this one
  catches people — verify with `find ~/.claude/projects -name "<uuid>.jsonl"`
  and cd to the path that encodes to the same dir name).

If any of those breaks, `claude -r` returns *"No conversation found with
session ID"* and you're stuck. That's portaconv territory:

| You hit… | Reach for… |
|---|---|
| Don't remember the UUID | `pconv list --workspace-toml auto` |
| Folder moved / different cwd / `claude -r` "not found" | `pconv dump <id>` (paste into a fresh `claude` from anywhere) |
| Cross-OS resume (WSL → Windows) | `pconv dump <id> --rewrite wsl-to-win` |
| `/resume` picker is lying | `pconv doctor` then `pconv rebuild-index` |
| Need a `.md` artifact in the repo | `pconv dump <id> > docs/agent-context/…md` |
| Agent should self-heal via tool calls | `pconv mcp serve` (3 tools, stdio JSON-RPC) |
| Last N messages only | `pconv dump <id> --tail 50` |

The framing: portaconv is the layer **around** `claude -r`, not a
replacement. For the narrow happy-path it covers, `claude -r` + `cd`
is right.

## Usage with Claude Code

The canonical wiring is via [portagenty](https://github.com/cybersader/portagenty):

```sh
pa init --with-agent-hooks   # scaffolds .mcp.json + .claude/ in your project
```

That writes an `.mcp.json` pointing at `pconv mcp serve`. Prefer to
hand-roll? Same shape works in `~/.claude.json` or a project-level
`.mcp.json`:

```jsonc
{
  "mcpServers": {
    "portaconv": { "command": "pconv", "args": ["mcp", "serve"] }
  }
}
```

Once wired, the agent gets `list_conversations` + `get_conversation`
tools and a `convos://conversation/{id}` resource template. See the
[agents + portagenty guide](https://cybersader.github.io/portaconv/concepts/agents-and-portagenty/)
for usage patterns (post-compact recovery, cross-tool handoff,
committed recovery artifacts).

## The unique value

Existing tools in this space are overwhelmingly GUI viewers. Try
`jhlee0409/claude-code-history-viewer` or
`d-kimuson/claude-code-viewer` if you want a browser-based UI —
they're good. `raine/claude-history` is a solid TUI.

portaconv fills the **terminal-native extract + MCP + path-rewrite**
niche. Specifically: no existing tool rewrites OS-specific absolute
paths inside conversation content. That's our wedge.

## Non-goals (explicit)

- **No GUI, no TUI.** Unix-pipe-first. Use the viewers above for
  browsing.
- **No daemon, no auto-sync.** On-demand reads.
- **No path-rewrite by default.** Opt-in transform.
- **No search / FTS / embeddings** in v0.1. Get-by-ID only.

## Related projects

| Project | Role |
|---|---|
| [portagenty](https://github.com/cybersader/portagenty) | Workspace launcher. Uses the workspace `id` field that portaconv will (eventually) leverage for folder-move recovery. |
| [mcp-workflow-and-tech-stack](https://github.com/cybersader/mcp-workflow-and-tech-stack) | Has the original Docker `claudecode-project-sync` tool — the approach we stepped away from. Will get a banner pointing at portaconv. |

## License

MIT.
