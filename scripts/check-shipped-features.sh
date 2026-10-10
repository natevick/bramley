#!/usr/bin/env bash
set -euo pipefail
BIN="${1:?usage: check-shipped-features.sh <release binary>}"
PREFIX="bramley-unshipped-feature:"
[ -s "$BIN" ] || { echo "no binary at $BIN" >&2; exit 2; }
command -v strings >/dev/null || { echo "strings is not installed" >&2; exit 2; }
ALL="$(strings -a "$BIN")"
[ -n "$ALL" ] || { echo "strings read nothing from $BIN" >&2; exit 2; }
FOUND="$(grep -o "${PREFIX}[A-Za-z0-9/_-]*" <<<"$ALL" | sort -u || true)"
if [ -n "$FOUND" ]; then
  echo "FAIL: $BIN ships test-only features:"
  sed "s/^${PREFIX}/  /" <<<"$FOUND"
  exit 1
fi
echo "OK: $BIN has no test-only feature markers"
