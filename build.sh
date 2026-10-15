#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$project_dir"
if [[ $(uname -s) != Darwin || $(uname -m) != arm64 ]]; then
  echo 'The menu-bar bundle currently builds on Apple Silicon macOS. Use make cli on other supported platforms.' >&2
  exit 1
fi
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/build/home --remap-path-prefix=$project_dir=/build/ac-man"
export MACOSX_DEPLOYMENT_TARGET=13.0
"${BOLTFFI:-boltffi}" pack apple --release --deny-skipped
cargo build --release --locked --bin ac-man
swift build -c release --product AcMan -Xswiftc -gnone \
  -Xswiftc -file-prefix-map -Xswiftc "$HOME=/build/home" \
  -Xswiftc -file-prefix-map -Xswiftc "$project_dir=/build/ac-man"
bundle_dir="$project_dir/dist/ac-man.app"
# Construct a fresh bundle from explicit inputs so old runtimes or data cannot leak.
staging=$(mktemp -d "$project_dir/dist-app.XXXXXX")
trap 'rm -rf -- "$staging"' EXIT
mkdir -p "$staging/Contents/MacOS" "$staging/Contents/Resources" "$project_dir/dist/bin"
cp .build/release/AcMan "$staging/Contents/MacOS/"
# Native archive debug symbols can retain local source paths after linking.
strip -S "$staging/Contents/MacOS/AcMan"
cp Info.plist "$staging/Contents/Info.plist"
cp assets/AppIcon.icns "$staging/Contents/Resources/"
cp LICENSE THIRD_PARTY_NOTICES.md "$staging/Contents/Resources/"
if [[ -f licenses/DEPENDENCIES.html ]]; then
  cp licenses/DEPENDENCIES.html "$staging/Contents/Resources/"
fi
codesign --force --sign - "$staging"
codesign --verify --strict "$staging"
mkdir -p "$project_dir/dist"
rm -rf -- "$bundle_dir"
mv "$staging" "$bundle_dir"
cp target/release/ac-man dist/bin/ac-man
printf 'Built %s\n' "$bundle_dir" "$project_dir/dist/bin/ac-man"
