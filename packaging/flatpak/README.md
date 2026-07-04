# Flatpak packaging for Moraine

Manifest for the **GTK desktop app** as a Flatpak — the universal Linux
GUI channel (Flathub). Files:

| File | Purpose |
|------|---------|
| `io.thern.moraine.yml` | build manifest (app + bundled rsync/ssh/rclone) |
| `io.thern.moraine.metainfo.xml` | AppStream metadata (name, screenshots, releases) |

> **Requires moraine ≥ 0.1.19.** Older versions look for assets only under
> `/usr/share/moraine/assets`; 0.1.19 resolves them via `XDG_DATA_DIRS`, which is
> what makes the sandboxed `/app` prefix work.

## Why the extra modules?

Moraine spawns `rsync`, `ssh` and `rclone` as external processes. A Flatpak
sandbox doesn't see the host's copies, so the manifest **bundles** them:
`rsync` and the OpenSSH client are built from source, and the official static
`rclone` binary is dropped into `/app/bin`. All three release hashes are pinned.

## Build & run locally

```sh
flatpak install flathub org.gnome.Platform//47 org.gnome.Sdk//47 \
    org.freedesktop.Sdk.Extension.rust-stable//23.08

flatpak-builder --user --install --force-clean build-dir \
    packaging/flatpak/io.thern.moraine.yml

flatpak run io.thern.moraine
```

The build is **offline** — the Cargo crates are vendored in `cargo-sources.json`
(committed, generated from `Cargo.lock`), so no network is needed at build time
and it's Flathub-ready as-is. To test against the current tree before a tag
exists, change the `moraine` source from `tag: v0.1.19` to `branch: main`.

## Publishing to Flathub

The manifest already builds offline, so submission is mostly validation:

1. If `Cargo.lock` changed since `cargo-sources.json` was generated, regenerate it
   with [flatpak-builder-tools](https://github.com/flatpak/flatpak-builder-tools):
   ```sh
   python3 flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
   ```
2. Validate the metadata:
   ```sh
   flatpak run org.freedesktop.appstream-glib validate io.thern.moraine.metainfo.xml
   desktop-file-validate <installed>/io.thern.moraine.desktop
   ```
4. Submit the manifest to https://github.com/flathub/flathub (new-app PR).

## On each new release

1. Bump the `moraine` source `tag:` and add a `<release>` entry to the metainfo.
2. Regenerate `cargo-sources.json`.
3. Bump the bundled `rsync` / `openssh` / `rclone` versions + `sha256` when you
   want newer tools (optional).
