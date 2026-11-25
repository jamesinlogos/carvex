//! H-25: a cut file must end at the last verified byte, not at the buffer end.
//!
//! The fixtures are generated in memory (Appendix C `tail_image`): a truncated
//! file followed by zeros, `0xFF` fill, or deterministic random bytes.  The
//! assertions mirror the S-TAIL acceptance bounds: at most one output per cut,
//! bounded length, an exact prefix of the original, and an honest `partial`
//! label.

mod common;

use common::{chunk, pdf_min, png_1x1, JPEG_1X1};
use serde_json::Value;
use std::fs;
use std::process::Command;

fn seeded_bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state & 0xFF) as u8
        })
        .collect()
}

/// Three small, structurally different fixtures per format.  They are built
/// from the shared deterministic fixtures; the extra segments/chunks/comments
/// move the structural boundaries so a 60 % cut does not always land at the
/// same place.
fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    let jpeg_with_segment = |marker: u8, payload: &[u8]| {
        let mut v = vec![0xFF, 0xD8, 0xFF, marker];
        v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(payload);
        v.extend_from_slice(&JPEG_1X1[2..]);
        v
    };
    let png = png_1x1();
    let iend = png
        .windows(4)
        .position(|w| w == b"IEND")
        .unwrap_or(png.len())
        .saturating_sub(4);
    let png_with_text = |at: usize| {
        let mut v = Vec::new();
        v.extend_from_slice(png.get(..at).unwrap_or(&png));
        v.extend_from_slice(&chunk(b"tEXt", b"Comment\0H-25 tail fixture"));
        v.extend_from_slice(png.get(at..).unwrap_or(&[]));
        v
    };
    let pdf = pdf_min();
    let first_nl = pdf.iter().position(|b| *b == b'\n').unwrap_or(8) + 1;
    let pdf_with_comment = |at: usize| {
        let mut v = Vec::new();
        v.extend_from_slice(pdf.get(..at).unwrap_or(&pdf));
        v.extend_from_slice(b"% H-25 tail fixture\n");
        v.extend_from_slice(pdf.get(at..).unwrap_or(&[]));
        v
    };
    vec![
        ("jpeg", JPEG_1X1.to_vec()),
        ("jpeg", jpeg_with_segment(0xE1, b"Exif\0\0tail")),
        ("jpeg", jpeg_with_segment(0xFE, b"comment")),
        ("png", png.clone()),
        ("png", png_with_text(33.min(png.len()))),
        ("png", png_with_text(iend)),
        ("pdf", pdf.clone()),
        ("pdf", pdf_with_comment(first_nl)),
        ("pdf", pdf_with_comment(pdf.len().saturating_sub(6))),
    ]
}

struct Scan {
    report: Value,
    out_dir: std::path::PathBuf,
    exit: Option<i32>,
    // Keep the temp directory alive until the assertions have read the output.
    _dir: tempfile::TempDir,
}

fn scan(image: &[u8], kind: &str, extra: &[&str]) -> Scan {
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("tail.img");
    let out = dir.path().join("out");
    let report_path = dir.path().join("report.json");
    fs::write(&image_path, image).unwrap();
    let mut args = vec![
        "scan".to_string(),
        image_path.to_str().unwrap().to_string(),
        "-o".to_string(),
        out.to_str().unwrap().to_string(),
        "--mode".to_string(),
        "raw".to_string(),
        "--no-fs".to_string(),
        "--types".to_string(),
        kind.to_string(),
        "--min-score".to_string(),
        "0".to_string(),
        "--min-size".to_string(),
        "1".to_string(),
        "--quiet".to_string(),
        "--json-report".to_string(),
        report_path.to_str().unwrap().to_string(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    let proc = Command::new(env!("CARGO_BIN_EXE_carvex"))
        .args(&args)
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    Scan {
        report,
        out_dir: out,
        exit: proc.status.code(),
        _dir: dir,
    }
}

fn carved_bytes(scan: &Scan, file: &Value) -> Vec<u8> {
    let path = scan.out_dir.join(file["path"].as_str().unwrap());
    fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}; out dir {:?}",
            path.display(),
            fs::read_dir(&scan.out_dir).map(|d| d
                .filter_map(Result::ok)
                .map(|e| e.file_name())
                .collect::<Vec<_>>())
        )
    })
}

fn tail_image(cut: &[u8], pad: &[u8]) -> Vec<u8> {
    let mut image = vec![0u8; 4096];
    image.extend_from_slice(cut);
    image.extend_from_slice(pad);
    image.extend_from_slice(&vec![0u8; 8192]);
    image
}

fn assert_tail_bounds(full: &[u8], cut_len: usize, pad_len: usize, scan: &Scan) {
    let files = scan.report["files"].as_array().unwrap();
    assert!(
        files.len() <= 1,
        "more than one output for one cut: {files:#?}"
    );
    for file in files {
        let size = file["size"].as_u64().unwrap() as usize;
        assert!(
            size <= cut_len + 4096,
            "swallowed padding: {} cut={} size={} limit={} full_len={} report={:#}",
            scan.report["files"][0]["path"].as_str().unwrap_or("?"),
            cut_len,
            size,
            cut_len + 4096,
            full.len(),
            scan.report["stats"]
        );
        assert!(
            size >= cut_len.saturating_sub(4096),
            "lost verified prefix: {size}"
        );
        let carved = carved_bytes(scan, file);
        // Only the bytes before the cut are original; anything after it is
        // padding accepted by a segment length and bounded by `size <= cut+4K`.
        let common = size.min(cut_len).min(carved.len());
        assert_eq!(
            carved[..common],
            full[..common],
            "output is not a prefix of the original"
        );
        assert_eq!(file["status"], "truncated", "cut must stay truncated");
        assert_eq!(file["validity"], "partial", "cut must stay partial");
        assert_ne!(file["validation"], "decoded");
        assert_eq!(file["repaired"], false);
        let trimmed = file["tail_trimmed_bytes"].as_u64().unwrap() as usize;
        assert!(
            trimmed <= pad_len + 4096 + 8192,
            "trimmed {trimmed} from {pad_len} padding bytes"
        );
    }
}

