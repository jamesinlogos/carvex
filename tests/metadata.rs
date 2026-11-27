//! H-28/H-29: opt-in recovery from deleted FAT32 directory entries and NTFS
//! `$MFT` records. Default output stays byte-for-byte unchanged.

mod common;

use carvex::scan::{MetadataPassSpec, ScanMode, ScanOptions};
use carvex::signatures::FileType;
use common::{fat32_volume_sectors_spc, png_1x1, png_with_cluster_split, scan_options, JPEG_1X1};
use proptest::prelude::*;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const CLUSTER: usize = 4096;
const DATA_START: usize = 19 * 512;

fn corpus(relative: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("bench/corpus")
        .join(relative);
    fs::read(&path).unwrap_or_else(|e| panic!("missing corpus fixture {}: {e}", path.display()))
}

fn sha256(data: &[u8]) -> String {
    carvex::util::sha256_hex(data)
}

fn le16(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([
        buf.get(off).copied().unwrap_or(0),
        buf.get(off.saturating_add(1)).copied().unwrap_or(0),
    ])
}

fn le64(buf: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(
        buf.get(off..off.saturating_add(8))
            .unwrap_or(&[0u8; 8][..])
            .try_into()
            .unwrap_or([0u8; 8]),
    )
}

struct Scan {
    report: Value,
    files: Vec<Vec<u8>>,
    exit: i32,
}

fn scan(image: &[u8], dir: &Path, metadata: bool) -> Scan {
    scan_with(image, dir, metadata, &[])
}

fn scan_with(image: &[u8], dir: &Path, metadata: bool, extra: &[&str]) -> Scan {
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
    ]);
    if !extra.contains(&"--types") {
        cmd.args(["--types", "jpeg,png,pdf"]);
    }
    cmd.args(["--validate", "decode", "--min-size", "1"]);
    if !extra.contains(&"--max-output") {
        cmd.args(["--max-output", "134217728"]);
    }
    if metadata {
        cmd.arg("--metadata");
    }
    cmd.args(extra);
    let output = cmd.output().unwrap();
    // Metadata warnings that mean evidence was skipped now exit 3 on purpose.
    let exit = output.status.code().unwrap_or(-1);
    assert!(
        exit == 0 || exit == 3,
        "scan failed: exit {exit}, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(&json).unwrap()).unwrap();
    let mut files = Vec::new();
    for entry in fs::read_dir(&out).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            files.push(fs::read(path).unwrap());
        }
    }
    Scan {
        report,
        files,
        exit,
    }
}

fn metadata_rows(report: &Value) -> Vec<&Value> {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|file| file["source"] == "metadata")
        .collect()
}

fn row_by_hash<'a>(report: &'a Value, hash: &str) -> &'a Value {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["sha256"] == hash)
        .unwrap_or_else(|| panic!("missing payload in {report}"))
}

fn short_checksum(name11: &[u8]) -> u8 {
    let mut sum = 0u8;
    for byte in name11.iter().take(11) {
        sum = sum.rotate_left(1).wrapping_add(*byte);
    }
    sum
}

fn short_name(stem: &[u8], ext: &[u8]) -> Vec<u8> {
    // The real 8.3 name before deletion; the directory entry stores 0xE5 in
    // byte 0, but the LFN checksum was computed over this original name.
    let mut out = vec![b'D'];
    for byte in stem.iter().take(7) {
        out.push(byte.to_ascii_uppercase());
    }
    for byte in ext.iter().take(3) {
        out.push(byte.to_ascii_uppercase());
    }
    while out.len() < 12 {
        out.push(b' ');
    }
    out
}

