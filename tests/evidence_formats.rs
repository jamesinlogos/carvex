//! B-04 evidence format coverage.
//!
//! Fixtures are generated in memory and written to a temporary directory.
//! Each container must expose exactly the same byte stream as the raw image;
//! the stdout contract is checked separately for stdin.

mod common;

use carvex::io::{open_image, read_exact_at, ImageSource, InputFormat};
use common::mixed_raw_image;
use flate2::{write::ZlibEncoder, Compression};
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

fn pad_to_sector(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    out.resize(data.len().div_ceil(512) * 512, 0);
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn put_descriptor(out: &mut Vec<u8>, kind: &str) -> usize {
    let pos = out.len();
    let mut descriptor = [0u8; 76];
    descriptor[..kind.len()].copy_from_slice(kind.as_bytes());
    out.extend_from_slice(&descriptor);
    pos
}

fn patch_descriptor(out: &mut [u8], pos: usize, next: u64, size: u64) {
    out[pos + 16..pos + 24].copy_from_slice(&next.to_le_bytes());
    out[pos + 24..pos + 32].copy_from_slice(&size.to_le_bytes());
}

fn build_e01(data: &[u8], compressed: bool) -> Vec<u8> {
    const CHUNK: usize = 32 * 1024;
    let mut out = Vec::new();
    out.extend_from_slice(b"EVF\x09\x0d\x0a\xff\x00");
    out.push(1);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    let volume_desc = put_descriptor(&mut out, "volume");
    let mut volume = vec![0u8; 1052];
    volume[8..12].copy_from_slice(&((CHUNK / 512) as u32).to_le_bytes());
    volume[12..16].copy_from_slice(&512u32.to_le_bytes());
    volume[16..24].copy_from_slice(&((data.len() / 512) as u64).to_le_bytes());
    out.extend_from_slice(&volume);
    let next = out.len() as u64;
    patch_descriptor(&mut out, volume_desc, next, 1052);

    let sectors_desc = put_descriptor(&mut out, "sectors");
    let sectors_data = out.len();
    let mut chunk_offsets = Vec::new();
    for chunk in data.chunks(CHUNK) {
        let mut full = vec![0u8; CHUNK];
        full[..chunk.len()].copy_from_slice(chunk);
        chunk_offsets.push((out.len() as u32, compressed));
        if compressed {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&full).unwrap();
            out.extend_from_slice(&encoder.finish().unwrap());
        } else {
            out.extend_from_slice(&full);
            out.extend_from_slice(&adler32(&full).to_le_bytes());
        }
    }
    let next = out.len() as u64;
    patch_descriptor(
        &mut out,
        sectors_desc,
        next,
        (next as usize - sectors_data) as u64,
    );

    let table_desc = put_descriptor(&mut out, "table");
    let table_data = out.len();
    out.extend_from_slice(&(chunk_offsets.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 20]);
    for (offset, is_compressed) in chunk_offsets {
        let flag = if is_compressed { 0x8000_0000 } else { 0 };
        out.extend_from_slice(&(offset | flag).to_le_bytes());
    }
    out.extend_from_slice(&[0u8; 4]);
    let next = out.len() as u64;
    patch_descriptor(
        &mut out,
        table_desc,
        next,
        (next as usize - table_data) as u64,
    );

    let done_desc = put_descriptor(&mut out, "done");
    patch_descriptor(&mut out, done_desc, done_desc as u64, 0);
    out
}

fn checksum(buf: &mut [u8], offset: usize) {
    let mut copy = buf.to_vec();
    copy[offset..offset + 4].fill(0);
    let sum: u32 = copy.iter().fold(0u64, |acc, b| acc + u64::from(*b)) as u32;
    buf[offset..offset + 4].copy_from_slice(&(!sum).to_be_bytes());
}

fn vhd_footer(current_size: u64, data_offset: u64, disk_type: u32) -> Vec<u8> {
    let mut footer = vec![0u8; 512];
    footer[..8].copy_from_slice(b"conectix");
    footer[16..24].copy_from_slice(&data_offset.to_be_bytes());
    footer[48..56].copy_from_slice(&current_size.to_be_bytes());
    footer[60..64].copy_from_slice(&disk_type.to_be_bytes());
    checksum(&mut footer, 64);
    footer
}

