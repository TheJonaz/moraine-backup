//! Snapshot naming and `latest` handling on the target.
//!
//! Layout on the target:
//! ```text
//! <dest>/<name>/
//!   2026-06-24T20-30-00/      ← full tree structure
//!   2026-06-24T21-30-00/      ← unchanged files are hardlinks against the previous
//!   latest -> 2026-06-24T21-30-00
//! ```
//!
//! A snapshot is written to a hidden work directory (`.incomplete-<timestamp>/`)
//! and only renamed to its final timestamp name once rsync finished — so an
//! interrupted or failed run can never be mistaken for a complete snapshot by
//! list/check/restore/prune. `ls -1` (the listing command) never shows the
//! dot-prefixed work directories.

use crate::config::Target;
use chrono::Local;

/// Timestamp for a new snapshot folder, e.g. `2026-06-24T20-30-00`.
/// `:` is avoided so the name is safe on all filesystems.
pub fn timestamp() -> String {
    Local::now().format("%Y-%m-%dT%H-%M-%S").to_string()
}

/// True if `s` parses exactly as a snapshot timestamp. Used to filter remote
/// listings so stray entries (`latest`, other directories) are never mistaken
/// for snapshots.
pub fn is_timestamp(s: &str) -> bool {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H-%M-%S").is_ok()
}

/// The base directory for a target: `<dest>/<name>` (without trailing slash).
pub fn base_dir(target: &Target) -> String {
    format!("{}/{}", target.dest.trim_end_matches('/'), target.name)
}

/// Destination for a new snapshot: `<dest>/<name>/<timestamp>/`.
/// Ends with `/` so rsync writes *into* the directory.
pub fn snapshot_dir(target: &Target, timestamp: &str) -> String {
    format!("{}/{}/", base_dir(target), timestamp)
}

/// Prefix for a snapshot directory that is still being written (SSH backend).
/// Dot-prefixed so `ls -1` ([`list_cmd`]) never shows it; [`finalize_cmd`]
/// renames it to the bare timestamp only after rsync succeeded.
pub const INCOMPLETE_PREFIX: &str = ".incomplete-";

/// Work directory a new snapshot is written into before it is finalized:
/// `<dest>/<name>/.incomplete-<timestamp>/`. Ends with `/` like [`snapshot_dir`].
/// Same depth under the base as the final directory, so rsync's relative
/// `--link-dest=../latest` resolves identically.
pub fn incomplete_dir(target: &Target, timestamp: &str) -> String {
    format!("{}/{INCOMPLETE_PREFIX}{}/", base_dir(target), timestamp)
}

/// Remote command that lists snapshots (one per line) under the target's base directory.
/// `2>/dev/null` so an empty/missing directory doesn't produce error output.
pub fn list_cmd(target: &Target) -> String {
    format!("ls -1 {} 2>/dev/null", shell_quote(&base_dir(target)))
}

/// Remote command that reads out what `<base>/latest` points to (empty if missing).
pub fn latest_cmd(target: &Target) -> String {
    format!(
        "readlink {}/latest 2>/dev/null",
        shell_quote(&base_dir(target))
    )
}

/// Remote command that reports whether dest is writable:
/// `writable` / `readonly` if it exists, otherwise `parent-writable` / `no-access`.
pub fn dest_check_cmd(target: &Target) -> String {
    let d = shell_quote(&target.dest);
    format!(
        "if [ -e {d} ]; then [ -w {d} ] && echo writable || echo readonly; \
         else p=$(dirname {d}); [ -w \"$p\" ] && echo parent-writable || echo no-access; fi"
    )
}

/// Remote command that deletes the given snapshot folders (`rm -rf`).
/// The timestamps come from the listing, so the paths are well-formed.
pub fn prune_cmd(target: &Target, timestamps: &[String]) -> String {
    let base = base_dir(target);
    let dirs: Vec<String> = timestamps
        .iter()
        .map(|ts| shell_quote(&format!("{base}/{ts}")))
        .collect();
    format!("rm -rf {}", dirs.join(" "))
}

/// Remote command that lists the contents of a snapshot, one entry per line
/// as `<type>\t<relative path>` (type d/f/l), relative to the snapshot root.
pub fn tree_cmd(target: &Target, timestamp: &str) -> String {
    let dir = format!("{}/{}", base_dir(target), timestamp);
    format!(
        "find {}/ -mindepth 1 -printf '%y\\t%P\\n' 2>/dev/null",
        shell_quote(&dir)
    )
}

