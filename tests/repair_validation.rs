//! H-21: `--repair` must revalidate before claiming `repaired` and must expose
//! the hash of the bytes read from the source when it changes the output.
//!
//! Red first: the current scanner appends a static footer, labels every
//! truncated candidate `repaired`, and emits no `sha256_original`.

mod common;

use common::{pdf_min, png_1x1, JPEG_1X1};
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

/// Scan one payload with `--repair --validate decode` and return the first file
/// row plus the bytes that were written.
fn scan_repaired(payload: &[u8], name: &str) -> (Value, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join(format!("{name}.img"));
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
            "decode",
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
    assert_eq!(
        csv.lines().nth(1).unwrap().split(',').next_back().unwrap(),
        file["sha256_original"].as_str().unwrap_or_default(),
        "CSV sha256_original must match JSON"
    );
    (file, carved)
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
