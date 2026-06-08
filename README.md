# Moonbox 月光宝盒

Moonbox is a cross-CLI session rewind workbench. It reads sessions from tools
such as Codex, Claude, and Hermes, normalizes them into a canonical timeline,
compiles a selected rewind point into a Work Capsule, and launches a new target
CLI branch.

This repository is intentionally not a raw session copier. The source session
is read-only. Compatibility and compression are delegated to replaceable
compiler skills.

## Screenshot

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

Verify the installed binary without reading real local sessions:

```bash
MOONBOX_SESSION_MODE=fixture moon sessions --json --filter codex
MOONBOX_SESSION_MODE=fixture moon doctor --json
moon completions zsh > /tmp/_moon
```

If `moon --help` still says the session list "uses demo data", the global binary
is stale. Reinstall from the current checkout or run through `cargo run --locked
-- ...` while testing local changes.

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
scripts/ci/package-hygiene.sh
cargo package --locked
scripts/ci/release-artifacts-smoke.sh
scripts/ci/install-smoke.sh
```

Production builds deny `unsafe`, `unwrap()`, `expect()`, `panic!`, `todo!`, and
`unimplemented!` through crate-level lint policy. Tests may still use explicit
`expect` messages for fixture setup and assertions.

The Rust library surface is intentionally minimal: downstream users should treat
the installed `moonbox` and `moon` commands as the stable public API. Internal
adapters, compiler plumbing, and TUI state remain crate-private until a library
API is explicitly designed and documented.

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

Release artifact staging is also automated but not published yet:

```bash
scripts/ci/release-artifacts-smoke.sh
scripts/release/stage-artifacts.sh --version 0.1.0
```

The staging script writes source, Cargo crate, and host binary archives plus
`SHA256SUMS` and `release-manifest.json` under `target/release-artifacts/`.

## Project Standards

- [Contributing guide](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [Homebrew release notes](docs/release/homebrew.md)

Pull requests are expected to pass formatting, check, test, fixture replay
eval, documentation build, fixture-safe CLI smoke, docs asset smoke, Homebrew
docs smoke, clippy, release build, package hygiene, package verification,
release artifact staging smoke, install smoke, and cargo-deny supply-chain
gates. GitHub Actions runs the same Rust quality gates and validates that the
README screenshot, install commands, Homebrew planned-state wording, release
artifact staging, and Cargo package contents stay in sync. Smoke gates set
`MOONBOX_SESSION_MODE=fixture`, redirect source homes to `target/`, and never
open or resume real local sessions.

## Current State

The first implementation focuses on the product shell:

- Rust + Ratatui standalone binary
- High-density TUI workbench
- Vim-style keyboard navigation
- Time-sorted global session list with source tags
- Real Codex resume-index discovery from `~/.codex/state_5.sqlite`,
  with rollout fallback from `~/.codex/sessions`
- Codex renamed thread titles from `~/.codex/session_index.jsonl` override
  stale `state_5.sqlite` titles, so Moonbox search/listing follows Codex
  resume-picker names
- Runtime Codex home override via `MOONBOX_CODEX_HOME` or `CODEX_HOME`
- Real Claude resume-index discovery from `~/.claude/history.jsonl`,
  with timeline/details hydrated from `~/.claude/projects`
- Runtime Claude home override via `MOONBOX_CLAUDE_HOME` or `CLAUDE_HOME`
- Real Hermes CLI-resume discovery from `~/.hermes/state.db`
  `source = cli` sessions, with explicit ID lookup still available across
  the Hermes store
- Runtime Hermes home override via `MOONBOX_HERMES_HOME` or `HERMES_HOME`
- Runtime list limit defaults to the newest 200 sessions per real adapter; explicit session lookup still searches the full store
- Set `MOONBOX_SESSION_LIMIT=0` for unlimited real-session list discovery
- Runtime scan entry limit defaults to 5000 filesystem entries for
  JSONL-backed fallback/detail scans, so list and Doctor discovery stay bounded
  on large local stores
- Set `MOONBOX_SESSION_SCAN_LIMIT=0` for unlimited JSONL fallback/detail scans,
  or a positive integer to tune the guardrail
- Runtime summary parsing defaults to the first 800 lines per listed JSONL session, so a few very large sessions cannot stall the global index
- Set `MOONBOX_SESSION_SUMMARY_LINE_LIMIT=0` for full summary parsing, or a positive integer to tune index latency
- TUI timeline preview defaults to the first 300 events per selected session, with a visible truncation marker for large sessions
- Set `MOONBOX_TIMELINE_EVENT_LIMIT=0` for full TUI timeline previews, or a positive integer to tune switching latency
- TUI session filtering is cached and the session list renders only the visible window, so large real indexes do not require formatting every row on every frame
- Set `MOONBOX_SESSION_MODE=fixture` to disable real source stores and force embedded fixture sessions
- Auto discovery only falls back to fixture sessions when no real source stores are present; real and fixture sources are not mixed
- Source filter defaults to `All`; `Source` is a session-list filter, not a global handoff mode
- `moonbox sessions --filter <source>` lists one source while keeping default output as the time-sorted global session index
- `moonbox sessions --json` keeps the stable session array shape and annotates each session with `source_provenance`, `source_path`, and `parse_skip_count`
- Original resume and target handoff are explicit action intents:
  `original_resume` for `open`, `target_handoff` for `launch`
- Target selection lives inside the launch flow, with explicit `> [x]` radio-list selection
- Target picker validates each target as `READY`, `WARN`, or `BLOCKED`; blocked targets cannot confirm or copy launch commands
- Target picker and Handoff Review show verifier-backed readiness rows so users can see the exact PASS/WARN/FAIL signal behind each target state
- `c` refreshes the Work Capsule and opens Handoff Review in one step; the
  previous TUI-only `d Diff` surface is removed to keep the handoff flow linear
- Handoff Review shows the target program, cwd, argument count, exact prompt
  argument, and grouped Source Health / Capsule Health / Target Readiness checks
  before target handoff can launch
- Target handoff uses a dedicated `x` shortcut, with `H` and `t` kept as
  compatibility aliases, and a three-stage TUI flow:
  choose target, review the command, then press `enter` to restore the terminal
  and launch, or `y` to copy it
- Target CLI first prompt is a readable Work Capsule Summary with source,
  target, goal, state, decisions, todo, evidence, risks, and instructions
  instead of a raw single-line JSON blob
- Last confirmed target is persisted in `~/.config/moonbox/config.json`
- Configured SSH hosts can be listed through `moonbox ssh` / `moon ssh`,
  combining Moonbox `ssh_hosts` entries with concrete `Host` aliases from
  `~/.ssh/config` without connecting to remote machines
- Real Codex, Claude, and Hermes resume-surface listing plus timeline parsing
- Original-session open command, Work Capsule, and branch tree previews
- Live `/` session search, combined filter display, and one-key clear with `a`
- Starred sessions with `s` toggle and a `Star` source filter immediately before
  `All` in the filter cycle
- Selected/filtered session drives timeline, Work Capsule, branch preview, token budget, and default rewind point
- Real-session metadata is labeled separately from draft Work Capsule guidance,
  so source store facts are not confused with built-in compiler placeholders
- Right Session Details keeps a compact Handoff Snapshot; full capsule content
  lives in Handoff Review after pressing `c`
- Header token display shows only indexed source token count or `-`, not a fake
  target context budget
- Default rewind selection for real sessions prefers user turns or explicit
  rewind markers instead of assistant/tool output
- Animated TUI loading screen while source sessions are indexed in the background
- Session movement, source filtering, and search keep the list responsive while the selected session preview hydrates in the background
- Session list secondary rows use relative resume-picker timestamps such as
  `16s ago` / `3m ago`, while exact timestamps stay in the right Session
  Details panel
- Resume-index rows with unknown event counts still hydrate their real timeline
  from `source_path`; sessions with no loadable rewind event stay as pending
  capsules instead of crashing the TUI at startup
- Fixture fallback with branch, token count, health reason, and session-specific timeline/capsule content
- Fixed status line for action feedback
- Context-aware key bar for the current panel or modal
- Visible rewind marker in the timeline, plus rewind-aware branch and launch preview
- Timeline parsing folds adjacent duplicate events across Codex, Claude, and
  Hermes so provider double-writes do not render repeated rows
- Timeline rendering folds low-signal tool/function-call rows by default while
  grouping consecutive AI output and keeping rewind selection on user turns or
  explicit rewind markers; selected rows preserve role accent colors so active
  user turns and active AI groups remain visually distinct
- Timeline auto-scroll, Capsule/modal scroll, and small-terminal modal polish
- Copyable launch/original wrapper commands via `y` with OSC52 clipboard
  support; main-list `enter` hands control directly to the selected session's
  original CLI, while `x` opens the target handoff flow
- Serializable core models for future adapters
- `SourceAdapter` contract and fixture-backed adapter fallback layer
- Fallible adapter discovery; bad source data returns structured errors instead of panics
- File-backed adapter fixtures for Codex, Claude, and Hermes session/timeline parsing
- `CapsuleCompiler` trait with fixture and process-backed compiler implementations
- External compiler skill runner via `MOONBOX_COMPILER`, JSON stdin/stdout, structured errors, and timeout handling
- Configurable compiler skill presets with catalog status and quality scores
- Compiler selection prefers explicit environment override, configured default,
  then the first ready external preset before falling back to the built-in
  draft compiler
- `capsule`, `compile-request`, and `compile-output` support explicit
  `--session`, `--target`, `--rewind`, and `--compiler`, so CLI automation can
  inspect the same selected session and rewind as the TUI and launch flow
- Canonical Timeline and compiler request/output JSON contract fixtures
- Target launch dry-run plans with Work Capsule verification reports
- `open --json` and `launch --json` include an `action` discriminator so tooling can distinguish original resume from target handoff
- Single core verifier policy shared by CLI and TUI target validation
- `--capsule` reads a real Work Capsule JSON file when provided; generated dry-run capsules do not pretend to have a file path
- Hardened verifier checks for compiler mode, Work Capsule version, required
  fields, handoff context, risk context, capsule size, target branch markers,
  and execution command preflight
- First-class `moon` binary alias installed alongside `moonbox`
- Shell completion generation for `moonbox` and `moon`
- Non-executing `doctor` diagnostics for config, source adapter provenance,
  session discovery, target binaries, and compiler catalog readiness
- TUI Doctor panel with refresh and JSON copy for the same non-executing
  diagnostics
- Hidden fixture-only `docs-snapshot` maintenance command for regenerating the
  README TUI screenshot from the real Ratatui render buffer
- Fixture session mode for demos, CI, release smoke, and other environments
  that must not read real local session stores
- Fixture-safe TUI render regression tests for main, Doctor, and Launch views
- Deterministic fixture-only replay eval for the Codex/Claude/Hermes source-target matrix plus synthetic verifier regressions
- Fixture-safe public CLI contract tests for the installed `moonbox` and `moon` command surfaces
- Full local quality gate through `scripts/ci/full-gate.sh`
- Cargo-deny supply-chain policy for advisories, duplicate versions, licenses, and crate sources
- Non-test Rust builds deny panic-prone primitives and unsafe code through
  crate-level lint policy
- Minimal documented Rust library API with CLI internals kept crate-private
- README screenshot and install-command smoke coverage through `scripts/ci/docs-assets-smoke.sh`
- Cargo package hygiene smoke coverage through `scripts/ci/package-hygiene.sh`
- Draft Homebrew formula template plus fixture-safe Homebrew docs smoke coverage
- Release artifact staging with source, Cargo crate, host binary archive,
  `SHA256SUMS`, and `release-manifest.json`, covered by fixture-safe smoke
- Fixture-safe installed-binary smoke coverage through
  `scripts/ci/install-smoke.sh`; it installs `moonbox` and `moon` into an
  isolated root, then exercises installed session listing, Doctor diagnostics,
  completion generation, and replay evaluation with source homes redirected
  away from real local session stores
- GitHub Actions CI for Rust quality gates, documentation build, fixture replay eval, fixture-safe CLI smoke, docs asset smoke, Homebrew docs smoke, package verification, release artifact smoke, and install smoke
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
MOONBOX_SESSION_MODE=fixture cargo run -- sessions --filter hermes --json
MOONBOX_SESSION_LIMIT=50 cargo run -- sessions --json
MOONBOX_SESSION_SCAN_LIMIT=1000 cargo run -- doctor --json
MOONBOX_SESSION_SUMMARY_LINE_LIMIT=200 cargo run -- sessions --json
MOONBOX_TIMELINE_EVENT_LIMIT=100 cargo run -- tui
cargo run -- open --session <session-id>
cargo run -- open --session <session-id> --json
cargo run -- open --execute --session <session-id>
cargo run -- capsule --session <session-id> --target hermes --rewind <event-id> --json
cargo run -- compile-request --session <session-id> --target hermes --rewind <event-id> --json
cargo run -- compile-output --session <session-id> --target hermes --rewind <event-id> --compiler <compiler-id> --json
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

`sessions` text output prints the active source filter plus each row's
`real`/`fixture` provenance. JSON output remains an array for compatibility and
adds per-session `source_provenance`, `source_path`, and `parse_skip_count`
fields.

`open`, `launch`, `capsule`, `compile-request`, and `compile-output` are dry-run
by default. Dry-runs may omit `--session` and will preview the newest discovered
session. Passing `--execute` runs the original CLI resume command or verified
target command and therefore requires an explicit `--session`, so automation
cannot accidentally open the newest active session. `verify`, `capsule`,
`compile-request`, and `compile-output` never resume or launch a real process.
`capsule`, `compile-request`, and `compile-output` accept `--session`,
`--target`, `--rewind`, and `--compiler`, so scripts can inspect an exact
selected rewind without relying on the old Codex-to-Hermes fixture defaults.
`doctor` is also
non-executing: it checks config resolution, source adapter provenance, session
summary discovery, target binary availability, and compiler catalog readiness
without loading timelines, resuming sessions, or spawning targets. Its JSON
output includes `source_adapters` entries with provenance, active/missing state,
store path, session count, skipped record count, last indexed timestamp, and
adapter filter status. It also reports list and scan guardrails through
`list_limit`, `scan_entry_limit`, `summary_line_limit`, `scan_entry_count`, and
`scan_truncated`, so a large local store cannot silently degrade into an
unbounded default scan or full-file summary parse. Target
binaries can be overridden with `MOONBOX_CODEX_BIN`,
`MOONBOX_CLAUDE_BIN`, or `MOONBOX_HERMES_BIN` for local testing and custom
installs.

The TUI starts with an animated loading screen while source sessions are
indexed. After indexing, session filtering is cached and the session list is
window-rendered: Moonbox formats only the visible rows around the current
selection instead of rebuilding every row on every frame. The left session list
stays compact for scanning with source-colored `Cdx` / `Clu` / `Hms` badges,
the original source title, and a single secondary line for time and branch.
Healthy source status is not shown in the left rail; only warning or failed
source-index states get a marker. The right Session Details panel keeps the raw
title, cwd, event count, token count, source health, and source path. Session
movement, source filtering, and `/` search move the selected row immediately
and keep the UI responsive while the selected timeline/capsule preview hydrates
in the background from the current session index snapshot.

Session switching uses a bounded timeline preview by default, so very large
JSONL sessions do not freeze navigation. When the preview reaches
`MOONBOX_TIMELINE_EVENT_LIMIT`, Moonbox adds a `Timeline preview truncated`
event to the timeline. Set `MOONBOX_TIMELINE_EVENT_LIMIT=0` only when you
explicitly want full timeline previews in the TUI.

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

In auto mode, Moonbox uses real source stores when any are present. Fixture
sessions are used only when no real Codex, Claude, or Hermes store is available,
which keeps local real-session indexes from being mixed with demo data.

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
      "enabled": true,
      "description": "Compresses source timelines for safe target-CLI continuation.",
      "homepage": "https://github.com/example/moonbox-handoff-compiler",
      "github_stars": 1200
    }
  ],
  "ssh_hosts": [
    {
      "name": "prod-api",
      "host": "prod-api.internal",
      "user": "deploy",
      "port": 22,
      "identity_file": "~/.ssh/prod-api",
      "tags": ["prod"]
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
cargo run -- compile-output --session <session-id> --target hermes --rewind <event-id> --compiler engineering-handoff --json
```

