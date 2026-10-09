#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
DISP=":97"
Xvfb "$DISP" -screen 0 1280x800x24 -nolisten tcp >/dev/null 2>&1 &
XPID=$!
for i in $(seq 1 50); do [ -e /tmp/.X11-unix/X97 ] && break; sleep 0.1; done
env -u WAYLAND_DISPLAY DISPLAY="$DISP" LD_LIBRARY_PATH="$HERE/syslibs/root/usr/lib/x86_64-linux-gnu" "$HERE/ui-baseline/target/release/$1" >/dev/null 2>&1 &
APID=$!
trap 'kill $APID 2>/dev/null || true; kill $XPID 2>/dev/null || true' EXIT
sleep 10
python3 -I "$HERE/smaps_breakdown.py" "$APID"
