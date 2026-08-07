//! OS-aware backup suggestions.
//!
//! Answers the beginner's first question — "what should I even back up?" — by
//! scanning the machine for a curated set of important locations and offering
//! them as a ready-to-use target. It only *suggests*: nothing is written or
//! transferred without the user acting on it.
//!
//! Paths are expressed with a leading `~` so the same string works for the
//! existence check ([`config::expand_tilde`]) and as a runtime source in the
//! config — no `%APPDATA%`/`$XDG_*` syntax that moraine wouldn't expand later.

use crate::config::{self, Backend, Target};
use std::path::PathBuf;

/// Broad grouping, used only to organise the suggestions for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Documents,
    Settings,
    Dev,
    Mail,
}

impl Category {
    pub fn label(&self) -> &'static str {
        match self {
            Category::Documents => "Documents & media",
            Category::Settings => "Application settings",
            Category::Dev => "Developer & keys",
            Category::Mail => "Mail",
        }
    }
}

/// One entry in the per-OS manifest: a place worth backing up.
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    /// Human label shown to the user.
    pub label: &'static str,
    pub category: Category,
    /// `~`-relative path (expands via [`config::expand_tilde`] on every OS).
    pub path: &'static str,
    /// Contains credentials/keys — copying it off-box needs encryption.
    pub sensitive: bool,
}

// The manifest is intentionally conservative: user data and settings that
// restore usefully, never whole-home sweeps that drag in caches, VM images or
// game libraries. Grow it here as apps move their config locations.

// Linux and the BSDs are one manifest, not four: a BSD desktop is the same XDG
// layout, the same `~/.config`, and the same dot-directories for Firefox and
// Thunderbird. Ports exist for FreeBSD, OpenBSD and NetBSD — without this arm
// they fall through to the empty fallback below, so `moraine recommend` builds
// and installs perfectly and then answers "nothing to suggest".
#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
))]
const MANIFEST: &[Candidate] = &[
    c("Documents", Category::Documents, "~/Documents", false),
    c("Desktop", Category::Documents, "~/Desktop", false),
    c("Pictures", Category::Documents, "~/Pictures", false),
    c("Music", Category::Documents, "~/Music", false),
    c("Videos", Category::Documents, "~/Videos", false),
    c(
        "App settings (~/.config)",
        Category::Settings,
        "~/.config",
        false,
    ),
    c("Firefox profile", Category::Settings, "~/.mozilla", false),
    c("Thunderbird mail", Category::Mail, "~/.thunderbird", false),
    c("Git config", Category::Dev, "~/.gitconfig", false),
    c("SSH keys", Category::Dev, "~/.ssh", true),
    c("GnuPG keys", Category::Dev, "~/.gnupg", true),
];

#[cfg(target_os = "macos")]
const MANIFEST: &[Candidate] = &[
    c("Documents", Category::Documents, "~/Documents", false),
    c("Desktop", Category::Documents, "~/Desktop", false),
    c("Pictures", Category::Documents, "~/Pictures", false),
    c("Movies", Category::Documents, "~/Movies", false),
    c("Music", Category::Documents, "~/Music", false),
    c(
        "App preferences",
        Category::Settings,
        "~/Library/Preferences",
        false,
    ),
    c("Mail", Category::Mail, "~/Library/Mail", false),
    c("Git config", Category::Dev, "~/.gitconfig", false),
    c("SSH keys", Category::Dev, "~/.ssh", true),
    c("GnuPG keys", Category::Dev, "~/.gnupg", true),
];

#[cfg(target_os = "windows")]
const MANIFEST: &[Candidate] = &[
    c("Documents", Category::Documents, "~/Documents", false),
    c("Desktop", Category::Documents, "~/Desktop", false),
    c("Pictures", Category::Documents, "~/Pictures", false),
    c("Music", Category::Documents, "~/Music", false),
    c("Videos", Category::Documents, "~/Videos", false),
    // Roaming only — AppData\Local is caches and machine-specific state.
    c(
        "App settings (Roaming)",
        Category::Settings,
        "~/AppData/Roaming",
        false,
    ),
    c("Git config", Category::Dev, "~/.gitconfig", false),
    c("SSH keys", Category::Dev, "~/.ssh", true),
];

