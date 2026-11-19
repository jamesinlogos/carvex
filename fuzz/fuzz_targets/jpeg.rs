#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = carvex::signatures::jpeg::analyze(data, false);
    let _ = carvex::signatures::jpeg::analyze(data, true);
});
