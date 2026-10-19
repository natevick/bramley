#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="x86_64-unknown-linux-gnu"
USAGE="usage: package-release.sh --check-tag <tag> | <tag> <release binary> <output dir>"

crate_version() {
  sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/crates/app/Cargo.toml" | head -n1
}

check_tag() {
  local tag="$1" expected
  expected="v$(crate_version)"
  [ "$expected" != "v" ] || { echo "cannot read the version from crates/app/Cargo.toml" >&2; exit 1; }
  [ "$tag" = "$expected" ] || { echo "tag $tag does not match the crate version ($expected)" >&2; exit 1; }
}

if [ "${1:-}" = "--check-tag" ]; then
  check_tag "${2:?$USAGE}"
  exit 0
fi

TAG="${1:?$USAGE}"
BIN="${2:?$USAGE}"
OUT="${3:?$USAGE}"
check_tag "$TAG"
VERSION="${TAG#v}"

"$ROOT/scripts/check-shipped-features.sh" "$BIN"
REPORTED="$("$BIN" --version)"
[ "$REPORTED" = "bramley $VERSION" ] || { echo "$BIN --version printed '$REPORTED', expected 'bramley $VERSION'" >&2; exit 1; }

NAME="bramley-$TAG-$TARGET"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
STAGE="$OUT/$NAME"
rm -rf "$STAGE" "$OUT/$NAME.tar.gz" "$OUT/$NAME.tar.gz.sha256"
mkdir -p "$STAGE"
install -m 755 "$BIN" "$STAGE/bramley"
(cd "$ROOT" && cp --parents LICENSE README.md packaging/applications/bramley.desktop packaging/icons/bramley.svg packaging/systemd/bramley-warm.service packaging/systemd/bramley-warm.timer "$STAGE/")
tar --sort=name --owner=0 --group=0 --numeric-owner -C "$OUT" -cf - "$NAME" | gzip -n >"$OUT/$NAME.tar.gz"
rm -rf "$STAGE"
(cd "$OUT" && sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256")
tar -tzf "$OUT/$NAME.tar.gz"
cat "$OUT/$NAME.tar.gz.sha256"
