# Changelog

All notable changes to Moonbox will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses semantic versioning once tagged releases start.

## [Unreleased]

### Added

- Root agent operating rules covering one-milestone-per-PR governance, fixture-safe
  tests, read-only source stores, and documentation sync expectations.
- Rust + Ratatui TUI workbench with Vim-style navigation.
- Global session list with source tags, search, source filtering, original
  session preview, and target picker.
- Canonical Timeline, Work Capsule, compile request, compile output, launch
  plan, and verification report models.
- File-backed fixture adapters for Codex, Claude, and Hermes.
- Real read-only Codex adapter for the `~/.codex/state_5.sqlite` resume thread
  index, with rollout fallback from `~/.codex/sessions`.
- Real read-only Claude adapter for the `~/.claude/history.jsonl` resume index,
  with timeline and detail hydration from `~/.claude/projects`.
- Real read-only Hermes adapter for `~/.hermes/state.db`, listing all
  non-archived Hermes sources by default while keeping explicit ID lookup
  available across the wider Hermes store.
- Fallible `SourceAdapter` discovery.
- Replaceable `CapsuleCompiler` trait with fixture and process-backed compilers.
- External compiler runner using JSON stdin/stdout, timeout handling, and
  structured process errors.
- Guarded target launcher execution through `launch --execute`.
- Target-specific Codex, Claude, and Hermes command generation with structured
  `target_command` plan output.
- Guarded original-session execution through `open --execute`.
- Structured original open plan output through `open --json`.
- `action` discriminators in dry-run `open --json`, `open-app --json`, and
  `launch --json` output, using `original_resume`, `app_deep_link`, and
  `target_handoff`.
- First-class `moon` binary alias installed alongside `moonbox`.
- Configurable compiler skill presets in `~/.config/moonbox/config.json`,
  including optional description, homepage, and GitHub stars metadata for the
  TUI Skill Picker.
- Compiler catalog output through `moonbox compilers`, including source,
  status, score, command, arguments, timeout, and quality reason.
- Hardened verification checks for Work Capsule version, required fields,
  handoff context, risk context, capsule size, handoff label markers, and
  execute-time target command preflight.
- Fixture-only replay evaluation through `moonbox replay-eval`, covering every
  Codex/Claude/Hermes source-target pair without scanning or opening local
  session stores.
- Synthetic fixture replay regressions for target mismatch, oversized capsule,
  and missing-tool preflight, with explicit scenario coverage in replay output.
- CI gates for fixture replay evaluation and `cargo package --locked`
  verification.
- Fixture-safe CLI smoke script for non-executing command surfaces, with source
  homes redirected away from real local session stores.
- Fixture-safe install smoke script that verifies `cargo install --path`
  produces working `moonbox` and `moon` executables, then exercises installed
  session listing, Doctor diagnostics, completion generation, and replay
  evaluation with source homes redirected away from real local session stores.
- Fixture-safe integration tests for public CLI contracts across `moonbox` and
  `moon` binaries.
- Documentation build gate with rustdoc warnings treated as errors.
- Full local quality gate script for patch hygiene plus CI/release checks.
- Cargo package hygiene gate that validates expected package contents and
  rejects editor backups, rejected patches, temporary files, and build
  directories before release packaging.
- Supply-chain gate with cargo-deny advisories, duplicate-version, license, and
  source checks.
- Shell completion generation for Bash, Zsh, Fish, PowerShell, and Elvish.
- Non-executing `moonbox doctor` diagnostics for config, session discovery,
  target binaries, and compiler catalog readiness.
- `MOONBOX_SESSION_MODE=fixture` to force embedded fixture sessions and disable
  real source-store discovery in demos, CI, and release smoke.
- TUI Doctor panel with `D` / `:doctor`, refresh, and JSON copy support for
  the same non-executing diagnostics.
- Fixture-safe TUI render regression tests for main, Doctor, and Launch views.
- Verifier-backed TUI target readiness explanation rows in the launch picker
  and Handoff Review.
- Documentation asset smoke coverage for README screenshot semantics,
  installation commands, and unpublished Homebrew wording.
- Hidden fixture-only `docs-snapshot` maintenance command that renders the real
  Ratatui Handoff Review buffer to SVG for the README screenshot asset.
