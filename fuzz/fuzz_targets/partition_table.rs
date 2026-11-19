#![no_main]
use carvex::io::ImageSource;
use libfuzzer_sys::fuzz_target;

#[path = "common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let src = common::MemSource::new(data.to_vec());
    let _ = carvex::fs::read_partition_table(&src, src.len());
});
