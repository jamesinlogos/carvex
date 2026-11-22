#![no_main]
//! H-14: E01 header, section, chunk table and zlib/Adler-32 decode paths.

use carvex::image::fuzz_support as support;
use carvex::image::{E01Image, InputFormat, OnReadError, OpenOptions};
use libfuzzer_sys::fuzz_target;
use std::sync::{Arc, Mutex};

fuzz_target!(|data: &[u8]| {
    let files = support::e01_fuzz(data);
    let Ok(dir) = support::write_temp_files(&files, "carvex-fuzz-e01") else {
        return;
    };
    let path = support::primary_path(dir.path(), &files);

    if let Ok(image) = E01Image::open_with(
        &path,
        OnReadError::ZeroFill,
        Arc::new(Mutex::new(Vec::new())),
    ) {
        support::exercise_source(&image, data);
    }
    if let Ok(opened) =
        carvex::io::open_image_with_options(&path, InputFormat::E01, &OpenOptions::default())
    {
        support::exercise_source(opened.source.as_ref(), data);
    }
});
