# Changelog

All notable changes to this project are documented here.
The format loosely follows [Keep a Changelog](https://keepachangelog.com/),
and the project uses [semantic versioning](https://semver.org/).

The version string embedded in the binary also includes the git hash and build
date, e.g. `0.1.0 (a1b2c3d, 2026-06-28)` — see `moraine --version`.

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
