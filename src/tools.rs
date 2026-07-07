//! Finding the backend tools (rsync / rclone) at runtime.
//!
//! On Windows the installer ships rsync and rclone next to the executable rather
//! than expecting them on PATH, so plain `Command::new("rsync")` wouldn't find
//! them. Prepending our own directory to PATH once at startup makes every spawn
//! site pick up the bundled copies, with no per-call changes.

/// On Windows, prepend the running executable's directory to `PATH` so bundled
/// `rsync.exe` / `rclone.exe` (and their DLLs) are found. No-op on other systems,
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
        if let Some(existing) = std::env::var_os("PATH") {
            path.push(";");
            path.push(existing);
        }
        std::env::set_var("PATH", path);
    }
}
