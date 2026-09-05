# NetBSD / pkgsrc package for Moraine

A `sysutils/moraine` pkgsrc package building the **command-line client**.
pkgsrc runs on far more than NetBSD — the same recipe serves SmartOS, Illumos,
and pkgsrc-on-macOS/Linux installations.

| File | Purpose |
|------|---------|
| `Makefile` | the package |
| `cargo-depends.mk` | `CARGO_CRATE_DEPENDS` — every crates.io dependency (generated) |
| `distinfo` | BLAKE2s + SHA512 + Size for the tarball and each crate (generated) |
| `DESCR` | long description |
| `PLIST` | packing list |

## Why no GUI

Consistency with the [OpenBSD port](../openbsd/), where the GUI is blocked by
`WANTLIB` needing a real build to generate. pkgsrc has no such obstacle — it
would be `.include "../../x11/gtk4/buildlink3.mk"` and `CARGO_FEATURES+= gui` —
so this is the cheaper of two reversible choices, not a limitation. The GTK
client ships on Linux and FreeBSD.

## Build it

```sh
cp -R packaging/netbsd /usr/pkgsrc/sysutils/moraine
cd /usr/pkgsrc/sysutils/moraine
make makesum          # only if distinfo was not generated here
make package
pkglint -Wall
```

On NetBSD the base `make` **is** bmake — there is no `bmake` binary, and
reaching for one is the first thing that fails on a fresh machine. `make
install` installs it. Note that the first line of `Makefile`, `PLIST` and
`distinfo` is a bare `$NetBSD$` — CVS expands it when the package is committed,
so leave it alone.

**Match the pkgsrc branch to the binary packages, or you will build Rust from
source.** `cargo.mk` pulls in `lang/rust` as a tool dependency, and pkgsrc
rebuilds it whenever the tree wants a newer one than is installed. The binary
repo for NetBSD 10.1 ships rust 1.96.0, which is `pkgsrc-2026Q2`;
`pkgsrc-current` was already on 1.96.1 and would have triggered a multi-hour
rustc build for a one-digit difference. Fetch the quarterly that matches:

```sh
ftp -o /pkgsrc.tar.gz https://cdn.netbsd.org/pub/pkgsrc/pkgsrc-2026Q2/pkgsrc.tar.gz
tar xzf /pkgsrc.tar.gz -C /usr
PKG_PATH=https://cdn.NetBSD.org/pub/pkgsrc/packages/NetBSD/amd64/10.1/All \
  pkg_add rust rsync rclone      # /usr/sbin/pkg_add — not on root's PATH
```

This is the same trap the FreeBSD port hit with `quarterly` vs ports HEAD.

## Regenerating cargo-depends.mk and distinfo

```sh
packaging/bsd-distinfo.py --tag v0.3.0
```

The generator does off NetBSD what `make makesum` does on it. It reads the crate
list from the `Cargo.lock` **inside the tagged tarball** rather than the working
tree, and verifies each downloaded `.crate` against the SHA-256 the lockfile
already records before computing BLAKE2s and SHA512. The output was checked
against a live package: for `cfg-if-1.0.4` it produces byte-identical BLAKE2s,
SHA512 and Size to `textproc/ripgrep`'s distinfo in pkgsrc.

`packaging/check-crate-lists.py` compares `cargo-depends.mk` against a lockfile
and is run by the release bump, so the list cannot silently rot.

## Status — builds clean on NetBSD 10.1

Verified end to end on 2026-08-06 against NetBSD 10.1/amd64 with
`pkgsrc-2026Q2` and the binary `rust-1.96.0`:

| Step | Result |
|------|--------|
| `make checksum` | 158/158 files, BLAKE2s **and** SHA512 |
| `make build` | 19m02s |
| `make install` | `.crates.toml`/`.crates2.json` are removed by `cargo.mk`, so `PLIST` need not list them |
| `make package` | `moraine-0.3.0.tgz` — the `file-check` step is what validates `PLIST` |
| `pkglint -Wall` | *Looks fine.* |
| `pkg_add` + run | installs, prints `moraine 0.3.0`, `man moraine` resolves |

`DISTNAME`, `PKGNAME` and `WRKSRC` all resolve as intended — `WRKSRC` lands on
`moraine-backup-0.3.0`, the repository name rather than `DISTNAME`, which is the
one thing the explicit `WRKSRC=` line exists to get right.

Two traps worth keeping in mind:

- **`pkgtools/digest` must be installed**, and pkgsrc does not say so. Without
  it every checksum "fails" — it reports `Checksum BLAKE2s mismatch` on the
  first distfile, which reads exactly like a stale `distinfo` and is not.
- **`pkglint` rejects non-ASCII in a `Makefile`.** An em dash in a comment is
  enough to earn a warning, so keep the punctuation plain. `bsd-distinfo.py`
  emits ASCII for the same reason.

**Not submitted.** The right on-ramp is **pkgsrc-wip**
(<https://pkgsrc.org/wip/>), not a direct commit request: it exists precisely
for packages that need review and testing by someone other than the author, and
moving from wip to the main tree afterwards is routine. Whether to submit at all
is Jonaz's call — two channels have already rejected moraine on author/AI
grounds.
