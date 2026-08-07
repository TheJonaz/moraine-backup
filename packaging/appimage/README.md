# AppImage packaging for Moraine

`build-appimage.sh` produces a single portable
`Moraine-<version>-<arch>.AppImage` containing the GTK desktop app
(`moraine-gui`) and the CLI (`moraine`), with GTK 4 and its dependencies bundled
via `linuxdeploy-plugin-gtk`.

**x86_64 and aarch64** are both built, each on its own native runner. There is
no cross-compiling here: linuxdeploy bundles the *host's* GTK stack, so the
architecture of the AppImage is the architecture of the machine that built it.
The script refuses to run anywhere else rather than emitting something
mislabelled.

> **Requires moraine ≥ 0.1.19** — the AppRun exports `XDG_DATA_DIRS` pointing
> inside the AppImage, and 0.1.19 resolves its assets from there.

## Which distros this runs on

Built on **Ubuntu 24.04**, which sets a **glibc 2.39 floor**:

| Runs | Does not run |
|---|---|
| Ubuntu 24.04+, Debian 13+, Fedora 40+, Arch/openSUSE TW and other rolling | Ubuntu 22.04, Debian 12, Mint 21 (glibc 2.35/2.36) |

The usual "build on the oldest distro you support" advice **cannot** be followed
here: moraine builds gtk4-rs with `features = ["v4_10"]` and Ubuntu 22.04 ships
GTK 4.6, so the build fails outright. 24.04 is the oldest base with a new enough
GTK. Older systems are covered by the Flatpak (own runtime, no glibc floor) and
the `.deb`, so nothing is actually lost — but do not "upgrade" the CI job to
`ubuntu-latest`, because that silently raises the floor and drops working
distros.

Verified 0.2.2 on Ubuntu 24.04 (glibc 2.39) and Arch (glibc 2.43): the app links
and reaches GTK's display check. Ubuntu 22.04 and Debian 12 fail at the loader
with `version 'GLIBC_2.39' not found`, as expected.

## Build

```sh
# deps: rustup/cargo, libgtk-4-dev, curl, patchelf, fuse (or the env var below)
./packaging/appimage/build-appimage.sh
```

It builds the binaries, assembles an `AppDir`, downloads `linuxdeploy` + the GTK
plugin into `.appimage-tools/`, and emits the `.AppImage` in the repo root.
Without FUSE (CI, containers), set `APPIMAGE_EXTRACT_AND_RUN=1` so linuxdeploy
unpacks itself instead of mounting.

CI builds both per release: the `appimage` job in `.github/workflows/release.yml`
is a matrix over `ubuntu-24.04` and `ubuntu-24.04-arm`, attaching
`Moraine-X.Y.Z-x86_64.AppImage` and `Moraine-X.Y.Z-aarch64.AppImage` to the
GitHub release, and the CDN pull picks them up from there. Each job checks the
binary inside its own bundle really is the architecture it claims — a
mislabelled AppImage installs perfectly and only fails on a user's machine.

## What the host must provide

The AppImage bundles GTK, pango, cairo, gdk-pixbuf and graphene, but leaves
**seven libraries to the host** — `libX11`, `libxcb`, `libwayland-client`,
`libfontconfig`, `libfreetype`, `libharfbuzz`, `libfribidi`. That is AppImage's
standard excludelist: they have to match the host's display and font stack.
Every desktop install has them. A bare container does not, so a container smoke
test must install them first or it fails with a misleading
`libharfbuzz.so.0: cannot open shared object file`.

It also relies on the host's **`rsync`, `ssh` and `rclone`** (present on
virtually every Linux install) — unlike the Flatpak/Snap, which bundle them for
sandbox isolation. If a host lacks one: `rsync` + `openssh-client` for the SSH
backend, `rclone` for cloud/FTP.

## Run

```sh
chmod +x Moraine-*.AppImage
./Moraine-*.AppImage
```

On a Raspberry Pi take the `aarch64` build, and note the GTK floor: the desktop
app needs GTK ≥ 4.10, which Raspberry Pi OS *bookworm* does not have. The CLI
inside the bundle runs regardless.

Optionally integrate it into menus with
[Gear Lever](https://github.com/mijorus/gearlever) or `appimaged`.
