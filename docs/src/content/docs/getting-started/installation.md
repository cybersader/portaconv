---
title: Installation
description: Install the pconv CLI.
sidebar:
  order: 1
---

:::note
**v0.0.1.** All three commands are implemented — `list`, `dump`
(markdown + json, path rewriting, thinking/result flags), and
`mcp serve` (stdio MCP for agent integration). Install from source
until the crate is published to crates.io.
:::

## From crates.io (planned, v0.1+)

```sh
cargo install portaconv
```

The binary is named `pconv` (the crate is `portaconv`). A single
static Rust binary lands at `~/.cargo/bin/pconv`. No daemon, no
runtime deps.

## From source

```sh
git clone https://github.com/cybersader/portaconv
cd portaconv
cargo install --path .
```

## Verify it works

```sh
# Version check
pconv --version

# Does pconv see your Claude Code storage?
pconv list --format json | jq 'length'

# Peek at the shape of one session entry (needs jq)
pconv list --format json | jq '.[0]'
```

If `list` returns `0` but you've used Claude Code, you may be
running as a different user than the one who owns `~/.claude/`.
Override the root with the `PORTACONV_CLAUDE_ROOT` environment
variable to point elsewhere.

Once this works, the [quickstart](/portaconv/getting-started/quickstart/)
walks through dumping a session, path rewriting, and MCP wiring.

## Requirements

- Rust 1.82+ (stable).
- Something that writes conversation history. Claude Code is the only
  supported tool in v0.1 (stores at `~/.claude/projects/*/*.jsonl`).
  opencode, Cursor, Aider, continue.dev adapters are planned for
  follow-up releases.