// Anything else (Solaris, Haiku, …) builds but has nothing curated to suggest.
// `scan()` returning empty is handled by the callers, so this stays a real
// fallback rather than a compile error.
#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "macos",
    target_os = "windows",
)))]
const MANIFEST: &[Candidate] = &[];

/// `const`-friendly constructor so the manifests above read as tables.
const fn c(
    label: &'static str,
    category: Category,
    path: &'static str,
    sensitive: bool,
) -> Candidate {
    Candidate {
        label,
        category,
        path,
        sensitive,
    }
}

/// A manifest entry that actually exists on this machine.
#[derive(Debug, Clone)]
pub struct Found {
    pub candidate: Candidate,
    /// The resolved absolute path (used for display).
    pub resolved: PathBuf,
}

/// Junk that should never ride along inside the suggested locations. Applied as
/// rsync/rclone exclude patterns on the generated target.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    ".cache",
    "Cache",
    "Caches",
    "CachedData",
    "node_modules",
    "target",
    "__pycache__",
    ".venv",
    "*.tmp",
    "*.swp",
    ".DS_Store",
    "Thumbs.db",
];

/// Scan the machine and return the manifest entries that exist, in manifest
/// order (grouped by category for display by the caller).
pub fn scan() -> Vec<Found> {
    MANIFEST
        .iter()
        .filter_map(|&candidate| {
            let resolved = config::expand_tilde(candidate.path);
            resolved.exists().then_some(Found {
                candidate,
                resolved,
            })
        })
        .collect()
}

/// True if any found location holds credentials/keys — the caller should then
/// nudge the user towards encryption.
pub fn has_sensitive(found: &[Found]) -> bool {
    found.iter().any(|f| f.candidate.sensitive)
}

/// Build a starting-point target from the found locations. Transport fields
/// (`host`, `user`, `dest`) are left as placeholders for the user to fill.
pub fn suggested_target(name: &str, found: &[Found]) -> Target {
    Target {
        name: name.to_string(),
        backend: Backend::Ssh,
        host: String::new(),
        user: String::new(),
        port: 22,
        key: None,
        password_spec: String::new(),
        strict_host_key: false,
        dest: "/backups".to_string(),
        sources: found.iter().map(|f| f.candidate.path.to_string()).collect(),
        exclude: DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect(),
        vpn: String::new(),
        healthcheck: String::new(),
        bwlimit: String::new(),
        crypt_password_spec: String::new(),
        crypt_salt_spec: String::new(),
        retention: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // scan() resolves through HOME (via expand_tilde), which is process-global,
    // so mutating it would race the other tests. Assert the invariant instead:
    // every suggested path must actually exist, and each is a `~`-template the
    // config can expand at runtime.
    #[test]
    fn scan_only_returns_existing_tilde_paths() {
        for f in scan() {
            assert!(
                f.resolved.exists(),
                "{} should exist, it was returned by scan()",
                f.resolved.display()
            );
            assert!(
                f.candidate.path.starts_with('~'),
                "{} must be a ~-relative template",
                f.candidate.path
            );
        }
    }

    /// Guards the `cfg` arms above on whichever platform the tests run.
    ///
    /// The BSDs used to fall through to the empty fallback, so `moraine
    /// recommend` compiled, installed and shipped while answering "nothing to
    /// suggest" — a typo in one `target_os` string still compiles, so only an
    /// assertion catches it. A ports or pkgsrc builder running `make test`
    /// checks its own platform here.
    #[test]
    fn every_supported_platform_has_suggestions() {
        if cfg!(any(
            target_os = "linux",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly",
            target_os = "macos",
            target_os = "windows",
        )) {
            assert!(
                !MANIFEST.is_empty(),
                "this platform is one moraine ships packages for, so it needs a \
                 manifest — check the cfg arms in this file"
            );
        }
    }

    #[test]
    fn ssh_dir_is_flagged_sensitive() {
        let found = vec![Found {
            candidate: c("SSH keys", Category::Dev, "~/.ssh", true),
            resolved: PathBuf::from("/home/x/.ssh"),
        }];
        assert!(has_sensitive(&found));
    }
}
