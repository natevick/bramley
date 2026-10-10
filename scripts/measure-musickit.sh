#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
export TMPDIR="$ROOT/target/tmp"
NAME="${1:?usage: measure-musickit.sh <name> [bramley-musickit args...]}"; shift
RESULTS="${RESULTS_DIR:-$ROOT/target/measure-musickit}"
SINK="musickit-measure-null"
BIN="$ROOT/target/release/bramley-musickit"
WARMUP="${WARMUP:-15}"; SAMPLE="${SAMPLE:-60}"
CONTROLS_AT=$(( WARMUP + SAMPLE + 5 ))
mkdir -p "$RESULTS" "$TMPDIR"
(cd "$ROOT" && cargo build --release --quiet -p bramley-engine --bin bramley-musickit --features sabotage,diagnostics)
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
python3 -I "$ROOT/scripts/playback/measure.py" --name "$NAME" --warmup "$WARMUP" --sample $(( SAMPLE + 60 )) --log "$RESULTS/$NAME.log" --out "$RESULTS/$NAME.json" -- "$BIN" --status "$STATUS" --output-device "$SINK" --controls-at "$CONTROLS_AT" --max-seconds $(( CONTROLS_AT + 60 )) "$@" &
MEAS_PID=$!
sleep $(( WARMUP + 15 ))
python3 -I "$ROOT/scripts/playback/pw_routing.py" "$STATUS" > "$RESULTS/$NAME.routing.json" || true
wait "$MEAS_PID"
MEAS_PID=""
DEFAULT_AFTER="$(wpctl inspect @DEFAULT_AUDIO_SINK@ 2>/dev/null | grep -m1 'node.name' || true)"
python3 -I "$ROOT/scripts/musickit/verdict.py" "$RESULTS/$NAME.json" "$STATUS" "$RESULTS/$NAME.routing.json" "$DEFAULT_BEFORE" "$DEFAULT_AFTER" "$SINK" "$WARMUP" "$SAMPLE" | tee "$RESULTS/$NAME.verdict.json"
