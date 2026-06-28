//! Build script: embeds the git hash and build date into the binary via env
//! variables that become available with `env!("GIT_HASH")` / `env!("BUILD_DATE")`.

use std::process::Command;

fn main() {
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "nogit".to_string());
    println!("cargo:rustc-env=GIT_HASH={git_hash}");

    let build_date = Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_DATE={build_date}");

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
