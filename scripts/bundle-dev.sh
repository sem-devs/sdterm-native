#!/bin/zsh
# Wraps the debug build in a real macOS app bundle and signs it, so macOS treats it as an app:
# dictation, privacy permissions and the Dock all depend on a bundle identifier.
# Signs with the Developer ID certificate when the keychain has it (permissions then survive
# rebuilds), otherwise ad hoc.
set -e
cd "$(dirname "$0")/.."
PROFILE=${1:-debug}
APP="target/$PROFILE/bundle/sdterm native.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/$PROFILE/sdterm-native" "$APP/Contents/MacOS/sdterm-native"
cp -R resources/shell "$APP/Contents/Resources/shell"
cp resources/icon.icns "$APP/Contents/Resources/icon.icns"
sed "s/__VERSION__/$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)-dev/g" resources/Info.plist > "$APP/Contents/Info.plist"
IDENTITY=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)
codesign --force --sign "${IDENTITY:--}" "$APP"
echo "$APP signed with: ${IDENTITY:-ad hoc}"
