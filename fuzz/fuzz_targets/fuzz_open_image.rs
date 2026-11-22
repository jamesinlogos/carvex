#![no_main]
//! H-14: `open_image` format detection and explicit-format dispatch.

use carvex::image::fuzz_support as support;
use carvex::image::{InputFormat, OpenOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let files = support::open_image_fuzz(data);
    let Ok(dir) = support::write_temp_files(&files, "carvex-fuzz-open") else {
        return;
    };
    let path = support::primary_path(dir.path(), &files);

    for format in [
        InputFormat::Auto,
        InputFormat::Raw,
        InputFormat::Split,
        InputFormat::E01,
        InputFormat::Vhd,
        InputFormat::Vmdk,
    ] {
        if let Ok(opened) = carvex::io::open_image_with_options(&path, format, &OpenOptions::default())
        {
            support::exercise_source(opened.source.as_ref(), data);
        }
    }
});
