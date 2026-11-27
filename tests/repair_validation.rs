//! H-21: `--repair` must revalidate before claiming `repaired` and must expose
//! the hash of the bytes read from the source when it changes the output.
//!
//! Red first: the current scanner appends a static footer, labels every
//! truncated candidate `repaired`, and emits no `sha256_original`.

mod common;

use common::{pdf_min, pillow_python, png_1x1, JPEG_1X1};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

fn sha256(data: &[u8]) -> String {
    carvex::util::sha256_hex(data)
}

fn write_image(path: &Path, payload: &[u8]) {
    let mut image = vec![0u8; 4096];
    image.extend_from_slice(payload);
    image.extend_from_slice(&[0u8; 4096]);
    fs::write(path, image).unwrap();
}

/// Scan one payload with `--repair` and return the first file row plus the
/// bytes that were written.
fn scan_repaired_mode(payload: &[u8], name: &str, mode: &str) -> (Value, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join(format!("{name}-{mode}.img"));
    write_image(&image, payload);
    let out = dir.path().join("out");
    let report_path = dir.path().join("report.json");
    let csv_path = dir.path().join("report.csv");
    let proc = Command::new(env!("CARGO_BIN_EXE_carvex"))
        .args([
            "scan",
            image.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--mode",
            "raw",
            "--no-fs",
            "--types",
            name.split('-').next().unwrap(),
            "--validate",
            mode,
            "--min-score",
            "0",
            "--min-size",
            "1",
            "--repair",
            "--quiet",
            "--json-report",
            report_path.to_str().unwrap(),
            "--csv-report",
            csv_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&proc.stderr).into_owned();
    assert_eq!(proc.status.code(), Some(0), "stderr: {stderr}");

    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    let files = report["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "report: {report:#}");
    let file = files[0].clone();
    let carved = fs::read(out.join(file["path"].as_str().unwrap())).unwrap();
    let csv = fs::read_to_string(&csv_path).unwrap();
    assert!(
        csv.lines().next().unwrap().contains("sha256_original"),
        "CSV must expose sha256_original: {csv}"
    );
    assert_eq!(csv.lines().count(), 2, "unexpected CSV rows: {csv}");
    let header = csv.lines().next().unwrap();
    assert!(
        header.ends_with(",sha256_original,alternatives"),
        "CSV must append alternatives after sha256_original: {header}"
    );
    // Notes can contain quoted commas, so the two new numeric/identity columns
    // are read from the end of the row rather than by a naive split index.
    let row: Vec<_> = csv.lines().nth(1).unwrap().split(',').collect();
    assert_eq!(
        row[row.len() - 2],
        file["sha256_original"].as_str().unwrap_or_default(),
        "CSV sha256_original must match JSON"
    );
    assert_eq!(row[row.len() - 1], "", "repairs have no alternatives count");
    (file, carved)
}

fn scan_repaired(payload: &[u8], name: &str) -> (Value, Vec<u8>) {
    scan_repaired_mode(payload, name, "decode")
}

fn corpus_jpeg_01() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("bench/corpus/jpeg/jpeg_01_baseline_640x480.jpg");
    fs::read(path).expect("corpus JPEG fixture must exist")
}

fn truncated_pdf() -> Vec<u8> {
    let full = pdf_min();
    let cut = full
        .windows(b"xref".len())
        .position(|window| window == b"xref")
        .expect("pdf_min fixture has an xref");
    full[..cut].to_vec()
}

fn check(file: &Value, name: &str) -> Option<Value> {
    file["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == name)
        .cloned()
}

/// Independent Pillow proof for a crop repair: the output must load, and every
/// retained row must be the original's row. JPEG skips the last two rows because
/// chroma subsampling can make the bottom edge differ legitimately.
fn assert_pillow_top_rows(original: &[u8], carved: &[u8], kind: &str, rows: u32, slack: u32) {
    let dir = tempfile::tempdir().unwrap();
    let original_path = dir.path().join("original");
    let carved_path = dir.path().join("carved");
    fs::write(&original_path, original).unwrap();
    fs::write(&carved_path, carved).unwrap();
    let script = r#"
import sys
from PIL import Image
orig, out, kind, rows, slack = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5])
try:
    with Image.open(out) as probe:
        probe.verify()
    with Image.open(out) as probe:
        image = probe.convert('RGB')
    with Image.open(orig) as probe:
        reference = probe.convert('RGB')
except Exception as exc:
    sys.exit('decode failed: %r' % exc)
keep = min(image.height - slack, rows if rows else image.height)
if keep <= 0:
    sys.exit('no retained rows')
if image.crop((0, 0, image.width, keep)).tobytes() != reference.crop((0, 0, reference.width, keep)).tobytes():
    sys.exit('retained rows differ from the original')
if kind == 'jpeg' and image.height >= reference.height:
    sys.exit('repaired JPEG kept the full canvas')
print('OK')
"#;
    let (python, prefix) = pillow_python();
    let proc = Command::new(python)
        .args(prefix)
        .arg("-c")
        .arg(script)
        .arg(original_path)
        .arg(carved_path)
        .arg(kind)
        .arg(rows.to_string())
        .arg(slack.to_string())
        .output()
        .unwrap();
    assert!(
        proc.status.success(),
        "Pillow oracle rejected {kind} repair: {}",
        String::from_utf8_lossy(&proc.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&proc.stdout).trim(),
        "OK",
        "Pillow oracle: {}",
        String::from_utf8_lossy(&proc.stdout)
    );
}

#[test]
fn revalidated_pdf_repair_is_honest_and_hashes_the_original() {
    let original = truncated_pdf();
    let (file, carved) = scan_repaired(&original, "pdf");

    assert_ne!(
        carved, original,
        "a successful repair must change the bytes"
    );
    assert_eq!(file["status"], "repaired");
    assert_eq!(file["repaired"], true);
    assert_eq!(file["validation"], "decoded");
    assert_eq!(file["validity"], "repaired");
    assert_eq!(
        file["score"], 60,
        "repair must not inflate the legacy score"
    );
    assert_eq!(file["carved_size"], original.len() as u64);
    assert_eq!(file["sha256"], sha256(&carved));
    assert_eq!(file["sha256_original"], sha256(&original));
    assert!(String::from_utf8_lossy(&carved).contains("startxref"));
    assert!(String::from_utf8_lossy(&carved).contains("/Root 1 0 R"));
    assert_eq!(check(&file, "repair").unwrap()["ok"], true);
    assert_eq!(check(&file, "root_catalog").unwrap()["ok"], true);
    assert_eq!(check(&file, "xref_entries").unwrap()["ok"], true);
    assert!(file["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str().is_some_and(|n| n.contains("revalidated"))));
}

#[test]
fn unvalidatable_pdf_repair_stays_truncated_and_unchanged() {
    let mut original = truncated_pdf();
    let pos = original
        .windows(b"/Type /Catalog".len())
        .position(|window| window == b"/Type /Catalog")
        .expect("fixture has a catalog");
    original[pos + b"/Type /".len()..pos + b"/Type /Catalog".len()].copy_from_slice(b"Page   ");

    let (file, carved) = scan_repaired(&original, "pdf");

    assert_eq!(carved, original, "a failed repair must not alter the bytes");
    assert_eq!(file["status"], "truncated");
    assert_eq!(file["repaired"], false);
    assert_eq!(file["validation"], "partial");
    assert_eq!(file["validity"], "partial");
    assert!(file["sha256_original"].is_null());
    assert!(file["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str() == Some("repair attempted, not validated")));
    assert_eq!(check(&file, "repair").unwrap()["ok"], false);
}

#[test]
fn mid_stream_png_and_jpeg_repair_stays_partial() {
    // These tiny 1x1 fixtures leave no complete scanline/MCU row after a 60 %
    // cut, so neither footer repair nor crop repair is possible. Pin the honest
    // fallback: partial, no bytes changed, no repair hash, failed `repair` check.
    let jpeg_full = JPEG_1X1;
    let jpeg = &jpeg_full[..jpeg_full.len() * 3 / 5];
    let (jpeg_file, jpeg_carved) = scan_repaired(jpeg, "jpeg");
    assert_eq!(jpeg_carved, jpeg, "failed repair must not alter bytes");
    assert_eq!(jpeg_file["status"], "truncated");
    assert_eq!(jpeg_file["validity"], "partial");
    assert_eq!(jpeg_file["repaired"], false);
    assert!(jpeg_file["sha256_original"].is_null());
    assert!(jpeg_file["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str() == Some("repair attempted, not validated")));
    assert_eq!(check(&jpeg_file, "repair").unwrap()["ok"], false);

    let png_full = png_1x1();
    let png = &png_full[..png_full.len() * 3 / 5];
    let (png_file, png_carved) = scan_repaired(png, "png");
    assert_eq!(png_carved, png, "failed repair must not alter bytes");
    assert_eq!(png_file["status"], "truncated");
    assert_eq!(png_file["validity"], "partial");
    assert_eq!(png_file["repaired"], false);
    assert!(png_file["sha256_original"].is_null());
    assert!(png_file["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str() == Some("repair attempted, not validated")));
    assert_eq!(check(&png_file, "repair").unwrap()["ok"], false);
}

#[test]
fn jpeg_mid_stream_repair_structure_mode_stays_partial() {
    // External harness H-04 item 0 removed the structure-mode over-claim: a
    // repair needs a decoded proof in both modes. The item-2 crop design then
    // makes these mid-stream cuts recoverable, but only by emitting a shorter
    // JPEG whose retained rows equal the original's. The test name is kept for
    // traceability; the invariant is "no invented pixel", not "no repair".
    let original = corpus_jpeg_01();
    for (pct, expected_rows) in [(30usize, 8u32), (60, 17), (90, 27)] {
        let cut = &original[..original.len() * pct / 100];
        let mut decoded_validity: Option<String> = None;
        for mode in ["structure", "decode"] {
            let (file, carved) = scan_repaired_mode(cut, "jpeg", mode);
            assert_eq!(file["status"], "repaired", "at {pct}% in {mode} mode");
            assert_eq!(file["validity"], "repaired", "at {pct}% in {mode} mode");
            assert_eq!(file["repaired"], true, "at {pct}% in {mode} mode");
            assert_eq!(file["validation"], "decoded", "at {pct}% in {mode} mode");
            assert_eq!(file["repair_validated"], true, "at {pct}% in {mode} mode");
            assert_eq!(file["sha256"], sha256(&carved));
            assert_eq!(file["sha256_original"], sha256(cut));
            assert_eq!(
                file["recovered_rows"].as_u64(),
                Some(expected_rows as u64),
                "at {pct}% in {mode} mode"
            );
            assert_eq!(
                file["total_rows"].as_u64(),
                Some(30),
                "at {pct}% in {mode} mode"
            );
            assert!(carved.len() < cut.len(), "at {pct}% in {mode} mode");
            assert_eq!(
                check(&file, "repair").unwrap()["ok"],
                true,
                "at {pct}% in {mode} mode"
            );
            assert_pillow_top_rows(&original, &carved, "jpeg", expected_rows, 2);
            let validity = file["validity"].as_str().unwrap().to_string();
            if let Some(previous) = &decoded_validity {
                assert_eq!(validity, *previous, "modes disagree at {pct}%");
            } else {
                decoded_validity = Some(validity);
            }
        }
    }
}

#[test]
fn jpeg_and_png_footer_repairs_are_revalidated() {
    let jpeg = &JPEG_1X1[..JPEG_1X1.len() - 2];
    let (jpeg_file, jpeg_carved) = scan_repaired(jpeg, "jpeg");
    assert_eq!(jpeg_file["status"], "repaired");
    assert_eq!(jpeg_file["validation"], "decoded");
    assert_eq!(jpeg_file["validity"], "repaired");
    assert_eq!(jpeg_file["sha256_original"], sha256(jpeg));
    assert_eq!(jpeg_carved, JPEG_1X1);

    let png_full = png_1x1();
    let png = &png_full[..png_full.len() - 12];
    let (png_file, png_carved) = scan_repaired(png, "png");
    assert_eq!(png_file["status"], "repaired");
    assert_eq!(png_file["validation"], "decoded");
    assert_eq!(png_file["validity"], "repaired");
    assert_eq!(png_file["sha256_original"], sha256(png));
    assert_eq!(png_carved, png_full);
}
