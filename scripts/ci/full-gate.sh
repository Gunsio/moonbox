#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

run() {
  printf '\n==> %s\n' "$*"
  "$@"
}

run git diff --check
run scripts/ci/supply-chain.sh
run cargo fmt --check
run cargo check --locked
run cargo test --locked

printf '\n==> RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps\n'
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps

run cargo run --locked -- replay-eval --json
run scripts/ci/cli-smoke.sh
run cargo clippy --locked -- -D warnings
run cargo build --release --locked

if [[ "${MOONBOX_PACKAGE_ALLOW_DIRTY:-0}" == "1" ]]; then
  run cargo package --locked --allow-dirty
else
  run cargo package --locked
fi

run scripts/ci/install-smoke.sh

printf '\nmoonbox full local gate passed\n'
