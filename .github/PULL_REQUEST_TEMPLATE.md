## Summary

Describe the change and why it matters.

## Behavior Changes

- 

## Verification

- [ ] `scripts/ci/full-gate.sh`
- [ ] `git diff --check`
- [ ] `cargo fmt --check`
- [ ] `cargo check --locked`
- [ ] `cargo test --locked`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps`
- [ ] `cargo run --locked -- replay-eval --json`
- [ ] `scripts/ci/cli-smoke.sh`
- [ ] `cargo clippy --locked -- -D warnings`
- [ ] `cargo build --release --locked`
- [ ] `cargo package --locked`
- [ ] `scripts/ci/install-smoke.sh`

## Documentation

- [ ] README updated, or not needed.
- [ ] Feishu plan updated, or not needed.
- [ ] Release/Homebrew docs updated, or not needed.

## Notes

Known gaps, follow-ups, or review focus.
