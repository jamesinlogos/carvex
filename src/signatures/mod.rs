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
use flate2::{Decompress, FlushDecompress, Status as InflateStatus};
use serde::Serialize;
use std::fmt;

/// Default upper bound on the decompressed size accepted by PNG decode
/// validation.  The validator streams inflation, so this bounds work and the
/// largest image that `--validate decode` will claim to have validated.
pub const DEFAULT_MAX_PNG_DECODE_BYTES: u64 = 256 * 1024 * 1024;

/// Resource limits for optional deep validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationLimits {
    /// Maximum expected PNG raw size (filter bytes plus scanline data).
    pub max_png_decoded_bytes: u64,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            max_png_decoded_bytes: DEFAULT_MAX_PNG_DECODE_BYTES,
        }
    }
}

impl ValidationLimits {
    /// Build limits from a configured byte budget.  `0` selects the default
    /// PNG decode bound rather than disabling the guard.
    pub fn new(max_png_decoded_bytes: u64) -> Self {
        Self {
            max_png_decoded_bytes: if max_png_decoded_bytes == 0 {
                DEFAULT_MAX_PNG_DECODE_BYTES
            } else {
                max_png_decoded_bytes
            },
        }
    }
}

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
    validate_deep_with_limits(kind, data, analysis, ValidationLimits::default())
}

