# AUR packaging for Moraine

This directory holds the Arch Linux `PKGBUILD` (and generated `.SRCINFO`) for the
AUR package **`moraine`**. The AUR is a *separate* git repo from this one — the
files here are the source of truth that you copy into the AUR repo to publish.

## One-time setup

1. Create an account at https://aur.archlinux.org and add your SSH public key
   under *My Account → SSH Public Key*.
2. Make sure git can reach the AUR:
   `ssh aur@aur.archlinux.org help` should print a help message.

## Publish / update

From an Arch (or Arch-container) machine with `base-devel`:

```sh
# 1. Clone the (empty, on first publish) AUR repo
git clone ssh://aur@aur.archlinux.org/moraine.git aur-moraine
cd aur-moraine

# 2. Copy the packaging files from this repo
cp /path/to/moraine-backup/packaging/aur/PKGBUILD .

# 3. Pin the release tarball checksum and regenerate .SRCINFO
updpkgsums                 # fills sha256sums from the real tarball
makepkg --printsrcinfo > .SRCINFO

# 4. Test it actually builds and installs cleanly
makepkg -si

# 5. Commit and push to the AUR
git add PKGBUILD .SRCINFO
git commit -m "moraine 0.1.17"
git push
```

## On each new release

1. Tag the release in this repo: `git tag vX.Y.Z && git push origin vX.Y.Z`
   (the `PKGBUILD` builds from `…/archive/refs/tags/vX.Y.Z.tar.gz`).
2. Bump `pkgver` in `PKGBUILD`, reset `pkgrel=1`.
3. Re-run steps 3–5 above (`updpkgsums`, `makepkg --printsrcinfo`, test, push).

`updpkgsums` and `makepkg --printsrcinfo` require Arch tooling, so the `.SRCINFO`
and final `sha256sums` are produced on an Arch machine at publish time.
