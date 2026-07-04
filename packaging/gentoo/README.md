# Gentoo packaging for Moraine

`moraine-0.1.19.ebuild` builds the CLI + GTK app via the `cargo` eclass. The
easiest place to publish is the community overlay **GURU** (low barrier); the
main tree needs a proxy maintainer (`proxy-maint`).

## Build & test

```sh
# in a local overlay (e.g. /var/db/repos/localrepo)
mkdir -p sys-apps/moraine
cp /path/to/moraine-backup/packaging/gentoo/moraine-0.1.19.ebuild sys-apps/moraine/
cd sys-apps/moraine
ebuild moraine-0.1.19.ebuild manifest   # fetches crates, writes Manifest
emerge -av sys-apps/moraine
```

## Keeping CRATES + LICENSE correct

The `CRATES` list and the full `LICENSE` string are dependency-derived. Don't
hand-maintain them — regenerate with
[pycargoebuild](https://github.com/projg2/pycargoebuild):

```sh
pycargoebuild /path/to/moraine-backup     # prints an up-to-date ebuild
```

Paste its `CRATES` and `LICENSE` into the ebuild (the `src_configure`/
`src_install` here already handle the `gui` feature, desktop entry, icons and
runtime assets).

## Publish

- **GURU**: commit `sys-apps/moraine/` to the GURU overlay (become a contributor
  first — see the GURU wiki).
- **::gentoo**: find a proxy maintainer via `proxy-maint@gentoo.org`.

## On each new release

Rename the ebuild to the new version, regenerate `CRATES`/`LICENSE` with
pycargoebuild, and re-run `ebuild … manifest`.
