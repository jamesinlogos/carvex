//! Signature definitions and structural validators.
//!
//! The registry below is the single source of truth for `--types`,
//! `carvex signatures`, header search and extension naming.  Adding a format
//! means adding one [`FormatSpec`] and one validator module.

#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
/// JPEG signature analysis and validation.
pub mod jpeg;
/// PDF signature analysis and validation.
pub mod pdf;
mod pdf_validate;
/// PNG signature analysis and validation.
pub mod png;
mod repair;

pub(crate) use repair::{build_suffix, validate as validate_repair, ValidatedRepair};

use clap::ValueEnum;
use flate2::{Decompress, FlushDecompress, Status as InflateStatus};
use serde::Serialize;
use std::fmt;

/// Default upper bound on the decompressed size accepted by PNG decode
/// validation.  The validator streams inflation, so this bounds work and the
/// largest image that `--validate decode` will claim to have validated.
pub const DEFAULT_MAX_PNG_DECODE_BYTES: u64 = 256 * 1024 * 1024;
/// Alias kept for the report field introduced with R-02 (H-05/H-06).
pub const DEFAULT_MAX_VALIDATOR_MEMORY: u64 = DEFAULT_MAX_PNG_DECODE_BYTES;

/// Resource limits for optional deep validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationLimits {
    /// Maximum working memory a decode validator may claim for one candidate.
    pub max_validator_memory: u64,
    /// Legacy name of the same budget, kept in reports for compatibility.
    pub max_png_decoded_bytes: u64,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_VALIDATOR_MEMORY)
    }
}

impl ValidationLimits {
    /// Build limits from a configured byte budget.  `0` selects the default
    /// validator budget rather than disabling the guard.
    pub fn new(max_validator_memory: u64) -> Self {
        let effective = if max_validator_memory == 0 {
            DEFAULT_MAX_VALIDATOR_MEMORY
        } else {
            max_validator_memory
        };
        Self {
            max_validator_memory: effective,
            max_png_decoded_bytes: effective,
        }
    }
}

/// The `FileType` enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    /// The `Jpeg` variant of `FileType`.
    #[value(alias = "jpg")]
    Jpeg,
    /// The `Png` variant of `FileType`.
    #[value(alias = "png")]
    Png,
    /// The `Pdf` variant of `FileType`.
    #[value(alias = "pdf")]
    Pdf,
}

impl FileType {
    /// Method.
    pub fn ext(self) -> &'static str {
        self.spec().ext
    }

    /// Method.
    pub fn label(self) -> &'static str {
        self.spec().label
    }

    /// Method.
    pub fn header(self) -> &'static [u8] {
        self.spec().header
    }

    /// Method.
    pub fn spec(self) -> &'static FormatSpec {
        FORMATS
            .iter()
            .find(|f| f.file_type == self)
            .unwrap_or(&FORMATS[0])
    }

    /// The `all` associated function.
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
    /// File type this spec describes.
    pub file_type: FileType,
    /// Default file extension.
    pub ext: &'static str,
    /// Human-readable format label.
    pub label: &'static str,
    /// Header signature bytes.
    pub header: &'static [u8],
    /// Footer signature bytes, if any.
    pub footer: &'static str,
    /// Validation depth supported by this format.
    pub validation: &'static str,
}

/// The `FORMATS` constant.
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

/// The `PdfEofMode` enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum PdfEofMode {
    /// The `First` variant of `PdfEofMode`.
    First,
    /// The `Last` variant of `PdfEofMode`.
    Last,
    /// The `Smart` variant of `PdfEofMode`.
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

/// The `Status` enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The `Complete` variant of `Status`.
    Complete,
    /// The `Truncated` variant of `Status`.
    Truncated,
    /// The `Repaired` variant of `Status`.
    Repaired,
}

/// Authoritative honesty label for a carved file (ADR-H1).
///
/// The legacy `validation` and `status` fields keep their historical meaning.
/// `validity` is additive and must be preferred by users and summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Validity {
    /// Every mandatory check for the format was implemented and passed.
    Verified,
    /// The structure parses, but payload content was not checked.
    Structural,
    /// The structure is complete, but at least one integrity check failed.
    Suspect,
    /// A truncated or prefix-only salvage.
    Partial,
    /// A repair was applied and re-validated.
    Repaired,
}

