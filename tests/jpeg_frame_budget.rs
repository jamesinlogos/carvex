//! H-06: a JPEG SOF header must not drive an unbounded decoder allocation.
//!
//! Red first: before the fix the validator calls the decoder even when the
//! claimed frame exceeds the shared validator budget.

mod common;

use carvex::signatures::{
    validate_deep_with_limits, Analysis, FileType, Need, Validation, ValidationLimits,
};
use common::JPEG_1X1;

fn oversized_sof(width: u16, height: u16) -> (Vec<u8>, Analysis) {
    let mut data = JPEG_1X1.to_vec();
    let sof = data
        .windows(2)
        .position(|w| w == [0xFF, 0xC0])
        .expect("fixture has no SOF0");
    data[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
    data[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
    let analysis = match carvex::signatures::jpeg::analyze(&data, true) {
        Need::Analyzed(analysis) => analysis,
        _ => panic!("unexpected analysis result"),
    };
    (data, analysis)
}

#[test]
fn claimed_frame_above_the_validator_budget_is_not_decoded() {
    let (data, analysis) = oversized_sof(2000, 2000);
    let (validation, notes) = validate_deep_with_limits(
        FileType::Jpeg,
        &data,
        &analysis,
        ValidationLimits::new(1024),
    );
    assert_eq!(validation, Validation::Unverified, "notes: {notes:?}");
    assert!(
        notes
            .iter()
            .any(|n| n.contains("claimed frame size exceeds validator budget")),
        "notes: {notes:?}"
    );
}

#[test]
fn implausible_entropy_claim_is_not_decoded_with_a_large_budget() {
    let (data, analysis) = oversized_sof(20000, 20000);
    let (validation, notes) = validate_deep_with_limits(
        FileType::Jpeg,
        &data,
        &analysis,
        ValidationLimits::new(2 * 1024 * 1024 * 1024),
    );
    assert_eq!(validation, Validation::Unverified, "notes: {notes:?}");
    assert!(
        notes
            .iter()
            .any(|n| n.contains("claimed frame size") && n.contains("implausible")),
        "notes: {notes:?}"
    );
}
