# Moonbox 月光宝盒

Moonbox is a cross-CLI session rewind workbench. It reads sessions from tools
such as Codex, Claude, and Hermes, normalizes them into a canonical timeline,
compiles a selected rewind point into a Work Capsule, and launches a new target
CLI branch.

This repository is intentionally not a raw session copier. The source session
is read-only. Compatibility and compression are delegated to replaceable
compiler skills.

![Moonbox TUI screenshot](docs/assets/moonbox-tui.svg)

## Install

### Cargo

Install the current repository version from Git:

```bash
cargo install --git https://github.com/Gunsio/moonbox
moonbox --version
moon --version
```

The package installs both `moonbox` and the short `moon` alias. From a local
checkout, install the same two binaries with:

```bash
cargo install --path . --locked
```

### Source Checkout

Requires Rust 1.88 or newer.

```bash
git clone https://github.com/Gunsio/moonbox.git
cd moonbox
cargo run --locked -- tui
```

For local development:

```bash
scripts/ci/full-gate.sh
```

That script runs patch hygiene plus the CI/release gates. It expects a clean
worktree for `cargo package --locked`; during pre-commit iteration use
`MOONBOX_PACKAGE_ALLOW_DIRTY=1 scripts/ci/full-gate.sh`, then rerun it without
the override after committing. It also requires `cargo-deny`; install it with
`cargo install --locked cargo-deny`, or set `CARGO_DENY=/path/to/cargo-deny`
when using a downloaded binary.

Individual gates:

```bash
git diff --check
scripts/ci/supply-chain.sh
cargo fmt --check
cargo check --locked
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
cargo run --locked -- replay-eval --json
scripts/ci/cli-smoke.sh
scripts/ci/docs-assets-smoke.sh
scripts/ci/homebrew-docs-smoke.sh
cargo clippy --locked -- -D warnings
cargo build --release --locked
cargo package --locked
scripts/ci/install-smoke.sh
```

### Homebrew

Homebrew distribution is planned, but not published yet. After the accepted
release is tagged, the intended install path is:

```bash
brew tap Gunsio/tap
brew install moonbox
```

See [docs/release/homebrew.md](docs/release/homebrew.md) for the release
checklist and formula shape. The draft formula lives at
[docs/release/homebrew/moonbox.rb](docs/release/homebrew/moonbox.rb), and its
syntax plus completion-generation behavior are covered by:

```bash
scripts/ci/homebrew-docs-smoke.sh
```

## Project Standards

