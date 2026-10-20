#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${1:-$ROOT/dist}"
BUNDLE_ID="${BRAMLEY_BUNDLE_ID:-codes.vick.bramley}"
IDENTITY="${BRAMLEY_SIGN_IDENTITY:--}"
[ "$(uname -s)" = "Darwin" ] || { echo "bundle.sh builds the macOS app and needs macOS" >&2; exit 1; }

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/crates/app/Cargo.toml" | head -n1)"
(cd "$ROOT" && cargo build --release -p bramley)
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/release/bramley"

LINKED="$(strings -a "$BIN")"
grep -qF "bramley-linked:macos-player" <<<"$LINKED" || { echo "$BIN does not link the MusicKit player" >&2; exit 1; }
if grep -qF "bramley-linked:engine" <<<"$LINKED"; then
  echo "$BIN links bramley-engine; the macOS build must not" >&2
  exit 1
fi

APP="$OUT/Bramley.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
install -m 755 "$BIN" "$APP/Contents/MacOS/bramley"
[ -z "${BRAMLEY_PROVISION_PROFILE:-}" ] || cp "$BRAMLEY_PROVISION_PROFILE" "$APP/Contents/embedded.provisionprofile"
cat >"$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleName</key><string>Bramley</string>
  <key>CFBundleExecutable</key><string>bramley</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppleMusicUsageDescription</key><string>Bramley plays your Apple Music library and catalog.</string>
</dict>
</plist>
PLIST
plutil -lint "$APP/Contents/Info.plist" >/dev/null

if [ "$IDENTITY" = "-" ]; then
  codesign --force --sign - "$APP"
else
  ENTITLEMENTS=()
  if [ -n "${BRAMLEY_PROVISION_PROFILE:-}" ]; then
    WORK="$(mktemp -d "${TMPDIR:-/tmp}/bramley-bundle.XXXXXX")"
    trap 'rm -rf "$WORK"' EXIT
    security cms -D -i "$BRAMLEY_PROVISION_PROFILE" >"$WORK/profile.plist"
    plutil -extract Entitlements xml1 -o "$WORK/entitlements.plist" "$WORK/profile.plist"
    ENTITLEMENTS=(--entitlements "$WORK/entitlements.plist")
  fi
  codesign --force --options runtime --timestamp ${ENTITLEMENTS[@]+"${ENTITLEMENTS[@]}"} --sign "$IDENTITY" "$APP"
fi
codesign --verify --strict "$APP"
echo "$APP"
