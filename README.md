<div align="center">

# portaconv

**Terminal-native conversation extractor + MCP server for agent CLIs.**

Paste-ready recovery for when `/resume` lets you down,
folder moves break the cache, or WSL and Windows fragment
your Claude Code history into two diverged buckets.

Sibling to [portagenty](https://github.com/cybersader/portagenty).

</div>

---

> **Status: v0.0.1 bootstrap.** Nothing works yet. The scaffolding
> is in place; Phase 1 (JSONL research + adapter trait) starts next.
> See `knowledgebase/` for the design context.

## The problem

Claude Code (and every other agent CLI) stores conversation
history keyed to the **absolute filesystem path** of the working
directory at launch. This breaks in two ways:

1. **Storage fragmentation.** The same project accessed via WSL
   and Windows produces two separate encoded directories under
   `~/.claude/projects/`. `/resume` from either only sees half
   the history.

2. **Content poisoning.** Spot-check of one 54 MB session: 9999+
   `/mnt/c/…` and 72 `C:\…` path references baked into
   conversation content (`cwd`, `file_path`, prose). Merging the
   storage layer with symlinks doesn't fix this — a
   Windows-launched Claude that resumes a WSL-authored session
   fails the first time it tries to `Read /mnt/c/…`.

File-level sync is folly. The content carries the OS it was
authored on.

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
pconv mcp serve                # stdio MCP server
```

Claude Code adapter only for v0.1. OpenCode / Cursor / Aider /
continue.dev adapters are separate PRs after the adapter trait
survives contact with reality.

## Install

```sh
cargo install --git https://github.com/cybersader/portaconv
```

(Published to crates.io once v0.1 lands.)

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