- Draft Homebrew formula template plus a fixture-safe Homebrew docs smoke gate.
- Release artifact staging script that produces source, Cargo crate, and host
  binary archives, generated shell completions, `SHA256SUMS`, and
  `release-manifest.json` without publishing.
- Fixture-safe release artifact smoke gate that validates staged checksums,
  manifest metadata, and archive contents.
- Source filtering for the public `moonbox sessions` command through
  `--filter <source>`.
- Per-session source provenance fields in `moonbox sessions --json`:
  `source_provenance`, `source_path`, and `parse_skip_count`.
- Structured Doctor source adapter reports under `source_adapters`, including
  provenance, active/missing state, store path, session count, skipped record
  count, last indexed timestamp, and adapter filter status.
- Versioned source adapter capability reports for local store, rich local RPC,
  cloud metadata, deep links, export/search, remote control, fork/resume, and
  native handoff support, plus per-session `runtime_status` / `runtime_reason`
  fields that keep live runtime activity separate from `updated_at`.
- Bounded JSONL real-store discovery through `MOONBOX_SESSION_SCAN_LIMIT`,
  with Doctor scan-cost fields for list limit, scan entry limit, visited entry
  count, and truncation state.
- Bounded JSONL session summary parsing through
  `MOONBOX_SESSION_SUMMARY_LINE_LIMIT`, so listed large sessions do not require
  full-file parsing before the TUI becomes usable.
- Bounded TUI timeline previews through `MOONBOX_TIMELINE_EVENT_LIMIT`, with a
  visible truncation marker for large sessions.
- Bounded timeline event body previews through
  `MOONBOX_TIMELINE_DETAIL_CHAR_LIMIT`, defaulting to 4000 characters so zoomed
  Timeline panels can show long-form context without unbounded transcript
  rendering.
- Animated TUI loading screen while source sessions are indexed before the
  workbench becomes interactive.
- Async selected-session preview hydration in the TUI, with stale-result
  protection for rapid navigation and loading guards before launch, verify,
  compile, rewind, or original-resume preview actions.
- Cached TUI session filters plus windowed session-list rendering, so large
  real indexes do not require re-filtering and re-formatting every row on every
  frame.
- Shared verifier policy for CLI and TUI launch validation.
- Real `--capsule` file parsing and target mismatch verification.
- README screenshot, installation notes, and Homebrew release planning docs.
- Production panic-boundary lint policy for non-test builds, denying `unsafe`,
  `unwrap()`, `expect()`, `panic!`, `todo!`, and `unimplemented!`.
- Minimal documented Rust library entrypoint through `moonbox::run()`, with
  CLI internals kept crate-private until a deliberate library API is stabilized.
- TUI real-vs-draft labeling that separates real source metadata from built-in
  Work Capsule draft guidance.
- Read-only SSH inventory through `moonbox ssh` / `moon ssh`, combining
  Moonbox `ssh_hosts` config entries with concrete OpenSSH `Host` aliases from
  `~/.ssh/config` or `MOONBOX_SSH_CONFIG` without opening remote connections.
- TUI data spaces backed by local session stores plus configured SSH/devbox
  inventories, switchable with `{` / `}` without opening, resuming, or
  launching any remote session.
- Handoff Review target-input preview, including target program, cwd, argument
  count, and the exact prompt argument that will be passed to the target CLI.
- Verifier `compiler_mode` checks that mark built-in draft compilers as a
  warning for real source handoffs.
- Workspace continuation snapshots through `moonbox snapshot`, capturing git
  HEAD, branch, staged/unstaged/untracked paths, bounded diff previews, key
  project files, environment summary, and explicit test-command results without
  reading or opening source CLI sessions.
- Auditable Capsule source maps with `raw_source_map`, `raw_refs`, and
  `coverage` fields, enriched from the canonical timeline for built-in and
  external compiler output.
- Redaction policy reports on Capsule compile requests and Work Capsules,
  covering secret-like value scanning, sensitive path masking, event/file
  allowlists, prompt-injection warnings, and external compiler disclosure.
- Semantic verifier checks for raw source map consistency, compiler coverage
  gaps, todo/timeline event references, local file references, and
  patch-shaped diff evidence.
- Fixture adapter contract coverage for session summary fields, report metadata,
  timeline schema version, unique event ids, and user/rewind anchors.
