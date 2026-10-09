#!/usr/bin/env bash
set -euo pipefail
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
setsid bash -c "{ echo 'create-node adapter { factory.name=support.null-audio-sink node.name=spike5-null node.description=spike5-null media.class=Audio/Sink audio.position=[FL FR] priority.session=0 priority.driver=0 object.linger=false }'; exec sleep 100000; } | pw-cli >/dev/null 2>&1" &
SINK_PID=$!
cleanup() { kill -- -"$SINK_PID" 2>/dev/null || true; }
trap cleanup EXIT
for i in $(seq 1 50); do wpctl status 2>/dev/null | grep -q 'spike5-null' && break; sleep 0.1; done
wpctl status | grep -q 'spike5-null' || { echo "null sink missing; refusing to play audio" >&2; exit 2; }
"$@"