Without configured presets or `MOONBOX_COMPILER`, Moonbox uses the built-in
fixture compiler.

List configured SSH hosts without opening a connection:

```bash
moonbox ssh
moonbox ssh --json
MOONBOX_SSH_CONFIG=/path/to/ssh_config moon ssh --json
```

The SSH inventory reads Moonbox `ssh_hosts` first, then concrete OpenSSH
`Host` aliases from `~/.ssh/config`. It skips wildcard patterns such as
`Host *` and `Host *.internal`, supports simple `Include` files/globs, and
deduplicates by alias with Moonbox config taking precedence.

In the TUI, `{` and `}` switch the main data space between Local and the
configured SSH/devbox entries. Remote spaces are read-only inventory sources:
Moonbox runs `ssh <host> moonbox sessions --json`, imports the returned session
summaries, and never opens or resumes a remote session during switching. The
remote host must have `moonbox` on `PATH`; override it with
`MOONBOX_REMOTE_BIN=/path/to/moonbox` when needed. For local tests, set
`MOONBOX_SSH_CONFIG=/path/to/ssh_config` to point at a fixture config.

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

- `enter`: hand the terminal directly to the selected session's original CLI.
  Moonbox prints the exact command before handoff and, on Unix, replaces itself
  with the source CLI instead of waiting as a parent process.
