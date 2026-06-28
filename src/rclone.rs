//! rclone-backend: speglar rsync-motorn men mot rclone (moln/objektlagring).
//!
//! Samma snapshot-layout som SSH-backenden — `<base>/<timestamp>/<källa>/` —
//! men oförändrade filer server-side-kopieras via `--copy-dest` (rclones
//! motsvarighet till rsync `--link-dest`). `<base>` är antingen ett rclone-
//! remote (`remote:path`) eller en lokal sökväg om `host` är tomt.

use crate::config::{expand_tilde, Backend, Target};
use crate::{rsync, snapshot};
use anyhow::{bail, Context, Result};
use std::process::Command;

/// Bas-sökvägen för ett mål i rclone-syntax:
///  * Rclone: `remote:dest/name` (eller lokal `dest/name` om host är tomt)
///  * Ftp: on-the-fly connection-string `:ftp,host=…,user=…,pass=…:dest/name`
pub fn base(target: &Target) -> String {
    match target.backend {
        Backend::Ftp => {
            let dest = target.dest.trim_matches('/');
            let port = if target.port == 0 { 21 } else { target.port };
            let pass = obscure(&target.password);
            // disable_mlsd=true: rclone skapar då kataloger korrekt och undviker
            // "501 No such directory" mot servrar med MLSD-quirks (vanligt).
            format!(
                ":ftp,host={},user={},port={},pass={},disable_mlsd=true:{}/{}",
                target.host.trim(),
                target.user.trim(),
                port,
                pass,
                dest,
                target.name
            )
        }
        _ => {
            let dest = target.dest.trim_end_matches('/');
            let host = target.host.trim();
            if host.is_empty() {
                format!("{dest}/{}", target.name)
            } else {
                format!("{host}:{dest}/{}", target.name)
            }
        }
    }
}

/// Obfuskerar ett lösenord via `rclone obscure` (FTP-backenden kräver det).
fn obscure(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    std::process::Command::new("rclone")
        .args(["obscure", plain])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Sökväg till en snapshot: `<base>/<ts>`.
pub fn snapshot_path(target: &Target, ts: &str) -> String {
    format!("{}/{ts}", base(target))
}

fn basename(src: &str) -> String {
    expand_tilde(src)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "data".to_string())
}

/// Backup-kommandon: ett `rclone copy` per källa in i `<base>/<ts>/<basename>`,
/// med `--copy-dest <base>/<prev>/<basename>` när en tidigare snapshot finns.
pub fn backup_cmds(
    target: &Target,
    ts: &str,
    prev: Option<&str>,
    dry_run: bool,
) -> Vec<(String, Vec<String>)> {
    let base = base(target);
    let snap = snapshot_path(target, ts);
    target
        .sources
        .iter()
        .map(|src| {
            let name = basename(src);
            let mut args = vec!["copy".to_string()];
            if dry_run {
                args.push("--dry-run".to_string());
            }
            args.push("-v".to_string()); // per-fil-utskrift för live-logg
            for pat in &target.exclude {
                args.push("--exclude".to_string());
                args.push(pat.clone());
            }
            // --copy-dest server-side-kopierar oförändrade filer (sparar
            // bandbredd). Anroparen sätter `prev` till None för backends utan
            // server-side copy (FTP/SMB/WebDAV/lokalt) → full kopia istället.
            if let Some(p) = prev {
                args.push("--copy-dest".to_string());
                args.push(format!("{base}/{p}/{name}"));
            }
            args.push(expand_tilde(src).display().to_string());
            args.push(format!("{snap}/{name}"));
            ("rclone".to_string(), args)
        })
        .collect()
}

/// Argument som listar snapshots (kataloger) under basen.
pub fn list_args(target: &Target) -> Vec<String> {
    vec!["lsf".into(), "--dirs-only".into(), base(target)]
}

/// Argument som listar en snapshots innehåll rekursivt (kataloger får `/`).
pub fn tree_args(target: &Target, ts: &str) -> Vec<String> {
    vec!["lsf".into(), "-R".into(), snapshot_path(target, ts)]
}

