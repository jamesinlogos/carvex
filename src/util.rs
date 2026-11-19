use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Hash a byte slice with SHA-256 and return lowercase hexadecimal.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex(&h.finalize())
}

/// Encode bytes as lowercase hexadecimal.
pub fn hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// Format a byte count for humans, using binary units.
pub fn human_size(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < UNITS.len() {
        v /= 1024.0;
        i += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}

/// Format a duration in milliseconds for humans.
pub fn human_duration(ms: u128) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.2} s", ms as f64 / 1000.0)
    } else {
        let m = ms / 60_000;
        let s = (ms % 60_000) as f64 / 1000.0;
        format!("{m} min {s:.1} s")
    }
}

/// Current Unix time in seconds.
pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Current UTC time as an ISO-8601 string with second precision.
pub fn iso8601_utc(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// Convert days since 1970-01-01 to a proleptic Gregorian date.
///
/// This is the well-known Howard Hinnant algorithm; it avoids pulling in a
/// date/time crate for one timestamp field.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + i64::from(m <= 2);
    (y, m as u32, d as u32)
}

/// Parse a size with optional binary suffix (`K`, `KiB`, `M`, `MiB`, …).
pub fn parse_size(text: &str) -> Result<u64, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("empty size".to_string());
    }
    let digits_end = t
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '_')
        .unwrap_or(t.len());
    let num = t[..digits_end].replace('_', "");
    let suffix = t[digits_end..].trim().to_ascii_lowercase();
    let value: f64 = num
        .parse()
        .map_err(|_| format!("invalid number in size: {text}"))?;
    let mult = match suffix.as_str() {
        "" | "b" => 1.0,
        "k" | "kb" | "kib" => 1024.0,
        "m" | "mb" | "mib" => 1024.0 * 1024.0,
        "g" | "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return Err(format!("unknown size suffix '{suffix}'")),
    };
    let out = value * mult;
    if !out.is_finite() || out < 0.0 {
        return Err(format!("invalid size: {text}"));
    }
    Ok(out.round() as u64)
}

/// Return file size and mtime (Unix seconds) without opening the file.
pub fn file_identity(path: &Path) -> Result<(u64, Option<u64>)> {
    let meta =
        std::fs::metadata(path).with_context(|| format!("cannot stat '{}'", path.display()))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    Ok((meta.len(), mtime))
}

/// Merge overlapping or physically adjacent extents (sorted by offset).
pub fn merge_extents(mut extents: Vec<Extent>) -> Vec<Extent> {
    extents.retain(|e| e.len > 0);
    extents.sort_by_key(|e| e.offset);
    let mut out: Vec<Extent> = Vec::with_capacity(extents.len());
    for e in extents {
        if let Some(last) = out.last_mut() {
            if e.offset <= last.offset.saturating_add(last.len) {
                let end = last
                    .offset
                    .saturating_add(last.len)
                    .max(e.offset.saturating_add(e.len));
                last.len = end.saturating_sub(last.offset);
                continue;
            }
        }
        out.push(e);
    }
    out
}

/// Complement of `free` within `[start, start + len)`.
pub fn complement_extents(start: u64, len: u64, free: &[Extent]) -> Vec<Extent> {
    let end = start.saturating_add(len);
    let mut out = Vec::new();
    let mut cur = start;
    for e in free {
        let e_end = e.offset.saturating_add(e.len).min(end);
        if e.offset > cur {
            out.push(Extent {
                offset: cur,
                len: e.offset.min(end).saturating_sub(cur),
            });
        }
        cur = cur.max(e_end);
        if cur >= end {
            break;
        }
    }
    if cur < end {
        out.push(Extent {
            offset: cur,
            len: end.saturating_sub(cur),
        });
    }
    out
}

/// A half-open byte range `[offset, offset + len)`.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Extent {
    /// Absolute byte offset in the image.
    pub offset: u64,
    /// Length in bytes.
    pub len: u64,
}

impl Extent {
    /// Exclusive end offset, saturating on overflow.
    pub fn end(&self) -> u64 {
        self.offset.saturating_add(self.len)
    }

    /// Whether `offset` lies inside this extent.
    pub fn contains(&self, offset: u64) -> bool {
        offset >= self.offset && offset < self.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert_eq!(parse_size("4K").unwrap(), 4096);
        assert_eq!(parse_size("1.5 MiB").unwrap(), 1_572_864);
        assert!(parse_size("nope").is_err());
    }

    #[test]
    fn formats_iso_time() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn merges_and_complements() {
        let m = merge_extents(vec![
            Extent { offset: 10, len: 0 },
            Extent { offset: 0, len: 10 },
            Extent { offset: 20, len: 5 },
            Extent {
                offset: 10,
                len: 10,
            },
        ]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].offset, 0);
        assert_eq!(m[0].len, 25);

        let c = complement_extents(
            0,
            100,
            &[Extent {
                offset: 10,
                len: 20,
            }],
        );
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].offset, 0);
        assert_eq!(c[0].len, 10);
        assert_eq!(c[1].offset, 30);
        assert_eq!(c[1].len, 70);
    }
}
