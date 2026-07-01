//! moraine — shared engine for the CLI and the desktop client.

pub mod config;
pub mod history;
pub mod prune;
pub mod rclone;
pub mod rsync;
pub mod snapshot;
pub mod ssh;
pub mod vpn;

/// Full version string: semver, plus the git short hash (when built from a
/// checkout) and the build date. Assembled by `build.rs`.
pub const VERSION: &str = env!("MORAINE_VERSION");