/// Argument för att radera en snapshot (rekursivt).
pub fn prune_args(target: &Target, ts: &str) -> Vec<String> {
    vec!["purge".into(), snapshot_path(target, ts)]
}

/// Restore-argument: kopierar (hela eller utvalda sökvägar) från en snapshot
/// till en lokal mapp. Utvalda sökvägar filtreras med `--include` (matchar
/// både filer och katalogträd), så strukturen bevaras.
pub fn restore_args(
    target: &Target,
    ts: &str,
    paths: &[String],
    local_dest: &str,
    dry_run: bool,
) -> Vec<String> {
    let mut args = vec!["copy".to_string()];
    if dry_run {
        args.push("--dry-run".to_string());
    }
    args.push("-v".to_string());
    for p in paths {
        args.push("--include".to_string());
        args.push(format!("/{p}"));
        args.push("--include".to_string());
        args.push(format!("/{p}/**"));
    }
    args.push(snapshot_path(target, ts));
    args.push(expand_tilde(local_dest).display().to_string());
    args
}

/// Listar befintliga snapshots. Tom lista om basen inte finns än.
pub fn list_snapshots(target: &Target) -> Result<Vec<String>> {
    let out = Command::new("rclone")
        .args(list_args(target))
        .output()
        .context("kunde inte starta rclone")?;
    if !out.status.success() {
        // Basen finns troligen inte än (första körningen) → tom lista.
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().trim_end_matches('/').to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// fs-strängen för `rclone backend features`: lokal sökväg eller `remote:`.
fn features_fs(target: &Target) -> String {
    let host = target.host.trim();
    if host.is_empty() {
        let dest = target.dest.trim_end_matches('/');
        format!("{dest}/{}", target.name)
    } else {
        format!("{host}:")
    }
}

/// Frågar rclone om backenden stödjer server-side copy (`--copy-dest`).
/// FTP/SMB/WebDAV/lokalt → false; S3/Drive/B2 m.fl. → true.
pub fn supports_server_side_copy(target: &Target) -> bool {
    let out = Command::new("rclone")
        .args(["backend", "features"])
        .arg(features_fs(target))
        .output();
    match out {
        Ok(o) if o.status.success() => serde_json::from_slice::<serde_json::Value>(&o.stdout)
            .ok()
            .and_then(|v| {
                v.get("Features")
                    .and_then(|f| f.get("Copy"))
                    .and_then(|c| c.as_bool())
            })
            .unwrap_or(false),
        _ => false,
    }
}

/// Kör backup (CLI): hittar föregående snapshot, kör en copy per källa.
/// Använder `--copy-dest` bara om backenden stödjer server-side copy.
/// Ärver stdio. Returnerar timestampen.
pub fn run_target(target: &Target, dry_run: bool) -> Result<String> {
    let ts = snapshot::timestamp();
    let prev = list_snapshots(target)?.into_iter().max();
    // Hoppa över --copy-dest för backends utan server-side copy (t.ex. FTP).
    let prev_eff = match prev.as_deref() {
        Some(p) if supports_server_side_copy(target) => Some(p),
        _ => None,
    };
    let cmds = backup_cmds(target, &ts, prev_eff, dry_run);
    for (prog, args) in &cmds {
        println!("$ {prog} {}", rsync::render(args));
        let status = Command::new(prog)
            .args(args)
            .status()
            .context("kunde inte starta rclone")?;
        if !status.success() {
            bail!("rclone misslyckades (exit {})", status.code().unwrap_or(-1));
        }
    }
    if dry_run {
        println!("(dry-run: ingen snapshot skapad)");
    } else {
        println!("snapshot {ts} klar");
    }
    Ok(ts)
}

/// Raderar en snapshot via `rclone purge`.
pub fn purge(target: &Target, ts: &str) -> Result<()> {
    let status = Command::new("rclone")
        .args(prune_args(target, ts))
        .status()
        .context("kunde inte starta rclone")?;
    if !status.success() {
        bail!("rclone purge misslyckades (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}
