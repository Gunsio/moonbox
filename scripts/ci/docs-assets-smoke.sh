#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

svg="docs/assets/moonbox-tui.svg"

xmllint --noout "$svg"

grep -Fq '![Moonbox TUI screenshot](docs/assets/moonbox-tui.svg)' README.md
grep -Fq 'cargo install --git https://github.com/Gunsio/moonbox' README.md
grep -Fq 'cargo install --path . --locked' README.md
grep -Fq 'brew tap Gunsio/tap' README.md
grep -Fq 'Homebrew distribution is planned, but not published yet.' README.md

grep -Fq 'Launch Review' "$svg"
grep -Fq 'Readiness details' "$svg"
grep -Fq 'moonbox launch --execute' "$svg"
grep -Fq 'enter Disabled' "$svg"
grep -Fq 'Copy guarded' "$svg"

echo "moonbox docs asset smoke passed"
