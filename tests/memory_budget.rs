//! R-01: `--max-memory` and candidate bounds must be enforced before unbounded
//! retention/growth, not only as a post-hoc sum of kept outputs.

mod common;

use carvex::io::ImageSource;
use carvex::scan::{run_scan, ScanContext, ScanInput, ScanMode, DEFAULT_MAX_CANDIDATES};
use carvex::signatures::FileType;
use common::{scan_options, MemSource, JPEG_1X1};
use std::fs;

fn vm_hwm_kib() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let kib = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())?;
            return Some(kib);
        }
    }
    None
}

fn run_raw(image: Vec<u8>, opts: &carvex::scan::ScanOptions) -> anyhow::Result<()> {
    let src = MemSource::new(image);
    let dir = tempfile::tempdir()?;
    let ctx = ScanContext::default();
    let input = ScanInput {
        source: &src,
        image: "dense.img",
        image_identity: None,
        image_len: src.len(),
        volumes: &[],
        extra_extents: &[],
        out_dir: dir.path(),
    };
    run_scan(&input, opts, &ctx).map(|_| ())
}

#[test]
fn dense_successful_carves_fail_early_against_memory_budget() {
    // Many valid files that would all be retained if the budget were only
    // checked after carving/postfilter.
    let count = 30_000usize;
    let file = JPEG_1X1;
    let stride = file.len() + 16;
    let mut image = vec![0u8; count * stride];
    for i in 0..count {
        let start = i * stride;
        image[start..start + file.len()].copy_from_slice(file);
    }

    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Raw);
    opts.min_size = 1;
    opts.max_size = 64 * 1024;
    opts.max_truncated_size = 64 * 1024;
    // Minimum allowed budget: enough for some files/buffers, far too small for
    // all retained outputs.
    opts.max_memory = 16 * 1024 * 1024;
    opts.max_candidates = count as u64;

    let before = vm_hwm_kib().unwrap_or(0);
    let err = run_raw(image, &opts).expect_err("budget must stop dense retention");
    let after = vm_hwm_kib().unwrap_or(before);
    let msg = err.to_string();
    assert!(
        msg.contains("--max-memory") || msg.contains("max-memory"),
        "expected memory-budget error, got: {msg}"
    );

    // The scan must not retain all carved outputs or grow buffers without bound.
    // VmHWM is process-wide and monotonic; allow generous headroom for test
    // harness overhead but far below unbounded retention of all inputs.
    if before > 0 && after >= before {
        let delta_kib = after - before;
        assert!(
            delta_kib <= 256 * 1024,
            "peak RSS grew by {} KiB during bounded scan (before={}, after={})",
            delta_kib,
            before,
            after
        );
    }
}

#[test]
fn candidate_count_is_bounded_before_carving() {
    // Many plausible JPEG headers; with a tiny candidate cap the scan must fail
    // before evaluating them all.
    let count = 4096usize;
    let mut image = vec![0u8; count * 512 + 64];
    for i in 0..count {
        let off = i * 512;
        image[off..off + 4].copy_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0]);
    }
    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Raw);
    opts.min_size = 1;
    opts.max_candidates = 128;
    // Keep memory ample so the candidate bound is the enforced limit.
    opts.max_memory = 256 * 1024 * 1024;

    let err = run_raw(image, &opts).expect_err("candidate bound must be enforced");
    let msg = err.to_string();
    assert!(
        msg.contains("--max-candidates") || msg.contains("candidates"),
        "expected candidate-bound error, got: {msg}"
    );
}

#[test]
fn default_candidate_limit_is_finite() {
    assert_eq!(DEFAULT_MAX_CANDIDATES, 1_000_000);
}
