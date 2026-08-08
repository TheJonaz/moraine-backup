#!/usr/bin/env bash
# Build a Moraine AppImage (GTK desktop app + CLI).
#
# Build on the OLDEST base that still has GTK >= 4.10, which today is Ubuntu
# 24.04 — moraine builds gtk4-rs with features = ["v4_10"], so the usual
# "build on 22.04 for portability" advice cannot work there at all (22.04 has
# GTK 4.6). That puts the glibc floor at 2.39: Ubuntu 24.04+, Debian 13+,
# Fedora 40+ and the rolling distros run this; Ubuntu 22.04 and Debian 12 do
# not, and are pointed at the Flatpak or the .deb instead.
#
# Needs: rustup/cargo, libgtk-4-dev, curl, patchelf, and FUSE — or set
# APPIMAGE_EXTRACT_AND_RUN=1 when there is no FUSE (CI, containers).
#
# The bundle deliberately leaves 7 libraries to the host (libX11, libxcb,
# libwayland-client, libfontconfig, libfreetype, libharfbuzz, libfribidi):
# they are on AppImage's excludelist because they must match the host's
# display and font stack. Every desktop has them; a bare container does not,
# which is why a container smoke test needs them installed first.
#
# The AppImage bundles GTK but relies on the HOST's rsync / ssh / rclone at
# runtime (present on virtually every Linux box). Requires moraine >= 0.1.19,
# whose asset lookup honours the XDG_DATA_DIRS that linuxdeploy's AppRun exports.
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo"
APPDIR="${APPDIR:-$PWD/AppDir}"
rm -rf "$APPDIR"

# Native builds only — linuxdeploy bundles the host's GTK, so the AppImage's
# architecture is whatever machine is running this. x86_64 and aarch64 are the
# two GitHub provides runners for.
arch=$(uname -m)
case "$arch" in
  x86_64|aarch64) ;;
  *) echo "build-appimage.sh: unsupported architecture $arch" >&2; exit 1 ;;
esac

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
fetch "linuxdeploy-$arch.AppImage" \
  "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$arch.AppImage"
fetch linuxdeploy-plugin-gtk.sh \
  https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh

echo "==> bundling GTK + producing the AppImage"
export DEPLOY_GTK_VERSION=4
version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
export OUTPUT="Moraine-$version-$arch.AppImage"
# appimagetool (which linuxdeploy invokes) guesses the architecture from the
# binaries and aborts when the guess is ambiguous. Telling it outright is one
# variable and removes the failure mode entirely.
export ARCH="$arch"
"$tools/linuxdeploy-$arch.AppImage" \
  --appdir "$APPDIR" \
  --plugin gtk \
  --desktop-file "$APPDIR/usr/share/applications/io.thern.moraine.desktop" \
  --icon-file "$APPDIR/moraine.svg" \
  --output appimage

echo "==> done: $OUTPUT"