fn lfn_entry(seq: u8, checksum: u8, chars: &[u16], deleted: bool) -> [u8; 32] {
    let mut entry = [0u8; 32];
    entry[0] = if deleted { 0xE5 } else { seq };
    entry[11] = 0x0F;
    entry[13] = checksum;
    for (index, unit) in chars.iter().take(13).enumerate() {
        let offset = match index {
            0..=4 => 1 + index * 2,
            5..=10 => 14 + (index - 5) * 2,
            _ => 28 + (index - 11) * 2,
        };
        if offset.saturating_add(1) < 32 {
            entry[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }
    entry
}

/// A FAT32 volume with one deleted entry in the root directory.
fn fat_image_ex(
    payload: &[u8],
    lfn: Option<&str>,
    valid_checksum: bool,
    reallocated: bool,
    deleted_lfn_slots: bool,
) -> Vec<u8> {
    let mut image = fat32_volume_sectors_spc(2048, 8);
    let file_offset = DATA_START + CLUSTER;
    image[file_offset..file_offset + payload.len()].copy_from_slice(payload);

    let stem = b"DELETED ";
    let ext = match payload.first() {
        Some(0xFF) => b"JPG",
        Some(0x89) => b"PNG",
        _ => b"PDF",
    };
    let short = short_name(stem, ext);
    let checksum = short_checksum(&short);
    let mut entries = Vec::new();
    if let Some(name) = lfn {
        let units: Vec<u16> = name.encode_utf16().collect();
        let count = units.len().div_ceil(13).max(1);
        for part in (0..count).rev() {
            let start = part * 13;
            let chars = &units[start..units.len().min(start + 13)];
            let mut padded = chars.to_vec();
            padded.resize(13, 0xFFFF);
            let seq =
                u8::try_from(part + 1).unwrap_or(1) | if part + 1 == count { 0x40 } else { 0 };
            entries.push(lfn_entry(
                seq,
                if valid_checksum {
                    checksum
                } else {
                    checksum ^ 0x48
                },
                &padded,
                deleted_lfn_slots,
            ));
        }
    }
    let mut dir = [0u8; 32];
    dir[0] = 0xE5;
    dir[1..12].copy_from_slice(&short[1..12]);
    dir[11] = 0x20;
    dir[0x14..0x16].copy_from_slice(&0u16.to_le_bytes());
    dir[0x16..0x18].copy_from_slice(&0u16.to_le_bytes());
    dir[0x18..0x1A].copy_from_slice(&0u16.to_le_bytes());
    dir[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
    dir[0x1C..0x20].copy_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    entries.push(dir);

    let mut offset = DATA_START;
    for entry in entries {
        image[offset..offset + 32].copy_from_slice(&entry);
        offset += 32;
    }
    if reallocated {
        let fat = 512;
        image[fat + 12..fat + 16].copy_from_slice(&4u32.to_le_bytes());
    }
    image
}

fn fat_image(
    payload: &[u8],
    lfn: Option<&str>,
    valid_checksum: bool,
    reallocated: bool,
) -> Vec<u8> {
    fat_image_ex(payload, lfn, valid_checksum, reallocated, false)
}

#[test]
fn fat_deleted_jpeg_png_pdf_are_recovered_with_exact_sha256_and_size() {
    for payload in [
        corpus("jpeg/jpeg_01_baseline_640x480.jpg"),
        png_1x1(),
        corpus("pdf/pdf_08_small_text.pdf"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let image = fat_image(&payload, None, true, false);
        let scan = scan(&image, dir.path(), true);
        let row = row_by_hash(&scan.report, &sha256(&payload));
        assert_eq!(row["size"], payload.len() as u64);
        assert_eq!(row["carved_size"], payload.len() as u64);
        assert_eq!(
            row["contiguous_assumed"],
            true,
            "missing metadata annotation for {} byte payload: {}",
            payload.len(),
            scan.report
        );
        assert!(row["original_name"].as_str().is_some());
        assert!(scan.files.iter().any(|f| sha256(f) == sha256(&payload)));
        assert_eq!(scan.report["options"]["metadata"], true);
    }
}

#[test]
fn fat_long_and_non_ascii_names_are_display_only() {
    let dir = tempfile::tempdir().unwrap();
    let payload = png_1x1();
    let image = fat_image(&payload, Some("café_photo.png"), true, false);
    let scan = scan(&image, dir.path(), true);
    let row = row_by_hash(&scan.report, &sha256(&payload));
    let name = row["original_name"].as_str().unwrap();
    assert_eq!(name, "café_photo.png");
    let checks = row["checks"].as_array().unwrap();
    assert!(checks
        .iter()
        .any(|check| check["name"] == "lfn_checksum" && check["ok"] == true));
    for entry in fs::read_dir(dir.path().join("out")).unwrap() {
        let filename = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(!filename.contains("café"));
        assert!(filename.ends_with(".png"));
    }
}

#[test]
fn fat_bad_lfn_checksum_drops_only_the_long_name() {
    let dir = tempfile::tempdir().unwrap();
    let payload = png_1x1();
    let image = fat_image(&payload, Some("wanted_name.png"), false, false);
    let scan = scan(&image, dir.path(), true);
    let row = row_by_hash(&scan.report, &sha256(&payload));
    let notes = row["metadata_notes"].as_array().unwrap_or_else(|| {
        panic!("no metadata notes on row in {}", scan.report);
    });
    assert!(
        notes.iter().any(|note| note == "lfn_checksum_mismatch"),
        "notes were {notes:?} in {}",
        scan.report
    );
    let name = row["original_name"].as_str().unwrap();
    assert_ne!(name, "wanted_name.png");
    let checks = row["checks"].as_array().unwrap();
    assert!(checks
        .iter()
        .any(|check| check["name"] == "lfn_checksum" && check["ok"] == false));
}

#[test]
fn fat_deleted_and_out_of_order_lfn_slots_bind_in_physical_order() {
    let dir = tempfile::tempdir().unwrap();
    let payload = png_1x1();
    // Every long-name slot has its sequence byte overwritten by the deletion
    // marker. Only the physical order (descending sequence) can recover it.
    let image = fat_image_ex(&payload, Some("café_photo.png"), true, false, true);
    let scan = scan(&image, dir.path(), true);
    let row = row_by_hash(&scan.report, &sha256(&payload));
    assert_eq!(row["original_name"], "café_photo.png");
    let checks = row["checks"].as_array().unwrap();
    assert!(checks
        .iter()
        .any(|check| check["name"] == "lfn_checksum" && check["ok"] == true));
}

#[test]
fn fat_reallocated_first_cluster_is_partial_with_overwrite_count() {
    let dir = tempfile::tempdir().unwrap();
    let payload = png_1x1();
    let image = fat_image(&payload, None, true, true);
    let scan = scan(&image, dir.path(), true);
    let row = metadata_rows(&scan.report)
        .into_iter()
        .find(|row| row["sha256"] == sha256(&payload))
        .expect("deleted PNG row");
    assert_eq!(row["overwritten_clusters"], 1);
    assert_eq!(row["validity"], "partial");
    assert_eq!(row["validity_reason"], "overwritten_clusters");
}

#[test]
fn fat_corrupted_directory_is_bounded_and_never_panics() {
    let dir = tempfile::tempdir().unwrap();
    let mut image = fat32_volume_sectors_spc(2048, 8);
    let mut seed = 0x1234_5678u32;
    for chunk in image[DATA_START..DATA_START + CLUSTER].chunks_exact_mut(4) {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        chunk.copy_from_slice(&seed.to_le_bytes());
    }
    let scan = scan(&image, dir.path(), true);
    assert!(scan.report["files"].is_array());
}

fn fat_image_with_live_subdir(payload: &[u8]) -> Vec<u8> {
    let mut image = fat32_volume_sectors_spc(2048, 8);
    let sub_cluster = 3u16;
    let file_cluster = 4u16;
    let file_offset = DATA_START + 2 * CLUSTER;
    image[file_offset..file_offset + payload.len()].copy_from_slice(payload);

    let mut live = [0u8; 32];
    live[0] = b'L';
    live[1..11].copy_from_slice(b"IVE       ");
    live[11] = 0x10;
    live[0x1A..0x1C].copy_from_slice(&sub_cluster.to_le_bytes());
    image[DATA_START..DATA_START + 32].copy_from_slice(&live);

    // The directory itself is allocated and ends at cluster 3; the deleted
    // JPEG begins in the next (free) cluster.
    let fat = 512;
    image[fat + 12..fat + 16].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());

    let short = short_name(b"DELETED ", b"JPG");
    let mut dir = [0u8; 32];
    dir[0] = 0xE5;
    dir[1..12].copy_from_slice(&short[1..12]);
    dir[11] = 0x20;
    dir[0x1A..0x1C].copy_from_slice(&file_cluster.to_le_bytes());
    dir[0x1C..0x20].copy_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    let sub_offset = DATA_START + CLUSTER;
    image[sub_offset..sub_offset + 32].copy_from_slice(&dir);
    image
}

#[test]
fn fat_deleted_file_inside_live_subdirectory_is_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let payload = JPEG_1X1.to_vec();
    let image = fat_image_with_live_subdir(&payload);
    let scan = scan(&image, dir.path(), true);
    let row = row_by_hash(&scan.report, &sha256(&payload));
    assert_eq!(row["size"], payload.len() as u64);
    assert!(
        row["original_name"].as_str().is_some(),
        "row was {row} in {}",
        scan.report
    );
}

#[test]
fn metadata_flag_rejects_incompatible_scan_modes() {
    let dir = tempfile::tempdir().unwrap();
    let image = fat_image(&png_1x1(), None, true, false);
    let image_path = dir.path().join("image.img");
    fs::write(&image_path, &image).unwrap();
    let image = image_path.to_str().unwrap();
    let out = dir.path().join("out");
    let out = out.to_str().unwrap();

    for extra in [vec!["--no-fs"], vec!["--mode", "allocated"]] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_carvex"));
        let reject_no_fs = extra == vec!["--no-fs"];
        cmd.args(["scan", image, "-o", out, "--quiet", "--metadata"])
            .args(extra);
        if reject_no_fs {
            cmd.arg("--force");
        }
        let result = cmd.output().unwrap();
        assert_eq!(result.status.code(), Some(1), "{:?}", result);
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains("--metadata"), "stderr was {stderr}");
    }
}

