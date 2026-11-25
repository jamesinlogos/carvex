#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
use super::{Analysis, Need, PdfEofMode, ValidationLimits};

const EOF_SUFFIX: &[u8] = b"\n%%EOF\n";

#[derive(Debug)]
struct EofCandidate {
    marker_pos: usize,
    end: usize,
    startxref: Option<u64>,
    xref_target: bool,
    objects: u32,
    has_root: bool,
}

impl EofCandidate {
    fn plausible(&self) -> bool {
        self.objects > 0 && (self.xref_target || self.has_root)
    }
}

/// Analyze a PDF candidate with the default decode budget.
pub fn analyze(data: &[u8], at_limit: bool, mode: PdfEofMode) -> Need {
    analyze_with_limits(data, at_limit, mode, ValidationLimits::default())
}

/// Analyze a PDF candidate and return structural parse results (H-25).
pub fn analyze_with_limits(
    data: &[u8],
    at_limit: bool,
    mode: PdfEofMode,
    limits: ValidationLimits,
) -> Need {
    if data.len() < 8 {
        return if at_limit {
            Need::Reject("too short for PDF".into())
        } else {
            Need::More
        };
    }
    if data.get(0..5) != Some(b"%PDF-") {
        return Need::Reject("missing PDF header".into());
    }
    let (Some(&major), Some(&dot), Some(&minor)) = (data.get(5), data.get(6), data.get(7)) else {
        return if at_limit {
            Need::Reject("truncated PDF header".into())
        } else {
            Need::More
        };
    };
    if !(major.is_ascii_digit() && dot == b'.' && minor.is_ascii_digit()) {
        return Need::Reject("invalid PDF version in header".into());
    }
    let version = format!("{}.{}", major as char, minor as char);

    // A `%PDF-` header immediately after a `%%EOF` starts the next document.
    // A `%PDF-` anywhere else (for example inside an uncompressed attachment)
    // belongs to this document and must not cut it short.
    let boundaries = document_boundaries(data);
    let search_end = boundaries.first().copied().unwrap_or(data.len());
    if search_end <= 8 {
        return if at_limit {
            Need::Reject("PDF document is empty".into())
        } else {
            Need::More
        };
    }

    let eofs = find_eofs(data.get(..search_end).unwrap_or(data));
    if eofs.is_empty() {
        if !at_limit && boundaries.is_empty() {
            return Need::More;
        }
        let (end, bound_note) = conservative_truncated_end(data, search_end, limits);
        let mut need = truncated(
            end,
            true,
            data.get(..end).unwrap_or(data),
            &version,
            if boundaries.is_empty() {
                "no %%EOF marker within the scan limit"
            } else {
                "next document header before any %%EOF"
            },
        );
        if let (Need::Analyzed(analysis), Some(note)) = (&mut need, bound_note) {
            analysis.notes.push(note);
        }
        return need;
    }

    let chosen_idx = match mode {
        PdfEofMode::First => 0,
        PdfEofMode::Last => eofs.len() - 1,
        PdfEofMode::Smart => eofs
            .iter()
            .rposition(EofCandidate::plausible)
            .unwrap_or(eofs.len() - 1),
    };
    let Some(chosen) = eofs.get(chosen_idx) else {
        return Need::Reject("PDF internal EOF selection error".into());
    };
    if chosen.objects == 0 {
        return Need::Reject("PDF has no object headers".into());
    }

    // The initial read window is deliberately small.  If the selected footer
    // touches the end of the buffer, or the bytes immediately after it look
    // like an incremental update, ask for more before declaring completion.
    if mode == PdfEofMode::Smart && !at_limit {
        let at_buffer_end = chosen.end >= data.len().saturating_sub(1);
        let tail = data.get(chosen.end..search_end).unwrap_or(&[]);
        // Only ASCII whitespace up to the end of the current window means the
        // document may continue beyond it (linearized files put page 1 before
        // the rest of the document).  NUL padding, JPEG bytes or other data
        // after %%EOF end the candidate immediately and must not trigger
        // unbounded read-ahead.
        let whitespace_to_window_end =
            search_end == data.len() && tail.iter().all(u8::is_ascii_whitespace);
        if at_buffer_end || whitespace_to_window_end || looks_like_more_revision(tail) {
            return Need::More;
        }
    }

    let has_xref = eofs.iter().any(|e| e.xref_target)
        || memchr::memmem::find(data.get(..chosen.end).unwrap_or(data), b"/XRef").is_some();
    let mut score = 50i32;
    score += 10; // objects > 0
    if chosen.startxref.is_some() && chosen.xref_target {
        score += 20;
    } else if chosen.startxref.is_some() {
        score += 5;
    }
    if chosen.has_root {
        score += 10;
    }
    if has_xref {
        score += 10;
    }
    score = score.clamp(0, 100);

    let mut notes = vec![format!(
        "PDF {version}, {} object(s), {} %%EOF candidate(s), selected revision {}",
        chosen.objects,
        eofs.len(),
        chosen_idx + 1
    )];
    if let Some(startxref) = chosen.startxref {
        notes.push(format!(
            "startxref={startxref} ({})",
            if chosen.xref_target {
                "target validated"
            } else {
                "target not an xref/object"
            }
        ));
    } else {
        notes.push("no startxref before selected %%EOF".into());
    }
    let _ = chosen.marker_pos;
    Need::Analyzed(Analysis {
        end: chosen.end,
        complete: true,
        score: score as u8,
        width: None,
        height: None,
        notes,
        repair_suffix: EOF_SUFFIX,
        integrity_checks: Vec::new(),
        validated_prefix: 0,
    })
}

