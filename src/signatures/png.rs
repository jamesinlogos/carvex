#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
use super::{png_chunk_crc_check, Analysis, Need, ValidationCheck, ValidationLimits};
use crc32fast::Hasher;

const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
pub(crate) const IEND_SUFFIX: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
];

/// Analyze a PNG candidate with the default decode budget.
pub fn analyze(data: &[u8], at_limit: bool) -> Need {
    analyze_with_limits(data, at_limit, ValidationLimits::default())
}

/// Analyze a PNG candidate and return structural parse results (H-25).
pub fn analyze_with_limits(data: &[u8], at_limit: bool, limits: ValidationLimits) -> Need {
    if data.len() < 8 {
        return if at_limit {
            Need::Reject("too short for PNG".into())
        } else {
            Need::More
        };
    }
    if data.get(0..8) != Some(PNG_MAGIC) {
        return Need::Reject("missing PNG signature".into());
    }

    let mut pos = 8usize;
    let mut chunks = 0u32;
    let mut progress = PngProgress {
        last_good: 8,
        validated_prefix: 8,
        ..PngProgress::default()
    };

    loop {
        let chunk = match read_png_chunk(data, pos, &progress, at_limit) {
            Ok(chunk) => chunk,
            Err(Need::Analyzed(mut analysis)) => {
                // H-25: an incomplete chunk is not a verified byte boundary.
                // The last inflatable IDAT byte is; for a cut inside IDAT this
                // extends the structural fallback up to that byte, and for a
                // cut after IDAT it trims anything the stream did not decode.
                let pending =
                    if data.get(pos + 4..pos + 8) == Some(&b"IDAT"[..]) && pos + 8 <= data.len() {
                        Some(pos + 8)
                    } else {
                        None
                    };
                if let Some((end, from_pending)) = png_inflate_end(
                    data,
                    &progress.idat_ranges,
                    pending,
                    png_row_len(&progress),
                    limits,
                ) {
                    let end = end.min(data.len());
                    if from_pending && end > analysis.end {
                        analysis.notes.push(format!(
                            "kept {} byte(s) of inflatable IDAT payload",
                            end - analysis.end
                        ));
                    } else if end < analysis.end {
                        analysis.notes.push(format!(
                            "trimmed {} byte(s) after the last inflatable IDAT byte",
                            analysis.end - end
                        ));
                    }
                    analysis.end = end;
                    analysis.validated_prefix = analysis.validated_prefix.min(end as u64);
                }
                return Need::Analyzed(analysis);
            }
            Err(need) => return need,
        };
        update_chunk_crc(pos, &chunk, &mut progress);

        if chunks == 0 && chunk.ctype != b"IHDR" {
            return Need::Reject("PNG first chunk is not IHDR".into());
        }
        match chunk.ctype {
            b"IHDR" => {
                if let Err(need) = apply_ihdr(&chunk, &mut progress) {
                    return need;
                }
            }
            b"PLTE" => progress.plte = true,
            b"IDAT" => {
                progress.idat = true;
                progress.idat_ranges.push((pos + 8, pos + 8 + chunk.len));
            }
            b"IEND" => {
                if chunk.len != 0 {
                    return Need::Reject("PNG IEND has non-zero length".into());
                }
                pos = chunk.chunk_end;
                progress.last_good = pos;
                break;
            }
            _ => {}
        }

        chunks += 1;
        if chunks > 1_000_000 {
            return Need::Reject("PNG chunk loop guard exceeded".into());
        }
        pos = chunk.chunk_end;
        progress.last_good = pos;
    }

    if !progress.ihdr {
        return Need::Reject("PNG has no IHDR".into());
    }
    if !progress.idat {
        return Need::Reject("PNG has no IDAT".into());
    }
    if progress.color_type == 3 && !progress.plte {
        return Need::Reject("indexed PNG has no PLTE".into());
    }
    png_finished(&progress, chunks)
}

struct PngChunk<'a> {
    ctype: &'a [u8],
    len: usize,
    chunk_end: usize,
    chunk_data: &'a [u8],
    stored_crc: u32,
}

