#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

version="$(
  sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1
)"
host_triple="$(rustc -vV | sed -n 's/^host: //p')"
smoke_root="${MOONBOX_RELEASE_SMOKE_DIR:-$repo_root/target/moonbox-release-artifacts-smoke}"
smoke_home="$smoke_root/source-home"

export MOONBOX_CODEX_HOME="$smoke_home/codex"
export MOONBOX_CLAUDE_HOME="$smoke_home/claude"
export MOONBOX_HERMES_HOME="$smoke_home/hermes"
export MOONBOX_CONFIG="$smoke_home/config.json"
export MOONBOX_SESSION_MODE=fixture

rm -rf "$smoke_root"
mkdir -p "$smoke_home/codex" "$smoke_home/claude" "$smoke_home/hermes"

run() {
  printf '\n==> %s\n' "$*"
  "$@"
}

stage_args=(--version "$version" --out-dir "$smoke_root")
if [[ "${MOONBOX_PACKAGE_ALLOW_DIRTY:-0}" == "1" ]]; then
  stage_args+=(--allow-dirty)
fi

run scripts/release/stage-artifacts.sh "${stage_args[@]}"

release_dir="$smoke_root/v$version"
crate="$release_dir/moonbox-$version.crate"
source="$release_dir/moonbox-$version-source.tar.gz"
binary="$release_dir/moonbox-$version-$host_triple.tar.gz"
checksums="$release_dir/SHA256SUMS"
manifest="$release_dir/release-manifest.json"

for path in "$crate" "$source" "$binary" "$checksums" "$manifest"; do
  [[ -s "$path" ]] || {
    echo "expected release artifact missing or empty: $path" >&2
    exit 1
  }
done

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

while read -r expected name; do
  [[ -n "$expected" && -n "$name" ]] || continue
  actual="$(sha256_file "$release_dir/$name")"
  if [[ "$expected" != "$actual" ]]; then
    echo "checksum mismatch for $name" >&2
    echo "expected: $expected" >&2
    echo "actual:   $actual" >&2
    exit 1
  fi
done < "$checksums"

crate_contents="$smoke_root/crate.contents"
source_contents="$smoke_root/source.contents"
binary_contents="$smoke_root/binary.contents"

tar -tzf "$crate" > "$crate_contents"
tar -tzf "$source" > "$source_contents"
tar -tzf "$binary" > "$binary_contents"

grep -q "^moonbox-$version/Cargo.toml$" "$crate_contents"
grep -q "^moonbox-$version/Cargo.toml$" "$source_contents"
grep -q "^moonbox-$version/README.md$" "$source_contents"
grep -q "^moonbox-$version/docs/release/homebrew.md$" "$source_contents"
if [[ "${MOONBOX_PACKAGE_ALLOW_DIRTY:-0}" != "1" || -z "$(git status --porcelain)" ]]; then
  grep -q "^moonbox-$version/scripts/release/stage-artifacts.sh$" "$source_contents"
fi
grep -q "^moonbox-$version-$host_triple/bin/moonbox$" "$binary_contents"
grep -q "^moonbox-$version-$host_triple/bin/moon$" "$binary_contents"
grep -q "^moonbox-$version-$host_triple/completions/bash/moonbox.bash$" "$binary_contents"
grep -q "^moonbox-$version-$host_triple/completions/zsh/_moon$" "$binary_contents"
grep -q "^moonbox-$version-$host_triple/completions/fish/moon.fish$" "$binary_contents"
grep -q "^moonbox-$version-$host_triple/completions/powershell/_moonbox.ps1$" "$binary_contents"

grep -Fq "moonbox-$version.crate" "$checksums"
grep -Fq "moonbox-$version-source.tar.gz" "$checksums"
grep -Fq "moonbox-$version-$host_triple.tar.gz" "$checksums"

ruby -rjson -e '
  manifest = JSON.parse(File.read(ARGV[0]))
  version = ARGV[1]
  host = ARGV[2]
  abort "bad version" unless manifest["version"] == version
  abort "bad tag" unless manifest["tag"] == "v#{version}"
  abort "bad host" unless manifest["host_triple"] == host
  abort "missing homebrew sha" if manifest.dig("homebrew", "sha256").to_s.empty?
  names = manifest["artifacts"].map { |artifact| artifact["name"] }
  [
    "moonbox-#{version}.crate",
    "moonbox-#{version}-source.tar.gz",
    "moonbox-#{version}-#{host}.tar.gz"
  ].each do |name|
    abort "missing artifact #{name}" unless names.include?(name)
  end
' "$manifest" "$version" "$host_triple"

echo "moonbox release artifact smoke passed at $release_dir"
