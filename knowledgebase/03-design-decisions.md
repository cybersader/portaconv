# 03 · Design decisions (locked via Q&A, 2026-04-19)

These decisions came from a multi-round Q&A with the user during
the portaconv planning session. They're the contract the v0.1
implementation must respect.

## Core identity

| Axis | Decision | Why |
|---|---|---|
| **Name** | `portaconv` crate + `pconv` CLI binary | Generic enough to be findable; short shortform; "porta-" prefix signals family with portagenty |
| **Distribution** | Standalone Rust crate, `cargo install portaconv` | Reaches Claude-only users too (not just pa users); decouples release cadence from portagenty |
| **Unique value** | Terminal-native CLI + MCP server | Existing viewers are GUI-first; CLI Unix-pipe-friendly extraction is an actual gap |
| **Durability concern** | User flagged: "Claude handling cache issues probably makes some value not durable over time" — format may change upstream | Adapter trait + extensions bag isolate format volatility. Core schema (OpenAI-format) is stable. |

## Architecture

**Standalone binary**, sibling to portagenty. Not a pa subcommand.

Rationale: pa integration is the cherry on top, not the core.
Users who never touch pa still benefit. pa integration happens
later as a thin shim (`pa convos` → shells out to `pconv`).

### Portagenty interop (later, not v0.1)

Two use cases the user flagged as valuable:

1. **Context retrieval for the current conversation**: A running
   Claude session could query pconv (via MCP) for relevant prior
   context — "show me what I did on this workspace before."

2. **Folder-move recovery**: When pa's auto-re-register-on-walk-up
   triggers (detects a workspace file at a new path), pa could
   prompt: "run `pconv` to bridge conversations from the old path."

Both require pa to pass workspace-scope info to pconv (TOML path,
workspace `id`, project paths). The interop layer is just argument
passing — pconv stays standalone; pa wraps it.

## v0.1 scope (minimum viable)

Just enough to validate the thesis. The user was explicit: don't
over-build.

### Commands

```
pconv list                        # list all Claude conversations
pconv dump <session-id>           # output markdown to stdout
pconv mcp serve                   # stdio MCP server
```

That's it. No `copy`, no `export`, no `adapters`, no search.
Claude Code adapter only. ~1 week of work.

### MCP tools

```
list_conversations(since?, workspace_id?, limit?)
get_conversation(id, format="markdown"|"json", rewrite?)
```

Plus each conversation exposed as a resource at
`convos://conversation/<id>`.

### Schema

OpenAI Chat Completions style for the `messages` array, Anthropic
content-blocks for tool calls preserved (not flattened), with an
`extensions: Value` field on both `Conversation` and `Message`
for tool-specific bits (`gitBranch`, `version`, Claude's
`permissionMode`, etc.).

Rationale: every tool we'll ever adapt serializes to/from OpenAI
format, so our output lands on the lingua franca automatically.
Losing tool-specific fidelity isn't acceptable, hence the
extensions bag.

## Non-goals (explicit)

- **No GUI / TUI viewer.** jhlee0409's multi-tool viewer covers
  that niche. We're Unix-pipe-friendly CLI.
- **No daemon, no file watching, no auto-sync.** On-demand reads.
- **No path-rewrite by default.** Opt-in transform via
  `--rewrite wsl-to-win` etc. Don't modify content unless asked.
- **No adapters other than Claude Code in v0.1.** Each new adapter
  is a separate PR after the trait survives contact with reality.
- **No search / FTS / embeddings in v0.1.** Get-by-ID only.
  Search is v0.2+.
- **No pa-integration in v0.1.** portaconv stands alone. The pa
  shim and folder-move recovery come after portaconv is stable.

## Q&A verbatim summary

**Q: Unique value?**
> Terminal native I guess. The whole Claude handling cache issues
> probably makes some of the value not durable over time. The
> terminal native piece is nice though. Claude code history viewer
> is really nice but not if you're only working in terminal.

**Q: Where does the code live?**
> If we are building it then it's probably a separate binary, yet
> we could design portagenty to be aware of it in the near term.

**Q: Internal schema?**
> Go with de facto standard but design system in a way to
> accommodate others in future.

**Q: MCP server?**
> MCP server and CLI tool, so yes technically [both].

**Q: Name + packaging?**
> portaconv i guess. Probably cargo or something like that. "pconv"
> command for short?

**Q: v0.1 scope?**
> Minimum viable: `list`, `dump`, `mcp serve` — Claude Code
> adapter only.

**Q: Portagenty integration strategy?**
> I'm thinking it would be cool if we use the portable TOML piece
> of this to somehow help with things. Maybe it's to help retrieve
> the context you want in the current conversation, maybe it's to
> help with the folder moving problem where pa would detect if a
> folder has moved and ask if you need pconv run to restore
> conversation (granted it will cost chat context with the ai).

**Q: MCP scope?**
> Minimal: `list_conversations`, `get_conversation(id)` as MCP
> tools.

**Q: Bootstrap as sibling project with seeded knowledgebase?**
> When we establish the project, it should be a folder in the same
> one that this project is in with all the "knowledge, context,
> etc" pasted into a knowledgebase for it so I can start a
> conversation over there as I develop. [...] Part of the very
> reason I'm developing this tool.

## Guiding principles (user emphasized)

1. **First principles.** Don't build on assumptions that can
   break. Work at stable abstraction levels.
2. **Resilient.** Survives format changes, tool churn, OS changes.
3. **Elegant.** Not a sprawl of commands. Composable.
4. **Future accounting.** Room for MCP standardization, new tools,
   schema evolution.

These bias every tradeoff toward: adapter trait stability, format
adoption (OpenAI) over invention, extension hooks over
flat-schema, standalone crate over tight pa coupling.
