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
- Demo sessions, timeline, original-session open command, Work Capsule, and branch tree
- Serializable core models for future adapters

## Run

```bash
cargo run
```

Useful commands:

```bash
cargo run -- tui
cargo run -- sessions --json
cargo run -- open --session codex-cxcp-design
cargo run -- capsule --json
```

## TUI Keys

| Key | Action |
| --- | --- |
| `j` / `k` | Move selection |
| `gg` / `G` | Jump to top / bottom |
| `tab` / `shift-tab` | Switch panel |
| `/` | Filter sessions by text |
| `f` | Cycle session source filter |
| `o` | Open original session |
| `[` / `]` | Previous / next source CLI and filter sessions by source |
| `{` / `}` | Previous / next target CLI |
| `space` | Set rewind point |
| `c` | Compile capsule |
| `v` | Verify capsule |
| `d` | Toggle diff preview |
| `s` | Cycle compiler skill |
| `enter` | Show launch command |
| `:` | Command mode |
| `?` | Help |
| `q` / `Esc` | Back / quit |

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