impl Validity {
    /// Map the legacy fields onto the authoritative label.
    pub fn from_legacy(validation: Validation, status: Status) -> Self {
        match status {
            Status::Repaired => Self::Repaired,
            Status::Truncated => Self::Partial,
            Status::Complete => match validation {
                Validation::Decoded => Self::Verified,
                Validation::Structure | Validation::Unverified => Self::Structural,
                Validation::Partial => Self::Suspect,
            },
        }
    }

    /// Short machine-readable name, identical to the JSON serialization.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Structural => "structural",
            Self::Suspect => "suspect",
            Self::Partial => "partial",
            Self::Repaired => "repaired",
        }
    }

    /// Human-readable label used in console lines and manifest comments.
    pub fn display_label(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Structural => "structural (content not checked)",
            Self::Suspect => "suspect (integrity check failed)",
            Self::Partial => "partial (salvaged prefix)",
            Self::Repaired => "repaired (revalidated)",
        }
    }
}

/// One structured validation check attached to a carved file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationCheck {
    /// Check identifier.
    pub name: String,
    /// `true` = passed, `false` = failed, `null` = not performed/unsupported.
    pub ok: Option<bool>,
    /// Human-readable check detail.
    pub note: String,
}

impl ValidationCheck {
    /// The `new` associated function.
    pub fn new(name: impl Into<String>, ok: Option<bool>, note: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok,
            note: note.into(),
        }
    }
}

/// Detailed result of a decode-validation pass.
///
/// `notes` preserves the historical human-readable result. `checks` and
/// `validated_bytes` carry the H-16/H-17 additions without changing the public
/// tuple API used by older callers.
#[derive(Debug, Clone)]
pub struct ValidationDetail {
    /// Legacy validation outcome.
    pub validation: Validation,
    /// Human-readable validation notes.
    pub notes: Vec<String>,
    /// Structured validation checks.
    pub checks: Vec<ValidationCheck>,
    /// Byte prefix covered by validation.
    pub validated_bytes: u64,
}

impl ValidationDetail {
    /// The `new` associated function.
    pub fn new(validation: Validation, notes: Vec<String>) -> Self {
        Self {
            validation,
            notes,
            checks: Vec::new(),
            validated_bytes: 0,
        }
    }
}

/// Build the checks that were actually performed for a legacy validation
/// result. Later findings append format-specific checks; this baseline never
/// claims a check that the current validator does not implement.
pub fn base_validity_checks(
    kind: FileType,
    validation: Validation,
    status: Status,
    notes: &[String],
) -> Vec<ValidationCheck> {
    let mut checks = match status {
        Status::Complete => vec![ValidationCheck::new(
            "footer",
            Some(true),
            "header and footer structure parsed",
        )],
        Status::Truncated => vec![ValidationCheck::new(
            "footer",
            Some(false),
            "no footer found; file is a salvaged prefix",
        )],
        Status::Repaired => vec![ValidationCheck::new(
            "footer",
            Some(false),
            "footer was absent; a revalidated repair suffix was appended",
        )],
    };

    match validation {
        Validation::Structure => checks.push(ValidationCheck::new(
            "content_check",
            None,
            "content not checked; structure parsed only",
        )),
        Validation::Decoded => checks.push(ValidationCheck::new(
            "content_check",
            Some(true),
            if kind == FileType::Pdf {
                "PDF structure checks passed; page content was not rendered"
            } else {
                "payload decoded successfully"
            },
        )),
        Validation::Unverified => checks.push(ValidationCheck::new(
            "content_check",
            None,
            "a mandatory content check is unsupported; content not checked",
        )),
        Validation::Partial => {
            let note = notes
                .first()
                .cloned()
                .unwrap_or_else(|| "content validation did not complete".into());
            checks.push(ValidationCheck::new("integrity_check", Some(false), note));
        }
    }
    checks
}

/// Structured check for one PNG chunk CRC mismatch. Used by both the
/// structural parser and decode validator so their notes agree.
pub(crate) fn png_chunk_crc_check(ctype: &[u8], offset: usize) -> ValidationCheck {
    ValidationCheck::new(
        "chunk_crc",
        Some(false),
        format!(
            "PNG chunk {} at byte {offset} has CRC mismatch",
            String::from_utf8_lossy(ctype)
        ),
    )
}

