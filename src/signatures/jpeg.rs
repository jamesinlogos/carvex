#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
use super::{Analysis, Need};

pub(crate) const EOI: &[u8] = &[0xFF, 0xD9];

#[derive(Default)]
struct JpegProgress {
    sof: bool,
    sos: bool,
    scans: u32,
    app_segments: u32,
    width: u32,
    height: u32,
    sof_name: String,
}

impl JpegProgress {
    fn truncated(&self, end: usize, at_limit: bool, why: &str) -> Need {
        truncated(
            end,
            at_limit,
            self.sof,
            self.sos,
            self.width,
            self.height,
            why,
        )
    }

    fn complete(&self, end: usize) -> Need {
        Need::Analyzed(Analysis {
            end,
            complete: true,
            score: 100,
            width: Some(self.width),
            height: Some(self.height),
            notes: vec![format!(
                "{} {}x{}, {} scan(s), {} APP segment(s)",
                self.sof_name, self.width, self.height, self.scans, self.app_segments
            )],
            repair_suffix: EOI,
            integrity_checks: Vec::new(),
            validated_prefix: 0,
        })
    }
}

fn jpeg_segment_end(
    data: &[u8],
    pos: usize,
    progress: &JpegProgress,
    at_limit: bool,
) -> Result<usize, Need> {
    // H-25: a declared length that runs past the supplied buffer is the cut,
    // not verified image data. Once SOF proves the header, end at the segment
    // boundary that is actually present instead of claiming the padding that
    // happens to satisfy the length field.
    let cut_end = |why: &str| {
        progress.truncated(
            if progress.sof {
                pos.saturating_sub(2)
            } else {
                data.len()
            },
            at_limit,
            why,
        )
    };
    if pos + 2 > data.len() {
        return Err(cut_end("truncated segment length"));
    }
    let Some(seg_len) = be16(data, pos) else {
        return Err(cut_end("truncated segment length"));
    };
    let seg_len = seg_len as usize;
    if seg_len < 2 {
        return Err(Need::Reject(format!(
            "invalid JPEG segment length at offset {pos}"
        )));
    }
    match pos.checked_add(seg_len) {
        Some(v) if v <= data.len() => Ok(v),
        _ => Err(cut_end("truncated segment payload")),
    }
}

fn apply_sof(
    data: &[u8],
    pos: usize,
    seg_len: usize,
    marker: u8,
    progress: &mut JpegProgress,
    at_limit: bool,
) -> Result<(), Need> {
    if seg_len < 8 {
        return Err(Need::Reject("SOF segment too short".into()));
    }
    let (Some(h), Some(w)) = (be16(data, pos + 3), be16(data, pos + 5)) else {
        return Err(progress.truncated(data.len(), at_limit, "truncated SOF dimensions"));
    };
    progress.height = h as u32;
    progress.width = w as u32;
    if progress.width == 0 || progress.height == 0 {
        return Err(Need::Reject("JPEG SOF has zero dimensions".into()));
    }
    progress.sof = true;
    progress.sof_name = sof_marker_name(marker).to_string();
    Ok(())
}

enum EntropyStep {
    Resume(usize),
    Complete(Need),
    Fail(Need),
}

/// Validate the payload of a table/scan marker found between entropy scans.
/// Payloads here exclude the two length bytes. Random padding can supply a
/// plausible marker byte and length, but almost never a structurally valid
/// DHT/DQT/SOS payload, so validating them keeps random `FF xx` sequences from
/// being mistaken for the rest of a progressive JPEG.
fn plausible_segment_payload(marker: u8, payload: &[u8]) -> bool {
    match marker {
        0xC4 => {
            // DHT: class/id byte, 16 code-count bytes, then exactly that many
            // Huffman value bytes.
            let Some((&id, counts)) = payload.split_first() else {
                return false;
            };
            if id >> 4 > 1 || id & 0x0F > 3 || counts.len() < 16 {
                return false;
            }
            let Some(counts) = counts.get(..16) else {
                return false;
            };
            let symbols: usize = counts.iter().map(|n| *n as usize).sum();
            symbols <= 256 && payload.len() == 17usize.saturating_add(symbols)
        }
        0xDB => {
            // DQT: precision/id byte plus 64 (8-bit) or 128 (16-bit) values.
            let Some((&id, values)) = payload.split_first() else {
                return false;
            };
            if id >> 4 > 1 || id & 0x0F > 3 {
                return false;
            }
            values.len() == if id >> 4 == 0 { 64 } else { 128 }
        }
        0xDA => {
            // SOS: component count Ns, Ns selector pairs, Ss, Se, Ah/Al.
            let Some((&ns, rest)) = payload.split_first() else {
                return false;
            };
            (1..=4).contains(&ns)
                && rest.len() == usize::from(ns).saturating_mul(2).saturating_add(3)
        }
        0xDC => payload.len() >= 2,
        0xDD => payload.len() == 2,
        _ => false,
    }
}

