//! Validated PNG/JPEG crop repair for `--repair` (external harness H-04).
//!
//! A cut through the payload cannot be repaired by appending a footer: a
//! decoder would fill the missing pixels with invented data.  For non-interlaced
//! PNG and baseline sequential JPEG this module rebuilds a *shorter* file that
//! contains only complete scanlines / MCU rows.  Every emitted pixel is a pixel
//! of the original prefix; the caller still requires a deep decode proof before
//! the rebuilt bytes are written.

#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]

use super::ValidationLimits;
use crc32fast::Hasher;
use flate2::{
    write::ZlibEncoder, Compression, Decompress, FlushDecompress, Status as InflateStatus,
};
use std::collections::HashMap;
use std::io::Write;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const PNG_INFLATE_BUFFER: usize = 64 * 1024;
const MAX_PNG_CHUNK_BYTES: usize = 64 * 1024 * 1024;
const MAX_JPEG_DHT_TABLES: usize = 32;
const MAX_JPEG_MCUS: usize = 8_000_000;

/// A PNG or JPEG crop repair: recovered row count, header row count and the
/// complete rebuilt output.
#[derive(Debug)]
pub(crate) struct CroppedRepair {
    pub output: Vec<u8>,
    pub recovered_rows: u32,
    pub total_rows: u32,
}

fn range(data: &[u8], start: usize, end: usize) -> Option<&[u8]> {
    if end < start {
        return None;
    }
    data.get(start..end)
}

fn u16be(data: &[u8], at: usize) -> Option<u16> {
    let bytes = range(data, at, at.checked_add(2)?)?;
    Some(u16::from_be_bytes([*bytes.first()?, *bytes.get(1)?]))
}

fn u32be(data: &[u8], at: usize) -> Option<u32> {
    let bytes = range(data, at, at.checked_add(4)?)?;
    Some(u32::from_be_bytes([
        *bytes.first()?,
        *bytes.get(1)?,
        *bytes.get(2)?,
        *bytes.get(3)?,
    ]))
}

fn reserve(out: &mut Vec<u8>, additional: usize) -> Result<(), String> {
    let needed = out
        .len()
        .checked_add(additional)
        .ok_or_else(|| "repaired image length is not addressable".to_string())?;
    out.try_reserve_exact(needed.saturating_sub(out.len()))
        .map_err(|_| "cannot reserve repaired image memory".to_string())
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) -> Result<(), String> {
    let len = u32::try_from(body.len()).map_err(|_| "PNG repair chunk is too large".to_string())?;
    let total = body
        .len()
        .checked_add(12)
        .ok_or_else(|| "PNG repair chunk length overflow".to_string())?;
    reserve(out, total)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut crc = Hasher::new();
    crc.update(kind);
    crc.update(body);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
    Ok(())
}

