//! moraine — shared engine for the CLI and the desktop client.

pub mod config;
pub mod history;
pub mod prune;
pub mod rclone;
pub mod rsync;
pub mod snapshot;
pub mod ssh;

/// Full version string: semver + git hash + build date.
/// Set by `build.rs` (GIT_HASH/BUILD_DATE) at compile time.
pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("GIT_HASH"),
    ", ",
    env!("BUILD_DATE"),
    ")"
);
