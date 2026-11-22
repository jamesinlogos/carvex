#![no_main]
//! H-14: split-raw segment extension parsing and directory discovery.

use carvex::image::fuzz_support as support;
use carvex::image::{InputFormat, OpenOptions, SplitRaw};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let files = support::split_names_fuzz(data);
    let Ok(dir) = support::write_temp_files(&files, "carvex-fuzz-split") else {
        return;
    };
    let path = support::primary_path(dir.path(), &files);

    if let Ok(image) = SplitRaw::open(&path) {
        support::exercise_source(&image, data);
    }
    for format in [InputFormat::Split, InputFormat::Auto] {
        if let Ok(opened) = carvex::io::open_image_with_options(&path, format, &OpenOptions::default())
        {
            support::exercise_source(opened.source.as_ref(), data);
        }
    }
});
