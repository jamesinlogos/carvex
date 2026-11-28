//! Bounded structural validation for PDF decode mode (H-19 stage 2).
//!
//! This module deliberately does not render page content. It verifies the
//! cross-reference graph, trailer fields, object headers and catalog chain so
//! that `validity=verified` means every mandatory *structural* check passed.
//! Work is bounded by [`ValidationLimits`] and by fixed caps on entries,
//! dictionary scans and inflated streams.

#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]

use super::{Analysis, Validation, ValidationCheck, ValidationDetail, ValidationLimits};
use flate2::{Decompress, FlushDecompress, Status as InflateStatus};
use std::collections::{HashMap, HashSet};

const MAX_XREF_ENTRIES: usize = 1_000_000;
const MAX_XREF_STREAM_BYTES: usize = 16 * 1024 * 1024;
const MAX_OBJECT_SCAN: usize = 64 * 1024;
const MAX_HEADER_PAIRS: usize = 1_000_000;
const MAX_PREV_DEPTH: usize = 8;
const INFLATE_BUFFER: usize = 64 * 1024;
// H-04 R2: bounded page-tree walk used only when validating a rebuilt PDF.
const MAX_PAGE_TREE_NODES: usize = 64;
const MAX_PAGE_TREE_KIDS: usize = 256;
const MAX_PAGE_TREE_DEPTH: usize = 8;

