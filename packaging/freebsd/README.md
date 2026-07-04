# FreeBSD port for Moraine

A ports skeleton (`Makefile` + `pkg-descr`) that builds the CLI + GTK app via
`USES=cargo`. FreeBSD's Ports tree is a separate repo; copy this into a
`sysutils/moraine/` directory to build and submit.

## Build & test

```sh
# on FreeBSD, with the ports tree at /usr/ports
mkdir -p /usr/ports/sysutils/moraine
cp Makefile pkg-descr /usr/ports/sysutils/moraine/
cd /usr/ports/sysutils/moraine
make makesum        # fetches the tarball + all crates, writes distinfo
make install
```

`make makesum` generates `distinfo` (SHA256 + size for the source tarball **and**
every vendored crate) — it must run on a machine with network access; that's why
`distinfo` isn't checked in here. Refresh `CARGO_CRATES` with `make cargo-crates`
if `Cargo.lock` changed.

## Submit

Open a PR/bug on FreeBSD's Bugzilla (category `Ports & Packages`) or via the
GitHub `freebsd/freebsd-ports` mirror, per the
[Porter's Handbook](https://docs.freebsd.org/en/books/porters-handbook/).

## On each new release

Bump `DISTVERSION`, run `make cargo-crates` (updates `CARGO_CRATES`) and
`make makesum` (updates `distinfo`).
