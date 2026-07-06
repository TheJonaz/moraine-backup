//! Best-effort desktop notifications via `notify-send`.
//!
//! Used to tell you when a backup finishes — especially a failure you'd otherwise
//! only find by reading the log. It shells out to `notify-send` (libnotify), in
//! keeping with the rest of the app talking to system tools (curl, gpg, rsync,
//! nmcli) rather than pulling in libraries. If `notify-send` isn't installed, or
//! there's no session bus (e.g. a headless cron run — the healthcheck ping covers
//! that case), it silently does nothing.

use std::process::{Command, Stdio};

/// Notify that a backup finished. Success is a normal notification; failure is
/// `critical` urgency with an error icon so it stands out.
pub fn backup_done(target: &str, ok: bool, detail: &str) {
    if ok {
        send(
            &format!("Backup complete — {target}"),
            detail,
            "normal",
            "moraine",
        );
    } else {
        send(
            &format!("Backup failed — {target}"),
            detail,
            "critical",
            "dialog-error",
        );
    }
}

/// Post a single desktop notification. Best-effort: never fails.
pub fn send(summary: &str, body: &str, urgency: &str, icon: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", "Moraine", "-u", urgency, "-i", icon, summary, body])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
