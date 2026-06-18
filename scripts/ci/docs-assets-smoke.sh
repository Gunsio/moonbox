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
unset NO_COLOR
export COLORTERM=truecolor
export TERM=xterm-256color

scenes=(
  "action-menu:moonbox-action-menu.svg"
  "yank:moonbox-yank.svg"
  "handoff:moonbox-handoff-review.svg"
  "timeline-details:moonbox-timeline-details.svg"
)

for entry in "${scenes[@]}"; do
  scene="${entry%%:*}"
  file="${entry#*:}"
  generated="$output_dir/$file"
  svg="docs/assets/$file"
  cargo run --locked -- docs-snapshot --scene "$scene" --output "$generated"
  if ! cmp -s "$generated" "$svg"; then
    diff -u "$generated" "$svg"
    echo "README screenshot asset is stale; regenerate with: MOONBOX_SESSION_MODE=fixture MOONBOX_TUI_NOW_UNIX=1780650000 COLORTERM=truecolor TERM=xterm-256color cargo run --locked -- docs-snapshot --scene $scene --output $svg" >&2
    exit 1
  fi
  xmllint --noout "$svg"
done

grep -Fq '![Moonbox action menu](docs/assets/moonbox-action-menu.svg)' README.md
grep -Fq '![Moonbox yank panel](docs/assets/moonbox-yank.svg)' README.md
grep -Fq '![Moonbox handoff review](docs/assets/moonbox-handoff-review.svg)' README.md
grep -Fq '![Moonbox timeline details](docs/assets/moonbox-timeline-details.svg)' README.md
grep -Fq 'cargo install --git https://github.com/Gunsio/moonbox' README.md
grep -Fq 'cargo install --path . --locked' README.md
grep -Fq 'MOONBOX_SESSION_MODE=fixture moon sessions --json --filter codex' README.md
grep -Fq 'MOONBOX_SESSION_MODE=fixture moon doctor --json' README.md
grep -Fq 'moon completions zsh > /tmp/_moon' README.md
grep -Fq 'brew tap Gunsio/tap' README.md
grep -Fq 'brew trust --formula gunsio/tap/moonbox' README.md
grep -Fq 'cross-CLI session workbench' README.md
grep -Fq 'like a Moonlight Box' README.md
grep -Fq 'Luoshen theme pack' README.md
grep -Fq 'Acknowledgements' README.md

grep -Fq 'Action Menu' docs/assets/moonbox-action-menu.svg
grep -Fq 'Session actions' docs/assets/moonbox-action-menu.svg
grep -Fq 'New Session' docs/assets/moonbox-action-menu.svg
grep -Fq 'Yank' docs/assets/moonbox-yank.svg
grep -Fq 'Portable JSON' docs/assets/moonbox-yank.svg
grep -Fq 'Handoff Review' docs/assets/moonbox-handoff-review.svg
grep -Fq 'Handoff Body' docs/assets/moonbox-handoff-review.svg
grep -Fq 'Timeline' docs/assets/moonbox-timeline-details.svg
grep -Fq 'Action Path' docs/assets/moonbox-timeline-details.svg

echo "moonbox docs asset smoke passed"