/// Crop a non-interlaced PNG to the scanlines present in the carved prefix.
pub(crate) fn crop_png(data: &[u8], limits: ValidationLimits) -> Result<CroppedRepair, String> {
    if !data.starts_with(PNG_SIGNATURE) {
        return Err("not a PNG".into());
    }
    let mut pos = PNG_SIGNATURE.len();
    let mut ihdr: Option<(u32, u32, u8, u8, u8)> = None;
    let mut before_idat: Vec<Vec<u8>> = Vec::new();
    let mut idat = Vec::new();
    while pos
        .checked_add(8)
        .ok_or_else(|| "invalid repair structure".to_string())?
        <= data.len()
    {
        let declared = usize::try_from(
            u32be(data, pos).ok_or_else(|| "invalid repair structure".to_string())?,
        )
        .map_err(|_| "PNG chunk length is not addressable".to_string())?;
        if declared > MAX_PNG_CHUNK_BYTES {
            return Err("PNG chunk exceeds the repair size limit".into());
        }
        let kind_at = pos
            .checked_add(4)
            .ok_or_else(|| "invalid repair structure".to_string())?;
        let body_at = pos
            .checked_add(8)
            .ok_or_else(|| "invalid repair structure".to_string())?;
        let body_end = body_at
            .checked_add(declared)
            .ok_or_else(|| "invalid repair structure".to_string())?;
        let chunk_end = body_end
            .checked_add(4)
            .ok_or_else(|| "invalid repair structure".to_string())?;
        let kind = range(data, kind_at, body_at)
            .ok_or_else(|| "PNG chunk type is truncated".to_string())?;
        let body = range(data, body_at, body_end.min(data.len()))
            .ok_or_else(|| "PNG chunk body is truncated".to_string())?;
        let complete = chunk_end <= data.len();

        if kind == b"IHDR" {
            if !complete {
                return Err("PNG IHDR is cut".into());
            }
            let width = u32be(body, 0).ok_or_else(|| "invalid repair structure".to_string())?;
            let height = u32be(body, 4).ok_or_else(|| "invalid repair structure".to_string())?;
            let depth = *body
                .get(8)
                .ok_or_else(|| "invalid repair structure".to_string())?;
            let colour = *body
                .get(9)
                .ok_or_else(|| "invalid repair structure".to_string())?;
            let interlace = *body
                .get(12)
                .ok_or_else(|| "invalid repair structure".to_string())?;
            if interlace != 0 {
                return Err("interlaced PNG is not repairable".into());
            }
            ihdr = Some((width, height, depth, colour, interlace));
        } else if kind == b"IDAT" {
            idat.try_reserve_exact(body.len())
                .map_err(|_| "cannot reserve PNG IDAT repair memory".to_string())?;
            idat.extend_from_slice(body);
            if idat.len() as u64 > limits.max_validator_memory {
                return Err("PNG IDAT exceeds the validator budget".into());
            }
        } else if kind == b"IEND" {
            break;
        } else if idat.is_empty() && complete {
            before_idat.push(
                range(data, pos, chunk_end)
                    .ok_or_else(|| "PNG ancillary chunk is truncated".to_string())?
                    .to_vec(),
            );
        }
        if !complete {
            break;
        }
        pos = chunk_end;
    }
    let (width, height, depth, colour, interlace) =
        ihdr.ok_or_else(|| "no PNG IHDR in the readable prefix".to_string())?;
    let channels = match colour {
        0 => 1u64,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err("unsupported PNG colour type".into()),
    };
    let pixels = (width as u64)
        .checked_mul(channels)
        .and_then(|v| v.checked_mul(depth as u64))
        .ok_or_else(|| "PNG row size overflow".to_string())?;
    let rowbytes = usize::try_from(
        pixels
            .checked_add(7)
            .ok_or_else(|| "invalid repair structure".to_string())?
            / 8,
    )
    .map_err(|_| "PNG row size is not addressable".to_string())?
    .checked_add(1)
    .ok_or_else(|| "PNG row size overflow".to_string())?;
    let expected = (rowbytes as u64)
        .checked_mul(height as u64)
        .ok_or_else(|| "PNG image size overflow".to_string())?;
    if expected > limits.max_validator_memory {
        return Err("PNG repair exceeds the decode-validation budget".into());
    }

    let mut raw = Vec::new();
    raw.try_reserve_exact(usize::try_from(expected).unwrap_or(usize::MAX))
        .map_err(|_| "cannot reserve PNG repair memory".to_string())?;
    let mut inflater = Decompress::new(true);
    let mut scratch = vec![0u8; PNG_INFLATE_BUFFER];
    let mut input: &[u8] = &idat;
    loop {
        let before_in = inflater.total_in();
        let before_out = inflater.total_out();
        let status = inflater
            .decompress(input, &mut scratch, FlushDecompress::None)
            .map_err(|e| format!("PNG IDAT inflate failed: {e}"))?;
        let consumed =
            usize::try_from(inflater.total_in().saturating_sub(before_in)).unwrap_or(usize::MAX);
        let written =
            usize::try_from(inflater.total_out().saturating_sub(before_out)).unwrap_or(usize::MAX);
        let produced = range(
            &scratch,
            0,
            written.min(usize::try_from(expected).unwrap_or(0)),
        )
        .ok_or_else(|| "PNG inflate produced an invalid span".to_string())?;
        raw.try_reserve_exact(produced.len())
            .map_err(|_| "cannot reserve PNG repair memory".to_string())?;
        raw.extend_from_slice(produced);
        if status == InflateStatus::StreamEnd || (consumed == 0 && written == 0) {
            break;
        }
        input = input.get(consumed..).unwrap_or(&[]);
    }
    let complete_rows = raw
        .len()
        .checked_div(rowbytes)
        .ok_or_else(|| "PNG row size is zero".to_string())?;
    let rows = complete_rows.min(height as usize);
    if rows == 0 {
        return Err("no complete PNG scanline in the carved prefix".into());
    }
    let keep = rows
        .checked_mul(rowbytes)
        .ok_or_else(|| "PNG repaired size overflow".to_string())?;
    let raw_prefix =
        range(&raw, 0, keep).ok_or_else(|| "PNG decoded rows are short".to_string())?;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(raw_prefix)
        .map_err(|e| format!("PNG repair re-deflate failed: {e}"))?;
    let compressed = encoder
        .finish()
        .map_err(|e| format!("PNG repair re-deflate failed: {e}"))?;

    let mut out = Vec::new();
    reserve(&mut out, PNG_SIGNATURE.len())?;
    out.extend_from_slice(PNG_SIGNATURE);
    let mut repaired_ihdr = Vec::with_capacity(13);
    repaired_ihdr.extend_from_slice(&width.to_be_bytes());
    repaired_ihdr.extend_from_slice(&(rows as u32).to_be_bytes());
    repaired_ihdr.extend_from_slice(&[depth, colour, 0, 0, interlace]);
    png_chunk(&mut out, b"IHDR", &repaired_ihdr)?;
    for chunk in &before_idat {
        reserve(&mut out, chunk.len())?;
        out.extend_from_slice(chunk);
    }
    png_chunk(&mut out, b"IDAT", &compressed)?;
    png_chunk(&mut out, b"IEND", &[])?;
    if out.len() as u64 > limits.max_validator_memory {
        return Err("repaired PNG exceeds the validator budget".into());
    }
    Ok(CroppedRepair {
        output: out,
        recovered_rows: rows as u32,
        total_rows: height,
    })
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit: 0 }
    }

    fn read_bit(&mut self) -> Option<u8> {
        let bit = *self.bytes.get(self.bit)?;
        self.bit = self.bit.checked_add(1)?;
        Some(bit)
    }

    fn skip(&mut self, count: u8) -> Option<()> {
        let end = self.bit.checked_add(count as usize)?;
        if end > self.bytes.len().checked_mul(8)? {
            return None;
        }
        self.bit = end;
        Some(())
    }
}

