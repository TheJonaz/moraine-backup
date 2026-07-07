//! SSH helpers shared by the rsync transport and the snapshot handling.

use crate::config::Target;
#[cfg(not(windows))]
use std::path::PathBuf;

/// SSH options (without the program name): port, key if specified, host-key policy.
/// Used both in rsync's `-e` string and for direct ssh commands.
pub fn ssh_options(target: &Target) -> Vec<String> {
    let mut opts = vec!["-p".to_string(), target.port.to_string()];
    if let Some(key) = target.key_path() {
        opts.push("-i".to_string());
        opts.push(key.display().to_string());
    }
    // Default accept-new: trust an unknown host key on first connect, reject
    // if it later changes (TOFU). With `strict_host_key = true` the host must
    // already be in known_hosts — protects the first connection too.
    opts.push("-o".to_string());
    opts.push(if target.strict_host_key {
        "StrictHostKeyChecking=yes".to_string()
    } else {
        "StrictHostKeyChecking=accept-new".to_string()
    });
    // When a login password (or key passphrase) is set, we authenticate via
    // SSH_ASKPASS. Force-enable password + keyboard-interactive *for this
    // connection* so a client ssh_config that disables them — common on Windows,
    // where `KbdInteractiveAuthentication no` silently breaks password logins —
    // can't get in the way. Per-connection `-o` overrides the config file.
    if !target.password.trim().is_empty() {
        opts.push("-o".to_string());
        opts.push("PasswordAuthentication=yes".to_string());
        opts.push("-o".to_string());
        opts.push("KbdInteractiveAuthentication=yes".to_string());
    }
    opts
}

/// The `-e` string rsync uses to start ssh.
pub fn transport(target: &Target) -> String {
    // On Windows the bundled (cygwin/msys) rsync needs a *matching* cygwin ssh —
    // native Windows OpenSSH as the transport garbles the remote command
    // ("rsync: argc is zero!"). We ship MSYS2's ssh as `moraine-ssh` (found on PATH
    // next to the exe), and use it only here; Moraine's own direct ssh calls stay
    // on the system OpenSSH.
    let prog = if cfg!(windows) { "moraine-ssh" } else { "ssh" };
    let mut parts = vec![prog.to_string()];
    parts.extend(ssh_options(target));
    parts.join(" ")
}

/// Arguments to run a command on the target via ssh (without "ssh" itself).
pub fn remote_command_args(target: &Target, remote_cmd: &str) -> Vec<String> {
    let mut args = ssh_options(target);
    args.push(target.ssh_dest());
    args.push(remote_cmd.to_string());
    args
}

/// Like `remote_command_args` but **fail-fast** (BatchMode + ConnectTimeout)
/// for verify/connection test — so it doesn't hang on a password prompt or
/// a dead host.
pub fn probe_command_args(target: &Target, remote_cmd: &str) -> Vec<String> {
    let mut args = ssh_options(target);
    if target.password.is_empty() {
        args.push("-o".to_string());
        args.push("BatchMode=yes".to_string());
    }
    args.push("-o".to_string());
    args.push("ConnectTimeout=8".to_string());
    args.push(target.ssh_dest());
    args.push(remote_cmd.to_string());
    args
}

/// Environment variables that let `ssh`/`rsync` authenticate non-interactively
/// using the target's stored secret (an encrypted-key passphrase or a login
/// password) via OpenSSH's `SSH_ASKPASS` mechanism. Returns an empty vec when
/// the target has no secret or isn't an SSH target — apply it unconditionally
/// with `.envs(...)` at every ssh/rsync spawn site.
///
/// The secret is passed through the environment, never written to disk; only a
/// tiny generic helper script is written (once).
pub fn askpass_env(target: &Target) -> Vec<(String, String)> {
    if target.password.is_empty() || !target.backend.is_ssh() {
        return Vec::new();
    }
    let Some(askpass) = askpass_program() else {
        return Vec::new();
    };
    let mut env = vec![
        ("SSH_ASKPASS".to_string(), askpass),
        // Force askpass even when a terminal is attached (OpenSSH >= 8.4).
        ("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()),
        ("MORAINE_SSH_SECRET".to_string(), target.password.clone()),
    ];
    // ssh consults askpass only when DISPLAY is set (the value is unused). Set it
    // on Windows too — the bundled cygwin `moraine-ssh` wants it as well.
    env.push((
        "DISPLAY".to_string(),
        std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string()),
    ));
    // On Windows the helper is our own executable (a /bin/sh script isn't runnable
    // there); MORAINE_ASKPASS tells it to print the secret and exit instead of
    // starting the app. See the check at the top of each binary's main().
    #[cfg(windows)]
    {
        env.push(("MORAINE_ASKPASS".to_string(), "1".to_string()));
        // The bundled cygwin `moraine-ssh` derives HOME as `/home/<user>` — a path
        // that doesn't exist on Windows — so it can't create ~/.ssh or write
        // known_hosts ("Could not create directory '/home/…/.ssh'"). Point HOME at
        // the real Windows profile; cygwin converts the `C:\…` value itself.
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            env.push(("HOME".to_string(), profile.to_string_lossy().into_owned()));
        }
    }
    env
}

