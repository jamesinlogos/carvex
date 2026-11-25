//! B-06 acceptance coverage for the S12 interleaved-fragment scenario.

mod common;

use carvex::fs;
use carvex::io::ImageSource;
use carvex::scan::{run_scan, ScanContext, ScanInput, ScanMode};
use carvex::signatures::{FileType, Status, Validation, ValidationMode, Validity};
use common::{
    fat32_interleaved, fat32_volume_sectors_spc, jpeg_with_cluster_split, pdf_with_cluster_split,
    png_with_cluster_split, scan_options, MemSource,
};
use std::collections::HashSet;
use std::fs as std_fs;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

const CLUSTER: usize = 4096;

fn run_bgc(
    image: Vec<u8>,
    opts: &carvex::scan::ScanOptions,
) -> (TempDir, carvex::scan::ScanReport) {
    let src = MemSource::new(image);
    let fs_info = fs::detect(&src, src.len(), None, None).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let ctx = ScanContext {
        argv: vec!["fragment-recovery-test".to_string()],
        ..Default::default()
    };
    let volumes = std::slice::from_ref(&fs_info);
    let regions = Arc::new(Mutex::new(Vec::new()));
    let input = ScanInput {
        source: &src,
        image: "s12.img",
        image_identity: None,
        image_len: src.len(),
        volumes,
        extra_extents: &[],
        out_dir: dir.path(),
        unreadable_regions: &regions,
        skip_unreadable: false,
        interrupt: None,
    };
    let report = run_scan(&input, opts, &ctx).unwrap();
    (dir, report)
}

// H-34: this scenario intentionally keeps all S12 assertions together; test-only length is outside A-10.
#[test]
#[allow(clippy::too_many_lines)]
fn s12_interleaved_fragments_are_recovered_exactly() {
    let cases = [
        ("jpeg", FileType::Jpeg, jpeg_with_cluster_split(CLUSTER)),
        ("png", FileType::Png, png_with_cluster_split(CLUSTER)),
        ("pdf", FileType::Pdf, pdf_with_cluster_split(CLUSTER)),
    ];

    for (name, kind, file_a) in cases {
        // Use a second, distinct file of the same type so a splice can be
        // detected by hash.  The fixtures differ only in filler bytes.
        let file_b = match kind {
            FileType::Jpeg => {
                let mut b = file_a.clone();
                b[10] = b'X';
                b
            }
            FileType::Png => {
                let mut b = file_a.clone();
                // Change a tEXt filler byte, leaving chunk CRCs to be fixed.
                let pos = b.windows(7).position(|w| w == b"Comment").unwrap() + 7;
                b[pos] = b'B';
                let len = u32::from_be_bytes(b[33..37].try_into().unwrap()) as usize;
                let mut hasher = crc32fast::Hasher::new();
                hasher.update(&b[37..41]);
                hasher.update(&b[41..41 + len]);
                let crc = hasher.finalize().to_be_bytes();
                b[41 + len..45 + len].copy_from_slice(&crc);
                b
            }
            FileType::Pdf => {
                let mut b = file_a.clone();
                b[20] = b'X';
                b
            }
        };
        assert_ne!(
            file_a, file_b,
            "{name}: fixtures must be distinct for splice detection"
        );

        let image = fat32_interleaved(&file_a, &file_b, CLUSTER);
        let mut opts = scan_options(vec![kind], ScanMode::Auto);
        opts.min_size = 1;
        opts.fragment_recovery = true;
        // B-06 always deep-validates its stitches; this setting only controls
        // ordinary contiguous carves.
        opts.validate = ValidationMode::Structure;
        let (_dir, report) = run_bgc(image, &opts);

        let hashes: HashSet<_> = report.files.iter().map(|f| f.sha256.clone()).collect();
        let expected_a = carvex::util::sha256_hex(&file_a);
        let expected_b = carvex::util::sha256_hex(&file_b);
        assert!(
            hashes.contains(&expected_a),
            "{name}: file A not recovered exactly: {:#?}",
            report.files
        );
        assert!(
            hashes.contains(&expected_b),
            "{name}: file B not recovered exactly: {:#?}",
            report.files
        );

        // H-29: A always needs stitching. For PDF, B's contiguous carve also
        // contains A's trailer, so its multiple-EOF signal forces a B-06 join
        // even though the legacy deep validator accepts that splice.
        let stitched: Vec<_> = report.files.iter().filter(|f| f.source == "bgc").collect();
        let expected_stitched = if kind == FileType::Pdf { 2 } else { 1 };
        assert_eq!(
            stitched.len(),
            expected_stitched,
            "{name}: unexpected stitched outputs: {:#?}",
            report.files
        );
        assert!(
            stitched
                .iter()
                .all(|f| f.sha256 == expected_a || f.sha256 == expected_b),
            "{name}: a stitch did not reproduce an original file"
        );
        for file in stitched {
            assert_eq!(file.status, Status::Complete);
            // H-29 supersedes the stage-1 PDF lexical exception: a stitch is
            // only emitted when the authoritative label is `verified`.
            assert_eq!(file.validity, Validity::Verified);
            // H-19 stage 2: a complete stitched PDF passes the bounded
            // xref/trailer/catalog checks and is Decoded; B-06 still requires
            // deep validation for every type.
            assert_eq!(
                file.validation,
                Validation::Decoded,
                "{name}: unexpected validation for B-06 stitch"
            );
            if kind == FileType::Pdf {
                assert!(
                    file.notes.iter().any(|n| n
                        .contains("PDF cross-reference, trailer, size and catalog checks passed")),
                    "{name}: PDF B-06 stitch missing structural validation note: {:?}",
                    file.notes
                );
            }
            assert_eq!(file.fragments.len(), 2);
            assert!(file.fragments[0].len > 0);
            assert!(file.fragments[1].len > 0);
            assert!(file.checks.unwrap_or(0) > 0, "{name}: missing check count");
            assert!(
                file.alternatives.unwrap_or(0) > 0,
                "{name}: missing alternatives count"
            );
            assert!(
                file.confidence.unwrap_or(0) > 0,
                "{name}: missing confidence"
            );
            assert!(
                file.notes.iter().any(|n| n.contains("B-06")),
                "{name}: B-06 note missing"
            );
        }

        if kind != FileType::Pdf {
            let contiguous = report
                .files
                .iter()
                .find(|f| f.sha256 == expected_b)
                .unwrap_or_else(|| panic!("{name}: file B missing from report"));
            assert_eq!(
                contiguous.source, "carve",
                "{name}: file B should not be stitched"
            );
            assert!(contiguous.fragments.is_empty());
            assert_eq!(contiguous.validity, Validity::Structural);
        }
        assert!(
            report
                .files
                .iter()
                .all(|f| f.validation != Validation::Decoded || f.source == "bgc"),
            "{name}: only stitched files may claim decode validation"
        );
    }
}

