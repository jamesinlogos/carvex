//! Property tests for extent arithmetic and parser robustness.
//!
//! The parsers must never panic on arbitrary image bytes: they are the first
//! thing a forensic tool runs against untrusted data.

mod common;

use carvex::fs;
use carvex::io::ImageSource;
use carvex::signatures::{jpeg, pdf, png, PdfEofMode};
use carvex::util::{complement_extents, merge_extents, Extent};
use common::MemSource;
use proptest::prelude::*;

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
}
