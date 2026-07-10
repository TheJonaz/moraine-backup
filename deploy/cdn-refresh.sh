#!/usr/bin/env bash
# cdn-refresh.sh — publish a release to the CDN NOW, instead of waiting for the
# host's ≤10-min timer. Run from the dev machine after `deploy/bump.sh` pushes.
#
#   deploy/cdn-refresh.sh <version> [--no-wait] [--dry-run]
#     <version>   the release just pushed, e.g. 0.2.0
#     --no-wait   don't wait for the GitHub build; trigger immediately
#     --dry-run   do the wait, then print the trigger command without running it
#
# Why a trigger at all: the CDN (cdn.thern.io) is PULL-based — its firewall drops
# inbound from GitHub Actions — so nothing can push to it. Its own systemd timer
# (moraine-cdn-pull.timer) polls GitHub every 10 min. This script just makes that
# pull happen right after a release instead of up to 10 min later: it waits for
# release.yml to attach the Linux packages, then reaches the host over the VPN
# (ssh alias `cdn`, see ~/.ssh/config) and runs the pull service once.
#
# Idempotent: cdn-pull.sh on the host records the last-published version and exits
# early when it is already current, so re-running (or the timer also firing) is a
# no-op. A failure here is non-fatal — the timer still catches the release.
set -euo pipefail

VER="${1:?usage: cdn-refresh.sh <version> [--no-wait] [--dry-run]}"; shift || true
VER="${VER#v}"; TAG="v$VER"
NOWAIT=0; DRY=0
for a in "$@"; do
    case "$a" in
        --no-wait) NOWAIT=1 ;;
        --dry-run) DRY=1 ;;
        *) printf 'cdn-refresh: unknown option: %s\n' "$a" >&2; exit 2 ;;
    esac
done

REPO="${MORAINE_REPO:-TheJonaz/moraine-backup}"
SSH_CDN="${MORAINE_CDN_SSH:-cdn}"                 # ssh alias (root@10.10.0.3, ProxyJump thern-vpn)
WAIT_TRIES="${MORAINE_CDN_WAIT_TRIES:-30}"        # 30 × 30s = up to ~15 min
log(){ printf 'cdn-refresh: %s\n' "$*"; }

# The Linux packages the CDN republishes, plus the checksum file the updater needs.
NEED=(
    "moraine_v${VER}_amd64.deb"
    "moraine-${VER}-1.fc44.x86_64.rpm"
    "moraine-${VER}-1-x86_64.pkg.tar.zst"
    "SHA256SUMS"
)

# 1. Wait for the release build to attach the Linux packages (best-effort).
if [ "$NOWAIT" = 0 ] && command -v gh >/dev/null 2>&1; then
    log "waiting for $TAG packages to build on GitHub (up to ~15 min; --no-wait to skip)…"
    ok=0
    for _ in $(seq 1 "$WAIT_TRIES"); do
        have=" $(gh release view "$TAG" --repo "$REPO" --json assets \
                    -q '[.assets[].name]|join(" ")' 2>/dev/null) "
        miss=0
        for a in "${NEED[@]}"; do case "$have" in *" $a "*) ;; *) miss=1 ;; esac; done
        if [ "$miss" = 0 ]; then ok=1; break; fi
        sleep 30
    done
    if [ "$ok" = 1 ]; then
        log "all packages present on $TAG."
    else
        log "timed out waiting — triggering anyway (the host will pull whatever is ready)."
    fi
else
    log "not waiting for the build (--no-wait, or gh not installed)."
fi

# 2. Trigger the CDN host to pull + publish now.
TRIGGER='systemctl start moraine-cdn-pull.service'
if [ "$DRY" = 1 ]; then
    log "dry-run — would run:  ssh $SSH_CDN '$TRIGGER'"
    exit 0
fi
log "triggering a publish on '$SSH_CDN'…"
if ssh -o ConnectTimeout=25 "$SSH_CDN" "$TRIGGER"; then
    sleep 8
    got=$(curl -fsS -4 --max-time 15 \
            https://cdn.thern.io/deb/dists/stable/main/binary-amd64/Packages 2>/dev/null \
            | awk -F': ' '/^Version:/{print $2; exit}')
    log "done — CDN deb repo now serves ${got:-<unknown>} (wanted ${VER}-1)."
else
    log "could not reach '$SSH_CDN' — the CDN will still update within ~10 min via its timer." >&2
    exit 1
fi
