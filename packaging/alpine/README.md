# Alpine Linux packaging for Moraine

This directory holds the `APKBUILD` for the Alpine package **`moraine`** (CLI +
GTK desktop app). Alpine's package tree (`aports`) is a *separate* git repo — the
`APKBUILD` here is the source of truth you copy into an `aports` checkout to
build and, if you become a maintainer, submit upstream.

It builds from the release source tarball
(`…/archive/refs/tags/vX.Y.Z.tar.gz`), so it tracks the last tagged release.

## Build it (on Alpine, or an `alpine:latest` container)

```sh
# 1. Tooling + build user (abuild refuses to run as root)
apk add alpine-sdk gtk4.0-dev
adduser -D builder && addgroup builder abuild
install -d -o builder /home/builder/pkg

# 2. As the build user, generate a signing key once
su builder
abuild-keygen -a -i

# 3. Drop the APKBUILD in a package dir and build
mkdir -p ~/aports/testing/moraine && cd ~/aports/testing/moraine
cp /path/to/moraine-backup/packaging/alpine/APKBUILD .
abuild -r                       # fetch, build, run tests, package
```

The resulting `.apk` lands in `~/packages/…/x86_64/`. Install it with:

```sh
apk add --allow-untrusted ~/packages/testing/x86_64/moraine-0.1.17-r0.apk
```

## On each new release

1. Tag the release in this repo (`git tag vX.Y.Z && git push origin vX.Y.Z`).
2. Bump `pkgver` in `APKBUILD` and reset `pkgrel=0`.
3. Refresh the checksum: `abuild checksum` (fills `sha512sums` from the real
   tarball). If you don't have Alpine handy, compute it manually:
   `curl -sL <tarball-url> | sha512sum`.
4. `abuild -r` to confirm it still builds, tests and packages cleanly.

## Notes

- `arch="all"` — Alpine builds from source, so this is not tied to which release
  binaries our CI publishes. rust, cargo-auditable and gtk4.0 (≥ 4.10, which the
  GUI needs) are available on every Alpine arch, and no dependency is
  arch-sensitive. Narrow it with `!armhf !armv7 !x86` if the 32-bit builders ever
  OOM on the release build.
- `build()`/`check()` use `--frozen` (= `--locked --offline`), which is why
  `prepare()` runs `cargo fetch`. That keeps the build network-free, so the
  aport does not need `options="net"`.
- `check()` runs `cargo test` in the dev profile — Alpine asks packagers not to
  pass `--release` there. Drop the function if a release ever ships without tests.
- Runtime deps are `rsync` + `openssh-client` (the `ssh` backend). `rclone` is an
  optional runtime dep for the rclone/FTP backends — install it separately.