#[test]
fn metadata_flag_requires_a_selected_volume() {
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("raw.img");
    fs::write(&image_path, png_1x1()).unwrap();
    let out = dir.path().join("out");
    let result = Command::new(env!("CARGO_BIN_EXE_carvex"))
        .args([
            "scan",
            image_path.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--quiet",
            "--metadata",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success(), "{:?}", result);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("requires at least one selected volume"),
        "stderr was {stderr}"
    );
}

#[test]
fn default_scan_omits_metadata_option_and_rows() {
    let dir = tempfile::tempdir().unwrap();
    let payload = png_1x1();
    let image = fat_image(&payload, None, true, false);
    let scan = scan(&image, dir.path(), false);
    assert_eq!(scan.exit, 0);
    assert!(scan.report["options"].get("metadata").is_none());
    assert!(metadata_rows(&scan.report).is_empty());
}

// ---------------------------------------------------------------------------
// NTFS fixtures
// ---------------------------------------------------------------------------

fn le16_mut(buf: &mut [u8], off: usize, value: u16) {
    if off.saturating_add(2) <= buf.len() {
        buf[off..off + 2].copy_from_slice(&value.to_le_bytes());
    }
}

fn le32_mut(buf: &mut [u8], off: usize, value: u32) {
    if off.saturating_add(4) <= buf.len() {
        buf[off..off + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn le64_mut(buf: &mut [u8], off: usize, value: u64) {
    if off.saturating_add(8) <= buf.len() {
        buf[off..off + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn align8(len: usize) -> usize {
    len.div_ceil(8) * 8
}

fn resident_attr(type_id: u32, value: &[u8]) -> Vec<u8> {
    let value_off = 0x18usize;
    let len = align8(value_off + value.len());
    let mut attr = vec![0u8; len];
    le32_mut(&mut attr, 0, type_id);
    le32_mut(&mut attr, 4, len as u32);
    attr[8] = 0;
    le32_mut(&mut attr, 0x10, value.len() as u32);
    le16_mut(&mut attr, 0x14, value_off as u16);
    if value_off + value.len() <= attr.len() {
        attr[value_off..value_off + value.len()].copy_from_slice(value);
    }
    attr
}

fn nonresident_attr(type_id: u32, runs: &[u8], data_size: u64, flags: u16) -> Vec<u8> {
    let run_off = 0x40usize;
    let len = align8(run_off + runs.len());
    let mut attr = vec![0u8; len];
    le32_mut(&mut attr, 0, type_id);
    le32_mut(&mut attr, 4, len as u32);
    attr[8] = 1;
    le16_mut(&mut attr, 0x0C, flags);
    le16_mut(&mut attr, 0x20, run_off as u16);
    le64_mut(&mut attr, 0x30, data_size);
    le64_mut(&mut attr, 0x38, data_size);
    if run_off + runs.len() <= attr.len() {
        attr[run_off..]
            .iter_mut()
            .zip(runs)
            .for_each(|(d, s)| *d = *s);
    }
    attr
}

fn standard_info() -> Vec<u8> {
    let mut value = vec![0u8; 72];
    let filetime = 1_700_000_000u64
        .saturating_mul(10_000_000)
        .saturating_add(116_444_736_000_000_000);
    le64_mut(&mut value, 0x08, filetime);
    value
}

fn file_name_value(name: &str) -> Vec<u8> {
    let units: Vec<u16> = name.encode_utf16().collect();
    let mut value = vec![0u8; 0x42 + units.len() * 2];
    le64_mut(&mut value, 0, 5 | ((5u64) << 48));
    value[0x40] = u8::try_from(units.len()).unwrap_or(0);
    value[0x41] = 1;
    for (index, unit) in units.iter().enumerate() {
        le16_mut(&mut value, 0x42 + index * 2, *unit);
    }
    value
}

fn mft_record(seq: u16, attrs: &[Vec<u8>], broken_fixup: bool) -> Vec<u8> {
    let mut rec = vec![0u8; 1024];
    rec[0..4].copy_from_slice(b"FILE");
    le16_mut(&mut rec, 0x04, 0x30); // update sequence array offset
    le16_mut(&mut rec, 0x06, 3); // two sector trailers plus the USN
    le16_mut(&mut rec, 0x10, seq);
    le16_mut(&mut rec, 0x12, 1);
    le16_mut(&mut rec, 0x14, 0x38);
    le16_mut(&mut rec, 0x16, 0); // deleted
    le32_mut(&mut rec, 0x18, 1024);
    le32_mut(&mut rec, 0x1C, 1024);
    let mut offset = 0x38usize;
    for attr in attrs {
        if offset + attr.len() + 4 > rec.len() {
            break;
        }
        rec[offset..offset + attr.len()].copy_from_slice(attr);
        offset += attr.len();
    }
    le32_mut(&mut rec, offset, 0xFFFF_FFFF);

    // Preserve the bytes that currently occupy each sector trailer in the USA
    // slots, then replace the trailers with the USN.  Fixup restores the saved
    // bytes, so attributes that cross a sector boundary stay byte-exact.
    let original = [[rec[510], rec[511]], [rec[1022], rec[1023]]];
    le16_mut(&mut rec, 0x30, 0xBBAA);
    rec[0x32..0x34].copy_from_slice(&original[0]);
    rec[0x34..0x36].copy_from_slice(&original[1]);
    let usn = if broken_fixup {
        [0xEEu8, 0xEEu8]
    } else {
        [0xAAu8, 0xBBu8]
    };
    rec[510..512].copy_from_slice(&usn);
    rec[1022..1024].copy_from_slice(&usn);
    rec
}

fn ntfs_geometry(image: &[u8]) -> (u64, u64, u64, u64) {
    let bps = u64::from(le16(image, 0x0B));
    let spc = u64::from(image.get(0x0D).copied().unwrap_or(1));
    let cluster = bps * spc;
    let mft_lcn = le64(image, 0x30);
    let cpr = image.get(0x40).copied().unwrap_or(0) as i8;
    let record = if cpr > 0 {
        u64::from(cpr as u8) * cluster
    } else {
        1u64 << cpr.unsigned_abs()
    };
    (cluster, record, mft_lcn * cluster, mft_lcn)
}

/// Create the NTFS fixture or fail the test.
///
/// A missing `mkntfs` used to be printed as `SKIP` and then returned, which
/// made the test a silent pass. The harness requires SKIP never to be silent,
/// so this deliberately panics with an actionable message instead. On
/// non-Linux runners the five mkntfs-dependent tests carry a visible
/// `#[ignore]` reason rather than pretending to have run.
fn mkntfs_image(dir: &Path) -> Vec<u8> {
    let probe = Command::new("mkntfs").arg("--version").output();
    assert!(
        probe.is_ok_and(|out| out.status.success()),
        "mkntfs is required for the NTFS metadata tests; install ntfs-3g"
    );
    let path = dir.join("ntfs.img");
    let file = fs::File::create(&path).expect("cannot create mkntfs fixture image");
    file.set_len(64 * 1024 * 1024)
        .expect("cannot size mkntfs fixture image");
    drop(file);
    let status = Command::new("mkntfs")
        .args(["-F", "-f", "-q"])
        .arg(&path)
        .arg("131072")
        .status()
        .expect("cannot execute mkntfs");
    assert!(status.success(), "mkntfs failed: {status}");
    fs::read(&path).expect("cannot read mkntfs fixture image")
}

fn runlist(clusters: u64, delta: i8) -> Vec<u8> {
    vec![0x11, clusters as u8, delta as u8]
}

fn ntfs_scan(dir: &Path, image: &[u8]) -> Scan {
    scan(image, dir, true)
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "H-29: mkntfs fixture requires Linux"
)]
fn ntfs_resident_deleted_record_recovers_exact_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let mut image = mkntfs_image(dir.path());
    let (_cluster, record, mft_off, _) = ntfs_geometry(&image);
    let payload = png_1x1();
    let attrs = vec![
        resident_attr(0x10, &standard_info()),
        resident_attr(0x30, &file_name_value("resident.png")),
        resident_attr(0x80, &payload),
    ];
    let rec = mft_record(7, &attrs, false);
    let offset = usize::try_from(mft_off + 8 * record).unwrap();
    image[offset..offset + rec.len()].copy_from_slice(&rec);
    let scan = ntfs_scan(dir.path(), &image);
    let rows = metadata_rows(&scan.report);
    let row = rows
        .iter()
        .find(|row| row["sha256"] == sha256(&payload))
        .unwrap_or_else(|| panic!("no resident row in {}", scan.report));
    assert_eq!(row["original_name"], "resident.png");
    assert_eq!(row["sparse_bytes"], 0);
    assert!(row["mtime"].as_str().is_some());
    assert!(scan.files.iter().any(|f| sha256(f) == sha256(&payload)));
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "H-29: mkntfs fixture requires Linux"
)]
fn ntfs_nonresident_fragmented_record_round_trips_with_negative_delta() {
    let dir = tempfile::tempdir().unwrap();
    let mut image = mkntfs_image(dir.path());
    let (cluster, record, mft_off, _) = ntfs_geometry(&image);
    let payload = png_with_cluster_split(CLUSTER);
    assert!(payload.len() > CLUSTER);
    let first_lcn = 100u64;
    let second_lcn = 99u64;
    let first = usize::try_from(first_lcn * cluster).unwrap();
    let second = usize::try_from(second_lcn * cluster).unwrap();
    image[first..first + CLUSTER].copy_from_slice(&payload[..CLUSTER]);
    image[second..second + (payload.len() - CLUSTER)].copy_from_slice(&payload[CLUSTER..]);
    let runs = [runlist(1, 100), runlist(1, -1)].concat();
    let attrs = vec![
        resident_attr(0x10, &standard_info()),
        resident_attr(0x30, &file_name_value("fragmented.png")),
        nonresident_attr(0x80, &runs, payload.len() as u64, 0),
    ];
    let rec = mft_record(9, &attrs, false);
    let offset = usize::try_from(mft_off + 8 * record).unwrap();
    image[offset..offset + rec.len()].copy_from_slice(&rec);
    let scan = ntfs_scan(dir.path(), &image);
    let row = metadata_rows(&scan.report)
        .into_iter()
        .find(|row| row["sha256"] == sha256(&payload))
        .unwrap_or_else(|| panic!("no fragmented row in {}", scan.report));
    assert_eq!(row["size"], payload.len() as u64);
    assert!(row["validity"] != "verified");
    assert!(scan.files.iter().any(|f| sha256(f) == sha256(&payload)));
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "H-29: mkntfs fixture requires Linux"
)]
fn ntfs_unsupported_attribute_list_is_never_verified() {
    let dir = tempfile::tempdir().unwrap();
    let mut image = mkntfs_image(dir.path());
    let (_cluster, record, mft_off, _) = ntfs_geometry(&image);
    let payload = png_1x1();
    let attrs = vec![
        resident_attr(0x10, &standard_info()),
        resident_attr(0x30, &file_name_value("unsupported.png")),
        resident_attr(0x20, &[0u8; 16]),
        resident_attr(0x80, &payload),
    ];
    let rec = mft_record(11, &attrs, false);
    let offset = usize::try_from(mft_off + 8 * record).unwrap();
    image[offset..offset + rec.len()].copy_from_slice(&rec);
    let scan = ntfs_scan(dir.path(), &image);
    let row = metadata_rows(&scan.report)
        .into_iter()
        .find(|row| row["sha256"] == sha256(&payload))
        .expect("unsupported row");
    assert_eq!(row["validity_reason"], "unsupported");
    assert_ne!(row["validity"], "verified");
    let notes = row["metadata_notes"].as_array().unwrap();
    assert!(notes.iter().any(|note| note == "unsupported_mft_feature"));
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "H-29: mkntfs fixture requires Linux"
)]
fn ntfs_fixup_mismatch_record_is_skipped_without_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut image = mkntfs_image(dir.path());
    let (_, record, mft_off, _) = ntfs_geometry(&image);
    let payload = png_1x1();
    let attrs = vec![
        resident_attr(0x10, &standard_info()),
        resident_attr(0x30, &file_name_value("broken.png")),
        resident_attr(0x80, &payload),
    ];
    let rec = mft_record(13, &attrs, true);
    let offset = usize::try_from(mft_off + 8 * record).unwrap();
    image[offset..offset + rec.len()].copy_from_slice(&rec);
    let scan = ntfs_scan(dir.path(), &image);
    assert!(metadata_rows(&scan.report)
        .iter()
        .all(|row| row["sha256"] != sha256(&payload)));
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "H-29: mkntfs fixture requires Linux"
)]
fn mkntfs_volume_with_deleted_record_is_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let mut image = mkntfs_image(dir.path());
    let (_, record, mft_off, _) = ntfs_geometry(&image);
    let payload = JPEG_1X1.to_vec();
    let attrs = vec![
        resident_attr(0x10, &standard_info()),
        resident_attr(0x30, &file_name_value("mkntfs.jpg")),
        resident_attr(0x80, &payload),
    ];
    let rec = mft_record(15, &attrs, false);
    let offset = usize::try_from(mft_off + 8 * record).unwrap();
    image[offset..offset + rec.len()].copy_from_slice(&rec);
    let scan = ntfs_scan(dir.path(), &image);
    assert!(
        metadata_rows(&scan.report)
            .iter()
            .any(|row| row["sha256"] == sha256(&payload)),
        "no mkntfs metadata row in {}",
        scan.report
    );
}

