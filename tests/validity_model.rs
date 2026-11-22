//! H-16: authoritative per-file validity and honest default-mode labels.
//!
//! Red first: the current report has no `validity`/`checks[]`, and a default
//! `structure` row is presented as merely `complete`, with no statement that
//! payload content was not checked.

mod common;

use common::JPEG_1X1;
use std::fs;
use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_carvex")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin()).args(args).output().unwrap()
}

/// A prefix from the baseline JPEG followed by a structurally complete,
/// byte-different JPEG payload. Default mode never decodes it.
fn spliced_jpeg() -> Vec<u8> {
    let mut payload = vec![0xFF, 0xD8, 0xFF, 0xFE];
    let comment = vec![b'S'; 512];
    payload.extend_from_slice(&((comment.len() + 2) as u16).to_be_bytes());
    payload.extend_from_slice(&comment);
    payload.extend_from_slice(&JPEG_1X1[2..]);

    let mut spliced = JPEG_1X1[..2].to_vec();
    spliced.extend_from_slice(&payload[2..]);
    assert_ne!(spliced, JPEG_1X1);
    spliced
}

fn write_image(path: &Path) {
    let mut image = vec![0u8; 4096];
    image.extend_from_slice(&spliced_jpeg());
    image.extend_from_slice(&[0u8; 4096]);
    fs::write(path, image).unwrap();
}

#[test]
fn default_structure_rows_are_labeled_content_not_checked() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("spliced.img");
    write_image(&image);
    let out_dir = dir.path().join("out");
    let report_path = dir.path().join("report.json");
    let csv_path = dir.path().join("report.csv");

    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--mode",
        "raw",
        "--no-fs",
        "--types",
        "jpeg",
        "--json-report",
        report_path.to_str().unwrap(),
        "--csv-report",
        csv_path.to_str().unwrap(),
        "--manifest",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");

    // Scores and emission are deliberately unchanged by H-16.
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    let files = report["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "report: {report:#}");
    let file = &files[0];
    assert_eq!(file["status"], "complete");
    assert_eq!(file["validation"], "structure");
    assert_eq!(file["score"], 100);
    assert_eq!(file["validated_bytes"], 0);

    // New authoritative labels.
    assert_eq!(file["validity"], "structural");
    let checks = file["checks"].as_array().expect("checks[] is missing");
    assert!(checks.iter().any(|check| {
        check["name"] == "content_check"
            && check["ok"].is_null()
            && check["note"]
                .as_str()
                .is_some_and(|note| note.contains("content not checked"))
    }));

    let csv = fs::read_to_string(&csv_path).unwrap();
    let header = csv.lines().next().unwrap();
    assert!(
        header.ends_with(",validity,sha256_original,alternatives"),
        "CSV must append validity, sha256_original and alternatives: {header}"
    );
    assert_eq!(csv.lines().count(), 2);
    assert!(csv.lines().nth(1).unwrap().ends_with(",structural,,"));

    let manifest = fs::read_to_string(out_dir.join("SHA256SUMS")).unwrap();
    assert!(
        manifest.contains("validity=structural"),
        "manifest must show validity: {manifest}"
    );
    assert!(
        stderr.contains("content not checked"),
        "console must state that content was not checked: {stderr}"
    );
    assert!(
        !stderr.contains("COMPLETE"),
        "summary must not call structural output complete: {stderr}"
    );
}
