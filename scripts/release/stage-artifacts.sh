#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

usage() {
  cat <<'EOF'
Usage: scripts/release/stage-artifacts.sh --version <semver> [options]

Stages release artifacts under target/release-artifacts/v<version> by default.
This script builds local artifacts only. It does not tag, upload, publish, or
open any user sessions.

Options:
  --version <semver>   Release version. Must match Cargo.toml.
  --tag <tag>          Release tag to record in the manifest. Defaults to v<version>.
  --ref <git-ref>      Git ref used for the source archive. Defaults to HEAD.
  --out-dir <path>     Base output directory. Defaults to target/release-artifacts.
  --allow-dirty        Allow dirty worktrees for pre-commit dry runs.
  -h, --help           Show this help.
EOF
}

version=""
tag=""
source_ref="HEAD"
out_base="$repo_root/target/release-artifacts"
allow_dirty=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      version="${2:-}"
      shift 2
      ;;
    --tag)
      tag="${2:-}"
      shift 2
      ;;
    --ref)
      source_ref="${2:-}"
      shift 2
      ;;
    --out-dir)
      out_base="${2:-}"
      shift 2
      ;;
    --allow-dirty)
      allow_dirty=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$version" ]]; then
  echo "--version is required" >&2
  usage >&2
  exit 2
fi

if [[ -z "$tag" ]]; then
  tag="v$version"
fi

if [[ "$out_base" != /* ]]; then
  out_base="$repo_root/$out_base"
fi

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$repo_root/$target_dir"
fi

cargo_version="$(
  sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1
)"

if [[ "$version" != "$cargo_version" ]]; then
  echo "release version $version does not match Cargo.toml version $cargo_version" >&2
  exit 1
fi

dirty=0
if [[ -n "$(git status --porcelain)" ]]; then
  dirty=1
fi

if [[ "$dirty" == "1" && "$allow_dirty" != "1" ]]; then
  echo "worktree is dirty; commit first or pass --allow-dirty for a dry run" >&2
  exit 1
fi

commit_sha="$(git rev-parse "${source_ref}^{commit}")"
short_sha="$(git rev-parse --short=12 "$commit_sha")"
tag_exists=false
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  tag_exists=true
fi

host_triple="$(rustc -vV | sed -n 's/^host: //p')"
release_dir="$out_base/v$version"
staging_dir="$release_dir/moonbox-$version-$host_triple"
checksum_file="$release_dir/SHA256SUMS"
manifest_file="$release_dir/release-manifest.json"

rm -rf "$release_dir"
mkdir -p "$release_dir"

package_args=(package --locked --offline)
if [[ "$allow_dirty" == "1" ]]; then
  package_args+=(--allow-dirty)
fi

cargo "${package_args[@]}"
cargo build --release --locked --offline

crate_name="moonbox-$version.crate"
crate_source="$target_dir/package/$crate_name"
if [[ ! -f "$crate_source" ]]; then
  echo "expected cargo package artifact missing: $crate_source" >&2
  exit 1
fi
cp "$crate_source" "$release_dir/$crate_name"

source_name="moonbox-$version-source.tar.gz"
git archive \
  --format=tar.gz \
  --prefix="moonbox-$version/" \
  --output="$release_dir/$source_name" \
  "$commit_sha"

binary_name="moonbox-$version-$host_triple.tar.gz"
mkdir -p "$staging_dir/bin" "$staging_dir/completions/bash" \
  "$staging_dir/completions/zsh" "$staging_dir/completions/fish" \
  "$staging_dir/completions/powershell"

install -m 0755 "$target_dir/release/moonbox" "$staging_dir/bin/moonbox"
install -m 0755 "$target_dir/release/moon" "$staging_dir/bin/moon"
install -m 0644 README.md LICENSE CHANGELOG.md "$staging_dir/"

completion_home="$release_dir/completion-home"
mkdir -p "$completion_home/codex" "$completion_home/claude" "$completion_home/hermes"
export MOONBOX_CODEX_HOME="$completion_home/codex"
export MOONBOX_CLAUDE_HOME="$completion_home/claude"
export MOONBOX_HERMES_HOME="$completion_home/hermes"
export MOONBOX_CONFIG="$completion_home/config.json"
export MOONBOX_SESSION_MODE=fixture

"$target_dir/release/moonbox" completions bash > "$staging_dir/completions/bash/moonbox.bash"
"$target_dir/release/moon" completions bash > "$staging_dir/completions/bash/moon.bash"
"$target_dir/release/moonbox" completions zsh > "$staging_dir/completions/zsh/_moonbox"
"$target_dir/release/moon" completions zsh > "$staging_dir/completions/zsh/_moon"
"$target_dir/release/moonbox" completions fish > "$staging_dir/completions/fish/moonbox.fish"
"$target_dir/release/moon" completions fish > "$staging_dir/completions/fish/moon.fish"
"$target_dir/release/moonbox" completions powershell > "$staging_dir/completions/powershell/_moonbox.ps1"
"$target_dir/release/moon" completions powershell > "$staging_dir/completions/powershell/_moon.ps1"

(
  cd "$release_dir"
  tar -czf "$binary_name" "moonbox-$version-$host_triple"
)
rm -rf "$staging_dir" "$completion_home"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

byte_size() {
  wc -c < "$1" | tr -d '[:space:]'
}

artifact_records=()
for artifact in "$crate_name" "$source_name" "$binary_name"; do
  path="$release_dir/$artifact"
  sha="$(sha256_file "$path")"
  bytes="$(byte_size "$path")"
  kind="binary"
  case "$artifact" in
    *.crate)
      kind="cargo_crate"
      ;;
    *-source.tar.gz)
      kind="source_archive"
      ;;
  esac
  artifact_records+=("$artifact|$kind|$sha|$bytes")
done

: > "$checksum_file"
for record in "${artifact_records[@]}"; do
  IFS='|' read -r name _kind sha _bytes <<< "$record"
  printf '%s  %s\n' "$sha" "$name" >> "$checksum_file"
done

homebrew_source_sha256=""
for record in "${artifact_records[@]}"; do
  IFS='|' read -r name kind sha _bytes <<< "$record"
  if [[ "$kind" == "source_archive" ]]; then
    homebrew_source_sha256="$sha"
  fi
done

created_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

{
  cat <<EOF
{
  "schema_version": 1,
  "project": "moonbox",
  "version": "$version",
  "tag": "$tag",
  "tag_exists": $tag_exists,
  "source_ref": "$source_ref",
  "commit": "$commit_sha",
  "short_commit": "$short_sha",
  "dirty_worktree": $([[ "$dirty" == "1" ]] && echo true || echo false),
  "host_triple": "$host_triple",
  "created_at": "$created_at",
  "homebrew": {
    "url": "https://github.com/Gunsio/moonbox/releases/download/$tag/$source_name",
    "sha256": "$homebrew_source_sha256"
  },
  "artifacts": [
EOF

  comma=""
  for record in "${artifact_records[@]}"; do
    IFS='|' read -r name kind sha bytes <<< "$record"
    cat <<EOF
    $comma{
      "name": "$name",
      "kind": "$kind",
      "path": "$name",
      "sha256": "$sha",
      "bytes": $bytes
    }
EOF
    comma=","
  done

  cat <<'EOF'
  ]
}
EOF
} > "$manifest_file"

printf 'release artifacts staged at %s\n' "$release_dir"
printf 'checksums: %s\n' "$checksum_file"
printf 'manifest: %s\n' "$manifest_file"
