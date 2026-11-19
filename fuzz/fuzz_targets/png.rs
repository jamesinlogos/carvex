#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = carvex::signatures::png::analyze(data, false);
    let _ = carvex::signatures::png::analyze(data, true);
});
