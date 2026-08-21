//! Thern account sign-in for the desktop client.
//!
//! The app pairs with the user's Thern account through a device-code flow against
//! `www.thern.io/api/app_link.php` (that endpoint documents the security model):
//! the app shows a short code, the user approves it in a browser where they sign
//! in the normal way, and the app polls until it is handed a durable bearer token
//! from the `app_tokens` table. Only *this* app ever holds the token.
//!
//! Storage split: the token is the secret, so it lives in the OS keyring (same
//! store as target passwords, service `moraine`). The display name and email are
//! not secret and sit in a small `account.json` beside the rest of the config, so
//! "Signed in as X" survives a restart without a network round-trip.
//!
//! HTTP is done with `curl`, exactly like the feedback and update-check paths —
//! no HTTP crate is pulled in for this.

use crate::tools::CommandExt;
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

const APP_LINK_URL: &str = "https://www.thern.io/api/app_link.php";
const LINK_PAGE_URL: &str = "https://www.thern.io/customers/link.html";

#[cfg(feature = "keyring")]
const KEYRING_ACCOUNT: &str = "thern-account-token";

/// A signed-in Thern account. `token` is the bearer; the rest is for display.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub token: String,
    pub name: String,
    pub email: String,
    pub is_admin: bool,
}

/// A freshly opened pairing request: the code to show, the secret to poll with.
#[derive(Clone, Debug)]
pub struct Started {
    pub code: String,
    pub secret: String,
    pub expires_in: u64,
}

/// The state of a pairing request as of the last poll.
pub enum Poll {
    Pending,
    Approved(Box<Session>),
    Denied,
    Expired,
    /// Unknown secret, or the request was already consumed.
    Gone,
}

// ── HTTP (curl) ─────────────────────────────────────────────────────────────

fn post(action: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let url = format!("{APP_LINK_URL}?action={action}");
    let mut child = Command::new("curl")
        .no_console()
        .args([
            "-fsS",
            "--max-time",
            "20",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-H",
            "Accept: application/json",
            "--data-binary",
            "@-",
            &url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("curl is required to sign in")?;
    child
        .stdin
        .take()
        .context("no stdin")?
        .write_all(body.to_string().as_bytes())
        .context("could not send request")?;
    let out = child.wait_with_output().context("curl failed")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        bail!(if err.is_empty() { "server unreachable" } else { err }.to_string());
    }
    serde_json::from_slice(&out.stdout).context("unexpected response from server")
}

/// Open a pairing request. The user then approves the returned code in a browser.
pub fn start() -> Result<Started> {
    let v = post("start", &serde_json::json!({}))?;
    if v["ok"] != serde_json::Value::Bool(true) {
        bail!(v["error"].as_str().unwrap_or("could not start sign-in").to_string());
    }
    Ok(Started {
        code: v["code"].as_str().unwrap_or_default().to_string(),
        secret: v["secret"].as_str().unwrap_or_default().to_string(),
        expires_in: v["expires_in"].as_u64().unwrap_or(300),
    })
}

/// Ask where a pairing request stands. Poll this every couple of seconds.
pub fn poll(secret: &str) -> Result<Poll> {
    let v = post("poll", &serde_json::json!({ "secret": secret }))?;
    Ok(match v["status"].as_str().unwrap_or("gone") {
        "approved" => {
            let s = Session {
                token: v["token"].as_str().unwrap_or_default().to_string(),
                name: v["name"].as_str().unwrap_or_default().to_string(),
                email: v["email"].as_str().unwrap_or_default().to_string(),
                is_admin: v["is_admin"].as_bool().unwrap_or(false),
            };
            if s.token.is_empty() {
                bail!("the server approved the sign-in but returned no token");
            }
            Poll::Approved(Box::new(s))
        }
        "denied" => Poll::Denied,
        "expired" => Poll::Expired,
        "pending" => Poll::Pending,
        _ => Poll::Gone,
    })
}

/// Best-effort: tell the server to drop a request the user cancelled locally.
pub fn cancel(secret: &str) {
    let _ = post("cancel", &serde_json::json!({ "secret": secret }));
}

/// The browser URL that approves `code`. The code is alphanumeric + '-', so it
/// needs no escaping.
pub fn approve_url(code: &str) -> String {
    format!("{LINK_PAGE_URL}?code={code}")
}

// ── Persistence ─────────────────────────────────────────────────────────────

/// `~/.config/moraine/account.json` — display fields only, never the token.
///
/// Only the keyring builds persist a session; without that feature there is no
/// token to pair the state with, so nothing calls this.
#[cfg(feature = "keyring")]
fn state_path() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(dir.join("moraine").join("account.json"))
}

