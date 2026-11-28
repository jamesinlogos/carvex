//! Build script that records the Git commit for CarveX reports.
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    println!("cargo:rerun-if-env-changed=CARVEX_REQUIRE_MKNTFS");
    println!("cargo:rustc-check-cfg=cfg(carvex_has_mkntfs)");

    // H-29: the five mkntfs-dependent integration tests are visible SKIPs by
    // default. CI opts in (and installs ntfs-3g) so a missing tool is a hard
    // failure there instead of a silent pass.
    if require_mkntfs() {
        println!("cargo:rustc-cfg=carvex_has_mkntfs");
    }

    let commit = env_commit()
        .or_else(git_commit)
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=CARVEX_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=CARVEX_RUSTC={}", rustc_version());
}

fn require_mkntfs() -> bool {
    let required = std::env::var("CARVEX_REQUIRE_MKNTFS").as_deref() == Ok("1");
    let linux = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux");
    required && linux
}

fn rustc_version() -> String {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "unknown".into())
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
