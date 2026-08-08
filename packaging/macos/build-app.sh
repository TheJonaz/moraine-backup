#!/usr/bin/env bash
# Build a self-contained macOS desktop app (GTK4 GUI) as Moraine.app + a .dmg.
#
# This is the counterpart to build-pkg.sh (the CLI installer). It bundles the
# whole GTK 4 runtime — dylibs, gdk-pixbuf loaders, compiled GSettings schemas
# and the Adwaita icon theme — inside the .app, so it runs on a clean Mac with
# nothing from Homebrew present.
#
# Apple Silicon (arm64) only. Unlike the CLI, the GUI links Homebrew's GTK
# dylibs, which are single-arch; a universal .app would mean universalising every
# dependent dylib, which Homebrew does not ship. Intel Macs build from source /
# the Homebrew formula. (Same reason the .tar.gz macOS asset is arm64.)
#
# Needs: macOS (arm64), Xcode command-line tools, Homebrew, rustup. Homebrew
# packages (gtk4, adwaita-icon-theme, librsvg, dylibbundler) are installed if
# missing.
#
# Signing is optional and off by default (the bundle is always *ad-hoc* signed,
# which arm64 requires to launch at all). For a Developer ID build, export:
#     MORAINE_CODESIGN_ID   "Developer ID Application: Name (TEAMID)"
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd); cd "$repo"
here="$repo/packaging/macos"
[ "$(uname -s)" = Darwin ] || { echo "build-app.sh: macOS only" >&2; exit 1; }

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
ident=io.thern.moraine
target=aarch64-apple-darwin
APP="Moraine.app"
out="moraine-$version-macos-arm64.dmg"
export MACOSX_DEPLOYMENT_TARGET=11.0

echo "==> Homebrew dependencies"
for f in gtk4 adwaita-icon-theme librsvg dylibbundler; do
  brew list "$f" >/dev/null 2>&1 || brew install "$f"
done
BREW=$(brew --prefix)

echo "==> building moraine-gui + moraine (arm64, --features gui)"
rustup target add "$target" >/dev/null
cargo build --release --locked --features gui --bin moraine-gui --bin moraine --target "$target"

echo "==> laying out $APP"
rm -rf "$APP" dist-app "$out"
CONTENTS="$APP/Contents"; MACOS="$CONTENTS/MacOS"; RES="$CONTENTS/Resources"; FW="$CONTENTS/Frameworks"
mkdir -p "$MACOS" "$RES" "$FW"
# The real GUI binary is *.bin; MacOS/moraine-gui is a launcher script (below).
cp "target/$target/release/moraine-gui" "$MACOS/moraine-gui.bin"
cp "target/$target/release/moraine"     "$MACOS/moraine"

# ---- app icon: build an .icns from our PNGs (no prebuilt .icns needed) ----
iconset="$(mktemp -d)/moraine.iconset"; mkdir -p "$iconset"
cp assets/moraine-16.png  "$iconset/icon_16x16.png"
cp assets/moraine-32.png  "$iconset/icon_16x16@2x.png"
cp assets/moraine-32.png  "$iconset/icon_32x32.png"
cp assets/moraine-64.png  "$iconset/icon_32x32@2x.png"
cp assets/moraine-128.png "$iconset/icon_128x128.png"
cp assets/moraine-256.png "$iconset/icon_128x128@2x.png"
cp assets/moraine-256.png "$iconset/icon_256x256.png"
cp assets/moraine-512.png "$iconset/icon_256x256@2x.png"
cp assets/moraine-512.png "$iconset/icon_512x512.png"
iconutil -c icns "$iconset" -o "$RES/moraine.icns"

# ---- Info.plist ----
sed -e "s/@VERSION@/$version/g" -e "s/@IDENT@/$ident/g" "$here/Info.plist.in" > "$CONTENTS/Info.plist"