/// Remote command that finalizes a successful snapshot, in one shell invocation:
///  1. renames the `.incomplete-<ts>` work directory to its final name (`mv` on
///     the same filesystem — atomic, so a listing never sees a half snapshot),
///  2. repoints `<base>/latest` to it (a relative symlink — just the timestamp —
///     so the tree can be moved),
///  3. removes work directories left behind by earlier interrupted runs. The
///     glob is deliberately outside the quotes; the just-renamed snapshot no
///     longer matches it. (Two *machines* backing up the same target/name were
///     never supported — same-machine overlap is prevented by the target lock.)
pub fn finalize_cmd(target: &Target, timestamp: &str) -> String {
    let base = base_dir(target);
    let from = shell_quote(&format!("{base}/{INCOMPLETE_PREFIX}{timestamp}"));
    let to = shell_quote(&format!("{base}/{timestamp}"));
    let link = shell_quote(&format!("{base}/latest"));
    format!(
        "mv {from} {to} && ln -sfn {} {link} && rm -rf {}/{INCOMPLETE_PREFIX}*",
        shell_quote(timestamp),
        shell_quote(&base),
    )
}

/// Remote command that removes work directories left behind by interrupted
/// runs. Normally the tail of [`finalize_cmd`] handles this — but a target
/// whose backups keep *failing* never reaches finalize, and the invisible
/// `.incomplete-*` trees would otherwise fill the disk with nothing (not even
/// prune) able to reclaim them. Callers hold the target lock, so no in-flight
/// work dir can be swept up.
pub fn cleanup_incomplete_cmd(target: &Target) -> String {
    // Glob outside the quotes on purpose; `-f` keeps a no-match glob silent.
    format!(
        "rm -rf {}/{INCOMPLETE_PREFIX}*",
        shell_quote(&base_dir(target))
    )
}

/// Single-quotes a string for safe use in a POSIX shell command (remote
/// commands here, and the local crontab line in the GUI).
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn target() -> Target {
        let cfg: Config = toml::from_str(
            r#"
            [[target]]
            name = "docs"
            host = "h"
            user = "u"
            dest = "/backups/"
            sources = ["/s"]
            "#,
        )
        .unwrap();
        cfg.targets.into_iter().next().unwrap()
    }

    #[test]
    fn shell_quote_neutralizes_quotes_and_metacharacters() {
        assert_eq!(shell_quote("plain"), "'plain'");
        // An embedded single quote can't escape the quoting.
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
        // Metacharacters stay inert inside single quotes.
        assert_eq!(shell_quote("$(rm -rf /);`x`"), "'$(rm -rf /);`x`'");
    }

    #[test]
    fn incomplete_dir_is_hidden_and_same_depth() {
        let t = target();
        assert_eq!(snapshot_dir(&t, "TS"), "/backups/docs/TS/");
        assert_eq!(incomplete_dir(&t, "TS"), "/backups/docs/.incomplete-TS/");
    }

    #[test]
    fn finalize_renames_links_and_cleans_stale_work_dirs() {
        let cmd = finalize_cmd(&target(), "2026-01-01T00-00-00");
        assert_eq!(
            cmd,
            "mv '/backups/docs/.incomplete-2026-01-01T00-00-00' \
             '/backups/docs/2026-01-01T00-00-00' \
             && ln -sfn '2026-01-01T00-00-00' '/backups/docs/latest' \
             && rm -rf '/backups/docs'/.incomplete-*"
        );
    }

    #[test]
    fn is_timestamp_accepts_only_snapshot_names() {
        assert!(is_timestamp("2026-06-24T20-30-00"));
        assert!(!is_timestamp("latest"));
        assert!(!is_timestamp(".incomplete-2026-06-24T20-30-00"));
        assert!(!is_timestamp("2026-06-24"));
    }

    #[test]
    fn prune_cmd_quotes_each_path() {
        let cmd = prune_cmd(
            &target(),
            &[
                "2026-01-01T00-00-00".to_string(),
                "2026-01-02T00-00-00".to_string(),
            ],
        );
        assert_eq!(
            cmd,
            "rm -rf '/backups/docs/2026-01-01T00-00-00' '/backups/docs/2026-01-02T00-00-00'"
        );
    }
}
