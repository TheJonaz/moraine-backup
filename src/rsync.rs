//! The rsync engine: builds the rsync arguments and runs the snapshot backup over SSH.
//!
//! Each run writes to `<dest>/<name>/<timestamp>/` with
//! `--link-dest=../latest`, so unchanged files become hardlinks against the
//! previous snapshot. After a successful run, `latest` is repointed.
//!
//! The argument building (`build_args`) is shared by the CLI and the desktop client.

use crate::config::{expand_tilde, Target};
use crate::{snapshot, ssh};
use anyhow::{bail, Context, Result};
use std::process::Command;

/// The `--link-dest` value, relative to the snapshot directory: points to `<base>/latest`.
pub const LINK_DEST: &str = "../latest";

/// Builds the argument list for `rsync` (everything except the program name itself).
/// `link_dest` hardlinks unchanged files against a previous snapshot.
pub fn build_args(
    target: &Target,
    remote_dest: &str,
    link_dest: Option<&str>,
    dry_run: bool,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        // -a archive (permissions/times/symlinks), -A ACLs, -X xattrs.
        // --delete mirrors away files that have been removed on the client.
        // --mkpath creates the destination path if it is missing (rsync ≥ 3.2.3).
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

    // SSH transport: port, key if specified, auto-accept new host key.
    args.push("-e".into());
    args.push(ssh::transport(target));

    // Sources on the client (with ~ expanded).
    for src in &target.sources {
        args.push(expand_tilde(src).display().to_string());
    }

    // Target: user@host:path
    args.push(format!("{}:{}", target.ssh_dest(), remote_dest));
    args
}

/// Builds rsync args to **restore** a snapshot to a local directory.
/// Fetches `user@host:<base>/<ts>/` → `local_dest/`. Deliberately WITHOUT `--delete`
/// so nothing in the restore directory is deleted.
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

    // Source: the snapshot directory on the target (trailing / → fetch the contents).
    let remote = format!("{}/{}/", snapshot::base_dir(target), timestamp);
    args.push(format!("{}:{}", target.ssh_dest(), remote));

    // Target: local directory (with ~ expanded).
    args.push(expand_tilde(local_dest).display().to_string());
    args
}

/// Builds rsync args to restore **selected** files/directories from a
/// snapshot. `-R` (--relative) + the `/./` marker preserves the tree structure
/// under `local_dest`. No `--delete`.
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

    // One source per selected path: `<base>/<ts>/./<relative path>`.
    let base = format!("{}/{}", snapshot::base_dir(target), timestamp);
    for p in paths {
        args.push(format!("{}:{}/./{}", target.ssh_dest(), base, p));
    }

    args.push(expand_tilde(local_dest).display().to_string());
    args
}

/// Runs a snapshot backup for a target (CLI). Inherits stdio so rsync writes
/// directly to the terminal. Returns the timestamp on a successful run.
pub fn run_target(target: &Target, dry_run: bool) -> Result<String> {
    let ts = snapshot::timestamp();
    let dest = snapshot::snapshot_dir(target, &ts);
    let args = build_args(target, &dest, Some(LINK_DEST), dry_run);

    println!("$ rsync {}", render(&args));
    let status = Command::new("rsync")
        .args(&args)
        .status()
        .context("could not start rsync — is it installed?")?;
    if !status.success() {
        bail!("rsync failed (exit {})", status.code().unwrap_or(-1));
    }

    if dry_run {
        println!("(dry run: no snapshot created)");
    } else {
        update_latest(target, &ts)?;
        println!("snapshot {ts} complete, latest updated");
    }
    Ok(ts)
}

/// Repoints `<base>/latest` to the new snapshot via ssh.
pub fn update_latest(target: &Target, timestamp: &str) -> Result<()> {
    let cmd = snapshot::update_latest_cmd(target, timestamp);
    let args = ssh::remote_command_args(target, &cmd);
    let status = Command::new("ssh")
        .args(&args)
        .status()
        .context("could not start ssh for latest symlink")?;
    if !status.success() {
        bail!(
            "could not update latest symlink (ssh exit {})",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

/// Renders the arguments readably for printing (not shell-safe quoting).
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
