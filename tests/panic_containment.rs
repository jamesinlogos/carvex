//! H-10: a panic in one candidate must not abort the whole scan.

mod common;

use carvex::image::{open_image_with_options, set_test_panic_in_open, InputFormat, OpenOptions};
use carvex::io::ImageSource;
use carvex::scan::{run_scan, set_test_panic_at_offset, ScanContext, ScanInput, ScanMode};
use carvex::signatures::FileType;
use common::{scan_options, MemSource, JPEG_1X1};
use std::sync::{Arc, Mutex};

#[test]
fn one_panicking_candidate_does_not_stop_other_candidates() {
    let gap = 512usize;
    let panic_offset = (JPEG_1X1.len() + gap) as u64;
    let mut image = JPEG_1X1.to_vec();
    image.extend_from_slice(&vec![0u8; gap]);
    image.extend_from_slice(JPEG_1X1);

    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Raw);
    opts.min_size = 1;
    opts.threads = 1;
    set_test_panic_at_offset(panic_offset);
    let reset = ResetPanic;

    let source = MemSource::new(image);
    let dir = tempfile::tempdir().unwrap();
    let regions = Arc::new(Mutex::new(Vec::new()));
    let input = ScanInput {
        source: &source,
        image: "panic-containment.img",
        image_identity: None,
        image_len: source.len(),
        volumes: &[],
        extra_extents: &[],
        out_dir: dir.path(),
        unreadable_regions: &regions,
        skip_unreadable: false,
    };
    let report = run_scan(&input, &opts, &ScanContext::default()).unwrap();

    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.code == "CANDIDATE_PANIC" && w.incomplete()),
        "warnings: {:?}",
        report.warnings
    );
    assert_eq!(
        report.files.len(),
        1,
        "the healthy candidate must still be written"
    );
    assert_eq!(report.stats.internal_errors, 1);
    assert_eq!(report.internal_errors.len(), 1);
    let record = &report.internal_errors[0];
    assert_eq!(record.offset, panic_offset);
    assert_eq!(record.file_type, FileType::Jpeg);
    assert!(
        record.message.contains("test-only panic injected"),
        "message: {}",
        record.message
    );
    drop(reset);
}

#[test]
fn parser_panics_become_typed_errors() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("raw.img");
    std::fs::write(&image, b"not really an image").unwrap();
    set_test_panic_in_open(true);
    let reset = ResetOpenPanic;

    let Err(error) = open_image_with_options(&image, InputFormat::Raw, &OpenOptions::default())
    else {
        panic!("a parser panic must become a typed error");
    };
    assert!(
        error.to_string().contains("image parser panicked"),
        "error: {error}"
    );
    drop(reset);
}

#[test]
fn every_explicit_profile_keeps_panic_unwind() {
    let manifest = include_str!("../Cargo.toml");
    assert!(manifest.contains("[profile.dev]"));
    assert!(manifest.contains("[profile.release]"));
    for section in manifest.split("[profile.").skip(1) {
        let name = section.split(']').next().unwrap_or("");
        let body = section.split("\n[").next().unwrap_or(section);
        assert!(
            body.contains("panic = \"unwind\""),
            "profile {name} must keep panic = \"unwind\""
        );
    }
}

struct ResetPanic;

impl Drop for ResetPanic {
    fn drop(&mut self) {
        set_test_panic_at_offset(u64::MAX);
    }
}

struct ResetOpenPanic;

impl Drop for ResetOpenPanic {
    fn drop(&mut self) {
        set_test_panic_in_open(false);
    }
}
