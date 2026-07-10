//! rclone backend: mirrors the rsync engine but targets rclone (cloud/object storage).
//!
//! Same snapshot layout as the SSH backend — `<base>/<timestamp>/<source>/` —
//! but unchanged files are server-side copied via `--copy-dest` (rclone's
//! equivalent to rsync `--link-dest`). `<base>` is either an rclone
//! remote (`remote:path`) or a local path if `host` is empty.
//!
//! Completeness marker: renaming a directory is expensive or impossible on
//! object stores (S3 has no rename), so instead a `<ts>.incomplete` marker
//! file is created next to the snapshot directory before the first byte is
//! copied and deleted only after every source finished. A snapshot whose
//! marker still exists is invisible to list/check/restore/prune; leftovers
//! from interrupted runs are removed by [`cleanup_stale`].

use crate::config::{expand_tilde, Backend, Target};
use crate::{rsync, snapshot, tools::CommandExt};
use anyhow::{bail, Context, Result};
use std::process::Command;

/// Name of the on-the-fly `rclone crypt` remote we define via the environment
/// when a target encrypts its destination at rest. Its `remote=` (the underlying,
/// unencrypted location) and passphrase are supplied in [`env_for`].
const CRYPT_REMOTE: &str = "mcrypt";

/// The unencrypted base path for a target in rclone syntax:
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

/// The location the `mcrypt:` crypt remote roots at — the same as [`base`] but
/// *without* the trailing `/<name>`, so `mcrypt:<name>` maps onto `<dest>/<name>`
/// with every path segment beneath it encrypted.
fn crypt_underlying(target: &Target) -> String {
    match target.backend {
        Backend::Ftp => format!(":ftp:{}", target.dest.trim_matches('/')),
        _ => {
            let dest = target.dest.trim_end_matches('/');
            let host = target.host.trim();
            if host.is_empty() {
                dest.to_string()
            } else {
                format!("{host}:{dest}")
            }
        }
    }
}

/// The base every operation actually reads/writes: the encrypting `mcrypt:<name>`
/// remote when the target enables crypt, otherwise the plain [`base`]. Routing all
/// paths through here means backup, restore, list, verify and prune are all
/// transparently encrypted with no other changes.
pub fn effective_base(target: &Target) -> String {
    if target.crypt_enabled() {
        format!("{CRYPT_REMOTE}:{}", target.name)
    } else {
        base(target)
    }
}

