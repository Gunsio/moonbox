#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

smoke_home="${MOONBOX_DOCS_ASSETS_SMOKE_HOME:-$repo_root/target/moonbox-docs-assets-smoke-home}"
output_dir="$smoke_home/output"
mkdir -p "$output_dir"

export MOONBOX_CODEX_HOME="$smoke_home/codex"
export MOONBOX_CLAUDE_HOME="$smoke_home/claude"
export MOONBOX_HERMES_HOME="$smoke_home/hermes"
export MOONBOX_CONFIG="$smoke_home/config.json"
export MOONBOX_SESSION_MODE=fixture
export MOONBOX_SESSION_LIMIT=50
export MOONBOX_TUI_NOW_UNIX=1780650000

check_asset() {
  local variant="$1"
  local asset="$2"
  local generated="$output_dir/$(basename "$asset")"

  cargo run --locked -- docs-snapshot --variant "$variant" --output "$generated"

  if ! cmp -s "$generated" "$asset"; then
    diff -u "$generated" "$asset"
    echo "README screenshot asset is stale; regenerate with: MOONBOX_TUI_NOW_UNIX=1780650000 cargo run --locked -- docs-snapshot --variant $variant --output $asset" >&2
    exit 1
  fi

  xmllint --noout "$asset"
}

check_asset main docs/assets/moonbox-main.svg
check_asset timeline docs/assets/moonbox-timeline-zoom.svg
check_asset handoff-review docs/assets/moonbox-tui.svg

grep -Fq '![Moonbox main workbench screenshot](docs/assets/moonbox-main.svg)' README.md
grep -Fq '![Moonbox timeline zoom screenshot](docs/assets/moonbox-timeline-zoom.svg)' README.md
grep -Fq '![Moonbox Handoff Review screenshot](docs/assets/moonbox-tui.svg)' README.md
grep -Fq 'cargo install --git https://github.com/Gunsio/moonbox' README.md
grep -Fq 'cargo install --path . --locked' README.md
grep -Fq 'MOONBOX_SESSION_MODE=fixture moon sessions --json --filter codex' README.md
grep -Fq 'MOONBOX_SESSION_MODE=fixture moon doctor --json' README.md
grep -Fq 'moon completions zsh > /tmp/_moon' README.md
grep -Fq 'brew tap Gunsio/tap' README.md
grep -Fq 'Homebrew distribution is planned, but not published yet.' README.md

grep -Fq 'Sessions' docs/assets/moonbox-main.svg
grep -Fq 'Timeline' docs/assets/moonbox-main.svg
grep -Fq 'Real Session Metadata' docs/assets/moonbox-main.svg
grep -Fq 'Action Path' docs/assets/moonbox-main.svg

grep -Fq 'Moonbox timeline zoom screenshot' docs/assets/moonbox-timeline-zoom.svg
grep -Fq 'Timeline' docs/assets/moonbox-timeline-zoom.svg
grep -Fq 'Zoomed Timeline' docs/assets/moonbox-timeline-zoom.svg
grep -Fq 'REWIND' docs/assets/moonbox-timeline-zoom.svg

grep -Fq 'Handoff Review' docs/assets/moonbox-tui.svg
grep -Fq 'Capsule Review' docs/assets/moonbox-tui.svg
grep -Fq 'Target receives' docs/assets/moonbox-tui.svg
grep -Fq 'Draft Work Capsule' docs/assets/moonbox-tui.svg
grep -Fq 'Readiness' docs/assets/moonbox-tui.svg
grep -Fq 'moonbox launch --execute' docs/assets/moonbox-tui.svg
grep -Fq 'Action Path' docs/assets/moonbox-tui.svg

echo "moonbox docs asset smoke passed"