fn read_png_chunk<'a>(
    data: &'a [u8],
    pos: usize,
    progress: &PngProgress,
    at_limit: bool,
) -> Result<PngChunk<'a>, Need> {
    let truncated_need = |why: &str| progress.truncated(at_limit, why);
    if pos + 8 > data.len() {
        return Err(truncated_need("truncated chunk header"));
    }
    let Some(len_bytes) = data.get(pos..pos + 4) else {
        return Err(truncated_need("truncated chunk header"));
    };
    let len = u32::from_be_bytes(len_bytes.try_into().unwrap_or([0; 4])) as usize;
    let Some(ctype) = data.get(pos + 4..pos + 8) else {
        return Err(truncated_need("truncated chunk header"));
    };
    if !ctype.iter().all(u8::is_ascii_alphabetic) {
        // A damaged chunk after a complete IHDR/IDAT is a salvageable
        // prefix, not a false header.  B-08 keeps the valid prefix.
        if progress.ihdr && progress.idat {
            return Err(truncated_need("invalid chunk type after IDAT"));
        }
        return Err(Need::Reject("PNG chunk type is not alphabetic".into()));
    }
    if len > 256 * 1024 * 1024 {
        return Err(Need::Reject(format!("implausible PNG chunk length {len}")));
    }
    let chunk_end = match pos.checked_add(12).and_then(|v| v.checked_add(len)) {
        Some(v) if v <= data.len() => v,
        _ => return Err(truncated_need("truncated chunk payload")),
    };
    let Some(chunk_data) = data.get(pos + 8..pos + 8 + len) else {
        return Err(truncated_need("truncated chunk payload"));
    };
    let Some(crc_bytes) = data.get(chunk_end - 4..chunk_end) else {
        return Err(truncated_need("truncated chunk CRC"));
    };
    let stored_crc = u32::from_be_bytes(crc_bytes.try_into().unwrap_or([0; 4]));
    Ok(PngChunk {
        ctype,
        len,
        chunk_end,
        chunk_data,
        stored_crc,
    })
}

fn update_chunk_crc(pos: usize, chunk: &PngChunk<'_>, progress: &mut PngProgress) {
    let mut hasher = Hasher::new();
    hasher.update(chunk.ctype);
    hasher.update(chunk.chunk_data);
    if hasher.finalize() != chunk.stored_crc {
        progress.bad_crc += 1;
        progress.crc_failed = true;
        progress.checks.push(png_chunk_crc_check(chunk.ctype, pos));
    } else if !progress.crc_failed {
        progress.validated_prefix = chunk.chunk_end as u64;
    }
}