/// Structural analysis result returned by format parsers.
#[derive(Debug, Clone)]
pub struct Analysis {
    /// Number of input bytes that belong to the file (before any repair suffix).
    pub end: usize,
    /// Whether the structure appears complete.
    pub complete: bool,
    /// Heuristic confidence score (0-100).
    pub score: u8,
    /// Image width in pixels, when known.
    pub width: Option<u32>,
    /// Image height in pixels, when known.
    pub height: Option<u32>,
    /// Parser notes and warnings.
    pub notes: Vec<String>,
    /// Appended when --repair is enabled and the file is truncated.
    pub repair_suffix: &'static [u8],
    /// Integrity failures found while parsing (for example PNG chunk CRCs).
    pub integrity_checks: Vec<ValidationCheck>,
    /// Length of the contiguous prefix covered by structural integrity checks.
    pub validated_prefix: u64,
}

impl Analysis {
    /// True when a structural integrity check failed.
    pub fn integrity_failed(&self) -> bool {
        self.integrity_checks
            .iter()
            .any(|check| check.ok == Some(false))
    }

    /// Method.
    pub fn status(&self, repaired: bool) -> Status {
        if repaired {
            Status::Repaired
        } else if self.complete {
            Status::Complete
        } else {
            Status::Truncated
        }
    }
}

/// The `Need` enumeration.
pub enum Need {
    /// The `Analyzed` variant of `Need`.
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
    let detail = validate_deep_detail(kind, data, analysis, limits);
    (detail.validation, detail.notes)
}

/// Detailed decode validation, including structured checks and the checked
/// byte prefix. This is the entry point used by the scanner.
pub fn validate_deep_detail(
    kind: FileType,
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
) -> ValidationDetail {
    match kind {
        FileType::Jpeg => validate_jpeg_detail(data, analysis, limits),
        FileType::Png => validate_png_detail(data, analysis, limits),
        FileType::Pdf => validate_pdf_detail(data, analysis, limits),
    }
}

fn jpeg_frame_preflight(
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
) -> Option<ValidationDetail> {
    let claim = jpeg::frame_claim(data)?;
    let samples = u64::from(claim.width)
        .checked_mul(u64::from(claim.height))
        .and_then(|v| v.checked_mul(u64::from(claim.components)))
        .unwrap_or(u64::MAX);
    if samples > limits.max_validator_memory {
        return Some(ValidationDetail {
            validation: Validation::Unverified,
            notes: vec![format!(
                "claimed frame size exceeds validator budget: {samples} samples > {} bytes",
                limits.max_validator_memory
            )],
            checks: Vec::new(),
            validated_bytes: 0,
        });
    }
    // A Huffman-coded 8x8 block needs at least two entropy bits (DC symbol
    // plus EOB), so one byte can justify at most 256 samples.  Reject a
    // claim above twice that optimistic bound; arithmetic-coded frames do
    // not use Huffman tables and are exempt from this check.
    if !claim.arithmetic {
        let plausible = (analysis.end as u64).saturating_mul(512);
        if samples > plausible {
            return Some(ValidationDetail {
                validation: Validation::Unverified,
                notes: vec![format!(
                    "claimed frame size {samples} samples is implausible for {} entropy bytes",
                    analysis.end
                )],
                checks: Vec::new(),
                validated_bytes: 0,
            });
        }
    }
    None
}

