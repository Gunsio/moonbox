# Changelog

All notable changes to Moonbox will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses semantic versioning once tagged releases start.

## [Unreleased]

### Added

- Rust + Ratatui TUI workbench with Vim-style navigation.
- Global session list with source tags, search, source filtering, original
  session preview, and target picker.
- Canonical Timeline, Work Capsule, compile request, compile output, launch
  plan, and verification report models.
- File-backed fixture adapters for Codex, Claude, and Hermes.
- Real read-only Codex adapter for `~/.codex/sessions`.
- Real read-only Claude adapter for `~/.claude/projects`.
- Real read-only Hermes adapter for `~/.hermes/state.db`, with optional
  metadata enrichment from `~/.hermes/sessions/sessions.json`.
- Fallible `SourceAdapter` discovery.
- Replaceable `CapsuleCompiler` trait with fixture and process-backed compilers.
- External compiler runner using JSON stdin/stdout, timeout handling, and
  structured process errors.
- Guarded target launcher execution through `launch --execute`.
- Target-specific Codex, Claude, and Hermes command generation with structured
  `target_command` plan output.
- Guarded original-session execution through `open --execute`.
- Structured original open plan output through `open --json`.
- `action` discriminators in dry-run `open --json` and `launch --json`
  output, using `original_resume` and `target_handoff`.
- First-class `moon` binary alias installed alongside `moonbox`.
- Configurable compiler skill presets in `~/.config/moonbox/config.json`.
- Compiler catalog output through `moonbox compilers`, including source,
  status, score, command, arguments, timeout, and quality reason.
- Hardened verification checks for Work Capsule version, required fields,
  handoff context, risk context, capsule size, target branch markers, and
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
  produces working `moonbox` and `moon` executables, with source homes
  redirected away from real local session stores.
- Fixture-safe integration tests for public CLI contracts across `moonbox` and
  `moon` binaries.
- Documentation build gate with rustdoc warnings treated as errors.
- Full local quality gate script for patch hygiene plus CI/release checks.
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
  and Launch Review.
- Documentation asset smoke coverage for README screenshot semantics,
  installation commands, and unpublished Homebrew wording.
- Hidden fixture-only `docs-snapshot` maintenance command that renders the real
  Ratatui Launch Review buffer to SVG for the README screenshot asset.
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
- Bounded JSONL real-store discovery through `MOONBOX_SESSION_SCAN_LIMIT`,
  with Doctor scan-cost fields for list limit, scan entry limit, visited entry
  count, and truncation state.
- Bounded JSONL session summary parsing through
  `MOONBOX_SESSION_SUMMARY_LINE_LIMIT`, so listed large sessions do not require
  full-file parsing before the TUI becomes usable.
- Bounded TUI timeline previews through `MOONBOX_TIMELINE_EVENT_LIMIT`, with a
  visible truncation marker for large sessions.
- Animated TUI loading screen while source sessions are indexed before the
  workbench becomes interactive.
- Shared verifier policy for CLI and TUI launch validation.
- Real `--capsule` file parsing and target mismatch verification.
- README screenshot, installation notes, and Homebrew release planning docs.

### Changed

- Generated dry-run launch plans report `capsule_path: null` and do not emit
  fake `--capsule` paths.
- Codex, Claude, and Hermes source discovery use real local stores when any
  real store is present; fixture fallback is reserved for the no-real-store
  demo/CI case or explicit fixture mode.
- Explicit session lookup routes obvious Hermes/Codex/Claude ids to the likely
  adapter before expensive full-store fallback.
- CLI launch/verify uses lightweight session artifacts instead of constructing
  a full TUI workbench for explicit session ids.
- Target launch execution is opt-in and refuses to spawn a target command when
  verification fails.
- TUI target handoff now uses a two-stage flow: choose a target, review the
  guarded execute command, then copy with `y`.
- TUI launch key hints now distinguish target selection from Launch Review, so
  `y` is shown as unavailable until review.
- TUI launch copy now points at `moonbox launch --execute`, keeping long
  handoff prompts out of the modal while preserving guarded execution.
- Original-session execution is opt-in and uses source-specific resume
  entrypoints; Hermes resume commands now use `hermes --resume <session>`.
- TUI original-session copy now points at `moonbox open --execute`.
- TUI original-session preview is copy-only; `enter` is disabled there to keep
  original resume distinct from target handoff.
- Compiler execution precedence is now explicit: environment override, config
  preset, then built-in fixture compiler.
- Unknown compiler ids and disabled compiler presets now return structured
  configuration errors instead of silently compiling through the fixture path.
- Saving the last selected target now preserves compiler presets and
  `default_compiler` in the user config file.
- TUI verify status no longer hard-codes the verifier check count.
- CLI runtime now lives behind a shared library entrypoint used by both
  `moonbox` and `moon`.
- Replay eval JSON now separates matrix and synthetic case counts, labels each
  case with `case_kind` and `scenario`, and reports expected scenario coverage.
- README screenshot now shows the current Launch Review readiness-details flow
  on a transparent SVG canvas, generated from the real TUI render path.
- Draft Homebrew formula now points at a staged GitHub release source archive
  instead of a GitHub auto-generated tag archive.
- Auto session discovery now uses fixture fallback only when no real source
  stores are present, so real local indexes are not mixed with demo sessions.
- `open --execute` and `launch --execute` now require an explicit `--session`;
  dry-runs can still omit `--session` to preview the newest discovered session.
- Doctor diagnostics now use a single source inventory scan for session
  discovery and adapter health reporting, avoiding duplicate real-store scans.
- TUI session rows now include REAL/FIXTURE provenance badges, while the Doctor
  overlay exposes full adapter path and skip-count details.

### Not Yet Released

- Homebrew formula, release archives, and package registry publishing are
  planned but not published.