- Continuation protocol plans in `launch --json`, covering explicit
  `prompt_only` target input, unsupported native Capsule import requests, and
  preview-only reversible branch/worktree workspace restore commands.
- `launch` and `verify` options for `--continuation` and
  `--workspace-restore`, with fixture-safe contract coverage proving unsupported
  import/restore requests are blocked instead of silently downgraded.
- Explicit opt-in Codex app-server source adapter support through
  `MOONBOX_CODEX_APP_SERVER_FIXTURE` or `MOONBOX_CODEX_APP_SERVER_PROXY=1`,
  preferring `thread/list`, `thread/read`, and `thread/turns/list` data while
  keeping local SQLite/JSONL as fallback.
- Non-executing `moonbox open-app` / `moon open-app` plans that preview
  `codex://threads/<id>` deep links for Codex sessions without launching the
  desktop app.
- Claude stream-json / SDK transcript metadata parsing for captured local JSONL
  records, including `system` init, `result`, `session_id`, cost, duration, API
  duration, turn count, hook events, partial stream events, fork parent metadata,
  and remote / remote-control observability records without invoking Claude.
- Hermes all-source inventory metadata in `sessions --json` through
  serde-default `provider_metadata`, including source, platform, user id,
  session key, parent session id, origin metadata, model config, system prompt
  snapshot, handoff state, archived state, and token breakdown.
- `moonbox sessions --hermes-source <source>` / `moon sessions --hermes-source
  <source>` provider-source filtering for Hermes inventories, with comma or
  repeated values and aliases such as `api-server`.

### Changed

- Work Capsule and launch plan JSON now emit `handoff_label` instead of the
  misleading `target_branch` name, while still accepting legacy
  `target_branch` capsule input for compatibility.
- Text launch and verify output now labels verifier readiness as
  `preflight_ready` and scopes it to structural and semantic preflight while
  still requiring user review before handoff.
- Real-session `launch --execute` now blocks built-in draft compiler handoffs
  unless `--allow-draft` is explicitly passed; fixture sessions remain
  executable for safe tests and demos.
- README planning now tracks the accepted M68-M72 product design milestones:
  handoff trail signature, session portraits, pre-flight pill, command
  palette, and visual system polish.
- README now frames Capsule as Moonbox's product/schema name for the generic
  continuation-package category, keeping `capsule` CLI and JSON fields stable
  while improving external discoverability.
- Claude local-command XML-like records such as `<local-command-caveat>`,
  `<local-command-stdout>`, and `<command-name>` are now treated as internal
  tool events instead of user messages, rewind anchors, or resume-index titles.
- Claude Doctor capabilities now report captured stream-json / SDK metadata
  parsing as available while keeping remote / remote-control surfaces explicitly
  unavailable for launch/probing and separate from local resume rows.
- Compiler stdin, Capsule JSON export, verifier output, and target handoff
  prompts now use the shared redaction policy; target prompts include a
  dedicated Privacy / Redaction section while local execution routing remains
  exact for verifiable dry-run previews.
- Handoff Review readiness groups now prioritize Target Readiness, Workspace
  Restore, Source Health, Capsule Health, and Semantic Evidence, and launch
  validation summaries stay concise while full verifier checks remain available
  in JSON and TUI readiness details.
- Generated dry-run launch plans report `capsule_path: null` and do not emit
  fake `--capsule` paths.
- Codex, Claude, and Hermes source discovery use real local stores when any
  real store is present; fixture fallback is reserved for the no-real-store
  demo/CI case or explicit fixture mode.
- Codex and Claude default session lists continue to mirror each source CLI's
  own resume surface, while Hermes now defaults to all non-archived provider
  sources instead of only `source = cli` rows.
- Hermes Doctor capabilities now report all-source local inventory and captured
  source metadata as available, while export/search remains planned for M65.
- Explicit session lookup routes obvious Hermes/Codex/Claude ids to the likely
  adapter before expensive full-store fallback.
- CLI launch/verify uses lightweight session artifacts instead of constructing
  a full TUI workbench for explicit session ids.
- Target launch execution is opt-in and refuses to spawn a target command when
  verification fails.
- TUI target handoff now uses a dedicated `x` shortcut, with `H` and `t` kept
  as compatibility aliases, and a two-stage flow: choose a target, review the
  guarded execute command, then copy with `y`.
