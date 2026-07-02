//! Optional VPN control via NetworkManager (`nmcli`): bring a connection up
//! before a backup and down afterwards. A target's `vpn` field holds the
//! NetworkManager connection name (empty = no VPN). Used by the CLI so that
//! scheduled runs raise the VPN too; the GUI shares these helpers from its
//! worker thread so it can stream progress to the log.
//!
//! `id` is passed explicitly (`nmcli connection up id <name>`) so a connection
//! name starting with `-` can't be parsed as an nmcli option.
use anyhow::{bail, Result};
use std::process::Command;

/// True if the named connection is currently active. Errors (nmcli missing,
/// unknown name) count as "not active" — the caller will then try `up` and get
/// a real error message from that instead.
pub fn is_active(name: &str) -> bool {
    Command::new("nmcli")
        .args(["-g", "GENERAL.STATE", "connection", "show", "id", name])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .eq_ignore_ascii_case("activated")
        })
        .unwrap_or(false)
}

/// Bring the named NetworkManager connection up. Errors if `nmcli` is missing
/// or the connection can't activate.
pub fn up(name: &str) -> Result<()> {
    let out = Command::new("nmcli")
        .args(["connection", "up", "id", name])
        .output()
        .map_err(|e| anyhow::anyhow!("could not run nmcli (is NetworkManager installed?): {e}"))?;
    if !out.status.success() {
        bail!(
            "could not bring up VPN \"{name}\": {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Bring the named connection down. Best effort — errors are ignored, since a
/// failed teardown must not fail an otherwise-successful backup.
pub fn down(name: &str) {
    let _ = Command::new("nmcli")
        .args(["connection", "down", "id", name])
        .output();
}
