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

    // Rebuild the version string if HEAD changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
}
