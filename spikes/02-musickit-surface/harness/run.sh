#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
PORT=8766
setsid bash -c "{ echo 'create-node adapter { factory.name=support.null-audio-sink node.name=spike2-null node.description=spike2-null media.class=Audio/Sink audio.position=[FL FR] priority.session=0 priority.driver=0 object.linger=false }'; exec sleep 100000; } | pw-cli >/dev/null 2>&1" &
SINK_PID=$!
python3 -I -m http.server "$PORT" --bind 127.0.0.1 --directory "$HERE" >/dev/null 2>&1 &
HTTP_PID=$!
cleanup() { kill -- -"$SINK_PID" 2>/dev/null || true; kill "$HTTP_PID" 2>/dev/null || true; }
trap cleanup EXIT
for i in $(seq 1 50); do wpctl status 2>/dev/null | grep -q 'spike2-null' && break; sleep 0.1; done
wpctl status | grep -q 'spike2-null' || { echo "null sink missing; refusing to play audio" >&2; exit 2; }
for i in $(seq 1 50); do curl -s -o /dev/null "http://127.0.0.1:$PORT/page.html" && break; sleep 0.1; done
NAME="$1"; shift
node "$HERE/harness.mjs" --name "$NAME" --port "$PORT" "$@"
