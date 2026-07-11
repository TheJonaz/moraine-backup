#!/usr/bin/env bash
# cdn-publish.sh — publish Moraine packages into the cdn.thern.io repos.
#
# Runs ON the CDN host (notroot@cdn.thern.io). cdn-pull.sh (a systemd timer)
# downloads the release packages into a staging dir and then invokes this script:
#
#     cdn-publish.sh <version> <staging-dir>
#
# e.g.  cdn-publish.sh 0.1.22 /tmp/cdn-incoming
#
# It refreshes each repo's metadata so apt/dnf/pacman clients see the new
# version, mirroring the layout already served at:
#   https://cdn.thern.io/deb/pool/main/m/moraine/moraine_<ver>-1_amd64.deb
#   https://cdn.thern.io/rpm/stable/moraine-<ver>-1.fc44.x86_64.rpm
#   https://cdn.thern.io/arch/x86_64/moraine-<ver>-1-x86_64.pkg.tar.zst
#
# ─────────────────────────────────────────────────────────────────────────────
# Paths verified against the live server (cdn.thern.io). The nginx web root is
# /srv/cdn; the systemd service also pins CDN_WWW via an Environment= override so
# a future redeploy of this script can't silently point publishing elsewhere.
CDN_WWW="${CDN_WWW:-/srv/cdn}"                 # nginx web root
DEB_BASE="${DEB_BASE:-$CDN_WWW/deb}"           # reprepro base (serves /deb)
DEB_CODENAME="${DEB_CODENAME:-stable}"
RPM_DIR="${RPM_DIR:-$CDN_WWW/rpm/stable}"
ARCH_DIR="${ARCH_DIR:-$CDN_WWW/arch/x86_64}"
ARCH_DB="${ARCH_DB:-thern-cdn.db.tar.gz}"      # pacman repo db name (repo-add)
FILES_BASE="${FILES_BASE:-$CDN_WWW/files}"     # version-less "latest" downloads
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

VERSION="${1:?usage: cdn-publish.sh <version> <staging-dir>}"
STAGE="${2:?usage: cdn-publish.sh <version> <staging-dir>}"

log() { printf '==> %s\n' "$*"; }

# Keep the newest $KEEP_VERSIONS moraine versions in a repo dir; delete files for
# older versions. The version is the token right after "moraine-" in the file
# name (moraine-<ver>-<rel>… for both rpm and arch). rpm: createrepo then indexes
# the kept versions so dnf can roll back; arch: the db points at the latest but
# the kept files stay downloadable for `pacman -U`. deb is left to reprepro
# (latest only). Every released version is also archived to backup.thern.io.
KEEP_VERSIONS="${KEEP_VERSIONS:-5}"
prune_versions() {  # prune_versions <dir>
    local dir="$1" old v
    old=$(ls "$dir"/moraine-*[0-9]* 2>/dev/null \
            | sed -n 's#.*/moraine-\([0-9][0-9.]*\)-.*#\1#p' \
            | sort -Vru | tail -n +"$((KEEP_VERSIONS + 1))")
    for v in $old; do
        log "prune $(basename "$dir"): drop moraine $v (keeping newest $KEEP_VERSIONS)"
        find "$dir" -maxdepth 1 -type f -name "moraine-$v-*" -delete
    done
}

# ── Debian: reprepro owns pool/ + dists/ (signed) under $DEB_BASE ──
deb=$(ls "$STAGE"/moraine_*"${VERSION}"*_amd64.deb 2>/dev/null | head -1 || true)
if [ -n "$deb" ] && command -v reprepro >/dev/null; then
    log "reprepro includedeb $DEB_CODENAME $(basename "$deb")"
    # remove any existing build of this version first so re-runs are idempotent
    reprepro -b "$DEB_BASE" remove "$DEB_CODENAME" moraine >/dev/null 2>&1 || true
    reprepro -b "$DEB_BASE" includedeb "$DEB_CODENAME" "$deb"
else
    log "SKIP deb (no .deb in staging or reprepro missing)"
fi

