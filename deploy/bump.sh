#!/usr/bin/env bash
# bump.sh — cut a new Moraine release in one command.
#
#   deploy/bump.sh <version> [--no-push] [--dry-run] [-y|--yes]
#   e.g.  deploy/bump.sh 0.1.23
#
# Bumps the version everywhere the CDN and the website read it, commits, tags and
# pushes. Pushing the tag triggers .github/workflows/release.yml, whose `cdn` job
# republishes the apt/dnf/pacman repos on cdn.thern.io — so the CDN tracks the new
# version automatically, no manual publish step.
#
# What it rewrites:
#   * Cargo.toml + Cargo.lock  — the source-of-truth version
#   * CHANGELOG.md             — stamps the [Unreleased] heading with version + date
#   * README.md                — the install-command version strings
#   * site/index.html          — every version label and the cdn.thern.io download
#                                URLs. site/ is gitignored and deployed separately,
#                                so it is edited in place; the script reminds you to
#                                publish it (moraine.thern.io) after the tag's
#                                packages have landed on the CDN.
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
for a in "$@"; do
    case "$a" in
        --no-push) PUSH=0 ;;
        --dry-run) DRYRUN=1 ;;
        -y|--yes)  ASSUME_YES=1 ;;
        -*) die "unknown option: $a" ;;
        *) [ -z "$NEW" ] || die "unexpected argument: $a"; NEW="$a" ;;
    esac
done

[ -n "$NEW" ] || die "usage: deploy/bump.sh <version> [--no-push] [--dry-run] [-y]"
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

cat <<EOF

bump: two follow-ups the script deliberately leaves to you —
  1. site/index.html is updated but gitignored. Deploy it to moraine.thern.io
     once release.yml has published v$N to the CDN (else the download links 404).
  2. downstream packaging recipes (AUR / Homebrew / nixpkgs / …) still need a
     checksum refresh against the new tag tarball.
EOF
