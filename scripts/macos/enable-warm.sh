#!/usr/bin/env bash
set -euo pipefail
APP="${1:-/Applications/Bramley.app}"
APP="$(cd -- "$APP" >/dev/null 2>&1 && pwd -P)" || { echo "${1:-/Applications/Bramley.app} is not a directory" >&2; exit 1; }
LABEL="codes.vick.bramley.warm"
TEMPLATE="$APP/Contents/Resources/$LABEL.plist"
AGENTS="$HOME/Library/LaunchAgents"
LOGS="$HOME/Library/Logs"
[ -x "$APP/Contents/MacOS/bramley" ] && [ -f "$TEMPLATE" ] || { echo "$APP is not a Bramley.app built by scripts/macos/bundle.sh" >&2; exit 1; }
escaped() { printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' | sed 's/[&|\\]/\\&/g'; }

mkdir -p "$AGENTS" "$LOGS"
sed -e "s|@APP@|$(escaped "$APP")|g" -e "s|@LOGS@|$(escaped "$LOGS")|g" "$TEMPLATE" >"$AGENTS/$LABEL.plist"
if command -v plutil >/dev/null; then plutil -lint "$AGENTS/$LABEL.plist" >/dev/null; fi
DOMAIN="gui/$(id -u)"
launchctl bootout "$DOMAIN/$LABEL" 2>/dev/null || true
launchctl bootstrap "$DOMAIN" "$AGENTS/$LABEL.plist"
echo "Bramley warms its cache every 5 minutes while the app is closed. Stop it with: launchctl bootout $DOMAIN/$LABEL"
