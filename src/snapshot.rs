//! Snapshot-namngivning och `latest`-hantering på målet.
//!
//! Layout på målet:
//! ```text
//! <dest>/<name>/
//!   2026-06-24T20-30-00/      ← full trädstruktur
//!   2026-06-24T21-30-00/      ← oförändrade filer är hårdlänkar mot förra
//!   latest -> 2026-06-24T21-30-00
//! ```

use crate::config::Target;
use chrono::Local;

/// Tidsstämpel för en ny snapshot-mapp, t.ex. `2026-06-24T20-30-00`.
/// `:` undviks så namnet är säkert på alla filsystem.
pub fn timestamp() -> String {
    Local::now().format("%Y-%m-%dT%H-%M-%S").to_string()
}

/// Baskatalogen för ett mål: `<dest>/<name>` (utan avslutande slash).
pub fn base_dir(target: &Target) -> String {
    format!("{}/{}", target.dest.trim_end_matches('/'), target.name)
}

/// Destination för en ny snapshot: `<dest>/<name>/<timestamp>/`.
/// Slutar på `/` så rsync skriver *in i* katalogen.
pub fn snapshot_dir(target: &Target, timestamp: &str) -> String {
    format!("{}/{}/", base_dir(target), timestamp)
}

/// Remote-kommando som listar snapshots (en per rad) under målets baskatalog.
/// `2>/dev/null` så en tom/saknad katalog inte ger fel-utskrift.
pub fn list_cmd(target: &Target) -> String {
    format!("ls -1 {} 2>/dev/null", shell_quote(&base_dir(target)))
}

/// Remote-kommando som läser ut vad `<base>/latest` pekar på (tom om saknas).
pub fn latest_cmd(target: &Target) -> String {
    format!("readlink {}/latest 2>/dev/null", shell_quote(&base_dir(target)))
}

/// Remote-kommando som rapporterar om dest går att skriva till:
/// `writable` / `readonly` om den finns, annars `parent-writable` / `no-access`.
pub fn dest_check_cmd(target: &Target) -> String {
    let d = shell_quote(&target.dest);
    format!(
        "if [ -e {d} ]; then [ -w {d} ] && echo writable || echo readonly; \
         else p=$(dirname {d}); [ -w \"$p\" ] && echo parent-writable || echo no-access; fi"
    )
}

/// Remote-kommando som raderar givna snapshot-mappar (`rm -rf`).
/// Tidsstämplarna kommer från listningen, så sökvägarna är välformade.
pub fn prune_cmd(target: &Target, timestamps: &[String]) -> String {
    let base = base_dir(target);
    let dirs: Vec<String> = timestamps
        .iter()
        .map(|ts| shell_quote(&format!("{base}/{ts}")))
        .collect();
    format!("rm -rf {}", dirs.join(" "))
}

/// Remote-kommando som listar innehållet i en snapshot, en post per rad
/// som `<typ>\t<relativ sökväg>` (typ d/f/l), relativt snapshot-roten.
pub fn tree_cmd(target: &Target, timestamp: &str) -> String {
    let dir = format!("{}/{}", base_dir(target), timestamp);
    format!(
        "find {}/ -mindepth 1 -printf '%y\\t%P\\n' 2>/dev/null",
        shell_quote(&dir)
    )
}

/// Remote-kommando som pekar om `<base>/latest` till den nya snapshoten.
/// `latest` blir en relativ symlänk (bara timestamp) så trädet kan flyttas.
pub fn update_latest_cmd(target: &Target, timestamp: &str) -> String {
    let link = format!("{}/latest", base_dir(target));
    format!("ln -sfn {} {}", shell_quote(timestamp), shell_quote(&link))
}

/// Single-quote:ar en sträng för säker användning i ett remote shell-kommando.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