type HuffmanTable = HashMap<(u8, u16), u8>;

fn decode_symbol(bits: &mut BitReader<'_>, table: &HuffmanTable) -> Option<u8> {
    let mut code = 0u16;
    for length in 1u8..=16 {
        let bit = bits.read_bit()? as u16;
        code = code.checked_shl(1)?.checked_add(bit)?;
        if let Some(symbol) = table.get(&(length, code)) {
            return Some(*symbol);
        }
    }
    None
}

fn parse_dht(body: &[u8], tables: &mut Vec<Option<HuffmanTable>>) -> Option<()> {
    let mut pos = 0usize;
    while pos < body.len() {
        let selector = *body.get(pos)? as usize;
        if selector >= MAX_JPEG_DHT_TABLES {
            return None;
        }
        let counts = range(body, pos.checked_add(1)?, pos.checked_add(17)?)?;
        let total = counts
            .iter()
            .try_fold(0usize, |sum, count| sum.checked_add(*count as usize))?;
        let symbols = range(
            body,
            pos.checked_add(17)?,
            pos.checked_add(17)?.checked_add(total)?,
        )?;
        let mut table = HuffmanTable::new();
        let mut code = 0u16;
        let mut symbol_pos = 0usize;
        for (index, count) in counts.iter().enumerate() {
            let length = (index as u8).checked_add(1)?;
            for _ in 0..*count {
                let symbol = *symbols.get(symbol_pos)?;
                symbol_pos = symbol_pos.checked_add(1)?;
                table.insert((length, code), symbol);
                code = code.checked_add(1)?;
            }
            code = code.checked_shl(1)?;
        }
        while tables.len() <= selector {
            tables.push(None);
        }
        *tables.get_mut(selector)? = Some(table);
        pos = pos.checked_add(17)?.checked_add(total)?;
    }
    Some(())
}

