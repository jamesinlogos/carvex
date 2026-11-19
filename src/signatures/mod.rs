//! Signature definitions and structural validators.
//!
//! The registry below is the single source of truth for `--types`,
//! `carvex signatures`, header search and extension naming.  Adding a format
//! means adding one [`FormatSpec`] and one validator module.

#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
pub mod jpeg;
pub mod pdf;
pub mod png;

use clap::ValueEnum;
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    #[value(alias = "jpg")]
    Jpeg,
    #[value(alias = "png")]
    Png,
    #[value(alias = "pdf")]
    Pdf,
}

impl FileType {
    pub fn ext(self) -> &'static str {
        self.spec().ext
    }

    pub fn label(self) -> &'static str {
        self.spec().label
    }

    pub fn header(self) -> &'static [u8] {
        self.spec().header
    }

    pub fn spec(self) -> &'static FormatSpec {
        FORMATS
            .iter()
            .find(|f| f.file_type == self)
            .unwrap_or(&FORMATS[0])
    }

    pub fn all() -> [FileType; 3] {
        [FileType::Jpeg, FileType::Png, FileType::Pdf]
    }
}

impl fmt::Display for FileType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// One supported format.  This is intentionally data, not code, so the CLI and
/// documentation cannot drift away from the parsers.
pub struct FormatSpec {
    pub file_type: FileType,
    pub ext: &'static str,
    pub label: &'static str,
    pub header: &'static [u8],
    pub footer: &'static str,
    pub validation: &'static str,
}

pub const FORMATS: [FormatSpec; 3] = [
    FormatSpec {
        file_type: FileType::Jpeg,
        ext: "jpg",
        label: "JPEG",
        header: &[0xFF, 0xD8, 0xFF],
        footer: "FF D9",
        validation: "marker/segment walk, SOF, SOS, EOI",
    },
    FormatSpec {
        file_type: FileType::Png,
        ext: "png",
        label: "PNG",
        header: &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        footer: "49 45 4E 44 AE 42 60 82",
        validation: "chunk walk, IHDR/IDAT/IEND, CRC-32",
    },
    FormatSpec {
        file_type: FileType::Pdf,
        ext: "pdf",
        label: "PDF",
        header: b"%PDF-",
        footer: "%%EOF",
        validation: "xref/startxref, trailer /Root, revisions",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum PdfEofMode {
    First,
    Last,
    #[default]
    Smart,
}

impl fmt::Display for PdfEofMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PdfEofMode::First => write!(f, "first"),
            PdfEofMode::Last => write!(f, "last"),
            PdfEofMode::Smart => write!(f, "smart"),
        }
    }
}

/// How much validation the scanner performs before writing a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum ValidationMode {
    /// Structural validation only (fast, default, backwards compatible).
    #[default]
    Structure,
    /// Additionally decode the payload to detect spliced or corrupt files.
    Decode,
}

/// Result of validation, stored in the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Validation {
    /// Structure is complete but payload was not decoded.
    Structure,
    /// The payload decoded successfully.
    Decoded,
    /// Only part of the payload could be validated.
    Partial,
    /// No validator was run.
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Complete,
    Truncated,
    Repaired,
}

#[derive(Debug, Clone)]
pub struct Analysis {
    /// Number of input bytes that belong to the file (before any repair suffix).
    pub end: usize,
    pub complete: bool,
    pub score: u8,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub notes: Vec<String>,
    /// Appended when --repair is enabled and the file is truncated.
    pub repair_suffix: &'static [u8],
}

impl Analysis {
    pub fn status(&self, repaired: bool) -> Status {
        if self.complete {
            Status::Complete
        } else if repaired {
            Status::Repaired
        } else {
            Status::Truncated
        }
    }
}

pub enum Need {
    Analyzed(Analysis),
    /// More bytes are required because the parser reached the current read limit
    /// while the footer may still lie ahead.
    More,
    /// The header matched but the structure is invalid (false positive).
    Reject(String),
}

/// Dispatch to the validator for `kind`.
pub fn analyze(kind: FileType, data: &[u8], at_limit: bool, pdf_mode: PdfEofMode) -> Need {
    match kind {
        FileType::Jpeg => jpeg::analyze(data, at_limit),
        FileType::Png => png::analyze(data, at_limit),
        FileType::Pdf => pdf::analyze(data, at_limit, pdf_mode),
    }
}

