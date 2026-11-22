//! Repair planning and revalidation for `--repair` (H-21).
//!
//! A truncated candidate is only labelled `repaired` after the reconstructed
//! bytes pass the same structural/decode checks as an intact file of that
//! type. PDF repair rebuilds a classic xref table and trailer from the
//! object headers and catalog found in the salvaged prefix; JPEG and PNG
//! append their real footers.

#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]

use super::{
    jpeg::EOI, png::IEND_SUFFIX, validate_deep_detail, Analysis, FileType, Need, PdfEofMode,
    Validation, ValidationDetail, ValidationLimits, ValidationMode,
};
use std::collections::HashMap;

const MAX_REPAIR_OBJECTS: usize = 1_000_000;
const MAX_OBJECT_BODY: usize = 64 * 1024;
const MAX_XREF_OFFSET: u64 = 9_999_999_999;

/// The result of a repair that passed its revalidation checks.
#[derive(Debug)]
pub(crate) struct ValidatedRepair {
    pub analysis: Analysis,
    pub detail: ValidationDetail,
    pub note: String,
}

/// Build the suffix that would repair `data`.
///
/// An `Err` means no honest repair is possible; the caller keeps the original
/// truncated bytes and records `repair attempted, not validated`.
pub(crate) fn build_suffix(
    kind: FileType,
    data: &[u8],
    limits: ValidationLimits,
) -> Result<Vec<u8>, String> {
    match kind {
        FileType::Jpeg => Ok(EOI.to_vec()),
        FileType::Png => Ok(IEND_SUFFIX.to_vec()),
        FileType::Pdf => build_pdf_suffix(data, limits),
    }
}

/// Validate a reconstructed file. The caller supplies the complete bytes;
/// `data` is never written unless this succeeds.
pub(crate) fn validate(
    kind: FileType,
    data: &[u8],
    limits: ValidationLimits,
    mode: ValidationMode,
    pdf_mode: PdfEofMode,
) -> Result<ValidatedRepair, String> {
    let analysis = match super::analyze(kind, data, true, pdf_mode) {
        Need::Analyzed(analysis) if analysis.complete && analysis.end == data.len() => analysis,
        Need::Analyzed(analysis) => {
            return Err(format!(
                "repaired file parsed only {}/{} byte(s)",
                analysis.end,
                data.len()
            ))
        }
        Need::More => return Err("repaired file still needs more bytes".into()),
        Need::Reject(reason) => return Err(format!("repaired file rejected: {reason}")),
    };
    // PDF needs the structural xref/catalog checks even in default mode,
    // because a bare `%%EOF` is exactly the false repair this finding fixes.
    let detail = if mode == ValidationMode::Decode || kind == FileType::Pdf {
        validate_deep_detail(kind, data, &analysis, limits)
    } else {
        ValidationDetail::new(Validation::Structure, Vec::new())
    };
    let accepted = match kind {
        FileType::Pdf => detail.validation == Validation::Decoded,
        FileType::Jpeg | FileType::Png if mode == ValidationMode::Decode => {
            // Progressive/unsupported variants are `Unverified` when intact;
            // only a hard decode failure is a failed repair.
            matches!(
                detail.validation,
                Validation::Decoded | Validation::Unverified
            )
        }
        FileType::Jpeg | FileType::Png => true,
    };
    if !accepted {
        return Err(format!(
            "repaired file revalidated as {:?}",
            detail.validation
        ));
    }
    let note = match kind {
        FileType::Pdf => "repair rebuilt the xref/trailer and revalidated".to_string(),
        FileType::Jpeg => "repair appended EOI and revalidated".to_string(),
        FileType::Png => "repair appended IEND and revalidated".to_string(),
    };
    Ok(ValidatedRepair {
        analysis,
        detail,
        note,
    })
}

fn budget_objects(limits: ValidationLimits) -> usize {
    usize::try_from(limits.max_validator_memory)
        .unwrap_or(usize::MAX)
        .clamp(1, MAX_REPAIR_OBJECTS)
}

