---
name: portagenty-workspace
description: This project is a portagenty workspace — a portable, terminal-native launcher for agent sessions, driven by a committed `*.portagenty.toml`. Read it to understand the workspace's declared sessions, multiplexer, project paths, and move history.
---

# portagenty-workspace

A portagenty workspace is a single TOML file at the project root
whose name ends in `.portagenty.toml`. It's the source of truth for
what sessions exist, what they run, and how they're multiplexed.

## Key fields

- `name` — human display name.
- `id` — UUID. Stable across folder moves; used by tooling
  (portaconv included) to track the workspace across environments.
- `multiplexer` — `"tmux"` or `"zellij"`.
- `projects = [...]` — project roots this workspace covers.
- `previous_paths = [...]` — auto-maintained by pa on walk-up
  re-registration. When the project folder moves, pa appends the
  old location here so external tools can bridge state
  (conversations, caches) authored at the old path.
- `[[session]]` blocks — `name`, `cwd`, `command`, optional
  `kind` (`claude-code`, `opencode`, `editor`, `dev-server`,
  `shell`, `other`) and `env`.

## Common commands

- `pa` — open the TUI (workspace picker → session list).
- `pa launch <name>` — attach a specific session.
- `pa claim <name>` — takeover: kicks other clients attached to
  the same session (the "move this session to this device" verb).
- `pa convos list` / `pa convos dump <id>` — see the `portaconv`
  skill.
- `pa add <name> --command "..."` — append a session without
  editing TOML by hand.

## Why sessions sometimes flash + exit

Multiplexers spawn commands under a *non-interactive* shell —
`~/.bashrc`/`~/.zshrc` aliases and functions are NOT loaded. A
`command = "my-alias"` that works in your interactive shell will
error with "command not found" inside pa. Write the literal command
or promote the alias to a real binary on PATH. See
<https://cybersader.github.io/portagenty/reference/schema/>
for the full story.