fn build_vhd_dynamic(data: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 64 * 1024;
    const BAT_OFFSET: u64 = 1536;
    const BLOCK_OFFSET: u64 = 2048;
    const BITMAP: usize = 512;

    let mut out = vec![0u8; (BLOCK_OFFSET as usize) + BITMAP + BLOCK];
    out[..512].copy_from_slice(&vhd_footer(data.len() as u64, 512, 3));

    let mut header = vec![0u8; 1024];
    header[..8].copy_from_slice(b"cxsparse");
    header[8..16].copy_from_slice(&u64::MAX.to_be_bytes());
    header[16..24].copy_from_slice(&BAT_OFFSET.to_be_bytes());
    header[24..28].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    header[28..32].copy_from_slice(&1u32.to_be_bytes());
    header[32..36].copy_from_slice(&(BLOCK as u32).to_be_bytes());
    checksum(&mut header, 36);
    out[512..1536].copy_from_slice(&header);

    out[BAT_OFFSET as usize..BAT_OFFSET as usize + 4]
        .copy_from_slice(&((BLOCK_OFFSET / 512) as u32).to_be_bytes());
    let data_start = BLOCK_OFFSET as usize + BITMAP;
    out[data_start..data_start + data.len()].copy_from_slice(data);

    out.extend_from_slice(&vhd_footer(data.len() as u64, 512, 3));
    out
}

fn build_vhd_fixed(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    out.extend_from_slice(&vhd_footer(data.len() as u64, u64::MAX, 2));
    out
}

fn build_vhd_dynamic_with_hole() -> (Vec<u8>, Vec<u8>) {
    const BLOCK: usize = 64 * 1024;
    const BITMAP: usize = 512;
    const BAT_OFFSET: u64 = 1536;
    const BLOCK0: u64 = 2048;
    const BLOCK2: u64 = BLOCK0 + (BITMAP as u64) + (BLOCK as u64);

    let mut logical = vec![0u8; BLOCK * 3];
    logical[..BLOCK].fill(0xAA);
    logical[BLOCK * 2..].fill(0xBB);

    let mut out = vec![0u8; (BLOCK2 as usize) + BITMAP + BLOCK];
    out[..512].copy_from_slice(&vhd_footer(logical.len() as u64, 512, 3));

    let mut header = vec![0u8; 1024];
    header[..8].copy_from_slice(b"cxsparse");
    header[8..16].copy_from_slice(&u64::MAX.to_be_bytes());
    header[16..24].copy_from_slice(&BAT_OFFSET.to_be_bytes());
    header[24..28].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    header[28..32].copy_from_slice(&3u32.to_be_bytes());
    header[32..36].copy_from_slice(&(BLOCK as u32).to_be_bytes());
    checksum(&mut header, 36);
    out[512..1536].copy_from_slice(&header);

    out[BAT_OFFSET as usize..BAT_OFFSET as usize + 4]
        .copy_from_slice(&((BLOCK0 / 512) as u32).to_be_bytes());
    out[BAT_OFFSET as usize + 4..BAT_OFFSET as usize + 8].copy_from_slice(&u32::MAX.to_be_bytes());
    out[BAT_OFFSET as usize + 8..BAT_OFFSET as usize + 12]
        .copy_from_slice(&((BLOCK2 / 512) as u32).to_be_bytes());

    out[BLOCK0 as usize..BLOCK0 as usize + BITMAP].fill(0xff);
    out[BLOCK0 as usize + BITMAP..BLOCK0 as usize + BITMAP + BLOCK].fill(0xAA);
    out[BLOCK2 as usize..BLOCK2 as usize + BITMAP].fill(0xff);
    out[BLOCK2 as usize + BITMAP..BLOCK2 as usize + BITMAP + BLOCK].fill(0xBB);
    out.extend_from_slice(&vhd_footer(logical.len() as u64, 512, 3));
    (out, logical)
}

