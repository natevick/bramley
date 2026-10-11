#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN="${1:?usage: acceptance.sh <run-name> [--llvmpipe]}"
MODE="${2:-}"
case "$MODE" in "" | --llvmpipe) ;; *) echo "unknown option $MODE" >&2; exit 2 ;; esac
OUT="$ROOT/target/acceptance/$RUN"
BIN="$OUT/bin"
[ -e "$OUT" ] && { echo "refusing: $OUT already exists" >&2; exit 2; }
python3 -I "$ROOT/scripts/acceptance/gate.py" --preflight >&2 || exit 2
mkdir -p "$BIN" "$ROOT/target/tmp"
export TMPDIR="$ROOT/target/tmp"
cd "$ROOT"
build() {
  cargo build --release --quiet -p bramley "${@:2}"
  cp target/release/bramley "$BIN/$1"
}
build bramley-ballast --features gate-sabotage
build bramley-dev --features dev
build bramley
set +e
SHIPPED_OUT="$(scripts/check-shipped-features.sh "$BIN/bramley" 2>&1)"; SHIPPED=$?
BALLAST_OUT="$(scripts/check-shipped-features.sh "$BIN/bramley-ballast" 2>&1)"; BALLAST=$?
set -e
python3 -I -c 'import json, sys; a = sys.argv; json.dump({"commit": a[1], "dirty": int(a[2]) > 0, "llvmpipe": a[3] == "--llvmpipe", "shippedFeaturesExit": int(a[4]), "shippedFeaturesOutput": a[5], "ballastFeaturesExit": int(a[6]), "ballastFeaturesOutput": a[7]}, open(a[8], "w"), indent=1)' \
  "$(git rev-parse HEAD)" "$(git status --porcelain | wc -l)" "$MODE" "$SHIPPED" "$SHIPPED_OUT" "$BALLAST" "$BALLAST_OUT" "$OUT/build.json"
if [ "$SHIPPED" -ne 0 ]; then
  echo "FAIL: the shipped binary did not pass check-shipped-features; no check runs" >&2
  python3 -I scripts/acceptance/gate.py --out "$OUT" --verdict-only
  exit 1
fi
LLVMPIPE_FLAG=""
[ "$MODE" = --llvmpipe ] && LLVMPIPE_FLAG=--llvmpipe
sg render -c "python3 -I '$ROOT/scripts/acceptance/gate.py' --out '$OUT' $LLVMPIPE_FLAG"
