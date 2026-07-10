//! moraine — CLI for snapshot-based backup over SSH/rsync and rclone.

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use moraine::config::{self, Config};
use moraine::history::{self, LogEntry};
use moraine::{healthcheck, lock, notify, prune, rclone, rsync, snapshot, ssh, tools, vpn};
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
// The Run variant carries the ad-hoc flag bundle, making it larger than the
// others — irrelevant for a command enum parsed once at startup.
#[allow(clippy::large_enum_variant)]
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
    /// Run backup for all targets, a selected one, or an ad-hoc target from flags.
    Run {
        #[arg(short, long)]
        target: Option<String>,
        /// Show what would be done without transferring anything.
        #[arg(long)]
        dry_run: bool,
        #[command(flatten)]
        adhoc: AdHoc,
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

/// An ad-hoc target defined entirely on the command line — no config file. Set
/// `--dest` and at least one `--source` (plus `--host` for ssh/ftp) to trigger it,
/// e.g. `moraine run --host nas --user me --key ~/.ssh/id --dest /backups
/// --source ~/docs`.
#[derive(Args)]
struct AdHoc {
    /// Ad-hoc: destination host — an SSH/FTP hostname, or the rclone remote name
    /// (empty with --backend rclone = a local path).
    #[arg(long, help_heading = "Ad-hoc target (no config file)")]
    host: Option<String>,
    /// Ad-hoc: backend — ssh (default), rclone or ftp.
    #[arg(
        long,
        default_value = "ssh",
        help_heading = "Ad-hoc target (no config file)"
    )]
    backend: String,
    /// Ad-hoc: SSH/FTP port.
    #[arg(
        long,
        default_value_t = 22,
        help_heading = "Ad-hoc target (no config file)"
    )]
    port: u16,
    /// Ad-hoc: username (SSH/FTP).
    #[arg(long, help_heading = "Ad-hoc target (no config file)")]
    user: Option<String>,
    /// Ad-hoc: path to the SSH private key.
    #[arg(long, help_heading = "Ad-hoc target (no config file)")]
    key: Option<String>,
    /// Ad-hoc: login password / key passphrase / FTP password. WARNING: a value
    /// here is visible to other local users in `ps` and /proc/<pid>/cmdline —
    /// prefer the MORAINE_PASSWORD environment variable.
    #[arg(long, help_heading = "Ad-hoc target (no config file)")]
    password: Option<String>,
    /// Ad-hoc: destination root on the target (a <name>/<timestamp>/ tree is made under it).
    #[arg(long, help_heading = "Ad-hoc target (no config file)")]
    dest: Option<String>,
    /// Ad-hoc: a file/folder on this machine to back up. Repeat for several.
    #[arg(long = "source", help_heading = "Ad-hoc target (no config file)")]
    sources: Vec<String>,
    /// Ad-hoc: snapshot folder name under --dest (default: the host, else "adhoc").
    #[arg(long, help_heading = "Ad-hoc target (no config file)")]
    name: Option<String>,
    /// Ad-hoc: an exclude pattern. Repeat for several.
    #[arg(long = "exclude", help_heading = "Ad-hoc target (no config file)")]
    excludes: Vec<String>,
    /// Ad-hoc: bandwidth limit, e.g. 2M or 500K.
    #[arg(long, help_heading = "Ad-hoc target (no config file)")]
    bwlimit: Option<String>,
    /// Ad-hoc: require the host key to already be known (StrictHostKeyChecking=yes).
    #[arg(long, help_heading = "Ad-hoc target (no config file)")]
    strict_host_key: bool,
    /// Ad-hoc: encrypt the destination at rest (rclone/ftp) with this passphrase.
    /// Same leak warning as --password — prefer MORAINE_CRYPT_PASSWORD.
    #[arg(long, help_heading = "Ad-hoc target (no config file)")]
    crypt_password: Option<String>,
    /// Ad-hoc: optional crypt salt (rclone crypt's password2).
    #[arg(long, help_heading = "Ad-hoc target (no config file)")]
    crypt_salt: Option<String>,
}

