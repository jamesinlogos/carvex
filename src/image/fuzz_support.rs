//! Hidden fixture builders shared by fuzz targets, seed generation and
//! structured parser property tests (H-14).
//!
//! These helpers are deliberately public only so `cargo-fuzz` targets and the
//! seed generator can reuse the same container layouts as
//! `tests/evidence_formats.rs`. They are not a stable API and are not used by
//! the production CLI. Every size derived from fuzz input is bounded before an
//! allocation or a slice is taken.

#![allow(dead_code)]

use crate::io::ImageSource;
use flate2::{write::ZlibEncoder, Compression};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Largest fuzz payload wrapped by a generated container.
pub const MAX_FUZZ_PAYLOAD: usize = 1024 * 1024;
/// Largest payload accepted by the fixed-layout VHD builders.
const MAX_VHD_PAYLOAD: usize = 64 * 1024;

/// A generated file set and the file that a parser should open first.
pub struct FuzzFiles {
    pub primary: String,
    pub files: Vec<(String, Vec<u8>)>,
}

/// Simple bounded in-memory image source for fuzz targets and property tests.
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

fn fuzz_byte(data: &[u8], offset: usize) -> u8 {
    data.get(offset).copied().unwrap_or(0)
}

fn fuzz_u64(data: &[u8], offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = fuzz_byte(data, offset + i);
    }
    u64::from_le_bytes(bytes)
}

/// Copy at most `max` bytes starting at `offset`, padding the tail with zeros.
pub fn fuzz_bytes(data: &[u8], offset: usize, max: usize) -> Vec<u8> {
    let mut out = vec![0u8; max];
    let Some(tail) = data.get(offset..) else {
        return out;
    };
    let n = tail.len().min(max);
    out[..n].copy_from_slice(&tail[..n]);
    out
}

/// Clamp a fuzz-derived value into an inclusive range without wrapping.
pub fn bounded_u64(value: u64, min: u64, max: u64) -> u64 {
    if max <= min {
        min
    } else {
        value.clamp(min, max)
    }
}

/// Pad to a 512-byte boundary using zeros.
pub fn pad_to_sector(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    out.resize(data.len().div_ceil(512) * 512, 0);
    out
}

/// Adler-32 used by uncompressed EWF-E01 chunks.
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % MOD;
        b = (b + a) % MOD;
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

