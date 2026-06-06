# Contributing

Moonbox is early, but it should already be treated like production open-source
software. Contributions must keep the CLI, TUI, data contracts, documentation,
and release surface consistent.

## Development Setup

Moonbox requires Rust 1.88 or newer.

```bash
git clone https://github.com/Gunsio/moonbox.git
cd moonbox
cargo run -- tui
```

## Required Checks

Run these before opening a pull request:

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy -- -D warnings
cargo build --release
```

For README screenshot changes:

```bash
xmllint --noout docs/assets/moonbox-tui.svg
```

## Engineering Standards

- Keep source sessions read-only.
- Prefer stable core contracts over UI-only behavior.
- Do not duplicate business policy between CLI and TUI. Shared rules belong in
  `src/core`.
- Do not add fake parameters or placeholder outputs. If an argument accepts a
  file, read and validate that file.
- Return structured errors for recoverable failures. Avoid panics outside tests
  and impossible invariants.
- Keep fixture data deterministic and representative enough to protect future
  real adapters.
- Update README and the Feishu plan whenever public behavior, install commands,
  release state, or architecture milestones change.

## Pull Request Shape

Each milestone PR should include:

- What changed.
- Why the change matters.
- User-visible behavior changes.
- Verification commands and important smoke-test output.
- Documentation updates.
- Known remaining gaps.

## Release Rules

Do not publish a Homebrew formula, release archive, or package registry version
until the milestone is accepted and a version tag is planned. The Homebrew path
is documented in [docs/release/homebrew.md](docs/release/homebrew.md), but it is
not live yet.
