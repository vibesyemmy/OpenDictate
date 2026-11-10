#!/bin/bash
# Packages open-typeless as "Open Dictate.app" inside a distributable .dmg.
#
# The Info.plist below is copied verbatim from the macos_bundle module in
# src/main.rs, which is the project's own authored bundle recipe. The
# microphone usage string is required: macOS only grants mic access to a
# signed app bundle, and a bare binary just records silence.
set -euo pipefail

cd "$(dirname "$0")"

APP_NAME="Open Dictate"
BIN_IN="open-typeless"          # cargo package name
BIN_OUT="open-dictate"          # must match CFBundleExecutable
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH="$(uname -m)"

STAGE="target/dmg-stage"
BUNDLE="$STAGE/$APP_NAME.app"
DIST="../dist"
DMG="$DIST/Open-Dictate-$VERSION-$ARCH.dmg"

echo "==> Building release binary"
cargo build --release

echo "==> Assembling $APP_NAME.app"
rm -rf "$STAGE"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
cp "target/release/$BIN_IN" "$BUNDLE/Contents/MacOS/$BIN_OUT"
chmod +x "$BUNDLE/Contents/MacOS/$BIN_OUT"

printf 'APPL????' > "$BUNDLE/Contents/PkgInfo"
cp assets/AppIcon.icns "$BUNDLE/Contents/Resources/AppIcon.icns"

cat > "$BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>$BIN_OUT</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundleIdentifier</key><string>app.opendictate.desktop</string>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSAudioCaptureUsageDescription</key><string>Meeting mode records what your Mac is playing, so the other voices on a call are in the transcript alongside yours. The recording stays on this Mac.</string>
    <key>NSMicrophoneUsageDescription</key><string>Open Dictate listens while you hold or press your dictation shortcut, and sends the recording only to the speech model you chose.</string>
    <key>NSAppleEventsUsageDescription</key><string>Open Dictate pastes your dictated text into the app you are using.</string>
</dict>
</plist>
PLIST

# macOS keys Microphone and Accessibility grants to the app's signing identity.
# An ad-hoc signature is keyed to a hash of the binary, so every rebuild looked
# like a brand new app and every permission reset. A real certificate keeps the
# designated requirement stable, and the grants survive.
#
# The default is the "Developer ID Application" certificate, which is what
# Gatekeeper accepts on other people's Macs (once the DMG is notarized below).
# Override with SIGN_ID=... for local builds, e.g.
#   SIGN_ID="Apple Development: Opeyemi Ajagbe (34598C886Y)" ./package-dmg.sh
SIGN_ID="${SIGN_ID:-Developer ID Application: Voicify Limited (NDXPR623CF)}"
if ! security find-identity -v -p codesigning | grep -qF "$SIGN_ID"; then
    echo "!!  signing identity not found: $SIGN_ID"
    echo "!!  falling back to ad-hoc; permissions will reset on every build"
    SIGN_ID="-"
fi

# The hardened runtime and a secure timestamp are both required by notarization.
# Ad-hoc signatures can't carry a timestamp, so they skip it.
SIGN_FLAGS=(--force --options runtime --entitlements assets/entitlements.plist)
if [ "$SIGN_ID" != "-" ]; then
    SIGN_FLAGS+=(--timestamp)
fi

echo "==> Signing as: $SIGN_ID"
codesign "${SIGN_FLAGS[@]}" --sign "$SIGN_ID" "$BUNDLE"
codesign --verify --strict --verbose=1 "$BUNDLE"

# Notarization is what lets Gatekeeper open the app without a warning. It
# authenticates with an App Store Connect API key, given either as
#   NOTARY_KEY=/path/AuthKey_XXXX.p8 NOTARY_KEY_ID=XXXX NOTARY_ISSUER=<issuer id>
# or as a keychain profile (default name open-dictate-notary, override with
# NOTARY_PROFILE=...), created once with `xcrun notarytool store-credentials`.
if [ -n "${NOTARY_KEY:-}" ]; then
    NOTARY_AUTH=(--key "$NOTARY_KEY" --key-id "${NOTARY_KEY_ID:?set NOTARY_KEY_ID}" --issuer "${NOTARY_ISSUER:?set NOTARY_ISSUER}")
else
    NOTARY_AUTH=(--keychain-profile "${NOTARY_PROFILE:-open-dictate-notary}")
fi
NOTARIZE=1
if [ "$SIGN_ID" = "-" ] || [[ "$SIGN_ID" != Developer\ ID* ]]; then
    echo "!!  not a Developer ID signature; skipping notarization"
    NOTARIZE=0
elif ! xcrun notarytool history "${NOTARY_AUTH[@]}" >/dev/null 2>&1; then
    echo "!!  notary credentials not usable (${NOTARY_AUTH[*]}); skipping notarization"
    echo "!!  this DMG will be blocked by Gatekeeper on other Macs"
    NOTARIZE=0
fi

# The app is notarized and stapled before the DMG is built, so the copy
# inside it carries its own ticket.
#
# Stapling only the DMG looked like it was enough - Gatekeeper accepted the
# app and reported "Notarized Developer ID" - but that answer came from
# asking Apple over the network. Dragged out of the DMG and opened on a Mac
# that is offline, or behind a firewall that blocks the check, an unstapled
# app has nothing local to prove itself with. The ticket has to be in the
# bundle, and a UDZO image is read-only, so it has to go in before the image
# is made. That costs a second notarization round trip: one for the app,
# one for the finished DMG.
if [ "$NOTARIZE" = 1 ]; then
    ZIP="target/$BIN_OUT-notarize.zip"
    rm -f "$ZIP"
    # ditto, not zip: it is the only archiver that preserves a bundle's
    # symlinks and extended attributes, which the signature covers.
    ditto -c -k --keepParent "$BUNDLE" "$ZIP"

    echo "==> Notarizing the app (this can take a few minutes)"
    xcrun notarytool submit "$ZIP" "${NOTARY_AUTH[@]}" --wait
    rm -f "$ZIP"

    echo "==> Stapling the app"
    xcrun stapler staple "$BUNDLE"
    xcrun stapler validate "$BUNDLE"
    spctl --assess --type exec --verbose=2 "$BUNDLE"
fi

echo "==> Building DMG"
ln -s /Applications "$STAGE/Applications"
mkdir -p "$DIST"
rm -f "$DMG"
hdiutil create \
    -volname "$APP_NAME" \
    -srcfolder "$STAGE" \
    -ov -format UDZO \
    "$DMG" >/dev/null

# The DMG gets its own signature and ticket, so the download itself opens
# without a warning too.
if [ "$NOTARIZE" = 1 ]; then
    echo "==> Signing DMG"
    codesign --force --timestamp --sign "$SIGN_ID" "$DMG"

    echo "==> Notarizing the DMG"
    xcrun notarytool submit "$DMG" "${NOTARY_AUTH[@]}" --wait

    echo "==> Stapling the DMG"
    xcrun stapler staple "$DMG"
    spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"
fi

# Leaving the staging bundle behind gives Spotlight a second "Open Dictate"
# to offer, which is how you end up launching a stale build by accident.
rm -rf "$STAGE"

echo
echo "DMG:  $(cd "$DIST" && pwd)/$(basename "$DMG")"
echo "Size: $(du -h "$DMG" | cut -f1)"