- TUI launch key hints now distinguish target selection from Handoff Review, so
  `y` is shown as unavailable until review.
- TUI launch copy now points at `moonbox launch --execute`, keeping long
  handoff prompts out of the modal while preserving guarded execution.
- TUI Handoff Review `enter` now restores the terminal and then launches the
  verified target CLI, while `y` still copies the guarded wrapper command.
- Original-session execution is opt-in and uses source-specific resume
  entrypoints; Hermes resume commands now use `hermes --resume <session>`.
- TUI original-session copy now points at `moonbox open --execute`.
- TUI original-session review `enter` now restores the terminal, prints the
  exact original resume command, and on Unix replaces the Moonbox process with
  the source CLI so the terminal is handed off without Moonbox waiting in the
  foreground; `y` still copies the guarded Moonbox wrapper.
- Main-list `enter` now directly opens the selected session with its original
  CLI; target handoff moved to the explicit `x` shortcut.
- TUI timeline hides provider-injected context rows such as
  `<environment_context>`, right-aligns event times, and scrolls by actual
  wrapped row height so the selected event stays visible.
- TUI timeline visually groups consecutive assistant messages into one
  source-specific `Codex xN` / `Claude Code xN` / `Hermes xN` block, and `j/k`
  navigation now moves by those visible groups instead of silently stepping
  through folded assistant events.
- TUI `S` now opens a Skill Picker instead of blindly cycling compiler skills;
  the picker shows status, kind, description, stars / `n/a`, and link/command
  metadata before `enter` applies the pending selection.
- Built-in draft compiler skills now show the Moonbox GitHub repository link in
  Skill Picker and display stars as `n/a`; external skills without metadata show
  `not configured` instead of ambiguous `unknown`.
- Action Path now shows the selected cwd plus Codex / Claude / Hermes session
  counts for that same path.
- TUI `+` / `=` now zooms the focused panel and `-` restores the default
  layout; zoom follows tab focus so Sessions, Timeline, Details, and Action
  Path can each be expanded without resetting selection or scroll state.
- Selected Timeline rows now preserve role accent colors, so active user turns
  stay blue and active AI groups stay gold instead of collapsing into one
  selected-state color.
- Compiler execution precedence is now explicit: environment override,
  configured default, first ready external preset, then built-in draft fallback.
- Built-in draft compilers now show warning catalog/readiness signals instead
  of presenting themselves as production-ready compiler skills.
- Unknown compiler ids and disabled compiler presets now return structured
  configuration errors instead of silently compiling through the fixture path.
- Saving the last selected target now preserves compiler presets and
  `default_compiler` in the user config file.
- TUI verify status no longer hard-codes the verifier check count.
- TUI session movement, source filtering, `/` search, `gg`, and `G` now update
  the selected row immediately, keep the left list compact with source-colored
  `Cdx` / `Clu` / `Hms` badges while preserving original source titles, hide
  healthy source markers in the left rail, show selected session metadata with
  Raw Title and Source Health in the right Session Details panel, and hydrate
  the timeline/capsule preview in the background from the current session index
  snapshot.
- TUI session-list secondary rows now use relative resume-picker timestamps
  such as `16s ago` / `3m ago`; exact timestamps remain available in the right
  Session Details panel.
- TUI `c` now refreshes the Work Capsule and opens Handoff Review directly; the
  old TUI-only `d Diff` surface was removed to keep the handoff flow linear.
- TUI sessions can now be starred with `s` (`*` remains an alias), persisted in
  user config, and filtered through the `Star` source filter before `All`.
- Session titles captured from Codex, Claude, and Hermes real stores now keep a
  longer preview for the right Session Details panel while the list remains
  windowed and clipped by the TUI.
- TUI header no longer shows a fake `/ 100K` token budget, keeps compiler status
  width stable across `ACTIVE` / `LOADING` / `COMPILED`, and mutes unselected
  session titles so only the active row reads as highlighted.
- The right Session Details panel now shows a compact Handoff Snapshot; full
  capsule decisions, todo, evidence, and risks move to Handoff Review.
- TUI session rows now render the selection arrow inline and only show the
  star/status marker when needed, removing the large blank gap before `Cdx` /
  `Clu` / `Hms` source badges.
