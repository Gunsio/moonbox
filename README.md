# Moonbox 月光宝盒

Moonbox is a cross-CLI session rewind workbench. It reads sessions from tools
such as Codex, Claude, and Hermes, normalizes them into a canonical timeline,
compiles a selected rewind point into a Work Capsule, and launches a new target
CLI branch.

This repository is intentionally not a raw session copier. The source session
is read-only. Compatibility and compression are delegated to replaceable
compiler skills.

## Current State

The first implementation focuses on the product shell:

- Rust + Ratatui standalone binary
- High-density TUI workbench
- Vim-style keyboard navigation
- Time-sorted global session list with source tags
- Source filter defaults to `All`; `Source` is a session-list filter, not a global handoff mode
- Target selection lives inside the launch flow, with explicit `> [x]` radio-list selection
- Target picker validates each target as `READY`, `WARN`, or `BLOCKED`; blocked targets cannot confirm or copy launch commands
- Last confirmed target is persisted in `~/.config/moonbox/config.json`
- Demo sessions, timeline, original-session open command, Work Capsule, and branch tree
- Live `/` session search, combined filter display, and one-key clear with `a`
- Selected/filtered session drives timeline, Work Capsule, branch preview, token budget, and default rewind point
- Per-source demo fixtures with branch, token count, health reason, and session-specific timeline/capsule content
- Fixed status line for action feedback
- Context-aware key bar for the current panel or modal
- Visible rewind marker in the timeline, plus rewind-aware branch and launch preview
- Timeline auto-scroll, Capsule/modal scroll, and small-terminal modal polish
- Copyable launch/original commands via `y` with OSC52 clipboard support
- Serializable core models for future adapters
- `SourceAdapter` contract and demo adapter fixture layer
- File-backed adapter fixtures for Codex, Claude, and Hermes session/timeline parsing
- Canonical Timeline and compiler request/output JSON contract fixtures

## Run

```bash
cargo run
```

Global command after local install:

```bash
moon
```

Useful commands:

```bash
cargo run -- tui
moon tui
cargo run -- tui --filter claude
cargo run -- tui --target codex
cargo run -- sessions --json
cargo run -- open --session codex-cxcp-design
cargo run -- capsule --json
cargo run -- compile-request --json
cargo run -- compile-output --json
```

## Interaction Model

Moonbox has two separate actions for a selected session:

- `o`: open the original session with its original CLI.
- `enter`: choose a target CLI and prepare the handoff launch command.

The main screen is a global session entry point. Sessions are sorted by time and
tagged by source CLI. Source filtering is controlled by `f` or `[` / `]` and
starts at `All`. Target is not shown as a global mode on the main screen; it is
chosen only in the launch picker. In the target picker, `j/k` moves the pending
selection, `enter` confirms and persists it, and `Esc` / `q` cancels without
changing the saved target. The picker keeps every target visible and annotates
each option with `READY`, `WARN`, or `BLOCKED`; blocked targets keep the launch
command disabled until validation passes.

Session search matches id, title, cwd, source, branch, and health reason. When a
different session becomes selected by movement, source filter, or search,
Moonbox reloads that session's timeline, capsule preview, branch preview, and
recommended rewind point.

## TUI Keys

| Key | Action |
| --- | --- |
| `j` / `k` | Move selection |
| `gg` / `G` | Jump to top / bottom |
| `tab` / `shift-tab` | Switch panel |
| `/` | Filter sessions by text |
| `f` | Cycle session source filter |
| `o` | Open original session with original CLI |
| `[` / `]` | Previous / next session source filter |
| `space` | Set rewind point |
| `c` | Compile capsule |
| `v` | Verify capsule |
| `d` | Toggle diff preview |
| `s` | Cycle compiler skill |
| `enter` | Choose target and show handoff launch command |
| `:` | Command mode |
| `?` | Help |
| `q` / `Esc` | Back / quit |

### Target Picker Keys

| Key | Action |
| --- | --- |
| `j` / `k` | Move target selection |
| `enter` | Confirm target and remember it |
| `q` / `Esc` | Cancel without changing target |

## Architecture Direction

```text
Source Adapter -> Canonical Timeline -> Rewind Engine
      -> Capsule Compiler -> Verifier -> Target Launcher
```

Stable interfaces matter more than any single framework:

- `SourceAdapter`: read-only session parsing
- `SkillRunner`: JSON input/output compiler skill execution
- `CapsuleCompiler`: snapshot to Work Capsule
- `TargetLauncher`: create target CLI new branch
- `Verifier`: schema, token, capability, and handoff checks

## TODO

### Completed Milestones

- M0: action feedback, contextual keybar, visible rewind marker, clearer timeline selection.
- M1: modal/capsule scroll, copyable launch/original commands, small-terminal polish.
- M2: serializable core models, `SourceAdapter`, Canonical Timeline, compiler request/output fixtures.
- M3: session-driven detail panes with per-source demo fixtures and searchable branch/health metadata.
- M4: launch validation with target picker READY/WARN/BLOCKED states and blocked command confirmation/copy guards.
- M5: file-backed adapter fixture snapshots for Codex, Claude, and Hermes session/timeline parsing.

### Can Build Now

- M6: target launcher dry-run plus verification loop around generated Work Capsule output.

### Prototype Now, Improve With Real Data

- Launch preview: keep the command structure now, generate exact commands after real adapters exist.
- Session health badges: demo status now, compute from real resume errors and compatibility signals later.

### Best After Real Session Data

- Real session discovery for Codex, Claude, and Hermes.
- Target compatibility checks, disabled target options, and human-readable incompatibility reasons.
- Token budget and compression strategy previews.
- Tool-call, attachment, git diff, and compact-point restoration status.
- Real original-session launching instead of command preview/printing only.
