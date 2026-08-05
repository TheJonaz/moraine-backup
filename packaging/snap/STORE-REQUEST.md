# Classic confinement request — draft

Post to <https://forum.snapcraft.io/c/store-requests/classic-confinement/26> **after** the first
upload, so reviewers have a revision to look at. Written to be read by someone
who reviews a dozen of these a week: the constraint first, the evidence second,
no adjectives.

---

**Title:** `Request for classic confinement: moraine`

---

Moraine is a snapshot backup client (CLI). It takes hard-linked snapshots of
user-chosen paths to a NAS or server over SSH/rsync, or to cloud storage via
rclone. MIT licensed, sources at https://github.com/TheJonaz/moraine-backup

**Why strict confinement does not work**

The whole function of the application is to read whatever paths the user asks it
to back up. That set is chosen by the user at runtime and cannot be enumerated
in advance.

Under strict confinement the `home` interface is documented as granting "access
to non-hidden files in the home directory", and the AppArmor rule generated for
it is `owner @{HOME}/[^s.]** rwkl` — every top-level dotfile is excluded. Of the
eleven locations the application suggests backing up on Linux, six are exactly
those:

| Reachable under `home` | Excluded |
| --- | --- |
| `~/Documents`, `~/Desktop`, `~/Pictures`, `~/Music`, `~/Videos` | `~/.config`, `~/.mozilla`, `~/.thunderbird`, `~/.gitconfig`, `~/.ssh`, `~/.gnupg` |

The reachable half is re-downloadable media. The excluded half is configuration
and keys — what a backup is for. A backup tool that silently omits `~/.config`
and `~/.gnupg` is worse than none, because the user believes they are covered.

`personal-files` does not close the gap: it declares named paths and would need
a request per path, while the set here is whatever the user picks — including
paths outside `$HOME` entirely, which is a common configuration for a machine
with a separate data partition.

Scheduled backups are installed into the user's crontab, which strict
confinement also denies.

**Scope**

This snap ships the command-line client only, with `rsync`, the OpenSSH client
and `rclone` bundled as stage-packages. The GTK desktop application is
distributed separately (Flatpak, AppImage, .deb) and is deliberately not part of
this snap.

**Category**

I believe this falls under the accepted "requires arbitrary filesystem access"
category, on the same basis as backup and file-synchronisation tools already
granted classic.

---

## Before posting

- [ ] `snapcraft upload` has completed at least once (the request needs a
      revision to point at)
- [ ] A forum account exists. The forum is a plain Discourse with its own
      sign-up (https://forum.snapcraft.io/signup) — it does NOT delegate to
      Ubuntu One, so this is a separate account from the store one. Say in the
      post which store account published the snap; reviewers verify the
      publisher themselves rather than inferring it from the login.
- [ ] Ready to answer the standard follow-up: *why not `system-files`?* — same
      answer as `personal-files` above: the paths are user-chosen and unbounded

## If it is refused

Nothing else in the distribution set depends on this. The channel simply stays
unpublished; the recipe and the build script remain useful for anyone who wants
to build it themselves with `--dangerous --classic`.