- [Contributing guide](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [Homebrew release notes](docs/release/homebrew.md)

Pull requests are expected to pass formatting, check, test, fixture replay
eval, documentation build, fixture-safe CLI smoke, docs asset smoke, Homebrew
docs smoke, clippy, release build, package verification, install smoke, and
cargo-deny supply-chain gates. GitHub Actions runs the same Rust quality gates
and validates that the README screenshot, install commands, and Homebrew
planned-state wording stay in sync. Smoke gates set `MOONBOX_SESSION_MODE=fixture`,
redirect source homes to `target/`, and never open or resume real local
sessions.

## Current State

The first implementation focuses on the product shell:

- Rust + Ratatui standalone binary
- High-density TUI workbench
- Vim-style keyboard navigation
- Time-sorted global session list with source tags
- Real Codex session discovery from `~/.codex/sessions`
- Runtime Codex home override via `MOONBOX_CODEX_HOME` or `CODEX_HOME`
- Real Claude session discovery from `~/.claude/projects`
- Runtime Claude home override via `MOONBOX_CLAUDE_HOME` or `CLAUDE_HOME`
- Real Hermes session discovery from `~/.hermes/state.db`, with optional metadata from `~/.hermes/sessions/sessions.json`
- Runtime Hermes home override via `MOONBOX_HERMES_HOME` or `HERMES_HOME`
- Runtime list limit defaults to the newest 200 sessions per real adapter; explicit session lookup still searches the full store
- Set `MOONBOX_SESSION_LIMIT=0` for unlimited real-session list discovery
- Set `MOONBOX_SESSION_MODE=fixture` to disable real source stores and force embedded fixture sessions
- Source filter defaults to `All`; `Source` is a session-list filter, not a global handoff mode
- Original resume and target handoff are explicit action intents:
  `original_resume` for `open`, `target_handoff` for `launch`
- Target selection lives inside the launch flow, with explicit `> [x]` radio-list selection
- Target picker validates each target as `READY`, `WARN`, or `BLOCKED`; blocked targets cannot confirm or copy launch commands
- Target picker and Launch Review show verifier-backed readiness rows so users can see the exact PASS/WARN/FAIL signal behind each target state
- Target handoff uses a two-stage TUI flow: choose target, review the execute command, then press `y` to copy it
- Last confirmed target is persisted in `~/.config/moonbox/config.json`
- Real Codex, Claude, and Hermes timeline parsing
- Original-session open command, Work Capsule, and branch tree previews
- Live `/` session search, combined filter display, and one-key clear with `a`
- Selected/filtered session drives timeline, Work Capsule, branch preview, token budget, and default rewind point
- Fixture fallback with branch, token count, health reason, and session-specific timeline/capsule content
- Fixed status line for action feedback
- Context-aware key bar for the current panel or modal
- Visible rewind marker in the timeline, plus rewind-aware branch and launch preview
- Timeline auto-scroll, Capsule/modal scroll, and small-terminal modal polish
- Copyable launch/original commands via `y` with OSC52 clipboard support; target handoff copy is only available from the launch review panel
- Serializable core models for future adapters
- `SourceAdapter` contract and fixture-backed adapter fallback layer
- Fallible adapter discovery; bad source data returns structured errors instead of panics
- File-backed adapter fixtures for Codex, Claude, and Hermes session/timeline parsing
- `CapsuleCompiler` trait with fixture and process-backed compiler implementations
- External compiler skill runner via `MOONBOX_COMPILER`, JSON stdin/stdout, structured errors, and timeout handling
- Configurable compiler skill presets with catalog status and quality scores
- Canonical Timeline and compiler request/output JSON contract fixtures
- Target launch dry-run plans with Work Capsule verification reports
- `open --json` and `launch --json` include an `action` discriminator so tooling can distinguish original resume from target handoff
- Single core verifier policy shared by CLI and TUI target validation
- `--capsule` reads a real Work Capsule JSON file when provided; generated dry-run capsules do not pretend to have a file path
- Hardened verifier checks for Work Capsule version, required fields, handoff context, risk context, capsule size, target branch markers, and execution command preflight
- First-class `moon` binary alias installed alongside `moonbox`
- Shell completion generation for `moonbox` and `moon`
- Non-executing `doctor` diagnostics for config, session discovery, target
  binaries, and compiler catalog readiness
- TUI Doctor panel with refresh and JSON copy for the same non-executing
  diagnostics
- Fixture session mode for demos, CI, release smoke, and other environments
  that must not read real local session stores
- Fixture-safe TUI render regression tests for main, Doctor, and Launch views
- Deterministic fixture-only replay eval for the Codex/Claude/Hermes source-target matrix plus synthetic verifier regressions
- Fixture-safe public CLI contract tests for the installed `moonbox` and `moon` command surfaces
- Full local quality gate through `scripts/ci/full-gate.sh`
- Cargo-deny supply-chain policy for advisories, duplicate versions, licenses, and crate sources
- README screenshot and install-command smoke coverage through `scripts/ci/docs-assets-smoke.sh`
- Draft Homebrew formula template plus fixture-safe Homebrew docs smoke coverage
- GitHub Actions CI for Rust quality gates, documentation build, fixture replay eval, fixture-safe CLI smoke, docs asset smoke, Homebrew docs smoke, package verification, and install smoke
- Dependabot configuration for Cargo and GitHub Actions updates
- Contributing, security, changelog, issue template, and PR template docs

## Run

```bash
moonbox
moon
```

Useful commands:

```bash
cargo run -- tui
MOONBOX_SESSION_MODE=fixture cargo run -- sessions --json
moon tui
cargo run -- tui --filter claude
cargo run -- tui --target codex
cargo run -- sessions --json
MOONBOX_SESSION_LIMIT=50 cargo run -- sessions --json
cargo run -- open --session <session-id>
cargo run -- open --session <session-id> --json
cargo run -- open --execute --session <session-id>
cargo run -- capsule --json
cargo run -- compile-request --json
cargo run -- compile-output --json
cargo run -- compile-output --compiler <compiler-id> --json
cargo run -- compilers --json
cargo run -- doctor --json
cargo run -- completions bash
cargo run -- completions zsh --bin moon
cargo run -- replay-eval --json
cargo run -- launch --target hermes --session <session-id> --json
cargo run -- launch --execute --target hermes --session <session-id>
cargo run -- verify --target hermes --session <session-id> --capsule ./capsule.json --json
cargo run -- verify --target hermes --session hermes-cxcp-502 --json
```

`open` and `launch` are dry-run by default. Passing `--execute` runs the
original CLI resume command or verified target command. `verify` never resumes
or launches a real process. `doctor` is also non-executing: it checks config
resolution, session summary discovery, target binary availability, and compiler
catalog readiness without loading timelines, resuming sessions, or spawning
targets. Target binaries can be overridden with `MOONBOX_CODEX_BIN`,
`MOONBOX_CLAUDE_BIN`, or `MOONBOX_HERMES_BIN` for local testing and custom
installs.

`replay-eval` is also non-executing. It uses only embedded fixtures, does not
scan local session stores, and reports verifier signals across every
source-target pair plus safe synthetic regressions for target mismatch,
oversized capsules, and missing target commands. The JSON output includes
matrix and synthetic case counts plus a coverage table for expected scenarios.

For demos, release smoke, or any automation that must not touch real local
session stores, force fixture mode:

```bash
MOONBOX_SESSION_MODE=fixture moonbox sessions --json
MOONBOX_SESSION_MODE=fixture moonbox doctor --json
```

Fixture mode disables real Codex, Claude, and Hermes source adapters even if
their default stores or `MOONBOX_*_HOME` overrides exist. Supported values are
`auto` and `fixture`; `real` is accepted as an alias for `auto`, and `demo` /
`fixtures` are accepted as aliases for `fixture`.

External compiler skills are optional. When configured, Moonbox sends a
`CapsuleCompileRequest` JSON object to the process stdin and expects a
`CapsuleCompileOutput` JSON object on stdout. Durable presets live in
`~/.config/moonbox/config.json`:

```json
{
  "default_compiler": "engineering-handoff",
  "compiler_presets": [
    {
      "id": "engineering-handoff",
      "command": "/path/to/moonbox-handoff-compiler",
      "args": ["--mode", "handoff"],
      "timeout_ms": 30000,
      "enabled": true
    }
  ]
}
```

List the current compiler catalog with:

```bash
moonbox compilers
moonbox compilers --json
```

Catalog entries include their source (`Environment`, `Config`, or `Builtin`),
status (`Ready`, `Warning`, or `Disabled`), score, command, arguments, timeout,
and the reason behind the quality signal.

Environment variables remain the highest-priority one-off override:

```bash
MOONBOX_COMPILER=/path/to/compiler \
MOONBOX_COMPILER_ID=engineering-handoff \
MOONBOX_COMPILER_ARGS='["--mode","handoff"]' \
MOONBOX_COMPILER_TIMEOUT_MS=30000 \
cargo run -- compile-output --compiler engineering-handoff --json
```

Without configured presets or `MOONBOX_COMPILER`, Moonbox uses the built-in
fixture compiler.

Generate shell completions with:

```bash
moonbox completions bash > moonbox.bash
moonbox completions zsh --bin moon > _moon
moon completions fish > moon.fish
moon completions powershell > _moon.ps1
moonbox completions elvish > moonbox.elv
```

Supported shells are Bash, Zsh, Fish, PowerShell, and Elvish. The generated
binary name defaults to the executable you invoked (`moonbox` or `moon`) and can
be overridden with `--bin moonbox` or `--bin moon`.

## Interaction Model

Moonbox has two separate actions for a selected session:

- `o`: preview an `original_resume` command for the selected session's
  original CLI.
- `enter`: choose a target CLI, then review a `target_handoff` command before
  copying it.

The main screen is a global session entry point. Sessions are sorted by time and
tagged by source CLI. Source filtering is controlled by `f` or `[` / `]` and
starts at `All`. Target is not shown as a global mode on the main screen; it is
chosen only in the launch picker. In the target picker, `j/k` moves the pending
selection, `enter` confirms and persists it, and `Esc` / `q` cancels without
changing the saved target. Confirming a ready or warning target opens a launch
review panel; only that panel exposes the copyable execute command. Pressing
`y` in the target picker does not copy anything. The picker keeps every target
visible and annotates each option with `READY`, `WARN`, or `BLOCKED`; blocked
targets keep launch review disabled until validation passes. The picker uses
the same verifier policy as the CLI, so `moon verify` and the TUI cannot
disagree on target readiness. The selected target also shows readiness detail
rows from the verifier report, with blocking failures and warnings prioritized
over pass checks. Press `D` or run `:doctor` to open the environment Doctor
panel; `r` refreshes diagnostics and `y` copies the JSON report. The panel is
read-only and does not load timelines, resume sessions, launch targets, or
spawn target binaries.

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
| `o` | Preview original resume command |
| `[` / `]` | Previous / next session source filter |
| `space` | Set rewind point |
| `c` | Compile capsule |
| `v` | Verify capsule |
| `d` | Toggle diff preview |
| `s` | Cycle compiler skill |
| `enter` | Choose target for handoff |
| `:` | Command mode |
| `?` | Help |
| `q` / `Esc` | Back / quit |

### Target Picker Keys

| Key | Action |
| --- | --- |
| `j` / `k` | Move target selection |
| `enter` | Review target handoff command and remember target |
| `y` | Unavailable before review |
| `q` / `Esc` | Cancel without changing target |

### Launch Review Keys

| Key | Action |
| --- | --- |
| `y` | Copy guarded `moonbox launch --execute` command |
| `enter` | Disabled; review is copy-only |
| `q` / `Esc` | Close review |

### Original Preview Keys

| Key | Action |
| --- | --- |
| `y` | Copy guarded `moonbox open --execute` command |
| `enter` | Disabled; preview is copy-only |
| `q` / `Esc` | Close preview |

## Architecture Direction

```text
Source Adapter -> Canonical Timeline -> Rewind Engine
      -> Capsule Compiler -> Verifier -> Target Launcher
```

Stable interfaces matter more than any single framework:

- `SourceAdapter`: read-only session parsing
- `CapsuleCompiler`: snapshot to Work Capsule; fixture and process runners exist now
- `Verifier`: schema, token, capability, handoff, size, and execution preflight checks; shared by CLI/TUI
- `SkillRunner`: JSON input/output compiler skill execution through a process runner
- `TargetLauncher`: target-specific command construction and guarded process execution

## TODO

### Completed Milestones

- M0: action feedback, contextual keybar, visible rewind marker, clearer timeline selection.
- M1: modal/capsule scroll, copyable launch/original commands, small-terminal polish.
- M2: serializable core models, `SourceAdapter`, Canonical Timeline, compiler request/output fixtures.
- M3: session-driven detail panes with per-source fixtures and searchable branch/health metadata.
- M4: launch validation with target picker READY/WARN/BLOCKED states and blocked command confirmation/copy guards.
- M5: file-backed adapter fixture snapshots for Codex, Claude, and Hermes session/timeline parsing.
- M6: target launcher dry-run plus Work Capsule verification loop.
- M7: core boundary hardening with fallible adapters, shared verifier policy, real `--capsule` file validation, and a `CapsuleCompiler` trait.
- M8: open-source hygiene with CI, dependency automation, contribution docs, security policy, changelog, and GitHub templates.
- M9: real Codex `SourceAdapter` for `~/.codex/sessions`, runtime source registry, and bounded real-session discovery.
- M10: source architecture hardening with `WorkbenchData` naming, non-demo workbench APIs, and unbounded explicit Codex session lookup.
- M11: process-backed compiler skill runner with JSON stdin/stdout contract, timeout/failure handling, CLI `--compiler`, and real TUI compile action.
- M12: real Claude `SourceAdapter` for `~/.claude/projects`, shared local JSONL adapter utilities, bounded Claude discovery, unbounded explicit Claude session lookup, and real Claude timeline parsing.
- M13: real Hermes `SourceAdapter` for `~/.hermes/state.db`, optional `sessions.json` enrichment, SQLite message timeline parsing, id-based explicit lookup routing, and lightweight CLI launch/verify artifacts for large real stores.
- M14: guarded target launcher execution with `launch --execute`, target-specific Codex/Claude/Hermes command generation, structured `target_command` JSON, binary overrides, verification blocking before spawn, and TUI copy commands that execute through Moonbox.
- M15: guarded original-session execution with `open --execute`, structured original open plan JSON, source-specific Codex/Claude/Hermes resume commands, corrected Hermes resume command generation, binary overrides, and TUI copy commands that execute through Moonbox.
- M16: configurable compiler skill presets with `default_compiler`, catalog status/score signals, `moonbox compilers`, environment override precedence, and stricter unknown/disabled compiler errors.
- M17: verifier hardening with Work Capsule version and required-field checks, handoff-context actionability, risk-context warnings, capsule-size thresholds, stricter target-branch validation, and execute-time target command preflight before spawn.
- M18: deterministic fixture-only replay eval covering the Codex/Claude/Hermes source-target matrix, with JSON/text CLI output and verifier signal counts without scanning or opening real sessions.
- M19: release gate hardening with fixture replay eval and `cargo package --locked` wired into GitHub Actions and the PR verification checklist.
- M20: fixture-safe CLI command smoke gate that overrides source homes to `target/moonbox-smoke-home`, validates non-executing command surfaces, and runs in GitHub Actions plus the PR checklist.
- M21: first-class `moon` binary alias via a shared library entrypoint, preserving `cargo run` default behavior and adding smoke coverage for the alias.
- M22: fixture-safe install smoke gate that runs `cargo install --path . --root target/moonbox-install-smoke --locked --offline --force`, verifies installed `moonbox` and `moon`, and checks installed `moon replay-eval --json` without scanning or opening real sessions.
- M23: fixture-safe integration tests for public CLI contracts, covering `moonbox`/`moon` version parity, fixture-only replay eval, fixture fallback session listing, and dry-run open/launch/verify JSON behavior.
- M24: documentation build gate with `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps` in CI, PR checklist, README, and contributor docs.
- M25: full local quality gate script that runs patch hygiene plus CI/release checks, with dirty-worktree package verification available during pre-commit iteration.
- M26: cargo-deny supply-chain gate for RustSec advisories, yanked crates, duplicate-version policy, license allowlists, and trusted crate sources.
- M27: shell completion generation for Bash, Zsh, Fish, PowerShell, and Elvish, with fixture-safe CLI contract and smoke coverage for both `moonbox` and `moon`.
- M28: non-executing `doctor` diagnostics for config readiness, session discovery, target binaries, and compiler catalog health.
- M29: TUI Doctor panel with status header, refresh, JSON copy, and shared non-executing diagnostics.
- M30: fixture-safe Ratatui render regression tests for the main workbench, Doctor overlay, and Launch overlay.
- M31: release docs hardening with local install commands, a draft Homebrew formula template, and a fixture-safe Homebrew docs smoke gate.
- M32: explicit fixture session mode through `MOONBOX_SESSION_MODE=fixture`, surfaced in Doctor diagnostics and wired into smoke scripts to prevent accidental real-session discovery.
- M33: action intent hardening with `original_resume` / `target_handoff` dry-run discriminators, two-stage TUI launch review, original-preview copy-only behavior, and contract/render tests for both paths.
- M34: fixture replay corpus expansion with 9 source-target matrix cases plus 3 synthetic regressions for target mismatch, oversized capsule, and missing-tool preflight; replay output now includes case kind, scenario, capsule target, coverage rows, and updated fixture-safe CLI smoke/contract checks.
- M35: target readiness explanation rows in the TUI launch picker and Launch Review, backed by verifier report checks with FAIL/WARN priority, READY pass-check context, corrected launch key hints, and render/App tests for blocked, warning, and ready states.
- M36: README screenshot/install polish with a Launch Review readiness screenshot, transparent SVG canvas, and `docs-assets-smoke` coverage for screenshot semantics, install commands, and unpublished Homebrew wording in both local and GitHub Actions gates.

### Can Build Now

- Add a generated screenshot pipeline if Ratatui snapshot export becomes worth the maintenance cost.

### Prototype Now, Improve With Real Data

- Session health badges: basic adapter status now, compute from real resume errors and compatibility signals later.
- Compression strategy previews: show selected compiler, expected capsule
  shape, and budget warnings now; tune thresholds from real handoff outcomes
  later.

### Best After Real Session Data

- Token budget thresholds and compression warning calibration from real handoff
  outcomes.
- Tool-call, attachment, git diff, and compact-point restoration status.
