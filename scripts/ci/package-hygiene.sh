#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

package_args=(package --list --locked)
if [[ "${MOONBOX_PACKAGE_ALLOW_DIRTY:-0}" == "1" ]]; then
  package_args+=(--allow-dirty)
fi

package_files="$(cargo "${package_args[@]}")"

forbidden_pattern='(^|/)(\.DS_Store|Thumbs\.db)$|(^|/)(target|node_modules)/|(~$|\.rej$|\.swp$|\.swo$|\.tmp$)'

if forbidden_files="$(printf '%s\n' "$package_files" | grep -E "$forbidden_pattern" || true)"; then
  if [[ -n "$forbidden_files" ]]; then
    printf 'forbidden files would be included in the Cargo package:\n%s\n' "$forbidden_files" >&2
    exit 1
  fi
fi

printf '%s\n' "$package_files" | grep -Fxq "Cargo.toml"
printf '%s\n' "$package_files" | grep -Fxq "src/lib.rs"
printf '%s\n' "$package_files" | grep -Fxq "src/main.rs"
printf '%s\n' "$package_files" | grep -Fxq "src/bin/moon.rs"
printf '%s\n' "$package_files" | grep -Fxq "README.md"
printf '%s\n' "$package_files" | grep -Fxq "LICENSE"

echo "moonbox package hygiene passed"
