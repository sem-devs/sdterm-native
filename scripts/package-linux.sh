#!/usr/bin/env bash
# Builds the Linux downloads from an already built release binary (cargo build --release):
#
#   scripts/package-linux.sh
#
# Writes into dist/: a .tar.gz, a .deb, an AppImage, the update feed latest-linux.json and the
# checksums. The AppImage is what the in-app updater replaces; the .deb and the .tar.gz send you to
# the download page instead. Meant for CI (Ubuntu), it needs dpkg-deb, wget and node.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
BIN=target/release/sdterm-native
OUT=dist
[[ -x "$BIN" ]] || { echo "Build first: cargo build --release" >&2; exit 1; }
rm -rf "$OUT" && mkdir -p "$OUT"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# --- .tar.gz: the program with its resources folder beside it (where the app looks for it).
TAR=sdterm-native-$VERSION-linux-x86_64
mkdir -p "$WORK/$TAR/resources"
cp "$BIN" "$WORK/$TAR/"
cp -R resources/shell "$WORK/$TAR/resources/shell"
cp resources/linux/sdterm-native.desktop resources/sdterm-native.png LICENSE README.md "$WORK/$TAR/"
tar -C "$WORK" -czf "$OUT/$TAR.tar.gz" "$TAR"

# --- .deb: the program and its files under /usr/lib, a launcher in /usr/bin, menu entry and icon.
DEB="$WORK/deb"
mkdir -p "$DEB/DEBIAN" "$DEB/usr/lib/sdterm-native/resources" "$DEB/usr/bin" \
  "$DEB/usr/share/applications" "$DEB/usr/share/icons/hicolor/512x512/apps" "$DEB/usr/share/doc/sdterm-native"
cp "$BIN" "$DEB/usr/lib/sdterm-native/"
cp -R resources/shell "$DEB/usr/lib/sdterm-native/resources/shell"
ln -s /usr/lib/sdterm-native/sdterm-native "$DEB/usr/bin/sdterm-native"
cp resources/linux/sdterm-native.desktop "$DEB/usr/share/applications/sdterm-native.desktop"
cp resources/sdterm-native.png "$DEB/usr/share/icons/hicolor/512x512/apps/sdterm-native.png"
cp LICENSE "$DEB/usr/share/doc/sdterm-native/copyright"
SIZE_KB=$(du -sk "$DEB/usr" | cut -f1)
cat > "$DEB/DEBIAN/control" <<EOF
Package: sdterm-native
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Installed-Size: $SIZE_KB
Depends: libc6, libxkbcommon0, libxkbcommon-x11-0, libxcb1, libx11-xcb1, libfontconfig1, libvulkan1, libwayland-client0
Maintainer: SEM Devs <info@sem-devs.com>
Homepage: https://sem-devs.com/sdterm
Description: A terminal workspace
 Many terminals in one window, each with a name, a note and a color, with live resource monitors.
EOF
dpkg-deb --root-owner-group --build "$DEB" "$OUT/sdterm-native-$VERSION-linux-amd64.deb"

# --- AppImage: one file that runs anywhere, and that the app can update in place.
APPDIR="$WORK/AppDir"
mkdir -p "$APPDIR/usr/bin/resources"
cp "$BIN" "$APPDIR/usr/bin/"
cp -R resources/shell "$APPDIR/usr/bin/resources/shell"
cp resources/linux/sdterm-native.desktop "$APPDIR/sdterm-native.desktop"
cp resources/sdterm-native.png "$APPDIR/sdterm-native.png"
ln -s sdterm-native.png "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/sdterm-native" "$@"
EOF
chmod +x "$APPDIR/AppRun"
TOOL="$WORK/appimagetool"
wget -q -O "$TOOL" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x "$TOOL"
# GitHub runners have no FUSE, so the tool unpacks itself and runs from there.
ARCH=x86_64 "$TOOL" --appimage-extract-and-run "$APPDIR" "$OUT/sdterm-native-$VERSION-linux-x86_64.AppImage"
chmod +x "$OUT/sdterm-native-$VERSION-linux-x86_64.AppImage"

# --- the feed the updater reads, and the checksums.
node scripts/make-feed.mjs linux "$OUT" "sdterm-native-$VERSION-linux-x86_64.AppImage" x64
ls -la "$OUT"
