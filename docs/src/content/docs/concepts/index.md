---
title: Concepts
description: How portaconv thinks about conversation history.
sidebar:
  order: 1
---

portaconv sits between a handful of non-obvious ideas. Skim these
before the reference.

## Adapter

A read-only normalizer for one agent CLI's on-disk conversation
storage. Each adapter implements a small trait: `detect()`, `list()`,
`load(id)`. The adapter's job is to turn the tool's native format
into portaconv's shared `Conversation` model without losing anything
a renderer might care about later.

The v0.1 built-in is [Claude Code](/portaconv/reference/adapter-claude-code/).
opencode, Cursor, Aider, and continue.dev are planned as separate PRs
after the trait survives contact with reality.

## Shared conversation model

One `Conversation` type, tool-agnostic:

- `messages: Vec<Message>` — ordered, with timestamps
- Each message has a role (user / assistant / system / tool) and a
  `Vec<ContentBlock>` (text / tool_use / tool_result / thinking)
- A per-conversation `extensions: serde_json::Value` bag carries
  tool-specific fields (Claude's `gitBranch`, `version`,
  `permissionMode`, …) without polluting the core schema
- Each message has its own `extensions` bag for the same reason

Shape is OpenAI Chat Completions on the outside, Anthropic
content-blocks on the inside. That's the convergence point every
serious tool already speaks.

## Renderer

Takes a `Conversation` and a format flag (`markdown` default, `json`,
`plain`) and produces stdout. The default markdown shape is tuned
for pasting back into another agent — `## User` / `## Assistant`
blocks, tool calls as fenced code, tool results truncated by default.

## Path-rewrite transforms

Claude Code JSONLs carry absolute filesystem paths from the authoring
OS inside `cwd` fields, tool-call args, and prose. A session authored
from WSL (`/mnt/c/Users/…`) pastes badly into a Windows-launched
session (`C:\Users\…`) and vice versa.

`pconv dump --rewrite wsl-to-win` (or `win-to-wsl`, or `strip`) is
the opt-in transform that addresses this at the **content** layer —
the thing file-level sync approaches cannot fix.

## MCP server

`pconv mcp serve` exposes the same capabilities over stdio MCP:

- Tools: `list_conversations`, `get_conversation(id)`
- Resources: each conversation at `convos://conversation/<id>`

A running agent can query portaconv to pull prior conversation
context into its working memory. Minimal by design; the heavy
lifting happens inside the CLI and the stored JSONLs.

## Read-only, on-demand

portaconv never writes to any tool's storage. There is no daemon,
no file watcher, no auto-sync. Every `pconv` invocation reads the
current state of disk and emits output. Simpler to reason about,
easier to reverse, composable with pipes.
