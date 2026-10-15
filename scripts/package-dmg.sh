#!/usr/bin/env bash
# Package an existing app; never rebuild it or include account data.
set -euo pipefail

project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
app=${1:-}
output=${2:-}
if [[ $(uname -s) != Darwin || $# != 2 || "$output" != *.dmg ]]; then
  echo 'Usage (macOS): scripts/package-dmg.sh /path/to/ac-man.app /path/to/output.dmg' >&2
  exit 2
fi
test -d "$app"
test ! -L "$app"
test -f "$app/Contents/Info.plist"
test -x "$app/Contents/MacOS/AcMan"
# Keep notices inside the app, so they remain available after installation.
for document in LICENSE THIRD_PARTY_NOTICES.md DEPENDENCIES.html; do
  test -s "$app/Contents/Resources/$document"
done
codesign --verify --deep --strict "$app"
mkdir -p -- "$(dirname -- "$output")"
# dmgbuild uses ditto, retaining the app's signature and stapled ticket.
"${DMGBUILD:-dmgbuild}" -s "$project_dir/scripts/dmg/settings.py" \
  -D "app=$app" -D "background=$project_dir/scripts/dmg/background.png" \
  'ac-man — Install' "$output"
hdiutil verify "$output"
# Verify the copied bundle and install shortcut, not just the image container.
mount_dir=$(mktemp -d "${TMPDIR:-/tmp}/ac-man-dmg-check.XXXXXX")
cleanup() {
  hdiutil detach "$mount_dir" >/dev/null 2>&1 || true
  rmdir "$mount_dir" 2>/dev/null || true
}
trap cleanup EXIT
hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$mount_dir" "$output" >/dev/null
codesign --verify --deep --strict "$mount_dir/ac-man.app"
test "$(readlink "$mount_dir/Applications")" = /Applications
test -f "$mount_dir/.DS_Store"
hdiutil detach "$mount_dir" >/dev/null
rmdir "$mount_dir"
trap - EXIT
