//! The rsync engine: builds the rsync arguments and runs the snapshot backup over SSH.
//!
//! Each run writes to the hidden work directory `<dest>/<name>/.incomplete-<ts>/`
//! with `--link-dest=../latest`, so unchanged files become hardlinks against the
//! previous snapshot. Only after rsync succeeds is the directory renamed to its
//! final `<timestamp>` name and `latest` repointed (see `snapshot::finalize_cmd`)
//! — an interrupted run can never be mistaken for a complete snapshot.
//!
//! The argument building (`build_args`) is shared by the CLI and the desktop client.

use crate::config::{expand_tilde, Target};
use crate::{snapshot, ssh, tools::CommandExt};
use anyhow::{bail, Context, Result};
use std::process::Command;

/// The `--link-dest` value, relative to the snapshot directory: points to `<base>/latest`.
pub const LINK_DEST: &str = "../latest";

/// A local path as rsync expects it on the command line. On Windows the bundled
/// (msys/cygwin) rsync reads a drive path like `C:\dir` as the remote host `C`,
/// so rewrite it to the msys form `/c/dir`. On Unix — and for any non-drive path
/// — the path is returned unchanged.
fn local_operand(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    #[cfg(windows)]
    {
        let b = s.as_bytes();
        if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
            let drive = (b[0] as char).to_ascii_lowercase();
            let rest = s[2..].replace('\\', "/");
            return format!("/{drive}{rest}");
        }
    }
    s
}

/// Local source paths that don't exist on this machine (with `~` expanded),
/// checked natively — before rsync's own msys-path rewriting. rsync reports a
/// missing source as an opaque `change_dir "/c/…" failed` on Windows; catching
/// it here lets the caller show the real Windows path. Empty vec = all present.
pub fn missing_sources(target: &Target) -> Vec<String> {
    target
        .sources
        .iter()
        .filter(|src| !expand_tilde(src).exists())
        .cloned()
        .collect()
}

/// A caller-facing hint for a source path that isn't on disk. On Windows,
/// `Documents`/`Pictures` are often redirected into OneDrive, so the plain
/// `C:\Users\<you>\Documents` path doesn't exist — the usual cause.
pub fn missing_sources_hint(missing: &[String]) -> String {
    let list = missing.join(", ");
    if cfg!(windows) {
        format!(
            "Source not found on this PC: {list}. Check the exact path — on Windows, \
             Documents/Pictures are often under OneDrive (e.g. C:\\Users\\you\\OneDrive\\Documents)."
        )
    } else {
        format!("Source not found on this machine: {list}.")
    }
}

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
        args.push(local_operand(&expand_tilde(src)));
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
        // The file list comes from the *server* — on restore that side is the
        // less-trusted one (Moraine explicitly supports untrusted destinations).
        // --safe-links skips symlinks whose target points outside the restored
        // tree, so a compromised destination can't plant e.g. `x -> ~/.ssh`
        // and write through it. Trade-off: legitimate absolute symlinks are
        // skipped too (rsync prints each one).
        "--safe-links".into(),
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
    args.push(local_operand(&expand_tilde(local_dest)));
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
        // Same server-supplied-symlink protection as the full restore above.
        "--safe-links".into(),
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

    args.push(local_operand(&expand_tilde(local_dest)));
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
        args.push(local_operand(&expand_tilde(src)));
    }
    let remote = format!("{}/{}/", snapshot::base_dir(target), timestamp);
    args.push(format!("{}:{}", target.ssh_dest(), remote));
    args
}

/// True when an rsync exit code still counts as a complete snapshot. Only 24
/// ("source files vanished during transfer") qualifies — unavoidable on a live
/// system, and the vanished files wouldn't be in the next snapshot either.
/// 23 (partial transfer: unreadable files, I/O errors) is a real failure: data
/// the user expects in the snapshot is missing, and treating it as success
/// would keep healthchecks green while auto-prune eventually deletes the last
/// snapshot that still held the unreadable files.
pub fn vanished_files_only(code: Option<i32>) -> bool {
    code == Some(24)
}

/// Runs a snapshot backup for a target (CLI). Inherits stdio so rsync writes
/// directly to the terminal. Returns the timestamp on a successful run.
pub fn run_target(target: &Target, dry_run: bool) -> Result<String> {
    let missing = missing_sources(target);
    if !missing.is_empty() {
        bail!("{}", missing_sources_hint(&missing));
    }
    let ts = snapshot::timestamp();
    // Real runs write to the hidden work directory and rename on success; a dry
    // run transfers nothing, so it shows the final path.
    let dest = if dry_run {
        snapshot::snapshot_dir(target, &ts)
    } else {
        snapshot::incomplete_dir(target, &ts)
    };
    let args = build_args(target, &dest, Some(LINK_DEST), dry_run);

    println!("$ rsync {}", render(&args));
    let status = Command::new("rsync")
        .no_console()
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
    let code = status.code();
    if !status.success() && !vanished_files_only(code) {
        if code == Some(23) {
            bail!(
                "rsync partial transfer (exit 23) — some source files could not be \
                 read or transferred (permissions? I/O errors?); the snapshot was NOT \
                 finalized and `latest` still points at the previous complete one. \
                 See the rsync errors above for which files."
            );
        }
        bail!("rsync failed (exit {})", code.unwrap_or(-1));
    }
    if vanished_files_only(code) {
        eprintln!(
            "  warning: some source files vanished during the run (rsync exit 24) — \
             normal on a live system; snapshot still created"
        );
    }

    if dry_run {
        println!("(dry run: no snapshot created)");
    } else {
        finalize(target, &ts)?;
        println!("snapshot {ts} complete, latest updated");
    }
    Ok(ts)
}

/// Finalizes a successful snapshot via ssh: renames the work directory to the
/// final timestamp name, repoints `<base>/latest`, and clears stale work
/// directories from earlier interrupted runs.
pub fn finalize(target: &Target, timestamp: &str) -> Result<()> {
    let cmd = snapshot::finalize_cmd(target, timestamp);
    let args = ssh::remote_command_args(target, &cmd);
    let status = Command::new("ssh")
        .no_console()
        .args(&args)
        .envs(ssh::askpass_env(target))
        .status()
        .context("could not start ssh to finalize the snapshot")?;
    if !status.success() {
        bail!(
            "could not finalize the snapshot (ssh exit {}) — it remains as \
             .incomplete-{timestamp} on the target and will not be listed",
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
    fn restores_skip_out_of_tree_symlinks() {
        // The restore file list is server-supplied; --safe-links keeps a
        // compromised destination from planting symlinks out of the tree.
        let full = super::restore_args(&target(""), "ts", "/local", false);
        assert!(full.contains(&"--safe-links".to_string()), "{full:?}");
        let sel = super::restore_selected_args(&target(""), "ts", &["a".into()], "/local", false);
        assert!(sel.contains(&"--safe-links".to_string()), "{sel:?}");
        // Backups go the other way (local → server) and must NOT skip symlinks.
        let up = super::build_args(&target(""), "/d/n/ts", None, false);
        assert!(!up.contains(&"--safe-links".to_string()), "{up:?}");
    }

    #[test]
    fn only_exit_24_is_a_tolerated_partial() {
        // 24 = files vanished mid-run (live system) → still a complete snapshot.
        assert!(super::vanished_files_only(Some(24)));
        // 23 = unreadable/failed files → data is MISSING; must fail the run.
        assert!(!super::vanished_files_only(Some(23)));
        assert!(!super::vanished_files_only(Some(0)));
        assert!(!super::vanished_files_only(None));
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
