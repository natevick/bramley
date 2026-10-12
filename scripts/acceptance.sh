#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN="${1:?usage: acceptance.sh <run-name> [--llvmpipe | --controls]}"
case "${2:-}" in "") MODE=counted ;; --llvmpipe) MODE=llvmpipe ;; --controls) MODE=controls ;; *) echo "unknown option $2" >&2; exit 2 ;; esac
OUT="$ROOT/target/acceptance/$RUN"
BIN="$OUT/bin"
[ -e "$OUT" ] && { echo "refusing: $OUT already exists" >&2; exit 2; }
python3 -I "$ROOT/scripts/acceptance/drive.py" --preflight >&2 || exit 2
mkdir -p "$BIN" "$ROOT/target/tmp"
export TMPDIR="$ROOT/target/tmp"
cd "$ROOT"
build() {
  cargo build --release --quiet -p bramley "${@:2}"
  cp target/release/bramley "$BIN/$1"
}
build bramley-sabotage --features gate-sabotage
build bramley-dev --features dev
build bramley
set +e
SHIPPED_OUT="$(scripts/check-shipped-features.sh "$BIN/bramley" 2>&1)"; SHIPPED=$?
SABOTAGE_OUT="$(scripts/check-shipped-features.sh "$BIN/bramley-sabotage" 2>&1)"; SABOTAGE=$?
set -e
python3 -I -c 'import json, sys; a = sys.argv; json.dump({"commit": a[1], "dirty": int(a[2]) > 0, "mode": a[3], "shippedFeaturesExit": int(a[4]), "shippedFeaturesOutput": a[5], "ballastFeaturesExit": int(a[6]), "ballastFeaturesOutput": a[7], "shippedBytes": int(a[8])}, open(a[9], "w"), indent=1)' \
  "$(git rev-parse HEAD)" "$(git status --porcelain | wc -l)" "$MODE" "$SHIPPED" "$SHIPPED_OUT" "$SABOTAGE" "$SABOTAGE_OUT" "$(stat -c %s "$BIN/bramley")" "$OUT/build.json"
if [ "$SHIPPED" -ne 0 ]; then
  echo "FAIL: the shipped binary did not pass check-shipped-features; no check runs" >&2
  exec python3 -I scripts/acceptance/gate.py "$OUT"
fi
sg render -c "python3 -I '$ROOT/scripts/acceptance/drive.py' --out '$OUT' --mode $MODE"
exec python3 -I scripts/acceptance/gate.py "$OUT"
