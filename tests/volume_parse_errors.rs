//! H-26: an unparsable volume beside a good one must not be downgraded to a
//! silent raw scan. Fixtures are generated in memory; no filesystem tools are
//! required.

mod common;

use common::{fat32_volume_sectors, mbr_image};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_carvex")
}

/// A bootable-looking NTFS partition whose `$MFT` record cannot be parsed.
fn broken_ntfs_volume(total_sectors: u32) -> Vec<u8> {
    let mut volume = vec![0u8; total_sectors as usize * 512];
    let boot = &mut volume[..512];
    boot[3..7].copy_from_slice(b"NTFS");
    boot[0x0b..0x0d].copy_from_slice(&512u16.to_le_bytes());
    boot[0x0d] = 1;
    boot[0x28..0x30].copy_from_slice(&(total_sectors as u64).to_le_bytes());
    boot[0x30..0x38].copy_from_slice(&0u64.to_le_bytes()); // $MFT at LCN 0
    boot[0x38..0x40].copy_from_slice(&1u64.to_le_bytes());
    boot[0x40] = 0xf6u8; // -10 clusters: 1024-byte MFT records
    boot[510..512].copy_from_slice(&[0x55, 0xAA]);
    volume
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn report_from(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn multi_volume_parse_error_is_warned_and_the_fallback_region_is_flagged() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("two-volumes.img");
    let fat = fat32_volume_sectors(2048);
    let ntfs = broken_ntfs_volume(2048);
    fs::write(
        &image,
        mbr_image(&[(2048, 0x0C, &fat), (4096, 0x07, &ntfs)]),
    )
    .unwrap();

    let report_path = dir.path().join("report.json");
    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        dir.path().join("out").to_str().unwrap(),
        "--types",
        "png",
        "--min-size",
        "1",
        "--quiet",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(3), "stderr: {}", stderr(&out));
    let report = report_from(&report_path);
    let warnings = report["warnings"].as_array().unwrap();
    let parse = warnings
        .iter()
        .find(|w| w["code"] == "VOLUME_PARSE_FAILED")
        .unwrap_or_else(|| panic!("missing VOLUME_PARSE_FAILED in {warnings:?}"));
    let message = parse["message"].as_str().unwrap();
    assert!(message.contains("partition 2"), "message: {message}");
    assert!(message.contains("byte 2097152"), "message: {message}");
    assert!(message.contains("$MFT"), "message: {message}");
    let console = stderr(&out);
    assert!(
        console.contains("warning[VOLUME_PARSE_FAILED]"),
        "stderr: {console}"
    );
    assert!(console.contains("$MFT"), "stderr: {console}");

    let region = report["regions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["partition_index"] == 2 && r["kind"] == "unsupported")
        .unwrap_or_else(|| panic!("missing fallback region in {}", report["regions"]));
    assert_eq!(region["scanned"], true, "region: {region}");
    assert_eq!(region["may_contain_allocated"], true, "region: {region}");
}

#[test]
fn single_volume_parse_error_replaces_the_misleading_generic_message() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("one-broken-volume.img");
    let ntfs = broken_ntfs_volume(2048);
    fs::write(&image, mbr_image(&[(2048, 0x07, &ntfs)])).unwrap();

    let report_path = dir.path().join("report.json");
    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        dir.path().join("out").to_str().unwrap(),
        "--types",
        "png",
        "--min-size",
        "1",
        "--quiet",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(3), "stderr: {}", stderr(&out));
    let report = report_from(&report_path);
    let warnings = report["warnings"].as_array().unwrap();
    let detection = warnings
        .iter()
        .find(|w| w["code"] == "FS_DETECTION_FAILED")
        .unwrap_or_else(|| panic!("missing FS_DETECTION_FAILED in {warnings:?}"));
    let message = detection["message"].as_str().unwrap();
    assert!(
        message.contains("NTFS boot sector at byte 1048576"),
        "message: {message}"
    );
    assert!(message.contains("$MFT"), "message: {message}");
    assert!(
        !message.contains("no FAT32/NTFS filesystem found"),
        "message still hides the real cause: {message}"
    );
}

#[test]
fn info_reports_the_specific_parse_cause_and_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("one-broken-volume.img");
    let ntfs = broken_ntfs_volume(2048);
    fs::write(&image, mbr_image(&[(2048, 0x07, &ntfs)])).unwrap();

    let out = run(&["info", image.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    let console = stderr(&out);
    assert!(
        console.contains("NTFS boot sector at byte 1048576"),
        "stderr: {console}"
    );
    assert!(console.contains("$MFT"), "stderr: {console}");
}
