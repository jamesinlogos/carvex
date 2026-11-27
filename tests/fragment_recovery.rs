//! B-06 acceptance coverage for the S12 interleaved-fragment scenario.

mod common;

use carvex::fs;
use carvex::io::ImageSource;
use carvex::scan::{run_scan, ScanContext, ScanInput, ScanMode};
use carvex::signatures::{FileType, Status, Validation, ValidationMode, Validity};
use common::{
    fat32_first_free_offset_spc, fat32_interleaved, fat32_volume_sectors_spc,
    jpeg_with_cluster_split, pdf_with_cluster_split, png_with_cluster_split, scan_options,
    MemSource,
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

/// H-30: plant a PNG in two forward fragments, with `gap` free clusters
/// between them. The split falls inside an IDAT chunk and the gap bytes are
/// caller-supplied; the head ends at a cluster boundary.
fn fat32_png_bifragment(
    data: &[u8],
    split: usize,
    gap: usize,
    island: &[u8],
    live: bool,
) -> Vec<u8> {
    const TOTAL_SECTORS: u32 = 8192;
    let mut image = fat32_volume_sectors_spc(TOTAL_SECTORS, 8);
    let first = fat32_first_free_offset_spc(TOTAL_SECTORS, 8);
    let tail_offset = first + split * CLUSTER + gap * CLUSTER;
    assert!(island.len() >= gap * CLUSTER);
    image[first..first + split * CLUSTER].copy_from_slice(&data[..split * CLUSTER]);
    image[first + split * CLUSTER..tail_offset].copy_from_slice(&island[..gap * CLUSTER]);
    let tail = &data[split * CLUSTER..];
    image[tail_offset..tail_offset + tail.len()].copy_from_slice(tail);
    if live {
        // FAT starts after the one reserved sector in this fixture builder.
        let fat = 512;
        for i in 0..gap {
            let cluster = first / CLUSTER + split + i;
            let next = cluster + 1;
            image[fat + cluster * 4..fat + cluster * 4 + 4]
                .copy_from_slice(&(next as u32).to_le_bytes());
        }
    }
    image
}

fn h30_opts() -> carvex::scan::ScanOptions {
    let mut opts = scan_options(vec![FileType::Png], ScanMode::Auto);
    opts.min_size = 1;
    opts.fragment_recovery = true;
    opts.validate = ValidationMode::Decode;
    opts
}

fn corpus_png(name: &str) -> Vec<u8> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/corpus/png");
    std_fs::read(root.join(name)).unwrap_or_else(|e| panic!("missing corpus fixture {name}: {e}"))
}

#[test]
fn h30_png_crc_join_recovers_random_and_other_png_gaps() {
    let cases = [
        ("png_03_grayscale_512x512.png", 9usize, 5usize),
        ("png_04_palette_600x400.png", 2, 16),
        ("png_01_rgba_640x480.png", 17, 3),
    ];
    let other = corpus_png("png_06_large_1200x800.png");
    for (name, split, gap) in cases {
        let data = corpus_png(name);
        for (kind, island) in [
            ("random", vec![0xA5u8; gap * CLUSTER]),
            ("other PNG", other[CLUSTER..(gap + 1) * CLUSTER].to_vec()),
        ] {
            let image = fat32_png_bifragment(&data, split, gap, &island, false);
            let (_dir, report) = run_bgc(image, &h30_opts());
            let joined = report
                .files
                .iter()
                .find(|f| f.source == "bgc")
                .unwrap_or_else(|| panic!("{name}/{kind}: no B-06 join: {:#?}", report.files));
            assert_eq!(joined.sha256, carvex::util::sha256_hex(&data));
            assert_eq!(joined.validity, Validity::Verified);
            assert_eq!(joined.validation, Validation::Decoded);
            assert_eq!(joined.fragments.len(), 2);
            let join = joined.join.as_ref().expect("H-30 join evidence");
            assert_eq!(join.method, "png_chunk_crc");
            assert!(join.unique, "{name}/{kind}: expected one unique join");
            assert_eq!(join.gap_clusters, gap as u64);
            assert!(joined.join_alternatives.is_none());
        }
    }
}

#[test]
fn h30_controls_leave_intact_and_missing_pngs_unchanged() {
    let data = corpus_png("png_03_grayscale_512x512.png");
    let total = 8192u32;
    let first = fat32_first_free_offset_spc(total, 8);
    let mut intact = fat32_volume_sectors_spc(total, 8);
    intact[first..first + data.len()].copy_from_slice(&data);
    let (_dir, report) = run_bgc(intact, &h30_opts());
    assert!(
        report.files.iter().all(|f| f.source != "bgc"),
        "intact PNG must not be joined"
    );

    let mut missing = fat32_volume_sectors_spc(total, 8);
    let head = 9 * CLUSTER;
    missing[first..first + head].copy_from_slice(&data[..head]);
    let (_dir, report) = run_bgc(missing, &h30_opts());
    assert!(
        report
            .files
            .iter()
            .all(|f| f.source != "bgc" || f.validity != Validity::Verified),
        "a missing tail must never be verified"
    );
    assert!(data.len() > 9 * CLUSTER);
}

#[test]
fn h30_live_island_is_not_read_in_unallocated_mode() {
    let data = corpus_png("png_03_grayscale_512x512.png");
    let gap = 5;
    let image = fat32_png_bifragment(&data, 9, gap, &vec![0x5Au8; gap * CLUSTER], true);
    let (_dir, report) = run_bgc(image, &h30_opts());
    assert!(
        report
            .files
            .iter()
            .all(|f| f.source != "bgc" || f.allocated != Some(true)),
        "unallocated mode must not join an allocated island"
    );
}

#[test]
fn h30_ambiguous_png_crc_joins_are_suspect() {
    // A chunk whose missing bytes appear more than once in the tail yields two
    // CRC-valid geometries. H-30 must emit one suspect row, not pick a winner.
    let data = corpus_png("png_05_1bit_400x300.png");
    let split = 1;
    let gap = 1;
    let mut island = vec![0u8; gap * CLUSTER];
    let missing_len = (41 + 14992 + 4 - split * CLUSTER).min(CLUSTER);
    island[..missing_len].copy_from_slice(&data[split * CLUSTER..split * CLUSTER + missing_len]);
    let image = fat32_png_bifragment(&data, split, gap, &island, false);
    let (_dir, report) = run_bgc(image, &h30_opts());
    let joined = report
        .files
        .iter()
        .find(|f| f.source == "bgc")
        .expect("a splice must still be reported");
    assert!(
        joined.join_alternatives.is_some() || joined.validity == Validity::Suspect,
        "an ambiguous join must expose alternatives or suspect validity"
    );
}

#[test]
fn h30_fuzz_helpers_are_smoke_tested() {
    carvex::scan::fuzz_png_crc_oracle(b"");
    carvex::scan::fuzz_png_crc_oracle(b"\x89PNG\r\n\x1a\nnot a PNG");
    let data = corpus_png("png_03_grayscale_512x512.png");
    carvex::scan::fuzz_png_crc_oracle(&data[..data.len().min(4096)]);
    carvex::scan::fuzz_fragment_join(&data[..data.len().min(4096)]);
}
