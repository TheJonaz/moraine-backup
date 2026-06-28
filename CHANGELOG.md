# Changelog

All notable changes to this project are documented here.
The format loosely follows [Keep a Changelog](https://keepachangelog.com/),
and the project uses [semantic versioning](https://semver.org/).

The version string embedded in the binary also includes the git hash and build
date, e.g. `0.1.0 (a1b2c3d, 2026-06-28)` — see `moraine --version`.

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
