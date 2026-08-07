# OpenBSD port for Moraine

A `sysutils/moraine` port building the **command-line client**.

| File | Purpose |
|------|---------|
| `Makefile` | the port |
| `crates.inc` | `MODCARGO_CRATES` — every crates.io dependency (generated) |
| `distinfo` | SHA256 (base64) + SIZE for the tarball and each crate (generated) |
| `pkg/DESCR` | long description |
| `pkg/PLIST` | packing list |

## Why no GUI

Not a shortcut, and not the Snap's reason. OpenBSD packages declare `WANTLIB` —
the exact list of shared libraries the built binaries link against — and the
tree enforces it with `make port-lib-depends-check` **on an OpenBSD machine**.
For a CLI the answer is `${MODCARGO_WANTLIB}` and nothing else, which is
correct by construction. For a GTK 4 application it is a few dozen entries with
version floors that can only be read off a real build. Writing that list from
here would mean inventing it, and a wrong `WANTLIB` is the first thing a ports
reviewer rejects.

The GTK client ships on Linux (Flatpak, AppImage, `.deb`, `.rpm`) and on
FreeBSD. Adding it here later is a `MODCARGO_FEATURES = gui` line, a `LIB_DEPENDS
= x11/gtk+4`, the generated `WANTLIB`, and four more `PLIST` entries.

## Build it

Copy the directory into a ports tree and build normally:

```sh
cp -R packaging/openbsd /usr/ports/sysutils/moraine
cd /usr/ports/sysutils/moraine
make makesum          # only if distinfo was not generated here
make
make package
make port-lib-depends-check
/usr/ports/infrastructure/bin/portcheck
```

There is no `make lint` target — `portcheck` is the linter, and it is a script
in the tree rather than a port target.

`doas pkg_add -D unsigned $(make show=PKGFILE)` installs the result.

The tree must match the running release (`7.9/ports.tar.gz` for a 7.9 machine).
A mismatched tree makes `lang/rust` newer than the installed `rust` package and
ports then compiles rustc from source — the trap the FreeBSD port hit.

## Regenerating crates.inc and distinfo

Both are produced by [`../bsd-distinfo.py`](../bsd-distinfo.py), which does off
OpenBSD what `make modcargo-gen-crates` and `make makesum` do on it:

```sh
packaging/bsd-distinfo.py --tag v0.2.2
```

It reads the crate list from the `Cargo.lock` **inside the tagged tarball**, not
the working tree, and verifies every downloaded `.crate` against the SHA-256 the
lockfile already records before computing anything else. The encoding was
checked against a live port — for `cfg-if-1.0.4` it produces the same base64
digest as `textproc/ripgrep`'s distinfo in the ports tree.

`packaging/check-crate-lists.py` compares `crates.inc` against a lockfile and is
run by the release bump, so this list cannot silently rot the way an unchecked
one would.

## Status — builds clean on OpenBSD 7.9

Verified end to end on 2026-08-06 against OpenBSD 7.9/amd64 with the matching
`7.9/ports.tar.gz` and the packaged `rust-1.94.1` (well above the 1.89 floor
`File::try_lock` needs, so nothing compiles rustc from source):

| Step | Result |
|------|--------|
| `make checksum` | `>> (SHA256) all files: OK` — 158 distfiles |
| `make build` | 28m25s, **33 crates** — proof `--no-default-features` took effect |
| `make fake` | clean; man page and example config land where `post-install` says |
| `make package` | `moraine-0.2.2.tgz` created — which is what validates `PLIST` |
| `make port-lib-depends-check` | clean, no output |
| `portcheck` | clean |
| `pkg_add` + `moraine --version` | installs, pulls `net/rsync` + `sysutils/rclone`, prints `moraine 0.2.2` |

`WANTLIB` resolves to `c pthread c++abi` through `${MODCARGO_WANTLIB}` — the
reason a CLI port is safe to write off-machine and a GTK one is not.

The variables all resolve as intended, which is the part that would have been
guesswork otherwise: `PKGNAME=moraine-0.2.2`, `DISTNAME=moraine-backup-0.2.2`
(the leading `v` stripped from the tag, matching what `distinfo` names), and
`MODCARGO_INSTALL_ARGS=--bin moraine --no-default-features`.

One environment note if you rebuild: a default 7.9 install leaves only ~1.2 GB
free on `/usr`, which is not enough for the tree plus distfiles plus the Rust
build. Put the tree on `/home` and symlink `/usr/ports` at it.

**Not submitted.** `ports@openbsd.org` is the destination, and OpenBSD expects a
porter — not the upstream author — to have tested it. Two channels have already
rejected moraine on author/AI grounds, so the submission itself is Jonaz's call,
not something to fire off automatically.
