# Gentoo packaging for Moraine

`moraine-<version>.ebuild` builds the CLI and, with `USE=gui`, the GTK4 app via
the `cargo` eclass. This directory is the **source of truth**; the published
copy lives in the self-hosted overlay
[TheJonaz/moraine-overlay](https://github.com/TheJonaz/moraine-overlay) as
`app-backup/moraine`.

## Where it is published, and why not GURU

GURU is an official Gentoo project and is bound by Gentoo's
[AI policy](https://wiki.gentoo.org/wiki/Project:Council/AI_policy), which
forbids contributions created with the assistance of NLP AI tools. Parts of this
ebuild were, so neither GURU nor `::gentoo` is open to it. The overlay is the
Gentoo channel instead — users add it with:

```sh
eselect repository add moraine git https://github.com/TheJonaz/moraine-overlay.git
emaint sync -r moraine
emerge -av app-backup/moraine
```

## Releasing

`moraine-deploy/gentoo-overlay-release.sh X.Y.Z [--push]` copies the bumped
ebuild into the overlay, downloads every distfile, writes the thin `Manifest`
from the real bytes (no portage needed on the workstation — verified to be
byte-identical to `ebuild … manifest`), commits and pushes. It **refuses to run
if `CRATES` disagrees with `Cargo.lock`**, which is the trap that has bitten this
package on FreeBSD, Gentoo and Flatpak — each time discovered only after a long
build.

Regenerate `CRATES` + the crate `LICENSE` set after a dependency change:

```sh
pycargoebuild -i packaging/gentoo/moraine-X.Y.Z.ebuild .
```

It needs Portage's `metadata/license-mapping.conf` (fetch from gitweb.gentoo.org
on a non-Gentoo host, pass with `-l`) and the exact marker comment
`# Dependent crate licenses` above `LICENSE+=`, or it aborts.

## Testing

Use a docker stage3 — far cheaper than a Gentoo install:

```sh
docker run --rm -v <overlay>:/var/db/repos/moraine -v <distfiles>:/var/cache/distfiles \
    gentoo/stage3:latest bash
# inside: /etc/portage/repos.conf/moraine.conf -> location = /var/db/repos/moraine
emerge-webrsync && emerge -v1 app-backup/moraine
```

Notes that cost time to rediscover:

- **HTTPS to github.com fails inside the stage3** — fetch distfiles on the host
  into a bind-mounted `DISTDIR` instead.
- `USE=gui` needs the `desktop` profile (a minimal stage3 lacks the USE flags the
  GTK4 stack wants: `freetype[harfbuzz]` and friends). `USE=-gui` builds on the
  default profile and is the fast check.
- `FEATURES="getbinpkg"` uses the image's preconfigured binhost, but a `USE=gui`
  run still compiles ~39 of 152 packages from source — budget hours.
- Cheap sanity check without any GTK4: `ebuild <file> clean unpack` sets up
  cargo's vendored registry and alone catches a stale `CRATES` list.
- `cargo_src_configure` does **not** add `--no-default-features` by itself; the
  CLI-only path passes it explicitly.
