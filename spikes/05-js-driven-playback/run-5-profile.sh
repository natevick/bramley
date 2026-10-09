#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
NAME="$1"; shift
"$HERE/with-null-sink.sh" "$HERE/engine/target/release/spike5-engine" --status "$HERE/results/$NAME.status.json" --sink spike5-null --max-seconds 80 "$@" > "$HERE/results/$NAME.log" 2>&1 &
RUNNER=$!
PID=""
for i in $(seq 1 100); do PID="$(pgrep -n -x spike5-engine || true)"; [ -n "$PID" ] && break; sleep 0.1; done
[ -n "$PID" ] || { echo "engine did not start" >&2; exit 2; }
[ "$(readlink /proc/$PID/exe)" = "$HERE/engine/target/release/spike5-engine" ] || { echo "pid $PID is not our engine" >&2; exit 2; }
echo "profiling pid=$PID exe=$(readlink /proc/$PID/exe)"
CLK=$(getconf CLK_TCK)
sleep 20
T1=$(awk '{print $14+$15}' /proc/$PID/stat); W1=$(date +%s.%N)
sleep 50
T2=$(awk '{print $14+$15}' /proc/$PID/stat); W2=$(date +%s.%N)
python3 -I "$HERE/smaps_breakdown.py" "$PID" > "$HERE/results/$NAME.smaps.txt"
python3 -I -c "import sys; t1,t2,w1,w2,clk=map(float,sys.argv[1:]); print('cpu_percent_one_core=%.2f' % (100*(t2-t1)/clk/(w2-w1)))" "$T1" "$T2" "$W1" "$W2" "$CLK" | tee "$HERE/results/$NAME.cpu.txt"
ls /proc/$PID/task | wc -l | sed 's/^/threads=/' | tee -a "$HERE/results/$NAME.cpu.txt"
wait "$RUNNER" || true
cat "$HERE/results/$NAME.smaps.txt"
