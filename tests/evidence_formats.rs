//! B-04 evidence format coverage.
//!
//! Fixtures are generated in memory and written to a temporary directory.
//! Each container must expose exactly the same byte stream as the raw image;
//! the stdout contract is checked separately for stdin.

mod common;

use carvex::image::fuzz_support::{
    build_e01, build_vhd_dynamic, build_vhd_dynamic_with_hole, build_vhd_fixed, build_vmdk_flat,
    build_vmdk_sparse, build_vmdk_sparse_with_hole, pad_to_sector,
};
use carvex::io::{open_image, read_exact_at, ImageSource, InputFormat};
use common::mixed_raw_image;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_carvex")
}

fn read_all(source: &dyn ImageSource) -> std::io::Result<Vec<u8>> {
    let mut data = vec![0u8; source.len() as usize];
    read_exact_at(source, &mut data, 0)?;
    Ok(data)
}

fn open_and_compare(path: &Path, expected: InputFormat, data: &[u8]) {
    let opened = open_image(path, InputFormat::Auto).unwrap();
    assert_eq!(opened.format, expected, "format for {}", path.display());
    assert_eq!(
        opened.len,
        data.len() as u64,
        "length for {}",
        path.display()
    );
    let identity = opened
        .identity
        .as_ref()
        .expect("disk-backed source has an identity");
    let (size, mtime, readable) = identity.current();
    assert!(
        readable,
        "backing files are readable for {}",
        path.display()
    );
    assert_eq!(size, identity.initial_size());
    assert_eq!(mtime, identity.initial_mtime());
    let actual = read_all(opened.source.as_ref())
        .unwrap_or_else(|e| panic!("read failed for {}: {e}", path.display()));
    assert_eq!(actual, data, "bytes for {}", path.display());
}

#[test]
fn split_raw_segments_are_concatenated() {
    let dir = tempfile::tempdir().unwrap();
    let data = mixed_raw_image();
    let mid = data.len() / 2;
    let first = dir.path().join("disk.001");
    let second = dir.path().join("disk.002");
    fs::write(&first, &data[..mid]).unwrap();
    fs::write(&second, &data[mid..]).unwrap();
    open_and_compare(&first, InputFormat::Split, &data);

    // Naming any segment finds the whole set.
    open_and_compare(&second, InputFormat::Split, &data);

    // The logical size differs from each segment's size, but stable backing
    // files must not produce a false IMAGE_MODIFIED warning.
    let output = dir.path().join("split-out");
    let report_path = dir.path().join("split-report.json");
    let result = Command::new(bin())
        .args([
            "scan",
            first.to_str().unwrap(),
            "--input-format",
            "split",
            "--output",
            output.to_str().unwrap(),
            "--mode",
            "raw",
            "--no-fs",
            "--min-size",
            "1",
            "--quiet",
            "--json-report",
            report_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert_eq!(report["input"]["unchanged"], true);
    assert_eq!(report["input"]["size"].as_u64().unwrap(), data.len() as u64);
    assert!(!report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["code"] == "IMAGE_MODIFIED"));
}

#[test]
fn e01_uncompressed_and_compressed_chunks_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let data = pad_to_sector(&mixed_raw_image());
    for (name, compressed) in [("plain.E01", false), ("zlib.E01", true)] {
        let path: PathBuf = dir.path().join(name);
        fs::write(&path, build_e01(&data, compressed)).unwrap();
        open_and_compare(&path, InputFormat::E01, &data);
    }
}

#[test]
fn vhd_fixed_and_dynamic_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let data = mixed_raw_image();
    let fixed = dir.path().join("fixed.vhd");
    let dynamic = dir.path().join("dynamic.vhd");
    fs::write(&fixed, build_vhd_fixed(&data)).unwrap();
    fs::write(&dynamic, build_vhd_dynamic(&data)).unwrap();
    open_and_compare(&fixed, InputFormat::Vhd, &data);
    open_and_compare(&dynamic, InputFormat::Vhd, &data);

    let (sparse, sparse_data) = build_vhd_dynamic_with_hole();
    let sparse_path = dir.path().join("hole.vhd");
    fs::write(&sparse_path, sparse).unwrap();
    open_and_compare(&sparse_path, InputFormat::Vhd, &sparse_data);

    // A read that crosses an unallocated block must continue into the next
    // allocated block instead of zero-filling the whole caller buffer.
    let opened = open_image(&sparse_path, InputFormat::Vhd).unwrap();
    let mut buf = vec![0u8; 32];
    read_exact_at(
        opened.source.as_ref(),
        &mut buf,
        (2 * 64 * 1024 - 12) as u64,
    )
    .unwrap();
    assert!(buf[..12].iter().all(|b| *b == 0));
    assert!(buf[12..].iter().all(|b| *b == 0xBB));
}

#[test]
fn vmdk_flat_and_sparse_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let data = pad_to_sector(&mixed_raw_image());
    let (descriptor, flat) = build_vmdk_flat(&data);
    let descriptor_path = dir.path().join("flat.vmdk");
    fs::write(&descriptor_path, descriptor).unwrap();
    fs::write(dir.path().join("disk-flat.vmdk"), flat).unwrap();
    open_and_compare(&descriptor_path, InputFormat::Vmdk, &data);

    let sparse_path = dir.path().join("sparse.vmdk");
    fs::write(&sparse_path, build_vmdk_sparse(&data)).unwrap();
    open_and_compare(&sparse_path, InputFormat::Vmdk, &data);

    let (sparse, sparse_data) = build_vmdk_sparse_with_hole();
    let sparse_path = dir.path().join("hole.vmdk");
    fs::write(&sparse_path, sparse).unwrap();
    open_and_compare(&sparse_path, InputFormat::Vmdk, &sparse_data);

    // A read that crosses an unallocated grain must continue into the next
    // allocated grain instead of zero-filling the whole caller buffer.
    let opened = open_image(&sparse_path, InputFormat::Vmdk).unwrap();
    let mut buf = vec![0u8; 32];
    read_exact_at(opened.source.as_ref(), &mut buf, (2 * 16 * 512 - 12) as u64).unwrap();
    assert!(buf[..12].iter().all(|b| *b == 0));
    assert!(buf[12..].iter().all(|b| *b == 0xBB));
}

#[test]
fn stdin_scan_reports_stdin_as_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("out");
    let report_path = dir.path().join("report.json");
    let mut child = Command::new(bin())
        .args([
            "scan",
            "-",
            "--output",
            output.to_str().unwrap(),
            "--mode",
            "raw",
            "--min-size",
            "1",
            "--quiet",
            "--json-report",
            report_path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&mixed_raw_image())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    // The fixture has no filesystem, so the normal FS detection warning is
    // expected and the CLI keeps its documented incomplete-run exit code.
    assert_eq!(
        result.status.code(),
        Some(3),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert!(report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["code"] == "FS_DETECTION_FAILED"));
    assert_eq!(report["image"], "-");
    assert_eq!(report["input"]["path"], "-");
    assert_eq!(report["input"]["unchanged"], true);
    assert!(report["input"]["modified_unix"].is_null());
    assert_eq!(report["stats"]["carved"].as_u64().unwrap(), 3);
}
