//! Shared fixtures for the integration tests.
//!
//! Everything is generated in memory or in a temp directory: the tests never
//! read the benchmark corpus and never write into a source image.

#![allow(dead_code)]

use carvex::io::ImageSource;
use carvex::scan::{AlignMode, ScanMode, ScanOptions};
use carvex::signatures::{FileType, PdfEofMode, ValidationMode};
use std::io;
use std::sync::Arc;

/// Read-only in-memory image source, independent from the crate's test-only
/// helper so that integration tests exercise only the public interface.
#[derive(Clone)]
pub struct MemSource {
    data: Arc<Vec<u8>>,
}

impl MemSource {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data: Arc::new(data),
        }
    }
}

impl ImageSource for MemSource {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if start >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        Ok(n)
    }
}

/// Options matching the historical CLI defaults, except for the fields the
/// caller needs to vary.
pub fn scan_options(types: Vec<FileType>, mode: ScanMode) -> ScanOptions {
    ScanOptions {
        types,
        mode,
        min_size: 256,
        max_size: 64 * 1024 * 1024,
        max_truncated_size: 16 * 1024 * 1024,
        max_memory: 1024 * 1024 * 1024,
        chunk_size: 64 * 1024,
        threads: 0,
        min_score: 50,
        repair: false,
        complete_only: false,
        pdf_eof: PdfEofMode::Smart,
        dedupe_hashes: false,
        suppress_nested: true,
        validate: ValidationMode::Structure,
        align: AlignMode::None,
        force: false,
        quiet: true,
        verbose: false,
        manifest: false,
        partition: "largest".to_string(),
        include_gaps: false,
        fragment_recovery: false,
        max_gap: carvex::scan::DEFAULT_MAX_GAP_CLUSTERS,
        max_frag_candidates: carvex::scan::DEFAULT_MAX_FRAG_CANDIDATES,
        frag_time_limit: std::time::Duration::from_millis(carvex::scan::DEFAULT_FRAG_TIME_LIMIT_MS),
    }
}

pub fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let len = u32::try_from(data.len()).unwrap();
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(kind);
    hasher.update(data);
    out.extend_from_slice(&hasher.finalize().to_be_bytes());
    out
}

/// A complete 1x1 RGB PNG with a valid zlib stream and CRCs.
pub fn png_1x1() -> Vec<u8> {
    let mut out = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));

    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write;
    encoder.write_all(&[0, 255, 0, 0]).unwrap();
    let idat = encoder.finish().unwrap();
    out.extend_from_slice(&chunk(b"IDAT", &idat));
    out.extend_from_slice(&chunk(b"IEND", b""));
    out
}

