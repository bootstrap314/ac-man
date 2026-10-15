#!/usr/bin/env bash
# Convert the source artwork into all standard macOS icon representations.
set -euo pipefail
project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
staging=$(mktemp -d)
trap 'rm -rf -- "$staging"' EXIT
mkdir "$staging/AppIcon.iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$project_dir/assets/AppIcon.png" \
    --out "$staging/AppIcon.iconset/icon_${size}x${size}.png" >/dev/null
  retina=$((size * 2))
  sips -z "$retina" "$retina" "$project_dir/assets/AppIcon.png" \
    --out "$staging/AppIcon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$staging/AppIcon.iconset" -o "$project_dir/assets/AppIcon.icns"
