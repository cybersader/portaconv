---
title: Installation
description: Install the pconv CLI.
sidebar:
  order: 1
---

:::caution
portaconv is at **v0.0.1 bootstrap**. Commands are not yet implemented.
The install path below is the intended shape; until v0.1 ships, clone
the repo and build from source.
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
