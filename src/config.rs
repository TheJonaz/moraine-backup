//! Config: reads and validates `backup.toml`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The entire config file: targets and schedules.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, rename = "target")]
    pub targets: Vec<Target>,
    #[serde(default, rename = "schedule", skip_serializing_if = "Vec::is_empty")]
    pub schedules: Vec<Schedule>,
}

/// How often a schedule runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Frequency {
    Hourly,
    Daily,
    Weekly,
}

impl Frequency {
    /// All variants, for pickers in the UI.
    pub const ALL: [Frequency; 3] = [Frequency::Hourly, Frequency::Daily, Frequency::Weekly];
}

impl std::fmt::Display for Frequency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Frequency::Hourly => "Hourly",
            Frequency::Daily => "Daily",
            Frequency::Weekly => "Weekly",
        };
        f.write_str(s)
    }
}

/// A schedule: which target to back up and when.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    /// Short name for the schedule.
    pub name: String,
    /// The name of the target (matches a `[[target]]`) to back up.
    pub target: String,
    /// Frequency.
    pub frequency: Frequency,
    /// Minute (0–59). Used by all frequencies.
    #[serde(default)]
    pub minute: u8,
    /// Hour (0–23). Used by Daily and Weekly.
    #[serde(default)]
    pub hour: u8,
    /// Weekday (0 = Sunday … 6 = Saturday). Used by Weekly.
    #[serde(default)]
    pub weekday: u8,
    /// Whether the schedule is active.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Schedule {
    /// cron expression (5 fields) that corresponds to the schedule.
    pub fn cron(&self) -> String {
        let m = self.minute.min(59);
        let h = self.hour.min(23);
        let wd = self.weekday.min(6);
        match self.frequency {
            Frequency::Hourly => format!("{m} * * * *"),
            Frequency::Daily => format!("{m} {h} * * *"),
            Frequency::Weekly => format!("{m} {h} * * {wd}"),
        }
    }
}

fn default_true() -> bool {
    true
}

/// Transport backend for a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// rsync over SSH (default) — hardlinked snapshots.
    #[default]
    Ssh,
    /// rclone — cloud/object storage, snapshots via `--copy-dest`.
    Rclone,
    /// FTP (via rclone's on-the-fly FTP backend) — host/user/password in the app.
    Ftp,
}

impl Backend {
    pub fn is_ssh(&self) -> bool {
        matches!(self, Backend::Ssh)
    }

    /// All variants, for pickers in the UI.
    pub const ALL: [Backend; 3] = [Backend::Ssh, Backend::Rclone, Backend::Ftp];
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Backend::Ssh => "ssh",
            Backend::Rclone => "rclone",
            Backend::Ftp => "ftp",
        })
    }
}

/// A backup target: where the files go, how to reach it, and what to include.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    /// Short name, used as the folder on the target and in the CLI (`--target`).
    pub name: String,
    /// Transport: `ssh` (default) or `rclone`.
    #[serde(default, skip_serializing_if = "Backend::is_ssh")]
    pub backend: Backend,
    /// SSH: IP/hostname. Rclone: remote name (empty = local path).
    pub host: String,
    /// SSH user (required for the ssh backend; ignored by rclone).
    #[serde(default)]
    pub user: String,
    /// SSH port. Default 22.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Path to the private SSH key. Optional — otherwise ssh-agent is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Secret stored in plaintext in the config: the FTP password for the `ftp`
    /// backend, or (for `ssh`) the SSH key passphrase or login password, which
    /// is supplied to ssh/rsync non-interactively via `SSH_ASKPASS`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    /// SSH host-key policy. Default (false) is `accept-new`: trust an unknown
    /// host on first connect, reject if the key later changes (TOFU). Set true
    /// for `StrictHostKeyChecking=yes` — the host must already be in
    /// known_hosts, protecting even the first connection from MITM.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strict_host_key: bool,
    /// Root directory on the target where `<name>/<timestamp>/` is created.
    pub dest: String,
    /// Files/directories on the client to back up.
    pub sources: Vec<String>,
    /// rsync exclude patterns. Optional.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// NetworkManager connection to bring up before the backup and down after
    /// (e.g. a WireGuard/OpenVPN VPN). Empty/omitted = no VPN. Activated via
    /// `nmcli connection up/down`, so it works with whatever VPNs you have
    /// configured in your desktop's network settings.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub vpn: String,
    /// Retention policy. Omitted = keep all snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<Retention>,
}

/// How many snapshots are kept when pruning (GFS-style). All 0 = keep all.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Retention {
    /// Keep the N most recent regardless of age.
    #[serde(default)]
    pub keep_last: u32,
    /// Keep the newest snapshot per day, for N days.
    #[serde(default)]
    pub keep_daily: u32,
    /// Keep the newest snapshot per ISO week, for N weeks.
    #[serde(default)]
    pub keep_weekly: u32,
    /// Keep the newest snapshot per month, for N months.
    #[serde(default)]
    pub keep_monthly: u32,
}

impl Retention {
    /// True if no tier is set (in which case no pruning is done).
    pub fn is_empty(&self) -> bool {
        self.keep_last == 0
            && self.keep_daily == 0
            && self.keep_weekly == 0
            && self.keep_monthly == 0
    }
}

fn default_port() -> u16 {
    22
}

