//! Property tests for extent arithmetic and parser robustness.
//!
//! The parsers must never panic on arbitrary image bytes: they are the first
//! thing a forensic tool runs against untrusted data.

mod common;

use carvex::fs;
use carvex::io::ImageSource;
use carvex::scan::{run_scan, ScanContext, ScanInput, ScanMode};
use carvex::signatures::{
    jpeg, pdf, png, FileType, PdfEofMode, Status, Validation, ValidationLimits,
};
use carvex::util::{complement_extents, merge_extents, Extent};
use common::{pdf_min, png_1x1, scan_options, MemSource, JPEG_1X1};
use proptest::prelude::*;
use std::sync::{Arc, Mutex};

/// Deterministic padding patterns used by the H-25 property test.
fn padding(kind: u8, len: usize) -> Vec<u8> {
    match kind % 4 {
        0 => vec![0u8; len],
        1 => vec![0xFF; len],
        2 => {
            let mut state = 0x9E37_79B9_7F4A_7C15u64;
            (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state & 0xFF) as u8
                })
                .collect()
        }
        _ => b"ABCDEFGH"
            .repeat(len.div_ceil(8))
            .get(..len)
            .unwrap_or(&[])
            .to_vec(),
    }
}

fn extent() -> impl Strategy<Value = Extent> {
    (0u64..1_000_000, 0u64..100_000).prop_map(|(offset, len)| Extent { offset, len })
}

