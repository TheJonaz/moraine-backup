#!/usr/bin/env bash
# Build a Moraine AppImage (GTK desktop app + CLI).
#
# Run on an OLD-ish glibc distro (e.g. Ubuntu 22.04) so the result runs
# everywhere newer. Needs: rustup/cargo, libgtk-4-dev, curl, FUSE.
#
# The AppImage bundles GTK but relies on the HOST's rsync / ssh / rclone at
# runtime (present on virtually every Linux box). Requires moraine >= 0.1.19,
# whose asset lookup honours the XDG_DATA_DIRS that linuxdeploy's AppRun exports.
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo"
APPDIR="${APPDIR:-$PWD/AppDir}"
rm -rf "$APPDIR"

echo "==> building release binaries"
cargo build --release --locked --features gui --bin moraine --bin moraine-gui

echo "==> laying out AppDir"
install -Dm755 target/release/moraine-gui "$APPDIR/usr/bin/moraine-gui"
install -Dm755 target/release/moraine     "$APPDIR/usr/bin/moraine"
install -Dm644 assets/moraine-gui.desktop "$APPDIR/usr/share/applications/io.thern.moraine.desktop"
install -Dm644 assets/moraine.svg         "$APPDIR/usr/share/icons/hicolor/scalable/apps/moraine.svg"
install -Dm644 assets/moraine-256.png     "$APPDIR/usr/share/icons/hicolor/256x256/apps/moraine.png"
# runtime assets — resolved via $XDG_DATA_DIRS/moraine/assets
for a in hero-bg.png moraine-64.png moraine-256.png; do
  install -Dm644 "assets/$a" "$APPDIR/usr/share/moraine/assets/$a"
done
# the .desktop Exec/Icon must match; top-level icon for the AppImage thumbnail
cp assets/moraine.svg "$APPDIR/moraine.svg"

echo "==> fetching linuxdeploy + gtk plugin"
tools="$PWD/.appimage-tools"; mkdir -p "$tools"
fetch() { [ -f "$tools/$1" ] || curl -fsSL -o "$tools/$1" "$2"; chmod +x "$tools/$1"; }
fetch linuxdeploy-x86_64.AppImage \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
fetch linuxdeploy-plugin-gtk.sh \
  https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh

echo "==> bundling GTK + producing the AppImage"
export DEPLOY_GTK_VERSION=4
export OUTPUT="Moraine-$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)-x86_64.AppImage"
"$tools/linuxdeploy-x86_64.AppImage" \
  --appdir "$APPDIR" \
  --plugin gtk \
  --desktop-file "$APPDIR/usr/share/applications/io.thern.moraine.desktop" \
  --icon-file "$APPDIR/moraine.svg" \
  --output appimage

echo "==> done: $OUTPUT"
