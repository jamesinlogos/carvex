//! H-17: a PNG chunk CRC mismatch is a suspect integrity failure in both
//! validation modes. The file is still written with its historical score.
//!
//! Red first: structure mode currently reports `complete/structure/85` with
//! `validity=structural` and no chunk-level check, and decode mode reports
//! `validated_bytes=0` instead of stopping at the last good chunk.

mod common;

use common::{chunk, png_1x1};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_carvex")
}

fn bad_crc_png() -> (Vec<u8>, u64) {
    let mut png = png_1x1();
    let idat = png
        .windows(4)
        .position(|w| w == b"IDAT")
        .expect("fixture has IDAT");
    let chunk_start = idat - 4;
    let crc_offset =
        idat + 4 + u32::from_be_bytes(png[chunk_start..idat].try_into().unwrap()) as usize;
    png[crc_offset] ^= 0x01;
    (png, chunk_start as u64)
}

fn png_three_bad_crc() -> Vec<u8> {
    let source = png_1x1();
    let idat = source
        .windows(4)
        .position(|w| w == b"IDAT")
        .expect("fixture has IDAT");
    let chunk_start = idat - 4;
    let len = u32::from_be_bytes(source[chunk_start..idat].try_into().unwrap()) as usize;
    let data = &source[idat + 4..idat + 4 + len];
    let (a, b) = (data.len() / 3, 2 * data.len() / 3);
    let mut out = source[..chunk_start].to_vec();
    for part in [&data[..a], &data[a..b], &data[b..]] {
        let mut c = chunk(b"IDAT", part);
        let crc = c.len() - 1;
        c[crc] ^= 0x01;
        out.extend_from_slice(&c);
    }
    out.extend_from_slice(&chunk(b"IEND", b""));
    out
}

fn scan(mode: &str, payload: &[u8], dir: &Path) -> (Value, String) {
    scan_with_min_score(mode, payload, dir, Some("0"))
}

fn scan_with_min_score(
    mode: &str,
    payload: &[u8],
    dir: &Path,
    min_score: Option<&str>,
) -> (Value, String) {
    let image = dir.join(format!("{mode}.img"));
    let mut bytes = vec![0u8; 4096];
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&[0u8; 4096]);
    fs::write(&image, bytes).unwrap();
    let out = dir.join(format!("out-{mode}"));
    let report_path = dir.join(format!("report-{mode}.json"));
    let mut args = vec![
        "scan",
        image.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--mode",
        "raw",
        "--no-fs",
        "--types",
        "png",
        "--validate",
        mode,
    ];
    if let Some(score) = min_score {
        args.extend(["--min-score", score]);
    }
    args.extend([
        "--min-size",
        "1",
        "--quiet",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    let proc = Command::new(bin()).args(args).output().unwrap();
    let stderr = String::from_utf8_lossy(&proc.stderr).into_owned();
    assert_eq!(proc.status.code(), Some(0), "stderr: {stderr}");
    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    (report, stderr)
}

fn assert_suspect(report: &Value, expected_bytes: u64, expected_score: u8) {
    let files = report["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "report: {report:#}");
    let file = &files[0];
    assert_eq!(file["status"], "complete");
    assert_eq!(file["score"], expected_score);
    assert_eq!(file["validation"], "partial");
    assert_eq!(file["validity"], "suspect");
    assert_eq!(file["validated_bytes"], expected_bytes);
    let checks = file["checks"].as_array().unwrap();
    assert!(
        checks.iter().any(|check| {
            check["name"] == "chunk_crc"
                && check["ok"] == false
                && check["note"]
                    .as_str()
                    .is_some_and(|note| note.contains("IDAT") && note.contains("byte"))
        }),
        "missing chunk_crc check: {checks:#?}"
    );
}

#[test]
fn bad_png_crc_is_suspect_in_structure_and_decode_modes() {
    let (png, expected_prefix) = bad_crc_png();
    let dir = tempfile::tempdir().unwrap();

    // Structure mode already computes CRCs while parsing; it reports the
    // integrity failure but does not claim decoded content bytes.
    let (structure, _) = scan("structure", &png, dir.path());
    assert_suspect(&structure, 0, 85);

    // Decode mode recomputes every CRC and stops the checked prefix at the
    // last good chunk. Its historical 20-point decode-penalty is preserved.
    let (decode, _) = scan("decode", &png, dir.path());
    assert_suspect(&decode, expected_prefix, 65);
}

#[test]
fn png_three_bad_crc_decode_is_not_silent() {
    let png = png_three_bad_crc();
    let dir = tempfile::tempdir().unwrap();

    // Default --min-score (50): three bad CRCs drop the decode score to 35.
    // The file may still be written as suspect (§9 Q3), but a silent drop is
    // not allowed: BELOW_MIN_SCORE must report the count and first offsets.
    let (report, stderr) = scan_with_min_score("decode", &png, dir.path(), None);
    let files = report["files"].as_array().unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    let low_score = warnings
        .iter()
        .find(|w| w["code"] == "BELOW_MIN_SCORE")
        .unwrap_or_else(|| {
            panic!("silent low-score drop: files={files:#?} warnings={warnings:#?} stderr={stderr}")
        });
    let message = low_score["message"].as_str().unwrap();
    assert!(message.contains("not written"), "message: {message}");
    assert!(message.contains("first offsets"), "message: {message}");
    assert!(
        report["stats"]["below_min_score"].as_u64().unwrap() >= 1,
        "stats: {}",
        report["stats"]
    );
}

#[test]
fn clean_png_keeps_its_historical_labels() {
    let png = png_1x1();
    let dir = tempfile::tempdir().unwrap();
    let (structure, _) = scan("structure", &png, dir.path());
    let (decode, _) = scan("decode", &png, dir.path());
    for (report, expected_validation, expected_validity, score, bytes) in [
        (&structure, "structure", "structural", 100, 0),
        (&decode, "decoded", "verified", 100, png.len() as u64),
    ] {
        let file = &report["files"][0];
        assert_eq!(file["validation"], expected_validation);
        assert_eq!(file["validity"], expected_validity);
        assert_eq!(file["score"], score);
        assert_eq!(file["validated_bytes"], bytes);
    }
}
