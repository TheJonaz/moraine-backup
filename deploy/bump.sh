#!/usr/bin/env bash
# bump.sh — cut a new Moraine release in one command.
#
#   deploy/bump.sh <version> [--no-push] [--no-site] [--no-cdn] [--dry-run] [-y]
#   e.g.  deploy/bump.sh 0.1.23
#
# Bumps the version everywhere the CDN and the website read it, commits, tags and
# pushes. Pushing the tag triggers .github/workflows/release.yml, which builds and
# uploads the OS packages to the GitHub release. The CDN (cdn.thern.io) is
# PULL-based — its firewall blocks inbound from GitHub Actions — so a systemd
# timer on the host polls GitHub every ~10 min and republishes. To avoid that
# wait, this script also triggers the pull directly once the build finishes (see
# the CDN step below / deploy/cdn-refresh.sh; skip with --no-cdn).
#
# What it rewrites:
#   * Cargo.toml + Cargo.lock  — the source-of-truth version
#   * CHANGELOG.md             — stamps the [Unreleased] heading with version + date
#   * README.md                — the install-command version strings
#   * site/index.html          — every version label and the cdn.thern.io download
#                                URLs. site/ is gitignored; after pushing, the
#                                script deploys index.html + the GUI screenshots
#                                to moraine.thern.io over SSH (skip with --no-site).
#
# NOT touched: the downstream packaging/ recipes (AUR, Homebrew, nixpkgs, …). Their
# source/binary checksums can only be computed once the tag's tarball exists on
# GitHub, so refresh those separately after this script pushes the tag.
set -euo pipefail

die() { printf 'bump: %s\n' "$*" >&2; exit 1; }

NEW=""
PUSH=1
DRYRUN=0
ASSUME_YES=0
DEPLOY_SITE=1
REFRESH_CDN=1
ARCHIVE_BACKUP=1
for a in "$@"; do
    case "$a" in
        --no-push)    PUSH=0 ;;
        --dry-run)    DRYRUN=1 ;;
        --no-site)    DEPLOY_SITE=0 ;;
        --no-cdn)     REFRESH_CDN=0 ;;
        --no-archive) ARCHIVE_BACKUP=0 ;;
        -y|--yes)     ASSUME_YES=1 ;;
        -*) die "unknown option: $a" ;;
        *) [ -z "$NEW" ] || die "unexpected argument: $a"; NEW="$a" ;;
    esac
done

