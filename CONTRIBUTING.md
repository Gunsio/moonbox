# Contributing

Moonbox is early, but it should already be treated like production open-source
software. Contributions must keep the CLI, TUI, data contracts, documentation,
and release surface consistent.

## Development Setup

Moonbox requires Rust 1.88 or newer.

```bash
git clone https://github.com/Gunsio/moonbox.git
cd moonbox
cargo run --locked -- tui
```

## Required Checks

Run these before opening a pull request:

```bash
scripts/ci/full-gate.sh
```

The full gate runs patch hygiene plus the CI/release checks. It expects a clean
worktree for `cargo package --locked`; during pre-commit iteration you can use
`MOONBOX_PACKAGE_ALLOW_DIRTY=1 scripts/ci/full-gate.sh`, then run it again
without the override after committing.

Expanded gates:

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

`cargo test --locked` includes public CLI contract tests for the actual
`moonbox` and `moon` binaries. Those tests redirect source homes into
`target/cli-contract-home` and must stay fixture-safe.

`cargo doc --locked --no-deps` must pass with `RUSTDOCFLAGS="-D warnings"` so
public Rust documentation stays buildable as the library surface evolves.

`scripts/ci/supply-chain.sh` requires `cargo-deny`. Install it with
`cargo install --locked cargo-deny`, or set `CARGO_DENY=/path/to/cargo-deny`
when using a downloaded binary. It checks advisories, duplicate-version policy,
licenses, and crate sources against `deny.toml`.

`scripts/ci/homebrew-docs-smoke.sh` validates the draft Homebrew formula syntax
and the exact completion-generation commands the formula will use. It redirects
source homes into `target/moonbox-homebrew-smoke-home`, sets
`MOONBOX_SESSION_MODE=fixture`, and must not scan, open, or resume real
sessions.

For README screenshot and install-documentation changes:

```bash
cargo run --locked -- docs-snapshot --output docs/assets/moonbox-tui.svg
scripts/ci/docs-assets-smoke.sh
```

`docs-snapshot` is a hidden maintenance command that renders the real TUI
Launch Review state from embedded fixtures; it must not scan, open, resume, or
launch real sessions. The smoke regenerates the same SVG under `target/`,
compares it with the committed asset, and then validates the README image
reference, install commands, planned Homebrew wording, and key screenshot
semantics.

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
- Keep tests and smoke scripts from opening or resuming recent active sessions.
  Use `MOONBOX_SESSION_MODE=fixture`, explicit fixture homes, or embedded
  fixtures for automated checks.
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
