//! H-27: `regions[]` must be a faithful, additive coverage map built from the
//! same extents the scanner reads. Fixtures are generated in memory.

mod common;

use common::{fat32_volume_sectors, mbr_image};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_carvex")
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

fn image_with_gap(dir: &Path) -> (std::path::PathBuf, Vec<u8>) {
    let image = dir.join("fat32-with-gap.img");
    let data = mbr_image(&[(2048, 0x0C, &fat32_volume_sectors(2048))]);
    fs::write(&image, &data).unwrap();
    (image, data)
}

fn assert_coverage(report: &Value, image_len: u64, scanned_bytes: u64) -> Vec<Value> {
    let regions = report["regions"].as_array().unwrap().clone();
    assert!(!regions.is_empty(), "no regions: {report}");
    let mut end = 0u64;
    for (i, region) in regions.iter().enumerate() {
        let offset = region["offset"].as_u64().unwrap();
        let len = region["len"].as_u64().unwrap();
        assert_eq!(offset, end, "region {i} starts at {offset}, expected {end}");
        end = offset + len;
        assert!(region["extent_id"].is_number() || region["extent_id"].is_null());
        assert!(region["summarized"].is_boolean());
    }
    assert_eq!(end, image_len, "regions cover {end} of {image_len} bytes");
    let scanned: u64 = regions
        .iter()
        .filter(|r| r["scanned"].as_bool().unwrap())
        .map(|r| r["len"].as_u64().unwrap())
        .sum();
    assert_eq!(scanned, scanned_bytes, "scanned region bytes do not match");
    regions
}

#[test]
fn auto_regions_are_real_extents_and_cover_the_image() {
    let dir = tempfile::tempdir().unwrap();
    let (image, data) = image_with_gap(dir.path());
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
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report = report_from(&report_path);
    let bytes_scanned = report["bytes_scanned"].as_u64().unwrap();
    assert_eq!(
        bytes_scanned,
        report["stats"]["bytes_in_extents"].as_u64().unwrap()
    );
    let regions = assert_coverage(&report, data.len() as u64, bytes_scanned);

    let fs = &report["filesystem"];
    let free: Vec<(u64, u64)> = fs["free_extents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["offset"].as_u64().unwrap(), e["len"].as_u64().unwrap()))
        .collect();
    let reported_free: Vec<(u64, u64)> = regions
        .iter()
        .filter(|r| r["kind"] == "free")
        .map(|r| (r["offset"].as_u64().unwrap(), r["len"].as_u64().unwrap()))
        .collect();
    assert_eq!(reported_free, free, "free regions must be the real extents");

    let volume_len = fs["length"].as_u64().unwrap();
    let free_sum: u64 = free.iter().map(|(_, len)| *len).sum();
    let allocated_sum: u64 = regions
        .iter()
        .filter(|r| r["kind"] == "allocated")
        .map(|r| r["len"].as_u64().unwrap())
        .sum();
    assert_eq!(free_sum + allocated_sum, volume_len);
    assert!(regions
        .iter()
        .any(|r| r["kind"] == "gap" && r["scanned"].as_bool().unwrap()));
    assert!(regions
        .iter()
        .filter(|r| r["scanned"].as_bool().unwrap())
        .all(|r| r["extent_id"].is_number()));
}

#[test]
fn max_regions_summarises_without_changing_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let (image, data) = image_with_gap(dir.path());
    let report_path = dir.path().join("capped.json");
    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        dir.path().join("capped").to_str().unwrap(),
        "--mode",
        "raw",
        "--types",
        "png",
        "--min-size",
        "1",
        "--max-regions",
        "1",
        "--quiet",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report = report_from(&report_path);
    assert_eq!(report["options"]["max_regions"], 1);
    let bytes_scanned = report["bytes_scanned"].as_u64().unwrap();
    assert_eq!(bytes_scanned, data.len() as u64);
    let regions = assert_coverage(&report, data.len() as u64, bytes_scanned);
    assert_eq!(
        regions.len(),
        1,
        "cap must summarise to one region: {regions:?}"
    );
    assert_eq!(regions[0]["kind"], "mixed");
    assert_eq!(regions[0]["summarized"], true);
}

#[test]
fn info_regions_use_real_free_extents() {
    let dir = tempfile::tempdir().unwrap();
    let (image, _) = image_with_gap(dir.path());
    let out = run(&["info", "--json", image.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let fs = &doc["filesystem"];
    let free: Vec<(u64, u64)> = fs["free_extents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["offset"].as_u64().unwrap(), e["len"].as_u64().unwrap()))
        .collect();
    let reported_free: Vec<(u64, u64)> = doc["regions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "free")
        .map(|r| (r["offset"].as_u64().unwrap(), r["len"].as_u64().unwrap()))
        .collect();
    assert_eq!(reported_free, free);
    let mut end = 0u64;
    for region in doc["regions"].as_array().unwrap() {
        let offset = region["offset"].as_u64().unwrap();
        assert_eq!(offset, end, "info regions must be sorted and contiguous");
        end = offset + region["len"].as_u64().unwrap();
    }
    assert_eq!(end, doc["image_size"].as_u64().unwrap());
}