/// Positions of `%PDF-` headers that start a new document.
///
/// A header starts a new document when it follows a `%%EOF` (whitespace
/// allowed), or when it is not inside an uncompressed `stream`.  An embedded
/// PDF attachment is normally stored inside a stream, so its header must not
/// cut the containing document short.  The first document is never a boundary.
fn document_boundaries(data: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut inside_stream = false;
    let mut scanned = 0usize;
    for pos in memchr::memmem::find_iter(data, b"%PDF-") {
        // Track `stream`/`endstream` tokens between the previous header and
        // this one.  This keeps the whole walk linear in the document size.
        let from = scanned.min(pos);
        for m in memchr::memmem::find_iter(data.get(from..pos).unwrap_or_default(), b"stream") {
            let abs = from + m;
            let is_end = abs >= 3 && data.get(abs - 3..abs) == Some(b"end");
            inside_stream = !is_end;
        }
        scanned = pos;
        if pos == 0 {
            continue;
        }
        if data.get(pos + 5..pos + 8).is_none_or(|v| {
            !(v.first().is_some_and(u8::is_ascii_digit)
                && v.get(1) == Some(&b'.')
                && v.get(2).is_some_and(u8::is_ascii_digit))
        }) {
            continue;
        }
        let mut q = pos;
        while q > 0 && data.get(q - 1).is_some_and(u8::is_ascii_whitespace) {
            q -= 1;
        }
        let after_eof = q >= 5 && data.get(q - 5..q) == Some(b"%%EOF");
        if after_eof || !inside_stream {
            out.push(pos);
            // A new document cannot inherit an open stream from the previous
            // one; reset so a missing `endstream` cannot hide later boundaries.
            inside_stream = false;
        }
        if out.len() >= 1024 {
            break;
        }
    }
    out
}

fn looks_like_more_revision(tail: &[u8]) -> bool {
    // Bounded window (H-07): revision markers appear immediately after %%EOF;
    // keep the scan small to avoid per-candidate overhead.
    let window = tail.get(..tail.len().min(64)).unwrap_or(tail);
    if window.iter().all(|b| *b == 0 || b.is_ascii_whitespace()) {
        return false;
    }
    [b"xref".as_slice(), b"startxref", b"trailer", b" obj"]
        .iter()
        .any(|needle| memchr::memmem::find(window, needle).is_some())
}

