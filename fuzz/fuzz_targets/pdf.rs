#![no_main]
use carvex::signatures::PdfEofMode;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for mode in [PdfEofMode::First, PdfEofMode::Last, PdfEofMode::Smart] {
        let _ = carvex::signatures::pdf::analyze(data, false, mode);
        let _ = carvex::signatures::pdf::analyze(data, true, mode);
    }
});
