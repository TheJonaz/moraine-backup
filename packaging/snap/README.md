# Snap packaging for Moraine

`snapcraft.yaml` builds a **classically confined, CLI-only** snap: the `moraine`
command with `rsync`, the OpenSSH client and `rclone` bundled, so every backend
works without installing anything else.

The GTK desktop app is **not** in this snap. It ships as a
[Flatpak](../flatpak/), an [AppImage](../appimage/) and a `.deb`.

## Why classic, and why no GUI

Both follow from the same constraint, and neither is a shortcut.

**Strict confinement cannot see the files worth backing up.** The `home`
interface is documented as granting "access to non-hidden files in the home
directory", and snapd's AppArmor rule is literally `owner @{HOME}/[^s.]** rwkl`
— every top-level dotfile is excluded. Of the eleven locations `moraine
recommend` suggests on Linux, six are exactly those:

| Reachable under `home` | Invisible to a strict snap |
| --- | --- |
| `~/Documents`, `~/Desktop`, `~/Pictures`, `~/Music`, `~/Videos` | `~/.config`, `~/.mozilla`, `~/.thunderbird`, `~/.gitconfig`, `~/.ssh`, `~/.gnupg` |

The split is worse than 5–6 suggests: what works is re-downloadable media, what
does not is configuration and keys — the things a backup exists for.
`personal-files` cannot close the gap either, because it declares *named* paths
and needs a store request per path, while the set here is whatever the user
chooses. Scheduling also writes to crontab, which strict confinement denies.

**Classic rules out the GUI.** The `gnome` extension, which supplies GTK and its
runtime to a snap, only exists for strict confinement. A GUI under classic would
mean bundling GTK 4 and patching rpath by hand — a separate project, for a
platform already covered three other ways.

## Build locally

No snapd required, so this works on distributions that block it (Linux Mint
ships `/etc/apt/preferences.d/nosnap.pref`):

```sh
./packaging/snap/build-in-docker.sh            # the release tag pinned in the yaml
./packaging/snap/build-in-docker.sh --local    # the working tree instead
```

The `.snap` lands in `packaging/snap/build/`. [`Dockerfile.build`](Dockerfile.build)
explains how it runs snapcraft without snapd — briefly: snapcraft, `core24` and
`snapd` are unsquashed into the absolute `/snap/<name>/current` paths their
binaries are linked against, and `snapcraft --destructive-mode` runs in place.

With snapd available, the conventional route still works:

```sh
mkdir -p snap && cp packaging/snap/snapcraft.yaml snap/snapcraft.yaml
snapcraft
```

## Install and run

```sh
sudo snap install --dangerous --classic moraine_0.2.2_amd64.snap
moraine --version
```

`--classic` is required; `--dangerous` only because a local file is unsigned.
There are no interfaces to connect: a classic snap has the host's filesystem
already.

## Publish to the Snap Store

Classic confinement needs manual review — the snap cannot be released until a
reviewer grants it.

```sh
snapcraft login
snapcraft register moraine        # once, if the name is free
snapcraft upload moraine_0.2.2_amd64.snap    # will be held for review
```

Then request classic on the [forum](https://forum.snapcraft.io/c/store-requests/19),
stating the case: a backup client has to read arbitrary user-chosen paths, and
the `home` interface's exclusion of top-level dotfiles removes most of what
users back up. The table above is the argument.

## Known limitations

- **Bundled rclone is Ubuntu 24.04's, currently v1.60.1** — released 2022, well
  behind upstream. Everything Moraine calls (`copy`, `--copy-dest`, `lsf`,
  `touch`, `deletefile`, `purge`, `obscure`, `backend features`, `crypt`) has
  been in rclone far longer than that, but newer cloud backends and fixes are
  missing. A `rclone` part built from source would remove the ceiling.
- **amd64 only** as written. Other architectures need `platforms:` and builders.

## On each new release

Bump `version:` and `source-tag:` in `snapcraft.yaml`, rebuild, upload. The
recipe builds from the GitHub tag, not the working tree, so the two must match.
