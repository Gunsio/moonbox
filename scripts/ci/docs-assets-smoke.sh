#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

svg="docs/assets/moonbox-tui.svg"
smoke_home="${MOONBOX_DOCS_ASSETS_SMOKE_HOME:-$repo_root/target/moonbox-docs-assets-smoke-home}"
output_dir="$smoke_home/output"
generated="$output_dir/moonbox-tui.svg"
mkdir -p "$output_dir"

export MOONBOX_CODEX_HOME="$smoke_home/codex"
export MOONBOX_CLAUDE_HOME="$smoke_home/claude"
export MOONBOX_HERMES_HOME="$smoke_home/hermes"
export MOONBOX_CONFIG="$smoke_home/config.json"
export MOONBOX_SESSION_MODE=fixture
export MOONBOX_SESSION_LIMIT=50
export MOONBOX_TUI_NOW_UNIX=1780650000

cargo run --locked -- docs-snapshot --output "$generated"

if ! cmp -s "$generated" "$svg"; then
  diff -u "$generated" "$svg"
  echo "README screenshot asset is stale; regenerate with: MOONBOX_TUI_NOW_UNIX=1780650000 cargo run --locked -- docs-snapshot --output docs/assets/moonbox-tui.svg" >&2
  exit 1
fi

xmllint --noout "$svg"

grep -Fq '![Moonbox TUI screenshot](docs/assets/moonbox-tui.svg)' README.md
grep -Fq 'cargo install --git https://github.com/Gunsio/moonbox' README.md
grep -Fq 'cargo install --path . --locked' README.md
grep -Fq 'MOONBOX_SESSION_MODE=fixture moon sessions --json --filter codex' README.md
grep -Fq 'MOONBOX_SESSION_MODE=fixture moon doctor --json' README.md
grep -Fq 'moon completions zsh > /tmp/_moon' README.md
grep -Fq 'brew tap Gunsio/tap' README.md
grep -Fq 'brew trust --formula gunsio/tap/moonbox' README.md
grep -Fq 'Moonbox prereleases are distributed through the dedicated Homebrew tap:' README.md
grep -Fq 'tap pours the published bottle by default' README.md
grep -Fq 'Rust, LLVM, and' README.md
grep -Fq 'Apple Command Line Tools are not required' README.md

grep -Fq 'Handoff Review' "$svg"
grep -Fq 'Capsule' "$svg"
grep -Fq '审' "$svg"
grep -Fq '阅' "$svg"
grep -Fq '目' "$svg"
grep -Fq '标' "$svg"
grep -Fq '草' "$svg"
grep -Fq '稿' "$svg"
grep -Fq '就' "$svg"
grep -Fq '绪' "$svg"
grep -Fq 'Pre-flight:' "$svg"
grep -Fq 'Real Session Metadata' "$svg"
grep -Fq 'Cdx  Moonbox session rewind' "$svg"
grep -Fq '命' "$svg"
grep -Fq '令' "$svg"
grep -Fq 'handoff-prompt' "$svg"
grep -Fq 'Action Path' "$svg"
grep -Fq 'source Codex' "$svg"
grep -Fq 'rewind evt' "$svg"
grep -Fq 'target Hermes' "$svg"
grep -Fq 'handoff trail' "$svg"
grep -Fq 'Portrait' "$svg"
grep -Fq 'cached timeline' "$svg"

echo "moonbox docs asset smoke passed"
