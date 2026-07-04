# Void Linux packaging for Moraine

`template` is the Void package recipe (CLI + GTK app), built with the `cargo`
build style. Void's package tree (`void-packages`) is a separate git repo — the
template here is the source of truth you copy in and submit as a PR.

## Build & test

```sh
git clone --depth=1 https://github.com/void-linux/void-packages
cd void-packages
./xbps-src binary-bootstrap
mkdir -p srcpkgs/moraine
cp /path/to/moraine-backup/packaging/void/template srcpkgs/moraine/template
./xbps-src pkg moraine
sudo xbps-install -R hostdir/binpkgs moraine
```

## Submit

Fork `void-linux/void-packages`, add `srcpkgs/moraine/template`, and open a PR.
Void reviews in-repo; no separate sponsor step, but a maintainer must approve.

## On each new release

Bump `version`, reset `revision=1`, and refresh `checksum` (sha256 of the release
source tarball): `curl -sL <tarball-url> | sha256sum`.
