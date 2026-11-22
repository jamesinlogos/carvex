//! H-05: a single `--max-validator-memory` flag bounds decode validators.
//!
//! Red first: the CLI has no such flag yet, so the help assertion fails before
//! the implementation lands.

mod common;

use std::fs;
use std::process::Command;

use common::chunk;

fn png_bomb() -> Vec<u8> {
    let mut out = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&16u32.to_be_bytes());
    ihdr.extend_from_slice(&16u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));

    // 16x16 grey needs 272 raw bytes, but the stream inflates to 1 MiB.
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    std::io::Write::write_all(&mut encoder, &vec![0u8; 1024 * 1024]).unwrap();
    out.extend_from_slice(&chunk(b"IDAT", &encoder.finish().unwrap()));
    out.extend_from_slice(&chunk(b"IEND", b""));
    out
}

#[test]
fn max_validator_memory_is_a_cli_flag() {
    let out = Command::new(env!("CARGO_BIN_EXE_carvex"))
        .args(["scan", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(
        help.contains("--max-validator-memory"),
        "missing H-05 validator budget flag: {help}"
    );
}

#[test]
fn png_bomb_is_not_verified_with_a_small_validator_budget() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("bomb.img");
    let mut payload = vec![0u8; 4096];
    payload.extend_from_slice(&png_bomb());
    payload.extend_from_slice(&[0u8; 8192]);
    fs::write(&image, payload).unwrap();
    let out_dir = dir.path().join("out");
    fs::create_dir(&out_dir).unwrap();
    let report = dir.path().join("report.json");

    let status = Command::new(env!("CARGO_BIN_EXE_carvex"))
        .args([
            "scan",
            image.to_str().unwrap(),
            "-o",
            out_dir.to_str().unwrap(),
            "--mode",
            "raw",
            "--no-fs",
            "--types",
            "png",
            "--min-size",
            "1",
            "--validate",
            "decode",
            "--max-validator-memory",
            "1K",
            "--quiet",
            "--json-report",
            report.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success() || status.code() == Some(3));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    for file in value["files"].as_array().unwrap() {
        assert_ne!(file["validation"], "decoded", "bomb was decoded: {file}");
    }
}
