---
title: Installation
description: Install the pconv CLI.
sidebar:
  order: 1
---

:::note
**v0.0.1.** `list` and `dump` (including `--rewrite` path transforms)
are implemented. `mcp serve` lands in a follow-up release. Install
from source until the crate is published to crates.io.
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

## Verify

```sh
pconv --version
```

## Requirements

- Rust 1.82+ (stable).
- Something that writes conversation history. Claude Code is the only
  supported tool in v0.1 (stores at `~/.claude/projects/*/*.jsonl`).
  opencode, Cursor, Aider, continue.dev adapters are planned for
  follow-up releases.
