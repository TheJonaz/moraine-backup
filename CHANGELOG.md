# Changelog

All notable changes to this project are documented here.
The format loosely follows [Keep a Changelog](https://keepachangelog.com/),
and the project uses [semantic versioning](https://semver.org/).

The version string embedded in the binary also includes the git hash and build
date, e.g. `0.1.0 (a1b2c3d, 2026-06-28)` — see `moraine --version`.

## [Unreleased]

### Added
- **Verify a snapshot** against the current sources by checksum — a new `moraine
  check [--target T] [--snapshot TS]` CLI command and a **Verify** button in the
  Restore tab. It confirms the snapshot faithfully holds your data (catching silent
  transfer corruption): zero differing paths right after a backup means it's intact
  and restorable. rsync/SSH targets use a `--checksum` dry-run; rclone targets use
  `rclone check --one-way`. Differences are expected for an older snapshot whose
  sources have since changed.
- **Per-target bandwidth limit** — a `bwlimit` field (e.g. `2M`, `500K`), set in the
  connection editor or config, passed to rsync/rclone `--bwlimit` for both backup
  and restore. Handy over a VPN or a metered link. Empty = unlimited.
- **Windows installer** — releases now ship a one-click `moraine-<ver>-setup.exe`
  (Inno Setup) that installs the CLI per-user and adds it to PATH, alongside the
  existing zip. The Windows `.exe` also carries the Moraine icon now (embedded via
  a build-script resource) instead of the generic default.

## [0.1.24] — 2026-07-06

### Added
- **Desktop notifications** when a backup finishes — a normal one on success, a
  critical one on failure (so a failed scheduled run doesn't go unnoticed). On by
  default; toggle in Settings → Notifications, or set `notify = false` in the
  config. Uses `notify-send` (libnotify), best-effort — silent if unavailable.
- **Healthcheck pings ("dead man's switch")** per target: an optional URL pinged
  after each backup — the URL on success, `<url>/fail` on failure (the
  healthchecks.io convention). An uptime monitor then alerts you if a *scheduled*
  backup silently stops running — the one failure a desktop notification can't
  catch. Set it per target in the connection editor or via `healthcheck = "…"`.
  Both fire for CLI/cron runs too, so scheduled backups are covered.

## [0.1.23] — 2026-07-06

### Changed
- **Update download happens in-app.** When the update banner offers a new version,
  the Download button now fetches the release in the background with a progress
  bar instead of opening the browser. It picks the asset matching how this build
  was installed — asking `dpkg`/`rpm`/`pacman` which one owns the binary (deb, rpm
  or pkg.tar.zst), falling back to the portable tarball — saves it to your
  Downloads folder, then becomes an **Open** button that hands the file to the
  system installer. A failed download reverts to opening the releases page.

## [0.1.22] — 2026-07-05

### Added
- **Check for updates**: the GUI now checks GitHub Releases on startup and shows a
  dismissable banner when a newer version is out, plus an on-demand "Check for
  updates" button in Settings → About. Works for every install method (deb, rpm,
  Flatpak, tarball, Windows) — not just APT — using the bundled `curl`, so no HTTP
  dependency is added.
- **System-tray icon**: a StatusNotifierItem (via `ksni`) with left-click to
  show/hide and a menu (Show / Quit). Autostart (`--minimized`) now starts hidden
  in the tray. Falls back to the previous present-then-minimize behaviour when no
  tray host is available.
- **Close-to-tray prompt**: pressing the window's X asks whether to minimize to the
  tray or quit, with an optional "remember my choice".
- **Bug & Feedback** submissions now flow to the admin panel on www.thern.io.

### Fixed
- **Window/taskbar icon**: the window now sets its icon explicitly
  (`set_icon_name`), so X11/XFCE shows the real Moraine mark instead of a generic
  fallback (the app-id and desktop-file name differ).

### Changed
- Settings → About: the "Website" link now points to `moraine.thern.io`.

## [0.1.19] — 2026-07-04

### Changed
- **Portable asset paths**: the GUI now resolves its bundled assets (background,
  icons) via `XDG_DATA_DIRS` in addition to `/usr/share`, so it renders correctly
  when installed under a sandbox/prefix — Flatpak (`/app`), Snap, AppImage and Nix.
  Distro packages (deb/rpm/Arch/Alpine) are unaffected.

### Packaging
- **Flatpak** (`packaging/flatpak/`): manifest + AppStream metainfo for the GTK
  app, bundling rsync, the OpenSSH client and rclone into the sandbox.
- **Snap** (`packaging/snap/`): strictly-confined snap with rsync/ssh/rclone
  staged in; CLI + GUI.
- **AppImage** (`packaging/appimage/`): `build-appimage.sh` using
  linuxdeploy + the GTK plugin.
- **Nix** (`packaging/nix/`): flake building CLI + GUI, wrapping rsync/ssh/rclone
  onto the runtime PATH.
- **winget** (`packaging/winget/`): manifests for the Windows CLI (`winget install
  TheJonaz.Moraine`).
- **Alpine** (`packaging/alpine/`): finished the `APKBUILD` — pinned the v0.1.17
  source checksum and added build/publish docs.

## [0.1.18] — 2026-07-03

### Changed
- **Autostart starts minimized**: when the desktop client is launched at login
  (via the "Start Moraine when I log in" autostart entry), it now starts
  iconified to the taskbar instead of popping up and grabbing focus. The
  autostart entry passes `--minimized`; launching Moraine manually is unaffected.
  Users who enabled autostart on an earlier version should toggle it off and back
  on to refresh the entry.

## [0.1.17] — 2026-07-02

### Packaging
- **Arch Linux**: `packaging/aur/PKGBUILD` (test-built on Arch) — install the
  prebuilt package from the release with `pacman -U`, or `makepkg -si` from the
  PKGBUILD; AUR package planned once registration reopens.
- **macOS / Windows CLI**: added a Homebrew formula
  (`packaging/homebrew/moraine.rb`) and a Scoop manifest
  (`packaging/scoop/moraine.json`) for the command-line client. The GTK desktop
  app stays Linux-only.
- **Fedora / RHEL / openSUSE**: added an RPM spec (`packaging/rpm/moraine.spec`,
  test-built in a Fedora container) — install the prebuilt RPM from the release
  with `dnf`, or build via Copr/OBS. Ships both binaries.

## [0.1.16] — 2026-07-02

- Maintenance release: version bump only, no functional or code changes since
  0.1.15.

## [0.1.15] — 2026-07-02

### Changed
- Settings → About: the personal link is now labelled **Website** (was "by
  Jonaz Thern").

## [0.1.14] — 2026-07-02

### Changed
- **Clearer config-import errors.** Importing a file that isn't an encrypted
  config now says "the selected file is not an encrypted Moraine config — pick
  the .gpg file you created with Export config" instead of gpg's cryptic
  "decrypt_message failed: Unknown system error"; a wrong password and a
  corrupt file are also reported plainly. The import dialog defaults to `*.gpg`
  files.

## [0.1.13] — 2026-07-02

### Changed
- **Test connection** no longer reports "connection FAILED" when the connection
  and destination are fine but a local **source** path is missing. It now says
  "connection OK — but N source(s) are missing", annotates each missing source
  with "does not exist on this computer", and the status bar stays neutral
  ("Some checks did not pass") instead of a red failure.

## [0.1.12] — 2026-07-02

### Added
- **Restore destination defaults to the original location.** "Restore to:" is
  pre-filled with the common parent of the target's sources, so restoring
  recreates the files where they were backed up from (e.g. sources under
  `/home/jonaz/…` default to `/home/jonaz`). It stays fully editable — type a
  path or use Browse… to restore elsewhere. Restore never deletes, so it only
  adds/overwrites at the destination.

## [0.1.11] — 2026-07-02

### Added
- **Restore auto-loads snapshots.** Opening the Restore tab now loads the
  selected target's snapshots automatically (when none are loaded yet), instead
  of requiring a click on "Load snapshots".

### Fixed
- **Selective restore of a file/folder now works reliably.** The checked
  selection is tracked in application state instead of being read back off the
  visible tree rows via an unsafe pointer — so a selection survives folder
  navigation and no longer depends on which rows happen to be visible. Stale
  ticks (from a snapshot with a different layout) are ignored. The selection is
  cleared when you pick a different snapshot or target.

## [0.1.10] — 2026-07-02

### Changed
- The two per-source **File…**/**Folder…** buttons are now a single **Browse…**
  button with a small menu (**Files…** / **Folders…**). Both pickers are
  **multi-select**, so you can add many files (or many folders) in one sweep —
  each becomes its own source row. (GTK can't select files *and* folders in the
  same dialog, hence the two menu entries.)

## [0.1.9] — 2026-07-02

### Added
- **Sources can be individual files, not just folders.** Each source row now
  has both a **File…** and a **Folder…** picker (GTK's file dialog can't do
  either in one shot).

### Fixed
- rclone backend: a **file** source is copied with `copyto` instead of `copy`,
  so it lands at `<snapshot>/<name>` instead of the nested `<name>/<name>`.
  (The rsync/SSH backend already handled file sources correctly.)

## [0.1.8] — 2026-07-02

- Maintenance release: version bump only, no functional or code changes since
  0.1.7.

## [0.1.7] — 2026-07-02

Fifth review pass (correctness/edge-case focus). Prune, progress parsing,
cron generation and CLI exit handling were all confirmed clean; the items
below were the real findings.

### Fixed
- **Restore no longer shows a stale list.** If you switch restore target (or
  snapshot) while a listing is still loading, an out-of-order result is now
  discarded instead of overwriting the current selection's snapshots/tree.
- **Uninstalling schedules only removes moraine's own crontab lines.** The
  marker match now requires `# moraine:` (with the colon), so a user's
  unrelated `# moraine …` comment line is left untouched.
- **Reject two sources with the same base name** (e.g. `/a/data` and
  `/b/data`): they would land in the same snapshot subdirectory and silently
  overwrite/merge. Now a clear config error. Unit-tested.
- Honest comment on run-log compaction (a concurrent cross-process append can
  still race; it only affects the advisory log, never backup data).

## [0.1.6] — 2026-07-02

### Fixed
- **rclone backups over a VPN are incremental again.** The previous-snapshot
  lookup for `--copy-dest` now runs inside the worker thread *after* the VPN is
  raised, so a remote reachable only over the target's VPN no longer silently
  falls back to a full re-upload. (Removes the known limitation noted in 0.1.5.)

## [0.1.5] — 2026-07-02

Fourth review pass — two parallel reviews (regression review of the 0.1.3/0.1.4
diff + a fresh adversarial trust-boundary pass), findings verified before fixing.

### Security
- **Argument-injection hardening (imported configs).** A hostile config could
  previously smuggle flags into rsync/rclone: `sources = ["--remove-source-files"]`
  (local data loss) or a `key` like `"x -o ProxyCommand=…"` (the key is
  space-joined into rsync's `-e` string → local command execution). Now:
  `Config::validate()` rejects a key/host/user that starts with `-` or contains
  whitespace/control characters, and every rsync/rclone invocation puts a `--`
  before the positional paths (and uses the combined `--exclude=`/`--include=`
  form). Unit-tested; imported configs are validated too.
- The SSH_ASKPASS helper no longer falls back to a shared `/tmp` path — only
  private per-user dirs; if none exists it just isn't used.
- The worker→UI channel is bounded (backpressure), so a very chatty or hostile
  remote can't grow memory without limit.
- Progress parsing rejects non-finite percentages from crafted remote output.

### Fixed
- **The GUI can no longer persist a config it then can't load.** `State::save`
  validates before writing, and if a config on disk fails to load the GUI warns
  (and points at `moraine.toml.bak`) instead of silently starting empty.
- The stale FTP `,`/`:` host/user check is gone (credentials moved to
  environment variables in 0.1.4), so **IPv6 FTP hosts work** again.
- Backups refuse to run against an empty `dest` (GUI + CLI), and the CLI logs
  that skip to history.
- A second "Run backup" during the rclone previous-snapshot lookup can't spawn
  a parallel run; `load_tree` respects the busy flag.
- History compaction writes atomically (temp file + rename).
- Delete-target / export surface a save error instead of swallowing it.

### Known limitation
- An rclone remote reachable *only* over a target's VPN falls back to a full
  copy (the previous-snapshot lookup runs before the VPN is raised). Correct,
  just not incremental — a rare combination.

## [0.1.4] — 2026-07-02

Closes the two remaining documented security tradeoffs, plus polish.

### Security
- **FTP credentials no longer appear in the process list**: rclone gets
  host/user/password via `RCLONE_FTP_*` environment variables (private to the
  process owner) instead of the `:ftp,…,pass=…:` connection string in argv.
- **Optional strict SSH host-key checking**: set `strict_host_key = true` on a
  target (or tick *Require known SSH host key* in its Settings) for
  `StrictHostKeyChecking=yes` — protects even the first connection. The
  default remains `accept-new` (trust on first use, reject changes).

### Fixed / improved
- Form validation with feedback: a typo'd port or hour/minute no longer
  silently becomes 22/0 — saving (and running) reports the invalid field.
- Out-of-range schedule times are rejected when the config loads, instead of
  being silently clamped.
- "Install to crontab" skips schedules whose target no longer exists (and
  says so) instead of installing jobs that would fail every night.
- The Targets list shows the snapshot count after snapshots are loaded.
- Windows: the Startup card is hidden (desktop autostart is a Linux thing)
  and the background-image URI handles `\` paths.

## [0.1.3] — 2026-07-02

Third security/bug review pass (two independent reviews of the GUI and the
engine, findings verified before fixing).

### Security
- **Target names are validated**: a name is used as a folder under `dest` and
  interpolated into remote commands, so `../`, `\`, control characters and the
  reserved names `.`, `..`, `latest` are now rejected (they could traverse
  outside the destination or hijack the `latest` pointer). For FTP targets,
  `,`/`:` in host/user are rejected — they could inject rclone options into the
  connection string. Unit-tested; an imported config is validated too.
- Restore file trees are sanitized: entries from the server containing `..` or
  absolute paths are dropped (a malicious server could otherwise steer a
  selective restore outside the chosen folder).
- `moraine init` writes the example config owner-only (0600) — users add
  passwords to that file in place.
- The encrypted config export is written 0600 (gpg's own output mode wasn't).
- `nmcli` is invoked with an explicit `id` argument so a VPN name starting
  with `-` can't be parsed as an option.
- Reproducible builds: the embedded build date honors `SOURCE_DATE_EPOCH`.

### Fixed
- **"+ New schedule" crashed the app** (RefCell double-borrow) — fixed.
- **SSH restore tree was unusable**: the file listing was parsed in the wrong
  format, so folders didn't expand and selective restore built wrong paths.
- GUI rclone backups are now **incremental** (`--copy-dest`), like the CLI —
  previously every GUI rclone backup re-uploaded everything.
- The previous-snapshot lookup ignores stray directories (only real timestamps
  qualify), so `--copy-dest` can't silently point at garbage.
- Failed runs are now recorded in History (previously only successes), and
  prune entries include the target name.
- Prune/test/load-snapshots won't start while a backup is running.
- A VPN that was already connected before a run is left up afterwards.
- The Startup autostart entry pins the working directory, so a login-started
  instance finds the same `moraine.toml`.
- Dry runs show the file list again (aggregate progress had hidden it).
- rclone selective restore escapes glob characters in file names.
- FTP: a broken/missing rclone now gives a clear error instead of a silent
  anonymous-login attempt (obscure preflight).
- CLI `prune` continues past a failing target and logs the failure; history
  entries keep the full error chain.
- The run log (`history.jsonl`) is capped (~1 MiB → newest 2000 entries kept).
- `~` alone now expands in paths; the GUI keeps a rolling `moraine.toml.bak`
  before every save.

## [0.1.2] — 2026-07-02

### Desktop app
- **Per-target VPN**: pick a NetworkManager connection in a target's Settings;
  Moraine brings it up before the backup and down afterwards. Scheduled (cron)
  runs raise the VPN too.
- **Start at login**: a Startup toggle in Settings writes/removes a desktop
  autostart entry (`~/.config/autostart/moraine-gui.desktop`).
- Dark navy-blue theme for text fields and buttons to match the app, and a
  visible grid background (fixed a `file://` URI issue that had hidden it).
- The ✕ button in the Sources/Exclude editors now actually removes the row.
- Clearer failure diagnostics in the log (e.g. a source folder that can't be
  read explains the permission problem and how to fix it).

### Security
- The config (`moraine.toml`) and run log (`history.jsonl`) are written
  **owner-only (0600)** — both can contain plaintext secrets or paths; a
  pre-existing looser file is tightened on the next write.
- The `SSH_ASKPASS` helper moved from a predictable shared `/tmp` path to a
  **private per-user directory** (`$XDG_RUNTIME_DIR`) and is always rewritten,
  so a local attacker can't pre-plant a script that would receive the secret.
- Secrets are never passed as command-line arguments: `rclone obscure` now reads
  the password on stdin.
- rsync now uses **`--protect-args`**, so remote paths/filenames with spaces or
  shell metacharacters aren't reinterpreted by the remote shell.
- Schedule names/targets are validated (no control characters) and shell-quoted
  before being written to crontab / the Windows `.cmd` wrapper, so a crafted or
  imported config can't inject commands.

### Fixed
- rsync partial-transfer exits (23/24) are treated as a valid snapshot, so
  `latest` is still updated instead of the whole run failing (e.g. when a source
  folder is unreadable).
- The `latest` pointer is no longer listed as a snapshot.
- Hardened snapshot-index accesses against a stale selection (no more potential
  panic if the snapshot list changed under the selection).
- Fixed a UI freeze on very large backups caused by flooding the log; the GUI
  now shows aggregate progress instead of every file and caps the log buffer.

## [0.1.1] — 2026-06-29

### Cross-platform
- Builds and tests run on **Linux, macOS and Windows** in CI (plus `fmt` +
  `clippy` gate); tagged releases ship a binary archive per OS and a Linux `.deb`.
- The `rfd` file-dialog dependency is split per target (xdg-portal on Linux,
  native dialogs on Windows/macOS) so the GUI compiles everywhere.
- **Scheduling is now cross-platform**: the Schedule tab installs to `crontab`
  on Linux/macOS and to **Windows Task Scheduler** (`schtasks`, via per-schedule
  `.cmd` wrappers under `%APPDATA%\Moraine\`) on Windows.

### Fixed
- Scheduled jobs referenced a non-existent `backup` binary after the rename;
  they now invoke `moraine` (`moraine.exe` on Windows) next to the GUI.

### Changed
- Source comments and CLI messages are now fully English.

### Desktop app
- The GUI is rewritten on **GTK 4** (was iced/wgpu). GTK 4 and async-channel are
  packaged in Debian, so the desktop app can now ship in official Debian
  alongside the CLI — in the **same package** (`moraine` provides both
  `/usr/bin/moraine` and `/usr/bin/moraine-gui`). Run whichever you prefer.

### Packaging
- The desktop dependencies are behind a default `gui` feature, so
  `cargo build --no-default-features` builds just the `moraine` CLI.
- Full Debian packaging (dh-cargo): builds both binaries against Debian's GTK 4
  crates; ships the `.desktop` entry, icon and manpages; verified with sbuild,
  lintian and autopkgtest.
- Bumped `toml` 0.8 → 1, matching the version in Debian.

## [0.1.0]

### Core
- **Snapshot backup** over SSH/rsync with hardlinked snapshots
  (`<dest>/<name>/<timestamp>/` + `--link-dest=../latest`) and a `latest` symlink.
- **Backends** — `ssh` (rsync over SSH), `rclone` (cloud, SFTP, SMB, WebDAV, S3,
  Drive, B2 …) and `ftp` (rclone's FTP backend, credentials entered in the app).
- **Retention / pruning** (GFS): keep N latest + N daily/weekly/monthly; auto-prune
  after a successful run. Planning logic is unit-tested.
- **Run history** — each backup/restore/prune is appended to `history.jsonl` next to
  the config file.
- **App versioning** — `build.rs` embeds the git hash and build date.

### CLI (`moraine`)
- `init`, `verify` (SSH/key/sources/dest), `run` (with `--dry-run`), `list`, `prune`.

### Desktop client (`moraine-gui`)
- System light/dark theme, native window/app icon.
- **Quick Backup** — edit targets, live-streamed rsync log, Test connection.
- **Schedule** — multiple schedules per target, crontab install, snapshot counts.
- **Restore** — list snapshots, browse the file tree, selective restore, snapshot counts.
- **History** — view the run log.
- Per-target **settings modal** (gear icon) for the advanced fields, including an
  inline filtered **schedule editor**.
- Native **file pickers** for the SSH key, sources and restore destination.
- Per-row delete confirmation in the target list.

### Engine
- Shared `moraine` library (config, rsync, snapshot, ssh, rclone, prune, history)
  plus two binaries (`moraine`, `moraine-gui`).
- Debian packaging via `cargo-deb`.
