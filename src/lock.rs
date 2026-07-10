//! Cross-process, per-target lock: two Moraine processes (a cron run and a
//! manual one, or the CLI and the desktop app) must never back up or prune the
//! same target at the same time — overlapping runs could write into the same
//! snapshot directory or prune the snapshot the other one is building on.
//!
//! Advisory OS file locks (`flock` on Unix, `LockFileEx` on Windows, via
//! `std::fs::File::try_lock`): the OS releases them automatically when the
//! process exits, so a crashed run never leaves a stale lock behind. The lock
//! *file* is deliberately never deleted — unlinking a lock file opens the
//! classic race where two processes each lock a different inode at the same
//! path and both think they hold "the" lock.

use crate::config::Target;
use anyhow::{bail, Context, Result};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::PathBuf;

/// Holds the OS lock for one target; dropping it releases the lock.
#[derive(Debug)]
pub struct TargetLock {
    _file: File,
}

/// Tries to take the exclusive lock for a target. Fails immediately (never
/// waits) with a clear message when another Moraine process holds it.
pub fn acquire(target: &Target) -> Result<TargetLock> {
    let path = lock_path(target);
    let mut opts = OpenOptions::new();
    opts.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts
        .open(&path)
        .with_context(|| format!("could not open lock file {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(TargetLock { _file: file }),
        Err(TryLockError::WouldBlock) => bail!(
            "target '{}' is busy — another Moraine run (CLI or desktop app) is \
             already backing up or pruning it",
            target.name
        ),
        Err(TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("could not lock {}", path.display()))
        }
    }
}

/// Where the lock file lives: a per-user directory that resolves to the SAME
/// path no matter how the process was launched. Deliberately NOT
/// `XDG_RUNTIME_DIR`/`TMPDIR`: cron and Task Scheduler strip session
/// environment, so an env-dependent path would give the desktop app and a
/// scheduled CLI run *different* lock files — exactly the pair this lock must
/// serialize. HOME is set by cron (from passwd) and desktop sessions alike;
/// USERPROFILE is the Windows equivalent (set for scheduled tasks too).
fn lock_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty());
    match home {
        Some(h) => PathBuf::from(h).join(".cache").join("moraine"),
        // Last resort (no home at all — e.g. a bare service account): the temp
        // dir is per-user on Windows; on Unix it can be shared, but the lock
        // files are 0600 and content-free.
        None => std::env::temp_dir(),
    }
}

/// Lock file for one target, keyed on the target's *identity*
/// (backend/host/port/dest/name), so the same target is serialized even when
/// the CLI and the GUI read different config files. `dest` and `host` are
/// normalized the same way the snapshot paths are (`base_dir` trims the
/// trailing slash), so `dest = "/backups/"` and `"/backups"` — the same tree
/// on the target — contend on the same lock.
fn lock_path(target: &Target) -> PathBuf {
    let dir = lock_dir();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(unix)]
    {
        // Owner-only, like the askpass dir (also tightens a pre-existing one).
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let id = format!(
        "{:?}/{}/{}/{}/{}",
        target.backend,
        target.host.trim(),
        target.port,
        target.dest.trim_end_matches('/'),
        target.name
    );
    // The name is config-validated, but sanitize anyway: the file name must be
    // portable, and the hash alone already makes the path unique.
    let safe: String = target
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir.join(format!("moraine-{safe}-{:016x}.lock", fnv1a(&id)))
}

/// FNV-1a — tiny, dependency-free, and stable across runs (unlike `DefaultHasher`,
/// whose output may change between Rust releases; two differently-built binaries
/// must agree on the lock path).
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    fn target(name: &str) -> crate::config::Target {
        let cfg: Config = toml::from_str(&format!(
            r#"
            [[target]]
            name = "{name}"
            host = "h"
            user = "u"
            dest = "/d"
            sources = ["/s"]
            "#
        ))
        .unwrap();
        cfg.targets.into_iter().next().unwrap()
    }

    #[test]
    fn second_acquire_fails_until_the_first_is_dropped() {
        let t = target("lock-test-a");
        let first = super::acquire(&t).unwrap();
        let err = super::acquire(&t).unwrap_err().to_string();
        assert!(err.contains("busy"), "{err}");
        drop(first);
        super::acquire(&t).expect("lock must be free again after drop");
    }

    #[test]
    fn different_targets_do_not_contend() {
        let a = super::acquire(&target("lock-test-b")).unwrap();
        let _b = super::acquire(&target("lock-test-c")).expect("independent targets");
        drop(a);
    }

    #[test]
    fn dest_trailing_slash_is_the_same_lock() {
        // Two config files describing the same tree must contend: base_dir
        // trims the trailing slash, so the lock identity must too.
        fn with_dest(dest: &str) -> crate::config::Target {
            let cfg: Config = toml::from_str(&format!(
                r#"
                [[target]]
                name = "lock-test-d"
                host = "h"
                user = "u"
                dest = "{dest}"
                sources = ["/s"]
                "#
            ))
            .unwrap();
            cfg.targets.into_iter().next().unwrap()
        }
        let held = super::acquire(&with_dest("/d")).unwrap();
        let err = super::acquire(&with_dest("/d/")).unwrap_err().to_string();
        assert!(err.contains("busy"), "{err}");
        drop(held);
    }

    #[test]
    fn lock_dir_ignores_session_environment() {
        // The path must not depend on XDG_RUNTIME_DIR/TMPDIR — cron strips
        // them, and the GUI + a cron CLI must lock the SAME file.
        let p = super::lock_path(&target("lock-test-e"));
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
        if let Some(rt) = runtime {
            assert!(
                !p.starts_with(&rt) || rt == std::path::Path::new("/"),
                "lock path {p:?} must not live under session-only XDG_RUNTIME_DIR {rt:?}"
            );
        }
    }
}
