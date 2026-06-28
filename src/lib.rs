//! moraine — delad motor för CLI:t och desktop-klienten.

pub mod config;
pub mod history;
pub mod prune;
pub mod rclone;
pub mod rsync;
pub mod snapshot;
pub mod ssh;

/// Fullständig versionssträng: semver + git-hash + byggdatum.
/// Sätts av `build.rs` (GIT_HASH/BUILD_DATE) vid kompilering.
pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("GIT_HASH"),
    ", ",
    env!("BUILD_DATE"),
    ")"
);
