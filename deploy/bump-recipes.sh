#!/usr/bin/env bash
# bump-recipes.sh — bump the downstream packaging/ recipes to a released version.
#
#   deploy/bump-recipes.sh <version> [--dry-run] [--no-commit] [--no-push] [-y]
#
# Run AFTER bump.sh has pushed the tag and release.yml has published the release:
# the recipes pin checksums of the tag tarball and the Windows zip, which only
# exist once the release is up (that is why bump.sh can't do this).
#
# It:
#   * reads the current recipe version and the new checksums from the live release
#   * bumps the version across every recipe (AUR, MPR, Void, Alpine, Homebrew,
#     Scoop, Chocolatey, winget, RPM, Snap, Flatpak, Nix, nixpkgs, FreeBSD)
#   * refreshes the source sha256 + sha512 and the Windows-zip sha256
#   * renames the Gentoo ebuild and adds a release entry to the RPM %changelog
#     and the Flatpak metainfo (their history is preserved, not overwritten)
#   * commits "packaging: bump downstream recipes to vX.Y.Z" and pushes
#
# It does NOT regenerate the vendored crate lists (flatpak/cargo-sources.json,
# gentoo CRATES, freebsd CARGO_CRATES) — those only change when Cargo.lock's
# dependencies change. Regenerate them with their platform tools when that happens.
set -euo pipefail

die(){ printf 'bump-recipes: %s\n' "$*" >&2; exit 1; }

NEW=""; DRY=0; NOCOMMIT=0; PUSH=1; YES=0
for a in "$@"; do
    case "$a" in
        --dry-run)   DRY=1 ;;
        --no-commit) NOCOMMIT=1 ;;
        --no-push)   PUSH=0 ;;
        -y|--yes)    YES=1 ;;
        -*) die "unknown option: $a" ;;
        *) [ -z "$NEW" ] || die "unexpected argument: $a"; NEW="$a" ;;
    esac
