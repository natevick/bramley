#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
SOCK="bramley-s0-$$"
weston --backend=headless --renderer=gl --socket="$SOCK" --width=1280 --height=800 --idle-time=0 --log="$HERE/results/weston-s0-probe.log" >/dev/null 2>&1 &
WPID=$!
for i in $(seq 1 50); do [ -S "$XDG_RUNTIME_DIR/$SOCK" ] && break; sleep 0.1; done
env -u DISPLAY WAYLAND_DISPLAY="$SOCK" "$HERE/ui-baseline/target/release/$1" >"$HERE/results/s0-probe-$1.log" 2>&1 &
APID=$!
trap 'kill $APID 2>/dev/null || true; kill $WPID 2>/dev/null || true' EXIT
sleep 8
grep -m1 gl_renderer "$HERE/results/s0-probe-$1.log"
python3 -I "$HERE/smaps_breakdown.py" "$APID"
