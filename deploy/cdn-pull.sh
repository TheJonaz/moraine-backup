#!/usr/bin/env bash
# cdn-pull.sh — pull the latest Moraine release from GitHub and publish it to the
# local CDN repos. Runs ON the CDN host (cdn.thern.io), driven by a systemd timer
# (see deploy/systemd/), so NO inbound SSH from GitHub Actions is required — the
# host reaches out to GitHub, the firewall can stay fully closed.
#
#   cdn-pull.sh [version] [--force] [--dry-run]
#     (no version)  resolve and publish the latest GitHub release
#     version       publish a specific version, e.g. 0.1.23
#     --force       republish even if the state file says it is already current
#     --dry-run     resolve + print the asset URLs, download/publish nothing
#                   (safe to run anywhere — used to validate resolution)
#
# Idempotent: records the last-published version in a state file and exits early
# when GitHub's latest already matches, so the timer can fire as often as you like.
#
# Requires: curl. To actually publish it also needs cdn-publish.sh next to this
# script (or set MORAINE_CDN_PUBLISH) plus what that needs: reprepro / createrepo_c
# / repo-add.
set -euo pipefail

REPO="${MORAINE_REPO:-TheJonaz/moraine-backup}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PUBLISH="${MORAINE_CDN_PUBLISH:-$HERE/cdn-publish.sh}"
STATE="${MORAINE_CDN_STATE:-${XDG_STATE_HOME:-$HOME/.local/state}/moraine-cdn/published}"

log(){ printf '==> %s\n' "$*"; }
die(){ printf 'cdn-pull: %s\n' "$*" >&2; exit 1; }

FORCE=0; DRY=0; VERSION=""
for a in "$@"; do
    case "$a" in
        --force)   FORCE=1 ;;
        --dry-run) DRY=1 ;;
        -*) die "unknown option: $a" ;;
        *) [ -z "$VERSION" ] || die "unexpected argument: $a"; VERSION="$a" ;;
    esac
done

command -v curl >/dev/null || die "curl is required"
api(){ curl -fsSL -H "Accept: application/vnd.github+json" -H "User-Agent: moraine-cdn-pull" "$@"; }

# Resolve the target version and the release JSON (which carries the asset URLs).
if [ -n "$VERSION" ]; then
    VERSION="${VERSION#v}"
    json="$(api "https://api.github.com/repos/$REPO/releases/tags/v$VERSION")" \
        || die "no release v$VERSION on GitHub"
else
    json="$(api "https://api.github.com/repos/$REPO/releases/latest")" \
        || die "could not reach GitHub"
    VERSION="$(printf '%s' "$json" | sed -n 's/.*"tag_name":[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' | head -1)"
    [ -n "$VERSION" ] || die "could not parse the latest release tag"
fi
log "target version: $VERSION"

# Pick the three repo packages by suffix from the release's asset URLs (parsing
# the URLs avoids hardcoding the .fcNN / -1 / v-prefix naming quirks).
urls="$(printf '%s' "$json" | grep -oE '"browser_download_url":[[:space:]]*"[^"]*"' | sed -E 's/.*"(https[^"]*)"/\1/')"
pick(){ printf '%s\n' "$urls" | grep -E "$1" | head -1; }
deb_url="$(pick '_amd64\.deb$')"
rpm_url="$(pick '\.x86_64\.rpm$')"
pkg_url="$(pick '\.pkg\.tar\.zst$')"

if [ "$DRY" = 1 ]; then
    log "dry run — resolved assets for v$VERSION:"
    printf '  deb: %s\n  rpm: %s\n  pkg: %s\n' "${deb_url:-<none>}" "${rpm_url:-<none>}" "${pkg_url:-<none>}"
    exit 0
fi

# Idempotency: skip if we already published this version.
if [ "$FORCE" != 1 ] && [ -f "$STATE" ] && [ "$(cat "$STATE" 2>/dev/null)" = "$VERSION" ]; then
    log "already published $VERSION — nothing to do (use --force to republish)"
    exit 0
fi

[ -x "$PUBLISH" ] || die "cdn-publish.sh not found/executable at $PUBLISH (set MORAINE_CDN_PUBLISH)"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
get(){  # get <kind> <url>
    local kind="$1" url="$2" f
    [ -n "$url" ] || { log "no $kind asset in v$VERSION — skipping"; return; }
    f="$(basename "$url")"
    log "download $f"
    curl -fL --retry 3 --max-time 300 -H "User-Agent: moraine-cdn-pull" -o "$STAGE/$f" "$url"
}
get deb "$deb_url"
get rpm "$rpm_url"
get pkg "$pkg_url"

log "publishing via $PUBLISH"
"$PUBLISH" "$VERSION" "$STAGE"

# Record success so the next timer tick is a no-op until a new release lands.
mkdir -p "$(dirname "$STATE")" 2>/dev/null || true
printf '%s\n' "$VERSION" > "$STATE" 2>/dev/null || log "warning: could not write state file $STATE"
log "done — published moraine $VERSION to the CDN"