/// A marker encountered between entropy scans is accepted only when it is a
/// structurally valid table/scan marker. Random padding can contain an `FF xx`
/// that resembles a segment header; validating the payload makes that rare.
/// The *following* scan is parsed by the normal loop, so an invalid continuation
/// still ends the candidate at the first bad `FF`, while a real progressive
/// JPEG keeps all of its intermediate DHT/DQT/SOS markers.
fn valid_interscan_marker(data: &[u8], mut pos: usize) -> bool {
    if data.get(pos) != Some(&0xFF) {
        return false;
    }
    while data.get(pos) == Some(&0xFF) {
        pos += 1;
    }
    let Some(&marker) = data.get(pos) else {
        return false;
    };
    pos += 1;
    if !matches!(marker, 0xC4 | 0xDA | 0xDB | 0xDC | 0xDD) {
        return false;
    }
    let Some(seg_len) = be16(data, pos) else {
        return false;
    };
    let seg_len = seg_len as usize;
    if seg_len < 2 {
        return false;
    }
    let Some(seg_end) = pos.checked_add(seg_len) else {
        return false;
    };
    if seg_end > data.len() {
        return false;
    }
    let Some(payload) = data.get(pos.saturating_add(2)..seg_end) else {
        return false;
    };
    plausible_segment_payload(marker, payload)
}

fn scan_entropy(
    data: &[u8],
    mut pos: usize,
    progress: &JpegProgress,
    at_limit: bool,
) -> EntropyStep {
    // Stuffed 0x00 and RSTn are data; EOI ends the image; any other marker
    // returns to segment parsing (needed for progressive JPEG).
    loop {
        let Some(rel) = memchr::memchr(0xFF, data.get(pos..).unwrap_or_default()) else {
            return EntropyStep::Fail(progress.truncated(
                data.len(),
                at_limit,
                "end of entropy-coded data",
            ));
        };
        let mut q = pos + rel;
        while data.get(q + 1) == Some(&0xFF) {
            q += 1;
        }
        if q + 1 >= data.len() {
            return EntropyStep::Fail(progress.truncated(
                data.len(),
                at_limit,
                "truncated entropy marker",
            ));
        }
        let Some(&code) = data.get(q + 1) else {
            return EntropyStep::Fail(progress.truncated(
                data.len(),
                at_limit,
                "truncated entropy marker",
            ));
        };
        match code {
            0x00 | 0xD0..=0xD7 | 0x01 => pos = q + 2,
            0xD9 => {
                if !progress.sof {
                    return EntropyStep::Fail(Need::Reject("JPEG EOI before SOF".into()));
                }
                return EntropyStep::Complete(progress.complete(q + 2));
            }
            _ => {
                if valid_interscan_marker(data, q) {
                    return EntropyStep::Resume(q);
                }
                // Back up over FF fill bytes: the cut begins at the first FF.
                let mut fill_start = q;
                while fill_start > pos && data.get(fill_start.saturating_sub(1)) == Some(&0xFF) {
                    fill_start -= 1;
                }
                return EntropyStep::Fail(progress.truncated(
                    fill_start,
                    at_limit,
                    "invalid continuation after SOS",
                ));
            }
        }
    }
}

fn next_jpeg_marker(
    data: &[u8],
    mut pos: usize,
    marker_start: usize,
    progress: &JpegProgress,
    at_limit: bool,
) -> Result<(u8, usize), Need> {
    if data.get(pos) != Some(&0xFF) {
        if progress.sof {
            return Err(progress.truncated(
                marker_start,
                at_limit,
                if progress.sos {
                    "invalid continuation after SOS"
                } else {
                    "invalid segment continuation"
                },
            ));
        }
        return Err(Need::Reject(format!(
            "expected JPEG marker at offset {pos}"
        )));
    }
    while data.get(pos) == Some(&0xFF) {
        pos += 1;
    }
    if pos >= data.len() {
        // A trailing FF run after a cut is fill, not entropy data: FF is
        // stuffed as FF 00 inside a scan. Never claim those bytes.
        return Err(progress.truncated(
            marker_start,
            at_limit,
            "trailing FF run after truncated data",
        ));
    }
    let Some(&marker) = data.get(pos) else {
        return Err(progress.truncated(marker_start, at_limit, "truncated marker"));
    };
    Ok((marker, pos + 1))
}

