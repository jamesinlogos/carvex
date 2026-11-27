//! H-27: the opt-in free-space stream skips allocated islands and reports
//! physical provenance. The default scan path is unchanged.

mod common;

use common::fat32_volume_sectors_spc;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

const CLUSTER: usize = 4096;

fn corpus(relative: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("bench/corpus")
        .join(relative);
    fs::read(&path).unwrap_or_else(|e| panic!("missing corpus fixture {}: {e}", path.display()))
}

fn sha256(data: &[u8]) -> String {
    carvex::util::sha256_hex(data)
}

/// FAT32 volume with `payload` split around a four-cluster island at cluster 4.
/// When `allocated_island` is true the island FAT entries are marked allocated,
/// so the free-space stream must skip them; otherwise it is free data from a
/// second deleted file and the stream must not produce a false verified file.
fn image_with_island(payload: &[u8], island: &[u8], allocated_island: bool) -> Vec<u8> {
    let mut image = fat32_volume_sectors_spc(2048, 8);
    let data_start = 19 * 512;
    let first = data_start + CLUSTER;
    let island_offset = data_start + 2 * CLUSTER;
    let second = data_start + 6 * CLUSTER;
    assert!(payload.len() > CLUSTER);
    image[first..first + CLUSTER].copy_from_slice(&payload[..CLUSTER]);
    image[island_offset..island_offset + 4 * CLUSTER].copy_from_slice(&island[..4 * CLUSTER]);
    image[second..second + payload.len() - CLUSTER].copy_from_slice(&payload[CLUSTER..]);
    if allocated_island {
        let fat = 512;
        for cluster in 4..8u32 {
            let entry = fat + cluster as usize * 4;
            image[entry..entry + 4].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
        }
    }
    image
}

struct Scan {
    report: Value,
    files: Vec<Vec<u8>>,
}

fn scan(image: &[u8], dir: &Path, free_stream: bool) -> Scan {
    let img_path = dir.join("image.img");
    let out = dir.join("out");
    let json = dir.join("report.json");
    fs::write(&img_path, image).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_carvex"));
    cmd.args([
        "scan",
        img_path.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--quiet",
        "--json-report",
        json.to_str().unwrap(),
        "--mode",
        "unallocated",
        "--types",
        "jpeg",
        "--validate",
        "decode",
        "--min-size",
        "1",
    ]);
    if free_stream {
        cmd.arg("--free-stream");
    }
    let status = cmd.status().unwrap();
    assert!(status.success(), "scan failed: {status}");
    let report: Value = serde_json::from_slice(&fs::read(&json).unwrap()).unwrap();
    let mut files = Vec::new();
    for entry in fs::read_dir(&out).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            files.push(fs::read(path).unwrap());
        }
    }
    Scan { report, files }
}

fn free_stream_files(report: &Value) -> Vec<&Value> {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|file| file["source"] == "free-stream")
        .collect()
}

#[test]
fn live_island_is_skipped_and_exact_file_has_physical_fragments() {
    let dir = tempfile::tempdir().unwrap();
    let original = corpus("jpeg/jpeg_01_baseline_640x480.jpg");
    let island = corpus("jpeg/jpeg_04_grayscale_512x512.jpg");
    let image = image_with_island(&original, &island, true);
    let scan = scan(&image, dir.path(), true);

    assert_eq!(scan.report["options"]["free_stream"], true);
    let recovered = free_stream_files(&scan.report);
    assert_eq!(recovered.len(), 1, "report: {}", scan.report);
    let file = recovered[0];
    assert_eq!(file["sha256"], sha256(&original));
    assert_eq!(file["validity"], "verified");
    assert!(scan
        .files
        .iter()
        .any(|bytes| sha256(bytes) == sha256(&original)));

    let free_extents = scan.report["filesystem"]["free_extents"]
        .as_array()
        .expect("filesystem free extents are reported");
    for fragment in file["fragments"].as_array().unwrap() {
        let offset = fragment["offset"].as_u64().unwrap();
        let len = fragment["len"].as_u64().unwrap();
        assert!(free_extents.iter().any(|extent| {
            let start = extent["offset"].as_u64().unwrap();
            let extent_len = extent["len"].as_u64().unwrap();
            offset >= start && offset + len <= start + extent_len
        }));
    }
}

#[test]
fn dead_island_does_not_produce_a_false_verified_file() {
    let dir = tempfile::tempdir().unwrap();
    let original = corpus("jpeg/jpeg_01_baseline_640x480.jpg");
    let island = corpus("jpeg/jpeg_02_large_1280x960.jpg");
    let image = image_with_island(&original, &island, false);
    let scan = scan(&image, dir.path(), true);
    let expected = sha256(&original);
    assert!(
        !scan.files.iter().any(|bytes| sha256(bytes) == expected),
        "free stream invented a verified file from a dead island"
    );
}

#[test]
fn default_scan_unchanged_without_the_flag() {
    let dir = tempfile::tempdir().unwrap();
    let original = corpus("jpeg/jpeg_01_baseline_640x480.jpg");
    let island = corpus("jpeg/jpeg_04_grayscale_512x512.jpg");
    let image = image_with_island(&original, &island, true);
    let scan = scan(&image, dir.path(), false);
    assert!(scan.report["options"].get("free_stream").is_none());
    assert!(free_stream_files(&scan.report).is_empty());
}
