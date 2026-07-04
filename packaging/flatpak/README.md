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

The `moraine` module builds with `--share=network` so cargo can fetch crates.
To test against the current tree before a tag exists, change the `moraine`
source from `tag: v0.1.19` to `branch: main`.

## Publishing to Flathub

Flathub builds **offline**, so you must vendor the Cargo dependencies instead of
letting cargo hit the network:

1. Generate the offline crate sources with
   [flatpak-builder-tools](https://github.com/flatpak/flatpak-builder-tools):
   ```sh
   python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json
   ```
2. Add `cargo-sources.json` to the `moraine` module's `sources:` and remove the
   `--share=network` build-arg.
3. Validate the metadata:
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
