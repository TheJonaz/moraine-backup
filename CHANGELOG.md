# Changelog

Alla noterbara ändringar i detta projekt dokumenteras här.
Formatet följer löst [Keep a Changelog](https://keepachangelog.com/),
och projektet använder [semantisk versionering](https://semver.org/).

Versionssträngen i binären inkluderar även git-hash och byggdatum, t.ex.
`0.1.0 (a1b2c3d, 2026-06-26)` — se `moraine --version`.

## [Ej släppt]

### Tillagt
- **Körningslogg** — varje backup/restore/prune skrivs till `history.jsonl`
  bredvid config-filen, och visas i en **History-flik** i GUI:t.
- **App-versionering** — `build.rs` bäddar in git-hash och byggdatum;
  visas i `moraine --version` och i GUI:ts header.

## [0.1.0]

### Tillagt
- **Snapshot-backup** över SSH/rsync med hårdlänkade snapshots
  (`<dest>/<namn>/<timestamp>/` + `--link-dest=../latest`) och en
  `latest`-symlänk.
- **CLI**: `init`, `verify` (SSH/nyckel/källor/dest), `run` (med dry-run),
  `list`, `prune`.
- **Retention/pruning** (GFS): behåll N senaste + N dagliga/veckovisa/
  månatliga; auto-prune efter `run`. Planeringslogiken är enhetstestad.
- **Desktop-klient** (iced) med systemtema (ljus/mörk):
  - *Quick Backup* — redigera mål, live-strömmad rsync-logg, Test connection,
    retention + Prune now.
  - *Schedule* — flera scheman, cron-installation, snapshot-antal.
  - *Restore* — lista snapshots, bläddra mappträd, selektiv återställning,
    snapshot-antal.
- Delad motor (config, rsync, snapshot, ssh, prune, history) som lib +
  två binärer (`moraine`, `moraine-gui`).