#[test]
fn invalid_bgc_limits_are_rejected() {
    let file = jpeg_with_cluster_split(CLUSTER);
    let image = fat32_interleaved(&file, &file, CLUSTER);
    let src = MemSource::new(image);
    let fs_info = fs::detect(&src, src.len(), None, None).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let volumes = std::slice::from_ref(&fs_info);
    let regions = Arc::new(Mutex::new(Vec::new()));
    let input = ScanInput {
        source: &src,
        image: "s12.img",
        image_identity: None,
        image_len: src.len(),
        volumes,
        extra_extents: &[],
        out_dir: dir.path(),
        unreadable_regions: &regions,
        skip_unreadable: false,
        interrupt: None,
    };
    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Auto);
    opts.min_size = 1;
    opts.fragment_recovery = true;
    opts.max_gap = 0;
    assert!(run_scan(&input, &opts, &ScanContext::default()).is_err());
}

fn assert_incomplete(report: &carvex::scan::ScanReport, code: &str) {
    let warning = report
        .warnings
        .iter()
        .find(|w| w.code == code)
        .unwrap_or_else(|| panic!("expected {code}, got {:?}", report.warnings));
    assert!(
        warning.incomplete(),
        "{code} must be an incomplete-work warning"
    );
}

#[test]
fn h29_work_unit_limit_is_authoritative_and_deterministic() {
    let file = jpeg_with_cluster_split(CLUSTER);
    let image = fat32_interleaved(&file, &file, CLUSTER);
    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Auto);
    opts.min_size = 1;
    opts.fragment_recovery = true;
    opts.validate = ValidationMode::Structure;
    opts.max_frag_candidates = 1;
    opts.threads = 1;

    let (_dir, report) = run_bgc(image, &opts);
    assert_incomplete(&report, "FRAGMENT_CANDIDATE_LIMIT");
    assert!(!report.nondeterministic_stop);
}

