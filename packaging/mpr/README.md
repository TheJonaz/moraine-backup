# MPR packaging for Moraine (makedeb)

The [makedeb Package Repository (MPR)](https://mpr.makedeb.org) is "the AUR for
Debian/Ubuntu" — a `PKGBUILD` built into a `.deb` with `makedeb`. It's a great
interim channel while the official Debian package waits for a sponsor.

Once published, users install it with the MPR helper `um`:

```sh
um install moraine
```

## Build & test locally

```sh
# install makedeb first: https://docs.makedeb.org
git clone https://mpr.makedeb.org/moraine   # (empty on first publish)
cd moraine
cp /path/to/moraine-backup/packaging/mpr/PKGBUILD .
makedeb -s        # build the .deb (resolves makedepends)
sudo apt install ./moraine_*.deb
```

## Publish / update

The MPR is a git repo per package (like the AUR):

```sh
cp PKGBUILD .SRCINFO ...          # .SRCINFO via `makedeb --print-srcinfo > .SRCINFO`
git commit -am "moraine 0.1.19"
git push
```

## On each new release

Bump `pkgver`, reset `pkgrel=1`, refresh `sha256sums`
(`curl -sL <tarball-url> | sha256sum`), regenerate `.SRCINFO`.
