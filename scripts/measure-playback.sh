#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
export TMPDIR="$ROOT/target/tmp"
SCENARIO=""
if [ "${1:-}" = "--scenario" ]; then SCENARIO=pause-seek; shift; fi
NAME="${1:?usage: measure-playback.sh [--scenario] <name> [engine-play args...]}"; shift
RESULTS="${RESULTS_DIR:-$ROOT/target/measure-playback}"
SINK="engine-measure-null"
BIN="$ROOT/target/release/engine-play"
mkdir -p "$RESULTS" "$TMPDIR"
(cd "$ROOT" && cargo build --release --quiet -p engine --bin engine-play)
if [ -n "$SCENARIO" ]; then
  WARMUP="${WARMUP:-5}"; SAMPLE="${SAMPLE:-115}"
  set -- --scenario "$SCENARIO" --no-loop --back-buffer 10 --max-buffer 10 "$@"
  VERDICT="$ROOT/scripts/playback/scenario_verdict.py"
else
  WARMUP="${WARMUP:-15}"; SAMPLE="${SAMPLE:-60}"
  VERDICT="$ROOT/scripts/playback/verdict.py"
fi
STATUS="$RESULTS/$NAME.status.json"
rm -f "$STATUS"
setsid bash -c "{ echo 'create-node adapter { factory.name=support.null-audio-sink node.name=$SINK node.description=$SINK media.class=Audio/Sink audio.position=[FL FR] priority.session=0 priority.driver=0 object.linger=false }'; exec sleep 100000; } | pw-cli >/dev/null 2>&1" &
SINK_PID=$!
MEAS_PID=""
cleanup() { [ -n "$MEAS_PID" ] && kill "$MEAS_PID" 2>/dev/null || true; kill -- -"$SINK_PID" 2>/dev/null || true; }
trap cleanup EXIT
for _ in $(seq 1 50); do wpctl status 2>/dev/null | grep -q "$SINK" && break; sleep 0.1; done
wpctl status | grep -q "$SINK" || { echo "null sink missing; refusing to play audio" >&2; exit 2; }
DEFAULT_BEFORE="$(wpctl inspect @DEFAULT_AUDIO_SINK@ 2>/dev/null | grep -m1 'node.name' || true)"
python3 -I "$ROOT/scripts/playback/measure.py" --name "$NAME" --warmup "$WARMUP" --sample "$SAMPLE" --log "$RESULTS/$NAME.log" --out "$RESULTS/$NAME.json" -- "$BIN" --status "$STATUS" --output-device "$SINK" "$@" &
MEAS_PID=$!
sleep $(( WARMUP + 15 ))
python3 -I "$ROOT/scripts/playback/pw_routing.py" "$STATUS" > "$RESULTS/$NAME.routing.json" || true
wait "$MEAS_PID"
MEAS_PID=""
DEFAULT_AFTER="$(wpctl inspect @DEFAULT_AUDIO_SINK@ 2>/dev/null | grep -m1 'node.name' || true)"
python3 -I "$VERDICT" "$RESULTS/$NAME.json" "$STATUS" "$RESULTS/$NAME.routing.json" "$DEFAULT_BEFORE" "$DEFAULT_AFTER" "$SINK" | tee "$RESULTS/$NAME.verdict.json"
