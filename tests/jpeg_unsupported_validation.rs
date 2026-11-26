//! H-09: an unsupported JPEG variant must never receive a decoded/verified
//! label by falling through the strict baseline scan.
//!
//! Red first: on `ab725a6` a progressive JPEG could reach `Validation::Decoded`
//! because `strict_baseline_scan` returned `None`; the value is now
//! `Validation::Unverified`.

use carvex::signatures::{
    validate_deep_with_limits, Analysis, FileType, Need, Validation, ValidationLimits,
};

fn progressive() -> (Vec<u8>, Analysis) {
    let data = include_bytes!("fixtures/jpeg_progressive_160x120.jpg").to_vec();
    let analysis = match carvex::signatures::jpeg::analyze(&data, true) {
        Need::Analyzed(analysis) => analysis,
        Need::More => panic!("progressive fixture requested more data"),
        Need::Reject(note) => panic!("progressive fixture rejected: {note}"),
    };
    (data, analysis)
}

#[test]
fn progressive_jpeg_is_unsupported_not_decoded() {
    let (data, analysis) = progressive();
    let (validation, notes) = validate_deep_with_limits(
        FileType::Jpeg,
        &data,
        &analysis,
        ValidationLimits::new(1024 * 1024),
    );
    assert_eq!(validation, Validation::Unverified, "notes: {notes:?}");
    assert!(
        notes
            .iter()
            .any(|n| n.contains("strict scan unsupported for this variant")),
        "notes: {notes:?}"
    );
}