fn build_vmdk_flat(data: &[u8]) -> (String, Vec<u8>) {
    let sectors = data.len() / 512;
    let descriptor = format!(
        "# Disk DescriptorFile\nversion=1\n# Extent description\nRW {sectors} FLAT \"disk-flat.vmdk\" 0\n"
    );
    (descriptor, data.to_vec())
}

fn build_vmdk_sparse(data: &[u8]) -> Vec<u8> {
    const GRAIN_SECTORS: u64 = 16;
    const GRAIN_SECTOR: u32 = 16;
    let sectors = (data.len() / 512) as u64;
    let mut out = vec![0u8; (GRAIN_SECTOR as usize * 512) + data.len()];

    let mut header = vec![0u8; 512];
    header[..4].copy_from_slice(b"KDMV");
    header[4..8].copy_from_slice(&1u32.to_le_bytes());
    header[12..20].copy_from_slice(&sectors.to_le_bytes());
    header[20..28].copy_from_slice(&GRAIN_SECTORS.to_le_bytes());
    header[28..36].copy_from_slice(&1u64.to_le_bytes());
    header[36..44].copy_from_slice(&1u64.to_le_bytes());
    header[44..48].copy_from_slice(&512u32.to_le_bytes());
    header[56..64].copy_from_slice(&2u64.to_le_bytes());
    header[64..72].copy_from_slice(&1u64.to_le_bytes());
    out[..512].copy_from_slice(&header);

    let descriptor = format!(
        "# Disk DescriptorFile\n# Extent description\nRW {sectors} SPARSE \"sparse.vmdk\" 0\n"
    );
    out[512..512 + descriptor.len()].copy_from_slice(descriptor.as_bytes());
    out[1024..1028].copy_from_slice(&3u32.to_le_bytes());
    out[1536..1540].copy_from_slice(&GRAIN_SECTOR.to_le_bytes());
    out[GRAIN_SECTOR as usize * 512..].copy_from_slice(data);
    out
}

fn build_vmdk_sparse_with_hole() -> (Vec<u8>, Vec<u8>) {
    const GRAIN_SECTORS: u64 = 16;
    const GRAIN_BYTES: usize = GRAIN_SECTORS as usize * 512;
    const GRAIN0_SECTOR: u32 = 16;
    const GRAIN2_SECTOR: u32 = 32;

    let mut logical = vec![0u8; GRAIN_BYTES * 3];
    logical[..GRAIN_BYTES].fill(0xAA);
    logical[GRAIN_BYTES * 2..].fill(0xBB);

    let mut out = vec![0u8; GRAIN2_SECTOR as usize * 512 + GRAIN_BYTES];
    let mut header = vec![0u8; 512];
    header[..4].copy_from_slice(b"KDMV");
    header[4..8].copy_from_slice(&1u32.to_le_bytes());
    header[8..12].copy_from_slice(&1u32.to_le_bytes());
    header[12..20].copy_from_slice(&48u64.to_le_bytes());
    header[20..28].copy_from_slice(&GRAIN_SECTORS.to_le_bytes());
    header[28..36].copy_from_slice(&1u64.to_le_bytes());
    header[36..44].copy_from_slice(&1u64.to_le_bytes());
    header[44..48].copy_from_slice(&512u32.to_le_bytes());
    header[56..64].copy_from_slice(&2u64.to_le_bytes());
    header[64..72].copy_from_slice(&1u64.to_le_bytes());
    out[..512].copy_from_slice(&header);

    let descriptor = b"# Disk DescriptorFile\n# Extent description\nRW 48 SPARSE \"hole.vmdk\" 0\n";
    out[512..512 + descriptor.len()].copy_from_slice(descriptor);
    out[1024..1028].copy_from_slice(&3u32.to_le_bytes());
    out[1536..1540].copy_from_slice(&GRAIN0_SECTOR.to_le_bytes());
    out[1544..1548].copy_from_slice(&GRAIN2_SECTOR.to_le_bytes());
    out[GRAIN0_SECTOR as usize * 512..GRAIN0_SECTOR as usize * 512 + GRAIN_BYTES].fill(0xAA);
    out[GRAIN2_SECTOR as usize * 512..GRAIN2_SECTOR as usize * 512 + GRAIN_BYTES].fill(0xBB);
    (out, logical)
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