/// Analyze a JPEG candidate and return structural parse results.
pub fn analyze(data: &[u8], at_limit: bool) -> Need {
    if data.len() < 4 {
        return if at_limit {
            Need::Reject("too short for a JPEG".into())
        } else {
            Need::More
        };
    }
    if data.first() != Some(&0xFF) || data.get(1) != Some(&0xD8) {
        return Need::Reject("missing JPEG SOI".into());
    }

    let mut pos = 2usize;
    let mut progress = JpegProgress::default();

    loop {
        if pos >= data.len() {
            return progress.truncated(data.len(), at_limit, "end of buffer");
        }
        // H-25: a marker must follow every segment. Once SOF has been seen the
        // candidate is provably a JPEG; an invalid continuation (padding or
        // unrelated data after a cut) ends the salvaged prefix here instead of
        // rejecting everything before it.
        let marker_start = pos;
        let (marker, next_pos) =
            match next_jpeg_marker(data, pos, marker_start, &progress, at_limit) {
                Ok(v) => v,
                Err(need) => return need,
            };
        pos = next_pos;

        match marker {
            0x00 => {
                if progress.sof {
                    return progress.truncated(
                        marker_start,
                        at_limit,
                        if progress.sos {
                            "invalid marker 0x00 after SOS"
                        } else {
                            "invalid marker 0x00 after SOF"
                        },
                    );
                }
                return Need::Reject("invalid JPEG marker 0x00 outside entropy data".into());
            }
            0xD8 => {
                if progress.sos {
                    return progress.truncated(marker_start, at_limit, "next SOI after SOS");
                }
                return Need::Reject("nested JPEG SOI".into());
            }
            0xD9 => {
                if !progress.sof {
                    return Need::Reject("JPEG EOI before any SOF".into());
                }
                if !progress.sos {
                    return Need::Reject("JPEG EOI before any SOS".into());
                }
                return progress.complete(pos);
            }
            // Standalone markers (TEM, RSTn).
            0x01 | 0xD0..=0xD7 => continue,
            _ => {}
        }

        let seg_end = match jpeg_segment_end(data, pos, &progress, at_limit) {
            Ok(v) => v,
            Err(Need::Reject(_)) if progress.sof => {
                return progress.truncated(marker_start, at_limit, "invalid segment after SOF");
            }
            Err(need) => return need,
        };
        if is_sof(marker) {
            if let Err(need) = apply_sof(data, pos, seg_end - pos, marker, &mut progress, at_limit)
            {
                if progress.sos && matches!(&need, Need::Reject(_)) {
                    return progress.truncated(marker_start, at_limit, "invalid SOF after SOS");
                }
                return need;
            }
        }

        if marker == 0xDA {
            progress.sos = true;
            progress.scans += 1;
            pos = seg_end;
            match scan_entropy(data, pos, &progress, at_limit) {
                EntropyStep::Resume(next) => pos = next,
                EntropyStep::Complete(need) | EntropyStep::Fail(need) => return need,
            }
            continue;
        }

        if (0xE0..=0xEF).contains(&marker) || marker == 0xFE {
            progress.app_segments += 1;
        }
        pos = seg_end;
    }
}

fn is_sof(m: u8) -> bool {
    matches!(
        m,
        0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF
    )
}

fn sof_marker_name(m: u8) -> &'static str {
    match m {
        0xC0 => "SOF0 baseline",
        0xC1 => "SOF1 extended sequential",
        0xC2 => "SOF2 progressive",
        0xC3 => "SOF3 lossless",
        0xC5 => "SOF5 differential sequential",
        0xC6 => "SOF6 differential progressive",
        0xC7 => "SOF7 differential lossless",
        0xC9 => "SOF9 arithmetic sequential",
        0xCA => "SOF10 arithmetic progressive",
        0xCB => "SOF11 arithmetic lossless",
        0xCD => "SOF13 differential arithmetic sequential",
        0xCE => "SOF14 differential arithmetic progressive",
        0xCF => "SOF15 differential arithmetic lossless",
        _ => "SOF",
    }
}

#[cfg(test)]
fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF, marker];
    v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

fn truncated(
    end: usize,
    at_limit: bool,
    sof: bool,
    sos: bool,
    width: u32,
    height: u32,
    why: &str,
) -> Need {
    if !at_limit {
        return Need::More;
    }
    let score = if sof && sos {
        60
    } else if sof {
        35
    } else {
        10
    };
    Need::Analyzed(Analysis {
        end,
        complete: false,
        score,
        width: if width > 0 { Some(width) } else { None },
        height: if height > 0 { Some(height) } else { None },
        notes: vec![format!(
            "truncated JPEG ({why}); SOF={}, SOS={}; repair appends EOI",
            sof, sos
        )],
        repair_suffix: EOI,
        integrity_checks: Vec::new(),
        validated_prefix: 0,
    })
}

// ---------------------------------------------------------------------------
// Baseline entropy-count validation (A-04)
//
// `jpeg-decoder` intentionally tolerates truncated entropy data, so a JPEG
// whose header comes from one file and whose scan comes from another still
// decodes to *some* pixels.  For baseline sequential JPEGs we can do better:
// count the MCUs in the entropy stream with the file's own Huffman tables and
// compare with the frame geometry.  Spliced scans then fail even though every
// marker is structurally valid.

use std::collections::HashMap;

type HuffmanTables = Vec<(usize, usize, HuffmanTable)>;

#[derive(Clone, Default)]
struct HuffmanTable {
    codes: HashMap<(u8, u16), u8>,
}

