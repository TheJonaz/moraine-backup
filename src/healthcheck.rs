//! Optional "dead man's switch" pings.
//!
//! A target may carry a `healthcheck` URL. After each real backup it is pinged —
//! the URL on success, `<url>/fail` on failure (the healthchecks.io convention,
//! also understood by many uptime monitors). If a scheduled backup silently stops
//! running, the monitor stops hearing from it and alerts you — catching the class
//! of failure a desktop notification can't (a job that never starts).
//!
//! The ping shells out to `curl` (already a dependency of the update check and
//! feedback), so no HTTP client is pulled in. It is strictly best-effort: any
//! error is swallowed, because a failed ping must never affect the backup itself.

use crate::tools::CommandExt;
use std::process::{Command, Stdio};

/// Ping a target's healthcheck endpoint. No-op when `url` is empty.
///
/// `ok` selects the endpoint: the bare URL on success, `<url>/fail` on failure.
pub fn ping(url: &str, ok: bool) {
    let url = url.trim();
    if url.is_empty() {
        return;
    }
    let endpoint = if ok {
        url.to_string()
    } else {
        format!("{}/fail", url.trim_end_matches('/'))
    };
    // -fsS: fail on HTTP errors, quiet, but still show a real error if asked.
    // A short timeout and a couple of retries keep a flaky link from stalling the
    // run, without blocking indefinitely.
    let _ = Command::new("curl")
        .no_console()
        .args([
            "-fsS",
            "-m",
            "10",
            "--retry",
            "2",
            "-o",
            "/dev/null",
            &endpoint,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
