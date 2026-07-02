<div align="center">
  <img src="assets/moraine.svg" width="96" alt="Moraine">
  <h1>Moraine</h1>
  <p><strong>Snapshot-based backup over SSH/rsync and rclone — CLI + desktop client.</strong></p>
</div>

<p align="center"><img src="assets/screenshot.png" width="760" alt="Moraine desktop client"></p>

Moraine takes **hardlinked snapshots** of your files to any destination: a NAS/server
over SSH, or cloud/FTP/SMB/WebDAV/S3/Drive via rclone. Every run becomes its own
timestamped snapshot where unchanged files share storage — full history at almost no
extra disk cost. Restore whole snapshots or individual files, schedule via cron, and
prune old snapshots automatically with a retention policy.

## Features

- **Snapshots** — `<dest>/<name>/<timestamp>/` using rsync `--link-dest` (hardlinks) plus a `latest` pointer.
- **Backends** — `ssh` (rsync over SSH) and `rclone` (cloud, **FTP**, SFTP, SMB, WebDAV, S3, Drive, B2 …). FTP is built in: enter host/user/password right in the app.
- **Restore** — list snapshots, browse the file tree, restore everything or selected files/folders.
- **Retention / pruning** (GFS) — keep N latest + N daily/weekly/monthly; auto-prune after each run.
- **Scheduling** — multiple schedules per target, installed into crontab (or Windows Task Scheduler).
- **Live progress** — a progress bar with transferred amount, rate and ETA while a run streams.
- **Run history** — every backup/restore/prune is recorded and shown in a History tab.
- **Per-target VPN** — optionally bring a NetworkManager VPN up before a backup and down after.
- **Desktop client** (GTK 4) with a themed UI, native file pickers, a per-target settings modal, start-at-login and encrypted config export/import.

## Installation

### Debian / Ubuntu / Linux Mint
```bash
sudo apt install ./moraine_0.1.4-1_amd64.deb
```
Installs `moraine` (CLI) and `moraine-gui` (desktop) plus a menu entry. Dependencies:
`rsync`, `openssh-client`; recommended: `rclone`, `xdg-desktop-portal`.

### Build from source
```bash
cargo build --release
./target/release/moraine --help
./target/release/moraine-gui
```
Build a `.deb`: `cargo install cargo-deb && cargo deb`.

## Platform support

Both binaries are pure Rust and build on Linux, macOS and Windows; CI builds and
tests all three on every push, and tagged releases ship a binary archive per OS
(plus a `.deb` for Linux). What each platform needs at runtime:

| Platform    | Build | rsync/SSH backend         | rclone backend | Scheduling           |
|-------------|:-----:|---------------------------|----------------|----------------------|
| Linux       |  ✅   | `rsync` + `openssh-client`| `rclone`       | `crontab` ✅         |
| macOS       |  ✅   | `rsync` + `ssh` (bundled) | `rclone` (brew)| `crontab` ✅         |
| Windows     |  ✅   | needs `rsync`/`ssh`¹      | `rclone`       | Task Scheduler ✅    |

¹ Windows has no bundled rsync; install via WSL, MSYS2 or Git-for-Windows, or
use the rclone backend (SFTP/FTP/SMB/cloud), which needs only the `rclone`
binary on `PATH`.

The **Schedule** tab installs jobs into the platform scheduler automatically:
`crontab` on Linux/macOS, **Windows Task Scheduler** on Windows (each schedule
becomes a task under the `\Moraine\` folder, driven by a small `.cmd` wrapper in
`%APPDATA%\Moraine\tasks\`).

## CLI

```bash
moraine init                       # create an example config (moraine.toml)
moraine verify                     # test SSH/key/sources/dest
moraine run [--target NAME] [--dry-run]
moraine list --target NAME         # list snapshots
moraine prune [--target NAME] [--dry-run]
```

## Config (`moraine.toml`)

```toml
[[target]]
name    = "nas"
host    = "192.168.1.50"          # IP or hostname
user    = "backup"
key     = "~/.ssh/id_ed25519"     # optional, otherwise ssh-agent
dest    = "/volume1/backups"
sources = ["/home/jonaz/documents", "/home/jonaz/pictures"]
exclude = ["*.tmp", "node_modules"]
# vpn   = "home-vpn"              # optional NetworkManager connection to raise for this backup

[target.retention]
keep_last = 7
keep_monthly = 6

# rclone backend (cloud/FTP/SMB/WebDAV/S3 …):
# [[target]]
# name = "ftp"
# backend = "ftp"                 # or "rclone" + host = <rclone-remote>
# host = "ftp.example.com"
# user = "jonaz"
# password = "..."
# dest = "backups"
# sources = ["/home/jonaz/documents"]
```

See [`moraine.example.toml`](moraine.example.toml) for a complete template.

## Architecture

A `moraine` library (engine: config, rsync, snapshot, ssh, rclone, prune, history)
plus two binaries (`moraine` CLI, `moraine-gui` desktop). The backends currently shell
out to external tools (`rsync`/`ssh`/`rclone`); a transport abstraction for in-process
Rust is planned for broader portability (Windows without rsync, mobile).

## Security

Moraine handles credentials for your backup destinations. How they are protected:

- **Secrets on disk are owner-only.** The config (`moraine.toml`) can hold SSH
  passwords / key passphrases and FTP passwords in plaintext, and the run log
  (`history.jsonl`) can contain paths and backend error text — both are written
  with mode `0600` (a pre-existing looser file is tightened on the next write).
  To move a config between machines, use **⚙ Settings → Export config**, which
  encrypts it with a password (gpg, AES-256).
- **Passwords are never passed as command-line arguments** (which are visible to
  other users via `ps`). SSH/rsync authenticate through OpenSSH's `SSH_ASKPASS`
  helper — a tiny script in a **private per-user directory** (`$XDG_RUNTIME_DIR`)
  that reads the secret from the environment; `gpg` reads its passphrase on
  stdin; and `rclone obscure` reads the password on stdin.
- **Host keys.** By default SSH uses `StrictHostKeyChecking=accept-new`: an
  unknown host key is trusted on first connect and pinned in `known_hosts`; a
  later change is rejected. Set `strict_host_key = true` on a target (or tick
  *Require known SSH host key* in its Settings) to require the key to already
  be in `known_hosts`, protecting the first connection too.
- **Scheduling is injection-safe.** Schedule names/targets are rejected if they
  contain control characters and are shell-quoted before being written to
  crontab (or the Windows `.cmd` wrapper), so a crafted or imported config can't
  inject commands.
- **FTP credentials stay out of the process list.** The FTP backend passes
  host/user/password to rclone via `RCLONE_FTP_*` environment variables (the
  environment is private to the process owner), not in the connection string
  on the command line.

## License

[MIT](LICENSE) © 2026 Jonaz Thern