impl AdHoc {
    /// True once the user has supplied enough to mean "run this ad-hoc target"
    /// rather than a target from the config file.
    fn is_set(&self) -> bool {
        self.host.is_some() || self.dest.is_some() || !self.sources.is_empty()
    }
}

/// Parse the `--backend` string into a `Backend`.
fn parse_backend(s: &str) -> Result<config::Backend> {
    match s.trim().to_lowercase().as_str() {
        "ssh" => Ok(config::Backend::Ssh),
        "rclone" => Ok(config::Backend::Rclone),
        "ftp" => Ok(config::Backend::Ftp),
        other => bail!("unknown --backend '{other}' — use ssh, rclone or ftp"),
    }
}

const EXAMPLE_CONFIG: &str = include_str!("../moraine.example.toml");

fn main() {
    // If ssh launched us as its SSH_ASKPASS helper (Windows), print the secret
    // and exit before doing anything else. No-op unless MORAINE_ASKPASS is set.
    ssh::maybe_run_as_askpass();
    // Find rsync/rclone bundled next to the exe (Windows installer).
    tools::add_bundled_tools_to_path();
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { force } => cmd_init(&cli.config, force),
        Command::Run {
            target,
            dry_run,
            adhoc,
        } => cmd_run(&cli.config, target.as_deref(), dry_run, adhoc),
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

fn cmd_run(path: &Path, target: Option<&str>, dry_run: bool, adhoc: AdHoc) -> Result<()> {
    // Ad-hoc mode: build one target from the flags and run it, no config file.
    if adhoc.is_set() {
        if target.is_some() {
            bail!(
                "--target names a target from the config; it can't be combined with the \
                 ad-hoc --host/--dest/--source flags"
            );
        }
        return cmd_run_adhoc(adhoc, dry_run);
    }
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
        // One Moraine process per target: cron + a manual run (or the desktop
        // app) writing the same snapshot tree concurrently would corrupt it.
        let _lock = match lock::acquire(t) {
            Ok(l) => l,
            Err(e) => {
                // A busy target is an overlap, not a failed backup — another
                // run is actively backing this target up. Record it and fail
                // the exit code, but do NOT ping the healthcheck /fail
                // endpoint or raise a failure notification: a cron job
                // overlapping a long-running seed backup would otherwise page
                // the user "backup failing" while the backup succeeds. (If
                // runs keep getting skipped, the healthcheck alerts anyway —
                // by the *absence* of success pings.)
                failures += 1;
                eprintln!("  {e:#}");
                if !dry_run {
                    log(
                        path,
                        LogEntry::new("backup", &t.name, false, format!("{e:#}")),
                    );
                }
                continue;
            }
        };
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
        // Auto-prune after a successful backup — while the VPN is still up
        // (pruning needs the same connectivity the backup did).
        let prune_result = match &result {
            Ok(_) => prune_target(path, t, dry_run),
            Err(_) => Ok(()),
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
                // A prune failure must be visible: it fails the exit code and
                // is logged, otherwise a broken retention setup can silently
                // fill the target disk while every backup reports success.
                if let Err(e) = prune_result {
                    failures += 1;
                    eprintln!("  prune failed: {e:#}");
                    if !dry_run {
                        log(
                            path,
                            LogEntry::new("prune", &t.name, false, format!("{e:#}")),
                        );
                    }
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

/// Build a single target from `--host/--dest/--source/…` flags and run it, with
/// no config file. Secrets come from the flag or, preferably, an env var.
fn cmd_run_adhoc(a: AdHoc, dry_run: bool) -> Result<()> {
    let backend = parse_backend(&a.backend)?;
    // Flag wins, else the env var (which never appears in ps/proc); else none.
    let password = a
        .password
        .or_else(|| std::env::var("MORAINE_PASSWORD").ok())
        .unwrap_or_default();
    let crypt_password = a
        .crypt_password
        .or_else(|| std::env::var("MORAINE_CRYPT_PASSWORD").ok())
        .unwrap_or_default();
    let host = a.host.unwrap_or_default().trim().to_string();
    // Snapshot folder: the given name, else the host's first label, else "adhoc".
    let name = a
        .name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match host.split(['.', ':']).next().unwrap_or("") {
            "" => "adhoc".to_string(),
            h => h.to_string(),
        });
    let sources: Vec<String> = a
        .sources
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Required-flag checks with actionable messages (validate() covers the rest).
    if a.dest.as_deref().unwrap_or("").trim().is_empty() {
        bail!("ad-hoc run needs --dest");
    }
    if sources.is_empty() {
        bail!("ad-hoc run needs at least one --source");
    }
    if matches!(backend, config::Backend::Ssh | config::Backend::Ftp) && host.is_empty() {
        bail!("--host is required for the ssh/ftp backend");
    }
    if backend.is_ssh() && a.user.as_deref().unwrap_or("").trim().is_empty() {
        bail!("--user is required for the ssh backend");
    }

    let target = config::Target {
        name,
        backend,
        host,
        user: a.user.unwrap_or_default().trim().to_string(),
        port: a.port,
        key: a
            .key
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty()),
        password, // not trimmed — a secret may legitimately have edge whitespace
        strict_host_key: a.strict_host_key,
        dest: a.dest.unwrap_or_default().trim().to_string(),
        sources,
        exclude: a.excludes,
        vpn: String::new(),
        healthcheck: String::new(),
        bwlimit: a.bwlimit.unwrap_or_default().trim().to_string(),
        crypt_password,
        crypt_salt: a.crypt_salt.unwrap_or_default(),
        retention: None,
    };

    // Reuse the same validation as a config file (name safety, argv-injection,
    // bwlimit shape, crypt/backend rules, duplicate source basenames, …).
    let cfg = Config {
        notify: None,
        targets: vec![target],
        schedules: Vec::new(),
    };
    cfg.validate()?;
    let t = &cfg.targets[0];

    println!("== {} ({}) ==", t.name, backend_dest(t));
    let _lock = lock::acquire(t)?;
    if t.backend.is_ssh() {
        rsync::run_target(t, dry_run)?;
    } else {
        rclone::run_target(t, dry_run)?;
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
/// Only real snapshot timestamps: `latest`, stray directories and anything else
/// under the base must never become "the newest snapshot" for check/restore.
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
            .collect::<Vec<_>>()
    } else {
        rclone::list_snapshots(t)?
    };
    snaps.retain(|s| snapshot::is_timestamp(s));
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
                // Same per-target lock as `run`: pruning during a backup could
                // delete the snapshot the backup hardlinks against.
                if let Err(e) = lock::acquire(t).and_then(|_lock| prune_target(path, t, dry_run)) {
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

    // Also collect snapshots left behind by interrupted runs — they are
    // invisible to the listing, so retention can never reclaim their space.
    // A successful backup cleans up after itself (finalize's rm / the marker
    // delete), but a target whose runs keep FAILING only ever accumulates;
    // prune is the escape hatch. Best-effort: a cleanup error must not stop
    // the prune. The caller holds the target lock, so nothing in flight can
    // be swept up.
    if !dry_run {
        if t.backend.is_ssh() {
            let ok = ssh_probe(t, &snapshot::cleanup_incomplete_cmd(t))
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !ok {
                eprintln!("  warning: could not clean interrupted snapshot work dirs");
            }
        } else {
            let cleaned = rclone::cleanup_stale(t, "");
            if !cleaned.is_empty() {
                println!("  prune: removed {} interrupted snapshot(s)", cleaned.len());
            }
        }
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

/// Pick out the targets to work with: a named one, or all. Zero configured
/// targets is an error with a pointer to the fix — not a silent no-op run.
fn select_targets<'a>(config: &'a Config, name: Option<&str>) -> Result<Vec<&'a config::Target>> {
    if config.targets.is_empty() {
        bail!("the config has no targets — add a [[target]] block, or use the ad-hoc flags (moraine run --help)");
    }
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
