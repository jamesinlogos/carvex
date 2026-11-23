//! `CarveX`: structural file carving for JPEG, PNG and PDF images.
//!
//! The library is deliberately small and dependency-light so that the parsers
//! can be audited and fuzzed independently from the command-line front end.
//! The binary entry point lives in `src/main.rs`; all real work happens here.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
// H-34 pedantic stage 1: low-risk readability and correctness lints. Numeric
// casts (stage 2) and API-ergonomics lints (stage 3) are staged separately so
// each change stays reviewable and no cast needs a blanket `allow`.
#![warn(
    clippy::cloned_instead_of_copied,
    clippy::doc_markdown,
    clippy::field_reassign_with_default,
    clippy::manual_let_else,
    clippy::map_unwrap_or,
    clippy::redundant_closure_for_method_calls,
    clippy::stable_sort_primitive,
    clippy::uninlined_format_args
)]

/// Command-line argument parsing and top-level command dispatch.
pub mod cli;
pub mod fs;
pub mod image;
pub mod io;
/// Report serialization, schema validation helpers, and output formatting.
pub mod report;
/// Scan planning, carving pipeline, post-filtering, and report assembly.
pub mod scan;
pub mod signatures;
/// Shared utility helpers (sizes, hashes, time, formatting).
pub mod util;
