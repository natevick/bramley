#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SVG="$ROOT/packaging/icons/bramley.svg"
OUT="${1:-$ROOT/packaging/macos/AppIcon.icns}"
[ "$(uname -s)" = "Darwin" ] || { echo "make-icon.sh needs macOS (sips, iconutil)" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/bramley-icon.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
SET="$WORK/AppIcon.iconset"
mkdir -p "$SET"
for entry in 16:icon_16x16 32:icon_16x16@2x 32:icon_32x32 64:icon_32x32@2x 128:icon_128x128 256:icon_128x128@2x 256:icon_256x256 512:icon_256x256@2x 512:icon_512x512 1024:icon_512x512@2x; do
  px="${entry%%:*}"
  sed "s/viewBox=\"0 0 256 256\" width=\"256\" height=\"256\"/viewBox=\"-32 -32 320 320\" width=\"$px\" height=\"$px\"/" "$SVG" >"$WORK/sized.svg"
  sips -s format png "$WORK/sized.svg" --out "$SET/${entry#*:}.png" >/dev/null
done
iconutil -c icns "$SET" -o "$OUT"
echo "$OUT"
