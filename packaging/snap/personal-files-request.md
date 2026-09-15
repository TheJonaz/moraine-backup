# Snap Store — `personal-files` request (moraine)

Post as a **new topic** on <https://forum.snapcraft.io> in the **store-requests**
category (the rejection email points at
<https://snapcraft.io/docs/process-for-aliases-auto-connections-and-tracks>).
Jonaz posts it personally.

Context: revision 2 was rejected because `personal-files` is a super-privileged
interface whose *use* — not just auto-connection — needs approval. This request
asks only for **use with manual `snap connect`**, deliberately NOT auto-connect,
which is the lower bar. Cross-link the earlier thread (52730) since Oliver
Grawert recommended this exact interface there.

---

**Title:** Request for `personal-files` (read-only dotfiles): moraine

**Body:**

**Snap:** `moraine` — https://dashboard.snapcraft.io/snaps/moraine/
**Upstream:** https://github.com/TheJonaz/moraine-backup · MIT
**Related:** https://forum.snapcraft.io/t/request-for-classic-confinement-moraine/52730

`moraine` is a CLI backup tool: it takes hard-linked snapshots of user-selected
paths to a NAS/server over SSH (rsync) or to cloud storage via rclone. In that
earlier thread I asked for classic confinement; that was declined, and
@ogra recommended `system-backup` plus `personal-files` instead. I reworked the
snap to **strict** on that advice. Revision 2 was then rejected because
`personal-files` needs its own request, so I published without it — the snap has
been live in `stable` since, currently **revision 4 (0.3.1)**, strict, with
`home`, `removable-media`, `network` and `ssh-keys`. This post is the missing
request.

**Plug name:** `dot-backup-sources` — renamed from `dot-files` on review feedback
(2026-09-15), so the name states what the access is for rather than what it is.

**What I'm asking for:** permission to *use* `personal-files` (read-only).
I am **not** requesting auto-connection — users would run
`snap connect moraine:dot-backup-sources` themselves.

**Plug requested**

```yaml
plugs:
  dot-backup-sources:
    interface: personal-files
    read:
      - $HOME/.ssh
      - $HOME/.gnupg
      - $HOME/.config
      - $HOME/.gitconfig
      - $HOME/.mozilla
      - $HOME/.thunderbird
```

**Why these paths**

The `home` interface excludes every top-level dotfile (`owner @{HOME}/[^s.]**`),
and those dotfiles are precisely what a backup tool exists to preserve. `moraine recommend` proposes exactly
eleven source locations by default on Linux, and the six above are precisely the
ones `home` cannot reach: the other five (`~/Documents`, `~/Desktop`,
`~/Pictures`, `~/Music`, `~/Videos`) are re-downloadable media, while these six
are configuration, keys and mail profiles. A backup snap that silently skips a user's
`~/.ssh`, `~/.gnupg` and `~/.config` is worse than no backup, because the gap is
invisible until a restore.

The list is not an open-ended grab: it is exactly the dotfile half of the tool's
own default source set (src/recommend.rs), fixed, enumerable and shipped in the
recipe. Nothing was added to it for this request.

**Why read-only**

`moraine` never writes to a source — it only reads sources and writes to the
*destination* (a remote host over SSH/rclone, or removable media). So every path
is declared under `read:`, not `write:`.

**Alternatives considered**

- `home` alone — excludes all of the above, i.e. the actual problem.
- classic confinement — declined in thread 52730; strict + interfaces is the
  route I was pointed to, and I agree it is the better one.
- `system-backup` — I plan to use it later for whole-system/arbitrary paths, but
  it exposes the host read-only under `/var/lib/snapd/hostfs`, which needs
  app-side path handling first, and it is a heavier permission than reading a
  fixed list of the user's own dotfiles.

Happy to trim the list or answer any questions. Thanks for reviewing!

---

## Notes for us (not part of the post)

- Revisions 3 and 4 ship WITHOUT this plug so the snap can be published:
  `home` + `removable-media` + `network` + `ssh-keys` only. Current: rev 4 (0.3.1).
- The plug list was trimmed 2026-09-14 to match `moraine recommend` exactly:
  `.local/share`, `.bashrc` and `.profile` were dropped, since the tool does not
  propose them and unjustified paths invite a reviewer to trim the request.
- When granted: restore the `dot-backup-sources` block that is commented into
  `snapcraft.yaml`, rebuild, upload, and mention `snap connect moraine:dot-backup-sources`
  in the description again.
- `ssh-keys` is NOT super-privileged (snapd's ssh_keys.go only sets
  `deny-auto-connection: true`), so it needs no request — just a manual connect.
