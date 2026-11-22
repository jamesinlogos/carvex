//! H-09: deterministic work limits and the wall-clock safety net must produce
//! coded warnings, a partial report and `nondeterministic_stop` only for time.

mod common;

use carvex::io::ImageSource;
use carvex::scan::{run_scan, ScanContext, ScanInput, ScanMode, ScanReport};
use carvex::signatures::FileType;
use common::{scan_options, MemSource, JPEG_1X1};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn run_raw(image: Vec<u8>, opts: &carvex::scan::ScanOptions) -> anyhow::Result<ScanReport> {
    let source = MemSource::new(image);
    let dir = tempfile::tempdir()?;
    let regions = Arc::new(Mutex::new(Vec::new()));
    let input = ScanInput {
        source: &source,
        image: "work-budgets.img",
        image_identity: None,
        image_len: source.len(),
        volumes: &[],
        extra_extents: &[],
        out_dir: dir.path(),
        unreadable_regions: &regions,
        skip_unreadable: false,
        interrupt: None,
    };
    run_scan(&input, opts, &ScanContext::default())
}

fn assert_code(report: &ScanReport, code: &str) {
    assert!(
        report
            .warnings
            .iter()
            .any(|w| { w.code == code && w.incomplete() }),
        "expected incomplete warning {code}, got {:?}",
        report.warnings
    );
}

#[test]
fn candidate_limit_keeps_a_deterministic_prefix() {
    let mut image = JPEG_1X1.to_vec();
    image.extend_from_slice(&[0u8; 512]);
    image.extend_from_slice(JPEG_1X1);
    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Raw);
    opts.min_size = 1;
    opts.max_candidates = 1;
    opts.threads = 1;

    let report = run_raw(image, &opts).unwrap();
    assert_code(&report, "CANDIDATE_LIMIT");
    assert_eq!(
        report.files.len(),
        1,
        "only the in-quota candidate is written"
    );
    assert!(!report.nondeterministic_stop);
}

#[test]
fn work_limit_skips_an_oversized_candidate() {
    let mut image = JPEG_1X1[..2].to_vec();
    image.extend_from_slice(&[0xff, 0xfe]);
    image.extend_from_slice(&4100u16.to_be_bytes());
    image.extend_from_slice(&vec![b'C'; 4098]);
    image.extend_from_slice(&JPEG_1X1[2..]);

    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Raw);
    opts.min_size = 1;
    opts.max_work_per_candidate = 1024;

    let report = run_raw(image, &opts).unwrap();
    assert_code(&report, "WORK_LIMIT");
    assert!(report.files.is_empty());
    assert!(!report.nondeterministic_stop);
}

#[test]
fn output_limit_stops_after_the_last_file_that_fits() {
    let mut image = JPEG_1X1.to_vec();
    image.extend_from_slice(&[0u8; 512]);
    image.extend_from_slice(JPEG_1X1);
    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Raw);
    opts.min_size = 1;
    opts.max_output = JPEG_1X1.len() as u64;
    opts.threads = 1;

    let report = run_raw(image, &opts).unwrap();
    assert_code(&report, "OUTPUT_LIMIT");
    assert_eq!(report.files.len(), 1, "only the file that fits is written");
    assert_eq!(report.stats.bytes_written, JPEG_1X1.len() as u64);
    assert!(!report.nondeterministic_stop);
}

#[test]
fn wall_clock_budget_flags_partial_results_as_nondeterministic() {
    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Raw);
    opts.min_size = 1;
    opts.time_budget = Duration::from_nanos(1);

    let report = run_raw(JPEG_1X1.to_vec(), &opts).unwrap();
    assert_code(&report, "TIME_LIMIT");
    assert!(report.nondeterministic_stop);
    assert!(report.files.is_empty());
}
