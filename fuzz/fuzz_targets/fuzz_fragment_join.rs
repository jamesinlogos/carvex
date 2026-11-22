#![no_main]
//! H-14: B-06 fragment-join reads, analysis, validation and budget accounting.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    carvex::scan::fuzz_fragment_join(data);
});
