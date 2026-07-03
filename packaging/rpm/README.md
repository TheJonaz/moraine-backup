# RPM packaging for Moraine (Fedora / RHEL / openSUSE)

`moraine.spec` builds both the CLI and the GTK desktop app. It was test-built
and installed in a Fedora container (`rpmbuild -bb` → install → run → correct
file layout).

## Install now (no Copr) — from the release

A prebuilt RPM is attached to each release:

```sh
sudo dnf install https://github.com/TheJonaz/moraine-backup/releases/download/v0.1.17/moraine-0.1.17-1.fc44.x86_64.rpm
```

(Built for current Fedora; on a hardened config add `--nogpgcheck` since the
release RPM is unsigned.)

## Publish via Copr (recommended — auto-builds + a real repo)

[Copr](https://copr.fedorainfracloud.org) is a self-service build service
(like a PPA for RPM):

1. Create a Copr project (once), e.g. `moraine`. **Enable "network" in the
   build settings** — the spec fetches crates with `cargo build` (or vendor the
   deps for a fully offline build).
2. Build from the spec + release tarball:

   ```sh
   # with the copr-cli tool, or via the web UI "New Build → SCM/Upload"
   copr-cli build TheJonaz/moraine packaging/rpm/moraine.spec
   ```

Users then:

```sh
sudo dnf copr enable TheJonaz/moraine
sudo dnf install moraine
```

## openSUSE / build-many-distros

The same spec builds on the [openSUSE Build Service](https://build.opensuse.org)
(OBS), which can emit RPMs (and .debs) for many distributions from one project.

## On each new release

Bump `Version:` in the spec, add a `%changelog` entry, and rebuild
(Copr/OBS rebuild from the new tag, or `rpmbuild` locally). `Requires`
(`rsync`, `openssh-clients`) and `Recommends` (`rclone`, `gnupg2`,
`NetworkManager`) map to the code's external tools.
