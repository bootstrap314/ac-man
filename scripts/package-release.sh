#!/usr/bin/env bash
# Package only explicitly selected build products. Never archive the project tree.
set -euo pipefail
umask 077

project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$project_dir"

kind=${1:-}
tag=${2:-}
target=${3:-}

if [[ ! "$tag" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$ ]]; then
  echo 'Expected a safe existing release tag as the second argument.' >&2
  exit 2
fi
case "$target" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin|x86_64-apple-darwin) ;;
  *) echo 'Unsupported release target.' >&2; exit 2 ;;
esac

mkdir -p release-assets
staging=$(mktemp -d)
trap 'rm -rf -- "$staging"' EXIT

copy_notices() {
  local destination=$1
  # A missing license report must fail packaging rather than ship an incomplete archive.
  test -s THIRD_PARTY_NOTICES.md
  test -s licenses/DEPENDENCIES.html
  for document in README.md SECURITY.md THIRD_PARTY_NOTICES.md LICENSE; do
    if [[ -f "$document" ]]; then
      cp "$document" "$destination/"
    fi
  done
  mkdir -p "$destination/licenses"
  cp licenses/DEPENDENCIES.html "$destination/licenses/"
  # Keep the README's relative screenshot and signing-guide links functional.
  for document in docs/macos-release.md docs/images/ac-man-controls.jpg; do
    test -s "$document"
    mkdir -p "$destination/$(dirname -- "$document")"
    cp "$document" "$destination/$document"
  done
}

case "$kind" in
  cli)
    binary="target/$target/release/ac-man"
    test -f "$binary"
    test -x "$binary"
    test ! -L "$binary"
    name="ac-man-$tag-$target"
    mkdir "$staging/$name"
    cp "$binary" "$staging/$name/ac-man"
    chmod 755 "$staging/$name/ac-man"
    copy_notices "$staging/$name"
    COPYFILE_DISABLE=1 tar -czf "release-assets/$name.tar.gz" -C "$staging" "$name"
    ;;
  app)
    [[ "$target" == aarch64-apple-darwin && $(uname -s) == Darwin ]]
    test -d 'dist/ac-man.app'
    test ! -L 'dist/ac-man.app'
    # Build.sh creates this clean bundle from explicit inputs, without account data.
    cp -R 'dist/ac-man.app' "$staging/"
    copy_notices "$staging"
    ditto -c -k "$staging" "release-assets/ac-man-macos-$tag-$target.zip"
    ;;
  *) echo 'Usage: scripts/package-release.sh cli|app TAG TARGET' >&2; exit 2 ;;
esac
