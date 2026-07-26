# FreeBSD port for Moraine

A ports skeleton (`Makefile` + `pkg-descr` + `distinfo`) that builds the CLI +
GTK app via `USES=cargo`. FreeBSD's Ports tree is a separate repo; copy this
into a `sysutils/moraine/` directory to build and submit.

**Build-verified on FreeBSD 15.1-RELEASE (2026-07-27):** `make install`
succeeds, `check-plist` reports no issues, and both binaries run
(`moraine 0.2.2`).

## Build & test

```sh
# on FreeBSD, with a ports tree at /usr/ports
mkdir -p /usr/ports/sysutils/moraine
cp Makefile pkg-descr distinfo /usr/ports/sysutils/moraine/
cd /usr/ports/sysutils/moraine
make install        # distinfo is checked in, so makesum isn't needed
make check-plist    # verify the packing list matches what's staged
```

> **Match the ports tree to your package set.** With a RELEASE you get the
> `quarterly` pkg repo, so clone ports from the matching quarterly branch
> (e.g. `git clone --depth 1 -b 2026Q3 https://git.FreeBSD.org/ports.git`).
> Cloning ports HEAD instead makes `lang/rust` a newer version than the
> installed `rust` package, and the build then tries to compile rustc from
> source — hours of work that needs well over 4 GB RAM. Install the binary
> build deps first: `pkg install rust gtk4 pkgconf desktop-file-utils`.

`distinfo` (SHA256 + size for the source tarball **and** every vendored crate)
is checked in here so the port is submittable as-is. Regenerate it with
`make makesum` on a machine with network access.

## Submit

Open a PR/bug on FreeBSD's Bugzilla (category `Ports & Packages`) or via the
GitHub `freebsd/freebsd-ports` mirror, per the
[Porter's Handbook](https://docs.freebsd.org/en/books/porters-handbook/).
Run `portlint -A` in a clean tree first.

## On each new release

Bump `DISTVERSION`, then regenerate **both** generated lists — they drift apart
otherwise, and a stale `CARGO_CRATES` fails the build with
`error: no matching package named <crate> found`:

```sh
make cargo-crates   # prints the up-to-date CARGO_CRATES; paste it into Makefile
make makesum        # rewrites distinfo for the new crate set
```
