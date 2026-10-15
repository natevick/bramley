#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN="${1:?usage: acceptance.sh <run-name> [--llvmpipe | --controls | --soak <minutes> [--trace] | --probe-seek <long-id> <short-id> | --probe-seek-v2 <pred-id> <long-id> <short-id> | --probe-seek-v3 <short-id> <pred-id> <long-id>]}"
SOAK_MIN=0
TRACE=0
case "${2:-}" in "") MODE=counted ;; --llvmpipe) MODE=llvmpipe ;; --controls) MODE=controls ;; --soak) MODE=soak; SOAK_MIN="${3:?--soak needs minutes}"; [[ "$SOAK_MIN" =~ ^[0-9]+$ ]] && [ "$SOAK_MIN" -ge 6 ] || { echo "--soak needs whole minutes >= 6" >&2; exit 2; }
  case "${4:-}" in "") ;; --trace) TRACE=1 ;; *) echo "--soak takes only --trace after the minutes" >&2; exit 2 ;; esac ;; --probe-seek) MODE=probe; PROBE_LONG="${3:?--probe-seek needs a long and a short item id}"; PROBE_SHORT="${4:?--probe-seek needs a long and a short item id}"
  [[ "$PROBE_LONG" =~ ^[A-Za-z0-9._-]+$ && "$PROBE_SHORT" =~ ^[A-Za-z0-9._-]+$ && "$PROBE_LONG" != "$PROBE_SHORT" ]] || { echo "--probe-seek needs two different item ids of letters, digits, . _ -" >&2; exit 2; }
  export BRAMLEY_PROBE_LONG="$PROBE_LONG" BRAMLEY_PROBE_SHORT="$PROBE_SHORT" ;;
  --probe-seek-v2) MODE=probe2; PROBE_PRED="${3:?--probe-seek-v2 needs a pred, a long and a short item id}"; PROBE_LONG="${4:?--probe-seek-v2 needs a pred, a long and a short item id}"; PROBE_SHORT="${5:?--probe-seek-v2 needs a pred, a long and a short item id}" ;;
  --probe-seek-v3) MODE=probe3; PROBE_SHORT="${3:?--probe-seek-v3 needs a short, a pred and a long item id}"; PROBE_PRED="${4:?--probe-seek-v3 needs a short, a pred and a long item id}"; PROBE_LONG="${5:?--probe-seek-v3 needs a short, a pred and a long item id}" ;;
  *) echo "unknown option $2" >&2; exit 2 ;; esac
case "$MODE" in probe2|probe3)
  [[ "$PROBE_PRED" =~ ^[A-Za-z0-9._-]+$ && "$PROBE_LONG" =~ ^[A-Za-z0-9._-]+$ && "$PROBE_SHORT" =~ ^[A-Za-z0-9._-]+$ && "$PROBE_PRED" != "$PROBE_LONG" && "$PROBE_PRED" != "$PROBE_SHORT" && "$PROBE_LONG" != "$PROBE_SHORT" ]] || { echo "$2 needs three different item ids of letters, digits, . _ -" >&2; exit 2; }
  export BRAMLEY_PROBE_PRED="$PROBE_PRED" BRAMLEY_PROBE_LONG="$PROBE_LONG" BRAMLEY_PROBE_SHORT="$PROBE_SHORT" ;; esac
OUT="$ROOT/target/acceptance/$RUN"
BIN="$OUT/bin"
[ -e "$OUT" ] && { echo "refusing: $OUT already exists" >&2; exit 2; }
REFERENCE_BIN="$ROOT/target/reference/bramley-main-4961e27"
[ -f "$REFERENCE_BIN" ] || { echo "refusing: $REFERENCE_BIN is missing; copy it from another worktree (sha256 starts 1a72eb4b2880ac5a)" >&2; exit 2; }
LOCK="/run/user/$(id -u)/bramley-gpu-session.lock"
exec 9>"$LOCK"
flock -n 9 || { echo "refusing: another acceptance or weston session holds $LOCK" >&2; exit 2; }
export BRAMLEY_GPU_LOCK_HELD=1
trap 'python3 -I "$ROOT/scripts/acceptance/drive.py" --cleanup "$RUN"' EXIT
python3 -I "$ROOT/scripts/acceptance/gate.py" --preflight >&2 || exit 2
mkdir -p "$BIN" "$ROOT/target/tmp"
export TMPDIR="$ROOT/target/tmp"
cd "$ROOT"
build() {
  cargo build --release --quiet -p bramley "${@:2}"
  cp target/release/bramley "$BIN/$1"
}
case "$MODE" in
  probe) ;;
  probe2|probe3) build bramley-trace --features trace ;;
  *) build bramley-sabotage --features gate-sabotage; build bramley-dev --features dev ;;
esac
if [ "$TRACE" = 1 ]; then build bramley-trace --features trace; fi
build bramley
set +e
SHIPPED_OUT="$(scripts/check-shipped-features.sh "$BIN/bramley" 2>&1)"; SHIPPED=$?
case "$MODE" in
  probe) SABOTAGE_OUT=""; SABOTAGE=0 ;;
  probe2|probe3) SABOTAGE_OUT="$(scripts/check-shipped-features.sh "$BIN/bramley-trace" 2>&1)"; SABOTAGE=$? ;;
  *) SABOTAGE_OUT="$(scripts/check-shipped-features.sh "$BIN/bramley-sabotage" 2>&1)"; SABOTAGE=$? ;;
esac
set -e
python3 -I -c 'import json, sys; a = sys.argv; json.dump({"commit": a[1], "dirty": int(a[2]) > 0, "mode": a[3], "shippedFeaturesExit": int(a[4]), "shippedFeaturesOutput": a[5], "ballastFeaturesExit": int(a[6]), "ballastFeaturesOutput": a[7], "shippedBytes": int(a[8]), "soakMinutes": int(a[10]), "trace": a[11] == "1"}, open(a[9], "w"), indent=1)' \
  "$(git rev-parse HEAD)" "$(git status --porcelain | wc -l)" "$MODE" "$SHIPPED" "$SHIPPED_OUT" "$SABOTAGE" "$SABOTAGE_OUT" "$(stat -c %s "$BIN/bramley")" "$OUT/build.json" "$SOAK_MIN" "$TRACE"
if [ "$SHIPPED" -ne 0 ]; then
  echo "FAIL: the shipped binary did not pass check-shipped-features; no check runs" >&2
  exec python3 -I scripts/acceptance/gate.py "$OUT"
fi
sg render -c "python3 -I '$ROOT/scripts/acceptance/drive.py' --out '$OUT' --mode $MODE $([ "$MODE" = soak ] && echo --soak-minutes "$SOAK_MIN") $([ "$TRACE" = 1 ] && echo --trace)"
exec python3 -I scripts/acceptance/gate.py "$OUT"