#[test]
fn tail_padding_bounds_for_jpeg_png_and_pdf() {
    let fractions = [15usize, 22, 30, 38, 46, 54, 62, 70, 78, 85];
    for (kind, full) in fixtures() {
        for numerator in fractions {
            let cut_len = full.len() * numerator / 100;
            for pad_len in [4096usize, 1024 * 1024] {
                for (pad_id, pad) in [
                    vec![0u8; pad_len],
                    vec![0xFF; pad_len],
                    seeded_bytes(pad_len, 0x5A5A + numerator as u64),
                    b"ABCDEFGH".repeat(pad_len.div_ceil(8))[..pad_len].to_vec(),
                ]
                .into_iter()
                .enumerate()
                {
                    let image = tail_image(&full[..cut_len], &pad);
                    let scan = scan(&image, kind, &[]);
                    assert_eq!(scan.exit, Some(0), "report: {:#}", scan.report);
                    assert_tail_bounds(&full, cut_len, pad_len, &scan);
                    let _ = pad_id;
                }
            }
        }
    }
}

#[test]
fn jpeg_48mib_ff_padding_stays_bounded() {
    // The S-TAIL 48 MiB cell: an all-FF run used to be swallowed and pushed
    // the candidate over --max-truncated-size, losing the file entirely.
    let cut_len = JPEG_1X1.len() * 3 / 5;
    let image = tail_image(&JPEG_1X1[..cut_len], &vec![0xFF; 48 * 1024 * 1024]);
    let scan = scan(&image, "jpeg", &[]);
    assert_eq!(scan.exit, Some(0), "report: {:#}", scan.report);
    assert_tail_bounds(JPEG_1X1, cut_len, 48 * 1024 * 1024, &scan);
}

#[test]
fn jpeg_neighbour_is_recovered_without_suppressing_the_complete_file() {
    for gap in [0usize, 4096] {
        let cut_len = JPEG_1X1.len() * 3 / 5;
        let mut image = vec![0u8; 4096];
        image.extend_from_slice(&JPEG_1X1[..cut_len]);
        image.extend_from_slice(&vec![0u8; gap]);
        image.extend_from_slice(JPEG_1X1);
        image.extend_from_slice(&vec![0u8; 8192]);
        let scan = scan(&image, "jpeg", &[]);
        let files = scan.report["files"].as_array().unwrap();
        assert_eq!(files.len(), 2, "both files must be carved: {files:#?}");
        let complete = files
            .iter()
            .find(|f| f["status"] == "complete" || f["status"] == "verified")
            .unwrap_or_else(|| panic!("no complete neighbour: {files:#?}"));
        assert_eq!(
            carved_bytes(&scan, complete),
            JPEG_1X1,
            "neighbour must be byte-exact"
        );
        let partial = files.iter().find(|f| f["status"] == "truncated").unwrap();
        assert_eq!(partial["validity"], "partial");
    }
}

#[test]
fn oversized_truncated_candidate_warns_and_exits_three() {
    // 200 KiB entropy scan after SOF+SOS, cut inside it: the structural prefix
    // is far above a 64 KiB --max-truncated-size even after zero trimming.
    let mut full = vec![0xFF, 0xD8];
    let sof = [8u8, 0, 1, 0, 1, 1, 1, 0x11, 0];
    full.extend_from_slice(&[0xFF, 0xC0]);
    full.extend_from_slice(&((sof.len() + 2) as u16).to_be_bytes());
    full.extend_from_slice(&sof);
    let sos = [1u8, 1, 0x11, 0, 63, 0];
    full.extend_from_slice(&[0xFF, 0xDA]);
    full.extend_from_slice(&((sos.len() + 2) as u16).to_be_bytes());
    full.extend_from_slice(&sos);
    full.extend(std::iter::repeat_n(b'A', 200 * 1024));
    full.extend_from_slice(&[0xFF, 0xD9]);
    let cut = &full[..150 * 1024];
    let image = tail_image(cut, &[]);
    let scan = scan(&image, "jpeg", &["--max-truncated-size", "64K"]);
    assert_eq!(scan.exit, Some(3), "report: {:#}", scan.report);
    assert_eq!(
        scan.report["stats"]["too_large_truncated"], 1,
        "report: {:#}",
        scan.report
    );
    let warnings = scan.report["warnings"].as_array().unwrap();
    let warning = warnings
        .iter()
        .find(|w| w["code"] == "TRUNCATED_TOO_LARGE")
        .unwrap_or_else(|| panic!("warnings: {warnings:#?}"));
    assert_eq!(warning["type"], "jpeg");
    assert_eq!(warning["offset"], 4096);
    assert_eq!(warning["size"], 150 * 1024);
    assert_eq!(scan.report["schema_version"], 2);
}

#[test]
fn complete_files_with_padding_are_unchanged() {
    let image = tail_image(JPEG_1X1, &[0xFF; 4096]);
    let scan = scan(&image, "jpeg", &[]);
    let files = scan.report["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "complete file with padding: {files:#?}");
    assert_eq!(carved_bytes(&scan, &files[0]), JPEG_1X1);
    assert_eq!(files[0]["status"], "complete");
    assert_eq!(files[0]["tail_trimmed_bytes"], 0);
}
