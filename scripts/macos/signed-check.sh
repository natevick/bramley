#!/usr/bin/env bash
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BUNDLE_ID="${BRAMLEY_BUNDLE_ID:-codes.vick.bramley}"
RESULT="${BRAMLEY_SIGNED_CHECK_OUT:-$HOME/bramley-spike/signed-check.out}"
SIGNED_DIR="${BRAMLEY_SIGNED_DIR:-$HOME/bramley-spike/signed}"
SPIKE_ENV="$HOME/bramley-spike/macnative-env.sh"
[ "$(uname -s)" = "Darwin" ] || { echo "signed-check.sh needs macOS" >&2; exit 1; }
# shellcheck source=/dev/null
[ ! -f "$SPIKE_ENV" ] || . "$SPIKE_ENV"
mkdir -p "$(dirname "$RESULT")" "$SIGNED_DIR"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/bramley-signed-check.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

IDENTITY="$(security find-identity -v -p codesigning | sed -n 's/^ *[0-9]*) \([0-9A-F]\{40\}\) "\(Apple Development: .*\)"$/\1/p' | head -n1)"
if [ -z "$IDENTITY" ]; then
  echo "No 'Apple Development' signing identity is visible. Run this in Terminal on the Mac's own screen with the login keychain unlocked; over ssh it is not in the search list." >&2
  exit 1
fi

PROFILE="${BRAMLEY_PROVISION_PROFILE:-}"
if [ -z "$PROFILE" ]; then
  for file in "$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"/*.provisionprofile "$HOME/Library/MobileDevice/Provisioning Profiles"/*.provisionprofile; do
    [ -f "$file" ] || continue
    security cms -D -i "$file" 2>/dev/null | grep -qF ".$BUNDLE_ID<" && { PROFILE="$file"; break; }
  done
fi

APP="$(cd "$ROOT" && BRAMLEY_BUNDLE_ID="$BUNDLE_ID" BRAMLEY_SIGN_IDENTITY="$IDENTITY" BRAMLEY_PROVISION_PROFILE="$PROFILE" scripts/macos/bundle.sh "$SIGNED_DIR" | tail -n1)" || exit 1

run_probe() {
  : >"$WORK/out"
  : >"$WORK/err"
  open -W -n --stdout "$WORK/out" --stderr "$WORK/err" "$APP" --args musickit-probe "$@"
  cat "$WORK/out"
  sed 's/^/stderr: /' "$WORK/err"
}

verdict() {
  local last="$1"
  case "$last" in
    *'"authorization":"authorized"'*'"developerBytes":'*) echo "VERDICT: PASS. Authorized and both tokens were issued." ;;
    *'"authorization":"denied"'*) echo "VERDICT: DENIED. Consent was refused or previously denied. Allow Bramley in System Settings > Privacy & Security > Media & Apple Music, then re-run." ;;
    *'"authorization":"restricted"'*) echo "VERDICT: RESTRICTED. Apple Music access is restricted on this Mac (Screen Time or a profile)." ;;
    *) echo "VERDICT: NO TOKENS. Either no consent prompt appeared or the tokens were refused. macOS likely needs a provisioning profile for $BUNDLE_ID that carries the MusicKit service. Create one by letting Xcode manage it: new macOS App project, bundle id $BUNDLE_ID, your team, automatic signing, add the MusicKit capability, build once. Then re-run; the script picks the profile up from ~/Library/Developer/Xcode/UserData/Provisioning Profiles, or set BRAMLEY_PROVISION_PROFILE to its path." ;;
  esac
}

{
  echo "date: $(date -u +%FT%TZ)"
  echo "bundle id: $BUNDLE_ID"
  echo "identity: $(security find-identity -v -p codesigning | grep "$IDENTITY" | sed 's/^[^"]*//')"
  echo "profile: ${PROFILE:-none}"
  echo "bundle: $APP"
  codesign -dv "$APP" 2>&1 | grep -E '^(Identifier|TeamIdentifier|Authority|CodeDirectory)'
  echo "entitlement keys: $(codesign -d --entitlements :- "$APP" 2>/dev/null | grep -o '<key>[^<]*</key>' | sed 's/<[^>]*>//g' | tr '\n' ' ')"
  echo "== status only"
  run_probe
  echo "== request (the consent prompt appears on this screen) and token request"
  run_probe --request >"$WORK/last"
  cat "$WORK/last"
  verdict "$(cat "$WORK/last")"
} >"$WORK/report" 2>&1

if grep -q 'eyJ' "$WORK/report"; then
  echo "output contained a token-shaped string and was withheld" >"$RESULT"
else
  cp "$WORK/report" "$RESULT"
fi
cat "$RESULT"
echo "result file: $RESULT"
