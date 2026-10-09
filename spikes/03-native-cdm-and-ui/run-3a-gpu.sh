#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HERE/results"
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
SOCK="bramley-gpu-$$"
weston --backend=headless --renderer=gl --socket="$SOCK" --width=1280 --height=800 --idle-time=0 --log="$HERE/results/weston-gpu.log" >/dev/null 2>&1 &
WPID=$!
trap 'kill $WPID 2>/dev/null || true' EXIT
for i in $(seq 1 50); do [ -S "$XDG_RUNTIME_DIR/$SOCK" ] && break; sleep 0.1; done
[ -S "$XDG_RUNTIME_DIR/$SOCK" ] || { echo "weston did not start"; tail -20 "$HERE/results/weston-gpu.log"; exit 2; }
for BIN in "$@"; do
  NAME="3a-gpu-$BIN${RUN_SUFFIX:-}"
  env -u DISPLAY WAYLAND_DISPLAY="$SOCK" python3 -I "$HERE/measure.py" --name "$NAME" --warmup 5 --sample 30 --log "$HERE/results/$NAME.log" --out "$HERE/results/$NAME.json" -- "$HERE/ui-baseline/target/release/$BIN"
  echo "$NAME | $(grep -m1 'gl_renderer' "$HERE/results/$NAME.log" || echo 'NO RENDERER LINE') | $(grep 'SPIKE frames' "$HERE/results/$NAME.log" | tail -1 || echo 'NO FRAMES') | pss=$(python3 -I -c "import json,sys;print(json.load(open(sys.argv[1]))['pssMedMiB'])" "$HERE/results/$NAME.json")"
done
grep -iE 'renderer|drm|gpu|EGL_VENDOR|GL_RENDERER' "$HERE/results/weston-gpu.log" | head -6
