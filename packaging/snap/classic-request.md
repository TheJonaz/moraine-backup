# Snap Store — classic confinement request (moraine)

Post this as a **new topic** on <https://forum.snapcraft.io> in the
**store-requests** category. A reviewer from the store team then evaluates it and
either grants classic or asks for changes. (Classic requests are handled manually
and can take days to a couple of weeks.)

---

**Title:** Request for classic confinement: moraine

**Body:**

**Snap:** `moraine` — https://dashboard.snapcraft.io/snaps/moraine/
**Upstream:** https://github.com/TheJonaz/moraine-backup · MIT · a CLI backup tool.

`moraine` takes hard-linked snapshots of **user-selected paths** to a NAS/server
over SSH (rsync) or to cloud/FTP/SMB/WebDAV/S3/Drive via rclone, and restores whole
snapshots or individual files. This snap ships the **command-line client only**
(the GTK desktop build is distributed as a Flatpak/AppImage/.deb elsewhere), with
`rsync`, the OpenSSH client and `rclone` bundled so every backend works out of the
box.

**Why classic confinement is required**

A backup tool has to read whatever paths the user points it at, and that set is
chosen at runtime — it cannot be known or enumerated in advance. Under strict
confinement:

- The **`home`** interface grants "access to non-hidden files in the home
  directory." Its AppArmor rule is `owner @{HOME}/[^s.]** rwkl`, which excludes
  every top-level dotfile. But the highest-value things to back up are exactly
  those — `~/.ssh`, `~/.gnupg`, `~/.config`, `~/.gitconfig`, `~/.mozilla`,
  `~/.thunderbird`: the configuration and keys a backup exists to protect. Six of
  the eleven default source locations `moraine` suggests on Linux are top-level
  dotfiles; a backup that silently skips them is worse than none.
- **`personal-files` / `system-files`** cannot close the gap: they declare
  *named* paths (one store request per path), whereas moraine's source set is
  whatever the user chooses. There is no fixed list to declare.
- Scheduling installs entries into the user's **crontab**, which strict
  confinement also denies.
- `rsync`/`ssh`/`rclone` run as helpers against arbitrary local sources and
  arbitrary remote destinations.

Taken together, the core function — "back up the paths I choose, including my
dotfiles, on a schedule" — cannot be delivered under strict confinement or the
current interface set, which is why the snap declares `confinement: classic`.

Happy to answer questions or adjust anything. Thanks for reviewing!

---

## Notes for us (not part of the post)

- The snap `moraine` v0.2.2 (rev 1) is already uploaded and sits in **"Manual
  review pending"** purely because of `confinement: classic`; this forum request
  is what unblocks it.
- Posting requires a confirmed **snapcraft.io forum account** (Ubuntu One). The
  Aug-5 confirmation email has likely expired — re-request/log in first.
- The technical argument mirrors the comment block in `packaging/snap/snapcraft.yaml`.
- Jonaz posts this **personally** — do not post on his behalf.