# ---- gdk-pixbuf image loaders + a relative cache ----
PBVER=$(ls "$BREW/lib/gdk-pixbuf-2.0")
LOAD="$RES/lib/gdk-pixbuf-2.0/$PBVER/loaders"
mkdir -p "$LOAD"
cp "$BREW/lib/gdk-pixbuf-2.0/$PBVER/loaders/"*.so "$LOAD/"
GDK_PIXBUF_MODULEDIR="$LOAD" gdk-pixbuf-query-loaders "$LOAD"/*.so \
  | sed "s|$LOAD|@executable_path/../Resources/lib/gdk-pixbuf-2.0/$PBVER/loaders|g" \
  > "$RES/lib/gdk-pixbuf-2.0/$PBVER/loaders.cache"

# ---- compiled GSettings schemas (GTK reads these at startup) ----
mkdir -p "$RES/share/glib-2.0/schemas"
cp "$BREW/share/glib-2.0/schemas/gschemas.compiled" "$RES/share/glib-2.0/schemas/" 2>/dev/null \
  || glib-compile-schemas "$BREW/share/glib-2.0/schemas" --targetdir "$RES/share/glib-2.0/schemas"

# ---- icon themes: Adwaita (symbolic icons the widgets use) + our app icon ----
mkdir -p "$RES/share/icons"
cp -R "$BREW/share/icons/Adwaita" "$RES/share/icons/"
cp -R "$BREW/share/icons/hicolor" "$RES/share/icons/" 2>/dev/null || true
for s in 16 24 32 48 64 128 256 512; do
  d="$RES/share/icons/hicolor/${s}x${s}/apps"; mkdir -p "$d"
  cp "assets/moraine-$s.png" "$d/moraine.png" 2>/dev/null || true
done
mkdir -p "$RES/share/icons/hicolor/scalable/apps"
cp assets/moraine.svg "$RES/share/icons/hicolor/scalable/apps/moraine.svg"
gtk4-update-icon-cache -q -t -f "$RES/share/icons/hicolor" 2>/dev/null || true

# ---- bundle every non-system dylib and rewrite install names ----
# dylibbundler walks the closure of the GUI binary AND the pixbuf loaders,
# copies each non-system dylib into Frameworks/ and repoints it at
# @executable_path/../Frameworks.
so_args=(); for so in "$LOAD"/*.so; do so_args+=(-x "$so"); done
dylibbundler -cd -of -b \
  -x "$MACOS/moraine-gui.bin" \
  "${so_args[@]}" \
  -d "$FW" -p "@executable_path/../Frameworks/"

# ---- launcher: point GTK at the bundled data, then exec the real binary ----
cat > "$MACOS/moraine-gui" <<'SH'
#!/bin/bash
here="$(cd "$(dirname "$0")" && pwd)"; res="$here/../Resources"
export GDK_PIXBUF_MODULE_FILE="$(ls "$res"/lib/gdk-pixbuf-2.0/*/loaders.cache)"
export GSETTINGS_SCHEMA_DIR="$res/share/glib-2.0/schemas"
export XDG_DATA_DIRS="$res/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "$here/moraine-gui.bin" "$@"
SH
chmod +x "$MACOS/moraine-gui"

# ---- code sign (ad-hoc by default — arm64 will not launch unsigned) ----
sign_id="${MORAINE_CODESIGN_ID:--}"   # "-" == ad-hoc
echo "==> codesign ($([ "$sign_id" = - ] && echo ad-hoc || echo "$sign_id"))"
codesign --force --deep --options runtime --timestamp=none --sign "$sign_id" "$APP"
codesign --verify --deep --strict "$APP"

# ---- .dmg with a drag-to-Applications target ----
echo "==> hdiutil create $out"
mkdir -p dist-app
cp -R "$APP" dist-app/
ln -sf /Applications dist-app/Applications
hdiutil create -volname "Moraine $version" -srcfolder dist-app -ov -format UDZO "$out"
rm -rf dist-app

echo "==> done: $out"
