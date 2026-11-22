#![no_main]
//! H-14: VMDK descriptors, flat extents, sparse grain directories and tables.

use carvex::image::fuzz_support as support;
use carvex::image::{InputFormat, OpenOptions, VmdkImage};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let files = support::vmdk_fuzz(data);
    let Ok(dir) = support::write_temp_files(&files, "carvex-fuzz-vmdk") else {
        return;
    };
    let path = support::primary_path(dir.path(), &files);

    if let Ok(image) = VmdkImage::open(&path) {
        support::exercise_source(&image, data);
    }
    if let Ok(opened) =
        carvex::io::open_image_with_options(&path, InputFormat::Vmdk, &OpenOptions::default())
    {
        support::exercise_source(opened.source.as_ref(), data);
    }
});
