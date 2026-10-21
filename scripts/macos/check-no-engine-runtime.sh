#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
runtime_deps=(pulseaudio rquickjs symphonia)
reaches() {
  local out
  out="$(cargo tree -p bramley --target "$1" -e normal -i "$2")" || { echo "FAIL: cargo tree failed for $2 on $1" >&2; exit 1; }
  grep -q "^$2 v" <<<"$out"
}
for crate in "${runtime_deps[@]}"; do
  if ! reaches x86_64-unknown-linux-gnu "$crate"; then
    echo "FAIL: the check is blind; the Linux build does not reach $crate, which bramley-engine's runtime feature pulls in" >&2
    exit 1
  fi
done
status=0
for crate in "${runtime_deps[@]}"; do
  if reaches aarch64-apple-darwin "$crate"; then
    echo "FAIL: the macOS build depends on $crate; something enabled bramley-engine's runtime feature" >&2
    status=1
  fi
done
[ "$status" -eq 0 ] && echo "PASS: the macOS build reaches none of ${runtime_deps[*]}; the Linux build reaches all of them"
exit "$status"