done
[ -n "$NEW" ] || die "usage: deploy/bump-recipes.sh <version> [--dry-run] [--no-commit] [--no-push] [-y]"
[[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version must look like X.Y.Z (got '$NEW')"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG="${PKG_DIR:-$ROOT/packaging}"
REPO="${MORAINE_REPO:-TheJonaz/moraine-backup}"
MAINT="${MORAINE_MAINTAINER:-Jonaz Thern <info@thern.io>}"
[ -d "$PKG" ] || die "no packaging dir at $PKG"
for t in curl sha256sum sha512sum awk; do command -v "$t" >/dev/null || die "$t is required"; done

# Current recipe version (canonical: the AUR PKGBUILD).
OLD="${OLD_VERSION:-$(sed -n 's/^pkgver=\(.*\)/\1/p' "$PKG/aur/PKGBUILD" | head -1)}"
[ -n "$OLD" ] || die "could not read the current recipe version from aur/PKGBUILD"
if [ "$OLD" = "$NEW" ]; then
    echo "bump-recipes: recipes already at $NEW — nothing to do"
    exit 0
fi
echo "bump-recipes: $OLD -> $NEW"

# Old checksums to swap out (read from the recipes themselves, before any edits).
OLD_SRC256="$(sed -n "s/.*sha256sums=('\([0-9a-f]*\)').*/\1/p" "$PKG/aur/PKGBUILD" | head -1)"
OLD_SRC512="$(grep -oE '[0-9a-f]{128}' "$PKG/alpine/APKBUILD" | head -1)"
OLD_WIN256="$(sed -n 's/.*"hash": *"\([0-9a-fA-F]*\)".*/\1/p' "$PKG/scoop/moraine.json" | head -1 | tr 'A-F' 'a-f')"

# New checksums from the live release.
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
SRC_URL="https://github.com/$REPO/archive/refs/tags/v$NEW.tar.gz"
WIN_URL="https://github.com/$REPO/releases/download/v$NEW/moraine-windows-x86_64.zip"
curl -fsSL --max-time 120 "$SRC_URL" -o "$TMP/src.tgz" || die "cannot fetch $SRC_URL (is v$NEW released yet?)"
curl -fsSL --max-time 120 "$WIN_URL" -o "$TMP/win.zip" || die "cannot fetch $WIN_URL"
NEW_SRC256="$(sha256sum "$TMP/src.tgz" | cut -d' ' -f1)"
NEW_SRC512="$(sha512sum "$TMP/src.tgz" | cut -d' ' -f1)"
NEW_WIN256="$(sha256sum "$TMP/win.zip" | cut -d' ' -f1)"

DATE="$(date +%F)"
RPMDATE="$(LC_ALL=C date '+%a %b %d %Y')"
# One-line release note: first bullet of the CHANGELOG section for this version.
NOTE="$(awk -v v="$NEW" '
    index($0,"## ["v"]")==1 {s=1; next}
    s && /^## \[/ {exit}
    s && /^- / {l=$0; sub(/^- +/,"",l); gsub(/\*\*/,"",l); print l; exit}
' "$ROOT/CHANGELOG.md" 2>/dev/null || true)"
NOTE="${NOTE%%. *}."; [ "$NOTE" = "." ] && NOTE=""
[ -n "$NOTE" ] || NOTE="Maintenance release."

if [ "$DRY" = 1 ]; then
    cat <<EOF
bump-recipes DRY RUN (nothing written)
  version : $OLD -> $NEW
  src256  : $OLD_SRC256
         -> $NEW_SRC256
  src512  : ${OLD_SRC512:0:20}... -> ${NEW_SRC512:0:20}...
  win256  : $OLD_WIN256
         -> $NEW_WIN256
  note    : $NOTE
EOF
    exit 0
fi

reP="${OLD//./\\.}"   # old version, escaped for use in a regex

# 1. Version bump across recipes, skipping history + crate-list files (handled below).
while IFS= read -r f; do
    case "$f" in
        */flatpak/io.thern.moraine.metainfo.xml|*/rpm/moraine.spec|*/freebsd/Makefile|*/gentoo/*.ebuild|*/flatpak/cargo-sources.json) continue ;;
    esac
    sed -i "s/$reP/$NEW/g" "$f"
done < <(grep -rl "$reP" "$PKG" 2>/dev/null || true)

# 2. FreeBSD: only the DISTVERSION line (never the CARGO_CRATES list).
sed -i "s/\(^DISTVERSION=[[:space:]]*\).*/\1$NEW/" "$PKG/freebsd/Makefile"

# 3. RPM spec: the Version field, plus a new %changelog entry on top.
sed -i "s/^Version:\([[:space:]]*\).*/Version:\1$NEW/" "$PKG/rpm/moraine.spec"
awk -v ver="$NEW" -v d="$RPMDATE" -v m="$MAINT" -v note="$NOTE" '
    /^%changelog/ && !done { print; print "* " d " " m " - " ver "-1"; print "- " note; print ""; done=1; next }
    { print }
' "$PKG/rpm/moraine.spec" > "$PKG/rpm/.spec.tmp" && mv "$PKG/rpm/.spec.tmp" "$PKG/rpm/moraine.spec"

# 4. Flatpak metainfo: prepend a <release> entry (XML-escape the note).
nx="${NOTE//&/&amp;}"; nx="${nx//</&lt;}"; nx="${nx//>/&gt;}"
awk -v ver="$NEW" -v d="$DATE" -v note="$nx" '
    /<releases>/ && !done {
        print
        print "    <release version=\"" ver "\" date=\"" d "\">"
        print "      <description>"
        print "        <p>" note "</p>"
        print "      </description>"
        print "    </release>"
        done=1; next
    }
    { print }
' "$PKG/flatpak/io.thern.moraine.metainfo.xml" > "$PKG/flatpak/.mi.tmp" && mv "$PKG/flatpak/.mi.tmp" "$PKG/flatpak/io.thern.moraine.metainfo.xml"

# 5. Checksum swaps.
[ -n "$OLD_SRC256" ] && grep -rl "$OLD_SRC256" "$PKG" | xargs -r sed -i "s/$OLD_SRC256/$NEW_SRC256/g"
[ -n "$OLD_SRC512" ] && sed -i "s/$OLD_SRC512/$NEW_SRC512/" "$PKG/alpine/APKBUILD"
if [ -n "$OLD_WIN256" ]; then
    grep -rl "$OLD_WIN256" "$PKG" | xargs -r sed -i "s/$OLD_WIN256/$NEW_WIN256/g"
    ou="$(printf '%s' "$OLD_WIN256" | tr 'a-f' 'A-F')"; nu="$(printf '%s' "$NEW_WIN256" | tr 'a-f' 'A-F')"
    grep -rl "$ou" "$PKG" 2>/dev/null | xargs -r sed -i "s/$ou/$nu/g"
fi

# 6. Gentoo ebuild rename (version lives in the filename).
old_ebuild="$PKG/gentoo/moraine-$OLD.ebuild"
new_ebuild="$PKG/gentoo/moraine-$NEW.ebuild"
if [ -f "$old_ebuild" ]; then
    if [ "$NOCOMMIT" = 0 ] && git -C "$ROOT" ls-files --error-unmatch "$old_ebuild" >/dev/null 2>&1; then
        git -C "$ROOT" mv "$old_ebuild" "$new_ebuild"
    else
        mv "$old_ebuild" "$new_ebuild"
    fi
fi

echo "bump-recipes: recipes updated to $NEW"

if [ "$NOCOMMIT" = 1 ]; then
    echo "bump-recipes: --no-commit, leaving changes unstaged"
    exit 0
fi

git -C "$ROOT" add -A packaging/
git -C "$ROOT" commit -q -m "packaging: bump downstream recipes to v$NEW"
echo "bump-recipes: committed \"packaging: bump downstream recipes to v$NEW\""

if [ "$PUSH" = 1 ] && [ "$YES" != 1 ]; then
    printf 'bump-recipes: push to origin? [y/N] '; read -r r; case "$r" in [yY]*) ;; *) PUSH=0 ;; esac
fi
if [ "$PUSH" = 1 ]; then
    git -C "$ROOT" push origin "$(git -C "$ROOT" rev-parse --abbrev-ref HEAD)"
    echo "bump-recipes: pushed."
else
    echo "bump-recipes: not pushed (git push origin <branch> when ready)."
fi

cat <<EOF

bump-recipes: the vendored crate lists were left untouched
(flatpak/cargo-sources.json, gentoo CRATES, freebsd CARGO_CRATES). If Cargo.lock's
dependencies changed this release, regenerate them with their platform tools.
EOF