/// A small, fully decodable 1x1 baseline JPEG (generated once, embedded so the
/// test suite has no image-processing dependency).
#[rustfmt::skip]
pub const JPEG_1X1: &[u8] = &[
    0xff,0xd8,0xff,0xe0,0x00,0x10,0x4a,0x46,0x49,0x46,0x00,0x01,0x01,0x00,0x00,0x01,
    0x00,0x01,0x00,0x00,0xff,0xdb,0x00,0x43,0x00,0x03,0x02,0x02,0x03,0x02,0x02,0x03,
    0x03,0x03,0x03,0x04,0x03,0x03,0x04,0x05,0x08,0x05,0x05,0x04,0x04,0x05,0x0a,0x07,
    0x07,0x06,0x08,0x0c,0x0a,0x0c,0x0c,0x0b,0x0a,0x0b,0x0b,0x0d,0x0e,0x12,0x10,0x0d,
    0x0e,0x11,0x0e,0x0b,0x0b,0x10,0x16,0x10,0x11,0x13,0x14,0x15,0x15,0x15,0x0c,0x0f,
    0x17,0x18,0x16,0x14,0x18,0x12,0x14,0x15,0x14,0xff,0xdb,0x00,0x43,0x01,0x03,0x04,
    0x04,0x05,0x04,0x05,0x09,0x05,0x05,0x09,0x14,0x0d,0x0b,0x0d,0x14,0x14,0x14,0x14,
    0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,
    0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,
    0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0x14,0xff,0xc0,
    0x00,0x11,0x08,0x00,0x01,0x00,0x01,0x03,0x01,0x22,0x00,0x02,0x11,0x01,0x03,0x11,
    0x01,0xff,0xc4,0x00,0x1f,0x00,0x00,0x01,0x05,0x01,0x01,0x01,0x01,0x01,0x01,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x01,0x02,0x03,0x04,0x05,0x06,0x07,0x08,0x09,
    0x0a,0x0b,0xff,0xc4,0x00,0xb5,0x10,0x00,0x02,0x01,0x03,0x03,0x02,0x04,0x03,0x05,
    0x05,0x04,0x04,0x00,0x00,0x01,0x7d,0x01,0x02,0x03,0x00,0x04,0x11,0x05,0x12,0x21,
    0x31,0x41,0x06,0x13,0x51,0x61,0x07,0x22,0x71,0x14,0x32,0x81,0x91,0xa1,0x08,0x23,
    0x42,0xb1,0xc1,0x15,0x52,0xd1,0xf0,0x24,0x33,0x62,0x72,0x82,0x09,0x0a,0x16,0x17,
    0x18,0x19,0x1a,0x25,0x26,0x27,0x28,0x29,0x2a,0x34,0x35,0x36,0x37,0x38,0x39,0x3a,
    0x43,0x44,0x45,0x46,0x47,0x48,0x49,0x4a,0x53,0x54,0x55,0x56,0x57,0x58,0x59,0x5a,
    0x63,0x64,0x65,0x66,0x67,0x68,0x69,0x6a,0x73,0x74,0x75,0x76,0x77,0x78,0x79,0x7a,
    0x83,0x84,0x85,0x86,0x87,0x88,0x89,0x8a,0x92,0x93,0x94,0x95,0x96,0x97,0x98,0x99,
    0x9a,0xa2,0xa3,0xa4,0xa5,0xa6,0xa7,0xa8,0xa9,0xaa,0xb2,0xb3,0xb4,0xb5,0xb6,0xb7,
    0xb8,0xb9,0xba,0xc2,0xc3,0xc4,0xc5,0xc6,0xc7,0xc8,0xc9,0xca,0xd2,0xd3,0xd4,0xd5,
    0xd6,0xd7,0xd8,0xd9,0xda,0xe1,0xe2,0xe3,0xe4,0xe5,0xe6,0xe7,0xe8,0xe9,0xea,0xf1,
    0xf2,0xf3,0xf4,0xf5,0xf6,0xf7,0xf8,0xf9,0xfa,0xff,0xc4,0x00,0x1f,0x01,0x00,0x03,
    0x01,0x01,0x01,0x01,0x01,0x01,0x01,0x01,0x01,0x00,0x00,0x00,0x00,0x00,0x00,0x01,
    0x02,0x03,0x04,0x05,0x06,0x07,0x08,0x09,0x0a,0x0b,0xff,0xc4,0x00,0xb5,0x11,0x00,
    0x02,0x01,0x02,0x04,0x04,0x03,0x04,0x07,0x05,0x04,0x04,0x00,0x01,0x02,0x77,0x00,
    0x01,0x02,0x03,0x11,0x04,0x05,0x21,0x31,0x06,0x12,0x41,0x51,0x07,0x61,0x71,0x13,
    0x22,0x32,0x81,0x08,0x14,0x42,0x91,0xa1,0xb1,0xc1,0x09,0x23,0x33,0x52,0xf0,0x15,
    0x62,0x72,0xd1,0x0a,0x16,0x24,0x34,0xe1,0x25,0xf1,0x17,0x18,0x19,0x1a,0x26,0x27,
    0x28,0x29,0x2a,0x35,0x36,0x37,0x38,0x39,0x3a,0x43,0x44,0x45,0x46,0x47,0x48,0x49,
    0x4a,0x53,0x54,0x55,0x56,0x57,0x58,0x59,0x5a,0x63,0x64,0x65,0x66,0x67,0x68,0x69,
    0x6a,0x73,0x74,0x75,0x76,0x77,0x78,0x79,0x7a,0x82,0x83,0x84,0x85,0x86,0x87,0x88,
    0x89,0x8a,0x92,0x93,0x94,0x95,0x96,0x97,0x98,0x99,0x9a,0xa2,0xa3,0xa4,0xa5,0xa6,
    0xa7,0xa8,0xa9,0xaa,0xb2,0xb3,0xb4,0xb5,0xb6,0xb7,0xb8,0xb9,0xba,0xc2,0xc3,0xc4,
    0xc5,0xc6,0xc7,0xc8,0xc9,0xca,0xd2,0xd3,0xd4,0xd5,0xd6,0xd7,0xd8,0xd9,0xda,0xe2,
    0xe3,0xe4,0xe5,0xe6,0xe7,0xe8,0xe9,0xea,0xf2,0xf3,0xf4,0xf5,0xf6,0xf7,0xf8,0xf9,
    0xfa,0xff,0xda,0x00,0x0c,0x03,0x01,0x00,0x02,0x11,0x03,0x11,0x00,0x3f,0x00,0xf9,
    0xd2,0x8a,0x28,0xaf,0xc3,0x0f,0xf5,0x4c,0xff,0xd9,
];