/// Header-only frame claim used by the H-06 memory guard.
pub(crate) struct FrameClaim {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) components: u8,
    pub(crate) arithmetic: bool,
}

/// Read the first SOF header without decoding any image data.
pub(crate) fn frame_claim(data: &[u8]) -> Option<FrameClaim> {
    if data.get(0..2) != Some(&[0xFF, 0xD8]) {
        return None;
    }
    let mut pos = 2usize;
    while pos + 1 < data.len() {
        if data.get(pos) != Some(&0xFF) {
            pos += 1;
            continue;
        }
        while data.get(pos) == Some(&0xFF) {
            pos += 1;
        }
        let marker = *data.get(pos)?;
        pos += 1;
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if marker == 0xD9 || marker == 0xDA {
            return None;
        }
        let len = be16(data, pos)? as usize;
        if len < 2 || pos.checked_add(len)? > data.len() {
            return None;
        }
        if is_sof(marker) {
            if len < 8 {
                return None;
            }
            let p = pos + 2;
            let height = be16(data, p + 1)? as u32;
            let width = be16(data, p + 3)? as u32;
            let components = *data.get(p + 5)?;
            if width == 0 || height == 0 || components == 0 {
                return None;
            }
            return Some(FrameClaim {
                width,
                height,
                components,
                arithmetic: matches!(marker, 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF),
            });
        }
        pos += len;
    }
    None
}

struct FrameInfo {
    width: u32,
    height: u32,
    max_h: u32,
    max_v: u32,
    components: Vec<(u8, u32, u32)>,
}

struct ScanComponent {
    id: u8,
    dc: usize,
    ac: usize,
}

struct ScanInfo {
    components: Vec<ScanComponent>,
    ss: u8,
    se: u8,
    ah: u8,
    al: u8,
}

/// Validate the entropy-coded scan count of a baseline sequential JPEG.
///
/// Returns `None` when the file uses a feature this small validator does not
/// model (progressive, arithmetic, multiple scans).  Returns `Some(Err)` only
/// when the scan is decodable and the MCU count contradicts the frame header,
/// which is the signature of a spliced file.
fn parse_dht_segment(data: &[u8], pos: usize) -> Option<(HuffmanTables, usize)> {
    let len = be16(data, pos)? as usize;
    if len < 2 || pos.checked_add(len)? > data.len() {
        return None;
    }
    let end = pos + len;
    let mut q = pos + 2;
    let mut entries = Vec::new();
    while q < end {
        let tc_th = *data.get(q)?;
        q += 1;
        let class = tc_th >> 4;
        let id = (tc_th & 0x0F) as usize;
        if class > 1 || id > 3 {
            return None;
        }
        let counts = data.get(q..q + 16)?;
        q += 16;
        let total: usize = counts.iter().map(|&c| c as usize).sum();
        let symbols = data.get(q..q + total)?;
        q += total;
        let mut code = 0u16;
        let mut idx = 0usize;
        let mut table = HuffmanTable::default();
        for (i, &count) in counts.iter().enumerate() {
            for _ in 0..count {
                table
                    .codes
                    .insert(((i + 1) as u8, code), *symbols.get(idx)?);
                idx += 1;
                code = code.checked_add(1)?;
            }
            code <<= 1;
        }
        entries.push((class as usize, id, table));
    }
    if q != end {
        return None;
    }
    Some((entries, end))
}

fn parse_baseline_frame(data: &[u8], pos: usize) -> Option<(FrameInfo, usize)> {
    let len = be16(data, pos)? as usize;
    if len < 8 || pos.checked_add(len)? > data.len() {
        return None;
    }
    let p = pos + 2;
    let precision = *data.get(p)?;
    let height = be16(data, p + 1)? as u32;
    let width = be16(data, p + 3)? as u32;
    let count = *data.get(p + 5)? as usize;
    if precision != 8 || width == 0 || height == 0 || !(1..=4).contains(&count) {
        return None;
    }
    if len < 8 + 3 * count {
        return None;
    }
    let mut components = Vec::with_capacity(count);
    let mut max_h = 1;
    let mut max_v = 1;
    for i in 0..count {
        let q = p + 6 + 3 * i;
        let id = *data.get(q)?;
        let sampling = *data.get(q + 1)?;
        let h = (sampling >> 4) as u32;
        let v = (sampling & 0x0F) as u32;
        if h == 0 || v == 0 || h > 4 || v > 4 {
            return None;
        }
        max_h = max_h.max(h);
        max_v = max_v.max(v);
        components.push((id, h, v));
    }
    Some((
        FrameInfo {
            width,
            height,
            max_h,
            max_v,
            components,
        },
        pos + len,
    ))
}