fn jpeg_decode_success(
    data: &[u8],
    analysis: &Analysis,
    pixels: Vec<u8>,
    info: Option<jpeg_decoder::ImageInfo>,
) -> ValidationDetail {
    let (w, h) = info.map_or((0, 0), |i| (u32::from(i.width), u32::from(i.height)));
    if let (Some(ew), Some(eh)) = (analysis.width, analysis.height) {
        if w != 0 && h != 0 && (w != ew || h != eh) {
            return ValidationDetail {
                validation: Validation::Partial,
                notes: vec![format!(
                    "decoded dimensions {w}x{h} do not match SOF {ew}x{eh}"
                )],
                checks: Vec::new(),
                validated_bytes: 0,
            };
        }
    }
    if pixels.is_empty() {
        return ValidationDetail {
            validation: Validation::Partial,
            notes: vec!["JPEG decoder returned no pixels".into()],
            checks: Vec::new(),
            validated_bytes: 0,
        };
    }
    match jpeg::strict_baseline_scan(data) {
        Some(Err(reason)) => {
            return ValidationDetail {
                validation: Validation::Partial,
                notes: vec![format!("baseline scan does not match the frame: {reason}")],
                checks: Vec::new(),
                validated_bytes: 0,
            };
        }
        Some(Ok(())) => {}
        None => {
            return ValidationDetail {
                validation: Validation::Unverified,
                notes: vec![
                    "JPEG strict scan unsupported for this variant; content not checked".into(),
                ],
                checks: vec![ValidationCheck::new(
                    "content_continuity",
                    None,
                    "not applicable for unsupported JPEG variant",
                )],
                validated_bytes: 0,
            };
        }
    }
    let components = if w > 0 && h > 0 {
        pixels.len() / (w as usize * h as usize)
    } else {
        0
    };
    let mut checks = Vec::new();
    let mut notes = vec![format!("decoded {} bytes of pixel data", pixels.len())];
    let mut validation = Validation::Decoded;
    let mut validated_bytes = analysis.end as u64;
    match jpeg::content_continuity_check(&pixels, w, h, components) {
        Some(Ok(())) => checks.push(ValidationCheck::new(
            "content_continuity",
            Some(true),
            "no extreme seam discontinuity detected",
        )),
        Some(Err(reason)) => {
            validation = Validation::Partial;
            validated_bytes = 0;
            notes.push(reason.clone());
            checks.push(ValidationCheck::new(
                "content_continuity",
                Some(false),
                reason,
            ));
        }
        None => checks.push(ValidationCheck::new(
            "content_continuity",
            None,
            "not applicable for this JPEG/image size",
        )),
    }
    ValidationDetail {
        validation,
        notes,
        checks,
        validated_bytes,
    }
}

fn validate_jpeg_detail(
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
) -> ValidationDetail {
    if let Some(detail) = jpeg_frame_preflight(data, analysis, limits) {
        return detail;
    }
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(data));
    match decoder.decode() {
        Ok(pixels) => jpeg_decode_success(data, analysis, pixels, decoder.info()),
        Err(e) => ValidationDetail {
            validation: Validation::Partial,
            notes: vec![format!("JPEG decode failed: {e}")],
            checks: Vec::new(),
            validated_bytes: 0,
        },
    }
}

fn png_partial(reason: impl Into<String>) -> (Validation, Vec<String>) {
    (Validation::Partial, vec![reason.into()])
}

struct PngValidationChunk<'a> {
    chunk_end: usize,
    ctype: &'a [u8],
    chunk_data: &'a [u8],
    crc_ok: bool,
}

fn read_validation_png_chunk(data: &[u8], pos: usize) -> Option<PngValidationChunk<'_>> {
    let len_bytes = data.get(pos..pos + 4)?;
    let len = u32::from_be_bytes(<[u8; 4]>::try_from(len_bytes).ok()?) as usize;
    let ctype = data.get(pos + 4..pos + 8)?;
    let chunk_end = pos
        .checked_add(12)?
        .checked_add(len)
        .filter(|&v| v <= data.len())?;
    let chunk_data = data.get(pos + 8..pos + 8 + len)?;
    let stored_crc = data
        .get(pos + 8 + len..pos + 12 + len)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(u32::from_be_bytes);
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(ctype);
    hasher.update(chunk_data);
    Some(PngValidationChunk {
        chunk_end,
        ctype,
        chunk_data,
        crc_ok: stored_crc == Some(hasher.finalize()),
    })
}

fn start_png_inflate(
    w: u32,
    h: u32,
    chunk_data: &[u8],
    max_decoded: u64,
) -> Result<PngInflate, String> {
    let (Some(&depth), Some(&color_type), Some(&interlace)) =
        (chunk_data.get(8), chunk_data.get(9), chunk_data.get(12))
    else {
        return Err("unsupported PNG geometry for decode validation".into());
    };
    let Some(expected) = expected_png_raw_size(w, h, depth, color_type, interlace) else {
        return Err("unsupported PNG geometry for decode validation".into());
    };
    if expected as u64 > max_decoded {
        return Err(format!(
            "PNG decoded size {expected} exceeds the \
             decode-validation budget of {max_decoded} bytes"
        ));
    }
    let Some(bpp) = png_bits_per_pixel(depth, color_type) else {
        return Err("unsupported PNG color type for decode validation".into());
    };
    // The budget is checked before any IDAT byte is inflated and before a row
    // cursor or output buffer is created.
    Ok(PngInflate::new(
        expected,
        PngRowCursor::new(w, h, bpp, interlace),
    ))
}

