# AppImage packaging for Moraine

`build-appimage.sh` produces a single portable `Moraine-<version>-x86_64.AppImage`
containing the GTK desktop app (`moraine-gui`) and the CLI (`moraine`), with GTK 4
and its dependencies bundled via `linuxdeploy-plugin-gtk`.

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

CI builds it per release: the `appimage` job in `.github/workflows/release.yml`
attaches `Moraine-X.Y.Z-x86_64.AppImage` to the GitHub release, and the CDN pull
picks it up from there.

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
chmod +x Moraine-*-x86_64.AppImage
./Moraine-*-x86_64.AppImage
```

Optionally integrate it into menus with
[Gear Lever](https://github.com/mijorus/gearlever) or `appimaged`.