fn parse_sos_header(data: &[u8], pos: usize, frame: &FrameInfo) -> Option<(ScanInfo, usize)> {
    let len = be16(data, pos)? as usize;
    if len < 3 || pos.checked_add(len)? > data.len() {
        return None;
    }
    let p = pos + 2;
    let ns = *data.get(p)? as usize;
    if ns == 0 || ns > frame.components.len() || len < 3 + 2 * ns + 3 {
        return None;
    }
    let mut components = Vec::with_capacity(ns);
    for i in 0..ns {
        let q = p + 1 + 2 * i;
        let id = *data.get(q)?;
        let tables = *data.get(q + 1)?;
        let dc = (tables >> 4) as usize;
        let ac = (tables & 0x0F) as usize;
        if dc > 3 || ac > 3 {
            return None;
        }
        components.push(ScanComponent { id, dc, ac });
    }
    let q = p + 1 + 2 * ns;
    let scan = ScanInfo {
        components,
        ss: *data.get(q)?,
        se: *data.get(q + 1)?,
        ah: *data.get(q + 2)? >> 4,
        al: *data.get(q + 2)? & 0x0F,
    };
    // Only interleaved baseline scans are modelled.
    if scan.ss != 0
        || scan.se != 63
        || scan.ah != 0
        || scan.al != 0
        || scan.components.len() != frame.components.len()
    {
        return None;
    }
    Some((scan, pos + len))
}

fn skip_jpeg_segment(data: &[u8], pos: usize) -> Option<usize> {
    let len = be16(data, pos)? as usize;
    if len < 2 || pos.checked_add(len)? > data.len() {
        return None;
    }
    Some(pos + len)
}

struct ContinuityGeometry<'a> {
    pixels: &'a [u8],
    w: usize,
    h: usize,
    stride: usize,
    components: usize,
}

fn jpeg_boundary_scores(geometry: &ContinuityGeometry<'_>) -> Option<Vec<f64>> {
    let ContinuityGeometry {
        pixels,
        w,
        h,
        stride,
        components,
    } = *geometry;
    let mut scores = Vec::new();
    let mut y = 8usize;
    while y < h {
        let row = y.checked_mul(stride)?;
        let prev = (y - 1).checked_mul(stride)?;
        let mut sum = 0u64;
        let mut n = 0u64;
        for x in 0..w {
            let base = x * components;
            let Some(a) = row.checked_add(base) else {
                y += 8;
                continue;
            };
            let Some(b) = prev.checked_add(base) else {
                y += 8;
                continue;
            };
            for c in 0..components {
                let Some(pa) = pixels.get(a + c) else {
                    continue;
                };
                let Some(pb) = pixels.get(b + c) else {
                    continue;
                };
                sum += pa.abs_diff(*pb) as u64;
                n += 1;
            }
        }
        if n > 0 {
            scores.push(sum as f64 / n as f64);
        }
        y += 8;
    }
    Some(scores)
}

fn jpeg_band_means(geometry: &ContinuityGeometry<'_>) -> Vec<f64> {
    let ContinuityGeometry {
        pixels,
        w,
        h,
        stride,
        components,
    } = *geometry;
    let mut band_means = Vec::new();
    let mut by = 0usize;
    while by < h {
        let end = (by + 8).min(h);
        let mut sum = 0u64;
        let mut n = 0u64;
        for ry in by..end {
            let row = ry * stride;
            for x in 0..w {
                let base = row + x * components;
                for c in 0..components {
                    if let Some(p) = pixels.get(base + c) {
                        sum += *p as u64;
                        n += 1;
                    }
                }
            }
        }
        if n > 0 {
            band_means.push(sum as f64 / n as f64);
        }
        by += 8;
    }
    band_means
}

