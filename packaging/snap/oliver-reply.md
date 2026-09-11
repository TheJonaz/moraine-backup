# Draft reply to the forum thread (52730)

Post this as a reply on
<https://forum.snapcraft.io/t/request-for-classic-confinement-moraine/52730>.
Jonaz posts it personally (AI-policy caution).

---

Thanks both — that's fair, and the `system-backup` pointer is exactly what I was
missing.

On the apparent contradiction: both lines are true, and your suggestion actually
reconciles them. `moraine recommend` proposes a fixed **default** set of sources
on Linux (the eleven locations, six of them top-level dotfiles) — that part is
knowable and declarable. But the tool also lets the user back up **any** path
they choose beyond that set, which is the "no fixed list" part. So:

- `personal-files` (read-only) for the default dotfiles — `~/.ssh`, `~/.gnupg`,
  `~/.config`, `~/.gitconfig`, `~/.mozilla`, `~/.thunderbird`, … — plus `home`
  and `removable-media` for the common case, and
- `system-backup` for the arbitrary-path / whole-system case.

I'm reworking the snap to **strict** on that basis (dropping the classic request),
and I'm happy to ship it with manual `snap connect` for the privileged plugs
rather than ask for auto-connect for now — that also sidesteps the maturity
concern, and I can revisit auto-connect as the project grows.

Two practical questions before I publish:

1. **Arbitrary paths via `system-backup`.** As I understand it, the host root is
   read-only at `/var/lib/snapd/hostfs`, so a source outside `$HOME` would be
   addressed as e.g. `/var/lib/snapd/hostfs/etc/...`. Is prefixing user-supplied
   paths with the hostfs root the expected UX for a backup tool, or is there a
   cleaner pattern you'd recommend? (Home and dotfiles I'll keep at their real
   paths via `home`/`personal-files`.)

2. **Scheduling.** The tool normally installs a cron entry per schedule, which
   strict confinement denies. My plan is to drop in-app scheduling in the snap
   and document a host systemd timer / cron running `snap run moraine backup
   <target>` instead. Is that what other backup snaps do, or is a snap
   `daemon` + `timer` preferred here?

Thanks again for steering me to the right interfaces.

---

## Second reply (closing) — post after Oliver's answers

Perfect, thanks — that settles it.

I'll ship a strict snap now for the common case: home directory, dotfiles via
`personal-files`, and removable media, all at their real paths (so no hostfs
prefix is needed there), scheduled from a host systemd timer / cron running
`snap run moraine backup`.

Whole-system paths via `system-backup` — with the hostfs prefix hidden in the UI,
per your tip — and in-snap scheduling via snap timers, I'll add in a follow-up
release once the app-side path handling is in. Really appreciate the guidance!