#[test]
fn h29_wall_clock_safety_net_is_flagged_as_nondeterministic() {
    let file = jpeg_with_cluster_split(CLUSTER);
    let image = fat32_interleaved(&file, &file, CLUSTER);
    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Auto);
    opts.min_size = 1;
    opts.fragment_recovery = true;
    opts.validate = ValidationMode::Structure;
    opts.frag_time_limit = Duration::from_nanos(1);
    opts.threads = 1;

    let (_dir, report) = run_bgc(image, &opts);
    assert_incomplete(&report, "FRAGMENT_TIME_LIMIT");
    assert!(report.nondeterministic_stop);
}

#[test]
fn h29_fragment_work_limit_exits_three() {
    let file = jpeg_with_cluster_split(CLUSTER);
    let image = fat32_interleaved(&file, &file, CLUSTER);
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("fragment.img");
    let report_path = dir.path().join("report.json");
    std_fs::write(&image_path, image).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_carvex"))
        .args([
            "scan",
            image_path.to_str().unwrap(),
            "-o",
            dir.path().join("out").to_str().unwrap(),
            "--quiet",
            "--types",
            "jpeg",
            "--min-size",
            "1",
            "--fragment-recovery",
            "--max-frag-candidates",
            "1",
            "--threads",
            "1",
            "--json-report",
            report_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std_fs::read(&report_path).unwrap()).unwrap();
    assert!(report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["code"] == "FRAGMENT_CANDIDATE_LIMIT"));
    assert_eq!(report["nondeterministic_stop"], false);
}

/// H-17 residual: the S-B06 fixtures stop before either B-06 budget binds, so
/// use a hostile layout with many distinct heads and valid footers instead.
fn hostile_fragment_image(heads: usize) -> Vec<u8> {
    const HEADS: usize = 64;
    const SPC: u8 = 8;
    assert!(heads <= HEADS);
    let mut image = fat32_volume_sectors_spc(2048, SPC);
    let fat_sectors = (2048u64 * 4 + 1024).div_ceil(512) as usize;
    let first_free = (1 + fat_sectors) * 512 + CLUSTER;
    let file = jpeg_with_cluster_split(CLUSTER);
    let tail = &file[CLUSTER..];
    assert!(tail.len() <= CLUSTER);
    let mut offset = first_free;
    for index in 0..heads {
        let mut head = file[..CLUSTER].to_vec();
        head[6] = (index + 1) as u8;
        image[offset..offset + CLUSTER].copy_from_slice(&head);
        offset += CLUSTER;
        offset += CLUSTER; // the gap remains zero-filled
        image[offset..offset + tail.len()].copy_from_slice(tail);
        offset += CLUSTER;
    }
    assert!(offset <= image.len());
    image
}

#[test]
fn h17_fragment_limits_bind_on_a_hostile_layout() {
    let image = hostile_fragment_image(64);
    let mut capped = scan_options(vec![FileType::Jpeg], ScanMode::Auto);
    capped.min_size = 1;
    capped.fragment_recovery = true;
    capped.validate = ValidationMode::Structure;
    capped.max_frag_candidates = 1;
    capped.frag_time_limit = Duration::from_secs(60);
    capped.threads = 1;
    let (_dir, report) = run_bgc(image.clone(), &capped);
    assert_incomplete(&report, "FRAGMENT_CANDIDATE_LIMIT");
    assert!(!report.nondeterministic_stop);

    let mut timed = scan_options(vec![FileType::Jpeg], ScanMode::Auto);
    timed.min_size = 1;
    timed.fragment_recovery = true;
    timed.validate = ValidationMode::Structure;
    timed.max_frag_candidates = 10_000;
    timed.frag_time_limit = Duration::from_nanos(1);
    timed.threads = 1;
    let (_dir, report) = run_bgc(image.clone(), &timed);
    assert_incomplete(&report, "FRAGMENT_TIME_LIMIT");
    assert!(report.nondeterministic_stop);

    let mut roomy = scan_options(vec![FileType::Jpeg], ScanMode::Auto);
    roomy.min_size = 1;
    roomy.fragment_recovery = true;
    roomy.validate = ValidationMode::Structure;
    roomy.max_frag_candidates = 10_000;
    roomy.frag_time_limit = Duration::from_secs(60);
    roomy.threads = 1;
    let (_dir, report) = run_bgc(image, &roomy);
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.code.starts_with("FRAGMENT_")),
        "raised limits should not truncate the hostile scan: {:?}",
        report.warnings
    );
}
