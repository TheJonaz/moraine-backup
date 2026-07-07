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
        // --protect-args: send remote paths verbatim, not through the remote
        // shell — so spaces/metacharacters in dest or filenames can't break or
        // inject the remote command.
        "-aAX".into(),
        "--delete".into(),
        "--mkpath".into(),
        "--protect-args".into(),
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
    if !target.bwlimit.trim().is_empty() {
        args.push(format!("--bwlimit={}", target.bwlimit.trim()));
    }
    if let Some(ld) = link_dest {
        args.push(format!("--link-dest={ld}"));
    }

    // SSH transport: port, key if specified, auto-accept new host key.
    args.push("-e".into());
    args.push(ssh::transport(target));

    // `--` ends option parsing: a source or dest path that begins with '-'
    // is then treated as a path, never an rsync flag.
    args.push("--".into());

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
    let mut args: Vec<String> = vec![
        "-aAX".into(),
        "--mkpath".into(),
        "--protect-args".into(),
        "--human-readable".into(),
    ];
    if !target.bwlimit.trim().is_empty() {
        args.push(format!("--bwlimit={}", target.bwlimit.trim()));
    }
    if dry_run {
        args.push("--dry-run".into());
        args.push("--verbose".into());
    } else {
        args.push("--stats".into());
    }

    args.push("-e".into());
    args.push(ssh::transport(target));
    args.push("--".into());

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
        "--protect-args".into(),
        "--human-readable".into(),
    ];
    if !target.bwlimit.trim().is_empty() {
        args.push(format!("--bwlimit={}", target.bwlimit.trim()));
    }
    if dry_run {
        args.push("--dry-run".into());
        args.push("--verbose".into());
    } else {
        args.push("--stats".into());
    }

    args.push("-e".into());
    args.push(ssh::transport(target));
    args.push("--".into());

    // One source per selected path: `<base>/<ts>/./<relative path>`.
    let base = format!("{}/{}", snapshot::base_dir(target), timestamp);
    for p in paths {
        args.push(format!("{}:{}/./{}", target.ssh_dest(), base, p));
    }

    args.push(expand_tilde(local_dest).display().to_string());
    args
}

/// Builds rsync args to **verify** a snapshot against the current sources: a
/// checksum dry-run that itemizes any file whose *content* differs from — or is
/// missing in — the snapshot. Nothing is transferred. `--checksum` compares by
/// hash (not size/mtime), so it catches silent corruption. Excludes are honoured
/// so intentionally-skipped files aren't reported as missing. The caller counts
/// itemize lines starting with `>`/`<` (content transfers) — zero means the
/// snapshot faithfully holds the current sources.
pub fn verify_args(target: &Target, timestamp: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-aAX".into(),
        "--dry-run".into(),
        "--checksum".into(),
        "--itemize-changes".into(),
        "--protect-args".into(),
    ];
    for pattern in &target.exclude {
        args.push(format!("--exclude={pattern}"));
    }
    args.push("-e".into());
    args.push(ssh::transport(target));
    args.push("--".into());
    for src in &target.sources {
        args.push(expand_tilde(src).display().to_string());
    }
    let remote = format!("{}/{}/", snapshot::base_dir(target), timestamp);
    args.push(format!("{}:{}", target.ssh_dest(), remote));
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
        .envs(ssh::askpass_env(target))
        .status()
        .with_context(|| {
            let how = if cfg!(windows) {
                "Windows has no built-in rsync — use the rclone backend, or run Moraine in WSL"
            } else if cfg!(target_os = "macos") {
                "install it with: brew install rsync"
            } else {
                "install it with: apt install rsync (or your package manager)"
            };
            format!("could not start rsync — is it installed? ({how})")
        })?;
    // rsync 23 (partial transfer) / 24 (source files vanished) mean *some*
    // files were skipped, but the snapshot is still valid — treat as success
    // (with a warning) so `latest` is still updated, like rsnapshot does.
    let code = status.code();
    if !status.success() && !matches!(code, Some(23) | Some(24)) {
        bail!("rsync failed (exit {})", code.unwrap_or(-1));
    }
    if matches!(code, Some(23) | Some(24)) {
        eprintln!(
            "  warning: rsync partial transfer (exit {}) — some files were skipped; \
             snapshot still created",
            code.unwrap_or(-1)
        );
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
        .envs(ssh::askpass_env(target))
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

#[cfg(test)]
mod tests {
    use crate::config::{Config, Target};

    fn target(bwlimit: &str) -> Target {
        let cfg: Config = toml::from_str(&format!(
            r#"
            [[target]]
            name = "n"
            host = "h"
            user = "u"
            dest = "/d"
            sources = ["/s"]
            bwlimit = "{bwlimit}"
            "#
        ))
        .unwrap();
        cfg.targets.into_iter().next().unwrap()
    }

    #[test]
    fn bwlimit_reaches_backup_and_restore_args() {
        // Present as --bwlimit=<v> when set.
        let a = super::build_args(&target("2M"), "/d/n/ts", None, false);
        assert!(a.iter().any(|x| x == "--bwlimit=2M"), "backup: {a:?}");
        let r = super::restore_args(&target("2M"), "ts", "/local", false);
        assert!(r.iter().any(|x| x == "--bwlimit=2M"), "restore: {r:?}");
        // Absent when unset.
        let n = super::build_args(&target(""), "/d/n/ts", None, false);
        assert!(
            !n.iter().any(|x| x.starts_with("--bwlimit")),
            "unset: {n:?}"
        );
    }
}