fn validate_png(
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
) -> (Validation, Vec<String>) {
    let Some((w, h)) = analysis.width.zip(analysis.height) else {
        return png_partial("PNG IHDR dimensions are unavailable");
    };
    let max_decoded = limits.max_validator_memory;
    let mut pos = 8usize;
    let mut saw_ihdr = false;
    let mut saw_iend = false;
    let mut bad_crcs: Vec<String> = Vec::new();
    let mut inflate: Option<PngInflate> = None;
    while pos + 12 <= data.len() {
        let Some(chunk) = read_validation_png_chunk(data, pos) else {
            break;
        };
        if !chunk.crc_ok {
            bad_crcs.push(format!(
                "PNG chunk {} has CRC mismatch",
                String::from_utf8_lossy(chunk.ctype)
            ));
        }
        match chunk.ctype {
            b"IHDR" if chunk.chunk_data.len() >= 13 => {
                if saw_ihdr {
                    return png_partial("duplicate PNG IHDR during decode validation");
                }
                saw_ihdr = true;
                let (Some(&_depth), Some(&_color_type), Some(&_interlace)) = (
                    chunk.chunk_data.get(8),
                    chunk.chunk_data.get(9),
                    chunk.chunk_data.get(12),
                ) else {
                    break;
                };
                inflate = Some(
                    match start_png_inflate(w, h, chunk.chunk_data, max_decoded) {
                        Ok(state) => state,
                        Err(reason) => return png_partial(reason),
                    },
                );
            }
            b"IDAT" => {
                let Some(state) = inflate.as_mut() else {
                    return png_partial("PNG IDAT appears before IHDR");
                };
                if let Err(reason) = state.feed(chunk.chunk_data) {
                    return png_partial(reason);
                }
            }
            b"IEND" => {
                saw_iend = true;
                break;
            }
            _ => {}
        }
        pos = chunk.chunk_end;
    }
    if !saw_iend {
        return png_partial("PNG decode stopped before IEND");
    }
    let Some(mut state) = inflate else {
        return png_partial("PNG decode validation found no IDAT data");
    };
    if let Err(reason) = state.finish_and_check() {
        return png_partial(reason);
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

fn validate_png_detail(
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
) -> ValidationDetail {
    let (validation, notes) = validate_png(data, analysis, limits);
    let (checks, crc_prefix) = png_crc_scan(data);
    // A clean decode validates the whole carved image. When an integrity
    // check fails, validated_bytes stops at the last chunk whose CRC passed.
    let validated_bytes = if checks.iter().all(|check| check.ok != Some(false)) {
        if validation == Validation::Decoded {
            analysis.end as u64
        } else {
            0
        }
    } else {
        crc_prefix
    };
    ValidationDetail {
        validation,
        notes,
        checks,
        validated_bytes,
    }
}

/// Recompute every PNG chunk CRC before decode claims success. The scan is
/// bounded by the already-parsed length and materializes no image data.
fn png_crc_scan(data: &[u8]) -> (Vec<ValidationCheck>, u64) {
    let mut checks = Vec::new();
    let mut good_prefix = 8u64;
    let mut crc_failed = false;
    let mut pos = 8usize;
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
        let stored = data
            .get(pos + 8 + len..pos + 12 + len)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_be_bytes);
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(ctype);
        hasher.update(chunk_data);
        let computed = hasher.finalize();
        if stored == Some(computed) {
            if !crc_failed {
                good_prefix = chunk_end as u64;
            }
        } else {
            crc_failed = true;
            checks.push(png_chunk_crc_check(ctype, pos));
        }
        pos = chunk_end;
    }
    (checks, good_prefix)
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

fn validate_pdf_detail(
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
) -> ValidationDetail {
    pdf_validate::validate(data, analysis, limits)
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