/// A minimal PDF with a valid xref table, trailer `/Root`, and one revision.
pub fn pdf_min() -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let o1 = out.len();
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    let o2 = out.len();
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [1 0 R] /Count 0 >>\nendobj\n");
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 3\n0000000000 65535 f \n");
    out.extend_from_slice(format!("{o1:010} 00000 n \n{o2:010} 00000 n \n").as_bytes());
    out.extend_from_slice(b"trailer\n<< /Size 3 /Root 1 0 R >>\n");
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// Insert a comment segment so the SOS header ends exactly at `cluster`.
/// Splitting there leaves all entropy bytes in the tail fragment while the
/// head still carries a complete marker prefix.
pub fn jpeg_with_cluster_split(cluster: usize) -> Vec<u8> {
    let mut sos_end = None;
    let mut pos = 0usize;
    while pos + 4 <= JPEG_1X1.len() {
        if JPEG_1X1.get(pos) == Some(&0xFF) && JPEG_1X1.get(pos + 1) == Some(&0xDA) {
            let len_bytes = JPEG_1X1.get(pos + 2..pos + 4).unwrap();
            let len = u16::from_be_bytes(len_bytes.try_into().unwrap()) as usize;
            sos_end = Some(pos + 2 + len);
            break;
        }
        pos += 1;
    }
    let sos_end = sos_end.expect("JPEG fixture has an SOS segment");
    let payload_len = cluster
        .checked_sub(sos_end)
        .and_then(|v| v.checked_sub(4))
        .expect("cluster is large enough for the JPEG fixture");
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xFE];
    out.extend_from_slice(&((payload_len + 2) as u16).to_be_bytes());
    out.extend(std::iter::repeat_n(b'J', payload_len));
    out.extend_from_slice(&JPEG_1X1[2..]);
    assert!(out.len() > cluster);
    out
}

/// Insert a tEXt chunk so the IEND boundary crosses a cluster boundary after
/// the IDAT payload starts.  Both PNG fragments carry valid chunk bytes; only
/// their concatenation has valid CRCs and a complete zlib stream.
pub fn png_with_cluster_split(cluster: usize) -> Vec<u8> {
    let base = png_1x1();
    let payload_len = cluster
        .checked_sub(33)
        .and_then(|v| v.checked_sub(12))
        .and_then(|v| v.checked_sub(8))
        .and_then(|v| v.checked_sub(4))
        .expect("cluster is large enough for the PNG fixture");
    let mut text_chunk = b"Comment\0".to_vec();
    text_chunk.extend(std::iter::repeat_n(b'P', payload_len - 8));
    let mut out = vec![];
    out.extend_from_slice(&base[..33]); // signature + IHDR
    out.extend_from_slice(&chunk(b"tEXt", &text_chunk));
    out.extend_from_slice(&base[33..]); // IDAT + IEND
    assert!(out.len() > cluster + 16);
    out
}

