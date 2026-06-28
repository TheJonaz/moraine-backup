//! moraine — CLI för snapshot-baserad backup över SSH/rsync och rclone.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use moraine::config::{self, Config};
use moraine::history::{self, LogEntry};
use moraine::{prune, rclone, rsync, snapshot, ssh};
use std::path::{Path, PathBuf};
use std::process::Command as SysCommand;

#[derive(Parser)]
#[command(name = "moraine", version = moraine::VERSION, about = "Moraine — snapshot-backup över SSH/rsync och rclone")]
struct Cli {
    /// Sökväg till config-filen.
    #[arg(short, long, global = true, default_value = "moraine.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Skriv en exempel-config att utgå från.
    Init {
        /// Skriv över om filen redan finns.
        #[arg(long)]
        force: bool,
    },
    /// Testa SSH-anslutning, nyckel, källor och att målet går att skriva till.
    Verify {
        #[arg(short, long)]
        target: Option<String>,
    },
    /// Kör backup för alla mål, eller ett valt.
    Run {
        #[arg(short, long)]
        target: Option<String>,
        /// Visa vad som skulle göras utan att överföra något.
        #[arg(long)]
        dry_run: bool,
    },
    /// Lista snapshots på målet.
    List {
        #[arg(short, long)]
        target: String,
    },
    /// Radera gamla snapshots enligt målets retention-policy.
    Prune {
        #[arg(short, long)]
        target: Option<String>,
        /// Visa vad som skulle raderas utan att radera.
        #[arg(long)]
        dry_run: bool,
    },
}

const EXAMPLE_CONFIG: &str = include_str!("../moraine.example.toml");

