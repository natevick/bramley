#!/usr/bin/env bash
set -euo pipefail
BIN="${1:?usage: check-shipped-features.sh <release binary>}"
PREFIX="bramley-unshipped-feature:"
LINKED="bramley-linked:engine"
refuse() { echo "NOT CHECKED: $BIN $1" >&2; exit 2; }
[ -f "$BIN" ] && [ -s "$BIN" ] || refuse "is not a regular, non-empty file"
[ "$(head -c 4 "$BIN" | od -An -tx1 | tr -d ' \n')" = "7f454c46" ] || refuse "is not an ELF binary"
command -v strings >/dev/null || refuse "cannot be read: strings is not installed"
ALL="$(strings -a "$BIN")"
grep -qF "$LINKED" <<<"$ALL" || refuse "does not link bramley-engine (no $LINKED marker)"
FOUND="$(grep -o "${PREFIX}[A-Za-z0-9/_-]*" <<<"$ALL" | sort -u || true)"
if [ -n "$FOUND" ]; then
  echo "FAIL: $BIN ships test-only features:"
  sed "s/^${PREFIX}/  /" <<<"$FOUND"
  exit 1
fi
echo "OK: $BIN links bramley-engine and has no test-only feature markers"
