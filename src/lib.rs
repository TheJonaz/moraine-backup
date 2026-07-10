//! moraine — shared engine for the CLI and the desktop client.
//!
//! **Stability:** this library is the internal engine behind the `moraine`
//! and `moraine-gui` binaries. Its API is NOT a semver contract — modules,
//! functions and types may change in any release. The stable interfaces are
//! the CLI, the config file format and the on-target snapshot layout.

pub mod config;
pub mod healthcheck;
pub mod history;
pub mod lock;
pub mod notify;
pub mod prune;
pub mod rclone;
pub mod rsync;
pub mod snapshot;
pub mod ssh;
pub mod tools;
pub mod vpn;

/// Full version string: semver, plus the git short hash (when built from a
/// checkout) and the build date. Assembled by `build.rs`.
pub const VERSION: &str = env!("MORAINE_VERSION");
