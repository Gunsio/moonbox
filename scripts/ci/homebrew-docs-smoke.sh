#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

formula="docs/release/homebrew/moonbox.rb"
smoke_home="${MOONBOX_HOMEBREW_SMOKE_HOME:-$repo_root/target/moonbox-homebrew-smoke-home}"
output_dir="$smoke_home/output"
mkdir -p "$output_dir"

export MOONBOX_CODEX_HOME="$smoke_home/codex"
export MOONBOX_CLAUDE_HOME="$smoke_home/claude"
export MOONBOX_HERMES_HOME="$smoke_home/hermes"
export MOONBOX_CONFIG="$smoke_home/config.json"
export MOONBOX_SESSION_MODE=fixture
export MOONBOX_SESSION_LIMIT=50

run() {
  printf '\n==> %s\n' "$*"
  "$@"
}

run ruby -c "$formula"

grep -Fq 'url "https://github.com/Gunsio/moonbox/releases/download/v0.1.0/moonbox-0.1.0-source.tar.gz"' "$formula"
grep -Fq 'sha256 "<release-source-sha256>"' "$formula"
grep -Fq 'release-manifest.json homebrew.sha256' "$formula"
grep -Fq 'system "cargo", "install", *std_cargo_args' "$formula"
grep -Fq 'generate_completions_from_executable(bin/"moonbox", "completions", shells: [:bash, :zsh, :fish, :pwsh])' "$formula"
grep -Fq 'generate_completions_from_executable(bin/"moon", "completions", shells: [:bash, :zsh, :fish, :pwsh])' "$formula"

run cargo build --locked

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$repo_root/$target_dir"
fi

moonbox="$target_dir/debug/moonbox"
moon="$target_dir/debug/moon"

"$moonbox" completions bash > "$output_dir/moonbox.bash"
grep -q "_moonbox" "$output_dir/moonbox.bash"
grep -q "replay-eval" "$output_dir/moonbox.bash"

"$moonbox" completions zsh > "$output_dir/_moonbox"
grep -q "#compdef moonbox" "$output_dir/_moonbox"

"$moonbox" completions fish > "$output_dir/moonbox.fish"
grep -q "complete -c moonbox" "$output_dir/moonbox.fish"

"$moonbox" completions powershell > "$output_dir/_moonbox.ps1"
grep -q "Register-ArgumentCompleter" "$output_dir/_moonbox.ps1"
grep -q "CommandName 'moonbox'" "$output_dir/_moonbox.ps1"

"$moon" completions bash > "$output_dir/moon.bash"
grep -q "_moon" "$output_dir/moon.bash"

"$moon" completions fish > "$output_dir/moon.fish"
grep -q "complete -c moon" "$output_dir/moon.fish"

"$moon" completions powershell > "$output_dir/_moon.ps1"
grep -q "CommandName 'moon'" "$output_dir/_moon.ps1"

echo "moonbox Homebrew docs smoke passed with fixture-only source homes at $smoke_home"