- `o`: preview an `original_resume` command for the selected session's
  original CLI, then press `enter` to hand the terminal to that CLI. Moonbox
  prints the exact command before handoff and, on Unix, replaces itself with
  the source CLI instead of waiting as a parent process.
- `x`: choose a target CLI, then review a `target_handoff` command before
  launching or copying it. `H` and `t` remain compatibility aliases.

The main screen is a global session entry point. Sessions are sorted by time and
tagged by source CLI. Source filtering is controlled by `f` or `[` / `]` and
starts at `All`. Target is not shown as a global mode on the main screen; it is
chosen only in the launch picker. In the target picker, `j/k` moves the pending
selection, `enter` confirms and persists it, and `Esc` / `q` cancels without
changing the saved target. Confirming a ready or warning target opens a launch
review panel. Pressing `enter` in that review restores the terminal first and
then launches the target CLI; pressing `y` copies the guarded wrapper command.
Pressing `y` in the target picker does not copy anything. The picker keeps
every target visible and annotates each option with `READY`, `WARN`, or
`BLOCKED`; blocked targets keep launch review disabled until validation passes.
The picker uses the same verifier policy as the CLI, so `moon verify` and the
TUI cannot disagree on target readiness. The selected target also shows
readiness detail rows from the verifier report, with blocking failures and
warnings prioritized over pass checks. Press `D` or run `:doctor` to open the
environment Doctor panel; `r` refreshes diagnostics and `y` copies the JSON
report. The panel shows adapter provenance, store path, session count, skipped
record count, and last indexed timestamp. It is read-only and does not load
timelines, resume sessions, launch targets, or spawn target binaries.