fn find_eofs(data: &[u8]) -> Vec<EofCandidate> {
    // Precompute once: " obj" offsets and first /Root position. This keeps the
    // per-EOF work O(log N) instead of rescanning the whole prefix (H-07/X04).
    let mut obj_offsets: Vec<usize> = Vec::new();
    for p in memchr::memmem::find_iter(data, b" obj") {
        obj_offsets.push(p);
        if obj_offsets.len() >= 2_000_000 {
            break;
        }
    }
    let first_root = memchr::memmem::find(data, b"/Root");

    let mut eofs = Vec::new();
    for p in memchr::memmem::find_iter(data, b"%%EOF") {
        if p < 8 {
            continue;
        }
        let marker_end = p + 5;
        let mut end = marker_end;
        if end < data.len() {
            if data.get(end) == Some(&b'\r') && data.get(end + 1) == Some(&b'\n') {
                end += 2;
            } else if data.get(end) == Some(&b'\r') || data.get(end) == Some(&b'\n') {
                end += 1;
            }
        }
        let (startxref, xref_target) = find_startxref(data, p);
        let objects = obj_offsets.partition_point(|&off| off < end) as u32;
        let has_root = first_root.is_some_and(|r| r < end);
        eofs.push(EofCandidate {
            marker_pos: p,
            end,
            startxref,
            xref_target,
            objects,
            has_root,
        });
        if eofs.len() >= 4096 {
            break;
        }
    }
    eofs
}

/// True when the buffer tail is a fill pattern the generic carve trimmer will
/// remove: a long zero run, or a long `0xFF` run after the trailing zeros.
/// The 16 KiB zero threshold is above the 8 KiB trailer used by the S-TAIL
/// fixtures, so random padding followed by that trailer still takes the
/// conservative path.
fn padding_after_cut_is_trim_only(prefix: &[u8]) -> bool {
    let mut end = prefix.len();
    let zero_run = prefix
        .get(..end)
        .unwrap_or(prefix)
        .iter()
        .rev()
        .take_while(|b| **b == 0)
        .count();
    if zero_run >= 16 * 1024 {
        return true;
    }
    end = end.saturating_sub(zero_run);
    let ff_run = prefix
        .get(..end)
        .unwrap_or(prefix)
        .iter()
        .rev()
        .take_while(|b| **b == 0xFF)
        .count();
    ff_run >= 512
}

/// Conservative end for a PDF with no footer (H-25). The last complete
/// top-level `endobj` is the structural floor; a cut `FlateDecode` stream with a
/// provable inflate prefix may extend it. The decode budget is the validator's
/// bounded budget, so no stream is inflated without a cap.
fn conservative_truncated_end(
    data: &[u8],
    search_end: usize,
    limits: ValidationLimits,
) -> (usize, Option<String>) {
    let prefix = data.get(..search_end).unwrap_or(data);
    // A long zero/FF fill after the cut is removed by the generic O(tail)
    // trimmer in `carve_one`. Letting it see the whole buffer keeps zero-padded
    // cuts byte-identical (S-REPAIR PDF and S-TAIL zero rows) while random or
    // text padding still gets the conservative structural end below.
    if padding_after_cut_is_trim_only(prefix) {
        return (search_end, None);
    }
    let mut end = 0usize;
    for pos in memchr::memmem::find_iter(prefix, b"endobj") {
        let mut candidate = pos.saturating_add(b"endobj".len());
        // The line terminator belongs to the object, not to the padding that
        // follows a cut. Take at most one line ending so a padded stream is
        // never extended on the strength of whitespace alone.
        if prefix.get(candidate) == Some(&b'\r') {
            candidate += 1;
        }
        if prefix.get(candidate) == Some(&b'\n') {
            candidate += 1;
        }
        if candidate <= search_end
            && memchr::memmem::find(prefix.get(..pos).unwrap_or(&[]), b" obj").is_some()
        {
            end = candidate;
        }
    }

    let mut bound_note = None;
    if let Some((stream_start, stream)) = last_open_flate_stream(prefix, search_end) {
        if let Some((_, consumed)) = super::zlib_prefix(&[stream], limits.max_validator_memory) {
            let stream_end = stream_start.saturating_add(consumed).min(search_end);
            if stream_end > end {
                end = stream_end;
                bound_note = Some("end set to last inflatable FlateDecode stream byte".to_string());
            }
        }
    }

    // No complete object: keep only the header and the last object header that
    // is actually present. A cut before the first `endobj` has no verified
    // body; claiming `search_end` would swallow arbitrary padding.
    if end < 8 {
        end = 8.min(prefix.len());
    }
    for pos in memchr::memmem::find_iter(prefix, b" obj") {
        let candidate = pos.saturating_add(b" obj".len());
        if candidate > end && candidate <= search_end {
            end = candidate;
        }
    }
    (end, bound_note)
}