/// Build a complete EWF-E01 segment over `data` with the requested chunk size.
pub fn build_e01_with_chunk(data: &[u8], compressed: bool, chunk_size: usize) -> Vec<u8> {
    let chunk_size = chunk_size.clamp(512, 64 * 1024);
    let mut out = Vec::new();
    out.extend_from_slice(b"EVF\x09\x0d\x0a\xff\x00");
    out.push(1);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    let volume_desc = put_descriptor(&mut out, "volume");
    let mut volume = vec![0u8; 1052];
    volume[8..12].copy_from_slice(&((chunk_size / 512) as u32).to_le_bytes());
    volume[12..16].copy_from_slice(&512u32.to_le_bytes());
    volume[16..24].copy_from_slice(&((data.len() / 512) as u64).to_le_bytes());
    out.extend_from_slice(&volume);
    let next = out.len() as u64;
    patch_descriptor(&mut out, volume_desc, next, 1052);

    let sectors_desc = put_descriptor(&mut out, "sectors");
    let sectors_data = out.len();
    let mut chunk_offsets = Vec::new();
    for chunk in data.chunks(chunk_size) {
        let mut full = vec![0u8; chunk_size];
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

/// Build the standard 32 KiB-chunk E01 fixture used by the integration tests.
pub fn build_e01(data: &[u8], compressed: bool) -> Vec<u8> {
    build_e01_with_chunk(data, compressed, 32 * 1024)
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

/// Build a dynamic VHD with one allocated 64 KiB block.
pub fn build_vhd_dynamic(data: &[u8]) -> Vec<u8> {
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

/// Build a fixed VHD (raw data followed by the footer).
pub fn build_vhd_fixed(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    out.extend_from_slice(&vhd_footer(data.len() as u64, u64::MAX, 2));
    out
}

/// Build a dynamic VHD whose middle block is unallocated.
pub fn build_vhd_dynamic_with_hole() -> (Vec<u8>, Vec<u8>) {
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

/// Build a monolithicFlat VMDK descriptor and its flat extent.
pub fn build_vmdk_flat(data: &[u8]) -> (String, Vec<u8>) {
    let sectors = data.len() / 512;
    let descriptor = format!(
        "# Disk DescriptorFile\nversion=1\n# Extent description\nRW {sectors} FLAT \"disk-flat.vmdk\" 0\n"
    );
    (descriptor, data.to_vec())
}

/// Build a monolithicSparse VMDK with an embedded descriptor.
pub fn build_vmdk_sparse(data: &[u8]) -> Vec<u8> {
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

/// Build a monolithicSparse VMDK with one unallocated grain.
pub fn build_vmdk_sparse_with_hole() -> (Vec<u8>, Vec<u8>) {
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

/// Hostile X07 seed: one tiny compressed chunk behind a 64 GiB declared chunk.
/// The parser must reject the chunk size before any allocation (H-11).
pub fn hostile_e01_huge_chunk() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"EVF\x09\x0d\x0a\xff\x00");
    out.push(1);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    let volume_desc = put_descriptor(&mut out, "volume");
    let mut volume = vec![0u8; 1052];
    volume[8..12].copy_from_slice(&(1u32 << 27).to_le_bytes());
    volume[12..16].copy_from_slice(&512u32.to_le_bytes());
    volume[16..24].copy_from_slice(&1u64.to_le_bytes());
    out.extend_from_slice(&volume);
    let volume_next = out.len() as u64;
    patch_descriptor(&mut out, volume_desc, volume_next, 1052);

    let sectors_desc = put_descriptor(&mut out, "sectors");
    let sectors_data = out.len();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&[0u8; 512]).unwrap();
    let compressed = encoder.finish().unwrap();
    let chunk_offset = out.len() as u32;
    out.extend_from_slice(&compressed);
    let sectors_next = out.len() as u64;
    let sectors_size = sectors_next as usize - sectors_data;
    patch_descriptor(&mut out, sectors_desc, sectors_next, sectors_size as u64);

    let table_desc = put_descriptor(&mut out, "table");
    let table_data = out.len();
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 20]);
    out.extend_from_slice(&(chunk_offset | 0x8000_0000).to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    let table_next = out.len() as u64;
    let table_size = table_next as usize - table_data;
    patch_descriptor(&mut out, table_desc, table_next, table_size as u64);

    let done_desc = put_descriptor(&mut out, "done");
    patch_descriptor(&mut out, done_desc, done_desc as u64, 0);
    out
}

/// Hostile X08 seed: 128 grain-directory entries pointing at one grain table.
pub fn hostile_vmdk_x08() -> Vec<u8> {
    const GRAIN_SECTORS: u64 = 128;
    const GTE_PER_TABLE: usize = 512;
    const GD_ENTRIES: usize = 128;
    let capacity = (GD_ENTRIES as u64) * (GTE_PER_TABLE as u64) * GRAIN_SECTORS;

    let mut header = vec![0u8; 512];
    header[..4].copy_from_slice(b"KDMV");
    header[4..8].copy_from_slice(&1u32.to_le_bytes());
    header[8..12].copy_from_slice(&1u32.to_le_bytes());
    header[12..20].copy_from_slice(&capacity.to_le_bytes());
    header[20..28].copy_from_slice(&GRAIN_SECTORS.to_le_bytes());
    header[28..36].copy_from_slice(&0u64.to_le_bytes());
    header[36..44].copy_from_slice(&1u64.to_le_bytes());
    header[44..48].copy_from_slice(&(GTE_PER_TABLE as u32).to_le_bytes());
    header[48..56].copy_from_slice(&2u64.to_le_bytes());
    header[56..64].copy_from_slice(&2u64.to_le_bytes());
    header[64..72].copy_from_slice(&u64::MAX.to_le_bytes());
    header[72] = 1;

    let descriptor = format!(
        "# Disk DescriptorFile\nversion=1\nCID=fffffffe\nparentCID=ffffffff\ncreateType=\"monolithicSparse\"\n\n# Extent description\nRW {capacity} SPARSE \"x08.vmdk\"\n\n# The Disk Data Base\n#DDB\n\nddb.virtualHWVersion = \"4\"\n"
    );
    let table_bytes = GTE_PER_TABLE * 4;
    let mut out = vec![0u8; 3 * 512 + table_bytes];
    out[..512].copy_from_slice(&header);
    out[512..512 + descriptor.len()].copy_from_slice(descriptor.as_bytes());
    for i in 0..GD_ENTRIES {
        out[1024 + i * 4..1028 + i * 4].copy_from_slice(&3u32.to_le_bytes());
    }
    out
}

/// Hostile X09 seed: a descriptor that tries to follow an absolute extent.
pub fn hostile_vmdk_descriptor_x09() -> Vec<u8> {
    b"# Disk DescriptorFile\nversion=1\nCID=00000001\nparentCID=ffffffff\ncreateType=\"monolithicFlat\"\n\n# Extent description\nRW 1 FLAT \"/etc/passwd\" 0\n"
        .to_vec()
}

/// Deterministic mixed payload with JPEG, PNG and PDF signatures. It mirrors
/// the shape of `tests/common::mixed_raw_image` without depending on test-only
/// modules from the library.
pub fn mixed_payload() -> Vec<u8> {
    let mut out = vec![0u8; 4096];
    out.extend_from_slice(b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00");
    out.extend_from_slice(&[0x42; 768]);
    out.extend_from_slice(b"\xff\xd9");
    out.resize(out.len() + 1024, 0);
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    out.extend_from_slice(&[0x31; 512]);
    out.extend_from_slice(b"\x00\x00\x00\x00IEND\xaeB`\x82");
    out.resize(out.len() + 1024, 0);
    out.extend_from_slice(b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n%%EOF\n");
    out.resize(out.len() + 4096, 0);
    out.truncate(MAX_FUZZ_PAYLOAD);
    out
}

fn safe_token(data: &[u8], offset: usize, len: usize, fallback: &str) -> String {
    let mut out = String::new();
    for i in 0..len {
        let byte = fuzz_byte(data, offset + i);
        let ch = char::from(byte);
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        }
        if out.len() == 12 {
            break;
        }
    }
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

/// Derive a bounded E01 file set from fuzz bytes.
pub fn e01_fuzz(data: &[u8]) -> FuzzFiles {
    if data.starts_with(b"EVF\x09\x0d\x0a\xff\x00") {
        return FuzzFiles {
            primary: "seed.E01".to_string(),
            files: vec![("seed.E01".to_string(), data.to_vec())],
        };
    }
    let compressed = fuzz_byte(data, 0) & 1 == 1;
    let chunks = [512usize, 4096, 32 * 1024, 64 * 1024];
    let chunk_size = chunks[usize::from(fuzz_byte(data, 1)) % chunks.len()];
    let payload = pad_to_sector(&fuzz_bytes(data, 8, MAX_FUZZ_PAYLOAD));
    FuzzFiles {
        primary: "fuzz.E01".to_string(),
        files: vec![(
            "fuzz.E01".to_string(),
            build_e01_with_chunk(&payload, compressed, chunk_size),
        )],
    }
}

/// Derive a bounded VHD file set from fuzz bytes.
pub fn vhd_fuzz(data: &[u8]) -> FuzzFiles {
    let footer = data.len().checked_sub(512).and_then(|i| data.get(i..i + 8));
    if footer == Some(&b"conectix"[..]) {
        return FuzzFiles {
            primary: "seed.vhd".to_string(),
            files: vec![("seed.vhd".to_string(), data.to_vec())],
        };
    }
    let payload = fuzz_bytes(data, 8, MAX_VHD_PAYLOAD);
    let bytes = match fuzz_byte(data, 0) % 3 {
        0 => build_vhd_fixed(&payload),
        1 => build_vhd_dynamic(&payload),
        _ => build_vhd_dynamic_with_hole().0,
    };
    FuzzFiles {
        primary: "fuzz.vhd".to_string(),
        files: vec![("fuzz.vhd".to_string(), bytes)],
    }
}

/// Derive a bounded VMDK file set from fuzz bytes.
pub fn vmdk_fuzz(data: &[u8]) -> FuzzFiles {
    let is_descriptor = data.windows(20).any(|w| w == b"# Extent description");
    if data.starts_with(b"KDMV") || is_descriptor {
        return FuzzFiles {
            primary: "seed.vmdk".to_string(),
            files: vec![("seed.vmdk".to_string(), data.to_vec())],
        };
    }
    let payload = pad_to_sector(&fuzz_bytes(data, 8, MAX_FUZZ_PAYLOAD));
    let variant = fuzz_byte(data, 0) % 4;
    if variant <= 1 {
        let (descriptor, flat) = build_vmdk_flat(&payload);
        FuzzFiles {
            primary: "fuzz.vmdk".to_string(),
            files: vec![
                ("fuzz.vmdk".to_string(), descriptor.into_bytes()),
                ("disk-flat.vmdk".to_string(), flat),
            ],
        }
    } else if variant == 2 {
        FuzzFiles {
            primary: "fuzz.vmdk".to_string(),
            files: vec![("fuzz.vmdk".to_string(), build_vmdk_sparse(&payload))],
        }
    } else {
        let (sparse, _) = build_vmdk_sparse_with_hole();
        FuzzFiles {
            primary: "fuzz.vmdk".to_string(),
            files: vec![("fuzz.vmdk".to_string(), sparse)],
        }
    }
}

/// Derive split-raw segment names and contents from fuzz bytes. All names stay
/// inside the temporary directory and never contain path separators.
pub fn split_names_fuzz(data: &[u8]) -> FuzzFiles {
    let base = safe_token(data, 0, 12, "fuzz");
    let content = fuzz_bytes(data, 16, 64 * 1024);
    let extensions = [
        "001",
        "002",
        "1",
        "0001",
        "abc",
        "-1",
        "99999999999999999999",
    ];
    let mut files = Vec::with_capacity(extensions.len());
    for (i, ext) in extensions.iter().enumerate() {
        let name = format!("{base}.{ext}");
        let start = (i * 97) % content.len().max(1);
        let end = (start + 512).min(content.len());
        files.push((name, content[start..end].to_vec()));
    }
    let primary = files[usize::from(fuzz_byte(data, 12)) % files.len()]
        .0
        .clone();
    FuzzFiles { primary, files }
}

/// Derive a file name extension and raw bytes for the dispatcher target.
pub fn open_image_fuzz(data: &[u8]) -> FuzzFiles {
    const EXTS: [&str; 7] = ["img", "E01", "vhd", "vmdk", "001", "Ex01", "L01"];
    let footer = data.len().checked_sub(512).and_then(|i| data.get(i..i + 8));
    let ext = if data.starts_with(b"EVF\x09\x0d\x0a\xff\x00") {
        "E01"
    } else if data.starts_with(b"KDMV") || data.windows(20).any(|w| w == b"# Extent description") {
        "vmdk"
    } else if footer == Some(&b"conectix"[..]) {
        "vhd"
    } else {
        EXTS[usize::from(fuzz_byte(data, 0)) % EXTS.len()]
    };
    let name = format!("image.{ext}");
    FuzzFiles {
        primary: name.clone(),
        files: vec![(name, fuzz_bytes(data, 1, MAX_FUZZ_PAYLOAD))],
    }
}

/// Bound a fragment-join payload while keeping the header bytes that select
/// offsets available to the hidden scan wrapper.
pub fn fragment_fuzz(data: &[u8]) -> Vec<u8> {
    fuzz_bytes(data, 0, MAX_FUZZ_PAYLOAD)
}

/// Write a generated file set into a fresh temporary directory.
pub fn write_temp_files(files: &FuzzFiles, prefix: &str) -> io::Result<tempfile::TempDir> {
    let dir = tempfile::Builder::new().prefix(prefix).tempdir()?;
    for (name, bytes) in &files.files {
        let path = Path::new(name);
        if path.file_name().and_then(|n| n.to_str()) != Some(name.as_str()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fuzz file name contains a path separator",
            ));
        }
        std::fs::write(dir.path().join(path), bytes)?;
    }
    Ok(dir)
}

/// Read a few bounded slices from every parsed source. Reading the whole
/// logical length would let a hostile size field drive an unbounded loop.
pub fn exercise_source(source: &dyn ImageSource, data: &[u8]) {
    let len = source.len();
    for i in 0..8usize {
        let offset = if len == 0 {
            0
        } else {
            bounded_u64(fuzz_u64(data, i * 8), 0, len - 1)
        };
        let mut buf = [0u8; 4096];
        let _ = source.read_at(offset, &mut buf);
    }
}

/// Build an owned path for diagnostics without relying on the caller's cwd.
pub fn primary_path(dir: &Path, files: &FuzzFiles) -> PathBuf {
    dir.join(&files.primary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{
        open_image_with_options, E01Image, InputFormat, OnReadError, OpenOptions, RegionLog,
        SplitRaw, VhdImage, VmdkImage,
    };
    use proptest::prelude::*;
    use std::sync::Mutex;

    fn open_opts() -> OpenOptions {
        OpenOptions {
            allow_external_extents: false,
            on_read_error: OnReadError::ZeroFill,
            max_stdin_bytes: 0,
            spool_dir: None,
        }
    }

    fn regions() -> RegionLog {
        Arc::new(Mutex::new(Vec::new()))
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]

        #[test]
        fn e01_builder_is_bounded_and_parses(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let files = e01_fuzz(&data);
            let dir = write_temp_files(&files, "carvex-e01-test").unwrap();
            let path = primary_path(dir.path(), &files);
            let image = E01Image::open_with(&path, OnReadError::ZeroFill, regions()).unwrap();
            prop_assert!(image.len() <= MAX_FUZZ_PAYLOAD as u64 + 512);
            exercise_source(&image, &data);
        }

        #[test]
        fn vhd_builders_are_bounded_and_parse(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let files = vhd_fuzz(&data);
            let dir = write_temp_files(&files, "carvex-vhd-test").unwrap();
            let path = primary_path(dir.path(), &files);
            let image = VhdImage::open(&path).unwrap();
            prop_assert!(image.len() <= MAX_VHD_PAYLOAD as u64 * 3);
            exercise_source(&image, &data);
        }

        #[test]
        fn vmdk_builders_are_bounded_and_parse(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let files = vmdk_fuzz(&data);
            let dir = write_temp_files(&files, "carvex-vmdk-test").unwrap();
            let path = primary_path(dir.path(), &files);
            let image = VmdkImage::open(&path).unwrap();
            prop_assert!(image.len() <= MAX_FUZZ_PAYLOAD as u64 + 512);
            exercise_source(&image, &data);
        }

        #[test]
        fn split_names_stay_inside_the_temp_directory(data in proptest::collection::vec(any::<u8>(), 0..512)) {
            let files = split_names_fuzz(&data);
            let dir = write_temp_files(&files, "carvex-split-test").unwrap();
            for (name, _) in &files.files {
                prop_assert_eq!(Path::new(name).file_name().and_then(|n| n.to_str()), Some(name.as_str()));
            }
            // Opening an odd extension is a valid negative case; use a known
            // numeric segment to assert the successful discovery path here.
            let path = dir.path().join(format!("{base}.001", base = safe_token(&data, 0, 12, "fuzz")));
            let image = SplitRaw::open(&path).unwrap();
            prop_assert!(image.len() <= 64 * 1024 * 8);
            exercise_source(&image, &data);
        }

        #[test]
        fn open_image_dispatch_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let files = open_image_fuzz(&data);
            let dir = write_temp_files(&files, "carvex-open-test").unwrap();
            let path = primary_path(dir.path(), &files);
            if let Ok(opened) = open_image_with_options(&path, InputFormat::Auto, &open_opts()) {
                exercise_source(opened.source.as_ref(), &data);
            }
        }
    }
}