fn apply_ihdr(chunk: &PngChunk<'_>, progress: &mut PngProgress) -> Result<(), Need> {
    if progress.ihdr {
        return Err(Need::Reject("duplicate PNG IHDR".into()));
    }
    if chunk.len != 13 {
        return Err(Need::Reject("PNG IHDR has wrong length".into()));
    }
    let (Some(w), Some(h)) = (
        chunk.chunk_data.get(0..4).and_then(|b| b.try_into().ok()),
        chunk.chunk_data.get(4..8).and_then(|b| b.try_into().ok()),
    ) else {
        return Err(Need::Reject("PNG IHDR is truncated".into()));
    };
    let width = u32::from_be_bytes(w);
    let height = u32::from_be_bytes(h);
    let (Some(&depth), Some(&ct), Some(&compression), Some(&filter), Some(&interlace)) = (
        chunk.chunk_data.get(8),
        chunk.chunk_data.get(9),
        chunk.chunk_data.get(10),
        chunk.chunk_data.get(11),
        chunk.chunk_data.get(12),
    ) else {
        return Err(Need::Reject("PNG IHDR is truncated".into()));
    };
    progress.width = width;
    progress.height = height;
    progress.color_type = ct;
    progress.bit_depth = depth;
    progress.interlace = interlace;
    if width == 0 || height == 0 {
        return Err(Need::Reject("PNG has zero dimensions".into()));
    }
    if width > 0x7FFF_FFFF || height > 0x7FFF_FFFF {
        return Err(Need::Reject(
            "PNG dimensions exceed the specification".into(),
        ));
    }
    if compression != 0 || filter != 0 || interlace > 1 {
        return Err(Need::Reject(
            "PNG IHDR compression/filter/interlace invalid".into(),
        ));
    }
    let depth_ok = match ct {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        2 => matches!(depth, 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        4 => matches!(depth, 8 | 16),
        6 => matches!(depth, 8 | 16),
        _ => false,
    };
    if !depth_ok {
        return Err(Need::Reject(
            "PNG bit depth/color type combination invalid".into(),
        ));
    }
    progress.ihdr = true;
    Ok(())
}

fn png_finished(progress: &PngProgress, chunks: u32) -> Need {
    let mut score = 100i32 - (progress.bad_crc.min(3) * 15) as i32;
    score = score.clamp(0, 100);
    let mut notes = vec![format!(
        "PNG {}x{}, {} chunk(s), color type {}",
        progress.width,
        progress.height,
        chunks + 1,
        progress.color_type
    )];
    if progress.bad_crc > 0 {
        notes.push(format!("{} chunk CRC error(s)", progress.bad_crc));
    }
    Need::Analyzed(Analysis {
        end: progress.last_good,
        complete: true,
        score: score as u8,
        width: Some(progress.width),
        height: Some(progress.height),
        notes,
        repair_suffix: IEND_SUFFIX,
        integrity_checks: progress.checks.clone(),
        // Decode mode recomputes CRCs and supplies its own checked prefix;
        // structural mode does not claim decoded content bytes.
        validated_prefix: 0,
    })
}

/// Byte length of one unfiltered scanline, used to stop a truncated IDAT at
/// the last row whose PNG filter byte is valid. Adam7 has a different row
/// layout, so it falls back to the plain inflate-prefix rule.
fn png_row_len(p: &PngProgress) -> Option<usize> {
    if p.interlace != 0 || p.bit_depth == 0 {
        return None;
    }
    let channels = match p.color_type {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    let bits = (p.width as u64)
        .checked_mul(channels)?
        .checked_mul(p.bit_depth as u64)?;
    usize::try_from(bits.div_ceil(8)).ok()?.checked_add(1)
}

/// Map a bounded zlib-prefix decision back to a byte offset in the PNG.
/// `pending` is the payload start of an IDAT chunk cut by the carve boundary.
/// The boolean in the result says whether the decision came from that chunk.
fn png_inflate_end(
    data: &[u8],
    ranges: &[(usize, usize)],
    pending: Option<usize>,
    row_len: Option<usize>,
    limits: ValidationLimits,
) -> Option<(usize, bool)> {
    let mut all = ranges.to_vec();
    if let Some(start) = pending {
        if start <= data.len() {
            all.push((start, data.len()));
        }
    }
    let slices: Vec<&[u8]> = all
        .iter()
        .map(|(start, end)| data.get(*start..*end))
        .collect::<Option<Vec<_>>>()?;
    let (index, consumed) =
        super::zlib_prefix_valid_rows(&slices, limits.max_validator_memory, row_len)?;
    let (start, payload_end) = all.get(index)?;
    let is_pending = index >= ranges.len();
    // A complete zlib stream ending exactly at an IDAT payload boundary still
    // owns that chunk's four CRC bytes; --repair must append IEND after them.
    let end = if !is_pending
        && consumed >= payload_end.saturating_sub(*start)
        && payload_end.saturating_add(4) <= data.len()
    {
        payload_end.saturating_add(4)
    } else {
        start.saturating_add(consumed)
    };
    Some((end.min(data.len()), is_pending))
}

#[derive(Clone, Default)]
struct PngProgress {
    last_good: usize,
    ihdr: bool,
    idat: bool,
    plte: bool,
    bad_crc: u32,
    width: u32,
    height: u32,
    color_type: u8,
    bit_depth: u8,
    interlace: u8,
    crc_failed: bool,
    checks: Vec<ValidationCheck>,
    validated_prefix: u64,
    /// Payload ranges of complete IDAT chunks, in file order (H-25).
    idat_ranges: Vec<(usize, usize)>,
}

impl PngProgress {
    fn truncated(&self, at_limit: bool, why: &str) -> Need {
        truncated(self.clone(), at_limit, why)
    }
}

fn truncated(p: PngProgress, at_limit: bool, why: &str) -> Need {
    if !at_limit {
        return Need::More;
    }
    let score = if p.ihdr && p.idat {
        60
    } else if p.ihdr {
        40
    } else {
        10
    };
    let mut notes = vec![format!(
        "truncated PNG ({why}); IHDR={}, IDAT={}; repair appends IEND",
        p.ihdr, p.idat
    )];
    if p.bad_crc > 0 {
        notes.push(format!("{} chunk CRC error(s)", p.bad_crc));
    }
    Need::Analyzed(Analysis {
        end: p.last_good,
        complete: false,
        score,
        width: if p.width > 0 { Some(p.width) } else { None },
        height: if p.height > 0 { Some(p.height) } else { None },
        notes,
        repair_suffix: IEND_SUFFIX,
        integrity_checks: p.checks,
        validated_prefix: p.validated_prefix,
    })
}

#[cfg(test)]
// Test fixtures use fixed slices and expect valid layouts; the production
// parser above remains subject to the deny list.
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn chunk(typ: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(data.len() as u32).to_be_bytes());
        v.extend_from_slice(typ);
        v.extend_from_slice(data);
        let mut h = Hasher::new();
        h.update(typ);
        h.update(data);
        v.extend_from_slice(&h.finalize().to_be_bytes());
        v
    }

    fn png(color_type: u8) -> Vec<u8> {
        let mut d = PNG_MAGIC.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&1u32.to_be_bytes());
        ihdr.extend_from_slice(&1u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]);
        d.extend(chunk(b"IHDR", &ihdr));
        if color_type == 3 {
            d.extend(chunk(b"PLTE", &[0, 0, 0]));
        }
        d.extend(chunk(b"IDAT", &[0x78, 0x9C, 0x63, 0, 0, 0, 1, 0, 1]));
        d.extend(chunk(b"IEND", &[]));
        d
    }

    #[test]
    fn parses_png() {
        let d = png(6);
        match analyze(&d, true) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert_eq!(a.end, d.len());
                assert_eq!(a.width, Some(1));
                assert_eq!(a.height, Some(1));
                assert_eq!(a.score, 100);
            }
            _ => panic!("expected complete PNG"),
        }
    }

    #[test]
    fn reports_bad_crc_but_keeps_file() {
        let mut d = png(2);
        // Corrupt the stored CRC of the first IDAT chunk.
        let idat = memchr::memmem::find(&d, b"IDAT").unwrap();
        let len = u32::from_be_bytes(d[idat - 4..idat].try_into().unwrap()) as usize;
        d[idat + 4 + len] ^= 0xFF;
        match analyze(&d, true) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert!(a.score < 100);
                assert!(a.notes.iter().any(|n| n.contains("CRC")));
                assert!(a.integrity_failed());
                let check = a
                    .integrity_checks
                    .iter()
                    .find(|c| c.name == "chunk_crc")
                    .expect("bad CRC must have a structured check");
                assert_eq!(check.ok, Some(false));
                assert!(check.note.contains("IDAT"));
                assert!(check.note.contains("byte"));
            }
            _ => panic!("expected PNG with CRC error"),
        }
    }

    #[test]
    fn truncated_at_chunk_boundary_is_repairable() {
        let d = png(2);
        let cut = &d[..d.len() - 12];
        assert!(matches!(analyze(cut, false), Need::More));
        match analyze(cut, true) {
            Need::Analyzed(a) => {
                assert!(!a.complete);
                assert_eq!(a.end, cut.len());
                assert_eq!(a.score, 60);
                assert_eq!(a.repair_suffix, IEND_SUFFIX);
            }
            _ => panic!("expected truncated PNG"),
        }
    }

    #[test]
    fn rejects_non_ihdr_first_chunk() {
        let mut d = PNG_MAGIC.to_vec();
        d.extend(chunk(b"IDAT", &[1, 2, 3]));
        d.extend(chunk(b"IEND", &[]));
        assert!(matches!(analyze(&d, true), Need::Reject(_)));
    }

    #[test]
    fn rejects_indexed_without_plte() {
        let mut d = PNG_MAGIC.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&1u32.to_be_bytes());
        ihdr.extend_from_slice(&1u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
        d.extend(chunk(b"IHDR", &ihdr));
        d.extend(chunk(b"IDAT", &[1, 2, 3]));
        d.extend(chunk(b"IEND", &[]));
        assert!(matches!(analyze(&d, true), Need::Reject(_)));
    }
}
