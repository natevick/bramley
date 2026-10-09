#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
NAME="$1"; shift
WARMUP="${WARMUP:-15}"; SAMPLE="${SAMPLE:-60}"
mkdir -p "$HERE/results"
STATUS="$HERE/results/$NAME.status.json"
rm -f "$STATUS"
setsid bash -c "{ echo 'create-node adapter { factory.name=support.null-audio-sink node.name=spike5-null node.description=spike5-null media.class=Audio/Sink audio.position=[FL FR] priority.session=0 priority.driver=0 object.linger=false }'; exec sleep 100000; } | pw-cli >/dev/null 2>&1" &
SINK_PID=$!
MEAS_PID=""
cleanup() { [ -n "$MEAS_PID" ] && kill "$MEAS_PID" 2>/dev/null || true; kill -- -"$SINK_PID" 2>/dev/null || true; }
trap cleanup EXIT
for i in $(seq 1 50); do wpctl status 2>/dev/null | grep -q 'spike5-null' && break; sleep 0.1; done
wpctl status | grep -q 'spike5-null' || { echo "null sink missing; refusing to play audio" >&2; exit 2; }
DEFAULT_BEFORE="$(wpctl inspect @DEFAULT_AUDIO_SINK@ 2>/dev/null | grep -m1 'node.name' || true)"
python3 -I "$HERE/measure.py" --name "$NAME" --warmup "$WARMUP" --sample "$SAMPLE" --log "$HERE/results/$NAME.log" --out "$HERE/results/$NAME.json" -- "$HERE/engine/target/release/spike5-engine" --status "$STATUS" --sink spike5-null "$@" &
MEAS_PID=$!
sleep $(( WARMUP + SAMPLE / 2 ))
python3 -I "$HERE/pw_routing.py" "$STATUS" > "$HERE/results/$NAME.routing.json" || true
wait "$MEAS_PID"
MEAS_PID=""
DEFAULT_AFTER="$(wpctl inspect @DEFAULT_AUDIO_SINK@ 2>/dev/null | grep -m1 'node.name' || true)"
python3 -I "$HERE/verdict_5.py" "$HERE/results/$NAME.json" "$STATUS" "$HERE/results/$NAME.routing.json" "$DEFAULT_BEFORE" "$DEFAULT_AFTER" | tee "$HERE/results/$NAME.verdict.json"
