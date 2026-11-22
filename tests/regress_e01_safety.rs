//! H-11 / H-22: hostile E01 headers, bounded chunks and read-error handling.
//!
//! Fixtures are built in memory so no external imaging tool is required.

use std::fs;
use std::path::{Path, PathBuf};

use carvex::io::{open_image, InputFormat};

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("carvex_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn le16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}
fn le32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn le64(buf: &mut [u8], off: usize, v: u64) {
    buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

fn name16(s: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[..s.len().min(16)].copy_from_slice(&s[..s.len().min(16)]);
    out
}

/// Append a section descriptor with an explicit link to the next one.
fn section(buf: &mut Vec<u8>, name: &[u8; 16], link: u64, data: &[u8]) {
    let mut desc = [0u8; 76];
    desc[..16].copy_from_slice(name);
    le64(&mut desc, 16, link);
    le64(&mut desc, 24, data.len() as u64);
    buf.extend_from_slice(&desc);
    buf.extend_from_slice(data);
}

/// Build an E01 declaring a 64 GiB chunk (`sectors_per_chunk = 2^27`,
/// `bytes_per_sector = 512`) with one tiny compressed data chunk (H-11).
fn hostile_chunk_e01(dir: &Path) -> PathBuf {
    let path = dir.join("hostile.E01");
    let mut buf = Vec::new();
    buf.extend_from_slice(b"EVF\x09\x0d\x0a\xff\x00\x01\x00\x00\x00\x00");
    le16(&mut buf, 9, 1);
    le16(&mut buf, 11, 0);

    let off_volume = 13u64;
    let off_table = off_volume + 76 + 24;
    let off_done = off_table + 76 + 28;

    let mut vol = [0u8; 24];
    le32(&mut vol, 8, 1 << 27);
    le32(&mut vol, 12, 512);
    le64(&mut vol, 16, 1 << 27);
    section(&mut buf, &name16(b"volume"), off_table, &vol);

    let mut table = vec![0u8; 28];
    le32(&mut table, 0, 1);
    le64(&mut table, 8, 0);
    section(&mut buf, &name16(b"table"), off_done, &table);
    section(&mut buf, &name16(b"done"), 0, &[]);
    fs::write(&path, &buf).unwrap();
    path
}

/// H-11: a crafted E01 must be rejected with a typed error, not abort the
/// process with a giant allocation.
#[test]
fn hostile_chunk_size_is_rejected() {
    let dir = tmp_dir("h11");
    let path = hostile_chunk_e01(&dir);
    let start = std::time::Instant::now();
    let result = open_image(&path, InputFormat::E01);
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    match result {
        Ok(_) => panic!("64 GiB chunk must be rejected"),
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            assert!(
                msg.contains("chunk") || msg.contains("limit") || msg.contains("size"),
                "unexpected error: {e}"
            );
        }
    }
}

/// H-22: a partial final chunk must not cause "cannot decompress EWF chunk".
#[test]
fn partial_final_chunk_is_tolerated() {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let dir = tmp_dir("h22_e01");
    let path = dir.join("partial.E01");
    let chunk_size = 4096usize;
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&[0x41u8].repeat(chunk_size)).unwrap();
    let chunk1 = enc.finish().unwrap();
    // Deliberately short final chunk (only 100 of 4096 bytes).
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&[0x42u8].repeat(100)).unwrap();
    let chunk2 = enc.finish().unwrap();

    let head = 13u64;
    let off_volume = head;
    let c1_off = off_volume + 76 + 24;
    let c2_off = c1_off + chunk1.len() as u64;
    let off_table = c2_off + chunk2.len() as u64;
    let off_done = off_table + 76 + 32;

    let mut buf = Vec::new();
    buf.extend_from_slice(b"EVF\x09\x0d\x0a\xff\x00\x01\x00\x00\x00\x00");
    le16(&mut buf, 9, 1);
    le16(&mut buf, 11, 0);

    let mut vol = [0u8; 24];
    le32(&mut vol, 8, (chunk_size / 512) as u32);
    le32(&mut vol, 12, 512);
    le64(&mut vol, 16, (chunk_size * 2 / 512) as u64);
    section(&mut buf, &name16(b"volume"), off_table, &vol);
    buf.extend_from_slice(&chunk1);
    buf.extend_from_slice(&chunk2);

    let mut table = vec![0u8; 32];
    le32(&mut table, 0, 2);
    le64(&mut table, 8, 0);
    le32(&mut table, 24, 0x8000_0000 | (c1_off as u32));
    le32(&mut table, 28, 0x8000_0000 | (c2_off as u32));
    section(&mut buf, &name16(b"table"), off_done, &table);
    section(&mut buf, &name16(b"done"), 0, &[]);
    fs::write(&path, &buf).unwrap();

    let image = open_image(&path, InputFormat::E01).expect("open partial E01");
    let mut out = vec![0u8; 16];
    let n = image
        .source
        .read_at(chunk_size as u64 + 200, &mut out)
        .expect("partial chunk read");
    assert_eq!(n, 16);
    assert!(
        out.iter().all(|b| *b == 0),
        "short chunk tail must be zero-filled"
    );
}
