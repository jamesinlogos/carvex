//! R-02: PNG decode validation must bound decompressed work and memory.
//!
//! The validator is exercised directly so the tests do not depend on carving
//! finding the hostile header first.  The streaming test uses a valid image
//! whose declared raw size is below the default validator budget but far above
//! the fixed inflate buffer; peak RSS must stay near the buffer size rather
//! than the declared image size.

mod common;

use carvex::signatures::{
    validate_deep, validate_deep_with_limits, Analysis, FileType, Validation, ValidationLimits,
    DEFAULT_MAX_PNG_DECODE_BYTES,
};
use common::chunk;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::fs;
use std::io::Write;

const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

fn analysis(width: u32, height: u32) -> Analysis {
    Analysis {
        end: 0,
        complete: true,
        score: 100,
        width: Some(width),
        height: Some(height),
        notes: Vec::new(),
        repair_suffix: &[],
    }
}

fn ihdr(width: u32, height: u32, depth: u8, color_type: u8, interlace: u8) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[depth, color_type, 0, 0, interlace]);
    chunk(b"IHDR", &data)
}

fn png_with_idat(width: u32, height: u32, idat: &[u8]) -> Vec<u8> {
    let mut out = PNG_MAGIC.to_vec();
    out.extend_from_slice(&ihdr(width, height, 8, 2, 0));
    out.extend_from_slice(&chunk(b"IDAT", idat));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

fn zlib_zeros(len: usize) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    let zeros = [0u8; 64 * 1024];
    let mut left = len;
    while left > 0 {
        let n = left.min(zeros.len());
        encoder.write_all(&zeros[..n]).unwrap();
        left -= n;
    }
    encoder.finish().unwrap()
}

fn vm_hwm_kib() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            return rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok());
        }
    }
    None
}

#[test]
fn extreme_dimensions_are_rejected_before_inflation() {
    // 100000 x 100000 RGB would inflate to ~30 GB.  The IHDR alone must be
    // rejected without creating a decoded buffer or even an IDAT stream.
    let data = png_with_idat(100_000, 100_000, &[]);
    let (validation, notes) = validate_deep(FileType::Png, &data, &analysis(100_000, 100_000));
    assert_eq!(validation, Validation::Partial, "notes: {notes:?}");
    assert!(
        notes.iter().any(|n| n.contains("budget")),
        "expected a validator-budget note, got: {notes:?}"
    );
}

#[test]
fn declared_size_above_an_explicit_budget_is_rejected() {
    let data = png_with_idat(4096, 4096, &[]);
    let limits = ValidationLimits::new(1024 * 1024);
    let (validation, notes) =
        validate_deep_with_limits(FileType::Png, &data, &analysis(4096, 4096), limits);
    assert_eq!(validation, Validation::Partial, "notes: {notes:?}");
    assert!(
        notes.iter().any(|n| n.contains("budget")),
        "expected a validator-budget note, got: {notes:?}"
    );
}

#[test]
fn high_expansion_png_is_streamed_with_bounded_memory() {
    // 4096x4096 RGB = 50,335,744 bytes after filtering, but the compressed
    // stream is tiny.  The validator must not hold the decoded image.
    let width = 4096u32;
    let height = 4096u32;
    let raw_len = height as usize * (1 + width as usize * 3);
    assert!(raw_len < DEFAULT_MAX_PNG_DECODE_BYTES as usize);
    let idat = zlib_zeros(raw_len);
    let data = png_with_idat(width, height, &idat);

    let before = vm_hwm_kib();
    let (validation, notes) = validate_deep(FileType::Png, &data, &analysis(width, height));
    let after = vm_hwm_kib();
    assert_eq!(validation, Validation::Decoded, "notes: {notes:?}");
    assert!(notes.iter().any(|n| n.contains("inflated")));

    if let (Some(before), Some(after)) = (before, after) {
        let delta_kib = after.saturating_sub(before);
        assert!(
            delta_kib <= 32 * 1024,
            "peak RSS grew by {delta_kib} KiB while inflating {raw_len} bytes"
        );
    }
}

#[test]
fn invalid_filter_byte_is_rejected_during_streaming() {
    let width = 2u32;
    let height = 1u32;
    // One row: filter byte 5 is invalid, then six RGB bytes.
    let raw = [5u8, 1, 2, 3, 4, 5, 6];
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&raw).unwrap();
    let idat = encoder.finish().unwrap();
    let data = png_with_idat(width, height, &idat);
    let (validation, notes) = validate_deep(FileType::Png, &data, &analysis(width, height));
    assert_eq!(validation, Validation::Partial, "notes: {notes:?}");
    assert!(
        notes.iter().any(|n| n.contains("filter")),
        "expected a filter-byte note, got: {notes:?}"
    );
}
