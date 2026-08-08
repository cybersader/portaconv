---
description: List this workspace's prior Claude Code conversations.
---

Run `pa convos list` via the Bash tool and show the output.

Use `pa convos dump <id>` to export a specific conversation as
paste-ready markdown. Pass `--rewrite wsl-to-win` (or `win-to-wsl`)
to normalize OS-specific paths baked into content before pasting
into a new session.

`pa convos ...` auto-scopes to this workspace's TOML via
`--workspace-toml <path>`; you don't need to pass it yourself.
