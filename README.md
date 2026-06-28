<div align="center">
  <img src="assets/moraine.svg" width="96" alt="Moraine">
  <h1>Moraine</h1>
  <p><strong>Snapshot-baserad backup över SSH/rsync och rclone — CLI + desktop-klient.</strong></p>
</div>

<p align="center"><img src="assets/screenshot.png" width="760" alt="Moraine desktop-klient"></p>

Moraine tar **hårdlänkade snapshots** av dina filer till valfritt mål: en NAS/server
över SSH, eller moln/FTP/SMB/WebDAV/S3/Drive via rclone. Varje körning blir en egen
tidsstämplad snapshot där oförändrade filer delar lagring — full historik till nästan
ingen extra diskkostnad. Återställ hela snapshots eller enstaka filer, schemalägg via
cron, och städa gamla snapshots automatiskt med en retention-policy.

## Funktioner

- **Snapshots** — `<dest>/<namn>/<timestamp>/` med rsync `--link-dest` (hårdlänkar) och en `latest`-pekare.
- **Backends** — `ssh` (rsync över SSH) och `rclone` (moln, **FTP**, SFTP, SMB, WebDAV, S3, Drive, B2 …). FTP är inbyggt: värd/användare/lösenord direkt i appen.
- **Restore** — lista snapshots, bläddra mappträdet, återställ allt eller utvalda filer/mappar.
- **Retention / pruning** (GFS) — behåll N senaste + N dagliga/veckovisa/månatliga; auto-prune efter varje körning.
- **Schemaläggning** — flera scheman per mål, installeras i crontab.
- **Live progress** — rsync-loggen strömmas medan den körs.
- **Körningslogg** — varje backup/restore/prune sparas och visas i en History-flik.
- **Desktop-klient** (iced) med systemtema, native filväljare och en inställnings-modal per mål.

## Installation

### Debian / Ubuntu / Linux Mint
```bash
sudo apt install ./moraine_0.1.0-1_amd64.deb
```
Installerar `moraine` (CLI) och `moraine-gui` (desktop) samt en menypost. Beroenden:
`rsync`, `openssh-client`; rekommenderat: `rclone`, `xdg-desktop-portal`.

### Bygga från källa
```bash
cargo build --release
./target/release/moraine --help
./target/release/moraine-gui
```
Bygg en `.deb`: `cargo install cargo-deb && cargo deb`.

## CLI

```bash
moraine init                       # skapa en exempel-config (moraine.toml)
moraine verify                     # testa SSH/nyckel/källor/dest
moraine run [--target NAMN] [--dry-run]
moraine list --target NAMN         # lista snapshots
moraine prune [--target NAMN] [--dry-run]
```

## Config (`moraine.toml`)

```toml
[[target]]
name    = "nas"
host    = "192.168.1.50"          # IP eller hostname
user    = "backup"
key     = "~/.ssh/id_ed25519"     # valfri, annars ssh-agent
dest    = "/volume1/backups"
sources = ["/home/jonaz/dokument", "/home/jonaz/bilder"]
exclude = ["*.tmp", "node_modules"]

[target.retention]
keep_last = 7
keep_monthly = 6

# rclone-backend (moln/FTP/SMB/WebDAV/S3 …):
# [[target]]
# name = "ftp"
# backend = "ftp"                 # eller "rclone" + host = <rclone-remote>
# host = "ftp.example.com"
# user = "jonaz"
# password = "..."
# dest = "backups"
# sources = ["/home/jonaz/dokument"]
```

Se [`moraine.example.toml`](moraine.example.toml) för en fullständig mall.

## Arkitektur

Ett `moraine`-bibliotek (motor: config, rsync, snapshot, ssh, rclone, prune, history)
plus två binärer (`moraine` CLI, `moraine-gui` desktop). Backenderna kör i dag externa
verktyg (`rsync`/`ssh`/`rclone`); en transport-abstraktion för in-process Rust planeras
för bredare portabilitet (Windows utan rsync, mobil).

## Licens

[MIT](LICENSE) © 2026 Jonaz Thern
