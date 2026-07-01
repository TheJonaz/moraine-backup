//! Optional VPN control via NetworkManager (`nmcli`): bring a connection up
//! before a backup and down afterwards. A target's `vpn` field holds the
//! NetworkManager connection name (empty = no VPN). Used by the CLI so that
//! scheduled runs raise the VPN too; the GUI does the same inline in its
//! worker thread so it can stream progress to the log.
use anyhow::{bail, Result};
use std::process::Command;

/// Bring the named NetworkManager connection up. Errors if `nmcli` is missing
/// or the connection can't activate.
pub fn up(name: &str) -> Result<()> {
    let out = Command::new("nmcli")
        .args(["connection", "up", name])
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
        .args(["connection", "down", name])
        .output();
}