/// Build a PDF whose object table starts after `cluster` bytes.  The xref
/// offsets are recalculated so the stitched file is byte-identical to the
/// original document.
pub fn pdf_with_cluster_split(cluster: usize) -> Vec<u8> {
    let header = b"%PDF-1.4\n";
    let comment_len = cluster + 16 - header.len();
    let mut out = header.to_vec();
    out.push(b'%');
    out.extend(std::iter::repeat_n(b'D', comment_len - 2));
    out.push(b'\n');
    let o1 = out.len();
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    let o2 = out.len();
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [1 0 R] /Count 0 >>\nendobj\n");
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 3\n0000000000 65535 f \n");
    out.extend_from_slice(format!("{o1:010} 00000 n \n{o2:010} 00000 n \n").as_bytes());
    out.extend_from_slice(b"trailer\n<< /Size 3 /Root 1 0 R >>\n");
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    assert!(out.len() > cluster + 32);
    out
}

/// Raw image containing a JPEG, a PNG and a PDF separated by NUL padding.
pub fn mixed_raw_image() -> Vec<u8> {
    let mut image = vec![0u8; 4096];
    image.extend_from_slice(JPEG_1X1);
    image.extend_from_slice(&vec![0u8; 1024]);
    image.extend_from_slice(&png_1x1());
    image.extend_from_slice(&vec![0u8; 1024]);
    image.extend_from_slice(&pdf_min());
    image.extend_from_slice(&vec![0u8; 4096]);
    image
}

/// A FAT32 volume with one allocated cluster and the rest free.
pub fn fat32_volume() -> Vec<u8> {
    fat32_volume_sectors(2048)
}

/// FAT32 volume of `total_sectors` 512-byte sectors.
pub fn fat32_volume_sectors(total_sectors: u32) -> Vec<u8> {
    fat32_volume_sectors_spc(total_sectors, 1)
}

/// FAT32 volume with a caller-selected sectors-per-cluster value.
pub fn fat32_volume_sectors_spc(total_sectors: u32, spc: u8) -> Vec<u8> {
    let bps: u16 = 512;
    let reserved: u16 = 1;
    let fats: u8 = 1;
    let fat_sectors: u32 = (u64::from(total_sectors) * 4 + 1024).div_ceil(512) as u32;

    let mut boot = vec![0u8; 512];
    boot[0..3].copy_from_slice(&[0xEB, 0x3C, 0x90]);
    boot[3..11].copy_from_slice(b"MSWIN4.1");
    boot[0x0B..0x0D].copy_from_slice(&bps.to_le_bytes());
    boot[0x0D] = spc;
    boot[0x0E..0x10].copy_from_slice(&reserved.to_le_bytes());
    boot[0x10] = fats;
    boot[0x13..0x15].copy_from_slice(&0u16.to_le_bytes());
    boot[0x15] = 0xF8;
    boot[0x16..0x18].copy_from_slice(&0u16.to_le_bytes());
    boot[0x20..0x24].copy_from_slice(&total_sectors.to_le_bytes());
    boot[0x24..0x28].copy_from_slice(&fat_sectors.to_le_bytes());
    boot[0x2C..0x30].copy_from_slice(&2u32.to_le_bytes());
    boot[0x47..0x52].copy_from_slice(b"CARVEX     ");
    boot[0x52..0x57].copy_from_slice(b"FAT32");
    boot[510..512].copy_from_slice(&[0x55, 0xAA]);

    let mut image = vec![0u8; usize::from(bps) * total_sectors as usize];
    image[0..512].copy_from_slice(&boot);

    // FAT entry 0/1 reserved; cluster 2 allocated, clusters 3+ free.
    let fat = usize::from(bps) * usize::from(reserved);
    image[fat..fat + 4].copy_from_slice(&0x0FFF_FFF8u32.to_le_bytes());
    image[fat + 4..fat + 8].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
    image[fat + 8..fat + 12].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
    image
}

