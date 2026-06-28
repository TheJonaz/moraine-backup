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
    /// Root directory on the target where `<name>/<timestamp>/` is created.
    pub dest: String,
    /// Files/directories on the client to back up.
    pub sources: Vec<String>,
    /// rsync exclude patterns. Optional.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
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

    /// Write the config to a TOML file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("could not serialize config")?;
        std::fs::write(path, text)
            .with_context(|| format!("could not write {}", path.display()))?;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.targets.is_empty() {
            bail!("config has no [[target]] block");
        }
        for t in &self.targets {
            if t.sources.is_empty() {
                bail!("target '{}' is missing 'sources'", t.name);
            }
            // Duplicate names create a collision in snapshot folders.
            let dupes = self.targets.iter().filter(|o| o.name == t.name).count();
            if dupes > 1 {
                bail!("multiple targets share the name '{}'", t.name);
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

/// Expands a leading `~/` against $HOME. Leaves everything else untouched.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return Path::new(&home).join(rest);
        }
    }
    PathBuf::from(path)
}
