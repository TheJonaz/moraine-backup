# Changelog

All notable changes to this project are documented here.
The format loosely follows [Keep a Changelog](https://keepachangelog.com/),
and the project uses [semantic versioning](https://semver.org/).

The version string embedded in the binary also includes the git hash and build
date, e.g. `0.1.0 (a1b2c3d, 2026-06-28)` — see `moraine --version`.

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
