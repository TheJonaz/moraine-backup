//! Finding the backend tools (rsync / rclone) at runtime.
//!
//! On Windows the installer ships the backend tools next to the executable rather
//! than expecting them on PATH, so plain `Command::new("rsync")` wouldn't find
//! them. Prepending our own directories to PATH once at startup makes every spawn
//! site pick up the bundled copies, with no per-call changes.
//!
//! Layout: `rclone.exe` (a native Windows exe) sits next to the app, while the
//! msys/cygwin tools live in a `usr\bin\` subdir. That subdir isn't just cosmetic:
//! cygwin derives its POSIX root from where `msys-2.0.dll` sits, and only finds
//! the bundled `etc\fstab` / `etc\nsswitch.conf` when the DLL is under a real
//! `usr\bin` root — which is what makes `/c/…` drive paths resolve and gives ssh
//! a valid HOME for known_hosts.

/// A directory only the current user can reach, for short-lived files that must
/// never be world-readable — currently the passphrase handed to `gpg` during a
/// config export.
///
/// Prefers `$XDG_RUNTIME_DIR` (`/run/user/UID`: already mode 0700, per-user, and
/// a tmpfs, so contents never reach a disk), then the user's cache dir, then the
/// OS temp dir. Never a shared, predictable path like `/tmp/moraine`, which a
/// local attacker could pre-plant.
pub fn private_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache"))
        })
        .or_else(|| {
            // Windows has no XDG dirs; LOCALAPPDATA is per-user and ACL'd.
            std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from)
        })?;
    let dir = base.join("moraine");
    std::fs::create_dir_all(&dir).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Also tightens a looser pre-existing directory.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
    }
    Some(dir)
}

/// Extension on `Command` to stop Windows flashing a console window every time
/// the (windowless) GUI spawns a console program — rsync, ssh, rclone, curl,
/// schtasks. Applied at every spawn site; a no-op on Linux/macOS.
pub trait CommandExt {
    /// Add `CREATE_NO_WINDOW` on Windows so no `cmd`/console window pops up.
    fn no_console(&mut self) -> &mut Self;
}

impl CommandExt for std::process::Command {
    fn no_console(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            self.creation_flags(CREATE_NO_WINDOW);
        }
        self
    }
}

/// On Windows, prepend the executable's directory *and* its `usr\bin` subdir to
/// `PATH` so bundled `rclone.exe` (next to the app) and the msys `rsync.exe` /
/// `moraine-ssh.exe` (under `usr\bin`) are all found. No-op on other systems,
/// where the tools come from the system package manager.
pub fn add_bundled_tools_to_path() {
    #[cfg(windows)]
    {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let Some(dir) = exe.parent() else {
            return;
        };
        let mut path = std::ffi::OsString::from(dir);
        path.push("\\usr\\bin;");
        path.push(dir);
        if let Some(existing) = std::env::var_os("PATH") {
            path.push(";");
            path.push(existing);
        }
        std::env::set_var("PATH", path);
    }
}
