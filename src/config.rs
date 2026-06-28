//! Config: läser och validerar `backup.toml`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Hela config-filen: mål och scheman.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, rename = "target")]
    pub targets: Vec<Target>,
    #[serde(default, rename = "schedule", skip_serializing_if = "Vec::is_empty")]
    pub schedules: Vec<Schedule>,
}

/// Hur ofta ett schema körs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Frequency {
    Hourly,
    Daily,
    Weekly,
}

impl Frequency {
    /// Alla varianter, för väljare i UI:t.
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

/// Ett schema: vilket mål som ska backas upp och när.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    /// Kort namn för schemat.
    pub name: String,
    /// Namnet på målet (matchar ett `[[target]]`) som ska backas upp.
    pub target: String,
    /// Frekvens.
    pub frequency: Frequency,
    /// Minut (0–59). Används av alla frekvenser.
    #[serde(default)]
    pub minute: u8,
    /// Timme (0–23). Används av Daily och Weekly.
    #[serde(default)]
    pub hour: u8,
    /// Veckodag (0 = söndag … 6 = lördag). Används av Weekly.
    #[serde(default)]
    pub weekday: u8,
    /// Om schemat är aktivt.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Schedule {
    /// cron-uttryck (5 fält) som motsvarar schemat.
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

/// Transport-backend för ett mål.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// rsync över SSH (default) — hårdlänkade snapshots.
    #[default]
    Ssh,
    /// rclone — moln/objektlagring, snapshots via `--copy-dest`.
    Rclone,
    /// FTP (via rclones on-the-fly FTP-backend) — värd/user/lösenord i appen.
    Ftp,
}

impl Backend {
    pub fn is_ssh(&self) -> bool {
        matches!(self, Backend::Ssh)
    }

    /// Alla varianter, för väljare i UI:t.
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

/// Ett backup-mål: vart filerna ska, hur man når dit, och vad som ska med.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    /// Kort namn, används som mapp på målet och i CLI (`--target`).
    pub name: String,
    /// Transport: `ssh` (default) eller `rclone`.
    #[serde(default, skip_serializing_if = "Backend::is_ssh")]
    pub backend: Backend,
    /// SSH: IP/hostname. Rclone: remote-namn (tomt = lokal sökväg).
    pub host: String,
    /// SSH-användare (krävs för ssh-backend; ignoreras av rclone).
    #[serde(default)]
    pub user: String,
    /// SSH-port. Default 22.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Sökväg till privat SSH-nyckel. Valfri — annars används ssh-agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Lösenord (används av FTP-backenden). Lagras i klartext i config.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    /// Rotkatalog på målet där `<name>/<timestamp>/` skapas.
    pub dest: String,
    /// Filer/kataloger på klienten som ska backas upp.
    pub sources: Vec<String>,
    /// rsync exclude-mönster. Valfri.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Retention-policy. Utelämnad = behåll alla snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<Retention>,
}

/// Hur många snapshots som behålls vid pruning (GFS-stil). Alla 0 = behåll alla.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Retention {
    /// Behåll de N senaste oavsett ålder.
    #[serde(default)]
    pub keep_last: u32,
    /// Behåll nyaste snapshot per dag, för N dagar.
    #[serde(default)]
    pub keep_daily: u32,
    /// Behåll nyaste snapshot per ISO-vecka, för N veckor.
    #[serde(default)]
    pub keep_weekly: u32,
    /// Behåll nyaste snapshot per månad, för N månader.
    #[serde(default)]
    pub keep_monthly: u32,
}

impl Retention {
    /// Sant om ingen tier är satt (då görs ingen pruning).
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
    /// Läs och parsa en config-fil.
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("kunde inte läsa config: {}", path.display()))?;
        let config: Config =
            toml::from_str(&text).with_context(|| format!("ogiltig TOML i {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    /// Skriv config till TOML-fil.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("kunde inte serialisera config")?;
        std::fs::write(path, text)
            .with_context(|| format!("kunde inte skriva {}", path.display()))?;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.targets.is_empty() {
            bail!("config saknar [[target]]-block");
        }
        for t in &self.targets {
            if t.sources.is_empty() {
                bail!("mål '{}' saknar 'sources'", t.name);
            }
            // Dubbletter av namn skapar kollision i snapshot-mappar.
            let dupes = self.targets.iter().filter(|o| o.name == t.name).count();
            if dupes > 1 {
                bail!("flera mål delar namnet '{}'", t.name);
            }
        }
        Ok(())
    }

    /// Hitta ett mål på namn.
    pub fn target(&self, name: &str) -> Option<&Target> {
        self.targets.iter().find(|t| t.name == name)
    }
}

impl Target {
    /// "user@host" för rsync/ssh.
    pub fn ssh_dest(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    /// Nyckelns sökväg med `~` expanderat, om en nyckel är angiven.
    pub fn key_path(&self) -> Option<PathBuf> {
        self.key.as_deref().map(expand_tilde)
    }
}

/// Expanderar inledande `~/` mot $HOME. Lämnar övrigt orört.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return Path::new(&home).join(rest);
        }
    }
    PathBuf::from(path)
}
