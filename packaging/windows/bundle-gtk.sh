#!/usr/bin/env bash
# Assemble a self-contained GTK4 runtime bundle around a freshly-built
# target/release/moraine-gui.exe. Run from the repo root inside an MSYS2 UCRT64
# shell (needs ldd, objdump, gdk-pixbuf-query-loaders, gtk4-update-icon-cache).
# Output: ./bundle/  — everything the app needs to run on a clean Windows.
set -euo pipefail

BIN=/ucrt64/bin
rm -rf bundle
mkdir -p bundle
cp target/release/moraine-gui.exe bundle/

# gdk-pixbuf image loaders (used for icons) + a relative-path cache.
PBVER=$(ls /ucrt64/lib/gdk-pixbuf-2.0)
LOADERS="bundle/lib/gdk-pixbuf-2.0/$PBVER/loaders"
mkdir -p "$LOADERS"
cp /ucrt64/lib/gdk-pixbuf-2.0/"$PBVER"/loaders/*.dll "$LOADERS/"
( cd "$LOADERS" && gdk-pixbuf-query-loaders.exe *.dll > ../loaders.cache )

# The working DLL closure of the exe + the loaders (everything under /ucrt64/bin).
{ ldd bundle/moraine-gui.exe; for l in "$LOADERS"/*.dll; do ldd "$l"; done; } \
  | awk '{print $3}' | grep -iE '^/ucrt64/bin/' | sort -u \
  | while read -r d; do cp -n "$d" bundle/ || true; done

# Drop GStreamer's DLLs when GTK doesn't hard-link it (unused media backend).
if ! objdump -p /ucrt64/bin/libgtk-4-1.dll | awk '/DLL Name:/{print $3}' | grep -qi gst; then
  rm -f bundle/libgst*.dll && echo "Trimmed GStreamer DLLs."
fi

# Compiled GSettings schemas (GTK reads these at startup).
mkdir -p bundle/share/glib-2.0/schemas
cp /ucrt64/share/glib-2.0/schemas/gschemas.compiled bundle/share/glib-2.0/schemas/ 2>/dev/null \
  || glib-compile-schemas /ucrt64/share/glib-2.0/schemas --targetdir bundle/share/glib-2.0/schemas

# Icon themes: Adwaita (symbolic icons the widgets use) + hicolor, and our own
# app icon so set_icon_name("moraine") resolves the window/taskbar icon.
mkdir -p bundle/share/icons
cp -r /ucrt64/share/icons/Adwaita bundle/share/icons/
cp -r /ucrt64/share/icons/hicolor bundle/share/icons/ 2>/dev/null || true
for s in 16 24 32 48 64 128 256; do
  mkdir -p "bundle/share/icons/hicolor/${s}x${s}/apps"
  cp "assets/moraine-${s}.png" "bundle/share/icons/hicolor/${s}x${s}/apps/moraine.png"
done
mkdir -p bundle/share/icons/hicolor/scalable/apps
cp assets/moraine.svg bundle/share/icons/hicolor/scalable/apps/moraine.svg
gtk4-update-icon-cache.exe -q -t -f bundle/share/icons/Adwaita || true
gtk4-update-icon-cache.exe -q -t -f bundle/share/icons/hicolor || true

# App icon for the installer's shortcuts.
cp assets/moraine.ico bundle/

# ── Backend tools, so backups work without a separate install ──
# rclone: the official native Windows build — a single static exe, no DLLs.
curl -fsSL -o /tmp/rclone.zip https://downloads.rclone.org/rclone-current-windows-amd64.zip
unzip -j -o /tmp/rclone.zip '*/rclone.exe' -d bundle/
# rsync: MSYS2's build (msys/cygwin) + its DLL closure. moraine rewrites local
# Windows paths to msys form (/c/…) so the drive-letter-as-remote quirk is avoided,
# and finds ssh from the system OpenSSH on PATH.
cp /usr/bin/rsync.exe bundle/
cp /usr/bin/msys-2.0.dll bundle/ 2>/dev/null || true
ldd /usr/bin/rsync.exe | awk '{print $3}' | grep -iE '^/usr/bin/' \
  | while read -r d; do cp -n "$d" bundle/ || true; done

echo "=== bundle: $(du -sh bundle | cut -f1), $(ls bundle/*.dll | wc -l) DLLs; tools: $(ls bundle/rclone.exe bundle/rsync.exe 2>/dev/null | wc -l)/2 ==="