fn encode_runlist(runs: &[(u64, Option<i64>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (clusters, delta) in runs {
        let len = u8::try_from(*clusters).expect("test runs fit one byte");
        match delta {
            Some(value) => {
                out.push(0x11);
                out.push(len);
                out.push(*value as i8 as u8);
            }
            None => {
                out.push(0x01);
                out.push(len);
            }
        }
    }
    out
}

fn direct_metadata_pass(
    image: &[u8],
    dir: &Path,
    configure: impl FnOnce(&mut ScanOptions),
) -> (
    Vec<carvex::scan::MetadataResult>,
    carvex::scan::MetadataPassStats,
    Vec<carvex::scan::Warning>,
) {
    let source = common::MemSource::new(image.to_vec());
    let set = carvex::fs::detect_all(&source, image.len() as u64).unwrap();
    let mut opts = scan_options(FileType::all().to_vec(), ScanMode::Unallocated);
    opts.metadata = true;
    opts.min_size = 1;
    configure(&mut opts);
    let mut known = HashSet::new();
    carvex::scan::run_metadata_pass(MetadataPassSpec {
        source: &source,
        volumes: &set.volumes,
        existing: &[],
        opts: &opts,
        out_dir: dir,
        start_seq: 1,
        known: &mut known,
        initial_output_bytes: 0,
    })
    .unwrap()
}

#[test]
fn metadata_verified_only_never_writes_partial_rows() {
    let dir = tempfile::tempdir().unwrap();
    let payload = png_1x1();
    let image = fat_image(&payload, None, true, false);
    let scan = scan_with(&image, dir.path(), true, &["--verified-only"]);
    assert!(metadata_rows(&scan.report).is_empty());
    assert!(scan.report["files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["validity"] == "verified"));
    let warnings = scan.report["warnings"].as_array().unwrap();
    assert!(warnings
        .iter()
        .any(|w| w["code"] == "METADATA_VERIFIED_ONLY"));
}

#[test]
fn metadata_types_filter_applies_to_the_second_pass() {
    let dir = tempfile::tempdir().unwrap();
    let payload = png_1x1();
    let image = fat_image(&payload, None, true, false);
    let scan = scan_with(&image, dir.path(), true, &["--types", "pdf"]);
    assert_eq!(scan.report["options"]["types"], serde_json::json!(["pdf"]));
    assert!(scan.report["files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["sha256"] != sha256(&payload)));
    assert!(metadata_rows(&scan.report).is_empty());
}

#[test]
fn metadata_output_cap_is_reserved_before_a_single_write() {
    let dir = tempfile::tempdir().unwrap();
    let payload = png_1x1();
    let image = fat_image(&payload, None, true, false);
    let scan = scan_with(&image, dir.path(), true, &["--max-output", "64"]);
    assert!(scan.report["files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["sha256"] != sha256(&payload)));
    let warnings = scan.report["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w["code"] == "OUTPUT_LIMIT"));
}

#[test]
fn metadata_memory_cap_bounds_materialised_candidate_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let payload = png_with_cluster_split(CLUSTER);
    let image = fat_image(&payload, None, true, false);
    let (results, stats, _) = direct_metadata_pass(&image, dir.path(), |opts| {
        opts.max_memory = 128;
    });
    assert!(results.iter().all(|result| result.file.carved_size <= 128));
    assert!(stats.parse_failed > 0 || results.is_empty());
}

#[test]
fn metadata_time_budget_zero_is_unlimited_and_tiny_budget_expires() {
    let image = fat_image(&png_1x1(), None, true, false);
    let unlimited_dir = tempfile::tempdir().unwrap();
    let (_, _, unlimited_warnings) = direct_metadata_pass(&image, unlimited_dir.path(), |_| {});
    assert!(!unlimited_warnings
        .iter()
        .any(|w| w.code == "METADATA_TIME_LIMIT"));

    let tiny_dir = tempfile::tempdir().unwrap();
    let (_, stats, tiny_warnings) = direct_metadata_pass(&image, tiny_dir.path(), |opts| {
        opts.time_budget = Duration::from_nanos(1);
    });
    assert!(stats.time_limited);
    assert!(tiny_warnings
        .iter()
        .any(|w| w.code == "METADATA_TIME_LIMIT"));
}

#[test]
fn metadata_manifest_includes_metadata_rows() {
    let dir = tempfile::tempdir().unwrap();
    let payload = png_1x1();
    let image = fat_image(&payload, None, true, false);
    let scan = scan_with(&image, dir.path(), true, &["--manifest"]);
    let annotated = scan.report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["original_name"].is_string())
        .unwrap_or_else(|| panic!("no annotated row in {}", scan.report));
    let manifest = fs::read_to_string(dir.path().join("out/SHA256SUMS")).unwrap();
    assert!(manifest.contains(annotated["sha256"].as_str().unwrap()));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]
    #[test]
    fn ntfs_runlist_roundtrip_includes_negative_offsets_and_sparse_runs(
        runs in prop::collection::vec((1u8..=16, -64i64..=64, any::<bool>()), 1..8)
    ) {
        let mut lcn = 0i64;
        let prepared: Vec<(u64, Option<i64>)> = runs
            .iter()
            .enumerate()
            .map(|(index, (clusters, delta, sparse))| {
                if *sparse {
                    return (u64::from(*clusters), None);
                }
                // The first extent must start at a positive LCN so the parser
                // can accumulate later negative deltas without going below 0.
                let requested = if index == 0 {
                    1i64.saturating_add(delta.saturating_abs())
                } else {
                    *delta
                };
                let next = lcn.saturating_add(requested);
                let applied = if next < 0 {
                    requested.saturating_abs()
                } else {
                    requested
                };
                lcn = lcn.saturating_add(applied);
                (u64::from(*clusters), Some(applied))
            })
            .collect();
        let encoded = encode_runlist(&prepared);
        let parsed = carvex::fs::ntfs::parse_runlist_public(&encoded)
            .expect("generated runlist must parse");
        prop_assert_eq!(parsed.len(), prepared.len());
        let mut vcn = 0u64;
        let mut lcn = 0i64;
        for ((clusters, delta), (got_vcn, got_lcn, got_clusters)) in prepared.iter().zip(parsed) {
            let expected_lcn = match delta {
                Some(value) => {
                    lcn = lcn.saturating_add(*value);
                    Some(u64::try_from(lcn).unwrap_or(0))
                }
                None => None,
            };
            prop_assert_eq!(got_vcn, vcn);
            prop_assert_eq!(got_lcn, expected_lcn);
            prop_assert_eq!(got_clusters, *clusters);
            vcn = vcn.saturating_add(*clusters);
        }
    }
}
