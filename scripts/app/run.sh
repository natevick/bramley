#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NAME="${1:?usage: [SINK=1] [MEASURE='warmup sample'] run.sh <name> <bramley> [args...]}"; shift
OUT="$ROOT/target/app-runs"
mkdir -p "$OUT" "$ROOT/target/tmp"
export TMPDIR="$ROOT/target/tmp" XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
rm -f "$OUT/$NAME".*
STATUS="$OUT/$NAME.status.json"
SOCK="bramley-s2a-$$"
SINK_NAME="bramley-s2a-null-$$"
WESTON=""; SINK_PID=""; APP=""
cleanup() {
  [ -n "$APP" ] && kill "$APP" 2>/dev/null || true
  [ -n "$SINK_PID" ] && kill -- -"$SINK_PID" 2>/dev/null || true
  [ -n "$WESTON" ] && kill "$WESTON" 2>/dev/null || true
}
trap cleanup EXIT
weston --backend=headless --renderer=gl --socket="$SOCK" --width=1280 --height=800 --idle-time=0 --log="$OUT/$NAME.weston.log" >/dev/null 2>&1 &
WESTON=$!
for _ in $(seq 1 50); do [ -S "$XDG_RUNTIME_DIR/$SOCK" ] && break; sleep 0.1; done
[ -S "$XDG_RUNTIME_DIR/$SOCK" ] || { echo "weston did not start" >&2; exit 2; }
ARGS=(--status "$STATUS")
default_sink() { wpctl inspect @DEFAULT_AUDIO_SINK@ 2>/dev/null | grep -m1 'node.name' || true; }
if [ -n "${SINK:-}" ]; then
  setsid bash -c "{ echo 'create-node adapter { factory.name=support.null-audio-sink node.name=$SINK_NAME node.description=$SINK_NAME media.class=Audio/Sink audio.position=[FL FR] priority.session=0 priority.driver=0 object.linger=false }'; exec sleep 100000; } | pw-cli >/dev/null 2>&1" &
  SINK_PID=$!
  for _ in $(seq 1 50); do wpctl status 2>/dev/null | grep -q "$SINK_NAME" && break; sleep 0.1; done
  wpctl status | grep -q "$SINK_NAME" || { echo "null sink missing; refusing to play audio" >&2; exit 2; }
  default_sink >"$OUT/$NAME.default-before"
  echo "$SINK_NAME" >"$OUT/$NAME.sink-name"
  ARGS+=(--output-device "$SINK_NAME")
fi
if [ -n "${MEASURE:-}" ]; then
  read -r WARMUP SAMPLE <<<"$MEASURE"
  env -u DISPLAY WAYLAND_DISPLAY="$SOCK" python3 -I "$ROOT/scripts/playback/measure.py" --name "$NAME" --warmup "$WARMUP" --sample "$SAMPLE" --log "$OUT/$NAME.log" --out "$OUT/$NAME.pss.json" -- "$@" "${ARGS[@]}" &
else
  env -u DISPLAY WAYLAND_DISPLAY="$SOCK" timeout 180 "$@" "${ARGS[@]}" >"$OUT/$NAME.log" 2>&1 &
fi
APP=$!
ROUTED=""
while kill -0 "$APP" 2>/dev/null; do
  if PID=$(python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))["pid"])' "$STATUS" 2>/dev/null); then
    THREADS=$(cat /proc/"$PID"/task/*/comm 2>/dev/null | grep -c '^bramley-player' || true)
    printf '{"playerThreads":%s,"status":%s}\n' "${THREADS:-0}" "$(cat "$STATUS")" >>"$OUT/$NAME.status.jsonl"
    if [ -n "$SINK_PID" ] && [ -z "$ROUTED" ] && grep -q '"state":"Playing"' "$STATUS"; then
      sleep 2
      python3 -I "$ROOT/scripts/playback/pw_routing.py" "$STATUS" >"$OUT/$NAME.routing.json" && ROUTED=1
    fi
  fi
  sleep 1
done
wait "$APP" || echo "app exited $?" >>"$OUT/$NAME.log"
APP=""
if [ -n "$SINK_PID" ]; then
  default_sink >"$OUT/$NAME.default-after"
  kill -- -"$SINK_PID" 2>/dev/null || true
  SINK_PID=""
  sleep 1
  if wpctl status | grep -q "$SINK_NAME"; then echo "left" >"$OUT/$NAME.sink-after"; else echo "gone" >"$OUT/$NAME.sink-after"; fi
fi
grep -iE 'GL renderer|GL_RENDERER|Using GL' "$OUT/$NAME.weston.log" | head -3 || true