#[derive(Clone, Debug)]
enum ObjEntry {
    Uncompressed { offset: u64, generation: u16 },
    Compressed { object_stream: u64, index: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamKind {
    Classic,
    Xref,
}

#[derive(Debug)]
struct Section {
    entries: Vec<(u64, ObjEntry)>,
    size: Option<u64>,
    root: Option<(u64, u16)>,
    prev: Option<u64>,
    xref_stm: Option<u64>,
    encrypt: bool,
    kind: StreamKind,
}

#[derive(Debug)]
enum Issue {
    Fail(&'static str, String),
    Unsupported(&'static str, String),
}

impl Issue {
    fn fail(name: &'static str, reason: impl Into<String>) -> Self {
        Self::Fail(name, reason.into())
    }

    fn unsupported(name: &'static str, reason: impl Into<String>) -> Self {
        Self::Unsupported(name, reason.into())
    }
}

#[derive(Default)]
struct Collector {
    checks: Vec<ValidationCheck>,
    notes: Vec<String>,
    failed: bool,
    unsupported: bool,
}

impl Collector {
    fn record(&mut self, name: &str, ok: Option<bool>, note: impl Into<String>) {
        let note = note.into();
        if let Some(existing) = self.checks.iter_mut().find(|c| c.name == name) {
            let overwrite = match (existing.ok, ok) {
                (Some(false), _) => false,
                (_, Some(false)) => true,
                (Some(true), None) => true,
                _ => false,
            };
            if overwrite {
                existing.ok = ok;
                existing.note = note;
            }
        } else {
            self.checks.push(ValidationCheck::new(name, ok, note));
        }
    }

    fn pass(&mut self, name: &str, note: impl Into<String>) {
        self.record(name, Some(true), note);
    }

    fn fail(&mut self, name: &'static str, note: impl Into<String>) {
        let note = note.into();
        self.failed = true;
        self.notes.push(note.clone());
        self.record(name, Some(false), note);
    }

    fn unsupported(&mut self, name: &'static str, note: impl Into<String>) {
        let note = note.into();
        self.unsupported = true;
        self.notes.push(note.clone());
        self.record(name, None, note);
    }

    fn issue(&mut self, issue: Issue) {
        match issue {
            Issue::Fail(name, reason) => self.fail(name, reason),
            Issue::Unsupported(name, reason) => self.unsupported(name, reason),
        }
    }

    fn finish(mut self) -> ValidationDetail {
        let validation = if self.failed {
            Validation::Partial
        } else if self.unsupported {
            Validation::Unverified
        } else {
            self.notes.push(
                "PDF cross-reference, trailer, size and catalog checks passed; page content was not rendered"
                    .into(),
            );
            Validation::Decoded
        };
        ValidationDetail {
            validation,
            notes: self.notes,
            checks: self.checks,
            // The checks are structural and visit scattered offsets; they do
            // not certify a contiguous decoded byte prefix.
            validated_bytes: 0,
        }
    }
}

/// Run bounded PDF structure validation. `data` is the carved candidate whose
/// length is `analysis.end`; no bytes beyond the selected revision are used.
pub(super) fn validate(
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
) -> ValidationDetail {
    validate_inner(data, analysis, limits, false)
}

/// H-04 R2: a rebuilt trailer is only a proof of repair if the document still
/// has a readable page. Ordinary validation deliberately stays structural; the
/// stricter check applies only to the repair path.
pub(super) fn validate_for_repair(
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
) -> ValidationDetail {
    validate_inner(data, analysis, limits, true)
}

fn validate_inner(
    data: &[u8],
    analysis: &Analysis,
    limits: ValidationLimits,
    require_readable_page: bool,
) -> ValidationDetail {
    let mut out = Collector::default();
    let selected = data.get(..analysis.end).unwrap_or(data);
    let Some(startxref) = find_startxref(selected) else {
        out.fail(
            "startxref_target",
            "no startxref value before the selected %%EOF",
        );
        return out.finish();
    };

    let mut sections: Vec<Section> = Vec::new();
    let mut seen: HashSet<u64> = HashSet::new();
    let mut target = startxref;
    for depth in 0..=MAX_PREV_DEPTH {
        if !seen.insert(target) {
            out.fail("prev_chain", format!("/Prev cycle at byte {target}"));
            break;
        }
        match parse_section(selected, target, limits) {
            Ok(section) => {
                out.pass(
                    "startxref_target",
                    format!("startxref target validated at byte {target}"),
                );
                if let Some(stream_target) = section.xref_stm {
                    match parse_section(selected, stream_target, limits) {
                        Ok(stream) if stream.kind == StreamKind::Xref => {
                            sections.push(stream);
                        }
                        Ok(_) => out.fail(
                            "xref_stream",
                            format!("/XRefStm at byte {stream_target} is not an xref stream"),
                        ),
                        Err(issue) => out.issue(issue),
                    }
                }
                let prev = section.prev;
                sections.push(section);
                match prev {
                    Some(prev) if depth < MAX_PREV_DEPTH => target = prev,
                    Some(_) => {
                        out.fail(
                            "prev_chain",
                            format!("/Prev chain exceeds {MAX_PREV_DEPTH} revisions"),
                        );
                        break;
                    }
                    None => break,
                }
            }
            Err(issue) => {
                out.issue(issue);
                break;
            }
        }
    }

    let Some(newest) = sections.first() else {
        return out.finish();
    };

    check_size(&mut out, newest, &sections);
    out.record(
        "xref_stream",
        Some(true),
        if newest.kind == StreamKind::Xref {
            "xref stream parsed with bounded inflation"
        } else {
            "classic xref table parsed; no xref stream present"
        },
    );
    out.record(
        "stream_length",
        Some(true),
        if newest.kind == StreamKind::Xref {
            "direct /Length matched the endstream position"
        } else {
            "classic xref has no xref stream length to check"
        },
    );
    out.record(
        "prev_chain",
        Some(true),
        if newest.prev.is_some() {
            "/Prev chain parsed with cycle and depth limits"
        } else {
            "no /Prev chain present"
        },
    );
    if newest.encrypt {
        out.pass(
            "encrypt",
            "encrypted PDF flagged; structure checked, encrypted content not decoded",
        );
    } else {
        out.pass("encrypt", "no /Encrypt entry in the selected trailer");
    }

    let mut entries: Vec<(u64, ObjEntry)> = Vec::new();
    for section in &sections {
        for (number, entry) in &section.entries {
            if !entries.iter().any(|(existing, _)| existing == number) {
                entries.push((*number, entry.clone()));
            }
        }
    }
    if let Err(issue) = validate_compressed_entries(selected, &entries, limits) {
        out.issue(issue);
    }
    out.record(
        "xref_entries",
        Some(true),
        format!(
            "{} xref entries resolved to object headers or object streams",
            entries.len()
        ),
    );
    check_root(&mut out, selected, &entries, newest, limits);
    if require_readable_page && !out.failed {
        check_page_tree(&mut out, selected, &entries, newest, limits);
    }
    out.finish()
}

/// Resolve one xref object to its dictionary body, using only bounded helpers.
/// Missing or malformed objects are reported with the same check name as
/// `check_root`; the page-tree walk treats them as unreadable pages.
fn object_body_bytes(
    data: &[u8],
    entries: &[(u64, ObjEntry)],
    number: u64,
    limits: ValidationLimits,
) -> Result<Vec<u8>, Issue> {
    let entry = entries
        .iter()
        .find(|(obj_number, _)| *obj_number == number)
        .map(|(_, entry)| entry)
        .ok_or_else(|| Issue::fail("startxref_target", format!("object {number} is absent")))?;
    let body: Vec<u8> = match entry {
        ObjEntry::Uncompressed {
            offset,
            generation: gen,
        } => {
            let offset = usize::try_from(*offset)
                .map_err(|_| Issue::fail("startxref_target", "object offset is out of range"))?;
            let range = object_body(data, offset, number, *gen)?;
            data.get(range).unwrap_or_default().to_vec()
        }
        ObjEntry::Compressed {
            object_stream,
            index,
        } => {
            let (buffer, range) =
                compressed_body(data, entries, *object_stream, *index, number, limits)?;
            buffer.get(range).unwrap_or_default().to_vec()
        }
    };
    Ok(body)
}

/// H-04 R2: walk the catalog's page tree with hard node/depth bounds. A `/Count 0`
/// tree claims no page and is left alone (the synthetic repair fixtures use it);
/// otherwise at least one `/Type /Page` object must resolve inside the carved
/// bytes. Nested `/Pages` nodes are followed one bounded level at a time.
fn check_page_tree(
    out: &mut Collector,
    data: &[u8],
    entries: &[(u64, ObjEntry)],
    newest: &Section,
    limits: ValidationLimits,
) {
    let Some((number, _)) = newest.root else {
        return;
    };
    let Ok(catalog) = object_body_bytes(data, entries, number, limits) else {
        return;
    };
    let Some((pages_number, _)) = ref_value(&catalog, 0..catalog.len(), b"/Pages") else {
        out.fail("page_tree", "catalog has no direct /Pages reference");
        return;
    };
    let Ok(pages) = object_body_bytes(data, entries, pages_number, limits) else {
        out.fail(
            "page_tree",
            format!("/Pages {pages_number} could not be resolved in the carved bytes"),
        );
        return;
    };
    if name_value(&pages, 0..pages.len(), b"/Type") != Some(b"/Pages") {
        out.fail(
            "page_tree",
            format!("/Pages {pages_number} has no /Type /Pages entry"),
        );
        return;
    }
    let Some(declared) = direct_int(&pages, 0..pages.len(), b"/Count") else {
        out.fail("page_tree", "page tree has no direct /Count value");
        return;
    };
    if declared == 0 {
        out.pass("page_tree", "page tree declares no pages");
        return;
    }
    let mut queue = vec![(pages_number, 0usize)];
    let mut visited = HashSet::new();
    let mut readable = 0usize;
    let mut examined = 0usize;
    while let Some((number, depth)) = queue.pop() {
        if !visited.insert(number) || examined >= MAX_PAGE_TREE_NODES {
            continue;
        }
        examined += 1;
        let Ok(body) = object_body_bytes(data, entries, number, limits) else {
            continue;
        };
        match name_value(&body, 0..body.len(), b"/Type") {
            Some(b"/Page") => readable += 1,
            Some(b"/Pages") if depth < MAX_PAGE_TREE_DEPTH => {
                let Some(kids) = array_refs(&body, 0..body.len(), b"/Kids") else {
                    continue;
                };
                for kid in kids.into_iter().take(MAX_PAGE_TREE_KIDS) {
                    queue.push((kid.0, depth + 1));
                }
            }
            _ => {}
        }
    }
    if readable == 0 {
        out.fail(
            "page_tree",
            format!(
                "page tree declares {declared} page(s) but no /Type /Page object resolved ({examined} node(s) examined)"
            ),
        );
    } else {
        out.pass(
            "page_tree",
            format!("{readable} readable page object(s) resolved in the page tree"),
        );
    }
}

fn check_size(out: &mut Collector, newest: &Section, sections: &[Section]) {
    let Some(size) = newest.size else {
        out.fail("xref_size", "selected trailer has no direct /Size");
        return;
    };
    if size == 0 {
        out.fail("xref_size", "/Size is zero");
        return;
    }
    if let Some((root, _)) = newest.root {
        if root >= size {
            out.fail(
                "xref_size",
                format!("/Root object {root} is outside /Size {size}"),
            );
            return;
        }
    }
    let mut seen = HashSet::new();
    for section in sections {
        for (number, _) in &section.entries {
            if *number >= size && seen.insert(*number) {
                out.fail(
                    "xref_size",
                    format!("object {number} is outside /Size {size}"),
                );
            }
        }
    }
    if !out.failed {
        out.pass("xref_size", format!("/Size {size} covers every xref entry"));
    }
}

fn check_root(
    out: &mut Collector,
    data: &[u8],
    entries: &[(u64, ObjEntry)],
    newest: &Section,
    limits: ValidationLimits,
) {
    let Some((number, generation)) = newest.root else {
        out.fail(
            "root_catalog",
            "selected trailer has no direct /Root reference",
        );
        return;
    };
    let Some(entry) = entries
        .iter()
        .find(|(obj_number, _)| *obj_number == number)
        .map(|(_, entry)| entry)
    else {
        out.fail(
            "root_catalog",
            format!("/Root {number} {generation} R is absent from the xref"),
        );
        return;
    };
    let body: Vec<u8> = match entry {
        ObjEntry::Uncompressed {
            offset,
            generation: gen,
        } => {
            let Ok(offset) = usize::try_from(*offset) else {
                out.fail(
                    "root_catalog",
                    format!("/Root {number} offset is out of range"),
                );
                return;
            };
            match object_body(data, offset, number, *gen) {
                Ok(range) => data.get(range).unwrap_or_default().to_vec(),
                Err(issue) => {
                    out.issue(issue);
                    return;
                }
            }
        }
        ObjEntry::Compressed {
            object_stream,
            index,
        } => match compressed_body(data, entries, *object_stream, *index, number, limits) {
            Ok((buffer, range)) => buffer.get(range).unwrap_or_default().to_vec(),
            Err(issue) => {
                out.issue(issue);
                return;
            }
        },
    };
    let body_range = 0..body.len();
    match name_value(&body, body_range, b"/Type") {
        Some(b"/Catalog") => out.pass(
            "root_catalog",
            format!("/Root {number} {generation} R resolves to /Type /Catalog"),
        ),
        Some(other) => out.fail(
            "root_catalog",
            format!(
                "/Root {number} {generation} R has /Type {} instead of /Catalog",
                String::from_utf8_lossy(other)
            ),
        ),
        None => out.fail(
            "root_catalog",
            format!("/Root {number} {generation} R has no /Type entry"),
        ),
    }
}

fn parse_section(data: &[u8], offset: u64, limits: ValidationLimits) -> Result<Section, Issue> {
    let start = usize::try_from(offset).map_err(|_| {
        Issue::fail(
            "startxref_target",
            "startxref does not fit the address space",
        )
    })?;
    let pos = skip_ws(data, start);
    if starts_token(data, pos, b"xref") {
        parse_classic(data, pos, limits)
    } else {
        let (number, generation, body) = object_header(data, pos)?;
        if number == 0 {
            return Err(Issue::fail(
                "startxref_target",
                "startxref points at object 0",
            ));
        }
        let _ = generation;
        parse_xref_stream(data, body, limits)
    }
}

fn parse_classic(data: &[u8], start: usize, limits: ValidationLimits) -> Result<Section, Issue> {
    let mut pos = start + b"xref".len();
    let mut entries = Vec::new();
    let mut max_entries = max_entries(limits);
    loop {
        pos = skip_ws(data, pos);
        if starts_token(data, pos, b"trailer") {
            break;
        }
        let (first, next) = parse_uint(data, pos).ok_or_else(|| {
            Issue::fail("xref_entries", "malformed classic xref subsection header")
        })?;
        let (count, next) = parse_uint(data, next).ok_or_else(|| {
            Issue::fail("xref_entries", "malformed classic xref subsection count")
        })?;
        let count = usize::try_from(count).map_err(|_| {
            Issue::unsupported(
                "xref_entries",
                "xref subsection count exceeds address space",
            )
        })?;
        if count > max_entries {
            return Err(Issue::unsupported(
                "xref_entries",
                format!("xref subsection has {count} entries, budget allows {max_entries}"),
            ));
        }
        max_entries = max_entries.saturating_sub(count);
        pos = next;
        for index in 0..count {
            let object_number = first.checked_add(u64::try_from(index).unwrap_or(u64::MAX));
            let object_number = object_number
                .ok_or_else(|| Issue::fail("xref_entries", "xref object number overflow"))?;
            let (offset, next) = parse_uint(data, pos).ok_or_else(|| {
                Issue::fail(
                    "xref_entries",
                    format!("xref entry {object_number} has no offset"),
                )
            })?;
            let (generation, next) = parse_uint(data, next).ok_or_else(|| {
                Issue::fail(
                    "xref_entries",
                    format!("xref entry {object_number} has no generation"),
                )
            })?;
            let kind_pos = skip_ws(data, next);
            let kind = *data.get(kind_pos).ok_or_else(|| {
                Issue::fail(
                    "xref_entries",
                    format!("xref entry {object_number} has no n/f field"),
                )
            })?;
            pos = kind_pos + 1;
            if kind == b'n' {
                let generation = u16::try_from(generation).map_err(|_| {
                    Issue::fail(
                        "xref_entries",
                        format!("xref entry {object_number} has generation {generation}"),
                    )
                })?;
                object_body(
                    data,
                    usize::try_from(offset).map_err(|_| {
                        Issue::fail(
                            "xref_entries",
                            format!("object {object_number} offset is out of range"),
                        )
                    })?,
                    object_number,
                    generation,
                )
                .map_err(|issue| match issue {
                    Issue::Fail(_, reason) => Issue::fail(
                        "xref_entries",
                        format!("object {object_number} xref entry: {reason}"),
                    ),
                    other => other,
                })?;
                entries.push((object_number, ObjEntry::Uncompressed { offset, generation }));
            } else if kind != b'f' {
                return Err(Issue::fail(
                    "xref_entries",
                    format!("xref entry {object_number} has invalid type byte"),
                ));
            }
        }
    }

    let trailer = find_token(data, pos, data.len(), b"trailer")
        .ok_or_else(|| Issue::fail("xref_entries", "classic xref has no trailer dictionary"))?;
    let dict_end = find_token(data, trailer, data.len(), b"startxref")
        .unwrap_or_else(|| data.len().min(trailer.saturating_add(MAX_OBJECT_SCAN)));
    let dict = trailer + b"trailer".len()..dict_end;
    common_fields(data, dict, entries, StreamKind::Classic)
}

struct XrefStreamHeader {
    dict: std::ops::Range<usize>,
    pos: usize,
    stream_end: usize,
}

fn xref_stream_header(data: &[u8], body: usize) -> Result<XrefStreamHeader, Issue> {
    let stream = find_token(
        data,
        body,
        data.len().min(body + MAX_OBJECT_SCAN),
        b"stream",
    )
    .ok_or_else(|| Issue::fail("xref_stream", "xref stream object has no stream keyword"))?;
    let dict = body..stream;
    let pos = skip_eol(data, stream + b"stream".len());
    let length = direct_int(data, dict.clone(), b"/Length").ok_or_else(|| {
        Issue::unsupported(
            "stream_length",
            "xref stream /Length is not a direct integer",
        )
    })?;
    let length = usize::try_from(length)
        .map_err(|_| Issue::fail("stream_length", "xref stream /Length exceeds address space"))?;
    let stream_end = pos
        .checked_add(length)
        .ok_or_else(|| Issue::fail("stream_length", "xref stream /Length overflows"))?;
    if stream_end > data.len() {
        return Err(Issue::fail(
            "stream_length",
            "xref stream /Length extends past the selected PDF revision",
        ));
    }
    if !stream_length_matches(data, stream_end) {
        return Err(Issue::fail(
            "stream_length",
            "direct /Length does not agree with the endstream position",
        ));
    }
    Ok(XrefStreamHeader {
        dict,
        pos,
        stream_end,
    })
}

fn xref_widths(data: &[u8], dict: &std::ops::Range<usize>) -> Result<(u64, u64, u64), Issue> {
    let widths = array_ints(data, dict.clone(), b"/W")
        .ok_or_else(|| Issue::fail("xref_stream", "xref stream has no direct /W array"))?;
    let [w1, w2, w3] = widths.as_slice() else {
        return Err(Issue::fail(
            "xref_stream",
            "xref stream /W must contain three field widths",
        ));
    };
    if [w1, w2, w3].iter().any(|width| **width > 8) {
        return Err(Issue::unsupported(
            "xref_stream",
            "xref stream /W field wider than 64 bits",
        ));
    }
    let (w1, w2, w3) = (*w1, *w2, *w3);
    if w1.saturating_add(w2).saturating_add(w3) == 0 {
        return Err(Issue::fail(
            "xref_stream",
            "xref stream /W fields are all zero",
        ));
    }
    Ok((w1, w2, w3))
}

fn xref_index(data: &[u8], dict: &std::ops::Range<usize>, size: u64) -> Result<Vec<u64>, Issue> {
    let index = array_ints(data, dict.clone(), b"/Index").unwrap_or_else(|| vec![0, size]);
    if index.len() % 2 != 0 {
        return Err(Issue::fail(
            "xref_entries",
            "xref stream /Index has an odd number of fields",
        ));
    }
    Ok(index)
}

fn decode_xref_entries(
    data: &[u8],
    payload: &[u8],
    index: &[u64],
    widths: (u64, u64, u64),
    limits: ValidationLimits,
) -> Result<Vec<(u64, ObjEntry)>, Issue> {
    let (w1, w2, w3) = widths;
    let mut entries = Vec::new();
    let mut budget = max_entries(limits);
    let mut cursor = 0usize;
    for pair in index.chunks(2) {
        let first = *pair.first().ok_or_else(|| {
            Issue::fail("xref_entries", "xref stream /Index has an empty field pair")
        })?;
        let count = *pair.get(1).ok_or_else(|| {
            Issue::fail(
                "xref_entries",
                "xref stream /Index has an odd number of fields",
            )
        })?;
        let count = usize::try_from(count).map_err(|_| {
            Issue::unsupported(
                "xref_entries",
                "xref stream range count exceeds address space",
            )
        })?;
        if count > budget {
            return Err(Issue::unsupported(
                "xref_entries",
                format!("xref stream has {count} entries, budget allows {budget}"),
            ));
        }
        budget -= count;
        for item in 0..count {
            let number = first.checked_add(u64::try_from(item).unwrap_or(u64::MAX));
            let number = number
                .ok_or_else(|| Issue::fail("xref_entries", "xref stream object number overflow"))?;
            let (field_type, next) = read_be(payload, cursor, w1);
            let (field_two, next) = read_be(payload, next, w2);
            let (field_three, next) = read_be(payload, next, w3);
            cursor = next;
            match field_type {
                0 => {}
                1 => {
                    let offset = usize::try_from(field_two).map_err(|_| {
                        Issue::fail(
                            "xref_entries",
                            format!("object {number} offset is out of range"),
                        )
                    })?;
                    let generation = u16::try_from(field_three).map_err(|_| {
                        Issue::fail(
                            "xref_entries",
                            format!("object {number} generation is out of range"),
                        )
                    })?;
                    object_body(data, offset, number, generation).map_err(|issue| match issue {
                        Issue::Fail(_, reason) => Issue::fail(
                            "xref_entries",
                            format!("object {number} xref entry: {reason}"),
                        ),
                        other => other,
                    })?;
                    entries.push((
                        number,
                        ObjEntry::Uncompressed {
                            offset: field_two,
                            generation,
                        },
                    ));
                }
                2 => entries.push((
                    number,
                    ObjEntry::Compressed {
                        object_stream: field_two,
                        index: field_three,
                    },
                )),
                other => {
                    return Err(Issue::fail(
                        "xref_entries",
                        format!("object {number} has unsupported xref type {other}"),
                    ))
                }
            }
        }
    }
    Ok(entries)
}

fn parse_xref_stream(data: &[u8], body: usize, limits: ValidationLimits) -> Result<Section, Issue> {
    let header = xref_stream_header(data, body)?;
    let filter = stream_filter(data, header.dict.clone())?;
    let payload = inflate_bounded(
        data.get(header.pos..header.stream_end).unwrap_or_default(),
        filter,
        max_inflate(limits),
        "xref_stream",
    )?;
    let widths = xref_widths(data, &header.dict)?;
    let size = direct_int(data, header.dict.clone(), b"/Size")
        .ok_or_else(|| Issue::fail("xref_size", "xref stream has no direct /Size"))?;
    let index = xref_index(data, &header.dict, size)?;
    let entries = decode_xref_entries(data, &payload, &index, widths, limits)?;
    common_fields(data, header.dict, entries, StreamKind::Xref)
}

fn common_fields(
    data: &[u8],
    dict: std::ops::Range<usize>,
    entries: Vec<(u64, ObjEntry)>,
    kind: StreamKind,
) -> Result<Section, Issue> {
    let type_name = name_value(data, dict.clone(), b"/Type");
    if kind == StreamKind::Xref && type_name != Some(b"/XRef".as_slice()) {
        return Err(Issue::fail(
            "xref_stream",
            format!(
                "xref stream /Type is {} instead of /XRef",
                String::from_utf8_lossy(type_name.unwrap_or_default())
            ),
        ));
    }
    let size = direct_int(data, dict.clone(), b"/Size");
    let root = ref_value(data, dict.clone(), b"/Root");
    let prev = direct_int(data, dict.clone(), b"/Prev");
    let xref_stm = direct_int(data, dict.clone(), b"/XRefStm");
    let encrypt = find_key(data, dict.start, dict.end, b"/Encrypt").is_some();
    Ok(Section {
        entries,
        size,
        root,
        prev,
        xref_stm,
        encrypt,
        kind,
    })
}

fn object_header(data: &[u8], offset: usize) -> Result<(u64, u16, usize), Issue> {
    let pos = skip_ws(data, offset);
    let (number, next) = parse_uint(data, pos)
        .ok_or_else(|| Issue::fail("startxref_target", "expected an object header at startxref"))?;
    let (generation, next) = parse_uint(data, next)
        .ok_or_else(|| Issue::fail("startxref_target", "object header has no generation"))?;
    let obj_token = skip_ws(data, next);
    if !starts_token(data, obj_token, b"obj") {
        return Err(Issue::fail(
            "startxref_target",
            "startxref does not point at an object header",
        ));
    }
    let pos = obj_token + b"obj".len();
    let generation = u16::try_from(generation)
        .map_err(|_| Issue::fail("startxref_target", "object generation is out of range"))?;
    Ok((number, generation, pos))
}

fn object_body(
    data: &[u8],
    offset: usize,
    expected_number: u64,
    expected_generation: u16,
) -> Result<std::ops::Range<usize>, Issue> {
    let (number, generation, body) = object_header(data, offset)?;
    if number != expected_number || generation != expected_generation {
        return Err(Issue::fail(
            "startxref_target",
            format!(
                "expected {expected_number} {expected_generation} obj at byte {offset}, found {number} {generation}"
            ),
        ));
    }
    let end = find_token(
        data,
        body,
        data.len().min(body + MAX_OBJECT_SCAN),
        b"endobj",
    )
    .or_else(|| {
        find_token(
            data,
            body,
            data.len().min(body + MAX_OBJECT_SCAN),
            b"stream",
        )
    })
    .unwrap_or_else(|| data.len().min(body + MAX_OBJECT_SCAN));
    Ok(body..end)
}

struct ObjectStream {
    decoded: Vec<u8>,
    first: usize,
    numbers: Vec<u64>,
    offsets: Vec<u64>,
}

fn validate_compressed_entries(
    data: &[u8],
    entries: &[(u64, ObjEntry)],
    limits: ValidationLimits,
) -> Result<(), Issue> {
    let mut streams = HashMap::new();
    for (number, entry) in entries {
        let ObjEntry::Compressed {
            object_stream,
            index,
        } = entry
        else {
            continue;
        };
        if !streams.contains_key(object_stream) {
            let loaded = load_object_stream(data, entries, *object_stream, limits)?;
            streams.insert(*object_stream, loaded);
        }
        let stream = streams.get(object_stream).ok_or_else(|| {
            Issue::fail(
                "xref_entries",
                format!("object stream {object_stream} was not loaded"),
            )
        })?;
        object_stream_range(stream, *index, *number)?;
    }
    Ok(())
}

fn compressed_body(
    data: &[u8],
    entries: &[(u64, ObjEntry)],
    object_stream: u64,
    index: u64,
    expected_number: u64,
    limits: ValidationLimits,
) -> Result<(Vec<u8>, std::ops::Range<usize>), Issue> {
    let stream = load_object_stream(data, entries, object_stream, limits)?;
    let range = object_stream_range(&stream, index, expected_number)?;
    Ok((stream.decoded, range))
}

fn load_object_stream(
    data: &[u8],
    entries: &[(u64, ObjEntry)],
    object_stream: u64,
    limits: ValidationLimits,
) -> Result<ObjectStream, Issue> {
    let entry = entries
        .iter()
        .find(|(number, _)| *number == object_stream)
        .map(|(_, entry)| entry)
        .ok_or_else(|| {
            Issue::fail(
                "xref_entries",
                format!("compressed object references missing object stream {object_stream}"),
            )
        })?;
    let (offset, generation) = match entry {
        ObjEntry::Uncompressed { offset, generation } => (*offset, *generation),
        ObjEntry::Compressed { .. } => {
            return Err(Issue::unsupported(
                "xref_entries",
                "object stream is itself compressed",
            ))
        }
    };
    let offset = usize::try_from(offset).map_err(|_| {
        Issue::fail(
            "xref_entries",
            format!("object stream {object_stream} offset is out of range"),
        )
    })?;
    let body = object_body(data, offset, object_stream, generation)?;
    let stream = find_token(data, body.start, body.end, b"stream")
        .ok_or_else(|| Issue::fail("xref_entries", "object stream has no stream keyword"))?;
    let dict = body.start..stream;
    if name_value(data, dict.clone(), b"/Type") != Some(b"/ObjStm".as_slice()) {
        return Err(Issue::fail(
            "xref_entries",
            format!("object {object_stream} is not an /ObjStm"),
        ));
    }
    let count = direct_int(data, dict.clone(), b"/N")
        .ok_or_else(|| Issue::unsupported("xref_entries", "object stream has no direct /N"))?;
    let first = direct_int(data, dict.clone(), b"/First")
        .ok_or_else(|| Issue::unsupported("xref_entries", "object stream has no direct /First"))?;
    let length = direct_int(data, dict.clone(), b"/Length").ok_or_else(|| {
        Issue::unsupported("stream_length", "object stream /Length is not direct")
    })?;
    let filter = stream_filter(data, dict)?;
    let pos = skip_eol(data, stream + b"stream".len());
    let length = usize::try_from(length).map_err(|_| {
        Issue::fail(
            "stream_length",
            "object stream /Length exceeds address space",
        )
    })?;
    let end = pos
        .checked_add(length)
        .ok_or_else(|| Issue::fail("stream_length", "object stream /Length overflows"))?;
    if end > data.len() {
        return Err(Issue::fail(
            "stream_length",
            "object stream /Length extends past the selected PDF revision",
        ));
    }
    if !stream_length_matches(data, end) {
        return Err(Issue::fail(
            "stream_length",
            "object stream /Length does not agree with endstream",
        ));
    }
    let decoded = inflate_bounded(
        data.get(pos..end).unwrap_or_default(),
        filter,
        max_inflate(limits),
        "xref_entries",
    )?;
    let first = usize::try_from(first)
        .map_err(|_| Issue::fail("xref_entries", "object stream /First exceeds address space"))?;
    let count = usize::try_from(count).map_err(|_| {
        Issue::unsupported("xref_entries", "object stream /N exceeds address space")
    })?;
    if count > MAX_HEADER_PAIRS {
        return Err(Issue::unsupported(
            "xref_entries",
            format!("object stream has {count} headers, budget allows {MAX_HEADER_PAIRS}"),
        ));
    }
    let mut cursor = 0usize;
    let mut numbers = Vec::new();
    let mut offsets = Vec::new();
    for _ in 0..count {
        let (number, next) = parse_uint(&decoded, cursor).ok_or_else(|| {
            Issue::fail(
                "xref_entries",
                "object stream header has a malformed object number",
            )
        })?;
        let (offset, next) = parse_uint(&decoded, next).ok_or_else(|| {
            Issue::fail(
                "xref_entries",
                "object stream header has a malformed offset",
            )
        })?;
        numbers.push(number);
        offsets.push(offset);
        cursor = next;
    }
    Ok(ObjectStream {
        decoded,
        first,
        numbers,
        offsets,
    })
}

fn object_stream_range(
    stream: &ObjectStream,
    index: u64,
    expected_number: u64,
) -> Result<std::ops::Range<usize>, Issue> {
    let index = usize::try_from(index)
        .map_err(|_| Issue::fail("xref_entries", "compressed index exceeds address space"))?;
    let number = *stream.numbers.get(index).ok_or_else(|| {
        Issue::fail(
            "xref_entries",
            format!("compressed index {index} is outside the object stream"),
        )
    })?;
    if number != expected_number {
        return Err(Issue::fail(
            "xref_entries",
            format!("compressed index {index} holds object {number}, expected {expected_number}"),
        ));
    }
    let offset = *stream
        .offsets
        .get(index)
        .ok_or_else(|| Issue::fail("xref_entries", "compressed object offset is missing"))?;
    let offset = usize::try_from(offset).map_err(|_| {
        Issue::fail(
            "xref_entries",
            "compressed object offset exceeds address space",
        )
    })?;
    let start = stream
        .first
        .checked_add(offset)
        .ok_or_else(|| Issue::fail("xref_entries", "compressed object offset overflows"))?;
    let end = stream
        .offsets
        .iter()
        .filter_map(|value| usize::try_from(*value).ok())
        .filter(|value| *value > offset)
        .min()
        .and_then(|value| stream.first.checked_add(value))
        .unwrap_or(stream.decoded.len())
        .min(stream.decoded.len());
    if start > end || start > stream.decoded.len() {
        return Err(Issue::fail(
            "xref_entries",
            "compressed object offset is outside the inflated object stream",
        ));
    }
    Ok(start..end)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    Raw,
    Flate,
}

fn stream_filter(data: &[u8], dict: std::ops::Range<usize>) -> Result<Filter, Issue> {
    let Some(filter) = name_value(data, dict, b"/Filter") else {
        return Ok(Filter::Raw);
    };
    match filter {
        b"/FlateDecode" | b"/Fl" => Ok(Filter::Flate),
        other => Err(Issue::unsupported(
            "xref_stream",
            format!(
                "unsupported stream filter {}; only FlateDecode is validated",
                String::from_utf8_lossy(other)
            ),
        )),
    }
}

fn inflate_bounded(
    input: &[u8],
    filter: Filter,
    max: usize,
    check: &'static str,
) -> Result<Vec<u8>, Issue> {
    if filter == Filter::Raw {
        if input.len() > max {
            return Err(Issue::unsupported(
                check,
                format!("stream has {} bytes, budget allows {max}", input.len()),
            ));
        }
        return Ok(input.to_vec());
    }
    let mut decoder = Decompress::new(true);
    let mut out = Vec::new();
    let mut buffer = vec![0u8; INFLATE_BUFFER];
    let mut pos = 0usize;
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(
                input.get(pos..).unwrap_or_default(),
                &mut buffer,
                FlushDecompress::None,
            )
            .map_err(|e| Issue::fail(check, format!("stream inflate failed: {e}")))?;
        let consumed = usize::try_from(decoder.total_in() - before_in).unwrap_or(usize::MAX);
        let written = usize::try_from(decoder.total_out() - before_out).unwrap_or(usize::MAX);
        pos = pos.saturating_add(consumed);
        let allowed = max.saturating_sub(out.len());
        if written > allowed {
            return Err(Issue::unsupported(
                check,
                format!("inflated stream exceeds the {max} byte budget"),
            ));
        }
        out.extend_from_slice(buffer.get(..written).unwrap_or_default());
        if status == InflateStatus::StreamEnd {
            return Ok(out);
        }
        if consumed == 0 && written == 0 {
            return Err(Issue::fail(check, "stream inflate made no progress"));
        }
    }
}

fn max_entries(limits: ValidationLimits) -> usize {
    usize::try_from(limits.max_validator_memory)
        .unwrap_or(usize::MAX)
        .checked_div(20)
        .unwrap_or(MAX_XREF_ENTRIES)
        .clamp(1, MAX_XREF_ENTRIES)
}

fn max_inflate(limits: ValidationLimits) -> usize {
    usize::try_from(limits.max_validator_memory)
        .unwrap_or(usize::MAX)
        .clamp(1, MAX_XREF_STREAM_BYTES)
}

fn find_startxref(data: &[u8]) -> Option<u64> {
    let pos = memchr::memmem::rfind(data, b"startxref")?;
    let pos = skip_ws(data, pos + b"startxref".len());
    let (value, _) = parse_uint(data, pos)?;
    Some(value)
}

fn find_token(data: &[u8], start: usize, end: usize, token: &[u8]) -> Option<usize> {
    let slice = data.get(start..end.min(data.len()))?;
    let mut from = 0usize;
    while let Some(relative) = memchr::memmem::find(slice.get(from..)?, token) {
        let pos = start + from + relative;
        let before = pos.checked_sub(1).and_then(|p| data.get(p)).copied();
        let after = data.get(pos + token.len()).copied();
        if before.is_none_or(is_delim) && after.is_none_or(is_delim) {
            return Some(pos);
        }
        from = from.saturating_add(relative).saturating_add(1);
    }
    None
}

fn find_key(data: &[u8], start: usize, end: usize, key: &[u8]) -> Option<usize> {
    let slice = data.get(start..end.min(data.len()))?;
    let mut from = 0usize;
    while let Some(relative) = memchr::memmem::find(slice.get(from..)?, key) {
        let pos = start + from + relative;
        let before = pos.checked_sub(1).and_then(|p| data.get(p)).copied();
        let after = data.get(pos + key.len()).copied();
        if before.is_none_or(is_delim) && after.is_none_or(is_delim) {
            return Some(pos + key.len());
        }
        from = from.saturating_add(relative).saturating_add(1);
    }
    None
}

fn skip_ws(data: &[u8], mut pos: usize) -> usize {
    while data
        .get(pos)
        .is_some_and(|b| b.is_ascii_whitespace() || *b == 0)
    {
        pos += 1;
    }
    pos
}

fn skip_ws_comments(data: &[u8], mut pos: usize, end: usize) -> usize {
    loop {
        pos = skip_ws(data, pos).min(end);
        if data.get(pos) != Some(&b'%') || pos >= end {
            return pos;
        }
        while pos < end && !data.get(pos).is_some_and(|b| *b == b'\n' || *b == b'\r') {
            pos += 1;
        }
    }
}

fn skip_eol(data: &[u8], pos: usize) -> usize {
    if data.get(pos) == Some(&b'\r') && data.get(pos + 1) == Some(&b'\n') {
        pos + 2
    } else if data.get(pos).is_some_and(|b| *b == b'\r' || *b == b'\n') {
        pos + 1
    } else {
        pos
    }
}

fn stream_length_matches(data: &[u8], stream_end: usize) -> bool {
    if starts_token(data, stream_end, b"endstream") {
        // A trailing EOL belongs after the stream data, not inside /Length.
        return !stream_end
            .checked_sub(1)
            .and_then(|pos| data.get(pos))
            .is_some_and(|byte| *byte == b'\r' || *byte == b'\n');
    }
    let after_eol = skip_eol(data, stream_end);
    after_eol > stream_end && starts_token(data, after_eol, b"endstream")
}

fn is_delim(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
}

fn starts_token(data: &[u8], pos: usize, token: &[u8]) -> bool {
    data.get(pos..pos.saturating_add(token.len())) == Some(token)
        && data
            .get(pos + token.len())
            .is_none_or(|byte: &u8| is_delim(*byte))
}

fn parse_uint(data: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    pos = skip_ws(data, pos);
    let start = pos;
    let mut value = 0u64;
    while let Some(byte) = data.get(pos) {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value.checked_mul(10)?.checked_add(u64::from(byte - b'0'))?;
        pos += 1;
    }
    if pos == start {
        None
    } else {
        Some((value, pos))
    }
}

fn read_be(data: &[u8], pos: usize, width: u64) -> (u64, usize) {
    let width = usize::try_from(width).unwrap_or(usize::MAX).min(8);
    let mut value = 0u64;
    for byte in data.get(pos..pos.saturating_add(width)).unwrap_or_default() {
        value = value.checked_shl(8).unwrap_or(0) | u64::from(*byte);
    }
    (value, pos.saturating_add(width))
}

fn name_value<'a>(data: &'a [u8], dict: std::ops::Range<usize>, key: &[u8]) -> Option<&'a [u8]> {
    let pos = find_key(data, dict.start, dict.end, key)?;
    let pos = skip_ws_comments(data, pos, dict.end);
    if data.get(pos) != Some(&b'/') || pos >= dict.end {
        return None;
    }
    let mut end = pos + 1;
    while end < dict.end && !data.get(end).is_some_and(|byte: &u8| is_delim(*byte)) {
        end += 1;
    }
    data.get(pos..end)
}

fn direct_int(data: &[u8], dict: std::ops::Range<usize>, key: &[u8]) -> Option<u64> {
    let pos = find_key(data, dict.start, dict.end, key)?;
    let (value, _) = parse_uint(data, skip_ws_comments(data, pos, dict.end))?;
    Some(value)
}

fn ref_value(data: &[u8], dict: std::ops::Range<usize>, key: &[u8]) -> Option<(u64, u16)> {
    let pos = find_key(data, dict.start, dict.end, key)?;
    let (number, pos) = parse_uint(data, skip_ws_comments(data, pos, dict.end))?;
    let (generation, pos) = parse_uint(data, skip_ws(data, pos))?;
    if !starts_token(data, skip_ws(data, pos), b"R") {
        return None;
    }
    Some((number, u16::try_from(generation).ok()?))
}

fn array_ints(data: &[u8], dict: std::ops::Range<usize>, key: &[u8]) -> Option<Vec<u64>> {
    let pos = find_key(data, dict.start, dict.end, key)?;
    let mut pos = skip_ws_comments(data, pos, dict.end);
    if data.get(pos) != Some(&b'[') {
        return None;
    }
    pos += 1;
    let mut values = Vec::new();
    loop {
        pos = skip_ws_comments(data, pos, dict.end);
        match data.get(pos) {
            Some(b']') => return Some(values),
            Some(byte) if byte.is_ascii_digit() => {
                let (value, next) = parse_uint(data, pos)?;
                values.push(value);
                pos = next;
            }
            _ => return None,
        }
    }
}

/// Bounded `/Kids [ n g R ... ]` reader for the repair page-tree check.
fn array_refs(data: &[u8], dict: std::ops::Range<usize>, key: &[u8]) -> Option<Vec<(u64, u16)>> {
    let pos = find_key(data, dict.start, dict.end, key)?;
    let mut pos = skip_ws_comments(data, pos, dict.end);
    if data.get(pos) != Some(&b'[') {
        return None;
    }
    pos += 1;
    let mut values = Vec::new();
    loop {
        pos = skip_ws_comments(data, pos, dict.end);
        match data.get(pos) {
            Some(b']') => return Some(values),
            Some(byte) if byte.is_ascii_digit() => {
                let (number, next) = parse_uint(data, pos)?;
                let (generation, next) = parse_uint(data, skip_ws(data, next))?;
                let rpos = skip_ws(data, next);
                if !starts_token(data, rpos, b"R") {
                    return None;
                }
                pos = rpos.saturating_add(1);
                if values.len() >= MAX_PAGE_TREE_KIDS {
                    return Some(values);
                }
                values.push((number, u16::try_from(generation).ok()?));
            }
            _ => return None,
        }
    }
}
