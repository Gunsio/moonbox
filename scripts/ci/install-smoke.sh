#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

install_root="${MOONBOX_INSTALL_SMOKE_ROOT:-$repo_root/target/moonbox-install-smoke}"
source_home="$install_root/source-home"
output_dir="$install_root/output"

# Keep install smoke isolated from the developer or runner's real session stores.
mkdir -p "$source_home/codex" "$source_home/claude" "$source_home/hermes" "$output_dir"

export MOONBOX_CODEX_HOME="$source_home/codex"
export MOONBOX_CLAUDE_HOME="$source_home/claude"
export MOONBOX_HERMES_HOME="$source_home/hermes"
export MOONBOX_CONFIG="$install_root/config.json"
export MOONBOX_SESSION_MODE=fixture

cargo install --path . --root "$install_root" --locked --offline --force

"$install_root/bin/moonbox" --version | grep -q "moonbox"
"$install_root/bin/moon" --version | grep -q "moonbox"
"$install_root/bin/moon" sessions --json --filter codex > "$output_dir/sessions.json"
grep -q "codex-cxcp-design" "$output_dir/sessions.json"
"$install_root/bin/moon" doctor --json > "$output_dir/doctor.json"
grep -q '"ready": true' "$output_dir/doctor.json"
grep -q '"source_adapters"' "$output_dir/doctor.json"
"$install_root/bin/moon" completions zsh > "$output_dir/_moon"
grep -q "#compdef moon" "$output_dir/_moon"
"$install_root/bin/moon" replay-eval --json | grep -q '"fixture_only": true'

echo "moonbox install smoke passed at $install_root"