Session search matches id, title/raw title, cwd, source path, source, branch,
and health reason. When a different session becomes selected by movement, source filter, or search,
Moonbox immediately marks the selected session as loading, then hydrates that
session's timeline, capsule preview, branch preview, and recommended rewind
point in the background. Timeline rendering hides provider-injected context
rows such as `<environment_context>`, folds low-signal tool rows by default,
groups consecutive AI output into one readable block, right-aligns event times,
and scrolls by wrapped row height so the selected event stays visible.

The target CLI receives a concise, human-readable Work Capsule Summary as its
first prompt. It includes source metadata, selected rewind, goal, state,
decisions, todo, evidence, risks, and execution instructions without dumping the
capsule as raw JSON. Machine-readable capsule data remains available through
the dry-run JSON surfaces and `capsule --json`.

## TUI Keys

| Key | Action |
| --- | --- |
| `j` / `k` | Move selection |
| `gg` / `G` | Jump to top / bottom |
| `tab` / `shift-tab` | Switch panel |
| `/` | Filter sessions by text |
| `f` | Cycle session source filter |
| `o` | Review original resume command |
| `[` / `]` | Previous / next session source filter |
| `s` | Star / unstar selected session |
| `*` | Star / unstar alias |
| `space` | Set rewind point |
| `c` | Refresh capsule and open Handoff Review |
| `v` | Verify capsule |
| `S` | Open Skill Picker |
| `+` / `=` | Zoom focused panel |
| `-` | Restore panel layout |
| `{` / `}` | Previous / next data space: Local or configured SSH/devbox |
| `enter` | Open selected session with original CLI |
| `x` / `H` / `t` | Choose target for handoff |
| `:` | Command mode |
| `?` | Help |
| `q` / `Ctrl-C` | Quit |
| `Esc` | Cancel command/search or close overlays; does not quit from the main screen |

