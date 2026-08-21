# Snap packaging for Moraine

`snapcraft.yaml` builds a **strictly confined, CLI-only** snap: the `moraine`
command with `rsync`, the OpenSSH client and `rclone` bundled, so every backend
works without installing anything else.

The GTK desktop app is **not** in this snap. It ships as a
[Flatpak](../flatpak/), an [AppImage](../appimage/) and a `.deb`.

## Why strict (it started out classic)

The first upload used `confinement: classic`, because the `home` interface only
grants "access to non-hidden files in the home directory" — its AppArmor rule is
literally `owner @{HOME}/[^s.]** rwkl`, which excludes every top-level dotfile.
Of the eleven locations `moraine recommend` suggests on Linux, six are exactly
those:

| Reachable under `home` | Excluded by `home` (need `personal-files`) |
| --- | --- |
| `~/Documents`, `~/Desktop`, `~/Pictures`, `~/Music`, `~/Videos` | `~/.config`, `~/.mozilla`, `~/.thunderbird`, `~/.gitconfig`, `~/.ssh`, `~/.gnupg` |

A store reviewer declined classic *"as of now"* on eligibility grounds (classic
is reserved for mature, well-known apps and the project is judged too new), and
Oliver Grawert pointed out the right approach:
[thread 52730](https://forum.snapcraft.io/t/request-for-classic-confinement-moraine/52730).
So the snap is now **strict**, and the excluded dotfiles come back via a
read-only `personal-files` plug (`dot-files`) declaring that default set —
exactly the "named paths" personal-files is for.

**Covered now** (the common case, at real paths, no host prefix):
`home` + `dot-files` (personal-files) + `removable-media` + `network` +
`ssh-keys`. `home` auto-connects; enable the rest once with
`snap connect moraine:dot-files`.

**Deferred to a later release** (each needs app-side work):

- **Whole-system / arbitrary paths outside `$HOME`** via the `system-backup`
  interface, which exposes the host root read-only at `/var/lib/snapd/hostfs`.
  Oliver confirmed source paths must be prefixed with that and the prefix should
  be hidden in the UI — so it lands once moraine gains snap-path handling.
- **In-app scheduling.** Strict confinement cannot write the host crontab. For
  now, schedule from a host systemd timer / cron running
  `snap run moraine backup <target>`. Snap systemd timers are the eventual path
  (dynamic per-target timers are still an open question).

**Why no GUI.** The snap is CLI-only regardless of confinement — the GTK app is
already covered by the Flatpak, AppImage and `.deb`, and bundling GTK 4 into a
snap is a separate project for no new reach.

## Build locally

No snapd required, so this works on distributions that block it (Linux Mint
ships `/etc/apt/preferences.d/nosnap.pref`):

```sh
./packaging/snap/build-in-docker.sh            # the release tag pinned in the yaml
./packaging/snap/build-in-docker.sh --local    # the working tree instead
```

The `.snap` lands in `packaging/snap/`. [`Dockerfile.build`](Dockerfile.build)
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
sudo snap install --dangerous moraine_0.2.2_amd64.snap
sudo snap connect moraine:dot-files    # to back up ~/.ssh, ~/.config, GPG, …
moraine --version
```

`--dangerous` is only because a local file is unsigned. `home`, `network`,
`removable-media` and `ssh-keys` connect automatically; `dot-files` is the one
manual step.

## Publish to the Snap Store

```sh
snapcraft login
snapcraft register moraine                            # once, if the name is free
snapcraft upload --release=stable moraine_0.2.2_amd64.snap
```

The `personal-files` plug is likely to be **flagged for a store review** (it can
read sensitive paths), but a narrow one — the reviewer checks the declared
dotfile paths, which are justified for a backup tool — not the classic
eligibility gate. Reply on [thread 52730](https://forum.snapcraft.io/t/request-for-classic-confinement-moraine/52730)
if the reviewer has questions.

## Known limitations

- **Bundled rclone is Ubuntu 24.04's, currently v1.60.1** — released 2022, well
  behind upstream. Everything Moraine calls (`copy`, `--copy-dest`, `lsf`,
  `touch`, `deletefile`, `purge`, `obscure`, `backend features`, `crypt`) has
  been in rclone far longer than that, but newer cloud backends and fixes are
  missing. A `rclone` part built from source would remove the ceiling.
- **amd64 only** as written. Other architectures need `platforms:` and builders.
- **Whole-system paths and in-app scheduling are deferred** — see the two bullets
  under "Why strict" above.

## On each new release

Bump `version:` and `source-tag:` in `snapcraft.yaml`, rebuild, upload. The
recipe builds from the GitHub tag, not the working tree, so the two must match.