fn covers(extents: &[Extent], offset: u64) -> bool {
    extents.iter().any(|e| e.contains(offset))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, .. ProptestConfig::default() })]

    #[test]
    fn merged_extents_are_sorted_disjoint_and_cover_the_input(input in proptest::collection::vec(extent(), 0..40)) {
        let merged = merge_extents(input.clone());
        for pair in merged.windows(2) {
            prop_assert!(pair[0].end() < pair[1].offset, "overlap or adjacency: {:?}", pair);
        }
        for e in &merged {
            prop_assert!(e.len > 0);
        }
        // Every byte of every input extent is covered by a merged extent.
        for e in &input {
            if e.len == 0 {
                continue;
            }
            for probe in [e.offset, e.offset + e.len / 2, e.end() - 1] {
                prop_assert!(covers(&merged, probe), "offset {probe} from {:?} not covered", e);
            }
        }
        // Nothing outside the input span is invented.
        if let (Some(first), Some(last)) = (merged.first(), merged.last()) {
            let input_start = input.iter().map(|e| e.offset).min().unwrap();
            let input_end = input.iter().map(Extent::end).max().unwrap();
            prop_assert!(first.offset >= input_start);
            prop_assert!(last.end() <= input_end);
        }
    }

    #[test]
    fn complement_is_the_exact_gap_between_free_extents(
        start in 0u64..100_000,
        len in 1u64..1_000_000,
        mut free in proptest::collection::vec(extent(), 0..20),
    ) {
        // Keep the input inside the window and sorted, as production callers do.
        for e in &mut free {
            e.offset = start + e.offset % len;
            e.len %= len + 1;
        }
        free.sort_by_key(|e| e.offset);
        let free = merge_extents(free);
        let used = complement_extents(start, len, &free);
        let end = start + len;
        for pair in used.windows(2) {
            prop_assert!(pair[0].end() <= pair[1].offset);
        }
        for e in &used {
            prop_assert!(e.offset >= start);
            prop_assert!(e.end() <= end);
        }
        // Every sampled byte is in exactly one of free/used.
        let mut probe = start;
        while probe < end {
            let in_free = free.iter().any(|e| e.contains(probe));
            let in_used = used.iter().any(|e| e.contains(probe));
            prop_assert_ne!(in_free, in_used, "byte {} in free={} used={}", probe, in_free, in_used);
            probe += (len / 97).max(1);
        }
    }

    #[test]
    fn jpeg_png_pdf_parsers_never_panic(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = jpeg::analyze(&data, false);
        let _ = jpeg::analyze(&data, true);
        let _ = png::analyze(&data, false);
        let _ = png::analyze(&data, true);
        for mode in [PdfEofMode::First, PdfEofMode::Last, PdfEofMode::Smart] {
            let _ = pdf::analyze(&data, false, mode);
            let _ = pdf::analyze(&data, true, mode);
        }
    }

    #[test]
    fn filesystem_probe_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let src = MemSource::new(data);
        let _ = fs::probe(&src, 0, src.len(), None, None);
        let _ = fs::detect_guarded(&src, src.len(), None, None);
    }

    #[test]
    fn checked_reads_never_panic(offset in any::<u64>(), len in 0usize..8192) {
        let src = MemSource::new(vec![0xAB; 4096]);
        if offset <= src.len() {
            let mut buf = vec![0u8; len];
            let _ = carvex::io::read_exact_at(&src, &mut buf, offset);
        }
    }

    /// H-25: a cut file followed by arbitrary padding must never grow more
    /// than 4 KiB past the cut, and any carved prefix must match the original.
    #[test]
    fn truncated_carve_end_is_bounded_for_random_cut_and_padding(
        kind_id in 0u8..3,
        fraction in 15u32..86,
        pad_kind in 0u8..4,
        pad_len in 0usize..4097,
    ) {
        let (kind, full): (FileType, Vec<u8>) = match kind_id {
            0 => (FileType::Jpeg, JPEG_1X1.to_vec()),
            1 => (FileType::Png, png_1x1()),
            _ => (FileType::Pdf, pdf_min()),
        };
        let cut_len = (full.len() * fraction as usize / 100).max(1);
        let mut image = vec![0u8; 4096];
        image.extend_from_slice(full.get(..cut_len).unwrap_or(&full));
        image.extend_from_slice(&padding(pad_kind, pad_len));
        image.extend_from_slice(&[0u8; 8192]);

        let src = MemSource::new(image);
        let dir = tempfile::tempdir().unwrap();
        let ctx = ScanContext::default();
        let regions = Arc::new(Mutex::new(Vec::new()));
        let mut opts = scan_options(vec![kind], ScanMode::Raw);
        opts.min_size = 1;
        opts.min_score = 0;
        let input = ScanInput {
            source: &src,
            image: "tail.raw",
            image_identity: None,
            image_len: src.len(),
            volumes: &[],
            extra_extents: &[],
            out_dir: dir.path(),
            unreadable_regions: &regions,
            skip_unreadable: false,
            interrupt: None,
        };
        let report = run_scan(&input, &opts, &ctx).unwrap();
        prop_assert!(report.files.len() <= 1, "more than one file: {:?}", report.files);
        for file in &report.files {
            let size = usize::try_from(file.size).unwrap_or(usize::MAX);
            prop_assert!(size <= cut_len.saturating_add(4096), "kind={:?} size={} cut={}", kind, size, cut_len);
            prop_assert!(size >= cut_len.saturating_sub(4096), "kind={:?} size={} cut={}", kind, size, cut_len);
            prop_assert_eq!(file.status, Status::Truncated);
            prop_assert_eq!(file.validation, Validation::Partial);
            let carved = std::fs::read(dir.path().join(&file.path)).unwrap();
            let common = size.min(cut_len).min(carved.len());
            prop_assert_eq!(
                carved.get(..common),
                full.get(..common),
                "kind={:?} carved bytes differ before the cut",
                kind
            );
        }
    }

    /// The structural prefix scan must honour a caller-supplied decode budget
    /// and still never return an end past the supplied buffer.
    #[test]
    fn png_prefix_analysis_respects_the_decode_budget(
        data in proptest::collection::vec(any::<u8>(), 0..1024),
    ) {
        let limits = ValidationLimits::new(64);
        let need = png::analyze_with_limits(&data, true, limits);
        if let carvex::signatures::Need::Analyzed(analysis) = need {
            prop_assert!(analysis.end <= data.len());
            prop_assert!(analysis.validated_prefix <= analysis.end as u64);
        }
    }
}