### Target Picker Keys

| Key | Action |
| --- | --- |
| `j` / `k` | Move target selection |
| `enter` | Review target handoff command and remember target |
| `y` | Unavailable before review |
| `q` / `Esc` | Cancel without changing target |

### Handoff Review Keys

| Key | Action |
| --- | --- |
| `y` | Copy guarded `moonbox launch --execute` command |
| `enter` | Restore terminal and launch target CLI |
| `q` / `Esc` | Close review |

### Original Preview Keys

| Key | Action |
| --- | --- |
| `y` | Copy guarded `moonbox open --execute` command |
| `enter` | Hand terminal directly to the original CLI |
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
- M9: real Codex `SourceAdapter` with runtime source registry, bounded
  real-session discovery, `~/.codex/state_5.sqlite` resume index support, and
  rollout fallback from `~/.codex/sessions`.
- M10: source architecture hardening with `WorkbenchData` naming, non-demo workbench APIs, and unbounded explicit Codex session lookup.
- M11: process-backed compiler skill runner with JSON stdin/stdout contract, timeout/failure handling, CLI `--compiler`, and real TUI compile action.
- M12: real Claude `SourceAdapter` with `~/.claude/history.jsonl`
  resume-index ordering, `~/.claude/projects` detail/timeline hydration,
  shared local JSONL utilities, bounded Claude discovery, unbounded explicit
  Claude session lookup, and real Claude timeline parsing.
