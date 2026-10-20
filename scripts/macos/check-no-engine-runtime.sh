#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
status=0
for crate in pulseaudio rquickjs symphonia; do
  found="$(cargo tree -p bramley --target aarch64-apple-darwin -e normal -i "$crate" 2>/dev/null || true)"
  if grep -q "^$crate v" <<<"$found"; then
    echo "the macOS build depends on $crate; something enabled bramley-engine's runtime feature" >&2
    status=1
  fi
done
exit "$status"