- Resume-index rows whose event count is unknown now still hydrate their real
  timeline from `source_path`; truly empty timelines build a pending capsule
  instead of crashing the TUI by compiling a missing rewind id.
- CLI runtime now lives behind a shared library entrypoint used by both
  `moonbox` and `moon`.
- Replay eval JSON now separates matrix and synthetic case counts, labels each
  case with `case_kind` and `scenario`, and reports expected scenario coverage.
- README screenshot now shows the current Handoff Review target-input flow on a
  transparent SVG canvas, generated from the real TUI render path.
- Draft Homebrew formula now points at a staged GitHub release source archive
  instead of a GitHub auto-generated tag archive.
- TUI `{` / `}` now cycles the main session inventory between Local and
  configured SSH/devbox data spaces. Remote spaces run
  `ssh <host> moonbox sessions --json`, load the returned sessions as a
  read-only inventory, and keep failures visible in the status line while
  preserving the previous local list.
- Codex session titles now prefer `session_index.jsonl` `thread_name` values
  over stale `state_5.sqlite` titles, so renamed Codex threads are searchable
  and displayed the same way as the Codex resume picker.
- Codex Doctor capability reports now distinguish unconfigured app-server
  fallback, configured app-server success, and app-server fallback errors; deep
  links are reported as non-executing preview support instead of a planned M62
  item.
- Timeline cursor and selected rewind anchor markers now use separate visual
  treatment, so the active row and the saved rewind point no longer both look
  like the current selection.
- Handoff Review readiness output is grouped into Source Health, Capsule
  Health, and Target Readiness sections, with the full target prompt available
  from the review panel before launch.
- Auto session discovery now uses fixture fallback only when no real source
  stores are present, so real local indexes are not mixed with demo sessions.
- `open --execute` and `launch --execute` now require an explicit `--session`;
  dry-runs can still omit `--session` to preview the newest discovered session.
- Doctor diagnostics now use a single source inventory scan for session
  discovery and adapter health reporting, avoiding duplicate real-store scans.
- TUI session rows now include REAL/FIXTURE provenance badges, while the Doctor
  overlay exposes full adapter path and skip-count details.
- Replay-eval fixture invariants now return structured `CoreError` failures
  instead of panicking, and generated SVG docs snapshot code no longer relies on
  infallible string-write `expect` calls.
- Internal adapters, compiler runners, source stores, and TUI state no longer
  leak through the public Rust crate API.
- TUI session-list panel titles use stable-width slots so source/filter
  switching no longer shifts the top border line.
- TUI timeline rendering folds low-signal tool/function-call rows by default,
  and timeline navigation/rewind selection skip hidden tool events.
- Canonical timeline parsing now folds adjacent duplicate events from Codex,
  Claude, and Hermes before rendering, so provider double-writes do not show as
  repeated timeline rows.
- Real-session default rewind selection now prefers user turns or explicit
  rewind markers instead of assistant/tool output or arbitrary fixed fixture
  ids such as `evt-091`.
- TUI `space` now rejects assistant/tool rows as rewind anchors while still
  allowing those rows to remain visible for reading context.
- TUI `Esc` no longer exits from the main workbench or loading screen; exit is
  reserved for `q`, `Ctrl-C`, or explicit handoff actions. `Esc` still closes
  overlays and cancels command/search modes.
- Built-in compiler output is labeled as deterministic draft guidance; real
  fields are limited to session id, title, cwd, selected rewind, and source
  health until an external compiler skill is configured.
- Target handoff prompts now render a readable Work Capsule Summary with
  source, target, goal, state, decisions, todo, evidence, risks, and
  instructions instead of dumping the capsule as raw JSON into the target CLI
  first screen.
- TUI target handoff also accepts `x` as the primary cross-CLI handoff
  shortcut, while `H` and `t` remain compatibility aliases.
- `capsule`, `compile-request`, and `compile-output` now accept explicit
  `--session`, `--target`, `--rewind`, and `--compiler` options, replacing the
  old hard-coded Codex-to-Hermes inspection defaults with the same selected
  session/rewind model used by launch and the TUI.
- CLI smoke coverage now calls those inspection surfaces with explicit fixture
  session, target, and rewind arguments, so the gate cannot depend on a user's
  persisted last-target setting.

### Not Yet Released

- Homebrew formula, release archives, and package registry publishing are
  planned but not published.