[ -n "$NEW" ] || die "usage: deploy/bump.sh <version> [--no-push] [--no-site] [--no-cdn] [--no-archive] [--dry-run] [-y]"
[[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version must look like X.Y.Z (got '$NEW')"

# Always operate from the repo root, wherever we were invoked from.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
grep -q '^name = "moraine"' Cargo.toml 2>/dev/null || die "not in the moraine repo root"

OLD="$(sed -n 's/^version = "\([0-9][0-9.]*\)"/\1/p' Cargo.toml | head -1)"
[ -n "$OLD" ] || die "could not read the current version from Cargo.toml"
[ "$OLD" != "$NEW" ] || die "version is already $NEW"

DATE="$(date +%F)"
printf 'bump: %s -> %s (%s)\n' "$OLD" "$NEW" "$DATE"

# ── rewrite helper: in place, or a unified diff under --dry-run ──
edit() {  # edit <file> <perl-expr>
    local f="$1" expr="$2"
    [ -f "$f" ] || { printf 'bump: skip %s (missing)\n' "$f" >&2; return; }
    if [ "$DRYRUN" = 1 ]; then
        perl -0777 -pe "$expr" "$f" | diff -u --label "$f" --label "$f (bumped)" "$f" - || true
    else
        perl -0777 -i -pe "$expr" "$f"
    fi
}

N="$NEW"

# ── source of truth ──
edit Cargo.toml 's/^version = "\d+\.\d+\.\d+"/version = "'"$N"'"/m'
edit Cargo.lock 's/(name = "moraine"\nversion = ")\d+\.\d+\.\d+/${1}'"$N"'/'

# ── changelog: promote the Unreleased heading to this version ──
if grep -q '## \[Unreleased\]' CHANGELOG.md; then
    edit CHANGELOG.md 's/## \[Unreleased\]/## ['"$N"'] — '"$DATE"'/'
else
    printf 'bump: note — no [Unreleased] section in CHANGELOG.md to stamp\n' >&2
fi

# ── README install commands ──
edit README.md '
    s{moraine_\d+\.\d+\.\d+-1_amd64\.deb}{moraine_'"$N"'-1_amd64.deb}g;
    s{download/v\d+\.\d+\.\d+/moraine-\d+\.\d+\.\d+-1-x86_64\.pkg\.tar\.zst}{download/v'"$N"'/moraine-'"$N"'-1-x86_64.pkg.tar.zst}g;
    s{moraine-backup/v\d+\.\d+\.\d+/packaging}{moraine-backup/v'"$N"'/packaging}g;
'

# ── website (gitignored — edited in place for a separate deploy) ──
edit site/index.html '
    s{(style\.css\?v=)\d+\.\d+\.\d+(-\d+)?}{${1}'"$N"'}g;
    s{(app\.js\?v=)\d+\.\d+\.\d+(-\d+)?}{${1}'"$N"'}g;
    s{Moraine&nbsp;\d+\.\d+\.\d+}{Moraine&nbsp;'"$N"'}g;
    s{Download Moraine \d+\.\d+\.\d+}{Download Moraine '"$N"'}g;
    s{· v\d+\.\d+\.\d+}{· v'"$N"'}g;
    s{moraine v\d+\.\d+\.\d+}{moraine v'"$N"'}g;
    s{moraine_\d+\.\d+\.\d+-1_amd64\.deb}{moraine_'"$N"'-1_amd64.deb}g;
    s{moraine-\d+\.\d+\.\d+-1-x86_64\.pkg\.tar\.zst}{moraine-'"$N"'-1-x86_64.pkg.tar.zst}g;
    s{moraine-\d+\.\d+\.\d+-1\.fc44\.x86_64\.rpm}{moraine-'"$N"'-1.fc44.x86_64.rpm}g;
    s{archive/refs/tags/v\d+\.\d+\.\d+\.tar\.gz}{archive/refs/tags/v'"$N"'.tar.gz}g;
    s{("softwareVersion": ")\d+\.\d+\.\d+}{${1}'"$N"'}g;
'

if [ "$DRYRUN" = 1 ]; then
    printf '\nbump: dry run — nothing was changed, committed or pushed.\n'
    exit 0
fi

# Guard against a silent regex miss before we commit a "release" that didn't bump.
grep -q "^version = \"$N\"$" Cargo.toml || die "Cargo.toml did not update to $N — aborting, tree left edited"

# site/ is gitignored, so git add -A picks up only the tracked files below.
git add -A
git commit -q -m "release: v$N"
git tag -a "v$N" -m "Moraine v$N"
printf 'bump: committed release: v%s and tagged v%s\n' "$N" "$N"

branch="$(git rev-parse --abbrev-ref HEAD)"

if [ "$PUSH" = 1 ]; then
    if [ "$ASSUME_YES" != 1 ]; then
        printf 'bump: push %s and tag v%s to origin? [y/N] ' "$branch" "$N"
        read -r reply
        case "$reply" in [yY]*) ;; *) PUSH=0 ;; esac
    fi
fi

if [ "$PUSH" = 1 ]; then
    git push origin "$branch"
    git push origin "v$N"
    printf 'bump: pushed — release.yml will build and publish v%s to cdn.thern.io.\n' "$N"
else
    printf 'bump: not pushed. When ready:  git push origin %s && git push origin v%s\n' "$branch" "$N"
fi

# ── Deploy the website (moraine.thern.io) ──
# The site is gitignored and lives on web.thern.io; bump.sh runs on your machine,
# which can reach it, so we push the updated index.html straight up (only
# index.html changes on a version bump). notroot can't write the moraine-owned
# web root directly but has passwordless sudo, so we stage in /tmp and
# `sudo install`. Override host/path/owner/key via env; skip with --no-site.
# ssh will prompt once for the key's passphrase (or use your agent).
SITE_HOST="${MORAINE_SITE_HOST:-notroot@web.thern.io}"
SITE_PATH="${MORAINE_SITE_PATH:-/home/moraine/public_html/index.html}"
SITE_OWNER="${MORAINE_SITE_OWNER:-moraine:moraine}"
SITE_KEY="${MORAINE_SITE_KEY:-$HOME/sshkeys/node1.key}"

# Staged deploy of one local site file to the moraine-owned web root: scp to
# /tmp, then `sudo install` into place (notroot can't write the web root directly
# but has passwordless sudo). Returns non-zero on failure. `ssh_opts` must be set.
deploy_site_file() {  # deploy_site_file <local-path> <remote-dest-path>
    local src="$1" dst="$2" tmp="/tmp/moraine-deploy-${2##*/}"
    scp "${ssh_opts[@]}" "$src" "$SITE_HOST:$tmp" \
        && ssh "${ssh_opts[@]}" "$SITE_HOST" \
            "sudo install -o '${SITE_OWNER%:*}' -g '${SITE_OWNER#*:}' -m 664 '$tmp' '$dst' && rm -f '$tmp'"
}

if [ "$DEPLOY_SITE" = 1 ] && [ "$PUSH" = 1 ]; then
    if [ ! -f "$ROOT/site/index.html" ]; then
        printf 'bump: no site/index.html — skipping website deploy\n' >&2
    else
        ssh_opts=(-4 -o StrictHostKeyChecking=accept-new)
        [ -f "$SITE_KEY" ] && ssh_opts+=(-i "$SITE_KEY" -o IdentitiesOnly=yes)
        SITE_DIR="${SITE_PATH%/*}"
        printf 'bump: deploying site/index.html to %s ...\n' "$SITE_HOST"
        if deploy_site_file "$ROOT/site/index.html" "$SITE_PATH"; then
            printf 'bump: site deployed — https://moraine.thern.io now shows v%s\n' "$N"
            printf 'bump: (its versioned CDN download links resolve once cdn-pull publishes v%s, ~10 min)\n' "$N"
        else
            printf 'bump: WARNING — site deploy failed; deploy site/index.html to %s manually\n' "$SITE_HOST" >&2
        fi
        # Ship the static site assets index.html references (the GUI screenshots),
        # so a screenshot refresh actually reaches the live site — bump.sh used to
        # deploy only index.html, leaving updated images stranded locally. Only
        # files present in site/ are pushed; unchanged ones simply overwrite with
        # the same bytes.
        for asset in app-hero.png app-hero.webp; do
            [ -f "$ROOT/site/$asset" ] || continue
            if deploy_site_file "$ROOT/site/$asset" "$SITE_DIR/$asset"; then
                printf 'bump: deployed site asset %s\n' "$asset"
            else
                printf 'bump: WARNING — failed to deploy %s; upload it to %s manually\n' "$asset" "$SITE_DIR" >&2
            fi
        done
    fi
elif [ "$DEPLOY_SITE" != 1 ]; then
    printf 'bump: --no-site — website not deployed (site/ was updated locally)\n'
fi

# ── Refresh the CDN (cdn.thern.io) ──
# The CDN is pull-based, so nothing can push to it; its systemd timer would
# publish this release within ~10 min. Trigger it NOW instead: cdn-refresh.sh
# waits for release.yml to attach the Linux packages, then reaches the host over
# the VPN (ssh alias `cdn`) and runs the pull once. Foreground, so its key
# passphrase prompt works and you see the result. Skip with --no-cdn (the timer
# still catches it). A failure is non-fatal — the timer remains the backstop.
if [ "$REFRESH_CDN" = 1 ] && [ "$PUSH" = 1 ] && [ -x "$ROOT/deploy/cdn-refresh.sh" ]; then
    printf 'bump: refreshing cdn.thern.io (waits for the release build, then publishes)…\n'
    "$ROOT/deploy/cdn-refresh.sh" "$N" \
        || printf 'bump: note — CDN not refreshed now; its timer publishes v%s within ~10 min\n' "$N" >&2
elif [ "$REFRESH_CDN" != 1 ]; then
    printf 'bump: --no-cdn — CDN not triggered (its timer publishes v%s within ~10 min)\n' "$N"
fi

# ── Archive this release to backup.thern.io ──
# The CDN keeps only the last 5 versions; backup.thern.io keeps them ALL. Runs
# after the CDN step (so the build has finished and the assets exist). Needs the
# WireGuard VPN up; best-effort — the packages are on GitHub regardless. Skip
# with --no-archive.
if [ "$ARCHIVE_BACKUP" = 1 ] && [ "$PUSH" = 1 ] && [ -x "$ROOT/deploy/archive-releases.sh" ]; then
    printf 'bump: archiving v%s to backup.thern.io…\n' "$N"
    "$ROOT/deploy/archive-releases.sh" "$N" \
        || printf 'bump: note — backup archive skipped (VPN down?); run deploy/archive-releases.sh %s later\n' "$N" >&2
elif [ "$ARCHIVE_BACKUP" != 1 ]; then
    printf 'bump: --no-archive — release not mirrored to backup.thern.io\n'
fi

cat <<EOF

bump: done. The rest is automatic —
  * release.yml builds the packages, then its 'recipes' job bumps the downstream
    packaging recipes (AUR / Homebrew / nixpkgs / …) and commits them to main.
  * the CDN was triggered above (keeps the last 5 versions; timer is the backup).
  * this release was archived to backup.thern.io (keeps ALL versions).
  (Manual fallbacks:  deploy/cdn-refresh.sh $N   ·   deploy/archive-releases.sh $N
                      deploy/bump-recipes.sh $N)
EOF
