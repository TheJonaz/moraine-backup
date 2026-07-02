//! Snapshot naming and `latest` handling on the target.
//!
//! Layout on the target:
//! ```text
//! <dest>/<name>/
//!   2026-06-24T20-30-00/      ← full tree structure
//!   2026-06-24T21-30-00/      ← unchanged files are hardlinks against the previous
//!   latest -> 2026-06-24T21-30-00
//! ```

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

/// Remote command that repoints `<base>/latest` to the new snapshot.
/// `latest` becomes a relative symlink (just the timestamp) so the tree can be moved.
pub fn update_latest_cmd(target: &Target, timestamp: &str) -> String {
    let link = format!("{}/latest", base_dir(target));
    format!("ln -sfn {} {}", shell_quote(timestamp), shell_quote(&link))
}

/// Single-quotes a string for safe use in a POSIX shell command (remote
/// commands here, and the local crontab line in the GUI).
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
