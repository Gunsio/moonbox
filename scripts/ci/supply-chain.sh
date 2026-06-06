#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

if [[ -n "${CARGO_DENY:-}" ]]; then
  cargo_deny=("$CARGO_DENY")
else
  cargo_deny=(cargo deny)
fi

if ! "${cargo_deny[@]}" --version >/dev/null 2>&1; then
  cat >&2 <<'EOF'
cargo-deny is required for the Moonbox supply-chain gate.

Install it with:
  cargo install --locked cargo-deny

Or point this script at a downloaded binary:
  CARGO_DENY=/path/to/cargo-deny scripts/ci/supply-chain.sh
EOF
  exit 127
fi

"${cargo_deny[@]}" check advisories bans licenses sources
