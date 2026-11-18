# Changelog

All notable changes to portaconv. The MCP tool surface is a public
contract; see the Stability note in the
[commands reference](https://cybersader.github.io/portaconv/reference/commands/#pconv-mcp-serve).

## [0.1.0]

First tagged release.

### CLI

- `pconv list` — list Claude Code conversations; filter, sort, limit,
  scope by portagenty workspace (`--workspace-toml auto`, including
  `previous_paths` for moved folders); per-file cache.
- `pconv dump` — paste-ready markdown or JSON; `--latest`, `--tail`,
  `--file`, opt-in `--rewrite wsl-to-win|win-to-wsl|strip`.
- `pconv doctor` — detect stale `sessions-index.json`; `--dump-stale`.
- `pconv rebuild-index` — rebuild `sessions-index.json` from the
  `.jsonl`s; atomic write with dated backup.
- `pconv handoff` — bounded, recency-biased recovery document for
  sessions too large to `/compact`.
- `pconv mcp serve` — stdio MCP server.

### MCP (stable from this release)

- Tools: `list_conversations`, `get_conversation`, `doctor`.
- Resource template: `convos://conversation/{id}`.
- `rebuild-index` and `handoff` are CLI-only.

[0.1.0]: https://github.com/cybersader/portaconv/releases/tag/v0.1.0
