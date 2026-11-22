//! H-19 stage 2: `decode` validation must check the PDF cross-reference table,
//! trailer and catalog instead of only checking the selected footer lexically.
//!
//! Red first: all three fixtures currently report `unverified`/`structural`,
//! including the valid minimal PDF and the two broken files.

mod common;

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn scan(name: &str, dir: &Path) -> (Value, Vec<u8>) {
    scan_with_args(name, dir, &[])
}

fn scan_with_args(name: &str, dir: &Path, extra: &[&str]) -> (Value, Vec<u8>) {
    let source = fs::read(fixture(name)).unwrap();
    let image = dir.join(format!("{name}.img"));
    let mut bytes = vec![0u8; 4096];
    bytes.extend_from_slice(&source);
    bytes.extend_from_slice(&[0u8; 4096]);
    fs::write(&image, bytes).unwrap();
    let out = dir.join(format!("out-{name}"));
    let report_path = dir.join(format!("report-{name}.json"));
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
            "pdf",
            "--validate",
            "decode",
            "--min-score",
            "0",
            "--min-size",
            "1",
            "--quiet",
            "--json-report",
            report_path.to_str().unwrap(),
        ])
        .args(extra)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&proc.stderr).into_owned();
    assert_eq!(proc.status.code(), Some(0), "stderr: {stderr}");
    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    let files = report["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "report: {report:#}");
    let path = out.join(files[0]["path"].as_str().unwrap());
    let carved = fs::read(&path).unwrap();
    (files[0].clone(), carved)
}

fn check(file: &Value, name: &str) -> Value {
    file["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == name)
        .unwrap_or_else(|| panic!("missing {name} check in {file:#}"))
        .clone()
}

#[test]
fn valid_minimal_pdf_is_verified() {
    let dir = tempfile::tempdir().unwrap();
    let (file, carved) = scan("pdf_minimal.pdf", dir.path());
    let source = fs::read(fixture("pdf_minimal.pdf")).unwrap();
    assert_eq!(carved, source, "output bytes must be unchanged");
    assert_eq!(file["validation"], "decoded");
    assert_eq!(file["validity"], "verified");
    assert_eq!(check(&file, "xref_entries")["ok"], true);
    assert_eq!(check(&file, "root_catalog")["ok"], true);
    assert_eq!(check(&file, "xref_size")["ok"], true);
}

#[test]
fn bad_xref_offset_is_suspect_and_still_written() {
    let dir = tempfile::tempdir().unwrap();
    let (file, carved) = scan("pdf_bad_xref.pdf", dir.path());
    let source = fs::read(fixture("pdf_bad_xref.pdf")).unwrap();
    assert_eq!(
        carved, source,
        "suspect PDF must still be written unchanged"
    );
    assert_eq!(file["status"], "complete");
    assert_eq!(file["validation"], "partial");
    assert_eq!(file["validity"], "suspect");
    assert_eq!(check(&file, "xref_entries")["ok"], false);
}

#[test]
fn inconsistent_xref_stream_length_is_suspect() {
    let dir = tempfile::tempdir().unwrap();
    let (file, carved) = scan("pdf_bad_stream_length.pdf", dir.path());
    let source = fs::read(fixture("pdf_bad_stream_length.pdf")).unwrap();
    assert_eq!(carved, source);
    assert_eq!(file["validation"], "partial");
    assert_eq!(file["validity"], "suspect");
    assert_eq!(check(&file, "stream_length")["ok"], false);
}

#[test]
fn prev_cycle_is_suspect() {
    let dir = tempfile::tempdir().unwrap();
    let (file, _) = scan("pdf_prev_cycle.pdf", dir.path());
    assert_eq!(file["validation"], "partial");
    assert_eq!(file["validity"], "suspect");
    assert_eq!(check(&file, "prev_chain")["ok"], false);
}

#[test]
fn root_pointing_at_a_missing_object_is_suspect() {
    let dir = tempfile::tempdir().unwrap();
    let (file, _) = scan("pdf_bad_root.pdf", dir.path());
    assert_eq!(file["validation"], "partial");
    assert_eq!(file["validity"], "suspect");
    assert_eq!(check(&file, "root_catalog")["ok"], false);
}

#[test]
fn compressed_root_catalog_is_verified() {
    let dir = tempfile::tempdir().unwrap();
    let (file, carved) = scan("pdf_object_stream.pdf", dir.path());
    let source = fs::read(fixture("pdf_object_stream.pdf")).unwrap();
    assert_eq!(carved, source, "output bytes must be unchanged");
    assert_eq!(file["validation"], "decoded");
    assert_eq!(file["validity"], "verified");
    assert_eq!(check(&file, "root_catalog")["ok"], true);
    assert!(check(&file, "xref_entries")["note"]
        .as_str()
        .is_some_and(|note| note.contains("object streams")));
}

#[test]
fn tiny_validator_budget_is_unverified_not_suspect() {
    let dir = tempfile::tempdir().unwrap();
    let (file, _) = scan_with_args(
        "pdf_xref_stream.pdf",
        dir.path(),
        &["--max-validator-memory", "1"],
    );
    assert_eq!(file["validation"], "unverified");
    assert_eq!(file["validity"], "structural");
    assert_eq!(check(&file, "xref_stream")["ok"], serde_json::Value::Null);
}

#[test]
fn bounded_xref_stream_is_verified() {
    let dir = tempfile::tempdir().unwrap();
    let (file, carved) = scan("pdf_xref_stream.pdf", dir.path());
    let source = fs::read(fixture("pdf_xref_stream.pdf")).unwrap();
    assert_eq!(carved, source, "output bytes must be unchanged");
    assert_eq!(file["validation"], "decoded");
    assert_eq!(file["validity"], "verified");
    assert_eq!(check(&file, "xref_stream")["ok"], true);
    assert_eq!(check(&file, "stream_length")["ok"], true);
    assert_eq!(check(&file, "root_catalog")["ok"], true);
}