/// Run optional decode-level validation with explicit resource limits.
pub fn validate_deep_with_limits(
    kind: FileType,
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
) -> (Validation, Vec<String>) {
    match kind {
        FileType::Jpeg => validate_jpeg(data, analysis),
        FileType::Png => validate_png(data, analysis, limits),
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
            match jpeg::strict_baseline_scan(data) {
                Some(Err(reason)) => {
                    return (
                        Validation::Partial,
                        vec![format!("baseline scan does not match the frame: {reason}")],
                    );
                }
                Some(Ok(())) => {}
                None => {
                    return (
                        Validation::Unverified,
                        vec![
                            "JPEG strict scan unsupported for this variant; content not checked"
                                .into(),
                        ],
                    );
                }
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

fn validate_png(
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
) -> (Validation, Vec<String>) {
    let Some((w, h)) = analysis.width.zip(analysis.height) else {
        return (
            Validation::Partial,
            vec!["PNG IHDR dimensions are unavailable".into()],
        );
    };
    let max_decoded = if limits.max_png_decoded_bytes == 0 {
        DEFAULT_MAX_PNG_DECODE_BYTES
    } else {
        limits.max_png_decoded_bytes
    };
    let mut pos = 8usize;
    let mut saw_ihdr = false;
    let mut saw_iend = false;
    let mut bad_crcs: Vec<String> = Vec::new();
    let mut inflate: Option<PngInflate> = None;
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
        let stored_crc = data
            .get(pos + 8 + len..pos + 12 + len)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_be_bytes);
        let computed_crc = {
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(ctype);
            hasher.update(chunk_data);
            hasher.finalize()
        };
        let crc_ok = stored_crc == Some(computed_crc);
        if !crc_ok {
            bad_crcs.push(format!(
                "PNG chunk {} has CRC mismatch",
                String::from_utf8_lossy(ctype)
            ));
        }
        match ctype {
            b"IHDR" if chunk_data.len() >= 13 => {
                if saw_ihdr {
                    return (
                        Validation::Partial,
                        vec!["duplicate PNG IHDR during decode validation".into()],
                    );
                }
                saw_ihdr = true;
                let (Some(&depth), Some(&color_type), Some(&interlace)) =
                    (chunk_data.get(8), chunk_data.get(9), chunk_data.get(12))
                else {
                    break;
                };
                let Some(expected) = expected_png_raw_size(w, h, depth, color_type, interlace)
                else {
                    return (
                        Validation::Partial,
                        vec!["unsupported PNG geometry for decode validation".into()],
                    );
                };
                if expected as u64 > max_decoded {
                    return (
                        Validation::Partial,
                        vec![format!(
                            "PNG decoded size {expected} exceeds the \
                             decode-validation budget of {max_decoded} bytes"
                        )],
                    );
                }
                let Some(bpp) = png_bits_per_pixel(depth, color_type) else {
                    return (
                        Validation::Partial,
                        vec!["unsupported PNG color type for decode validation".into()],
                    );
                };
                // The budget is checked before any IDAT byte is inflated and
                // before a row cursor or output buffer is created.
                inflate = Some(PngInflate::new(
                    expected,
                    PngRowCursor::new(w, h, bpp, interlace),
                ));
            }
            b"IDAT" => {
                let Some(state) = inflate.as_mut() else {
                    return (
                        Validation::Partial,
                        vec!["PNG IDAT appears before IHDR".into()],
                    );
                };
                if let Err(reason) = state.feed(chunk_data) {
                    return (Validation::Partial, vec![reason]);
                }
            }
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
    let Some(mut state) = inflate else {
        return (
            Validation::Partial,
            vec!["PNG decode validation found no IDAT data".into()],
        );
    };
    if let Err(reason) = state.finish_and_check() {
        return (Validation::Partial, vec![reason]);
    }
    if !bad_crcs.is_empty() {
        let mut notes = bad_crcs;
        notes.push(format!(
            "inflated {} bytes of PNG image data but chunk CRCs failed",
            state.expected
        ));
        return (Validation::Partial, notes);
    }
    (
        Validation::Decoded,
        vec![format!(
            "inflated {} bytes of PNG image data",
            state.expected
        )],
    )
}

const PNG_INFLATE_BUFFER: usize = 64 * 1024;

/// Bounded, incremental PNG inflater.  It never materializes the complete
/// decompressed image: compressed IDAT bytes are consumed chunk by chunk and
/// output is checked against the IHDR row layout in a fixed-size buffer.
struct PngInflate {
    inflater: Decompress,
    rows: PngRowCursor,
    expected: usize,
    produced: u64,
    stream_ended: bool,
    out: Vec<u8>,
}

impl PngInflate {
    fn new(expected: usize, rows: PngRowCursor) -> Self {
        Self {
            inflater: Decompress::new(true),
            rows,
            expected,
            produced: 0,
            stream_ended: false,
            out: vec![0u8; PNG_INFLATE_BUFFER],
        }
    }

    fn feed(&mut self, mut input: &[u8]) -> Result<(), String> {
        while !input.is_empty() {
            if self.stream_ended {
                return Err("PNG has IDAT data after the zlib stream ended".into());
            }
            let before_in = self.inflater.total_in();
            let before_out = self.inflater.total_out();
            let status = self
                .inflater
                .decompress(input, &mut self.out, FlushDecompress::None)
                .map_err(|e| format!("PNG IDAT inflate failed: {e}"))?;
            let consumed =
                usize::try_from(self.inflater.total_in() - before_in).unwrap_or(usize::MAX);
            let written =
                usize::try_from(self.inflater.total_out() - before_out).unwrap_or(usize::MAX);
            input = input.get(consumed..).unwrap_or(&[]);
            self.accept_output(written)?;
            if status == InflateStatus::StreamEnd {
                self.stream_ended = true;
                if !input.is_empty() {
                    return Err("PNG has IDAT data after the zlib stream ended".into());
                }
                break;
            }
            if consumed == 0 && written == 0 {
                return Err("PNG IDAT inflate made no progress".into());
            }
        }
        Ok(())
    }

    fn accept_output(&mut self, written: usize) -> Result<(), String> {
        if written == 0 {
            return Ok(());
        }
        let Some(chunk) = self.out.get(..written) else {
            return Err("PNG inflate produced an invalid output span".into());
        };
        self.rows.consume(chunk)?;
        self.produced = self.produced.saturating_add(written as u64);
        if self.produced > self.expected as u64 {
            return Err(format!(
                "PNG decoded more than the expected {} bytes",
                self.expected
            ));
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        if self.stream_ended {
            return Ok(());
        }
        loop {
            let before_out = self.inflater.total_out();
            let status = self
                .inflater
                .decompress(&[], &mut self.out, FlushDecompress::Finish)
                .map_err(|e| format!("PNG IDAT inflate failed: {e}"))?;
            let written =
                usize::try_from(self.inflater.total_out() - before_out).unwrap_or(usize::MAX);
            self.accept_output(written)?;
            if status == InflateStatus::StreamEnd {
                self.stream_ended = true;
                return Ok(());
            }
            if written == 0 {
                return Err("PNG zlib stream did not terminate".into());
            }
        }
    }

    fn finish_and_check(&mut self) -> Result<(), String> {
        self.finish()?;
        if !self.stream_ended {
            return Err("PNG zlib stream did not end".into());
        }
        if self.produced != self.expected as u64 {
            return Err(format!(
                "PNG decoded {} bytes, expected {}",
                self.produced, self.expected
            ));
        }
        self.rows.finish(self.expected)
    }
}

/// Lazy row iterator.  It avoids allocating a `Vec` of offsets for images with
/// many scanlines while still handling Adam7 passes.
struct PngRowIter {
    passes: [(u32, u32, u32, u32); 7],
    pass_count: usize,
    pass_index: usize,
    width: u32,
    height: u32,
    bpp: u8,
    rows_left: u32,
    row_bytes: usize,
}

impl PngRowIter {
    fn new(width: u32, height: u32, bpp: u8, interlace: u8) -> Self {
        let mut passes = png_passes();
        let pass_count = if interlace == 1 {
            passes.len()
        } else {
            if let Some(first) = passes.first_mut() {
                *first = (0, 0, 1, 1);
            }
            1
        };
        Self {
            passes,
            pass_count,
            pass_index: 0,
            width,
            height,
            bpp,
            rows_left: 0,
            row_bytes: 0,
        }
    }

    fn next_row(&mut self) -> Option<usize> {
        loop {
            if self.rows_left == 0 {
                if self.pass_index >= self.pass_count {
                    return None;
                }
                let pass = self.passes.get(self.pass_index)?;
                self.pass_index += 1;
                let (x0, y0, dx, dy) = *pass;
                let pw = pass_dim(self.width, x0, dx);
                let ph = pass_dim(self.height, y0, dy);
                self.rows_left = ph;
                self.row_bytes = (pw as usize).checked_mul(self.bpp as usize)?.div_ceil(8);
                continue;
            }
            self.rows_left -= 1;
            return Some(self.row_bytes);
        }
    }
}

struct PngRowCursor {
    rows: PngRowIter,
    row_index: u64,
    row_remaining: usize,
    expecting_filter: bool,
}

impl PngRowCursor {
    fn new(width: u32, height: u32, bpp: u8, interlace: u8) -> Self {
        Self {
            rows: PngRowIter::new(width, height, bpp, interlace),
            row_index: 0,
            row_remaining: 0,
            expecting_filter: true,
        }
    }

    fn consume(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut idx = 0usize;
        while idx < bytes.len() {
            if self.expecting_filter {
                let Some(&filter) = bytes.get(idx) else {
                    break;
                };
                idx += 1;
                if filter > 4 {
                    return Err(format!(
                        "PNG row {} has invalid filter byte {filter}",
                        self.row_index
                    ));
                }
                let Some(row_bytes) = self.rows.next_row() else {
                    return Err("PNG decoded data has more rows than IHDR declares".into());
                };
                self.row_remaining = row_bytes;
                self.row_index += 1;
                self.expecting_filter = row_bytes == 0;
            } else {
                let available = bytes.len() - idx;
                let take = self.row_remaining.min(available);
                if take == 0 {
                    self.expecting_filter = true;
                    continue;
                }
                idx += take;
                self.row_remaining -= take;
                if self.row_remaining == 0 {
                    self.expecting_filter = true;
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self, expected: usize) -> Result<(), String> {
        if self.expecting_filter && self.row_remaining == 0 && self.rows.next_row().is_none() {
            Ok(())
        } else {
            Err(format!(
                "PNG row layout does not match the expected {expected} bytes"
            ))
        }
    }
}

fn validate_pdf(data: &[u8], analysis: &Analysis) -> (Validation, Vec<String>) {
    let selected = data.get(..analysis.end).unwrap_or(data);
    let trimmed = match selected
        .iter()
        .rposition(|b| !matches!(b, b'\r' | b'\n' | b' ' | b'\t' | 0))
    {
        Some(i) => selected.get(..=i).unwrap_or(selected),
        None => selected,
    };
    if !trimmed.ends_with(b"%%EOF") {
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
            Validation::Unverified,
            vec![
                "PDF decode is lexical-only (startxref target validated); content not fully verified"
                    .into(),
            ],
        )
    } else {
        (
            Validation::Unverified,
            vec![
                "PDF footer present but xref target was not validated; content not checked".into(),
            ],
        )
    }
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
            let row_bytes = (pw as usize).checked_mul(bpp as usize)?.div_ceil(8);
            let row = 1usize.checked_add(row_bytes)?;
            total = total.checked_add((ph as usize).checked_mul(row)?)?;
        }
    } else {
        let row_bytes = (width as usize).checked_mul(bpp as usize)?.div_ceil(8);
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
