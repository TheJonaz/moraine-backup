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
    args.push("-o".to_string());
    args.push("BatchMode=yes".to_string());
    args.push("-o".to_string());
    args.push("ConnectTimeout=8".to_string());
    args.push(target.ssh_dest());
    args.push(remote_cmd.to_string());
    args
}