/// Last `stream` keyword that has no `endstream` after it and whose object
/// dictionary declares `FlateDecode`. Returns the payload start and the bounded
/// slice ending at `search_end`.
fn last_open_flate_stream(prefix: &[u8], search_end: usize) -> Option<(usize, &[u8])> {
    let mut found = None;
    for pos in memchr::memmem::find_iter(prefix, b"stream") {
        if pos >= 3 && prefix.get(pos - 3..pos) == Some(b"end".as_slice()) {
            continue;
        }
        let after = pos.saturating_add(b"stream".len());
        if after > search_end {
            continue;
        }
        let dict_start = pos.saturating_sub(512);
        let dict = prefix.get(dict_start..pos).unwrap_or(&[]);
        if memchr::memmem::find(dict, b"/FlateDecode").is_none() {
            continue;
        }
        if memchr::memmem::find(prefix.get(after..search_end).unwrap_or(&[]), b"endstream")
            .is_some()
        {
            continue;
        }
        let mut start = after;
        if prefix.get(start) == Some(&b'\r') {
            start += 1;
        }
        if prefix.get(start) == Some(&b'\n') {
            start += 1;
        }
        found = Some((start, prefix.get(start..search_end).unwrap_or(&[])));
    }
    found
}

fn truncated(end: usize, at_limit: bool, prefix: &[u8], version: &str, why: &str) -> Need {
    if !at_limit {
        return Need::More;
    }
    let objects = count_objects(prefix);
    if objects == 0 {
        return Need::Reject("PDF candidate has no object headers".into());
    }
    let has_xref = memchr::memmem::find(prefix, b"/XRef").is_some();
    let mut score = 50i32 + 10; // header + at least one object
    if has_xref {
        score += 5;
    }
    Need::Analyzed(Analysis {
        end,
        complete: false,
        score: score.clamp(0, 100) as u8,
        width: None,
        height: None,
        notes: vec![format!(
            "truncated PDF {version} ({why}); {objects} object(s); repair rebuilds xref/trailer when possible"
        )],
        repair_suffix: EOF_SUFFIX,
        integrity_checks: Vec::new(),
        validated_prefix: 0,
    })
}

fn find_startxref(data: &[u8], eof_pos: usize) -> (Option<u64>, bool) {
    let window_start = eof_pos.saturating_sub(4096);
    let Some(window) = data.get(window_start..eof_pos) else {
        return (None, false);
    };
    let Some(rel) = memchr::memmem::rfind(window, b"startxref") else {
        return (None, false);
    };
    let mut pos = window_start + rel + b"startxref".len();
    while pos < eof_pos && data.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    let num_start = pos;
    while pos < eof_pos && data.get(pos).is_some_and(u8::is_ascii_digit) {
        pos += 1;
    }
    if pos == num_start {
        return (None, false);
    }
    let val = std::str::from_utf8(data.get(num_start..pos).unwrap_or(&[]))
        .ok()
        .and_then(|s| s.parse::<u64>().ok());
    match val {
        Some(target) => {
            let target = target as usize;
            let kind = target < data.len() && looks_like_xref_or_obj(data, target);
            (Some(target as u64), kind)
        }
        None => (None, false),
    }
}