fn build_pdf_suffix(data: &[u8], limits: ValidationLimits) -> Result<Vec<u8>, String> {
    let max_objects = budget_objects(limits);
    let mut objects: HashMap<u64, (u16, usize)> = HashMap::new();
    for marker in memchr::memmem::find_iter(data, b" obj") {
        let Some((number, generation, offset)) = object_header(data, marker) else {
            continue;
        };
        if number == 0 {
            continue;
        }
        if !objects.contains_key(&number) && objects.len() >= max_objects {
            return Err("too many PDF objects to repair within the validator budget".into());
        }
        // Later headers win, matching incremental-update semantics.
        objects.insert(number, (generation, offset));
    }
    if objects.is_empty() {
        return Err("no PDF object headers in the salvaged prefix".into());
    }

    let mut ordered: Vec<(u64, u16, usize)> = objects
        .iter()
        .map(|(number, (generation, offset))| (*number, *generation, *offset))
        .collect();
    ordered.sort_unstable_by_key(|(number, _, _)| *number);

    let root = root_reference(data)
        .filter(|(number, _)| objects.contains_key(number))
        .or_else(|| catalog_object(data, &ordered))
        .ok_or_else(|| "no resolvable /Root or /Type /Catalog object".to_string())?;
    let max_object = ordered
        .last()
        .map(|(number, _, _)| *number)
        .unwrap_or_default();
    let size = max_object.max(root.0).saturating_add(1);
    if size > u64::try_from(max_objects.saturating_add(1)).unwrap_or(u64::MAX) {
        return Err("PDF /Size would exceed the validator budget".into());
    }
    let xref_offset = u64::try_from(data.len())
        .map_err(|_| "PDF prefix is too large to rebuild an xref".to_string())?;
    if xref_offset > MAX_XREF_OFFSET {
        return Err("PDF xref offset does not fit a classic xref entry".into());
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"xref\n0 ");
    out.extend_from_slice(size.to_string().as_bytes());
    out.extend_from_slice(b"\n0000000000 65535 f \n");
    for number in 1..size {
        match objects.get(&number) {
            Some((generation, offset)) => {
                let offset = u64::try_from(*offset).map_err(|_| {
                    "PDF object offset does not fit a classic xref entry".to_string()
                })?;
                if offset > MAX_XREF_OFFSET {
                    return Err("PDF object offset does not fit a classic xref entry".into());
                }
                out.extend_from_slice(format!("{offset:010} {generation:05} n \n").as_bytes());
            }
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    out.extend_from_slice(b"trailer\n<< /Size ");
    out.extend_from_slice(size.to_string().as_bytes());
    out.extend_from_slice(b" /Root ");
    out.extend_from_slice(format!("{} {} R", root.0, root.1).as_bytes());
    out.extend_from_slice(b" >>\nstartxref\n");
    out.extend_from_slice(format!("{xref_offset}\n%%EOF\n").as_bytes());
    if out.len() > max_objects.saturating_mul(24).saturating_add(1024) {
        return Err("rebuilt xref exceeds the validator budget".into());
    }
    Ok(out)
}

/// Parse `N G obj` where `marker` is the byte offset of b" obj".
fn object_header(data: &[u8], marker: usize) -> Option<(u64, u16, usize)> {
    let mut pos = marker;
    while pos > 0 && data.get(pos - 1).is_some_and(u8::is_ascii_whitespace) {
        pos -= 1;
    }
    let generation_end = pos;
    while pos > 0 && data.get(pos - 1).is_some_and(u8::is_ascii_digit) {
        pos -= 1;
    }
    if pos == generation_end {
        return None;
    }
    let generation = std::str::from_utf8(data.get(pos..generation_end)?)
        .ok()?
        .parse::<u16>()
        .ok()?;
    while pos > 0 && data.get(pos - 1).is_some_and(u8::is_ascii_whitespace) {
        pos -= 1;
    }
    let number_end = pos;
    while pos > 0 && data.get(pos - 1).is_some_and(u8::is_ascii_digit) {
        pos -= 1;
    }
    if pos == number_end || (pos > 0 && !data.get(pos - 1).is_some_and(u8::is_ascii_whitespace)) {
        return None;
    }
    let number = std::str::from_utf8(data.get(pos..number_end)?)
        .ok()?
        .parse::<u64>()
        .ok()?;
    Some((number, generation, pos))
}

/// Prefer a `/Root` already present in a surviving trailer.
fn root_reference(data: &[u8]) -> Option<(u64, u16)> {
    let mut found = None;
    for rel in memchr::memmem::find_iter(data, b"/Root") {
        let mut pos = skip_ws(data, rel + b"/Root".len());
        let Some((number, next)) = parse_uint(data, pos) else {
            continue;
        };
        pos = skip_ws(data, next);
        let Some((generation, next)) = parse_uint(data, pos) else {
            continue;
        };
        pos = skip_ws(data, next);
        if data.get(pos) == Some(&b'R') {
            if let Ok(generation) = u16::try_from(generation) {
                found = Some((number, generation));
            }
        }
    }
    found
}

fn catalog_object(data: &[u8], objects: &[(u64, u16, usize)]) -> Option<(u64, u16)> {
    let offsets: Vec<usize> = objects.iter().map(|(_, _, offset)| *offset).collect();
    for (number, generation, offset) in objects {
        let next = offsets
            .iter()
            .find(|candidate| **candidate > *offset)
            .copied()
            .unwrap_or(data.len());
        let end = next
            .min(data.len())
            .min(offset.saturating_add(MAX_OBJECT_BODY));
        let body = data.get(*offset..end)?;
        if memchr::memmem::find(body, b"/Type /Catalog").is_some() {
            return Some((*number, *generation));
        }
    }
    None
}

fn skip_ws(data: &[u8], mut pos: usize) -> usize {
    while data.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    pos
}

fn parse_uint(data: &[u8], start: usize) -> Option<(u64, usize)> {
    let mut pos = start;
    while data.get(pos).is_some_and(u8::is_ascii_digit) {
        pos += 1;
    }
    if pos == start {
        return None;
    }
    let value = std::str::from_utf8(data.get(start..pos)?)
        .ok()?
        .parse::<u64>()
        .ok()?;
    Some((value, pos))
}
