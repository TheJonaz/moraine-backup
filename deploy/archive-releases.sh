#!/usr/bin/env bash
# archive-releases.sh — mirror Moraine's GitHub release packages to
# backup.thern.io, so the FULL version history survives even though the CDN only
# keeps the last 5 versions (and even if GitHub itself is lost).
#
#   deploy/archive-releases.sh            # archive every release that's missing
#   deploy/archive-releases.sh 0.2.0      # archive just this version (bump.sh uses this)
#
# Runs from the dev machine: it needs `gh` and the WireGuard VPN up, because
# backup.thern.io only accepts SSH from the VPN egress. Idempotent — a release
# whose remote dir already has files is skipped, so re-runs are cheap. Best
# effort: a failure here never blocks a release (the packages are still on
# GitHub and the CDN).
set -euo pipefail

REPO="${MORAINE_REPO:-TheJonaz/moraine-backup}"
BACKUP_SSH="${MORAINE_BACKUP_SSH:-root@5.189.191.239}"   # backup.thern.io
BACKUP_KEY="${MORAINE_BACKUP_KEY:-$HOME/sshkeys/backup.key}"
BACKUP_DIR="${MORAINE_BACKUP_DIR:-/srv/moraine-releases}"
ONLY="${1:-}"

log(){ printf 'archive: %s\n' "$*"; }
command -v gh >/dev/null || { echo "archive: gh is required" >&2; exit 1; }

SSH=(ssh -o ConnectTimeout=20 -o StrictHostKeyChecking=accept-new -i "$BACKUP_KEY")

# Reachability + ensure the archive root exists (fails clearly if the VPN is down).
if ! "${SSH[@]}" "$BACKUP_SSH" "mkdir -p '$BACKUP_DIR'" 2>/dev/null; then
    echo "archive: cannot reach $BACKUP_SSH — is the WireGuard VPN up (wg-vpn.conf)?" >&2
    exit 1
fi

if [ -n "$ONLY" ]; then
    tags="v${ONLY#v}"
else
    tags=$(gh release list --repo "$REPO" --limit 200 --json tagName -q '.[].tagName')
fi

tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
n=0
for tag in $tags; do
    # In an all-releases run, skip a release whose remote dir already has content
    # (cheap re-runs). For an explicit single version we always re-sync, so a
    # previously-partial archive (e.g. run before the build finished) is filled
    # in — rsync only transfers what's missing.
    if [ -z "$ONLY" ] && "${SSH[@]}" "$BACKUP_SSH" \
         "d='$BACKUP_DIR/$tag'; [ -d \"\$d\" ] && [ -n \"\$(ls -A \"\$d\" 2>/dev/null)\" ]" 2>/dev/null; then
        log "$tag already archived — skip"
        continue
    fi
    log "downloading $tag assets from GitHub…"
    mkdir -p "$tmp/$tag"
    if ! gh release download "$tag" --repo "$REPO" --dir "$tmp/$tag" --clobber 2>/dev/null; then
        log "WARN: no assets for $tag (draft/failed build?) — skip"
        continue
    fi
    log "syncing $tag → $BACKUP_SSH:$BACKUP_DIR/$tag/"
    rsync -a -e "ssh -o ConnectTimeout=20 -i $BACKUP_KEY" \
        "$tmp/$tag/" "$BACKUP_SSH:$BACKUP_DIR/$tag/"
    n=$((n + 1))
done
log "done — archived $n new release(s) to $BACKUP_SSH:$BACKUP_DIR"