/// R2-03/H-18: deterministic content-continuity check for decoded baseline
/// JPEGs. Tail swaps between compatible baseline JPEGs can produce a
/// structurally self-consistent bitstream; this check looks for an extreme
/// horizontal seam discontinuity relative to the image's own block-boundary
/// statistics. It is conservative: only strong outliers become suspect.
pub(crate) fn content_continuity_check(
    pixels: &[u8],
    width: u32,
    height: u32,
    components: usize,
) -> Option<Result<(), String>> {
    if components == 0 || width < 64 || height < 32 {
        return None;
    }
    let w = width as usize;
    let h = height as usize;
    let stride = w.checked_mul(components)?;
    if pixels.len() < stride.checked_mul(h)? {
        return None;
    }
    let geometry = ContinuityGeometry {
        pixels,
        w,
        h,
        stride,
        components,
    };
    let scores = jpeg_boundary_scores(&geometry)?;
    // 8-row band means for a DC-shift/step signal.
    let band_means = jpeg_band_means(&geometry);
    if scores.len() < 4 {
        return None;
    }
    let mut sorted = scores.clone();
    sorted.sort_by(f64::total_cmp);
    let med = median_f64(&sorted);
    let mut abs_dev: Vec<f64> = sorted.iter().map(|v| (v - med).abs()).collect();
    abs_dev.sort_by(f64::total_cmp);
    let mad = median_f64(&abs_dev);
    let max_score = scores.iter().copied().fold(0.0f64, f64::max);
    // If most boundaries are near-zero (flat/block-aligned content), the
    // median/MAD outlier test is ambiguous; be conservative.
    let mad_scale = mad.max(2.0);
    let outlier = med >= 1.0
        && max_score > med + 20.0 * mad_scale
        && max_score > med * 4.0
        && max_score >= 24.0;
    if outlier {
        let (idx, _) = scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap_or((0, &0.0));
        let boundary_y = (idx + 1) * 8;
        return Some(Err(format!(
            "content continuity outlier at row boundary {boundary_y}: score {max_score:.2}, median {med:.2}, MAD {mad:.2}"
        )));
    }
    // Secondary signal: large adjacent band mean step (DC shift). Conservative.
    if band_means.len() >= 6 {
        let mut steps = Vec::with_capacity(band_means.len().saturating_sub(1));
        for i in 1..band_means.len() {
            if let (Some(a), Some(b)) = (band_means.get(i), band_means.get(i - 1)) {
                steps.push((a - b).abs());
            }
        }
        let mut ssorted = steps.clone();
        ssorted.sort_by(f64::total_cmp);
        let smed = median_f64(&ssorted);
        let mut sdev: Vec<f64> = ssorted.iter().map(|v| (v - smed).abs()).collect();
        sdev.sort_by(f64::total_cmp);
        let smad = median_f64(&sdev).max(0.5);
        let smax = steps.iter().copied().fold(0.0f64, f64::max);
        if smax > smed + 40.0 * smad && smax >= 45.0 {
            let (idx, _) = steps
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap_or((0, &0.0));
            let boundary_y = (idx + 1) * 8;
            return Some(Err(format!(
                "content continuity band-mean step at row boundary {boundary_y}: step {smax:.2}, median {smed:.2}, MAD {smad:.2}"
            )));
        }
    }
    Some(Ok(()))
}

fn median_f64(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted.get(mid).copied().unwrap_or(0.0)
    } else {
        let a = sorted.get(mid.saturating_sub(1)).copied().unwrap_or(0.0);
        let b = sorted.get(mid).copied().unwrap_or(0.0);
        (a + b) / 2.0
    }
}

pub(crate) fn strict_baseline_scan(data: &[u8]) -> Option<Result<(), String>> {
    if data.get(0..2) != Some(&[0xFF, 0xD8]) {
        return None;
    }
    let mut pos = 2usize;
    let mut frame: Option<FrameInfo> = None;
    let mut dc_tables: [Option<HuffmanTable>; 4] = Default::default();
    let mut ac_tables: [Option<HuffmanTable>; 4] = Default::default();
    let mut restart_interval = 0u32;
    while pos + 1 < data.len() {
        if data.get(pos) != Some(&0xFF) {
            return None;
        }
        while data.get(pos) == Some(&0xFF) {
            pos += 1;
        }
        let marker = *data.get(pos)?;
        pos += 1;
        match marker {
            0xD8 => {}
            0xD9 => return None,
            0x01 | 0xD0..=0xD7 => {}
            0xC4 => {
                let (entries, end) = parse_dht_segment(data, pos)?;
                for (class, id, table) in entries {
                    if class == 0 {
                        *dc_tables.get_mut(id)? = Some(table);
                    } else {
                        *ac_tables.get_mut(id)? = Some(table);
                    }
                }
                pos = end;
            }
            0xC0 | 0xC1 => {
                let (parsed, end) = parse_baseline_frame(data, pos)?;
                frame = Some(parsed);
                pos = end;
            }
            0xDD => {
                let len = be16(data, pos)? as usize;
                if len != 4 || pos.checked_add(len)? > data.len() {
                    return None;
                }
                restart_interval = be16(data, pos + 2)? as u32;
                pos += len;
            }
            0xDA => {
                let frame = frame.as_ref()?;
                let (scan, entropy) = parse_sos_header(data, pos, frame)?;
                return Some(check_baseline_mcus(
                    data.get(entropy..)?,
                    frame,
                    &scan,
                    &dc_tables,
                    &ac_tables,
                    restart_interval,
                ));
            }
            _ => pos = skip_jpeg_segment(data, pos)?,
        }
    }
    None
}

fn be16(data: &[u8], pos: usize) -> Option<u16> {
    let b: [u8; 2] = data.get(pos..pos.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_be_bytes(b))
}

enum Marker {
    Restart(u8),
    Eoi,
    Other,
}

enum Loaded {
    Byte(u8),
    Marker(Marker),
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    current: u8,
    bits_left: u8,
    pending: Option<Marker>,
}