fn main() {
    if let Err(e) = run() {
        eprintln!("fel: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { force } => cmd_init(&cli.config, force),
        Command::Run { target, dry_run } => cmd_run(&cli.config, target.as_deref(), dry_run),
        Command::Verify { target } => cmd_verify(&cli.config, target.as_deref()),
        Command::List { target } => cmd_list(&cli.config, &target),
        Command::Prune { target, dry_run } => cmd_prune(&cli.config, target.as_deref(), dry_run),
    }
}

fn cmd_init(path: &PathBuf, force: bool) -> Result<()> {
    if path.exists() && !force {
        anyhow::bail!(
            "{} finns redan — använd --force för att skriva över",
            path.display()
        );
    }
    std::fs::write(path, EXAMPLE_CONFIG)
        .with_context(|| format!("kunde inte skriva {}", path.display()))?;
    println!("Skrev exempel-config till {}", path.display());
    println!("Redigera den och kör sedan: moraine run --dry-run");
    Ok(())
}

fn cmd_run(path: &PathBuf, target: Option<&str>, dry_run: bool) -> Result<()> {
    let config = Config::load(path)?;
    let targets = select_targets(&config, target)?;
    let mut failures = 0;
    for t in targets {
        println!("== {} ({}) ==", t.name, backend_dest(t));
        let result = if t.backend.is_ssh() {
            rsync::run_target(t, dry_run)
        } else {
            rclone::run_target(t, dry_run)
        };
        match result {
            Ok(ts) => {
                if !dry_run {
                    log(path, LogEntry::new("backup", &t.name, true, format!("snapshot {ts}")));
                }
                // Auto-prune efter lyckad backup om målet har retention.
                if let Err(e) = prune_target(path, t, dry_run) {
                    eprintln!("  prune misslyckades: {e:#}");
                }
            }
            Err(e) => {
                failures += 1;
                eprintln!("  {e:#}");
                if !dry_run {
                    log(path, LogEntry::new("backup", &t.name, false, e.to_string()));
                }
            }
        }
    }
    if failures > 0 {
        bail!("{failures} mål misslyckades");
    }
    Ok(())
}

fn cmd_verify(path: &PathBuf, target: Option<&str>) -> Result<()> {
    let config = Config::load(path)?;
    let targets = select_targets(&config, target)?;
    let mut all_ok = true;
    for t in targets {
        println!("== {} ({}) ==", t.name, backend_dest(t));
        if !verify_target(t) {
            all_ok = false;
        }
        println!();
    }
    if all_ok {
        println!("All checks passed ✓");
        Ok(())
    } else {
        bail!("some checks failed");
    }
}

/// Kör alla kontroller för ett mål. Returnerar true om allt gick bra.
fn verify_target(t: &config::Target) -> bool {
    if !t.backend.is_ssh() {
        return verify_rclone(t);
    }
    let mut ok = true;

    // SSH-nyckel (finns den lokalt?)
    match t.key_path() {
        Some(key) if key.exists() => check(true, &format!("SSH key: {}", key.display())),
        Some(key) => {
            ok = false;
            check(false, &format!("SSH key missing: {}", key.display()));
        }
        None => println!("  · no key set (using ssh-agent)"),
    }

    // Källor (finns de lokalt?)
    for src in &t.sources {
        let p = config::expand_tilde(src);
        let exists = p.exists();
        ok &= exists;
        check(exists, &format!("source {}", p.display()));
    }

    // SSH-anslutning
    match ssh_probe(t, "echo connection-ok").output() {
        Ok(out) if out.status.success() => check(true, "SSH connection"),
        Ok(out) => {
            check(
                false,
                &format!(
                    "SSH connection: {}",
                    String::from_utf8_lossy(&out.stderr)
                        .lines()
                        .next()
                        .unwrap_or("failed")
                        .trim()
                ),
            );
            return false; // utan anslutning kan vi inte testa dest
        }
        Err(e) => {
            check(false, &format!("SSH connection: {e}"));
            return false;
        }
    }

    // Dest skrivbar? (remote)
    match ssh_probe(t, &snapshot::dest_check_cmd(t)).output() {
        Ok(out) if out.status.success() => {
            match String::from_utf8_lossy(&out.stdout).trim() {
                "writable" => check(true, &format!("dest writable: {}", t.dest)),
                "parent-writable" => {
                    check(true, &format!("dest will be created: {}", t.dest))
                }
                "readonly" => {
                    ok = false;
                    check(false, &format!("dest not writable: {}", t.dest));
                }
                other => {
                    ok = false;
                    check(false, &format!("dest not accessible ({other}): {}", t.dest));
                }
            }
        }
        _ => {
            ok = false;
            check(false, "dest check failed");
        }
    }

    ok
}

/// Visar målets destination olika beroende på backend.
fn backend_dest(t: &config::Target) -> String {
    match t.backend {
        config::Backend::Ssh => t.ssh_dest(),
        config::Backend::Ftp => format!("ftp {}@{}:{}", t.user, t.host, t.dest),
        config::Backend::Rclone => format!("rclone {}", rclone::base(t)),
    }
}

/// Hämtar snapshot-listan (nyaste först) för ett mål, oavsett backend.
fn list_snapshots(t: &config::Target) -> Result<Vec<String>> {
    let mut snaps = if t.backend.is_ssh() {
        let out = ssh_probe(t, &snapshot::list_cmd(t))
            .output()
            .context("kunde inte starta ssh")?;
        if !out.status.success() {
            bail!(
                "ssh misslyckades (exit {}): {}",
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && l != "latest")
            .collect::<Vec<_>>()
    } else {
        rclone::list_snapshots(t)?
    };
    snaps.sort();
    snaps.reverse();
    Ok(snaps)
}

fn cmd_list(path: &PathBuf, target_name: &str) -> Result<()> {
    let config = Config::load(path)?;
    let t = config
        .target(target_name)
        .with_context(|| format!("inget mål heter '{target_name}'"))?;

    let snaps = list_snapshots(t)?;
    println!("Snapshots for {} ({}):", t.name, backend_dest(t));
    if snaps.is_empty() {
        println!("  (none)");
    }
    for s in &snaps {
        println!("  {s}");
    }
    println!("\n{} snapshot(s)", snaps.len());
    Ok(())
}

/// Verify för rclone-mål: källor lokalt + att rclone finns.
fn verify_rclone(t: &config::Target) -> bool {
    let mut ok = true;
    for src in &t.sources {
        let p = config::expand_tilde(src);
        let exists = p.exists();
        ok &= exists;
        check(exists, &format!("source {}", p.display()));
    }
    match SysCommand::new("rclone").arg("version").output() {
        Ok(o) if o.status.success() => {
            check(true, &format!("rclone backend → {}", rclone::base(t)))
        }
        _ => {
            ok = false;
            check(false, "rclone not installed (apt install rclone)");
        }
    }
    ok
}

fn cmd_prune(path: &PathBuf, target: Option<&str>, dry_run: bool) -> Result<()> {
    let config = Config::load(path)?;
    let targets = select_targets(&config, target)?;
    for t in targets {
        println!("== {} ({}) ==", t.name, backend_dest(t));
        match &t.retention {
            Some(p) if !p.is_empty() => prune_target(path, t, dry_run)?,
            _ => println!("  no retention policy — keeping all snapshots"),
        }
    }
    Ok(())
}

/// Skriver en loggpost; fel ignoreras (loggning ska aldrig fälla en körning).
fn log(config_path: &Path, entry: LogEntry) {
    if let Err(e) = history::append(config_path, &entry) {
        eprintln!("  varning: kunde inte skriva history: {e:#}");
    }
}

/// Listar snapshots, planerar enligt retention och raderar de äldre.
/// No-op om målet saknar (eller har tom) retention-policy.
fn prune_target(config_path: &Path, t: &config::Target, dry_run: bool) -> Result<()> {
    let Some(policy) = &t.retention else {
        return Ok(());
    };
    if policy.is_empty() {
        return Ok(());
    }

    let snaps = list_snapshots(t)?;
    let plan = prune::plan(&snaps, policy);
    if plan.delete.is_empty() {
        println!("  prune: nothing to delete ({} kept)", plan.keep.len());
        return Ok(());
    }
    println!(
        "  prune: deleting {} of {} snapshot(s), keeping {}",
        plan.delete.len(),
        snaps.len(),
        plan.keep.len()
    );
    for ts in &plan.delete {
        println!("    - {ts}");
    }
    if dry_run {
        println!("  (dry-run: nothing deleted)");
        return Ok(());
    }

    if t.backend.is_ssh() {
        let del = ssh_probe(t, &snapshot::prune_cmd(t, &plan.delete))
            .output()
            .context("kunde inte starta ssh för radering")?;
        if !del.status.success() {
            bail!(
                "radering misslyckades (exit {}): {}",
                del.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&del.stderr).trim()
            );
        }
    } else {
        for ts in &plan.delete {
            rclone::purge(t, ts)?;
        }
    }
    println!("  prune: done");
    log(
        config_path,
        LogEntry::new(
            "prune",
            &t.name,
            true,
            format!("deleted {}, kept {}", plan.delete.len(), plan.keep.len()),
        ),
    );
    Ok(())
}

/// Bygger ett fail-fast ssh-kommando (BatchMode + timeout) för verify/list,
/// så det inte hänger på lösenordsprompt eller en död host.
fn ssh_probe(target: &config::Target, remote_cmd: &str) -> SysCommand {
    let mut cmd = SysCommand::new("ssh");
    cmd.args(ssh::probe_command_args(target, remote_cmd));
    cmd
}

/// Skriver ut en kontrollrad med ✓/✗.
fn check(ok: bool, msg: &str) {
    println!("  {} {msg}", if ok { "✓" } else { "✗" });
}

/// Plocka ut målen att jobba med: ett namngivet, eller alla.
fn select_targets<'a>(config: &'a Config, name: Option<&str>) -> Result<Vec<&'a config::Target>> {
    match name {
        Some(n) => {
            let t = config
                .target(n)
                .with_context(|| format!("inget mål heter '{n}'"))?;
            Ok(vec![t])
        }
        None => Ok(config.targets.iter().collect()),
    }
}