- M13: real Hermes `SourceAdapter` for `~/.hermes/state.db`, default
  CLI-resume listing from `source = cli` sessions, SQLite message timeline
  parsing, id-based explicit lookup routing across the Hermes store, and
  lightweight CLI launch/verify artifacts for large real stores.
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
- M35: target readiness explanation rows in the TUI launch picker and Handoff Review, backed by verifier report checks with FAIL/WARN priority, READY pass-check context, corrected launch key hints, and render/App tests for blocked, warning, and ready states.
- M36: README screenshot/install polish with a Handoff Review readiness screenshot, transparent SVG canvas, and `docs-assets-smoke` coverage for screenshot semantics, install commands, and unpublished Homebrew wording in both local and GitHub Actions gates.
- M37: generated docs screenshot pipeline with a hidden fixture-only `docs-snapshot` command that renders the real Ratatui Handoff Review buffer to SVG, compares the generated output byte-for-byte in `docs-assets-smoke`, and keeps the command hidden from normal help while covered by CLI contract tests.
- M38: release artifact staging with source, Cargo crate, and host binary archives, generated shell completions, `SHA256SUMS`, `release-manifest.json`, Homebrew source archive URL/checksum guidance, and CI smoke validation without publishing.
- M39: real-session index hardening with fixture fallback only when no real stores exist, CLI `sessions --filter <source>` support, and execute-time guards requiring explicit `--session` before original resume or target handoff can spawn a process.
- M40: adapter health reporting with per-session provenance fields, structured `doctor.source_adapters`, TUI source badges, missing-store reports when real adapters are active, and single-scan inventory plumbing to avoid duplicate source discovery during diagnostics.
- M41: real-store performance guardrails with bounded JSONL scan discovery, Doctor scan-cost fields, animated TUI loading while indexing, and bounded TUI timeline previews so large sessions do not freeze navigation.
- M42: TUI responsiveness and resume-surface hardening with source-colored short
  badges, original source titles, hidden healthy markers, terminal-restored
  original/target launch handoff, compact left session rows, right-side Session
  Details metadata, cached session filters, windowed session-list rendering,
  async selected-session preview loading, stale-result protection for rapid
  navigation, loading guards before launch/verify/compile actions, Codex
  `state_5.sqlite` resume titles, Claude `history.jsonl` resume ordering, and
  Hermes `source = cli` default listing; zero-event resume-index rows now
  hydrate from `source_path`, while truly empty timelines get a pending capsule
  instead of running a compiler against a missing rewind id.