# ── Fedora/RPM: drop the rpm in place and rebuild the repodata ──
rpm=$(ls "$STAGE"/moraine-"${VERSION}"-*.x86_64.rpm 2>/dev/null | head -1 || true)
if [ -n "$rpm" ] && command -v createrepo_c >/dev/null; then
    log "createrepo_c $RPM_DIR"
    mkdir -p "$RPM_DIR"
    cp -f "$rpm" "$RPM_DIR/"
    prune_versions "$RPM_DIR"          # keep the newest $KEEP_VERSIONS for rollback
    createrepo_c --update "$RPM_DIR"
else
    log "SKIP rpm (no .rpm in staging or createrepo_c missing)"
fi

# ── Arch: drop the package and update the repo db ──
pkg=$(ls "$STAGE"/moraine-"${VERSION}"-*-x86_64.pkg.tar.zst 2>/dev/null | head -1 || true)
if [ -n "$pkg" ] && command -v repo-add >/dev/null; then
    log "repo-add $ARCH_DB $(basename "$pkg")"
    mkdir -p "$ARCH_DIR"
    cp -f "$pkg" "$ARCH_DIR/"
    repo-add "$ARCH_DIR/$ARCH_DB" "$ARCH_DIR/$(basename "$pkg")"
    prune_versions "$ARCH_DIR"         # keep the newest $KEEP_VERSIONS package files
else
    log "SKIP arch (no .pkg.tar.zst in staging or repo-add missing)"
fi

# ── Static "latest" downloads the website links to (files/{linux,macos,windows}) ──
# Version-less names so moraine.thern.io's download buttons always point here.
put_latest() {  # put_latest <staged-basename> <subdir>
    local src="$STAGE/$1"
    if [ -f "$src" ]; then
        mkdir -p "$FILES_BASE/$2"
        cp -f "$src" "$FILES_BASE/$2/$1"
        log "files/$2/$1 updated"
    else
        log "SKIP files/$2/$1 (not in staging)"
    fi
}
put_latest moraine-linux-x86_64.tar.gz linux
put_latest moraine-macos-arm64.tar.gz macos
put_latest moraine-windows-x86_64.zip windows

# The Windows installers are versioned; publish them under version-less names so
# the website's download buttons are stable. moraine-[0-9]… is the CLI installer;
# moraine-gui-… is the desktop-app installer.
cli_exe=$(ls "$STAGE"/moraine-[0-9]*-setup.exe 2>/dev/null | head -1 || true)
if [ -n "$cli_exe" ]; then
    mkdir -p "$FILES_BASE/windows"
    cp -f "$cli_exe" "$FILES_BASE/windows/moraine-setup.exe"
    log "files/windows/moraine-setup.exe <- $(basename "$cli_exe")"
fi
gui_exe=$(ls "$STAGE"/moraine-gui-*-setup.exe 2>/dev/null | head -1 || true)
if [ -n "$gui_exe" ]; then
    mkdir -p "$FILES_BASE/windows"
    cp -f "$gui_exe" "$FILES_BASE/windows/moraine-gui-setup.exe"
    log "files/windows/moraine-gui-setup.exe <- $(basename "$gui_exe")"
fi

# ── Regenerate the human-facing storefront pages (manifest + /app/<slug>.html) ──
# The repos above are what apt/dnf/pacman read; the shareable download pages at
# https://cdn.thern.io/app/<slug> are generated separately by cdn-reindex, which
# scans /srv/cdn. Run it here so every publish refreshes them — otherwise the
# storefront freezes on an old version AND its direct-download links 404 once
# reprepro purges the previous .deb from the pool. Non-fatal: the repos are
# already updated; a stale storefront is cosmetic, not a failed publish.
# Absolute path (not just PATH) so a minimal systemd service environment still
# finds it; override with MORAINE_CDN_REINDEX.
REINDEX="${MORAINE_CDN_REINDEX:-/usr/local/bin/cdn-reindex}"
if [ -x "$REINDEX" ]; then
    log "$(basename "$REINDEX") (refresh storefront pages)"
    "$REINDEX" || log "WARN: storefront reindex failed — /app pages may be stale"
else
    log "SKIP storefront reindex ($REINDEX not found)"
fi

log "done — published moraine $VERSION to the CDN repos"
