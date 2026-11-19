//! CarveX: structural file carving for JPEG, PNG and PDF images.
//!
//! The library is deliberately small and dependency-light so that the parsers
//! can be audited and fuzzed independently from the command-line front end.
//! The binary entry point lives in `src/main.rs`; all real work happens here.

#![forbid(unsafe_code)]

pub mod cli;
pub mod fs;
pub mod io;
pub mod report;
pub mod scan;
pub mod signatures;
pub mod util;
