//! H-12b / R2-01: scan work must be planned lazily from declared extents.
//!
//! The source below declares 512 TiB without allocating it. A plan that
//! materialises one task per chunk allocates gigabytes before any budget can
//! be checked; the fixed implementation streams bounded batches and lets the
//! wall-clock safety net stop the run with a partial report.

use carvex::io::ImageSource;
use carvex::scan::{run_scan, ScanContext, ScanInput, ScanMode, ScanOptions};
use carvex::signatures::FileType;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DECLARED_LEN: u64 = 512 * 1024 * 1024 * 1024 * 1024;

struct DeclaredSparseSource {
    len: u64,
}

impl ImageSource for DeclaredSparseSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.len {
            return Ok(0);
        }
        let n = buf
            .len()
            .min((self.len - offset).min(usize::MAX as u64) as usize);
        buf[..n].fill(0);
        Ok(n)
    }
}

fn vm_hwm_kib() -> u64 {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").expect("read /proc/self/status");
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("VmHWM:") {
                return value
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("parse VmHWM");
            }
        }
        panic!("VmHWM missing from /proc/self/status");
    }
    #[cfg(not(target_os = "linux"))]
    0
}

fn opts() -> ScanOptions {
    ScanOptions {
        mode: ScanMode::Raw,
        types: vec![FileType::Jpeg],
        chunk_size: 4 * 1024 * 1024,
        max_candidates: 1000,
        time_budget: Duration::from_secs(5),
        quiet: true,
        ..ScanOptions::default()
    }
}

#[test]
fn declared_size_plan_is_lazy_and_honours_the_time_budget() {
    let source = DeclaredSparseSource { len: DECLARED_LEN };
    let dir = tempfile::tempdir().expect("temp output dir");
    let regions = Arc::new(Mutex::new(Vec::new()));
    let input = ScanInput {
        source: &source,
        image: "declared-512-tib.img",
        image_identity: None,
        image_len: source.len(),
        volumes: &[],
        extra_extents: &[],
        out_dir: dir.path(),
        unreadable_regions: &regions,
        skip_unreadable: false,
        interrupt: None,
    };
    let before = vm_hwm_kib();
    let started = Instant::now();
    let report = run_scan(&input, &opts(), &ScanContext::default()).expect("scan declared size");
    let elapsed = started.elapsed();
    let growth_mib = vm_hwm_kib().saturating_sub(before) / 1024;

    assert!(elapsed < Duration::from_secs(7), "scan took {elapsed:?}");
    assert!(growth_mib < 64, "peak RSS grew by {growth_mib} MiB");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.code == "TIME_LIMIT" && w.incomplete()),
        "expected TIME_LIMIT warning, got {:?}",
        report.warnings
    );
    assert!(report.nondeterministic_stop);
    assert!(report.files.is_empty());
}