impl BitReader<'_> {
    fn load(&mut self) -> Option<Loaded> {
        let byte = *self.data.get(self.pos)?;
        self.pos += 1;
        if byte != 0xFF {
            return Some(Loaded::Byte(byte));
        }
        loop {
            let next = *self.data.get(self.pos)?;
            self.pos += 1;
            if next == 0xFF {
                continue;
            }
            return Some(match next {
                0x00 => Loaded::Byte(0xFF),
                0xD0..=0xD7 => Loaded::Marker(Marker::Restart(next)),
                0xD9 => Loaded::Marker(Marker::Eoi),
                _ => Loaded::Marker(Marker::Other),
            });
        }
    }

    fn read_bit(&mut self) -> Result<u8, String> {
        if self.bits_left == 0 {
            match self.load() {
                Some(Loaded::Byte(b)) => {
                    self.current = b;
                    self.bits_left = 8;
                }
                Some(Loaded::Marker(m)) => {
                    self.pending = Some(m);
                    return Err("marker before all MCUs were decoded".into());
                }
                None => return Err("entropy data ended before all MCUs".into()),
            }
        }
        self.bits_left -= 1;
        Ok((self.current >> self.bits_left) & 1)
    }

    fn read_bits(&mut self, n: u32) -> Result<u32, String> {
        if n > 16 {
            return Err("invalid coefficient bit count".into());
        }
        let mut value = 0u32;
        for _ in 0..n {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Ok(value)
    }

    fn take_marker(&mut self) -> Result<Marker, String> {
        if let Some(marker) = self.pending.take() {
            return Ok(marker);
        }
        self.bits_left = 0;
        if *self.data.get(self.pos).ok_or("missing marker")? != 0xFF {
            return Err("extra entropy data after the last MCU".into());
        }
        self.pos += 1;
        loop {
            let marker = *self.data.get(self.pos).ok_or("truncated marker")?;
            self.pos += 1;
            if marker == 0xFF {
                continue;
            }
            return Ok(match marker {
                0xD0..=0xD7 => Marker::Restart(marker),
                0xD9 => Marker::Eoi,
                _ => Marker::Other,
            });
        }
    }
}

fn check_baseline_mcus(
    entropy: &[u8],
    frame: &FrameInfo,
    scan: &ScanInfo,
    dc_tables: &[Option<HuffmanTable>; 4],
    ac_tables: &[Option<HuffmanTable>; 4],
    restart_interval: u32,
) -> Result<(), String> {
    let mcu_w = (frame.width as u64).div_ceil(8 * frame.max_h as u64);
    let mcu_h = (frame.height as u64).div_ceil(8 * frame.max_v as u64);
    let expected = mcu_w.saturating_mul(mcu_h);
    let mut reader = BitReader {
        data: entropy,
        pos: 0,
        current: 0,
        bits_left: 0,
        pending: None,
    };
    let mut decoded = 0u64;
    let mut restart_index = 0u8;
    while decoded < expected {
        if restart_interval > 0 && decoded > 0 && decoded % restart_interval as u64 == 0 {
            match reader.take_marker()? {
                Marker::Restart(marker) if marker == 0xD0 + restart_index => {
                    restart_index = (restart_index + 1) & 7;
                }
                Marker::Restart(_) => return Err("restart markers are out of sequence".into()),
                Marker::Eoi => return Err("EOI before all MCUs were decoded".into()),
                Marker::Other => return Err("unexpected marker between MCU rows".into()),
            }
        }
        for component in &scan.components {
            let (_, h, v) = frame
                .components
                .iter()
                .find(|(id, _, _)| *id == component.id)
                .ok_or("scan references a missing frame component")?;
            let dc = dc_tables
                .get(component.dc)
                .and_then(Option::as_ref)
                .ok_or("missing DC Huffman table")?;
            let ac = ac_tables
                .get(component.ac)
                .and_then(Option::as_ref)
                .ok_or("missing AC Huffman table")?;
            for _ in 0..(h * v) {
                decode_block(&mut reader, dc, ac)?;
            }
        }
        decoded += 1;
    }
    match reader.take_marker()? {
        Marker::Eoi => Ok(()),
        Marker::Restart(_) => Err("restart marker after the last MCU".into()),
        Marker::Other => Err("unexpected marker after the last MCU".into()),
    }
}

fn decode_block(
    reader: &mut BitReader<'_>,
    dc: &HuffmanTable,
    ac: &HuffmanTable,
) -> Result<(), String> {
    let category = decode_symbol(reader, dc)?;
    if category > 11 {
        return Err("DC coefficient category is too large".into());
    }
    reader.read_bits(u32::from(category))?;
    let mut k = 1u32;
    while k < 64 {
        let symbol = decode_symbol(reader, ac)?;
        let run = u32::from(symbol >> 4);
        let size = u32::from(symbol & 0x0F);
        if size == 0 {
            if run == 15 {
                k += 16;
                continue;
            }
            if run != 0 {
                return Err("invalid zero-size AC coefficient".into());
            }
            break;
        }
        k = k.checked_add(run).ok_or("AC coefficient index overflow")?;
        if k >= 64 {
            return Err("AC coefficient index runs past the block".into());
        }
        reader.read_bits(size)?;
        k += 1;
    }
    Ok(())
}

