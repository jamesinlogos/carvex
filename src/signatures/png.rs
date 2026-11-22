#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
use super::{png_chunk_crc_check, Analysis, Need, ValidationCheck};
use crc32fast::Hasher;

const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const IEND_SUFFIX: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
];

pub fn analyze(data: &[u8], at_limit: bool) -> Need {
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
    let mut bad_crc = 0u32;
    let mut crc_checks: Vec<ValidationCheck> = Vec::new();
    let mut good_crc_prefix = 8u64;
    let mut crc_failed = false;
    let mut ihdr = false;
    let mut idat = false;
    let mut plte = false;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut color_type = 0u8;
    let mut last_good = 8usize;

    loop {
        if pos + 8 > data.len() {
            return truncated(
                PngProgress {
                    last_good,
                    ihdr,
                    idat,
                    bad_crc,
                    width,
                    height,
                    checks: crc_checks.clone(),
                    validated_prefix: good_crc_prefix,
                },
                at_limit,
                "truncated chunk header",
            );
        }
        let Some(len_bytes) = data.get(pos..pos + 4) else {
            return truncated(
                PngProgress {
                    last_good,
                    ihdr,
                    idat,
                    bad_crc,
                    width,
                    height,
                    checks: crc_checks.clone(),
                    validated_prefix: good_crc_prefix,
                },
                at_limit,
                "truncated chunk header",
            );
        };
        let len = u32::from_be_bytes(len_bytes.try_into().unwrap_or([0; 4])) as usize;
        let Some(ctype) = data.get(pos + 4..pos + 8) else {
            return truncated(
                PngProgress {
                    last_good,
                    ihdr,
                    idat,
                    bad_crc,
                    width,
                    height,
                    checks: crc_checks.clone(),
                    validated_prefix: good_crc_prefix,
                },
                at_limit,
                "truncated chunk header",
            );
        };
        if !ctype.iter().all(|b| b.is_ascii_alphabetic()) {
            // A damaged chunk after a complete IHDR/IDAT is a salvageable
            // prefix, not a false header.  B-08 keeps the valid prefix.
            if ihdr && idat {
                return truncated(
                    PngProgress {
                        last_good,
                        ihdr,
                        idat,
                        bad_crc,
                        width,
                        height,
                        checks: crc_checks.clone(),
                        validated_prefix: good_crc_prefix,
                    },
                    at_limit,
                    "invalid chunk type after IDAT",
                );
            }
            return Need::Reject("PNG chunk type is not alphabetic".into());
        }
        if len > 256 * 1024 * 1024 {
            return Need::Reject(format!("implausible PNG chunk length {len}"));
        }
        let chunk_end = match pos.checked_add(12).and_then(|v| v.checked_add(len)) {
            Some(v) if v <= data.len() => v,
            _ => {
                return truncated(
                    PngProgress {
                        last_good,
                        ihdr,
                        idat,
                        bad_crc,
                        width,
                        height,
                        checks: crc_checks.clone(),
                        validated_prefix: good_crc_prefix,
                    },
                    at_limit,
                    "truncated chunk payload",
                )
            }
        };
        let Some(chunk_data) = data.get(pos + 8..pos + 8 + len) else {
            return truncated(
                PngProgress {
                    last_good,
                    ihdr,
                    idat,
                    bad_crc,
                    width,
                    height,
                    checks: crc_checks.clone(),
                    validated_prefix: good_crc_prefix,
                },
                at_limit,
                "truncated chunk payload",
            );
        };
        let Some(crc_bytes) = data.get(chunk_end - 4..chunk_end) else {
            return truncated(
                PngProgress {
                    last_good,
                    ihdr,
                    idat,
                    bad_crc,
                    width,
                    height,
                    checks: crc_checks.clone(),
                    validated_prefix: good_crc_prefix,
                },
                at_limit,
                "truncated chunk CRC",
            );
        };
        let stored_crc = u32::from_be_bytes(crc_bytes.try_into().unwrap_or([0; 4]));
        let mut hasher = Hasher::new();
        hasher.update(ctype);
        hasher.update(chunk_data);
        let calc_crc = hasher.finalize();
        if calc_crc != stored_crc {
            bad_crc += 1;
            crc_failed = true;
            crc_checks.push(png_chunk_crc_check(ctype, pos));
        } else if !crc_failed {
            good_crc_prefix = chunk_end as u64;
        }

        if chunks == 0 && ctype != b"IHDR" {
            return Need::Reject("PNG first chunk is not IHDR".into());
        }

        if ctype == b"IHDR" {
            if ihdr {
                return Need::Reject("duplicate PNG IHDR".into());
            }
            if len != 13 {
                return Need::Reject("PNG IHDR has wrong length".into());
            }
            let (Some(w), Some(h)) = (
                chunk_data.get(0..4).and_then(|b| b.try_into().ok()),
                chunk_data.get(4..8).and_then(|b| b.try_into().ok()),
            ) else {
                return Need::Reject("PNG IHDR is truncated".into());
            };
            width = u32::from_be_bytes(w);
            height = u32::from_be_bytes(h);
            let (Some(&depth), Some(&ct), Some(&compression), Some(&filter), Some(&interlace)) = (
                chunk_data.get(8),
                chunk_data.get(9),
                chunk_data.get(10),
                chunk_data.get(11),
                chunk_data.get(12),
            ) else {
                return Need::Reject("PNG IHDR is truncated".into());
            };
            color_type = ct;
            if width == 0 || height == 0 {
                return Need::Reject("PNG has zero dimensions".into());
            }
            if width > 0x7FFF_FFFF || height > 0x7FFF_FFFF {
                return Need::Reject("PNG dimensions exceed the specification".into());
            }
            if compression != 0 || filter != 0 || interlace > 1 {
                return Need::Reject("PNG IHDR compression/filter/interlace invalid".into());
            }
            let depth_ok = match color_type {
                0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
                2 => matches!(depth, 8 | 16),
                3 => matches!(depth, 1 | 2 | 4 | 8),
                4 => matches!(depth, 8 | 16),
                6 => matches!(depth, 8 | 16),
                _ => false,
            };
            if !depth_ok {
                return Need::Reject("PNG bit depth/color type combination invalid".into());
            }
            ihdr = true;
        } else if ctype == b"PLTE" {
            plte = true;
        } else if ctype == b"IDAT" {
            idat = true;
        } else if ctype == b"IEND" {
            if len != 0 {
                return Need::Reject("PNG IEND has non-zero length".into());
            }
            pos = chunk_end;
            last_good = pos;
            break;
        }

        chunks += 1;
        if chunks > 1_000_000 {
            return Need::Reject("PNG chunk loop guard exceeded".into());
        }
        pos = chunk_end;
        last_good = pos;
    }

    if !ihdr {
        return Need::Reject("PNG has no IHDR".into());
    }
    if !idat {
        return Need::Reject("PNG has no IDAT".into());
    }
    if color_type == 3 && !plte {
        return Need::Reject("indexed PNG has no PLTE".into());
    }

    let mut score = 100i32 - (bad_crc.min(3) * 15) as i32;
    score = score.clamp(0, 100);
    let mut notes = vec![format!(
        "PNG {}x{}, {} chunk(s), color type {}",
        width,
        height,
        chunks + 1,
        color_type
    )];
    if bad_crc > 0 {
        notes.push(format!("{bad_crc} chunk CRC error(s)"));
    }
    Need::Analyzed(Analysis {
        end: last_good,
        complete: true,
        score: score as u8,
        width: Some(width),
        height: Some(height),
        notes,
        repair_suffix: IEND_SUFFIX,
        integrity_checks: crc_checks,
        // Decode mode recomputes CRCs and supplies its own checked prefix;
        // structural mode does not claim decoded content bytes.
        validated_prefix: 0,
    })
}

struct PngProgress {
    last_good: usize,
    ihdr: bool,
    idat: bool,
    bad_crc: u32,
    width: u32,
    height: u32,
    checks: Vec<ValidationCheck>,
    validated_prefix: u64,
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
