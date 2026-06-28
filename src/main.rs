//! moraine — CLI for snapshot-based backup over SSH/rsync and rclone.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use moraine::config::{self, Config};
use moraine::history::{self, LogEntry};
use moraine::{prune, rclone, rsync, snapshot, ssh};
use std::path::{Path, PathBuf};
use std::process::Command as SysCommand;

#[derive(Parser)]
#[command(name = "moraine", version = moraine::VERSION, about = "Moraine — snapshot backup over SSH/rsync and rclone")]
struct Cli {
    /// Path to the config file.
    #[arg(short, long, global = true, default_value = "moraine.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write an example config to start from.
    Init {
        /// Overwrite if the file already exists.
        #[arg(long)]
        force: bool,
    },
    /// Test the SSH connection, key, sources, and that the target is writable.
    Verify {
        #[arg(short, long)]
        target: Option<String>,
    },
    /// Run backup for all targets, or a selected one.
    Run {
        #[arg(short, long)]
        target: Option<String>,
        /// Show what would be done without transferring anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// List snapshots on the target.
    List {
        #[arg(short, long)]
        target: String,
    },
    /// Delete old snapshots according to the target's retention policy.
    Prune {
        #[arg(short, long)]
        target: Option<String>,
        /// Show what would be deleted without deleting.
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
            "{} already exists — use --force to overwrite",
            path.display()
        );
    }
    std::fs::write(path, EXAMPLE_CONFIG)
        .with_context(|| format!("could not write {}", path.display()))?;
    println!("Wrote example config to {}", path.display());
    println!("Edit it, then run: moraine run --dry-run");
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
                // Auto-prune after a successful backup if the target has retention.
                if let Err(e) = prune_target(path, t, dry_run) {
                    eprintln!("  prune failed: {e:#}");
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
        bail!("{failures} target(s) failed");
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

/// Runs all checks for a target. Returns true if everything went well.
fn verify_target(t: &config::Target) -> bool {
    if !t.backend.is_ssh() {
        return verify_rclone(t);
    }
    let mut ok = true;

    // SSH key (does it exist locally?)
    match t.key_path() {
        Some(key) if key.exists() => check(true, &format!("SSH key: {}", key.display())),
        Some(key) => {
            ok = false;
            check(false, &format!("SSH key missing: {}", key.display()));
        }
        None => println!("  · no key set (using ssh-agent)"),
    }

    // Sources (do they exist locally?)
    for src in &t.sources {
        let p = config::expand_tilde(src);
        let exists = p.exists();
        ok &= exists;
        check(exists, &format!("source {}", p.display()));
    }

    // SSH connection
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
            return false; // without a connection we cannot test dest
        }
        Err(e) => {
            check(false, &format!("SSH connection: {e}"));
            return false;
        }
    }

    // Dest writable? (remote)
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

/// Shows the target's destination differently depending on the backend.
fn backend_dest(t: &config::Target) -> String {
    match t.backend {
        config::Backend::Ssh => t.ssh_dest(),
        config::Backend::Ftp => format!("ftp {}@{}:{}", t.user, t.host, t.dest),
        config::Backend::Rclone => format!("rclone {}", rclone::base(t)),
    }
}

/// Fetches the snapshot list (newest first) for a target, regardless of backend.
fn list_snapshots(t: &config::Target) -> Result<Vec<String>> {
    let mut snaps = if t.backend.is_ssh() {
        let out = ssh_probe(t, &snapshot::list_cmd(t))
            .output()
            .context("could not start ssh")?;
        if !out.status.success() {
            bail!(
                "ssh failed (exit {}): {}",
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
        .with_context(|| format!("no target named '{target_name}'"))?;

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

/// Verify for rclone targets: sources locally + that rclone exists.
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

/// Writes a log entry; errors are ignored (logging should never fail a run).
fn log(config_path: &Path, entry: LogEntry) {
    if let Err(e) = history::append(config_path, &entry) {
        eprintln!("  warning: could not write history: {e:#}");
    }
}

/// Lists snapshots, plans according to retention, and deletes the older ones.
/// No-op if the target lacks (or has an empty) retention policy.
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
            .context("could not start ssh for deletion")?;
        if !del.status.success() {
            bail!(
                "deletion failed (exit {}): {}",
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

/// Builds a fail-fast ssh command (BatchMode + timeout) for verify/list,
/// so it does not hang on a password prompt or a dead host.
fn ssh_probe(target: &config::Target, remote_cmd: &str) -> SysCommand {
    let mut cmd = SysCommand::new("ssh");
    cmd.args(ssh::probe_command_args(target, remote_cmd));
    cmd
}

/// Prints a check line with ✓/✗.
fn check(ok: bool, msg: &str) {
    println!("  {} {msg}", if ok { "✓" } else { "✗" });
}

/// Pick out the targets to work with: a named one, or all.
fn select_targets<'a>(config: &'a Config, name: Option<&str>) -> Result<Vec<&'a config::Target>> {
    match name {
        Some(n) => {
            let t = config
                .target(n)
                .with_context(|| format!("no target named '{n}'"))?;
            Ok(vec![t])
        }
        None => Ok(config.targets.iter().collect()),
    }
}
