//! SSH helpers shared by the rsync transport and the snapshot handling.

use crate::config::Target;

/// SSH options (without the program name): port, key if specified, host-key policy.
/// Used both in rsync's `-e` string and for direct ssh commands.
pub fn ssh_options(target: &Target) -> Vec<String> {
    let mut opts = vec!["-p".to_string(), target.port.to_string()];
    if let Some(key) = target.key_path() {
        opts.push("-i".to_string());
        opts.push(key.display().to_string());
    }
    // accept-new: trust an unknown host key automatically but warn if it changes.
    opts.push("-o".to_string());
    opts.push("StrictHostKeyChecking=accept-new".to_string());
    opts
}

/// The `-e` string rsync uses to start ssh.
pub fn transport(target: &Target) -> String {
    let mut parts = vec!["ssh".to_string()];
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
    let Some(script) = ensure_askpass_script() else {
        return Vec::new();
    };
    vec![
        ("SSH_ASKPASS".to_string(), script),
        // Force askpass even when a terminal is attached (OpenSSH >= 8.4).
        ("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()),
        // Some ssh builds only use askpass when DISPLAY is set; value is unused.
        (
            "DISPLAY".to_string(),
            std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string()),
        ),
        ("MORAINE_SSH_SECRET".to_string(), target.password.clone()),
    ]
}

/// Writes (once) a tiny askpass helper that echoes `$MORAINE_SSH_SECRET`, and
/// returns its path. The secret itself never touches the file.
fn ensure_askpass_script() -> Option<String> {
    let dir = std::env::temp_dir().join("moraine");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("askpass.sh");
    if !path.exists() {
        std::fs::write(&path, "#!/bin/sh\nprintf '%s\\n' \"$MORAINE_SSH_SECRET\"\n").ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).ok()?;
        }
    }
    Some(path.display().to_string())
}
