//! Run log: each backup/restore/prune is written as a JSON line to
//! `history.jsonl` next to the config file. A "version" = a run that can be
//! traced afterwards (in the CLI or the GUI's History tab).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// An entry in the run log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Local timestamp, `YYYY-MM-DD HH:MM:SS`.
    pub time: String,
    /// Operation: `backup` | `restore` | `prune`.
    pub op: String,
    /// The target's name.
    pub target: String,
    /// Did the operation succeed?
    pub ok: bool,
    /// Short description (snapshot id, count, or error message).
    pub detail: String,
}

impl LogEntry {
    /// Creates an entry with the current timestamp.
    pub fn new(op: &str, target: &str, ok: bool, detail: impl Into<String>) -> LogEntry {
        LogEntry {
            time: now(),
            op: op.to_string(),
            target: target.to_string(),
            ok,
            detail: detail.into(),
        }
    }
}

/// Current local time as a log string.
pub fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Path to the log file, next to the config file.
pub fn path_for(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("history.jsonl")
}

/// Appends an entry to the end of the log file (creates it if needed).
/// Owner-readable only: log lines can contain paths and backend error text.
pub fn append(config_path: &Path, entry: &LogEntry) -> Result<()> {
    let path = path_for(config_path);
    let line = serde_json::to_string(entry).context("serialize log entry")?;
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(&path)
        .with_context(|| format!("could not open {}", path.display()))?;
    #[cfg(unix)]
    {
        // Tighten a pre-existing log that may have been created world-readable.
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    writeln!(file, "{line}").context("could not write log line")?;
    drop(file);
    compact_if_large(&path);
    Ok(())
}

/// Caps the log: when the file grows past ~1 MiB, keep only the newest 2000
/// lines. Cheap size check on every append; the rewrite is rare.
fn compact_if_large(path: &Path) {
    const MAX_BYTES: u64 = 1_000_000;
    const KEEP_LINES: usize = 2000;
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() <= MAX_BYTES {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(KEEP_LINES);
    let kept = format!("{}\n", lines[start..].join("\n"));
    // Write to a sibling temp file then rename: an interrupted compaction (or a
    // concurrent append) can't truncate/lose the existing log.
    let tmp = path.with_extension("jsonl.tmp");
    if crate::config::write_private(&tmp, kept.as_bytes()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Reads all log entries, newest first. Corrupt lines are skipped.
pub fn read(config_path: &Path) -> Vec<LogEntry> {
    let path = path_for(config_path);
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut entries: Vec<LogEntry> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<LogEntry>(l).ok())
        .collect();
    entries.reverse();
    entries
}
