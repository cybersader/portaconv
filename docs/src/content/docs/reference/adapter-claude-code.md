---
title: Claude Code adapter notes
description: The v0.1 contract for how portaconv reads ~/.claude/projects/ JSONLs.
sidebar:
  order: 2
---

Empirical contract the Claude Code adapter was built against —
originally a Phase 1 spike against six real JSONLs pulled from
`~/.claude/projects/` on 2026-04-20 (host: Linux/WSL, Claude Code
versions 2.0.76 → 2.1.51 observed). The v0.1 adapter in
`src/adapters/claude_code.rs` implements this mapping; changes there
should be reflected back into this doc.

## 1. Scope

- Normative: any record type listed in §3 is handled as specified.
- Any record type not listed is **unknown** — the adapter must not
  silently drop it; it must either land in a per-record
  `extensions: serde_json::Value` bag or flip a one-line warning.
- The schema contract itself (`Conversation` / `Message` /
  `ContentBlock`) is locked in the project's
  [design decisions](https://github.com/cybersader/portaconv/blob/main/knowledgebase/03-design-decisions.md);
  this doc does not reopen it. It only defines the adapter's mapping
  onto it.

## 2. Sample inventory

Six JSONLs, spread across size, OS encoding, and on-disk shape.
Parser used for the spike was a throwaway `examples/scan-claude-jsonl.rs`,
long since deleted — this doc is its empirical residue.

Project directory names from the sampled corpus are redacted
(`<project-N>`) since this repo is public and they reference the
repo author's own other projects.

| # | Slot | Size | Lines | Encoded dir (prefix) | Filename shape | Claude ver(s) |
|---|------|-----:|------:|---------------------|----------------|---------------|
| 1 | Tiny | 708 B | 3 | `-mnt-c-…-<project-1>` (WSL) | `<uuid>.jsonl` | (no version field in records) |
| 2 | Small WSL | 100 KB | 42 | `-mnt-c-…-<project-2>` (WSL) | `<uuid>.jsonl` | 2.1.39, 2.1.42 |
| 3 | Medium Win | 995 KB | 489 | `C--…-<project-3>` (Windows) | `<uuid>.jsonl` | 2.1.20 |
| 4 | Subagent (new) | 11 KB | 6 | `-mnt-c-…-<project-4>/<parent-uuid>/subagents/` | `agent-acompact-<hash>.jsonl` | 2.1.51 |
| 5 | Large | 19.8 MB | 805 | `-mnt-c-…-<project-5>` (WSL) | `<uuid>.jsonl` | 2.1.2, 2.1.9 |
| 6 | Agent (old) | 80 KB | 23 | `-mnt-c-…-<project-6>` (WSL) | `agent-<hash>.jsonl` | 2.0.76 |

Streaming line-parse completed all six (≈22 MB combined) in **5.5 s
wall-time** — well under the "seconds not minutes" bar the plan set.
Zero parse errors across 1368 records.

## 3. Record type table

The top-level `type` discriminator has six values in this sample.

| `type` | Description | Bucket | v0.1 handling | Notes |
|---|---|---|---|---|
| `user` | Conversational message, user role. `message.{role, content[]}` is the event-stream shape. | **Schema** | Map to `Message { role: User, content: Vec<ContentBlock>, timestamp, extensions }`. Copy top-level `uuid`, `parentUuid`, `requestId`, `isSidechain`, `slug`, `userType` into `message.extensions`. | Includes compact-summary records (`isCompactSummary: true`) — those are regular `user` records whose content is the summary text. Keep them. |
| `assistant` | Conversational message, assistant role. Same event-stream shape. | **Schema** | Same as `user`. Content includes `tool_use` and `thinking` blocks alongside `text`. | `thinkingMetadata` on the record goes into `message.extensions`. |
| `system` | Metadata event (e.g. `subtype: "turn_duration"` with `durationMs`). No `message.content`. | **Extensions** (conv-level) | Append as-is to `Conversation.extensions.system_events[]` (append-only array). Do not render to paste output in v0.1. | Only 6–9 per long session. Small. |
| `file-history-snapshot` | Claude Code's file-tracking metadata: `{messageId, snapshot: {trackedFileBackups, timestamp}, isSnapshotUpdate}`. | **Skip** | Drop. Not surfaced in `Conversation.extensions`. | These files are tracked elsewhere in Claude's workspace; portaconv is not a file-state tool. |
| `progress` | Live streaming events for subagent runs. Carries `data.{message, type: "agent_progress", prompt, agentId}` + `toolUseID` + `parentToolUseID`. | **Skip** | Drop. | The final assistant message already carries the consolidated `tool_use`; the stream is transient. Re-evaluate if v0.1 paste output looks lossy on subagent-heavy sessions. |
| `queue-operation` | Claude Code's prompt-queue bookkeeping (`operation: "enqueue"`, raw user-typed `content`). | **Skip** | Drop. | Internal scheduling state; not part of the rendered dialogue. |

Unknown record types encountered during adapter load must be
accumulated into `Conversation.extensions.unknown_records[]` with
their raw JSON — this is the resilience hook the locked plan calls
for. Don't silently ignore.

**Types observed in production but not in the original spike sample**
(first real-corpus run on 2026-04-20 surfaced 150 such records across
5 distinct types in a 457-message session):

| `type` | Payload fields | v0.1 handling | Target bucket (future polish) |
|---|---|---|---|
| `permission-mode` | `permissionMode`, `sessionId` | `unknown_records` | Extensions — track state changes |
| `attachment` | `attachment`, `entrypoint`, `cwd`, `sessionId`, plus the standard record-header fields | `unknown_records` | Extensions until renderer decides how to show |
| `last-prompt` | `lastPrompt`, `sessionId` | `unknown_records` | Skip |
| `custom-title` | `customTitle`, `sessionId` | `unknown_records` | **Schema** — promote to `Conversation.title` when present (overrides the first-user-message derivation) |
| `agent-name` | `agentName`, `sessionId` | `unknown_records` | Extensions — relates to subagent naming |

v0.1 lands all five as `unknown_records` via the adapter's catch-all —
they're preserved losslessly, just not promoted. A future polish pass
should pick off the "Target bucket" column; `custom-title` in
particular would noticeably improve list titles. Tracked on the
[roadmap](/portaconv/project/roadmap/#likely-next).

## 4. Subagent decision

**Two on-disk shapes observed for subagent sessions:**

- **Old (pre-2.1.x?)**: `<project-dir>/agent-<hash>.jsonl` at the
  project root. Example: the v2.0.76 sample #6. Records carry
  `agentId` but no nested subdir.
- **New (≥ 2.1.x)**: `<project-dir>/<parent-session-uuid>/subagents/agent-<name>-<hash>.jsonl`.
  Example: the v2.1.51 sample #4.

Both carry the distinguishing field `agentId` on every record, and
both omit `version` information on some records (sample #6 did not
emit `requestId` on user records, for instance — earlier Claude
versions carried less metadata).

**v0.1 decision: option (a) from the plan — ignore subagent JSONLs
in `pconv list` and `pconv dump`.** Rationale:

- Subagent sessions are transient reasoning loops, not user-facing
  dialogues the user typically wants to paste-resume.
- The parent session's `tool_use` / `tool_result` pair already
  captures the subagent invocation and its consolidated output in
  the main stream.
- Surfacing subagents would require either a `subagent_of` field
  (breaks the Conversation model shape) or a join (exposes
  tool-call structure the schema is designed to hide behind
  `ContentBlock::ToolUse`).

**Detection rule** for the adapter's `list()`: skip any JSONL whose
path contains `/subagents/` **or** whose filename matches
`^agent-[a-z0-9_-]+\.jsonl$` at the project root.

Re-evaluate in v0.2+ — if users ask for "show me what the subagent
actually thought," option (b) (`subagent_of` in extensions) becomes
the natural add without reshaping the core model.

## 5. Content-block variants

Observed across the six samples:

| Block `type` | Count across samples | v0.1 handling |
|---|---:|---|
| `text` | 120 | `ContentBlock::Text(String)` |
| `tool_use` | 271 | `ContentBlock::ToolUse { id, name, input }` |
| `tool_result` | 270 | `ContentBlock::ToolResult { tool_use_id, output, is_error }` |
| `thinking` | 211 | `ContentBlock::Text` with a one-line prefix noting it was a thinking block. Rendering decision: paste output collapses thinking by default; `--include-thinking` opts in. |
| `<string-content>` | 36 | When `message.content` is a bare JSON string instead of an array (older Claude versions do this on some user records), treat as a single `Text(String)` block. |
| `<no-content>` | 13 | Observed on some `assistant` records with tool-only turns mid-stream. Normalize to an empty `Vec<ContentBlock>`; don't drop the message. |

**Not observed in this sample but known to exist in Anthropic's API**:
`image`, `document`, `redacted_thinking`. Adapter must accept them:

- `image` → `ContentBlock::Text("<image omitted in v0.1>")` with the
  raw block stashed in the `Message.extensions.original_content[]`.
- `document` → same passthrough.
- `redacted_thinking` → dropped entirely, not even mentioned in
  paste output (respects the redaction).

Any other unknown block type goes to `Text("<unknown block: X>")`
with the raw JSON in `message.extensions.original_content[]`.

## 6. Path-content observations

Feeds the later `--rewrite` transform design. Counts below are
line-level presence (regex match anywhere in a record line), not
strict content-only substring counts; the real transform needs a
stricter scope (prose text + tool-call args, not JSON field names or
`cwd`). Noted as an open question in §7.

| Sample | Lines with `/mnt/` | Lines with `X:\\` |
|---|---:|---:|
| #1 tiny | 0 | 0 |
| #2 small WSL | 38 (90%) | 4 |
| #3 medium Windows-encoded | 464 (95%) | 57 |
| #4 subagent | 6 (100%) | 0 |
| #5 large WSL | 800 (99%) | 158 |
| #6 old-agent | 23 (100%) | 3 |

Key observation: **the Windows-encoded bucket (#3) has 464 lines
mentioning `/mnt/` paths but only 57 mentioning `C:\\`.** This is
the content-layer path-poisoning the research doc predicted —
confirmed at scale. A WSL-authored session lives in the
Windows-encoded dir because it was launched from Windows once, but
its content still carries WSL path references from the authoring
side. Pure file-layer sync cannot fix this; `--rewrite wsl-to-win`
on this session's output is the right fix.

Single-session WSL-encoded files (#2, #5) also carry small numbers
of `C:\` references — 4 and 158. These are probably paste-ins from
user messages or web search results, not authored paths. The
rewrite transform must not assume all encountered paths are
rewritable.

## 7. Open questions, resolved during Phase 2

These were flagged during the spike. Status as of v0.1:

1. **Multi-session JSONLs.** **Resolved: option (a).** `scan_file()`
   surfaces every distinct `sessionId` it sees; `parse_session(path,
   id)` filters to records matching that id. Matches Claude's own
   `/resume` mental model (sessionId is the identity; the file is an
   implementation detail).
2. **Compact-summary record inclusion.** **Resolved: include as-is.**
   `isCompactSummary: true` user records land as normal user messages
   — the summary text IS the recovered content. No
   `--exclude-compact-summaries` flag shipped; no user has asked.
3. **Path-rewrite scope.** **Resolved.** The transform targets prose
   inside `Text` blocks + tool-call `input` fields (e.g.
   `Read.file_path`) + tool-result bodies — **never** the
   `Conversation.cwd` metadata (authoring-env info, not content).
   See the [rewrite scope table](/portaconv/reference/commands/#rewrite-scope).
4. **Oldest Claude Code version supported.** **Still open.** No
   explicit floor declared; the adapter tolerates missing `requestId`
   / `slug` gracefully (tested against 2.0.76 → 2.1.51). Picking a
   formal floor is tracked for a future polish pass.
5. **`progress` record skip.** **Resolved: skip confirmed.** The
   final `assistant` message's `tool_use` / `tool_result` pair
   captures the consolidated subagent output; the streaming
   `progress` records were transient duplicates. Spot-check validated
   — no user-visible loss. Re-evaluate only if a specific
   subagent-paste use case surfaces gaps.

## Appendix A — full top-level field set observed

Union across all six samples, for reference when building the
serde structs:

```
agentId, compactMetadata, content, cwd, data, durationMs,
error, gitBranch, isApiErrorMessage, isCompactSummary, isMeta,
isSidechain, isSnapshotUpdate, isVisibleInTranscriptOnly, level,
logicalParentUuid, message, messageId, operation, parentToolUseID,
parentUuid, permissionMode, requestId, sessionId, slug, snapshot,
sourceToolAssistantUUID, subtype, thinkingMetadata, timestamp,
todos, toolUseID, toolUseResult, type, userType, uuid, version
```

Core set the schema claims first-class: `uuid`, `parentUuid`,
`sessionId`, `cwd`, `gitBranch`, `version`, `timestamp`, `type`,
`message`. Everything else goes into extensions until a future
adapter version promotes it.