- M43: production panic-boundary enforcement with crate-level lint policy for
  non-test builds, structured replay-eval invariant errors, and panic-free docs
  snapshot rendering.
- M44: public API and package hygiene hardening with crate-private internals,
  documented `moonbox::run()` as the only stable Rust entrypoint, dead-code
  cleanup exposed by the tighter API boundary, Cargo package contents smoke
  validation, stable-width TUI panel titles, real-vs-draft capsule labeling,
  low-signal tool-row folding, and high-signal default rewind selection for
  real sessions; original-session resume now prints the command and execs the
  source CLI on Unix, with main-list `enter` opening original sessions and `x`
  reserved for target handoff while `H` and `t` remain compatibility aliases.
- M45: readable target handoff prompt hardening; target CLIs now receive a
  structured Work Capsule Summary instead of a raw JSON blob, and public CLI
  contracts assert that dry-run launch plans keep this prompt readable.
- M46: parameterized compiler inspection surfaces; `capsule`,
  `compile-request`, and `compile-output` now accept explicit `--session`,
  `--target`, `--rewind`, and `--compiler`, with public CLI contract coverage
  proving they no longer fall back to hard-coded Codex / Hermes / `evt-091`
  defaults.
- M47: timeline duplicate hardening and user-turn rewind anchors; canonical
  parsing now drops adjacent duplicate rows from Codex, Claude, and Hermes
  before rendering, real-session default rewind prefers user turns or explicit
  rewind markers, and TUI `space` rejects assistant/tool rows as rewind anchors.
- M48: read-only SSH inventory; `moonbox ssh` / `moon ssh` list Moonbox
  `ssh_hosts` plus concrete OpenSSH `Host` aliases from `~/.ssh/config` or
  `MOONBOX_SSH_CONFIG`, with JSON/text output and fixture-safe smoke coverage.
