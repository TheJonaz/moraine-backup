//! SSH-hjälpare som delas av rsync-transporten och snapshot-hanteringen.

use crate::config::Target;

/// SSH-optioner (utan programnamnet): port, nyckel om angiven, host-key-policy.
/// Används både i rsync:s `-e`-sträng och för direkta ssh-kommandon.
pub fn ssh_options(target: &Target) -> Vec<String> {
    let mut opts = vec!["-p".to_string(), target.port.to_string()];
    if let Some(key) = target.key_path() {
        opts.push("-i".to_string());
        opts.push(key.display().to_string());
    }
    // accept-new: lita på okänd host-nyckel automatiskt men larma om den byts.
    opts.push("-o".to_string());
    opts.push("StrictHostKeyChecking=accept-new".to_string());
    opts
}

/// `-e`-strängen rsync använder för att starta ssh.
pub fn transport(target: &Target) -> String {
    let mut parts = vec!["ssh".to_string()];
    parts.extend(ssh_options(target));
    parts.join(" ")
}

/// Argument för att köra ett kommando på målet via ssh (utan "ssh" självt).
pub fn remote_command_args(target: &Target, remote_cmd: &str) -> Vec<String> {
    let mut args = ssh_options(target);
    args.push(target.ssh_dest());
    args.push(remote_cmd.to_string());
    args
}

/// Som `remote_command_args` men **fail-fast** (BatchMode + ConnectTimeout)
/// för verify/anslutningstest — så det inte hänger på lösenordsprompt eller
/// en död host.
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
