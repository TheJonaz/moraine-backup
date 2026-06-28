//! Körningslogg: varje backup/restore/prune skrivs som en JSON-rad till
//! `history.jsonl` bredvid config-filen. En "version" = en körning man kan
//! spåra i efterhand (i CLI eller GUI:ts History-flik).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// En post i körningsloggen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Lokal tidsstämpel, `YYYY-MM-DD HH:MM:SS`.
    pub time: String,
    /// Operation: `backup` | `restore` | `prune`.
    pub op: String,
    /// Målets namn.
    pub target: String,
    /// Lyckades operationen?
    pub ok: bool,
    /// Kort beskrivning (snapshot-id, antal, eller felmeddelande).
    pub detail: String,
}

impl LogEntry {
    /// Skapar en post med aktuell tidsstämpel.
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

/// Aktuell lokal tid som loggsträng.
pub fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Sökväg till loggfilen, bredvid config-filen.
pub fn path_for(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("history.jsonl")
}

/// Lägger till en post sist i loggfilen (skapar den vid behov).
pub fn append(config_path: &Path, entry: &LogEntry) -> Result<()> {
    let path = path_for(config_path);
    let line = serde_json::to_string(entry).context("serialisera loggpost")?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("kunde inte öppna {}", path.display()))?;
    writeln!(file, "{line}").context("kunde inte skriva loggrad")?;
    Ok(())
}

/// Läser alla loggposter, nyaste först. Trasiga rader hoppas över.
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