impl Config {
    /// Read and parse a config file.
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read config: {}", path.display()))?;
        let config: Config =
            toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    /// Write the config to a TOML file, owner-readable only (it holds secrets).
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("could not serialize config")?;
        write_private(path, text.as_bytes())
            .with_context(|| format!("could not write {}", path.display()))?;
        Ok(())
    }

    /// Sanity- and safety-checks the config. Public so the GUI can validate an
    /// imported config before replacing the current one.
    pub fn validate(&self) -> Result<()> {
        if self.targets.is_empty() {
            bail!("config has no [[target]] block");
        }
        for t in &self.targets {
            // The name becomes a folder under `dest` and is interpolated into
            // remote commands (always shell-quoted, but quoting doesn't stop
            // path traversal). Reject anything that could escape `<dest>/<name>`
            // or collide with the `latest` pointer.
            let name = t.name.trim();
            if name.is_empty() {
                bail!("a target is missing 'name'");
            }
            if name.contains(['/', '\\'])
                || name.chars().any(|c| c.is_control())
                || name == "."
                || name == ".."
                || name == "latest"
            {
                bail!(
                    "target name '{}' is not allowed: it is used as a folder on the \
                     destination, so it must not contain '/', '\\' or control \
                     characters, and cannot be '.', '..' or 'latest'",
                    t.name
                );
            }
            // The FTP backend builds an rclone connection string
            // (`:ftp,host=…,user=…,pass=…:`): a ',' would inject arbitrary
            // rclone options and a ':' truncates the string.
            if matches!(t.backend, Backend::Ftp) {
                for (field, v) in [("host", &t.host), ("user", &t.user)] {
                    if v.contains([',', ':']) {
                        bail!(
                            "target '{name}': {field} must not contain ',' or ':' \
                             (it is embedded in the rclone FTP connection string)"
                        );
                    }
                }
            }
            if t.sources.is_empty() {
                bail!("target '{name}' is missing 'sources'");
            }
            // Duplicate names create a collision in snapshot folders.
            let dupes = self.targets.iter().filter(|o| o.name == t.name).count();
            if dupes > 1 {
                bail!("multiple targets share the name '{name}'");
            }
        }
        for s in &self.schedules {
            // Out-of-range values would otherwise be silently clamped by cron().
            if s.minute > 59 || s.hour > 23 || s.weekday > 6 {
                bail!(
                    "schedule '{}' has an out-of-range time (minute 0–59, hour 0–23, \
                     weekday 0–6)",
                    s.name
                );
            }
        }
        Ok(())
    }

    /// Find a target by name.
    pub fn target(&self, name: &str) -> Option<&Target> {
        self.targets.iter().find(|t| t.name == name)
    }
}

impl Target {
    /// "user@host" for rsync/ssh.
    pub fn ssh_dest(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    /// The key's path with `~` expanded, if a key is specified.
    pub fn key_path(&self) -> Option<PathBuf> {
        self.key.as_deref().map(expand_tilde)
    }
}

/// Writes a file that only the owner can read/write (mode 0600 on Unix). The
/// config and its encrypted-export counterpart hold plaintext secrets (SSH
/// passwords / key passphrases), so they must not be world-readable. New files
/// are created 0600 from the start (no race); a pre-existing world-readable
/// file is also locked down.
pub fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    #[cfg(unix)]
    {
        // Fix a file that already existed with looser permissions.
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(contents)?;
    Ok(())
}

/// Expands `~` / a leading `~/` against $HOME. Leaves everything else untouched.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        if path == "~" {
            return PathBuf::from(home);
        }
        if let Some(rest) = path.strip_prefix("~/") {
            return Path::new(&home).join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::{write_private, Config};
    use std::os::unix::fs::PermissionsExt;

    fn cfg_with_name(name: &str) -> Config {
        toml::from_str(&format!(
            r#"
            [[target]]
            name = "{name}"
            host = "h"
            user = "u"
            dest = "/tmp/x"
            sources = ["/tmp"]
            "#
        ))
        .unwrap()
    }

    #[test]
    fn validate_rejects_dangerous_target_names() {
        // These become folders under dest and are embedded in remote rm -rf:
        // traversal or reserved names must never pass.
        for bad in ["../evil", "a/b", "a\\b", "latest", "..", ".", " "] {
            assert!(
                cfg_with_name(bad).validate().is_err(),
                "name {bad:?} should be rejected"
            );
        }
        assert!(cfg_with_name("nas-1.home").validate().is_ok());
    }

    #[test]
    fn validate_rejects_ftp_option_injection() {
        // ',' in host would inject rclone options into `:ftp,host=…:`.
        let cfg: Config = toml::from_str(
            r#"
            [[target]]
            name = "ftp1"
            backend = "ftp"
            host = "h,tls=false"
            user = "u"
            dest = "backups"
            sources = ["/tmp"]
            "#,
        )
        .unwrap();
        assert!(cfg.validate().is_err());
    }

    fn mode(p: &std::path::Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn write_private_new_file_is_owner_only() {
        let path = std::env::temp_dir().join(format!("moraine-priv-new-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        write_private(&path, b"secret").unwrap();
        assert_eq!(mode(&path), 0o600, "new secret file must be 0600");
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_private_tightens_existing_world_readable() {
        let path = std::env::temp_dir().join(format!("moraine-priv-old-{}", std::process::id()));
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, b"new").unwrap();
        assert_eq!(mode(&path), 0o600, "pre-existing file must be tightened to 0600");
        let _ = std::fs::remove_file(&path);
    }
}
