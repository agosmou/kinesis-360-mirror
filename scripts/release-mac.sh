#!/bin/sh
# Build a signed, notarized DMG and attach it to a draft GitHub release.
#
# Distribution is by download. There is no store route: store apps must be
# sandboxed, and a sandboxed app cannot hold Input Monitoring, so the capture
# this app exists to do would not work. The DMG therefore has to stand on its
# own — signed with a Developer ID certificate and notarized by Apple, or
# Gatekeeper refuses to open it on anyone else's Mac.
set -e

# .signing.env uses `export`, so it would clobber anything already set. Let an
# explicit environment win — .signing.env usually holds the *development*
# identity, which is the wrong one for a release.
if [ -z "$APPLE_SIGNING_IDENTITY" ] && [ -f .signing.env ]; then
  . ./.signing.env
fi

: "${APPLE_SIGNING_IDENTITY:?set APPLE_SIGNING_IDENTITY in .signing.env}"
: "${APPLE_ID:?set APPLE_ID (your Apple account email)}"
: "${APPLE_PASSWORD:?set APPLE_PASSWORD (an app-specific password)}"
: "${APPLE_TEAM_ID:?set APPLE_TEAM_ID}"

case "$APPLE_SIGNING_IDENTITY" in
  "Developer ID Application:"*) ;;
  *)
    echo "error: APPLE_SIGNING_IDENTITY is not a Developer ID Application cert."
    echo "       An 'Apple Development' cert works locally but Gatekeeper will"
    echo "       block the download on every other Mac. Create a Developer ID"
    echo "       Application certificate in your Apple Developer account."
    exit 1
    ;;
esac

VERSION=$(sed -n 's/.*"version": "\([0-9][^"]*\)".*/\1/p' src-tauri/tauri.conf.json | head -1)
[ -n "$VERSION" ] || { echo "could not read version from tauri.conf.json"; exit 1; }
echo "building v$VERSION"

bun run tauri build --bundles dmg

DMG=$(ls -t src-tauri/target/release/bundle/dmg/*.dmg 2>/dev/null | head -1)
[ -n "$DMG" ] || { echo "no DMG produced"; exit 1; }

# The step people skip, and then ship something that will not open. Verify the
# notarization ticket is actually attached rather than trusting the build log.
APP=src-tauri/target/release/bundle/macos/kinesis-360-mirror.app
echo "--- verifying signature and notarization ---"
codesign --verify --deep --strict --verbose=2 "$APP"
spctl --assess --type execute --verbose=4 "$APP"
xcrun stapler validate "$DMG" || {
  echo "error: no notarization ticket stapled to the DMG. Do not publish this."
  exit 1
}

echo "--- creating draft release ---"
gh release create "v$VERSION" "$DMG" \
  --draft \
  --title "v$VERSION" \
  --notes "On-screen mirror for the Kinesis Advantage 360 Pro.

Download the DMG, drag the app to /Applications, and grant it Input Monitoring
when asked (System Settings → Privacy & Security → Input Monitoring). It only
reads the Kinesis, stores nothing, and has no network code — see SECURITY.md
for how to verify that yourself."

echo
echo "Draft release created. Review it, then publish:"
echo "  gh release edit v$VERSION --draft=false"
