# macOS packaging for Moraine

[`build-pkg.sh`](build-pkg.sh) produces `moraine-<version>-macos-universal.pkg`:
a standard macOS installer that puts the `moraine` command in `/usr/local/bin`,
its manual page in `/usr/local/share/man/man1`, and the docs in
`/usr/local/share/doc/moraine`.

The binary is universal — arm64 and x86_64 in one file, so a single package
covers Apple Silicon and Intel.

## Why a .pkg and not a .dmg

The macOS build is the **CLI only**. The desktop app is GTK 4, and bundling that
into a `.app` is a separate project (the same reason the [Snap](../snap/) has no
GUI). A `.dmg` is a drag-to-`/Applications` gesture, and there is no `.app` to
drag — a disk image containing a bare executable leaves the user holding a file
with nowhere to put it. A `.pkg` installs a command-line tool onto the PATH,
which is exactly what this is.

## Build

On a Mac with the Xcode command-line tools and rustup:

```sh
./packaging/macos/build-pkg.sh
```

It cross-compiles the Intel slice from an Apple Silicon machine (or the reverse)
and `lipo`s the two together, then fails loudly if the result is not actually
fat. There is no Linux path: `pkgbuild` and `productbuild` are macOS-only, which
is why this runs in CI rather than on the workstation.

## Gatekeeper — read this before publishing

The package is **unsigned and unnotarized**. Double-clicking a downloaded copy
gets refused: *"…can't be opened because it is from an unidentified developer"*
(macOS 15 words it as *"Apple could not verify…"*). Nothing is wrong with the
file; it simply carries a quarantine flag and no signature to check against.

Three ways past it, in the order worth telling users about:

```sh
# 1. The command line does no Gatekeeper assessment at all.
sudo installer -pkg moraine-0.3.0-macos-universal.pkg -target /

# 2. Strip the quarantine flag, then open normally.
xattr -d com.apple.quarantine moraine-0.3.0-macos-universal.pkg
```

3. Control-click the file → **Open** → **Open**, or approve it afterwards under
   System Settings → Privacy & Security → *Open Anyway*.

**Signing it properly costs money and identity, not effort.** It needs an Apple
Developer Program membership (99 USD/year) and two certificates — *Developer ID
Application* for the binary, *Developer ID Installer* for the package. With
those, the script signs automatically:

```sh
export MORAINE_CODESIGN_ID="Developer ID Application: Name (TEAMID)"
export MORAINE_PKG_SIGN_ID="Developer ID Installer: Name (TEAMID)"
./packaging/macos/build-pkg.sh
```

Notarization is a further step after signing, and cannot be skipped if the goal
is a clean double-click:

```sh
xcrun notarytool submit moraine-*.pkg \
  --apple-id <id> --team-id <TEAMID> --password <app-specific-password> --wait
xcrun stapler staple moraine-*.pkg
```

Until then, Homebrew remains the friction-free route on macOS — `brew` installs
from source and never meets Gatekeeper. The `.pkg` is for people who want an
installer rather than a package manager.

## One deliberate difference from the .tar.gz

The release also carries `moraine-macos-arm64.tar.gz`, built
`--no-default-features`. This package is built `--no-default-features --features
keyring`, which on macOS is the native Keychain and pulls in no runtime
dependency. So `password = "keyring:"` and `moraine secrets set` work out of the
box from the installer, and do not from the tarball. That is the difference, and
it is intentional: a desktop install is exactly the case where a Keychain
exists.

## What the package does not carry

`rsync` and `rclone` are **not** bundled. Apple ships an rsync, but which one
depends on the macOS release, and the modern ones are reduced; `brew install
rsync` is the reliable answer. `rclone` is not present at all and is needed for
every cloud target. The OpenSSH client is part of macOS and is fine as shipped.
The installer's welcome pane says all of this before the user commits.

## Uninstall

There is no uninstaller — a package this small does not warrant one:

```sh
sudo rm -f /usr/local/bin/moraine \
           /usr/local/share/man/man1/moraine.1
sudo rm -rf /usr/local/share/doc/moraine
sudo pkgutil --forget io.thern.moraine
```

`pkgutil --forget` only drops the receipt; the `rm`s are what actually remove
the files. User configuration in `~/.config/moraine` is left alone on purpose.

## On each new release

Nothing to bump — the version comes from `Cargo.toml` and is substituted into
[`distribution.xml`](distribution.xml) at build time. The release workflow builds
and attaches the package automatically.