#[cfg(feature = "keyring")]
fn write_state(s: &Session) -> Result<()> {
    let path = state_path().context("no config directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let json = serde_json::json!({ "name": s.name, "email": s.email, "is_admin": s.is_admin });
    std::fs::write(&path, json.to_string()).context("could not write account state")
}

#[cfg(feature = "keyring")]
fn read_state() -> (String, String, bool) {
    let Some(path) = state_path() else {
        return Default::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Default::default();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Default::default();
    };
    (
        v["name"].as_str().unwrap_or_default().to_string(),
        v["email"].as_str().unwrap_or_default().to_string(),
        v["is_admin"].as_bool().unwrap_or(false),
    )
}

/// Persist a session: token to the keyring, display fields to `account.json`.
#[cfg(feature = "keyring")]
pub fn save(s: &Session) -> Result<()> {
    keyring::Entry::new(crate::secrets::SERVICE, KEYRING_ACCOUNT)
        .context("could not open the keyring")?
        .set_password(&s.token)
        .context("could not store the account token in the keyring")?;
    write_state(s)
}

/// The stored session, or None when signed out.
#[cfg(feature = "keyring")]
pub fn load() -> Option<Session> {
    let token = keyring::Entry::new(crate::secrets::SERVICE, KEYRING_ACCOUNT)
        .ok()?
        .get_password()
        .ok()?;
    if token.is_empty() {
        return None;
    }
    let (name, email, is_admin) = read_state();
    Some(Session { token, name, email, is_admin })
}

/// Forget the account on this device: drop the keyring token and the state file.
///
/// This is a local sign-out — the `app_tokens` row stays valid server-side until
/// a revoke endpoint exists. Fine for the common case (this machine forgets it);
/// revocation is a follow-up.
#[cfg(feature = "keyring")]
pub fn sign_out() {
    if let Ok(entry) = keyring::Entry::new(crate::secrets::SERVICE, KEYRING_ACCOUNT) {
        let _ = entry.delete_credential();
    }
    if let Some(path) = state_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// A curl config file (`curl -K <path>`) carrying the account's bearer token,
/// deleted when dropped.
///
/// The token deliberately does NOT travel as a `-H` argument: process arguments
/// are world-readable on Linux (`/proc/<pid>/cmdline`), so any other local user
/// could read the credential for as long as curl runs. A config file passed by
/// path leaks only the path. It is written mode 0600 inside our own config
/// directory rather than the shared temp dir, which also rules out a symlink
/// race on a predictable /tmp name.
pub struct CurlAuth(std::path::PathBuf);

impl CurlAuth {
    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for CurlAuth {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Write the signed-in account's `Authorization` header to a private curl config
/// file, or None when signed out (or when it cannot be written privately —
/// attribution is a nicety, never worth leaking a token for).
pub fn curl_auth() -> Option<CurlAuth> {
    #[cfg(feature = "keyring")]
    {
        let session = load()?;
        if session.token.is_empty() {
            return None;
        }
        let path = state_path()?.with_file_name("auth-curl.conf");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        write_private(
            &path,
            &format!("header = \"Authorization: Bearer {}\"\n", session.token),
        )
        .ok()?;
        Some(CurlAuth(path))
    }
    #[cfg(not(feature = "keyring"))]
    {
        None
    }
}

/// Create (or truncate) `path` readable only by this user, and write `contents`.
#[cfg(feature = "keyring")]
fn write_private(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)?.write_all(contents.as_bytes())
}
