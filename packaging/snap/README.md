# Snap packaging for Moraine

`snapcraft.yaml` builds a strictly-confined snap containing **both** the GTK
desktop app (`moraine-gui`) and the CLI (`moraine`), with rsync, the OpenSSH
client and rclone bundled via `stage-packages` so every backend works offline.

> **Requires moraine ≥ 0.1.19** — the app resolves its assets via
> `XDG_DATA_DIRS`, which the snap sets to include `$SNAP/usr/share`.

## Build & install locally

`snapcraft` builds in a `core24` container (LXD or multipass):

```sh
# from the repo root — snapcraft expects the yaml at ./snap/snapcraft.yaml
mkdir -p snap && cp packaging/snap/snapcraft.yaml snap/snapcraft.yaml
snapcraft                       # produces moraine_0.1.19_amd64.snap
sudo snap install --dangerous moraine_0.1.19_amd64.snap

# connect the manual interfaces (auto-connected once published to the store)
sudo snap connect moraine:ssh-keys
sudo snap connect moraine:removable-media
```

Run with `moraine.moraine-gui` / `moraine.moraine`, or alias them at the store.

## Confinement & filesystem access

The snap is `strict` and reaches:

- `home` — everything under `$HOME`
- `removable-media` — `/media`, `/mnt` (external drives)
- `ssh-keys` — `~/.ssh` keys for the SSH backend
- `network` — ssh / rclone / ftp transports

That covers the common case (back up your home + external drives). To back up
paths **outside** `$HOME`, either:

- add a `system-files` plug declaring the specific paths (needs a store request), or
- switch to `confinement: classic` (full host access, but classic snaps need
  manual store review and can't be strictly confined).

## Publish to the Snap Store

```sh
snapcraft login
snapcraft register moraine        # once, if the name is free
snapcraft upload --release=stable moraine_0.1.19_amd64.snap
```

## On each new release

Bump `version:` and `source-tag:` in `snapcraft.yaml`, rebuild, upload.