- M48.1: timeline polish; provider-injected environment context no longer
  appears as a user turn, event times move to the right side of rows, and
  timeline scrolling accounts for wrapped detail height.
- M48.2: timeline visual grouping; consecutive assistant messages render as
  one source-specific block such as `Codex xN`, `Claude Code xN`, or
  `Hermes xN`, and Timeline navigation moves by visible groups so `j/k` never
  appear to stall inside a folded assistant burst.
- M48.3: timeline selected-state polish; active user rows keep the blue user
  accent, active AI groups keep the gold AI accent, and rewind selection still
  overrides to the gold rewind accent.
- M48.4: session-list timestamp polish; left list secondary rows now use
  resume-picker style relative time while the right Session Details panel keeps
  the exact `updated` timestamp.
- M49: TUI handoff flow consolidation and starred sessions; `c` now refreshes
  the capsule and opens Handoff Review, TUI `d Diff` is removed, `s` persists
  starred session ids (`*` remains an alias), the source filter cycle includes
  `Star` immediately before `All`, the right panel is reduced to a compact
  Handoff Snapshot, fake `/ 100K` token budget text is removed, and `Action
  Path` replaces the misleading branch-tree copy.
- M49.1: session-list marker spacing polish; selection arrows now render inside
  the row, and star/status markers only appear when meaningful, removing the
  large empty gap before `Cdx` / `Clu` / `Hms` source badges.
- M49.2: timeline and skill-picker polish; folded assistant groups now name the
  source CLI instead of generic `AI`, `S` opens a metadata-rich Skill Picker
  with status, description, stars / `n/a`, and link/command reference, and
  `Action Path` shows the selected cwd plus per-tool session counts.
- M50: panel zoom and focus layout; `+` / `=` zooms the focused Sessions,
  Timeline, Details, or Action Path panel, `-` restores the normal layout, and
  tab navigation keeps zoom attached to the active panel without resetting
  selection or scroll state.
- M51: local/devbox data-space switching; `{` / `}` cycles the main TUI between
  Local and configured SSH/devbox data spaces, remote spaces load read-only
  session inventory through `ssh <host> moonbox sessions --json`, and failures
  surface as explicit status messages without opening, resuming, or launching
  sessions.
- M52: production compiler and verifier chain hardening; compiler selection now
  prefers explicit environment override, configured default, then ready external
  presets before built-in draft fallback, built-in compilers warn for real
  handoffs, Handoff Review shows target program/cwd/args/exact prompt plus
  grouped Source Health / Capsule Health / Target Readiness checks, and Codex
  renamed thread titles from `session_index.jsonl` override stale
  `state_5.sqlite` titles in CLI/TUI listings.
- M53: release and distribution readiness; README installation verification now
  includes fixture-safe installed `moon` checks, install smoke validates
  installed session listing, Doctor diagnostics, completion generation, and
  replay evaluation, release artifact/Homebrew docs gates remain wired into the
  full quality gate, and Timeline cursor vs selected rewind markers are visually
  distinct so the workbench does not appear to have two active rows.

### Remaining Milestones

- No implementation milestone remains in this branch. Homebrew publication,
  release tagging, and registry publication stay gated until explicit approval
  after final validation.

### Can Build Now

- Use the staged release artifacts and generated checksums for a tagged release
  once publication is accepted.

### Prototype Now, Improve With Real Data

- Session health signals: left rail now hides healthy adapter status and only
  calls out warning/failed indexing states; compute richer resume-error and
  compatibility signals later.
- Compression strategy previews: show selected compiler, expected capsule
  shape, and budget warnings now; tune thresholds from real handoff outcomes
  later.

### Best After Real Session Data

- Token budget thresholds and compression warning calibration from real handoff
  outcomes.
- Tool-call, attachment, git diff, and compact-point restoration status.
