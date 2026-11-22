//! B-06 acceptance coverage for the S12 interleaved-fragment scenario.

mod common;

use carvex::fs;
use carvex::io::ImageSource;
use carvex::scan::{run_scan, ScanContext, ScanInput, ScanMode};
use carvex::signatures::{FileType, Status, Validation, ValidationMode};
use common::{
    fat32_interleaved, jpeg_with_cluster_split, pdf_with_cluster_split, png_with_cluster_split,
    scan_options, MemSource,
};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
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
    };
    let report = run_scan(&input, opts, &ctx).unwrap();
    (dir, report)
}

#[test]
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

        let stitched: Vec<_> = report.files.iter().filter(|f| f.source == "bgc").collect();
        assert_eq!(
            stitched.len(),
            2,
            "{name}: expected exactly two stitched outputs, got {:#?}",
            report.files
        );
        for file in stitched {
            assert_eq!(file.status, Status::Complete);
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
                file.confidence.unwrap_or(0) > 0,
                "{name}: missing confidence"
            );
            assert!(
                file.notes.iter().any(|n| n.contains("B-06")),
                "{name}: B-06 note missing"
            );
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
    };
    let mut opts = scan_options(vec![FileType::Jpeg], ScanMode::Auto);
    opts.min_size = 1;
    opts.fragment_recovery = true;
    opts.max_gap = 0;
    assert!(run_scan(&input, &opts, &ScanContext::default()).is_err());
}
