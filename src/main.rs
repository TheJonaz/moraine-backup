//! moraine — CLI for snapshot-based backup over SSH/rsync and rclone.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use moraine::config::{self, Config};
use moraine::history::{self, LogEntry};
use moraine::{healthcheck, notify, prune, rclone, rsync, snapshot, ssh, vpn};
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
    /// Verify a snapshot's contents against the current sources (by checksum).
    Check {
        #[arg(short, long)]
        target: Option<String>,
        /// Snapshot timestamp to verify. Defaults to the latest.
        #[arg(short, long)]
        snapshot: Option<String>,
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
    // If ssh launched us as its SSH_ASKPASS helper (Windows), print the secret
    // and exit before doing anything else. No-op unless MORAINE_ASKPASS is set.
    ssh::maybe_run_as_askpass();
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
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
        Command::Check { target, snapshot } => {
            cmd_check(&cli.config, target.as_deref(), snapshot.as_deref())
        }
        Command::Prune { target, dry_run } => cmd_prune(&cli.config, target.as_deref(), dry_run),
    }
}

fn cmd_init(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        anyhow::bail!(
            "{} already exists — use --force to overwrite",
            path.display()
        );
    }
    // Owner-only from the start: users add passwords to this file in place.
    config::write_private(path, EXAMPLE_CONFIG.as_bytes())
        .with_context(|| format!("could not write {}", path.display()))?;
    println!("Wrote example config to {}", path.display());
    println!("Edit it, then run: moraine run --dry-run");
    Ok(())
}

fn cmd_run(path: &Path, target: Option<&str>, dry_run: bool) -> Result<()> {
    let config = Config::load(path)?;
    let targets = select_targets(&config, target)?;
    let notify_on = config.notify_enabled();
    let mut failures = 0;
    // Record the outcome in history, ping the target's healthcheck, and (unless
    // disabled) raise a desktop notification. Called for every real run so cron
    // jobs get the healthcheck ping too.
    let finish = |t: &config::Target, ok: bool, detail: String| {
        log(path, LogEntry::new("backup", &t.name, ok, detail.clone()));
        healthcheck::ping(&t.healthcheck, ok);
        if notify_on {
            notify::backup_done(&t.name, ok, &detail);
        }
    };
    for t in targets {
        println!("== {} ({}) ==", t.name, backend_dest(t));
        // The destination is used to build remote paths (and prune runs
        // `rm -rf` under it) — refuse to run against an empty one.
        if t.dest.trim().is_empty() {
            failures += 1;
            let msg = "empty 'dest' — refusing to run";
            eprintln!("  target '{}': {msg}", t.name);
            if !dry_run {
                finish(t, false, msg.to_string());
            }
            continue;
        }
        // Bring the target's VPN up first (if any); skip the target if it
        // fails. A VPN the user already brought up is left untouched.
        let has_vpn = !t.vpn.trim().is_empty();
        let vpn_ours = has_vpn && !vpn::is_active(&t.vpn);
        if vpn_ours {
            println!("  VPN: connecting {}…", t.vpn);
            if let Err(e) = vpn::up(&t.vpn) {
                failures += 1;
                eprintln!("  {e:#}");
                if !dry_run {
                    finish(t, false, format!("{e:#}"));
                }
                continue;
            }
        } else if has_vpn {
            println!("  VPN: {} already connected", t.vpn);
        }
        let result = if t.backend.is_ssh() {
            rsync::run_target(t, dry_run)
        } else {
            rclone::run_target(t, dry_run)
        };
        // Tear the VPN down afterwards — only if we brought it up.
        if vpn_ours {
            vpn::down(&t.vpn);
        }
        match result {
            Ok(ts) => {
                if !dry_run {
                    finish(t, true, format!("snapshot {ts}"));
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
                    finish(t, false, format!("{e:#}"));
                }
            }
        }
    }
    if failures > 0 {
        bail!("{failures} target(s) failed");
    }
    Ok(())
}

