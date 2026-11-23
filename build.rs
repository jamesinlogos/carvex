//! Build script that records the Git commit for CarveX reports.
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");

    let commit = env_commit()
        .or_else(git_commit)
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=CARVEX_GIT_COMMIT={commit}");
}

fn env_commit() -> Option<String> {
    let v = std::env::var("CARVEX_GIT_COMMIT").ok()?;
    let trimmed = v.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn git_commit() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8(out.stdout).ok()?;
    let sha = sha.trim();
    if sha.is_empty() {
        return None;
    }
    let dirty = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false);
    if dirty {
        Some(format!("{sha}-dirty"))
    } else {
        Some(sha.to_string())
    }
}