/// The program ssh runs as SSH_ASKPASS: a tiny shell script on Unix, and our own
/// executable on Windows (where Windows OpenSSH needs a real .exe, not a script).
fn askpass_program() -> Option<String> {
    #[cfg(windows)]
    {
        std::env::current_exe()
            .ok()
            .map(|p| p.display().to_string())
    }
    #[cfg(not(windows))]
    {
        ensure_askpass_script()
    }
}

/// If invoked by ssh as its askpass helper (Windows), print the stored secret to
/// stdout and exit — called first thing in each binary's `main()`, before any UI
/// or work. No-op unless `MORAINE_ASKPASS` is set (only on Windows).
pub fn maybe_run_as_askpass() {
    if std::env::var_os("MORAINE_ASKPASS").is_none() {
        return;
    }
    if let Some(secret) = std::env::var_os("MORAINE_SSH_SECRET") {
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = writeln!(out, "{}", secret.to_string_lossy());
        let _ = out.flush();
    }
    std::process::exit(0);
}

/// Writes a tiny askpass helper that echoes `$MORAINE_SSH_SECRET`, and returns
/// its path. The secret itself never touches the file.
///
/// The helper lives in a *private, per-user* directory (see [`askpass_dir`]) and
/// is **always rewritten** — never trusting a pre-existing file. Otherwise a
/// local attacker could pre-plant a script at a predictable shared /tmp path and
/// have ssh run it with the secret in the environment.
#[cfg(not(windows))]
fn ensure_askpass_script() -> Option<String> {
    let dir = askpass_dir()?;
    let path = dir.join("askpass.sh");
    std::fs::write(&path, "#!/bin/sh\nprintf '%s\\n' \"$MORAINE_SSH_SECRET\"\n").ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).ok()?;
    }
    Some(path.display().to_string())
}

/// A directory only the current user can access, for the askpass helper.
/// Prefers `$XDG_RUNTIME_DIR` (`/run/user/UID`, already mode 0700 and per-user),
/// then the user's cache dir; falls back to the temp dir only if neither is set.
/// Deliberately avoids a shared, predictable path like `/tmp/moraine`.
#[cfg(not(windows))]
fn askpass_dir() -> Option<PathBuf> {
    // Only per-user private locations — never a shared, predictable /tmp path
    // (a local attacker could pre-plant a script there). If none is available
    // we return None and simply don't use SSH_ASKPASS.
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    let dir = base.join("moraine");
    std::fs::create_dir_all(&dir).ok()?;
    #[cfg(unix)]
    {
        // Lock the directory to the owner (also tightens a looser pre-existing one).
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Some(dir)
}

#[cfg(test)]
mod tests {
    use crate::config::{Config, Target};

    fn target(password: &str) -> Target {
        let cfg: Config = toml::from_str(&format!(
            r#"
            [[target]]
            name = "n"
            host = "h"
            user = "u"
            dest = "/d"
            sources = ["/s"]
            password = "{password}"
            "#
        ))
        .unwrap();
        cfg.targets.into_iter().next().unwrap()
    }

    #[test]
    fn password_forces_password_and_kbdinteractive_auth() {
        // With a password/passphrase set, moraine overrides the client config so
        // a disabled KbdInteractive/Password method can't break the login.
        let opts = super::ssh_options(&target("secret")).join(" ");
        assert!(opts.contains("PasswordAuthentication=yes"), "{opts}");
        assert!(opts.contains("KbdInteractiveAuthentication=yes"), "{opts}");
        // Without one, we don't force those methods (avoids an interactive prompt
        // when key/agent auth is intended).
        let none = super::ssh_options(&target("")).join(" ");
        assert!(!none.contains("PasswordAuthentication"), "{none}");
    }
}
