//! rsync-motorn: bygger rsync-argumenten och kör snapshot-backupen över SSH.
//!
//! Varje körning skriver till `<dest>/<name>/<timestamp>/` med
//! `--link-dest=../latest`, så oförändrade filer blir hårdlänkar mot
//! föregående snapshot. Efter lyckad körning pekas `latest` om.
//!
//! Argumentbygget (`build_args`) delas av CLI:t och desktop-klienten.

use crate::config::{expand_tilde, Target};
use crate::{snapshot, ssh};
use anyhow::{bail, Context, Result};
use std::process::Command;

/// `--link-dest`-värdet, relativt snapshot-katalogen: pekar på `<base>/latest`.
pub const LINK_DEST: &str = "../latest";

/// Bygger argumentlistan till `rsync` (allt utom programnamnet självt).
/// `link_dest` hårdlänkar oförändrade filer mot en tidigare snapshot.
pub fn build_args(
    target: &Target,
    remote_dest: &str,
    link_dest: Option<&str>,
    dry_run: bool,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        // -a arkiv (rättigheter/tider/symlänkar), -A ACL:er, -X xattrs.
        // --delete speglar bort filer som tagits bort på klienten.
        // --mkpath skapar destinationssökvägen om den saknas (rsync ≥ 3.2.3).
        "-aAX".into(),
        "--delete".into(),
        "--mkpath".into(),
        "--human-readable".into(),
    ];

    if dry_run {
        args.push("--dry-run".into());
        args.push("--verbose".into());
    } else {
        args.push("--stats".into());
    }

    for pattern in &target.exclude {
        args.push(format!("--exclude={pattern}"));
    }
    if let Some(ld) = link_dest {
        args.push(format!("--link-dest={ld}"));
    }

    // SSH-transport: port, nyckel om angiven, auto-acceptera ny host-nyckel.
    args.push("-e".into());
    args.push(ssh::transport(target));

    // Källor på klienten (med ~ expanderat).
    for src in &target.sources {
        args.push(expand_tilde(src).display().to_string());
    }

    // Mål: user@host:sökväg
    args.push(format!("{}:{}", target.ssh_dest(), remote_dest));
    args
}

/// Bygger rsync-args för att **återställa** en snapshot till en lokal katalog.
/// Hämtar `user@host:<base>/<ts>/` → `local_dest/`. Medvetet UTAN `--delete`
/// så inget i återställningsmappen raderas.
pub fn restore_args(
    target: &Target,
    timestamp: &str,
    local_dest: &str,
    dry_run: bool,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["-aAX".into(), "--mkpath".into(), "--human-readable".into()];
    if dry_run {
        args.push("--dry-run".into());
        args.push("--verbose".into());
    } else {
        args.push("--stats".into());
    }

    args.push("-e".into());
    args.push(ssh::transport(target));

    // Källa: snapshot-katalogen på målet (avslutande / → hämta innehållet).
    let remote = format!("{}/{}/", snapshot::base_dir(target), timestamp);
    args.push(format!("{}:{}", target.ssh_dest(), remote));

    // Mål: lokal katalog (med ~ expanderat).
    args.push(expand_tilde(local_dest).display().to_string());
    args
}

/// Bygger rsync-args för att återställa **utvalda** filer/mappar ur en
/// snapshot. `-R` (--relative) + `/./`-markören bevarar trädstrukturen
/// under `local_dest`. Ingen `--delete`.
pub fn restore_selected_args(
    target: &Target,
    timestamp: &str,
    paths: &[String],
    local_dest: &str,
    dry_run: bool,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-aAX".into(),
        "-R".into(),
        "--mkpath".into(),
        "--human-readable".into(),
    ];
    if dry_run {
        args.push("--dry-run".into());
        args.push("--verbose".into());
    } else {
        args.push("--stats".into());
    }

    args.push("-e".into());
    args.push(ssh::transport(target));

    // En källa per vald sökväg: `<base>/<ts>/./<relativ sökväg>`.
    let base = format!("{}/{}", snapshot::base_dir(target), timestamp);
    for p in paths {
        args.push(format!("{}:{}/./{}", target.ssh_dest(), base, p));
    }

    args.push(expand_tilde(local_dest).display().to_string());
    args
}

/// Kör snapshot-backup för ett mål (CLI). Ärver stdio så rsync skriver
/// direkt till terminalen. Returnerar timestampen vid lyckad körning.
pub fn run_target(target: &Target, dry_run: bool) -> Result<String> {
    let ts = snapshot::timestamp();
    let dest = snapshot::snapshot_dir(target, &ts);
    let args = build_args(target, &dest, Some(LINK_DEST), dry_run);

    println!("$ rsync {}", render(&args));
    let status = Command::new("rsync")
        .args(&args)
        .status()
        .context("kunde inte starta rsync — är det installerat?")?;
    if !status.success() {
        bail!("rsync misslyckades (exit {})", status.code().unwrap_or(-1));
    }

    if dry_run {
        println!("(dry-run: ingen snapshot skapad)");
    } else {
        update_latest(target, &ts)?;
        println!("snapshot {ts} klar, latest uppdaterad");
    }
    Ok(ts)
}

/// Pekar om `<base>/latest` till den nya snapshoten via ssh.
pub fn update_latest(target: &Target, timestamp: &str) -> Result<()> {
    let cmd = snapshot::update_latest_cmd(target, timestamp);
    let args = ssh::remote_command_args(target, &cmd);
    let status = Command::new("ssh")
        .args(&args)
        .status()
        .context("kunde inte starta ssh för latest-symlänk")?;
    if !status.success() {
        bail!(
            "kunde inte uppdatera latest-symlänk (ssh exit {})",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

/// Återger argumenten läsbart för utskrift (inte shell-säkert citerat).
pub fn render(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.contains(' ') {
                format!("'{a}'")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
