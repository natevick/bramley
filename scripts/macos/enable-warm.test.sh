#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/bramley-enable-warm.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
LABEL="codes.vick.bramley.warm"

APP="$WORK/Bramley & Co.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$WORK/bin" "$WORK/home"
printf '#!/bin/sh\n' >"$APP/Contents/MacOS/bramley"
chmod +x "$APP/Contents/MacOS/bramley"
cp "$ROOT/packaging/macos/$LABEL.plist" "$APP/Contents/Resources/"
printf '#!/bin/sh\necho "$@" >>"%s/launchctl.calls"\n' "$WORK" >"$WORK/bin/launchctl"
chmod +x "$WORK/bin/launchctl"

HOME="$WORK/home" PATH="$WORK/bin:$PATH" "$ROOT/scripts/macos/enable-warm.sh" "$APP" >/dev/null
INSTALLED="$WORK/home/Library/LaunchAgents/$LABEL.plist"

python3 -I - "$INSTALLED" "$APP" "$WORK/home/Library/Logs" <<'PY'
import plistlib, sys
installed, app, logs = sys.argv[1:]
agent = plistlib.load(open(installed, "rb"))
assert agent["Label"] == "codes.vick.bramley.warm", agent["Label"]
assert agent["ProgramArguments"] == [app + "/Contents/MacOS/bramley", "warm"], agent["ProgramArguments"]
assert agent["StartInterval"] == 300, agent["StartInterval"]
assert agent["ProcessType"] == "Background" and agent["LowPriorityIO"] is True
assert agent["StandardOutPath"] == logs + "/bramley-warm.log", agent["StandardOutPath"]
PY

UID_DOMAIN="gui/$(id -u)"
grep -qxF "bootstrap $UID_DOMAIN $INSTALLED" "$WORK/launchctl.calls" || { echo "launchctl bootstrap was not called for $INSTALLED" >&2; exit 1; }

mkdir -p "$WORK/bare.app/Contents/MacOS"
cp "$APP/Contents/MacOS/bramley" "$WORK/bare.app/Contents/MacOS/"
for refused in "$WORK/missing.app" "$WORK/bare.app"; do
  if "$ROOT/scripts/macos/enable-warm.sh" "$refused" 2>/dev/null; then
    echo "enable-warm.sh accepted $refused, which is not a Bramley.app from bundle.sh" >&2
    exit 1
  fi
done
echo "enable-warm: ok"
