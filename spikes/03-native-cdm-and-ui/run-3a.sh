#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HERE/results"
DISP=":97"
Xvfb "$DISP" -screen 0 1280x800x24 -nolisten tcp >/dev/null 2>&1 &
XPID=$!
trap 'kill $XPID 2>/dev/null || true' EXIT
for i in $(seq 1 50); do [ -e /tmp/.X11-unix/X97 ] && break; sleep 0.1; done
for BIN in "$@"; do
  NAME="3a-$BIN${RUN_SUFFIX:-}"
  env -u WAYLAND_DISPLAY DISPLAY="$DISP" LD_LIBRARY_PATH="$HERE/syslibs/root/usr/lib/x86_64-linux-gnu" python3 -I "$HERE/measure.py" --name "$NAME" --warmup 5 --sample 30 --log "$HERE/results/$NAME.log" --out "$HERE/results/$NAME.json" -- "$HERE/ui-baseline/target/release/$BIN"
  echo "checks: $(grep -m1 'gl_renderer' "$HERE/results/$NAME.log" || echo 'NO RENDERER LINE') | $(grep 'SPIKE frames' "$HERE/results/$NAME.log" | tail -1 || echo 'NO FRAMES')"
done