/// MBR image with a partition at `start_lba`; the partition is a byte slice
/// that must fit in the returned image.
pub fn mbr_image(parts: &[(u64, u8, &[u8])]) -> Vec<u8> {
    let end = parts
        .iter()
        .map(|(lba, _, data)| (*lba as usize) * 512 + data.len())
        .max()
        .unwrap_or(512)
        .max(512);
    let mut image = vec![0u8; end];
    image[510..512].copy_from_slice(&[0x55, 0xAA]);
    for (i, (start_lba, type_id, data)) in parts.iter().enumerate() {
        let entry = 446 + i * 16;
        image[entry] = 0x00;
        image[entry + 4] = *type_id;
        let sectors = (data.len() as u64).div_ceil(512);
        image[entry + 8..entry + 12].copy_from_slice(&(*start_lba as u32).to_le_bytes());
        image[entry + 12..entry + 16].copy_from_slice(&(sectors as u32).to_le_bytes());
        let off = (*start_lba as usize) * 512;
        image[off..off + data.len()].copy_from_slice(data);
    }
    image
}

/// Byte offset of cluster 3 (the first free cluster) inside a FAT32 volume
/// built by [`fat32_volume_sectors`].
fn fat32_first_free_offset(total_sectors: u32) -> usize {
    fat32_first_free_offset_spc(total_sectors, 1)
}

fn fat32_first_free_offset_spc(total_sectors: u32, spc: u8) -> usize {
    let fat_sectors = (u64::from(total_sectors) * 4 + 1024).div_ceil(512) as u32;
    // Data start = (reserved 1 + 1 FAT * fat_sectors) sectors; cluster 2 is
    // allocated in the fixture, so the first free cluster is 3.
    let data_start = (1 + fat_sectors) as usize * 512;
    data_start + usize::from(spc) * 512
}

/// S12-style FAT32 image: `[A1][B1][A2][B2]` at 4 KiB cluster boundaries.
/// Each pair uses the same cluster split point, so only BGC can recover A and
/// B exactly; a header/footer splice mixes the two files.
pub fn fat32_interleaved(a: &[u8], b: &[u8], cluster: usize) -> Vec<u8> {
    const TOTAL_SECTORS: u32 = 2048;
    const SPC: u8 = 8;
    assert_eq!(cluster, usize::from(SPC) * 512);
    let mut image = fat32_volume_sectors_spc(TOTAL_SECTORS, SPC);
    let first = fat32_first_free_offset_spc(TOTAL_SECTORS, SPC);
    for (fragment_index, payload) in [a, b, a, b].into_iter().enumerate() {
        let split = if fragment_index < 2 {
            &payload[..cluster]
        } else {
            &payload[cluster..]
        };
        let offset = first + fragment_index * cluster;
        image[offset..offset + split.len()].copy_from_slice(split);
    }
    image
}

/// MBR image with a 1 MiB leading gap and two FAT32 partitions: a larger one
/// (type 0x0C) at LBA 2048 and a smaller one right after it.  Each volume
/// carries one payload planted in its free space (A-07 fixtures).
pub fn mbr_two_fat32(payload1: &[u8], payload2: &[u8]) -> Vec<u8> {
    const BIG_SECTORS: u32 = 4096;
    const SMALL_SECTORS: u32 = 2048;
    let mut big = fat32_volume_sectors(BIG_SECTORS);
    let mut small = fat32_volume_sectors(SMALL_SECTORS);
    let off1 = fat32_first_free_offset(BIG_SECTORS);
    let off2 = fat32_first_free_offset(SMALL_SECTORS);
    big[off1..off1 + payload1.len()].copy_from_slice(payload1);
    small[off2..off2 + payload2.len()].copy_from_slice(payload2);
    mbr_image(&[
        (2048, 0x0C, &big),
        (2048 + u64::from(BIG_SECTORS), 0x0C, &small),
    ])
}