fn decode_symbol(reader: &mut BitReader<'_>, table: &HuffmanTable) -> Result<u8, String> {
    let mut code = 0u16;
    for length in 1..=16u8 {
        code = (code << 1) | u16::from(reader.read_bit()?);
        if let Some(&symbol) = table.codes.get(&(length, code)) {
            return Ok(symbol);
        }
    }
    Err("invalid Huffman code in entropy data".into())
}

#[cfg(test)]
// Test fixtures use fixed slices and expect valid layouts; the production
// parsers above remain subject to the deny list.
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn synthetic() -> Vec<u8> {
        let mut d = vec![0xFF, 0xD8];
        d.extend(seg(0xE0, &[0u8; 14]));
        let mut sof = vec![8u8, 0x01, 0xE0, 0x02, 0x80, 3];
        sof.extend_from_slice(&[1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        d.extend(seg(0xC0, &sof));
        let sos = [3u8, 1, 0, 2, 0x11, 3, 0x11, 0, 63, 0];
        d.extend(seg(0xDA, &sos));
        d.extend_from_slice(&[0x12, 0x34, 0xFF, 0x00, 0x56]);
        d.extend_from_slice(EOI);
        d
    }

    #[test]
    fn parses_baseline_jpeg() {
        let d = synthetic();
        match analyze(&d, true) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert_eq!(a.end, d.len());
                assert_eq!(a.width, Some(640));
                assert_eq!(a.height, Some(480));
                assert_eq!(a.score, 100);
            }
            _ => panic!("expected complete JPEG"),
        }
    }

    #[test]
    fn ignores_exif_thumbnail_eoi() {
        let mut d = vec![0xFF, 0xD8];
        let mut app1 = vec![0x45, 0x78, 0x69, 0x66, 0, 0];
        app1.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xD9, 1, 2, 3]);
        d.extend(seg(0xE1, &app1));
        d.extend(synthetic().into_iter().skip(2));
        match analyze(&d, true) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert_eq!(a.end, d.len());
            }
            _ => panic!("expected complete JPEG"),
        }
    }

    #[test]
    fn handles_progressive_second_scan() {
        let mut d = vec![0xFF, 0xD8];
        let mut sof = vec![8u8, 0x01, 0xE0, 0x02, 0x80, 3];
        sof.extend_from_slice(&[1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        d.extend(seg(0xC2, &sof));
        let sos = [3u8, 1, 0, 2, 0x11, 3, 0x11, 0, 63, 0];
        d.extend(seg(0xDA, &sos));
        d.extend_from_slice(&[1, 2, 3]);
        // H-25: a real DHT has a class/id byte, 16 count bytes and exactly
        // that many symbol bytes. An empty table is a valid minimal fixture.
        d.extend(seg(0xC4, &[0u8; 17]));
        d.extend(seg(0xDA, &sos));
        d.extend_from_slice(&[4, 5, 6]);
        d.extend_from_slice(EOI);
        match analyze(&d, true) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert_eq!(a.end, d.len());
                assert!(a.notes[0].contains("2 scan"));
            }
            _ => panic!("expected progressive JPEG"),
        }
    }

    #[test]
    fn truncated_jpeg_requests_more_or_reports() {
        let d = synthetic();
        let cut = &d[..d.len() - 2];
        assert!(matches!(analyze(cut, false), Need::More));
        match analyze(cut, true) {
            Need::Analyzed(a) => {
                assert!(!a.complete);
                assert_eq!(a.score, 60);
                assert_eq!(a.repair_suffix, EOI);
            }
            _ => panic!("expected truncated analysis"),
        }
    }

    #[test]
    fn h18_strict_scan_rejects_extra_entropy_before_eoi() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/bench/corpus/jpeg/jpeg_01_baseline_640x480.jpg"
        );
        let original = std::fs::read(path).unwrap();
        assert!(strict_baseline_scan(&original).is_some_and(|r| r.is_ok()));
        let split = original.len() - EOI.len();
        let mut mutated = original[..split].to_vec();
        mutated.push(0x00);
        mutated.extend_from_slice(EOI);
        match strict_baseline_scan(&mutated) {
            Some(Err(reason)) => assert!(
                reason.contains("after the last MCU") || reason.contains("padding"),
                "reason: {reason}"
            ),
            other => panic!("extra entropy byte was accepted: {other:?}"),
        }
    }

    #[test]
    fn h18_strict_scan_rejects_truncated_entropy() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/bench/corpus/jpeg/jpeg_01_baseline_640x480.jpg"
        );
        let original = std::fs::read(path).unwrap();
        let split = original.len() - EOI.len();
        let mut truncated = original[..split].to_vec();
        truncated.extend_from_slice(EOI);
        truncated.truncate(truncated.len() - 1024);
        assert!(strict_baseline_scan(&truncated).is_some_and(|r| r.is_err()));
    }

    #[test]
    fn rejects_bogus_header() {
        let d = [0xFF, 0xD8, 0xFF, 0x00, 0x01, 0x02];
        assert!(matches!(analyze(&d, true), Need::Reject(_)));
    }
}
