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
grep -Fq 'brew tap Gunsio/tap' README.md
grep -Fq 'Homebrew distribution is planned, but not published yet.' README.md

grep -Fq 'Launch Review' "$svg"
grep -Fq 'Readiness details' "$svg"
grep -Fq 'Real Session Metadata' "$svg"
grep -Fq 'Cdx  Moonbox session rewind' "$svg"
grep -Fq 'moonbox launch --execute' "$svg"
grep -Fq 'enter launch after restore' "$svg"
grep -Fq 'copy execute command' "$svg"

echo "moonbox docs asset smoke passed"
