#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
use super::{Analysis, Need};

const EOI: &[u8] = &[0xFF, 0xD9];

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
    let mut sof = false;
    let mut sos = false;
    let mut scans = 0u32;
    let mut app_segments = 0u32;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut sof_name = String::new();

    loop {
        if pos >= data.len() {
            return truncated(
                data.len(),
                at_limit,
                sof,
                sos,
                width,
                height,
                "end of buffer",
            );
        }
        if data.get(pos) != Some(&0xFF) {
            return Need::Reject(format!("expected JPEG marker at offset {pos}"));
        }
        while data.get(pos) == Some(&0xFF) {
            pos += 1;
        }
        if pos >= data.len() {
            return truncated(
                data.len(),
                at_limit,
                sof,
                sos,
                width,
                height,
                "truncated marker",
            );
        }
        let Some(&marker) = data.get(pos) else {
            return truncated(
                data.len(),
                at_limit,
                sof,
                sos,
                width,
                height,
                "truncated marker",
            );
        };
        pos += 1;

        match marker {
            0x00 => return Need::Reject("invalid JPEG marker 0x00 outside entropy data".into()),
            0xD8 => return Need::Reject("nested JPEG SOI".into()),
            0xD9 => {
                if !sof {
                    return Need::Reject("JPEG EOI before any SOF".into());
                }
                if !sos {
                    return Need::Reject("JPEG EOI before any SOS".into());
                }
                return Need::Analyzed(Analysis {
                    end: pos,
                    complete: true,
                    score: 100,
                    width: Some(width),
                    height: Some(height),
                    notes: vec![format!(
                        "{} {}x{}, {} scan(s), {} APP segment(s)",
                        sof_name, width, height, scans, app_segments
                    )],
                    repair_suffix: EOI,
                    integrity_checks: Vec::new(),
                    validated_prefix: 0,
                });
            }
            // Standalone markers (TEM, RSTn).
            0x01 | 0xD0..=0xD7 => continue,
            _ => {}
        }

        if pos + 2 > data.len() {
            return truncated(
                data.len(),
                at_limit,
                sof,
                sos,
                width,
                height,
                "truncated segment length",
            );
        }
        let Some(seg_len) = be16(data, pos) else {
            return truncated(
                data.len(),
                at_limit,
                sof,
                sos,
                width,
                height,
                "truncated segment length",
            );
        };
        let seg_len = seg_len as usize;
        if seg_len < 2 {
            return Need::Reject(format!("invalid JPEG segment length at offset {pos}"));
        }
        let seg_end = match pos.checked_add(seg_len) {
            Some(v) if v <= data.len() => v,
            _ => {
                return truncated(
                    data.len(),
                    at_limit,
                    sof,
                    sos,
                    width,
                    height,
                    "truncated segment payload",
                )
            }
        };

        if is_sof(marker) {
            if seg_len < 8 {
                return Need::Reject("SOF segment too short".into());
            }
            let (Some(h), Some(w)) = (be16(data, pos + 3), be16(data, pos + 5)) else {
                return truncated(
                    data.len(),
                    at_limit,
                    sof,
                    sos,
                    width,
                    height,
                    "truncated SOF dimensions",
                );
            };
            height = h as u32;
            width = w as u32;
            if width == 0 || height == 0 {
                return Need::Reject("JPEG SOF has zero dimensions".into());
            }
            sof = true;
            sof_name = sof_marker_name(marker).to_string();
        }

        if marker == 0xDA {
            sos = true;
            scans += 1;
            pos = seg_end;
            // Walk entropy-coded data. Stuffed 0x00 and RSTn are data; EOI ends
            // the image; any other marker returns to segment parsing (needed for
            // progressive JPEG, where DHT/DQT/SOS repeat).
            loop {
                let rel = match memchr::memchr(0xFF, data.get(pos..).unwrap_or_default()) {
                    Some(v) => v,
                    None => {
                        return truncated(
                            data.len(),
                            at_limit,
                            sof,
                            sos,
                            width,
                            height,
                            "end of entropy-coded data",
                        )
                    }
                };
                let mut q = pos + rel;
                while data.get(q + 1) == Some(&0xFF) {
                    q += 1;
                }
                if q + 1 >= data.len() {
                    return truncated(
                        data.len(),
                        at_limit,
                        sof,
                        sos,
                        width,
                        height,
                        "truncated entropy marker",
                    );
                }
                let Some(&code) = data.get(q + 1) else {
                    return truncated(
                        data.len(),
                        at_limit,
                        sof,
                        sos,
                        width,
                        height,
                        "truncated entropy marker",
                    );
                };
                match code {
                    0x00 | 0xD0..=0xD7 | 0x01 => pos = q + 2,
                    0xD9 => {
                        if !sof {
                            return Need::Reject("JPEG EOI before SOF".into());
                        }
                        return Need::Analyzed(Analysis {
                            end: q + 2,
                            complete: true,
                            score: 100,
                            width: Some(width),
                            height: Some(height),
                            notes: vec![format!(
                                "{} {}x{}, {} scan(s), {} APP segment(s)",
                                sof_name, width, height, scans, app_segments
                            )],
                            repair_suffix: EOI,
                            integrity_checks: Vec::new(),
                            validated_prefix: 0,
                        });
                    }
                    _ => {
                        // DHT/DQT/DNL or the next SOS: parse it as a segment.
                        pos = q;
                        break;
                    }
                }
            }
            continue;
        }

        if (0xE0..=0xEF).contains(&marker) || marker == 0xFE {
            app_segments += 1;
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
                let len = be16(data, pos)? as usize;
                if len < 2 || pos.checked_add(len)? > data.len() {
                    return None;
                }
                let end = pos + len;
                let mut q = pos + 2;
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
                    if class == 0 {
                        *dc_tables.get_mut(id)? = Some(table);
                    } else {
                        *ac_tables.get_mut(id)? = Some(table);
                    }
                }
                if q != end {
                    return None;
                }
                pos = end;
            }
            0xC0 | 0xC1 => {
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
                frame = Some(FrameInfo {
                    width,
                    height,
                    max_h,
                    max_v,
                    components,
                });
                pos += len;
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
                let entropy = pos + len;
                return Some(check_baseline_mcus(
                    data.get(entropy..)?,
                    frame,
                    &scan,
                    &dc_tables,
                    &ac_tables,
                    restart_interval,
                ));
            }
            _ => {
                let len = be16(data, pos)? as usize;
                if len < 2 || pos.checked_add(len)? > data.len() {
                    return None;
                }
                pos += len;
            }
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
        d.extend(seg(0xC4, &[0, 0]));
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
    fn rejects_bogus_header() {
        let d = [0xFF, 0xD8, 0xFF, 0x00, 0x01, 0x02];
        assert!(matches!(analyze(&d, true), Need::Reject(_)));
    }
}
