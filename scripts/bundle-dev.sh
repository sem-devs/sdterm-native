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
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>sdterm-native</string>
  <key>CFBundleIdentifier</key><string>com.semdevs.sdterm.native</string>
  <key>CFBundleName</key><string>sdterm native</string>
  <key>CFBundleDisplayName</key><string>sdterm native</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSDocumentsFolderUsageDescription</key><string>A terminal needs access to your folders to run commands there.</string>
  <key>NSDownloadsFolderUsageDescription</key><string>A terminal needs access to your folders to run commands there.</string>
  <key>NSDesktopFolderUsageDescription</key><string>A terminal needs access to your folders to run commands there.</string>
</dict>
</plist>
PLIST
IDENTITY=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)
codesign --force --sign "${IDENTITY:--}" "$APP"
echo "$APP signed with: ${IDENTITY:-ad hoc}"