/// Environment variables carrying the FTP connection details for the `:ftp:`
/// on-the-fly remote (rclone reads `RCLONE_FTP_*` as backend defaults). Empty
/// for other backends. Apply with `.envs(...)` at every rclone spawn site.
pub fn env_for(target: &Target) -> Vec<(String, String)> {
    let mut env = Vec::new();
    // FTP connection details — as backend defaults, they also apply when a crypt
    // remote wraps an `:ftp:` underlying remote.
    if matches!(target.backend, Backend::Ftp) {
        let port = if target.port == 0 { 21 } else { target.port };
        env.push((
            "RCLONE_FTP_HOST".to_string(),
            target.host.trim().to_string(),
        ));
        env.push((
            "RCLONE_FTP_USER".to_string(),
            target.user.trim().to_string(),
        ));
        env.push(("RCLONE_FTP_PORT".to_string(), port.to_string()));
        // disable_mlsd: rclone then creates directories correctly and avoids
        // "501 No such directory" against servers with MLSD quirks (common).
        env.push(("RCLONE_FTP_DISABLE_MLSD".to_string(), "true".to_string()));
        if !target.password.is_empty() {
            env.push(("RCLONE_FTP_PASS".to_string(), obscure(&target.password)));
        }
    }
    // Destination encryption: define the `mcrypt:` crypt remote on the fly, rooted
    // at the plain (unencrypted) location. Passphrases go in the environment,
    // obscured — never on the command line (world-readable in /proc/*/cmdline).
    if target.crypt_enabled() {
        let p = "RCLONE_CONFIG_MCRYPT_";
        env.push((format!("{p}TYPE"), "crypt".to_string()));
        env.push((format!("{p}REMOTE"), crypt_underlying(target)));
        env.push((format!("{p}PASSWORD"), obscure(&target.crypt_password)));
        if !target.crypt_salt.trim().is_empty() {
            env.push((format!("{p}PASSWORD2"), obscure(&target.crypt_salt)));
        }
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
        .no_console()
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
    format!("{}/{ts}", effective_base(target))
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
    if target.crypt_enabled() && obscure(&target.crypt_password).is_empty() {
        bail!("could not obscure the encryption passphrase — is rclone installed and working?");
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

/// Backup commands: one rclone copy per source into `<base>/<ts>/<basename>`,
/// with `--copy-dest <base>/<prev>/<basename>` when a previous snapshot exists.
/// A directory source uses `copy` (dest is a directory); a *file* source uses
/// `copyto` (`copy FILE dir/name` would nest it as `name/name`).
pub fn backup_cmds(
    target: &Target,
    ts: &str,
    prev: Option<&str>,
    dry_run: bool,
) -> Vec<(String, Vec<String>)> {
    let base = effective_base(target);
    let snap = snapshot_path(target, ts);
    target
        .sources
        .iter()
        .map(|src| {
            let name = basename(src);
            let expanded = expand_tilde(src);
            let is_file = expanded.is_file();
            let mut args = vec![if is_file { "copyto" } else { "copy" }.to_string()];
            if dry_run {
                args.push("--dry-run".to_string());
            }
            args.push("-v".to_string()); // per-file output for the live log
            for pat in &target.exclude {
                // Combined form: a pattern starting with '-' stays part of the
                // value, never parsed as a flag.
                args.push(format!("--exclude={pat}"));
            }
            if !target.bwlimit.trim().is_empty() {
                args.push(format!("--bwlimit={}", target.bwlimit.trim()));
            }
            // --copy-dest server-side copies unchanged files (saves bandwidth).
            // Only meaningful for directory copies; the caller sets `prev` to
            // None for backends without server-side copy (FTP/SMB/WebDAV/local).
            if !is_file {
                if let Some(p) = prev {
                    args.push("--copy-dest".to_string());
                    args.push(format!("{base}/{p}/{name}"));
                }
            }
            // `--` ends flag parsing: a source/dest path beginning with '-' is
            // then a path, not an rclone flag.
            args.push("--".to_string());
            args.push(expanded.display().to_string());
            args.push(format!("{snap}/{name}"));
            ("rclone".to_string(), args)
        })
        .collect()
}

/// Path to the marker file that flags a snapshot as still being written:
/// `<base>/<ts>.incomplete`, a sibling of the snapshot directory.
fn incomplete_marker(target: &Target, ts: &str) -> String {
    format!("{}/{ts}.incomplete", effective_base(target))
}

/// Arguments that create the in-progress marker (an empty file).
pub fn marker_create_args(target: &Target, ts: &str) -> Vec<String> {
    vec!["touch".into(), incomplete_marker(target, ts)]
}

/// Arguments that remove the in-progress marker — the commit point that makes
/// the snapshot visible.
pub fn marker_delete_args(target: &Target, ts: &str) -> Vec<String> {
    vec!["deletefile".into(), incomplete_marker(target, ts)]
}

/// Arguments that list everything (files and directories) under the base —
/// directories get a trailing `/`, so snapshot dirs and `.incomplete` markers
/// come back in one call. Parsed by [`parse_listing`].
pub fn list_args(target: &Target) -> Vec<String> {
    vec!["lsf".into(), effective_base(target)]
}

/// What an `lsf` listing of the base contains.
#[derive(Default)]
struct Listing {
    /// Snapshot directories with no `.incomplete` marker — safe to use.
    complete: Vec<String>,
    /// Timestamps whose marker exists (dir may or may not): interrupted runs.
    stale: Vec<Stale>,
}

struct Stale {
    ts: String,
    has_dir: bool,
}

/// Splits an `lsf` listing into complete snapshots and stale (marker still
/// present) ones. Pure, for testability.
fn parse_listing(text: &str) -> Listing {
    let mut dirs: Vec<String> = Vec::new();
    let mut markers: Vec<String> = Vec::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(d) = l.strip_suffix('/') {
            dirs.push(d.to_string());
        } else if let Some(ts) = l.strip_suffix(".incomplete") {
            markers.push(ts.to_string());
        }
    }
    // Only timestamp-shaped markers are ours. A stray `X.incomplete` file next
    // to the snapshots (a common downloader suffix) must not flag — and later
    // get the unrelated directory `X` recursively purged by — cleanup_stale.
    markers.retain(|ts| snapshot::is_timestamp(ts));
    let stale: Vec<Stale> = markers
        .into_iter()
        .map(|ts| {
            let has_dir = dirs.contains(&ts);
            Stale { ts, has_dir }
        })
        .collect();
    dirs.retain(|d| !stale.iter().any(|s| s.ts == *d));
    Listing {
        complete: dirs,
        stale,
    }
}

/// Fetches and parses the base listing. Empty on any rclone failure (the base
/// probably does not exist yet — first run).
fn listing(target: &Target) -> Result<Listing> {
    let out = Command::new("rclone")
        .no_console()
        .args(list_args(target))
        .envs(env_for(target))
        .output()
        .context("could not start rclone")?;
    if !out.status.success() {
        return Ok(Listing::default());
    }
    Ok(parse_listing(&String::from_utf8_lossy(&out.stdout)))
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
    if !target.bwlimit.trim().is_empty() {
        args.push(format!("--bwlimit={}", target.bwlimit.trim()));
    }
    for p in paths {
        let esc = escape_filter(p);
        args.push(format!("--include=/{esc}"));
        args.push(format!("--include=/{esc}/**"));
    }
    args.push("--".to_string());
    args.push(snapshot_path(target, ts));
    args.push(expand_tilde(local_dest).display().to_string());
    args
}

/// Lists existing **complete** snapshots (a directory whose `.incomplete`
/// marker is gone). Empty list if the base does not exist yet.
pub fn list_snapshots(target: &Target) -> Result<Vec<String>> {
    Ok(listing(target)?.complete)
}

/// Best-effort removal of snapshots left behind by interrupted runs (their
/// `.incomplete` marker still present). Skips `current_ts` (pass `""` when
/// there is no run in flight). Errors are printed, never propagated — cleanup
/// must not fail an otherwise-successful backup or prune. Returns the cleaned
/// timestamps.
pub fn cleanup_stale(target: &Target, current_ts: &str) -> Vec<String> {
    let stale = match listing(target) {
        Ok(l) => l.stale,
        Err(_) => return Vec::new(),
    };
    let mut cleaned = Vec::new();
    // Belt-and-braces on top of parse_listing's marker filter: only ever
    // delete timestamp-shaped names.
    for s in stale
        .iter()
        .filter(|s| s.ts != current_ts && snapshot::is_timestamp(&s.ts))
    {
        if s.has_dir {
            if let Err(e) = purge(target, &s.ts) {
                eprintln!(
                    "  warning: could not remove interrupted snapshot {}: {e:#}",
                    s.ts
                );
                continue; // keep the marker so it stays hidden and is retried later
            }
        }
        let status = Command::new("rclone")
            .no_console()
            .args(marker_delete_args(target, &s.ts))
            .envs(env_for(target))
            .status();
        match status {
            Ok(st) if st.success() => cleaned.push(s.ts.clone()),
            _ => eprintln!("  warning: could not remove marker {}.incomplete", s.ts),
        }
    }
    cleaned
}

/// The fs string for `rclone backend features`: local path, `remote:`, or the
/// on-the-fly `:ftp:` remote (its details come from the environment).
fn features_fs(target: &Target) -> String {
    // Query the crypt remote itself — its reported features (incl. server-side
    // Copy) mirror the underlying remote's.
    if target.crypt_enabled() {
        return format!("{CRYPT_REMOTE}:");
    }
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
        .no_console()
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

/// The full command sequence for one backup run: create the in-progress marker,
/// one copy per source, then delete the marker (the commit point — only then is
/// the snapshot visible). Dry runs skip the marker commands. Does the
/// previous-snapshot lookup (a network call) — callers that raise a VPN must
/// call this only after it is up. Shared by the CLI and the desktop client.
pub fn run_cmds(target: &Target, ts: &str, dry_run: bool) -> Vec<(String, Vec<String>)> {
    // Only real snapshot timestamps qualify as --copy-dest base; a stray
    // directory (or a migrated `latest`) would win a lexicographic max and
    // silently degrade every run to a full copy. Incomplete snapshots are
    // already filtered out by list_snapshots.
    let prev = list_snapshots(target)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| snapshot::is_timestamp(s))
        .max()
        // Skip --copy-dest for backends without server-side copy (e.g. FTP).
        .filter(|_| supports_server_side_copy(target));
    let mut cmds = Vec::new();
    if !dry_run {
        cmds.push(("rclone".to_string(), marker_create_args(target, ts)));
    }
    cmds.extend(backup_cmds(target, ts, prev.as_deref(), dry_run));
    if !dry_run {
        cmds.push(("rclone".to_string(), marker_delete_args(target, ts)));
    }
    cmds
}

/// Runs the backup (CLI): marker, one copy per source, marker removal — then a
/// best-effort cleanup of older interrupted runs. Inherits stdio. Returns the
/// timestamp.
pub fn run_target(target: &Target, dry_run: bool) -> Result<String> {
    preflight(target)?;
    let ts = snapshot::timestamp();
    let cmds = run_cmds(target, &ts, dry_run);
    for (prog, args) in &cmds {
        println!("$ {prog} {}", rsync::render(args));
        let status = Command::new(prog)
            .no_console()
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
        let cleaned = cleanup_stale(target, &ts);
        if !cleaned.is_empty() {
            println!("  removed {} interrupted snapshot(s)", cleaned.len());
        }
        println!("snapshot {ts} complete");
    }
    Ok(ts)
}

/// Deletes a snapshot via `rclone purge`.
pub fn purge(target: &Target, ts: &str) -> Result<()> {
    let status = Command::new("rclone")
        .no_console()
        .args(prune_args(target, ts))
        .envs(env_for(target))
        .status()
        .context("could not start rclone")?;
    if !status.success() {
        bail!("rclone purge failed (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::{Config, Target};

    fn target(toml_body: &str) -> Target {
        let cfg: Config = toml::from_str(&format!(
            r#"
            [[target]]
            name = "n"
            backend = "rclone"
            host = "remote"
            dest = "/backups"
            sources = ["/s"]
            {toml_body}
            "#
        ))
        .unwrap();
        cfg.targets.into_iter().next().unwrap()
    }

    #[test]
    fn listing_hides_snapshots_whose_incomplete_marker_remains() {
        let l = super::parse_listing(
            "2026-01-01T00-00-00/\n\
             2026-01-02T00-00-00/\n\
             2026-01-02T00-00-00.incomplete\n\
             2026-01-03T00-00-00.incomplete\n\
             latest/\n",
        );
        // The marked snapshot is hidden; the orphan marker (crash before any
        // copy) is stale without a dir; stray dirs still list (filtered by
        // is_timestamp at the use sites).
        assert_eq!(l.complete, vec!["2026-01-01T00-00-00", "latest"]);
        let stale: Vec<(&str, bool)> = l.stale.iter().map(|s| (s.ts.as_str(), s.has_dir)).collect();
        assert_eq!(
            stale,
            vec![
                ("2026-01-02T00-00-00", true),
                ("2026-01-03T00-00-00", false)
            ]
        );
    }

    #[test]
    fn stray_incomplete_files_never_mark_unrelated_dirs_stale() {
        // ".incomplete" is a common downloader suffix. A user's own
        // "archive.incomplete" next to the snapshots must NOT flag the
        // unrelated "archive/" directory for deletion by cleanup_stale.
        let l = super::parse_listing(
            "2026-01-01T00-00-00/\n\
             archive/\n\
             archive.incomplete\n",
        );
        assert!(
            l.stale.is_empty(),
            "stray marker must not create stale work"
        );
        assert_eq!(l.complete, vec!["2026-01-01T00-00-00", "archive"]);
    }

    #[test]
    fn marker_paths_route_through_the_effective_base() {
        let plain = target("");
        assert_eq!(
            super::marker_create_args(&plain, "TS"),
            vec!["touch", "remote:/backups/n/TS.incomplete"]
        );
        // Under crypt the marker lives in the encrypted remote too.
        let enc = target(r#"crypt_password = "hunter2""#);
        assert_eq!(
            super::marker_delete_args(&enc, "TS"),
            vec!["deletefile", "mcrypt:n/TS.incomplete"]
        );
    }

    #[test]
    fn dry_run_cmds_have_no_marker_commands() {
        // A dry run copies nothing, so it must not create (or delete) markers.
        let t = target("");
        let cmds = super::run_cmds(&t, "TS", true);
        assert!(cmds
            .iter()
            .all(|(_, args)| args.iter().all(|a| !a.contains(".incomplete"))));
        assert!(cmds
            .iter()
            .all(|(_, args)| args.contains(&"--dry-run".to_string())));
    }

    #[test]
    fn crypt_routes_paths_through_the_crypt_remote() {
        // No crypt → plain remote path.
        let plain = target("");
        assert_eq!(super::effective_base(&plain), "remote:/backups/n");
        assert_eq!(super::snapshot_path(&plain, "TS"), "remote:/backups/n/TS");
        assert!(super::env_for(&plain).is_empty());

        // Crypt on → mcrypt: remote, rooted at the plain dest (minus /name).
        let enc = target(r#"crypt_password = "hunter2""#);
        assert_eq!(super::effective_base(&enc), "mcrypt:n");
        assert_eq!(super::snapshot_path(&enc, "TS"), "mcrypt:n/TS");
        assert_eq!(super::crypt_underlying(&enc), "remote:/backups");
    }

    #[test]
    fn crypt_env_defines_the_remote_with_obscured_secrets() {
        let enc = target(
            r#"
            crypt_password = "hunter2"
            crypt_salt = "pepper"
            "#,
        );
        let env: std::collections::HashMap<_, _> = super::env_for(&enc).into_iter().collect();
        assert_eq!(env.get("RCLONE_CONFIG_MCRYPT_TYPE").unwrap(), "crypt");
        assert_eq!(
            env.get("RCLONE_CONFIG_MCRYPT_REMOTE").unwrap(),
            "remote:/backups"
        );
        // The obscuring itself needs the `rclone` binary; skip that assertion where
        // it isn't installed (e.g. a minimal CI image) but still require the keys.
        assert!(env.contains_key("RCLONE_CONFIG_MCRYPT_PASSWORD"));
        assert!(env.contains_key("RCLONE_CONFIG_MCRYPT_PASSWORD2"));
        if !super::obscure("probe").is_empty() {
            // Passphrases are obscured (rclone's reversible config obfuscation),
            // never the plaintext.
            let pw = env.get("RCLONE_CONFIG_MCRYPT_PASSWORD").unwrap();
            assert!(pw != "hunter2", "password stored in plaintext: {pw}");
        }
    }
}
