//! rclone backend: mirrors the rsync engine but targets rclone (cloud/object storage).
//!
//! Same snapshot layout as the SSH backend — `<base>/<timestamp>/<source>/` —
//! but unchanged files are server-side copied via `--copy-dest` (rclone's
//! equivalent to rsync `--link-dest`). `<base>` is either an rclone
//! remote (`remote:path`) or a local path if `host` is empty.

use crate::config::{expand_tilde, Backend, Target};
use crate::{rsync, snapshot};
use anyhow::{bail, Context, Result};
use std::process::Command;

/// The base path for a target in rclone syntax:
///  * Rclone: `remote:dest/name` (or local `dest/name` if host is empty)
///  * Ftp: on-the-fly remote `:ftp:dest/name` — host/user/pass are supplied
///    via environment variables (see [`env_for`]), NOT inline: command-line
///    arguments are world-readable in /proc/*/cmdline, the environment is not.
pub fn base(target: &Target) -> String {
    match target.backend {
        Backend::Ftp => {
            let dest = target.dest.trim_matches('/');
            format!(":ftp:{}/{}", dest, target.name)
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

/// Environment variables carrying the FTP connection details for the `:ftp:`
/// on-the-fly remote (rclone reads `RCLONE_FTP_*` as backend defaults). Empty
/// for other backends. Apply with `.envs(...)` at every rclone spawn site.
pub fn env_for(target: &Target) -> Vec<(String, String)> {
    if !matches!(target.backend, Backend::Ftp) {
        return Vec::new();
    }
    let port = if target.port == 0 { 21 } else { target.port };
    let mut env = vec![
        ("RCLONE_FTP_HOST".to_string(), target.host.trim().to_string()),
        ("RCLONE_FTP_USER".to_string(), target.user.trim().to_string()),
        ("RCLONE_FTP_PORT".to_string(), port.to_string()),
        // disable_mlsd: rclone then creates directories correctly and avoids
        // "501 No such directory" against servers with MLSD quirks (common).
        ("RCLONE_FTP_DISABLE_MLSD".to_string(), "true".to_string()),
    ];
    if !target.password.is_empty() {
        env.push(("RCLONE_FTP_PASS".to_string(), obscure(&target.password)));
    }
    env
}

/// Obscures a password via `rclone obscure` (the FTP backend requires it).
/// The plaintext is fed on **stdin** (`rclone obscure -`), never as a command
/// argument, so it doesn't appear in `ps`/`/proc/*/cmdline`.
fn obscure(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    use std::io::Write;
    let child = Command::new("rclone")
        .args(["obscure", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return String::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(plain.as_bytes());
    }
    child
        .wait_with_output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Path to a snapshot: `<base>/<ts>`.
pub fn snapshot_path(target: &Target, ts: &str) -> String {
    format!("{}/{ts}", base(target))
}

/// Checks that the FTP password can actually be obscured before any command
/// embeds `pass=` in a connection string. Without this, an obscure failure
/// (rclone missing/broken) would silently produce `pass=` → a confusing
/// anonymous-login error instead of the real cause.
pub fn preflight(target: &Target) -> Result<()> {
    if matches!(target.backend, Backend::Ftp)
        && !target.password.is_empty()
        && obscure(&target.password).is_empty()
    {
        bail!("could not obscure the FTP password — is rclone installed and working?");
    }
    Ok(())
}

/// Escapes rclone filter-pattern metacharacters so a literal file name can be
/// used in `--include` (otherwise `*?[]{}` in names match too much/nothing).
fn escape_filter(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    for c in p.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']' | '{' | '}') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn basename(src: &str) -> String {
    expand_tilde(src)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "data".to_string())
}

/// Backup commands: one `rclone copy` per source into `<base>/<ts>/<basename>`,
/// with `--copy-dest <base>/<prev>/<basename>` when a previous snapshot exists.
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
            args.push("-v".to_string()); // per-file output for the live log
            for pat in &target.exclude {
                args.push("--exclude".to_string());
                args.push(pat.clone());
            }
            // --copy-dest server-side copies unchanged files (saves
            // bandwidth). The caller sets `prev` to None for backends without
            // server-side copy (FTP/SMB/WebDAV/local) → full copy instead.
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

/// Arguments that list snapshots (directories) under the base.
pub fn list_args(target: &Target) -> Vec<String> {
    vec!["lsf".into(), "--dirs-only".into(), base(target)]
}

/// Arguments that list a snapshot's contents recursively (directories get `/`).
pub fn tree_args(target: &Target, ts: &str) -> Vec<String> {
    vec!["lsf".into(), "-R".into(), snapshot_path(target, ts)]
}

/// Arguments to delete a snapshot (recursively).
pub fn prune_args(target: &Target, ts: &str) -> Vec<String> {
    vec!["purge".into(), snapshot_path(target, ts)]
}

/// Restore arguments: copies (the whole snapshot or selected paths) from a snapshot
/// to a local directory. Selected paths are filtered with `--include` (matches
/// both files and directory trees), so the structure is preserved.
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
        let esc = escape_filter(p);
        args.push("--include".to_string());
        args.push(format!("/{esc}"));
        args.push("--include".to_string());
        args.push(format!("/{esc}/**"));
    }
    args.push(snapshot_path(target, ts));
    args.push(expand_tilde(local_dest).display().to_string());
    args
}

/// Lists existing snapshots. Empty list if the base does not exist yet.
pub fn list_snapshots(target: &Target) -> Result<Vec<String>> {
    let out = Command::new("rclone")
        .args(list_args(target))
        .envs(env_for(target))
        .output()
        .context("could not start rclone")?;
    if !out.status.success() {
        // The base probably does not exist yet (first run) → empty list.
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().trim_end_matches('/').to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// The fs string for `rclone backend features`: local path, `remote:`, or the
/// on-the-fly `:ftp:` remote (its details come from the environment).
fn features_fs(target: &Target) -> String {
    if matches!(target.backend, Backend::Ftp) {
        return ":ftp:".to_string();
    }
    let host = target.host.trim();
    if host.is_empty() {
        let dest = target.dest.trim_end_matches('/');
        format!("{dest}/{}", target.name)
    } else {
        format!("{host}:")
    }
}

/// Asks rclone whether the backend supports server-side copy (`--copy-dest`).
/// FTP/SMB/WebDAV/local → false; S3/Drive/B2 and others → true.
pub fn supports_server_side_copy(target: &Target) -> bool {
    let out = Command::new("rclone")
        .args(["backend", "features"])
        .arg(features_fs(target))
        .envs(env_for(target))
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

/// Runs the backup (CLI): finds the previous snapshot, runs one copy per source.
/// Uses `--copy-dest` only if the backend supports server-side copy.
/// Inherits stdio. Returns the timestamp.
pub fn run_target(target: &Target, dry_run: bool) -> Result<String> {
    preflight(target)?;
    let ts = snapshot::timestamp();
    // Only real snapshot timestamps qualify as --copy-dest base; a stray
    // directory (or a migrated `latest`) would win a lexicographic max and
    // silently degrade every run to a full copy.
    let prev = list_snapshots(target)?
        .into_iter()
        .filter(|s| snapshot::is_timestamp(s))
        .max();
    // Skip --copy-dest for backends without server-side copy (e.g. FTP).
    let prev_eff = match prev.as_deref() {
        Some(p) if supports_server_side_copy(target) => Some(p),
        _ => None,
    };
    let cmds = backup_cmds(target, &ts, prev_eff, dry_run);
    for (prog, args) in &cmds {
        println!("$ {prog} {}", rsync::render(args));
        let status = Command::new(prog)
            .args(args)
            .envs(env_for(target))
            .status()
            .context("could not start rclone")?;
        if !status.success() {
            bail!("rclone failed (exit {})", status.code().unwrap_or(-1));
        }
    }
    if dry_run {
        println!("(dry run: no snapshot created)");
    } else {
        println!("snapshot {ts} complete");
    }
    Ok(ts)
}

/// Deletes a snapshot via `rclone purge`.
pub fn purge(target: &Target, ts: &str) -> Result<()> {
    let status = Command::new("rclone")
        .args(prune_args(target, ts))
        .envs(env_for(target))
        .status()
        .context("could not start rclone")?;
    if !status.success() {
        bail!("rclone purge failed (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}
