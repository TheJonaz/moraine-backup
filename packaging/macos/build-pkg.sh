#!/usr/bin/env bash
# Build a macOS installer package (.pkg) for the Moraine CLI.
#
# macOS ships the command-line client only — the desktop app is GTK, and
# bundling GTK 4 into a .app is a separate project. So this is a plain
# "install a tool into /usr/local" package, not a drag-to-Applications .dmg:
# there is no .app to drag, and a .dmg holding a bare binary teaches the user
# nothing about where it should go.
#
# The binary is **universal** (arm64 + x86_64). GitHub's macOS runners are
# arm64-only, so the Intel half is cross-compiled — safe here because the CLI
# feature set is pure Rust plus system frameworks, both of which the SDK
# carries for either arch. Anything that linked a third-party C library would
# need a real Intel builder instead.
#
# Needs: macOS, Xcode command-line tools (pkgbuild/productbuild/lipo), rustup.
#
# Signing is optional and off by default — see README.md for what an unsigned
# package costs the user. To sign, export both of:
#     MORAINE_PKG_SIGN_ID   "Developer ID Installer: Name (TEAMID)"
#     MORAINE_CODESIGN_ID   "Developer ID Application: Name (TEAMID)"
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo"
here="$repo/packaging/macos"

[ "$(uname -s)" = Darwin ] || { echo "build-pkg.sh: macOS only" >&2; exit 1; }

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
ident=io.thern.moraine
out="moraine-$version-macos-universal.pkg"

# macOS 11 is the floor: arm64 has no earlier release, so a universal binary
# cannot honestly claim more. Set it for both slices so the two halves agree.
export MACOSX_DEPLOYMENT_TARGET=11.0

# The CLI is built without the GUI, but *with* the keyring — on macOS that is
# the native Keychain (`apple-native-keyring-store`), no extra runtime
# dependency. It lets `password = "keyring:"` work out of the box, which is the
# whole point of installing a desktop package rather than unpacking a tarball.
# The plain .tar.gz asset is still built --no-default-features; that difference
# is deliberate and documented in README.md.
features=(--no-default-features --features keyring)

echo "==> building universal binary (arm64 + x86_64)"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  rustup target add "$target" >/dev/null
  cargo build --release --locked "${features[@]}" --bin moraine --target "$target"
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
root="$work/root"

lipo -create -output "$work/moraine" \
  target/aarch64-apple-darwin/release/moraine \
  target/x86_64-apple-darwin/release/moraine
lipo -info "$work/moraine"

# Refuse to ship a binary that is not actually both architectures — a silent
# fallback to one slice would install fine and only fail on the other kind of
# Mac, which is the worst possible time to find out.
for arch in arm64 x86_64; do
  lipo -info "$work/moraine" | grep -qw "$arch" || {
    echo "build-pkg.sh: universal binary is missing $arch" >&2; exit 1; }
done

echo "==> laying out the payload"
install -d "$root/usr/local/bin" \
           "$root/usr/local/share/man/man1" \
           "$root/usr/local/share/doc/moraine"
install -m 755 "$work/moraine"        "$root/usr/local/bin/moraine"
install -m 644 man/moraine.1          "$root/usr/local/share/man/man1/moraine.1"
install -m 644 README.md CHANGELOG.md LICENSE moraine.example.toml \
                                      "$root/usr/local/share/doc/moraine/"

if [ -n "${MORAINE_CODESIGN_ID:-}" ]; then
  echo "==> code-signing the binary"
  codesign --force --options runtime --timestamp \
    --sign "$MORAINE_CODESIGN_ID" "$root/usr/local/bin/moraine"
  codesign --verify --strict "$root/usr/local/bin/moraine"
fi

echo "==> pkgbuild"
pkgbuild --root "$root" \
         --identifier "$ident" \
         --version "$version" \
         --install-location / \
         "$work/moraine-component.pkg"

echo "==> productbuild"
res="$work/resources"
install -d "$res"
cp LICENSE "$res/LICENSE.txt"
cp "$here/welcome.html" "$res/welcome.html"
# The distribution file carries the version in two places; keep the template
# single-sourced rather than maintaining a copy per release.
sed "s/@VERSION@/$version/g" "$here/distribution.xml" > "$work/distribution.xml"

sign=()
[ -n "${MORAINE_PKG_SIGN_ID:-}" ] && sign=(--sign "$MORAINE_PKG_SIGN_ID")

productbuild --distribution "$work/distribution.xml" \
             --package-path "$work" \
             --resources "$res" \
             "${sign[@]}" \
             "$out"

echo "==> verifying the package"
# Two checks, neither of which runs `installer` — that would need root and
# would modify the build machine.
#
# The payload listing is read off the *component* package: --payload-files
# takes a flat package, and --expand turns the component inside a product
# archive back into a directory, which it will not accept.
pkgutil --payload-files "$work/moraine-component.pkg" | sort | sed 's/^/    /'
pkgutil --payload-files "$work/moraine-component.pkg" \
  | grep -qx './usr/local/bin/moraine'
# And the product archive has to be a well-formed distribution.
rm -rf "$work/expanded"
pkgutil --expand "$out" "$work/expanded"
test -f "$work/expanded/Distribution"

echo "==> done: $out"
