# AppImage packaging for Moraine

`build-appimage.sh` produces a single portable `Moraine-<version>-x86_64.AppImage`
containing the GTK desktop app (`moraine-gui`) and the CLI (`moraine`), with GTK 4
and its dependencies bundled via `linuxdeploy-plugin-gtk`.

> **Requires moraine ≥ 0.1.19** — the AppRun exports `XDG_DATA_DIRS` pointing
> inside the AppImage, and 0.1.19 resolves its assets from there.

## Build

Build on the **oldest glibc you want to support** (e.g. Ubuntu 22.04) so the
AppImage runs on newer systems too.

```sh
# deps: rustup/cargo, libgtk-4-dev, curl, fuse (libfuse2)
./packaging/appimage/build-appimage.sh
```

It builds the binaries, assembles an `AppDir`, downloads `linuxdeploy` + the GTK
plugin into `.appimage-tools/`, and emits the `.AppImage` in the repo root.

## Runtime tools

The AppImage bundles GTK but **relies on the host's `rsync`, `ssh` and `rclone`**
(present on virtually every Linux install) — unlike the Flatpak/Snap, which bundle
them for sandbox isolation. If a host lacks one, install it from its distro:
`rsync` + `openssh-client` for the SSH backend, `rclone` for cloud/FTP.

## Run

```sh
chmod +x Moraine-*-x86_64.AppImage
./Moraine-*-x86_64.AppImage
```

Optionally integrate it into menus with
[Gear Lever](https://github.com/mijorus/gearlever) or `appimaged`.

## CI / releases

This is a good candidate to run in `release.yml` on an `ubuntu-22.04` runner and
attach the `.AppImage` to the GitHub release, alongside the existing tarballs.
