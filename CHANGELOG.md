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
- CI gates for fixture replay evaluation and `cargo package --locked`
  verification.
- Fixture-safe CLI smoke script for non-executing command surfaces, with source
  homes redirected away from real local session stores.
- Shared verifier policy for CLI and TUI launch validation.
- Real `--capsule` file parsing and target mismatch verification.
- README screenshot, installation notes, and Homebrew release planning docs.

### Changed

- Generated dry-run launch plans report `capsule_path: null` and do not emit
  fake `--capsule` paths.
- Codex, Claude, and Hermes source discovery prefer real local stores when
  present, with fixture fallback when stores are missing.
- Explicit session lookup routes obvious Hermes/Codex/Claude ids to the likely
  adapter before expensive full-store fallback.
- CLI launch/verify uses lightweight session artifacts instead of constructing
  a full TUI workbench for explicit session ids.
- Target launch execution is opt-in and refuses to spawn a target command when
  verification fails.
- TUI launch copy now points at `moonbox launch --execute`, keeping long
  handoff prompts out of the modal while preserving guarded execution.
- Original-session execution is opt-in and uses source-specific resume
  entrypoints; Hermes resume commands now use `hermes --resume <session>`.
- TUI original-session copy now points at `moonbox open --execute`.
- Compiler execution precedence is now explicit: environment override, config
  preset, then built-in fixture compiler.
- Unknown compiler ids and disabled compiler presets now return structured
  configuration errors instead of silently compiling through the fixture path.
- Saving the last selected target now preserves compiler presets and
  `default_compiler` in the user config file.
- TUI verify status no longer hard-codes the verifier check count.
- CLI runtime now lives behind a shared library entrypoint used by both
  `moonbox` and `moon`.

### Not Yet Released

- Homebrew formula and release archives are planned but not published.