/// Run optional decode-level validation.
///
/// This is intentionally separate from `analyze`: structural carving remains
/// fast and predictable, while `--validate decode` pays for a stronger check.
/// A failure is reported as [`Validation::Partial`] rather than silently
/// relabelling the file as complete.
pub fn validate_deep(
    kind: FileType,
    data: &[u8],
    analysis: &Analysis,
) -> (Validation, Vec<String>) {
    match kind {
        FileType::Jpeg => validate_jpeg(data, analysis),
        FileType::Png => validate_png(data, analysis),
        FileType::Pdf => validate_pdf(data, analysis),
    }
}

fn validate_jpeg(data: &[u8], analysis: &Analysis) -> (Validation, Vec<String>) {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(data));
    match decoder.decode() {
        Ok(pixels) => {
            let info = decoder.info();
            let (w, h) = info
                .map(|i| (u32::from(i.width), u32::from(i.height)))
                .unwrap_or((0, 0));
            if let (Some(ew), Some(eh)) = (analysis.width, analysis.height) {
                if w != 0 && h != 0 && (w != ew || h != eh) {
                    return (
                        Validation::Partial,
                        vec![format!(
                            "decoded dimensions {w}x{h} do not match SOF {ew}x{eh}"
                        )],
                    );
                }
            }
            if pixels.is_empty() {
                return (
                    Validation::Partial,
                    vec!["JPEG decoder returned no pixels".into()],
                );
            }
            if let Some(Err(reason)) = jpeg::strict_baseline_scan(data) {
                return (
                    Validation::Partial,
                    vec![format!("baseline scan does not match the frame: {reason}")],
                );
            }
            (
                Validation::Decoded,
                vec![format!("decoded {} bytes of pixel data", pixels.len())],
            )
        }
        Err(e) => (
            Validation::Partial,
            vec![format!("JPEG decode failed: {e}")],
        ),
    }
}

fn validate_png(data: &[u8], analysis: &Analysis) -> (Validation, Vec<String>) {
    let Some((w, h)) = analysis.width.zip(analysis.height) else {
        return (
            Validation::Partial,
            vec!["PNG IHDR dimensions are unavailable".into()],
        );
    };
    let mut pos = 8usize;
    let mut idat = Vec::new();
    let mut depth = 0u8;
    let mut color_type = 0u8;
    let mut interlace = 0u8;
    let mut saw_iend = false;
    while pos + 12 <= data.len() {
        let Some(len_bytes) = data.get(pos..pos + 4) else {
            break;
        };
        let Ok(len_arr) = <[u8; 4]>::try_from(len_bytes) else {
            break;
        };
        let len = u32::from_be_bytes(len_arr) as usize;
        let Some(ctype) = data.get(pos + 4..pos + 8) else {
            break;
        };
        let Some(chunk_end) = pos
            .checked_add(12)
            .and_then(|v| v.checked_add(len))
            .filter(|&v| v <= data.len())
        else {
            break;
        };
        let Some(chunk_data) = data.get(pos + 8..pos + 8 + len) else {
            break;
        };
        match ctype {
            b"IHDR" if chunk_data.len() >= 13 => {
                let (Some(&d), Some(&ct), Some(&i)) =
                    (chunk_data.get(8), chunk_data.get(9), chunk_data.get(12))
                else {
                    break;
                };
                depth = d;
                color_type = ct;
                interlace = i;
            }
            b"IDAT" => idat.extend_from_slice(chunk_data),
            b"IEND" => {
                saw_iend = true;
                break;
            }
            _ => {}
        }
        pos = chunk_end;
    }
    if !saw_iend {
        return (
            Validation::Partial,
            vec!["PNG decode stopped before IEND".into()],
        );
    }
    let Some(expected) = expected_png_raw_size(w, h, depth, color_type, interlace) else {
        return (
            Validation::Partial,
            vec!["unsupported PNG geometry for decode validation".into()],
        );
    };
    let mut decoded = Vec::new();
    let mut decoder = flate2::read::ZlibDecoder::new(idat.as_slice());
    if let Err(e) = std::io::Read::read_to_end(&mut decoder, &mut decoded) {
        return (
            Validation::Partial,
            vec![format!("PNG IDAT inflate failed: {e}")],
        );
    }
    if decoded.len() != expected {
        return (
            Validation::Partial,
            vec![format!(
                "PNG decoded {} bytes, expected {}",
                decoded.len(),
                expected
            )],
        );
    }
    for row in png_rows(w, h, depth, color_type, interlace) {
        if row >= decoded.len() {
            break;
        }
        if decoded.get(row).is_some_and(|&b| b > 4) {
            return (
                Validation::Partial,
                vec![format!("PNG row at {row} has invalid filter byte")],
            );
        }
    }
    (
        Validation::Decoded,
        vec![format!("inflated {expected} bytes of PNG image data")],
    )
}

