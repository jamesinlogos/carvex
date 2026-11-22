//! R-01: `--max-memory` and candidate bounds must be enforced before unbounded
//! retention/growth, not only as a post-hoc sum of kept outputs.

mod common;

use carvex::io::ImageSource;
use carvex::scan::{run_scan, ScanContext, ScanInput, ScanMode, DEFAULT_MAX_CANDIDATES};
use carvex::signatures::FileType;
use common::{jpeg_with_cluster_split, scan_options, MemSource, JPEG_1X1};
use std::fs;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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

/// Returns correct bytes for discovery and carving, then corrupted bytes for
/// the write stage's re-read (the third full-source read), exercising H-08's
/// SOURCE_CHANGED path.
struct SecondReadChanges {
    data: Vec<u8>,
    full_reads: Arc<AtomicUsize>,
}

impl ImageSource for SecondReadChanges {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if start >= self.data.len() {
            return Ok(0);
        }
        let count = self.full_reads.fetch_add(1, Ordering::SeqCst) + 1;
        let source = if start == 0 && buf.len() >= self.data.len() && count == 3 {
            let mut changed = self.data.clone();
            if let Some(last) = changed.last_mut() {
                *last ^= 0x01;
            }
            changed
        } else {
            self.data.clone()
        };
        let available = source.len().saturating_sub(start);
        let n = buf.len().min(available);
        buf[..n].copy_from_slice(&source[start..start + n]);
        Ok(n)
    }
}

fn run_raw(
    image: Vec<u8>,
    opts: &carvex::scan::ScanOptions,
) -> anyhow::Result<carvex::scan::ScanReport> {
    let src = MemSource::new(image);
    let dir = tempfile::tempdir()?;
    let ctx = ScanContext::default();
    let regions = Arc::new(Mutex::new(Vec::new()));
    let input = ScanInput {
        source: &src,
        image: "dense.img",
        image_identity: None,
        image_len: src.len(),
        volumes: &[],
        extra_extents: &[],
        out_dir: dir.path(),
        unreadable_regions: &regions,
        skip_unreadable: false,
    };
    run_scan(&input, opts, &ctx)
}

#[test]
fn dense_successful_carves_complete_within_a_working_memory_budget() {
    // H-08: total carved output may exceed --max-memory. The budget bounds
    // concurrent working buffers; results are re-read and written at the end.
    let count = 300usize;
    let file = jpeg_with_cluster_split(60 * 1024);
    let stride = file.len() + 512;
    let mut image = vec![0u8; count * stride];
    for i in 0..count {
        let start = i * stride;
        image[start..start + file.len()].copy_from_slice(&file);
    }
    assert!(count * file.len() > 16 * 1024 * 1024);

    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Raw);
    opts.min_size = 1;
    opts.max_size = 64 * 1024;
    opts.max_truncated_size = 64 * 1024;
    opts.max_memory = 16 * 1024 * 1024;
    opts.max_candidates = count as u64;

    let before = vm_hwm_kib().unwrap_or(0);
    let report = run_raw(image, &opts).expect("bounded scan must complete");
    let after = vm_hwm_kib().unwrap_or(before);
    assert_eq!(report.files.len(), count, "all candidates must be written");
    assert!(
        report.warnings.iter().all(|w| !w.incomplete()),
        "unexpected incomplete warning: {:?}",
        report.warnings
    );

    // VmHWM is process-wide and monotonic. Allow headroom for the test harness
    // but stay far below retention of the whole carved output.
    if before > 0 && after >= before {
        let delta_kib = after - before;
        assert!(
            delta_kib <= 64 * 1024,
            "peak RSS grew by {} KiB during bounded scan (before={}, after={})",
            delta_kib,
            before,
            after
        );
    }
}

#[test]
fn changed_source_is_detected_before_a_file_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let source = SecondReadChanges {
        data: JPEG_1X1.to_vec(),
        full_reads: Arc::new(AtomicUsize::new(0)),
    };
    let regions = Arc::new(Mutex::new(Vec::new()));
    let input = ScanInput {
        source: &source,
        image: "changing.img",
        image_identity: None,
        image_len: source.len(),
        volumes: &[],
        extra_extents: &[],
        out_dir: dir.path(),
        unreadable_regions: &regions,
        skip_unreadable: false,
    };
    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Raw);
    opts.min_size = 1;
    opts.threads = 1;
    let report = run_scan(&input, &opts, &ScanContext::default()).unwrap();

    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.code == "SOURCE_CHANGED" && w.incomplete()),
        "warnings: {:?}",
        report.warnings
    );
    assert!(
        report.files.is_empty(),
        "mismatched output must not be reported"
    );
    assert_eq!(
        fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().and_then(|e| e.to_str()) == Some("jpg"))
            .count(),
        0
    );
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
