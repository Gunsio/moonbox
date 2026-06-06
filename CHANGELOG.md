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
- Fallible `SourceAdapter` discovery.
- Replaceable `CapsuleCompiler` trait with fixture and process-backed compilers.
- External compiler runner using JSON stdin/stdout, timeout handling, and
  structured process errors.
- Shared verifier policy for CLI and TUI launch validation.
- Real `--capsule` file parsing and target mismatch verification.
- README screenshot, installation notes, and Homebrew release planning docs.

### Changed

- Generated dry-run launch plans report `capsule_path: null` and do not emit
  fake `--capsule` paths.
- Codex and Claude source discovery prefer real local stores when present, with
  fixture fallback when stores are missing.

### Not Yet Released

- Homebrew formula and release archives are planned but not published.
- Real Hermes source adapter is not implemented yet.
- Real target launcher execution is not implemented yet.
