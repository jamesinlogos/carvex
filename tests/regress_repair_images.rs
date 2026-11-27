//! H-04 item 4: corpus-backed acceptance for validated PNG/JPEG crop repair.
//!
//! Every output labelled `repaired` must be a shorter file whose retained rows
//! come from the carved prefix. An independent Pillow oracle decodes the output
//! and compares those rows with the original; a grey-filled or re-rendered file
//! cannot pass.

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

fn corpus(relative: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("bench/corpus")
        .join(relative);
    fs::read(&path).unwrap_or_else(|e| panic!("missing corpus fixture {}: {e}", path.display()))
}

fn sha256(data: &[u8]) -> String {
    carvex::util::sha256_hex(data)
}

/// Scan one payload through the CLI with `--repair` and return the first file
/// row and the bytes actually written.
fn scan_repaired(payload: &[u8], kind: &str, mode: &str) -> (Value, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let mut image = vec![0u8; 4096];
    image.extend_from_slice(payload);
    image.extend_from_slice(&[0u8; 4096]);
    let image_path = dir.path().join("image.img");
    fs::write(&image_path, &image).unwrap();
    let out = dir.path().join("out");
    let report_path = dir.path().join("report.json");
    let proc = Command::new(env!("CARGO_BIN_EXE_carvex"))
        .args([
            "scan",
            image_path.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--mode",
            "raw",
            "--no-fs",
            "--types",
            kind,
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
        ])
        .output()
        .unwrap();
    assert_eq!(
        proc.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&proc.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    let files = report["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "report: {report:#}");
    let file = files[0].clone();
    let carved = fs::read(out.join(file["path"].as_str().unwrap())).unwrap();
    (file, carved)
}

fn check(file: &Value, name: &str) -> Option<Value> {
    file["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == name)
        .cloned()
}

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
if rows == 0:
    sys.exit('repair reported no rows')
if image.height >= reference.height:
    sys.exit('repair kept the full canvas')
keep = min(image.height - slack, rows if kind == 'png' else image.height)
if keep <= 0:
    sys.exit('no retained rows')
if image.crop((0, 0, image.width, keep)).tobytes() != reference.crop((0, 0, reference.width, keep)).tobytes():
    sys.exit('retained rows differ from the original')
print('OK')
"#;
    let proc = Command::new("python3")
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

fn assert_validated_crop(file: &Value, carved: &[u8], input: &[u8], kind: &str) -> u32 {
    assert_eq!(file["status"], "repaired");
    assert_eq!(file["validity"], "repaired");
    assert_eq!(file["repaired"], true);
    assert_eq!(file["repair_validated"], true);
    assert_eq!(file["validation"], "decoded");
    assert_eq!(file["sha256"], sha256(carved));
    let source_len = file["carved_size"].as_u64().expect("carved_size") as usize;
    assert!(source_len <= input.len(), "carved prefix exceeds the input");
    assert_eq!(file["sha256_original"], sha256(&input[..source_len]));
    let rows = file["recovered_rows"].as_u64().expect("recovered_rows") as u32;
    let total = file["total_rows"].as_u64().expect("total_rows") as u32;
    assert!(rows > 0 && rows < total, "bad row coverage {rows}/{total}");
    let expected_fraction = f64::from(rows) / f64::from(total);
    assert!(
        (file["recovered_fraction"].as_f64().unwrap() - expected_fraction).abs() < 1e-9,
        "recovered_fraction disagrees with {rows}/{total}"
    );
    assert_eq!(check(file, "repair").unwrap()["ok"], true);
    assert!(file["notes"].as_array().unwrap().iter().any(|note| note
        .as_str()
        .is_some_and(|n| n.contains("repair cropped to"))));
    let _ = kind;
    rows
}

fn assert_unrepairable(file: &Value, carved: &[u8], cut: &[u8]) {
    assert_eq!(carved, cut, "a failed repair must not alter the input");
    assert_eq!(file["status"], "truncated");
    assert_eq!(file["validity"], "partial");
    assert_eq!(file["repaired"], false);
    assert_eq!(file["repair_validated"], false);
    assert!(file["sha256_original"].is_null());
    assert!(file["recovered_rows"].is_null());
    assert!(file["total_rows"].is_null());
    assert!(file["recovered_fraction"].is_null());
    assert_eq!(check(file, "repair").unwrap()["ok"], false);
    assert!(file["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str() == Some("repair attempted, not validated")));
}

fn check_corpus_file(kind: &str, path: &str, check_both_modes: bool) {
    let original = corpus(path);
    for pct in [30usize, 60, 90] {
        let cut = &original[..original.len() * pct / 100];
        let (decode, decode_carved) = scan_repaired(cut, kind, "decode");
        let rows = assert_validated_crop(&decode, &decode_carved, cut, kind);
        assert_pillow_top_rows(
            &original,
            &decode_carved,
            kind,
            rows,
            u32::from(kind == "jpeg") * 2,
        );
        if check_both_modes {
            let (structure, structure_carved) = scan_repaired(cut, kind, "structure");
            assert_eq!(structure_carved, decode_carved, "{path} at {pct}%");
            assert_eq!(
                structure["validity"], decode["validity"],
                "{path} at {pct}%"
            );
        }
    }
}

#[test]
fn png_rgba_cuts_are_validated_crops() {
    check_corpus_file("png", "png/png_01_rgba_640x480.png", false);
}

#[test]
fn png_rgb_cuts_agree_in_both_modes() {
    check_corpus_file("png", "png/png_02_rgb_800x600.png", true);
}

#[test]
fn png_palette_cuts_are_validated_crops() {
    check_corpus_file("png", "png/png_04_palette_600x400.png", false);
}

#[test]
fn jpeg_baseline_cuts_agree_in_both_modes() {
    check_corpus_file("jpeg", "jpeg/jpeg_01_baseline_640x480.jpg", true);
}

#[test]
fn jpeg_large_cuts_are_validated_crops() {
    check_corpus_file("jpeg", "jpeg/jpeg_02_large_1280x960.jpg", false);
}

#[test]
fn jpeg_small_cuts_are_validated_crops() {
    check_corpus_file("jpeg", "jpeg/jpeg_09_small_320x240.jpg", false);
}

#[test]
fn unsupported_formats_stay_partial() {
    let cases = [
        ("png", "png/png_08_interlaced_700x500.png"),
        ("jpeg", "jpeg/jpeg_03_progressive_800x600.jpg"),
        ("jpeg", "jpeg/jpeg_10_progressive_1200x800.jpg"),
    ];
    for (kind, path) in cases {
        let original = corpus(path);
        let cut = &original[..original.len() * 3 / 5];
        for mode in ["structure", "decode"] {
            let (file, carved) = scan_repaired(cut, kind, mode);
            assert_unrepairable(&file, &carved, cut);
        }
    }
}

fn first_idat_end(png: &[u8]) -> usize {
    let mut pos = 8usize;
    while pos + 8 <= png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
        if &png[pos + 4..pos + 8] == b"IDAT" {
            return pos + 12 + len;
        }
        pos += 12 + len;
    }
    panic!("fixture has no complete IDAT chunk");
}

#[test]
fn png_cut_inside_next_chunk_header_keeps_first_idat_rows() {
    let original = corpus("png/png_02_rgb_800x600.png");
    let boundary = first_idat_end(&original);
    for into_header in [1usize, 3, 7] {
        let cut = &original[..boundary + into_header];
        for mode in ["structure", "decode"] {
            let (file, carved) = scan_repaired(cut, "png", mode);
            let rows = assert_validated_crop(&file, &carved, cut, "png");
            assert_eq!(rows, 27, "at {into_header} byte(s) into the header");
            assert_eq!(file["total_rows"], 600);
            assert_pillow_top_rows(&original, &carved, "png", rows, 0);
        }
    }
}

#[test]
fn padding_after_the_cut_does_not_invent_rows() {
    for (kind, path, expected_rows) in [
        ("png", "png/png_02_rgb_800x600.png", 359u32),
        ("jpeg", "jpeg/jpeg_01_baseline_640x480.jpg", 17u32),
    ] {
        let original = corpus(path);
        let cut = &original[..original.len() * 3 / 5];
        let (plain_file, plain_carved) = scan_repaired(cut, kind, "decode");
        assert_eq!(
            assert_validated_crop(&plain_file, &plain_carved, cut, kind),
            expected_rows
        );

        let mut padded = cut.to_vec();
        padded.extend(std::iter::repeat_n(0u8, 4096));
        let (padded_file, padded_carved) = scan_repaired(&padded, kind, "decode");
        assert_eq!(
            padded_carved, plain_carved,
            "{path}: padding changed repair"
        );
        assert_eq!(
            assert_validated_crop(&padded_file, &padded_carved, &padded, kind),
            expected_rows
        );
        assert!(padded_file["tail_trimmed_bytes"].as_u64().unwrap() >= 1);
        assert_pillow_top_rows(
            &original,
            &padded_carved,
            kind,
            expected_rows,
            u32::from(kind == "jpeg") * 2,
        );
    }
}

#[test]
fn intact_images_are_not_repaired() {
    for (kind, path) in [
        ("png", "png/png_02_rgb_800x600.png"),
        ("jpeg", "jpeg/jpeg_01_baseline_640x480.jpg"),
    ] {
        let original = corpus(path);
        let (file, carved) = scan_repaired(&original, kind, "decode");
        assert_eq!(carved, original, "{path} changed byte-for-byte");
        assert_eq!(file["repaired"], false);
        assert_eq!(file["repair_validated"], false);
        assert!(file["sha256_original"].is_null());
    }
}
