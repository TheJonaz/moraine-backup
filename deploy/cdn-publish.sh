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
# VERIFY THESE PATHS/NAMES against the real server before enabling the workflow.
# They are inferred from the public download URLs, not read from the server.
CDN_WWW="${CDN_WWW:-/var/www/cdn.thern.io}"   # nginx web root
DEB_BASE="${DEB_BASE:-$CDN_WWW/deb}"           # reprepro base (serves /deb)
DEB_CODENAME="${DEB_CODENAME:-stable}"
RPM_DIR="${RPM_DIR:-$CDN_WWW/rpm/stable}"
ARCH_DIR="${ARCH_DIR:-$CDN_WWW/arch/x86_64}"
ARCH_DB="${ARCH_DB:-thern-cdn.db.tar.gz}"      # pacman repo db name (repo-add)
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

VERSION="${1:?usage: cdn-publish.sh <version> <staging-dir>}"
STAGE="${2:?usage: cdn-publish.sh <version> <staging-dir>}"

log() { printf '==> %s\n' "$*"; }

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
else
    log "SKIP arch (no .pkg.tar.zst in staging or repo-add missing)"
fi

log "done — published moraine $VERSION to the CDN repos"
