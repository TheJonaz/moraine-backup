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
# rclone: the official native Windows build — a single static exe, no DLLs. It's
# not a cygwin program, so it lives next to the app (on PATH via the exe dir).
curl -fsSL -o /tmp/rclone.zip https://downloads.rclone.org/rclone-current-windows-amd64.zip
unzip -j -o /tmp/rclone.zip '*/rclone.exe' -d bundle/
# rsync + a matching cygwin ssh: MSYS2's msys/cygwin builds. These MUST sit in a
# real `usr/bin` root (not flat next to the app): cygwin derives its POSIX root
# from where msys-2.0.dll lives, and only then reads the sibling `etc/` config
# below — which is what makes `/c/…` drive paths resolve and gives ssh a valid
# HOME. rsync finds `moraine-ssh` via PATH (see tools::add_bundled_tools_to_path).
mkdir -p bundle/usr/bin
cp /usr/bin/rsync.exe bundle/usr/bin/
cp /usr/bin/msys-2.0.dll bundle/usr/bin/ 2>/dev/null || true
# `moraine-ssh`, renamed so it doesn't shadow the system ssh that Moraine's own
# ssh calls use (native Windows OpenSSH as rsync's transport garbles the command).
cp /usr/bin/ssh.exe bundle/usr/bin/moraine-ssh.exe
{ ldd /usr/bin/rsync.exe; ldd /usr/bin/ssh.exe; } | awk '{print $3}' \
  | grep -iE '^/usr/bin/' | sort -u \
  | while read -r d; do cp -n "$d" bundle/usr/bin/ || true; done

# Cygwin/msys runtime config for the bundled rsync + moraine-ssh. Read from
# <root>/etc where <root> is the parent of the usr/bin holding msys-2.0.dll:
#   fstab       — map drive letters at /c, /d, … so a source like
#                 /c/Users/you/… actually resolves (else rsync reports the
#                 opaque `change_dir "/c/…" failed`). Without a read fstab,
#                 cygdrive falls back to the /cygdrive prefix and /c/ doesn't map.
#   nsswitch    — derive HOME from the real Windows profile, so ssh writes
#                 ~/.ssh/known_hosts there instead of a nonexistent /home/<user>.
mkdir -p bundle/etc
printf 'none / cygdrive binary,posix=0,noacl 0 0\n' > bundle/etc/fstab
printf 'db_home: windows\n' > bundle/etc/nsswitch.conf

# GUI assets (logo + hero background) — asset() looks for these next to the exe.
mkdir -p bundle/assets
cp assets/moraine-64.png assets/hero-bg.png bundle/assets/

echo "=== bundle: $(du -sh bundle | cut -f1), $(ls bundle/*.dll | wc -l) DLLs; tools: $(ls bundle/rclone.exe bundle/usr/bin/rsync.exe bundle/usr/bin/moraine-ssh.exe 2>/dev/null | wc -l)/3 ==="