fn looks_like_xref_or_obj(data: &[u8], mut pos: usize) -> bool {
    while pos < data.len() && data.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    if data.get(pos..).is_some_and(|s| s.starts_with(b"xref")) {
        return true;
    }
    // "N G obj" xref-stream object header.
    let mut digits = 0;
    while pos < data.len() && data.get(pos).is_some_and(u8::is_ascii_digit) {
        pos += 1;
        digits += 1;
    }
    if digits == 0 {
        return false;
    }
    while pos < data.len() && matches!(data.get(pos), Some(b' ' | b'\t')) {
        pos += 1;
    }
    let mut gen = 0;
    while pos < data.len() && data.get(pos).is_some_and(u8::is_ascii_digit) {
        pos += 1;
        gen += 1;
    }
    if gen == 0 {
        return false;
    }
    while pos < data.len() && data.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    data.get(pos..).is_some_and(|s| s.starts_with(b"obj"))
}

fn count_objects(data: &[u8]) -> u32 {
    memchr::memmem::find_iter(data, b" obj")
        .take(2_000_000)
        .count() as u32
}

#[cfg(test)]
// Test fixtures use fixed slices and expect valid layouts; the production
// parser above remains subject to the deny list.
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn base_pdf() -> Vec<u8> {
        let mut out = b"%PDF-1.4\n".to_vec();
        let o1 = out.len();
        out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        let o2 = out.len();
        out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [1 0 R] /Count 0 >>\nendobj\n");
        let xref = out.len();
        out.extend_from_slice(
            format!(
                "xref\n0 3\n0000000000 65535 f \n{o1:010} 00000 n \n{o2:010} 00000 n \n\
                 trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
            )
            .as_bytes(),
        );
        out
    }

    fn incremental(base: &[u8]) -> Vec<u8> {
        let mut out = base.to_vec();
        let o3 = out.len();
        out.extend_from_slice(b"3 0 obj\n<< /Type /Metadata >>\nendobj\n");
        let xref = out.len();
        let prev = find_prev_xref(base);
        out.extend_from_slice(
            format!(
                "xref\n0 1\n0000000000 65535 f \n3 1\n{o3:010} 00000 n \n\
                 trailer\n<< /Size 4 /Root 1 0 R /Prev {prev} >>\nstartxref\n{xref}\n%%EOF\n"
            )
            .as_bytes(),
        );
        out
    }

    fn find_prev_xref(base: &[u8]) -> u64 {
        let s = String::from_utf8_lossy(base);
        let pos = s.rfind("startxref\n").unwrap() + "startxref\n".len();
        s[pos..].lines().next().unwrap().trim().parse().unwrap()
    }

    #[test]
    fn parses_pdf_with_valid_xref() {
        let d = base_pdf();
        match analyze(&d, true, PdfEofMode::Smart) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert_eq!(a.end, d.len());
                assert_eq!(a.score, 100);
                assert!(a.notes.iter().any(|n| n.contains("target validated")));
            }
            _ => panic!("expected complete PDF"),
        }
    }

    #[test]
    fn smart_selects_last_incremental_revision() {
        let d = incremental(&base_pdf());
        match analyze(&d, true, PdfEofMode::Smart) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert_eq!(a.end, d.len());
                assert!(a.notes[0].contains("2 %%EOF"));
            }
            _ => panic!("expected complete PDF"),
        }
        match analyze(&d, true, PdfEofMode::First) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert!(a.end < d.len());
            }
            _ => panic!("expected first revision"),
        }
    }

    #[test]
    fn ignores_fake_eof_inside_stream() {
        let mut d = b"%PDF-1.4\n".to_vec();
        d.extend_from_slice(
            b"1 0 obj\n<< /Length 20 >>\nstream\nFAKE %%EOF HERE\nendstream\nendobj\n",
        );
        let o1 = 9usize;
        let xref = d.len();
        d.extend_from_slice(
            format!(
                "xref\n0 2\n0000000000 65535 f \n{o1:010} 00000 n \n\
                 trailer\n<< /Size 2 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
            )
            .as_bytes(),
        );
        match analyze(&d, true, PdfEofMode::Smart) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert_eq!(a.end, d.len());
            }
            _ => panic!("expected complete PDF"),
        }
    }

    #[test]
    fn truncated_pdf_requests_more_or_reports() {
        let d = base_pdf();
        let cut = &d[..d.len() - 6];
        assert!(matches!(analyze(cut, false, PdfEofMode::Smart), Need::More));
        match analyze(cut, true, PdfEofMode::Smart) {
            Need::Analyzed(a) => {
                assert!(!a.complete);
                // H-25: a footerless PDF ends after the last complete object
                // and its line terminator, not at the buffer end (the cut
                // trailer is not verified).
                assert_eq!(a.end, 114 + 1);
                assert!(a.end < cut.len());
                assert!(a.score >= 50);
                assert_eq!(a.repair_suffix, EOF_SUFFIX);
            }
            _ => panic!("expected truncated PDF"),
        }
    }

    #[test]
    fn stops_at_next_pdf_header_without_eof() {
        let first = b"%PDF-1.4\n1 0 obj\n<< /Length 0 >>\nstream\nendstream\nendobj\n".to_vec();
        let second = base_pdf();
        let mut d = first.clone();
        d.extend_from_slice(&second);
        match analyze(&d, true, PdfEofMode::Smart) {
            Need::Analyzed(a) => {
                assert!(!a.complete);
                // H-25: the object's own newline is kept, but the next
                // document header is not swallowed.
                assert_eq!(a.end, 56 + 1);
            }
            _ => panic!("expected truncated first PDF"),
        }
    }

    #[test]
    fn embedded_pdf_is_not_a_document_boundary() {
        let inner = base_pdf();
        let mut outer = b"%PDF-1.5\n1 0 obj\n<< /Length 0 >>\nstream\n".to_vec();
        outer.extend_from_slice(&inner);
        outer.extend_from_slice(b"\nendstream\nendobj\n");
        let xref = outer.len();
        outer.extend_from_slice(
            format!(
                "xref\n0 2\n0000000000 65535 f \n0000000009 00000 n \n\
                 trailer\n<< /Size 2 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
            )
            .as_bytes(),
        );
        match analyze(&outer, true, PdfEofMode::Smart) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert_eq!(a.end, outer.len(), "outer PDF must not stop at inner %PDF-");
            }
            _ => panic!("expected complete outer PDF"),
        }
    }

    #[test]
    fn adjacent_pdfs_are_split_at_the_second_header() {
        let first = base_pdf();
        let second = base_pdf();
        let mut d = first.clone();
        d.extend_from_slice(&second);
        match analyze(&d, true, PdfEofMode::Smart) {
            Need::Analyzed(a) => {
                assert_eq!(a.end, first.len());
            }
            _ => panic!("expected first PDF"),
        }
    }

    #[test]
    fn early_eof_requests_more_when_incremental_update_follows() {
        let mut base = base_pdf();
        // Pad the first revision immediately before its %%EOF so that the
        // marker sits 64 bytes before the end of the initial 1 MiB read
        // window, exactly the S2 condition from the review.
        let target = 1024 * 1024 - base.len() - 64;
        let eof = base.len() - b"%%EOF\n".len();
        base.splice(eof..eof, std::iter::repeat_n(b' ', target));
        let d = incremental(&base);
        let window = &d[..1024 * 1024];
        assert!(matches!(
            analyze(window, false, PdfEofMode::Smart),
            Need::More
        ));
        match analyze(&d, true, PdfEofMode::Smart) {
            Need::Analyzed(a) => {
                assert!(a.complete);
                assert_eq!(a.end, d.len());
            }
            _ => panic!("expected complete incremental PDF"),
        }
    }

    #[test]
    fn rejects_pdf_without_objects() {
        let d = b"%PDF-1.4\nxref\n0 0\ntrailer\n<< >>\nstartxref\n9\n%%EOF\n";
        assert!(matches!(
            analyze(d, true, PdfEofMode::Smart),
            Need::Reject(_)
        ));
    }
}