/// Crop baseline sequential JPEG entropy data to complete MCU rows.
pub(crate) fn crop_jpeg(data: &[u8], limits: ValidationLimits) -> Result<CroppedRepair, String> {
    let mut pos = 2usize;
    let mut tables: Vec<Option<HuffmanTable>> = Vec::new();
    let mut sof: Option<(usize, Vec<u8>)> = None;
    let mut sos: Option<Vec<u8>> = None;
    while pos
        .checked_add(4)
        .ok_or_else(|| "invalid repair structure".to_string())?
        <= data.len()
        && data.get(pos) == Some(&0xFF)
    {
        let marker = *data
            .get(
                pos.checked_add(1)
                    .ok_or_else(|| "invalid repair structure".to_string())?,
            )
            .ok_or_else(|| "JPEG marker is truncated".to_string())?;
        let length = u16be(
            data,
            pos.checked_add(2)
                .ok_or_else(|| "invalid repair structure".to_string())?,
        )
        .ok_or_else(|| "invalid repair structure".to_string())? as usize;
        if length < 2 {
            return Err("JPEG segment length is invalid".into());
        }
        let body_at = pos
            .checked_add(4)
            .ok_or_else(|| "invalid repair structure".to_string())?;
        let body_end = body_at
            .checked_add(
                length
                    .checked_sub(2)
                    .ok_or_else(|| "invalid repair structure".to_string())?,
            )
            .ok_or_else(|| "JPEG segment length overflow".to_string())?;
        if body_end > data.len() {
            break;
        }
        let body = range(data, body_at, body_end)
            .ok_or_else(|| "JPEG segment body is truncated".to_string())?;
        match marker {
            0xC0 => sof = Some((pos, body.to_vec())),
            0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF => {
                return Err("non-baseline JPEG is not repairable".into())
            }
            0xDD => return Err("JPEG restart intervals are not repairable".into()),
            0xC4 => {
                parse_dht(body, &mut tables)
                    .ok_or_else(|| "JPEG Huffman table is malformed".to_string())?;
            }
            0xDA => {
                sos = Some(body.to_vec());
                pos = pos
                    .checked_add(length)
                    .ok_or_else(|| "invalid repair structure".to_string())?
                    .checked_add(2)
                    .ok_or_else(|| "invalid repair structure".to_string())?;
                break;
            }
            _ => {}
        }
        pos = pos
            .checked_add(length)
            .ok_or_else(|| "invalid repair structure".to_string())?
            .checked_add(2)
            .ok_or_else(|| "invalid repair structure".to_string())?;
    }
    let (sof_pos, sof_body) =
        sof.ok_or_else(|| "no baseline SOF in the readable JPEG prefix".to_string())?;
    let sos_body = sos.ok_or_else(|| "no SOS in the readable JPEG prefix".to_string())?;

    let height = u16be(&sof_body, 1).ok_or_else(|| "invalid repair structure".to_string())? as u32;
    let width = u16be(&sof_body, 3).ok_or_else(|| "invalid repair structure".to_string())? as u32;
    let components = *sof_body
        .get(5)
        .ok_or_else(|| "JPEG SOF component count is missing".to_string())?
        as usize;
    if components == 0 {
        return Err("JPEG SOF has no components".into());
    }
    let comp_end = 6usize
        .checked_add(
            components
                .checked_mul(3)
                .ok_or_else(|| "invalid repair structure".to_string())?,
        )
        .ok_or_else(|| "JPEG SOF component table overflow".to_string())?;
    if sof_body.len() < comp_end {
        return Err("JPEG SOF component table is truncated".into());
    }
    let mut sampling = HashMap::new();
    for index in 0..components {
        let at = 6usize
            .checked_add(
                index
                    .checked_mul(3)
                    .ok_or_else(|| "invalid repair structure".to_string())?,
            )
            .ok_or_else(|| "JPEG component index overflow".to_string())?;
        let id = *sof_body
            .get(at)
            .ok_or_else(|| "invalid repair structure".to_string())?;
        let hv = *sof_body
            .get(
                at.checked_add(1)
                    .ok_or_else(|| "invalid repair structure".to_string())?,
            )
            .ok_or_else(|| "invalid repair structure".to_string())?;
        let h = hv
            .checked_shr(4)
            .ok_or_else(|| "invalid repair structure".to_string())? as u32;
        let v = (hv & 0x0F) as u32;
        if h == 0 || v == 0 {
            return Err("JPEG component sampling is invalid".into());
        }
        sampling.insert(id, (h, v));
    }

    let scan_count = *sos_body
        .first()
        .ok_or_else(|| "invalid repair structure".to_string())? as usize;
    let scan_end = 1usize
        .checked_add(
            scan_count
                .checked_mul(2)
                .ok_or_else(|| "invalid repair structure".to_string())?,
        )
        .and_then(|v| v.checked_add(3))
        .ok_or_else(|| "JPEG SOS length overflow".to_string())?;
    if sos_body.len() < scan_end {
        return Err("JPEG SOS component table is truncated".into());
    }
    let mut scan = Vec::with_capacity(scan_count);
    for index in 0..scan_count {
        let at = 1usize
            .checked_add(
                index
                    .checked_mul(2)
                    .ok_or_else(|| "invalid repair structure".to_string())?,
            )
            .ok_or_else(|| "JPEG scan index overflow".to_string())?;
        let id = *sos_body
            .get(at)
            .ok_or_else(|| "invalid repair structure".to_string())?;
        let hv = *sos_body
            .get(
                at.checked_add(1)
                    .ok_or_else(|| "invalid repair structure".to_string())?,
            )
            .ok_or_else(|| "invalid repair structure".to_string())?;
        scan.push((
            id,
            hv.checked_shr(4)
                .ok_or_else(|| "invalid repair structure".to_string())? as usize,
            (hv & 0x0F) as usize,
        ));
    }
    if scan.len() != components {
        return Err("non-interleaved JPEG scan is not repairable".into());
    }
    let mut hmax = 0u32;
    let mut vmax = 0u32;
    for &(h, v) in sampling.values() {
        hmax = hmax.max(h);
        vmax = vmax.max(v);
    }
    if hmax == 0 || vmax == 0 {
        return Err("JPEG sampling maximum is invalid".into());
    }
    let mcux = width
        .checked_add(
            8u32.checked_mul(hmax)
                .ok_or_else(|| "invalid repair structure".to_string())?
                .checked_sub(1)
                .ok_or_else(|| "invalid repair structure".to_string())?,
        )
        .and_then(|v| v.checked_div(8u32.checked_mul(hmax)?))
        .ok_or_else(|| "JPEG MCU width overflow".to_string())?;
    let mcuy = height
        .checked_add(
            8u32.checked_mul(vmax)
                .ok_or_else(|| "invalid repair structure".to_string())?
                .checked_sub(1)
                .ok_or_else(|| "invalid repair structure".to_string())?,
        )
        .and_then(|v| v.checked_div(8u32.checked_mul(vmax)?))
        .ok_or_else(|| "JPEG MCU height overflow".to_string())?;
    let mcu_count = (mcux as usize)
        .checked_mul(mcuy as usize)
        .and_then(|v| v.checked_mul(scan.len()))
        .ok_or_else(|| "JPEG MCU count overflow".to_string())?;
    if mcu_count > MAX_JPEG_MCUS {
        return Err("JPEG repair MCU count exceeds the limit".into());
    }

    // Un-stuff the entropy stream while retaining, for every emitted bit, the
    // offset of the original byte that owns it.  The reference algorithm uses
    // that map to trim at an MCU boundary even when `FF 00` stuffing is present.
    let mut entropy = Vec::new();
    let mut owners = Vec::new();
    let mut raw_ends = Vec::new();
    entropy
        .try_reserve_exact(data.len().saturating_sub(pos))
        .map_err(|_| "cannot reserve JPEG repair memory".to_string())?;
    let mut at = pos;
    while at < data.len() {
        let byte = *data
            .get(at)
            .ok_or_else(|| "invalid repair structure".to_string())?;
        if byte == 0xFF {
            if data.get(
                at.checked_add(1)
                    .ok_or_else(|| "invalid repair structure".to_string())?,
            ) == Some(&0)
            {
                at = at
                    .checked_add(2)
                    .ok_or_else(|| "invalid repair structure".to_string())?;
            } else {
                break;
            }
        } else {
            at = at
                .checked_add(1)
                .ok_or_else(|| "invalid repair structure".to_string())?;
        }
        let owner = raw_ends.len();
        for shift in (0..8u32).rev() {
            owners.push(owner);
            entropy.push((byte >> shift) & 1);
        }
        raw_ends.push(at);
    }
    if entropy.is_empty() {
        return Err("no JPEG entropy data in the carved prefix".into());
    }
    let mut bits = BitReader::new(&entropy);
    let mut rows = 0usize;
    let mut end_bit = 0usize;
    'mcus: for _ in 0..mcuy {
        for _ in 0..mcux {
            for (component, dc, ac) in &scan {
                let (h, v) = *sampling
                    .get(component)
                    .ok_or_else(|| "JPEG scan references an unknown component".to_string())?;
                for _ in 0..h
                    .checked_mul(v)
                    .ok_or_else(|| "invalid repair structure".to_string())?
                {
                    let dc_table = tables
                        .get(*dc)
                        .and_then(Option::as_ref)
                        .ok_or_else(|| "JPEG DC table is missing".to_string())?;
                    let size = decode_symbol(&mut bits, dc_table)
                        .ok_or_else(|| "JPEG entropy data ends before a DC symbol".to_string())?;
                    if size > 0 && bits.skip(size).is_none() {
                        break 'mcus;
                    }
                    let ac_table = tables
                        .get(
                            ac.checked_add(16)
                                .ok_or_else(|| "invalid repair structure".to_string())?,
                        )
                        .and_then(Option::as_ref)
                        .ok_or_else(|| "JPEG AC table is missing".to_string())?;
                    let mut k = 1usize;
                    while k < 64 {
                        let Some(symbol) = decode_symbol(&mut bits, ac_table) else {
                            break 'mcus;
                        };
                        let run = symbol
                            .checked_shr(4)
                            .ok_or_else(|| "invalid repair structure".to_string())?;
                        let extra = symbol & 0x0F;
                        if extra == 0 {
                            if run == 15 {
                                k = k
                                    .checked_add(16)
                                    .ok_or_else(|| "invalid repair structure".to_string())?;
                                continue;
                            }
                            break;
                        }
                        if bits.skip(extra).is_none() {
                            break 'mcus;
                        }
                        k = k
                            .checked_add(run as usize)
                            .ok_or_else(|| "invalid repair structure".to_string())?
                            .checked_add(1)
                            .ok_or_else(|| "invalid repair structure".to_string())?;
                    }
                }
            }
        }
        rows = rows
            .checked_add(1)
            .ok_or_else(|| "invalid repair structure".to_string())?;
        end_bit = bits.bit;
    }
    if rows == 0 || end_bit == 0 {
        return Err("no complete JPEG MCU row in the carved prefix".into());
    }
    let owner = *owners
        .get(
            end_bit
                .checked_sub(1)
                .ok_or_else(|| "invalid repair structure".to_string())?,
        )
        .ok_or_else(|| "JPEG decoded bit map is short".to_string())?;
    let block_start = owner
        .checked_mul(8)
        .ok_or_else(|| "invalid repair structure".to_string())?;
    let used = end_bit
        .checked_sub(block_start)
        .ok_or_else(|| "JPEG decoded bit map is inconsistent".to_string())?;
    let ent_end = *raw_ends
        .get(owner)
        .ok_or_else(|| "JPEG byte map is short".to_string())?;
    let mut trimmed = range(data, pos, ent_end)
        .ok_or_else(|| "JPEG entropy span is invalid".to_string())?
        .to_vec();
    if used < 8 && !trimmed.ends_with(&[0xFF, 0]) {
        let last = trimmed
            .len()
            .checked_sub(1)
            .ok_or_else(|| "invalid repair structure".to_string())?;
        let byte = trimmed
            .get_mut(last)
            .ok_or_else(|| "JPEG entropy data is empty".to_string())?;
        *byte |= (1u16
            .checked_shl((8 - used) as u32)
            .ok_or_else(|| "invalid repair structure".to_string())?
            - 1) as u8;
        if *byte == 0xFF {
            trimmed.push(0);
        }
    }

    let mut out = range(data, 0, pos)
        .ok_or_else(|| "JPEG header is short".to_string())?
        .to_vec();
    let height_at = sof_pos
        .checked_add(5)
        .ok_or_else(|| "invalid repair structure".to_string())?;
    let height_end = height_at
        .checked_add(2)
        .ok_or_else(|| "JPEG SOF height offset overflow".to_string())?;
    let height_bytes = out
        .get_mut(height_at..height_end)
        .ok_or_else(|| "JPEG SOF height is outside the header".to_string())?;
    let repaired_height = (rows as u32)
        .checked_mul(vmax)
        .ok_or_else(|| "invalid repair structure".to_string())?
        .checked_mul(8)
        .ok_or_else(|| "invalid repair structure".to_string())?
        .min(height);
    let repaired_height = u16::try_from(repaired_height)
        .map_err(|_| "JPEG repaired height exceeds the SOF field".to_string())?;
    height_bytes.copy_from_slice(&repaired_height.to_be_bytes());

    reserve(
        &mut out,
        trimmed
            .len()
            .checked_add(2)
            .ok_or_else(|| "invalid repair structure".to_string())?,
    )?;
    out.extend_from_slice(&trimmed);
    out.extend_from_slice(&[0xFF, 0xD9]);
    if out.len() as u64 > limits.max_validator_memory {
        return Err("repaired JPEG exceeds the validator budget".into());
    }
    Ok(CroppedRepair {
        output: out,
        recovered_rows: rows as u32,
        total_rows: mcuy,
    })
}
