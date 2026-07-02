//! Build script: assembles the full version string and exposes it as
//! `env!("MORAINE_VERSION")`. In a git checkout it includes the short hash; in
//! a source tarball (no `.git`, e.g. a Debian build) the hash is simply omitted
//! rather than shown as "nogit".

use std::process::Command;

fn main() {
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    // Honor SOURCE_DATE_EPOCH (reproducible-builds.org): two builds of the same
    // source then embed the same date. Fall back to the wall clock.
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let date_args: Vec<String> = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(epoch) => vec![
            "-u".into(),
            "-d".into(),
            format!("@{epoch}"),
            "+%Y-%m-%d".into(),
        ],
        Err(_) => vec!["+%Y-%m-%d".into()],
    };
    let build_date = Command::new("date")
        .args(&date_args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let pkg_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let version = match &git_hash {
        Some(hash) => format!("{pkg_version} ({hash}, {build_date})"),
        None => format!("{pkg_version} ({build_date})"),
    };
    println!("cargo:rustc-env=MORAINE_VERSION={version}");

    // Re-run when the checked-out commit changes. HEAD itself does NOT change
    // on a new commit to the same branch — the branch ref does — so watch HEAD,
    // the resolved branch ref (e.g. refs/heads/main) and the reflog. Only emit
    // for paths that exist (a source tarball has no .git).
    let git_dir = std::path::Path::new(".git");
    for rel in ["HEAD", "logs/HEAD"] {
        let p = git_dir.join(rel);
        if p.exists() {
            println!("cargo:rerun-if-changed={}", p.display());
        }
    }
    if let Ok(head) = std::fs::read_to_string(git_dir.join("HEAD")) {
        if let Some(ref_rel) = head.strip_prefix("ref:").map(str::trim) {
            let ref_path = git_dir.join(ref_rel);
            if ref_path.exists() {
                println!("cargo:rerun-if-changed={}", ref_path.display());
            }
        }
    }
}
