#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

smoke_home="${MOONBOX_SMOKE_HOME:-$repo_root/target/moonbox-smoke-home}"
output_dir="$smoke_home/output"
mkdir -p "$output_dir"

export MOONBOX_CODEX_HOME="$smoke_home/codex"
export MOONBOX_CLAUDE_HOME="$smoke_home/claude"
export MOONBOX_HERMES_HOME="$smoke_home/hermes"
export MOONBOX_SESSION_LIMIT=50

cargo build --locked

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$repo_root/$target_dir"
fi
moonbox="$target_dir/debug/moonbox"

"$moonbox" --help > "$output_dir/help.txt"
grep -q "replay-eval" "$output_dir/help.txt"

"$moonbox" sessions --json > "$output_dir/sessions.json"
grep -q "codex-cxcp-design" "$output_dir/sessions.json"
grep -q "claude-qc-platform" "$output_dir/sessions.json"
grep -q "hermes-cxcp-502" "$output_dir/sessions.json"

"$moonbox" capsule --json > "$output_dir/capsule.json"
grep -q '"source_cli": "codex"' "$output_dir/capsule.json"

"$moonbox" compile-request --json > "$output_dir/compile-request.json"
grep -q '"compiler": "engineering-handoff"' "$output_dir/compile-request.json"

"$moonbox" compile-output --json > "$output_dir/compile-output.json"
grep -q '"target_branch": "moonbox/hermes-rewind-evt-091"' "$output_dir/compile-output.json"

"$moonbox" compilers --json > "$output_dir/compilers.json"
grep -q '"id": "engineering-handoff"' "$output_dir/compilers.json"

"$moonbox" open --session codex-cxcp-design --json > "$output_dir/open.json"
grep -q '"dry_run": true' "$output_dir/open.json"
grep -q '"program": "codex"' "$output_dir/open.json"

"$moonbox" launch --target hermes --session codex-cxcp-design --json > "$output_dir/launch.json"
grep -q '"dry_run": true' "$output_dir/launch.json"
grep -q '"ready": true' "$output_dir/launch.json"

"$moonbox" verify --target hermes --session codex-cxcp-design --json > "$output_dir/verify.json"
grep -q '"status": "pass"' "$output_dir/verify.json"

"$moonbox" replay-eval --json > "$output_dir/replay-eval.json"
grep -q '"fixture_only": true' "$output_dir/replay-eval.json"
grep -q '"case_count": 9' "$output_dir/replay-eval.json"

echo "moonbox CLI smoke passed with fixture-only source homes at $smoke_home"
