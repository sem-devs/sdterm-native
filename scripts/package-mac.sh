#!/bin/zsh
# Builds the macOS release: a signed, notarized app in a signed, notarized DMG, plus the zip the
# updater downloads and the feed file that points to it.
#
#   scripts/package-mac.sh [arm64|x64|all]     (default: the architecture of this Mac)
#
# Needs, once on this Mac:
#   * a "Developer ID Application" certificate in the login keychain
#   * notary credentials: xcrun notarytool store-credentials "sdterm-notary" ...
#   * for the Intel build on an Apple Silicon Mac: rustup target add x86_64-apple-darwin
#
# Output goes to dist/<version>/.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
PROFILE=${SDTERM_NOTARY_PROFILE:-sdterm-notary}
IDENTITY=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)
[[ -n "$IDENTITY" ]] || { echo "No Developer ID Application certificate in the keychain." >&2; exit 1; }
xcrun notarytool history --keychain-profile "$PROFILE" >/dev/null 2>&1 || { echo "Notary profile '$PROFILE' is missing or not accepted by Apple." >&2; exit 1; }

case "${1:-native}" in
  arm64) ARCHS=(arm64) ;;
  x64) ARCHS=(x64) ;;
  all) ARCHS=(arm64 x64) ;;
  *) [[ $(uname -m) == arm64 ]] && ARCHS=(arm64) || ARCHS=(x64) ;;
esac

OUT="dist/$VERSION"
mkdir -p "$OUT"
FEED_FILES=()

for ARCH in $ARCHS; do
  if [[ $ARCH == arm64 ]]; then TARGET=aarch64-apple-darwin; else TARGET=x86_64-apple-darwin; fi
  echo "== $ARCH ($TARGET)"
  cargo build --release --target $TARGET

  NAME="sdterm-native-$VERSION-mac-$ARCH"
  WORK="$OUT/.work-$ARCH"
  APP="$WORK/sdterm native.app"
  rm -rf "$WORK"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
  cp "target/$TARGET/release/sdterm-native" "$APP/Contents/MacOS/sdterm-native"
  cp -R resources/shell "$APP/Contents/Resources/shell"
  cp resources/icon.icns "$APP/Contents/Resources/icon.icns"
  sed "s/__VERSION__/$VERSION/g" resources/Info.plist > "$APP/Contents/Info.plist"

  # Sign with the hardened runtime, then have Apple notarize the app and attach the ticket.
  codesign --force --options runtime --timestamp --entitlements resources/entitlements.plist --sign "$IDENTITY" "$APP"
  ditto -c -k --keepParent "$APP" "$WORK/notarize.zip"
  xcrun notarytool submit "$WORK/notarize.zip" --keychain-profile "$PROFILE" --wait
  xcrun stapler staple "$APP"
  spctl --assess --type execute --verbose=2 "$APP"

  # The zip the updater downloads: the stapled app.
  ZIP="$OUT/$NAME.zip"
  rm -f "$ZIP"; ditto -c -k --keepParent "$APP" "$ZIP"

  # The DMG people install from, signed and notarized too.
  DMG="$OUT/$NAME.dmg"
  rm -f "$DMG"
  STAGE="$WORK/dmg"; mkdir -p "$STAGE"; cp -R "$APP" "$STAGE/"; ln -s /Applications "$STAGE/Applications"
  hdiutil create -volname "sdterm native $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
  codesign --force --timestamp --sign "$IDENTITY" "$DMG"
  xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait
  xcrun stapler staple "$DMG"

  rm -rf "$WORK"
done

# The feed the updater reads: every architecture that has a zip in this folder.
FEED_FILES=()
for ZIP in "$OUT"/sdterm-native-$VERSION-mac-*.zip(N); do
  FILE=${ZIP:t}
  ARCH=${${FILE%.zip}##*-}
  SHA=$(openssl dgst -sha512 -binary "$ZIP" | base64)
  FEED_FILES+=("{\"arch\":\"$ARCH\",\"url\":\"$FILE\",\"sha512\":\"$SHA\",\"size\":$(stat -f %z "$ZIP")}")
done
NOTES=${SDTERM_RELEASE_NOTES:-}
printf '{"version":"%s","notes":"%s","releaseDate":"%s","files":[%s]}\n' \
  "$VERSION" "$NOTES" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "${(j:,:)FEED_FILES}" > "$OUT/latest-mac.json"

(cd "$OUT" && shasum -a 256 *.dmg *.zip > SHA256SUMS.txt)
echo "Done: $OUT"; ls -la "$OUT"