fn validate_pdf(data: &[u8], analysis: &Analysis) -> (Validation, Vec<String>) {
    let selected = data.get(..analysis.end).unwrap_or(data);
    let has_eof = selected.ends_with(b"%%EOF") || selected.ends_with(b"%%EOF\n");
    if !has_eof {
        return (
            Validation::Partial,
            vec!["selected PDF revision does not end with %%EOF".into()],
        );
    }
    if memchr::memmem::find(selected, b"/Root").is_none() {
        return (Validation::Partial, vec!["PDF has no /Root catalog".into()]);
    }
    if analysis
        .notes
        .iter()
        .any(|n| n.contains("target validated"))
    {
        (
            Validation::Decoded,
            vec!["xref/startxref target validated".into()],
        )
    } else {
        (
            Validation::Partial,
            vec!["PDF footer present but xref target was not validated".into()],
        )
    }
}

fn png_rows(width: u32, height: u32, depth: u8, color_type: u8, interlace: u8) -> Vec<usize> {
    let Some(bpp) = png_bits_per_pixel(depth, color_type) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut offset = 0usize;
    if interlace == 1 {
        for (x0, y0, dx, dy) in png_passes() {
            let pw = pass_dim(width, x0, dx);
            let ph = pass_dim(height, y0, dy);
            let row_bytes = (pw as usize * bpp as usize).div_ceil(8);
            for _ in 0..ph {
                rows.push(offset);
                offset += 1 + row_bytes;
            }
        }
    } else {
        let row_bytes = (width as usize * bpp as usize).div_ceil(8);
        for _ in 0..height {
            rows.push(offset);
            offset += 1 + row_bytes;
        }
    }
    rows
}

fn expected_png_raw_size(
    width: u32,
    height: u32,
    depth: u8,
    color_type: u8,
    interlace: u8,
) -> Option<usize> {
    let bpp = png_bits_per_pixel(depth, color_type)?;
    let mut total = 0usize;
    if interlace == 1 {
        for (x0, y0, dx, dy) in png_passes() {
            let pw = pass_dim(width, x0, dx);
            let ph = pass_dim(height, y0, dy);
            let row_bytes = (pw as usize * bpp as usize).div_ceil(8);
            let row = 1usize.checked_add(row_bytes)?;
            total = total.checked_add((ph as usize).checked_mul(row)?)?;
        }
    } else {
        let row_bytes = (width as usize * bpp as usize).div_ceil(8);
        let row = 1usize.checked_add(row_bytes)?;
        total = (height as usize).checked_mul(row)?;
    }
    Some(total)
}

fn png_bits_per_pixel(depth: u8, color_type: u8) -> Option<u8> {
    let channels = match color_type {
        0 | 3 => 1u8,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    depth.checked_mul(channels)
}

fn pass_dim(full: u32, start: u32, step: u32) -> u32 {
    if full <= start {
        0
    } else {
        (full - start).div_ceil(step)
    }
}

fn png_passes() -> [(u32, u32, u32, u32); 7] {
    [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ]
}

/// Print the format registry.
pub fn print_signatures() {
    println!("CarveX built-in signatures and structural validators\n");
    println!(
        "{:<6} {:<5} {:<28} {:<22} VALIDATION",
        "TYPE", "EXT", "HEADER", "FOOTER"
    );
    println!("{:-<6} {:-<5} {:-<28} {:-<22} {:-<30}", "", "", "", "", "");
    for spec in &FORMATS {
        let header = spec
            .header
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "{:<6} .{:<4} {:<28} {:<22} {}",
            spec.label, spec.ext, header, spec.footer, spec.validation
        );
    }
    println!();
    println!("Validation rejects headers whose file structure cannot be parsed, which");
    println!("suppresses most accidental header matches in unallocated space.");
}

#[cfg(test)]
// Test fixtures use fixed slices and expect valid layouts; the production
// validator above remains subject to the deny list.
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_every_type() {
        for kind in FileType::all() {
            let spec = kind.spec();
            assert_eq!(spec.file_type, kind);
            assert!(!spec.header.is_empty());
        }
    }

    #[test]
    fn png_expected_size_for_1x1_rgba() {
        assert_eq!(expected_png_raw_size(1, 1, 8, 6, 0), Some(5));
    }
}
