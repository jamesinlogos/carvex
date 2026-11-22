#![no_main]
//! H-14: fixed/dynamic VHD footers, checksums, BAT and block reads.

use carvex::image::fuzz_support as support;
use carvex::image::{InputFormat, OpenOptions, VhdImage};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let files = support::vhd_fuzz(data);
    let Ok(dir) = support::write_temp_files(&files, "carvex-fuzz-vhd") else {
        return;
    };
    let path = support::primary_path(dir.path(), &files);

    if let Ok(image) = VhdImage::open(&path) {
        support::exercise_source(&image, data);
    }
    if let Ok(opened) =
        carvex::io::open_image_with_options(&path, InputFormat::Vhd, &OpenOptions::default())
    {
        support::exercise_source(opened.source.as_ref(), data);
    }
});