fn cmd_verify(path: &Path, target: Option<&str>) -> Result<()> {
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
        Ok(out) if out.status.success() => match String::from_utf8_lossy(&out.stdout).trim() {
            "writable" => check(true, &format!("dest writable: {}", t.dest)),
            "parent-writable" => check(true, &format!("dest will be created: {}", t.dest)),
            "readonly" => {
                ok = false;
                check(false, &format!("dest not writable: {}", t.dest));
            }
            other => {
                ok = false;
                check(false, &format!("dest not accessible ({other}): {}", t.dest));
            }
        },
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

fn cmd_list(path: &Path, target_name: &str) -> Result<()> {
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

/// Verify a snapshot against the current sources by checksum. Reports, per target,
/// whether the snapshot faithfully holds the sources (0 differing paths) or how
/// many differ — differences are normal for an older snapshot whose sources have
/// since changed, but should be zero right after a backup.
fn cmd_check(path: &Path, target: Option<&str>, snapshot: Option<&str>) -> Result<()> {
    let config = Config::load(path)?;
    let targets = select_targets(&config, target)?;
    let mut failures = 0;
    for t in targets {
        let ts = match snapshot {
            Some(s) => s.to_string(),
            None => match list_snapshots(t)?.into_iter().next() {
                Some(s) => s,
                None => {
                    println!("== {} ==\n  no snapshots to verify", t.name);
                    continue;
                }
            },
        };
        println!("== {} — verifying snapshot {ts} ==", t.name);
        let res = if t.backend.is_ssh() {
            check_rsync(t, &ts)
        } else {
            check_rclone(t, &ts)
        };
        match res {
            Ok(0) => println!("  ✓ verified — the snapshot matches the current sources"),
            Ok(n) => println!(
                "  ⚠ {n} path(s) differ from the current sources \
                 (expected if the sources changed since this snapshot was made)"
            ),
            Err(e) => {
                failures += 1;
                eprintln!("  {e:#}");
            }
        }
    }
    if failures > 0 {
        bail!("{failures} target(s) could not be verified");
    }
    Ok(())
}

/// rsync checksum dry-run; returns the number of paths whose content differs from
/// or is missing in the snapshot, printing each.
fn check_rsync(t: &config::Target, ts: &str) -> Result<usize> {
    let out = SysCommand::new("rsync")
        .args(rsync::verify_args(t, ts))
        .envs(ssh::askpass_env(t))
        .output()
        .context("could not run rsync")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Itemize lines beginning with '>' or '<' are files rsync would transfer —
    // content differs or missing. (Attribute-only diffs start with '.'.)
    let mut n = 0;
    for line in stdout.lines() {
        if line.starts_with('>') || line.starts_with('<') {
            n += 1;
            println!(
                "    differs: {}",
                line.split_whitespace().last().unwrap_or(line)
            );
        }
    }
    if !out.status.success() && n == 0 {
        bail!(
            "rsync verify failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(n)
}

/// rclone one-way check of each directory source against its copy in the snapshot;
/// returns the total number of differing paths. File sources are skipped (rclone
/// check compares directories).
fn check_rclone(t: &config::Target, ts: &str) -> Result<usize> {
    let snap = rclone::snapshot_path(t, ts);
    let env = rclone::env_for(t);
    let mut total = 0;
    for src in &t.sources {
        let local = config::expand_tilde(src);
        let base = Path::new(src.trim_end_matches('/'))
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !local.is_dir() {
            println!(
                "    skipped {} (rclone verify supports directory sources)",
                local.display()
            );
            continue;
        }
        let out = SysCommand::new("rclone")
            .args([
                "check",
                &local.display().to_string(),
                &format!("{snap}/{base}"),
                "--one-way",
            ])
            .envs(env.clone())
            .output()
            .context("could not run rclone")?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        match parse_rclone_diffs(&stderr) {
            Some(d) => {
                total += d;
                if d > 0 {
                    println!("    {d} difference(s) under {base}");
                }
            }
            None if !out.status.success() => bail!("rclone check failed: {}", stderr.trim()),
            None => {}
        }
    }
    Ok(total)
}

/// Pulls the differing-file count out of rclone check's "… N differences found"
/// summary line. None when there's no such line.
fn parse_rclone_diffs(stderr: &str) -> Option<usize> {
    stderr.lines().find_map(|l| {
        let idx = l.find("differences found")?;
        l[..idx].split_whitespace().last()?.parse().ok()
    })
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
            let how = if cfg!(windows) {
                "winget install Rclone.Rclone, or rclone.org"
            } else if cfg!(target_os = "macos") {
                "brew install rclone, or rclone.org"
            } else {
                "apt install rclone, or rclone.org"
            };
            check(false, &format!("rclone not installed ({how})"));
        }
    }
    ok
}

fn cmd_prune(path: &Path, target: Option<&str>, dry_run: bool) -> Result<()> {
    let config = Config::load(path)?;
    let targets = select_targets(&config, target)?;
    let mut failures = 0;
    for t in targets {
        println!("== {} ({}) ==", t.name, backend_dest(t));
        match &t.retention {
            // One failing target shouldn't abort pruning of the rest.
            Some(p) if !p.is_empty() => {
                if let Err(e) = prune_target(path, t, dry_run) {
                    failures += 1;
                    eprintln!("  {e:#}");
                    if !dry_run {
                        log(
                            path,
                            LogEntry::new("prune", &t.name, false, format!("{e:#}")),
                        );
                    }
                }
            }
            _ => println!("  no retention policy — keeping all snapshots"),
        }
    }
    if failures > 0 {
        bail!("{failures} target(s) failed to prune");
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
    cmd.envs(ssh::askpass_env(target));
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
