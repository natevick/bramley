#!/usr/bin/env bash
set -euo pipefail
BUNDLE_ID="${BRAMLEY_BUNDLE_ID:-codes.vick.bramley}"
TEAM="${BRAMLEY_TEAM_ID:?set BRAMLEY_TEAM_ID to your Apple developer team id}"
PROFILES="$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"
[ "$(uname -s)" = "Darwin" ] || { echo "make-profile.sh needs macOS and Xcode" >&2; exit 1; }
command -v xcodegen >/dev/null || { echo "xcodegen is not installed (brew install xcodegen)" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/bramley-profile.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/Sources"
echo 'print("stub")' >"$WORK/Sources/main.swift"
cat >"$WORK/Stub.entitlements" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.developer.aps-environment</key>
  <string>development</string>
</dict>
</plist>
PLIST
cat >"$WORK/project.yml" <<YAML
name: BramleyStub
options:
  bundleIdPrefix: ${BUNDLE_ID%.*}
  deploymentTarget:
    macOS: "14.0"
targets:
  BramleyStub:
    type: application
    platform: macOS
    sources: [Sources]
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: $BUNDLE_ID
        PRODUCT_NAME: BramleyStub
        DEVELOPMENT_TEAM: $TEAM
        CODE_SIGN_STYLE: Automatic
        CODE_SIGN_IDENTITY: "Apple Development"
        GENERATE_INFOPLIST_FILE: YES
        INFOPLIST_KEY_NSAppleMusicUsageDescription: "Bramley plays your Apple Music library."
        ENABLE_HARDENED_RUNTIME: YES
        CODE_SIGN_ENTITLEMENTS: Stub.entitlements
YAML

(cd "$WORK" && xcodegen generate --quiet)
(cd "$WORK" && xcodebuild -project BramleyStub.xcodeproj -scheme BramleyStub -configuration Debug -derivedDataPath "$WORK/dd" -allowProvisioningUpdates -allowProvisioningDeviceRegistration build) | grep -E "error|BUILD (SUCCEEDED|FAILED)"

for file in "$PROFILES"/*.provisionprofile; do
  if security cms -D -i "$file" 2>/dev/null | grep -qF ".$BUNDLE_ID<"; then echo "$file"; fi
done
