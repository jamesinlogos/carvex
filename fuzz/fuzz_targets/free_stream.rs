#![no_main]
use carvex::io::{FreeStreamSource, ImageSource};
use carvex::util::Extent;
use libfuzzer_sys::fuzz_target;

#[path = "common.rs"]
mod common;

fn u16_at(data: &[u8], offset: usize) -> u16 {
    data.get(offset..offset.saturating_add(2))
        .and_then(|slice| <[u8; 2]>::try_from(slice).ok())
        .map(u16::from_le_bytes)
        .unwrap_or(0)
}

fuzz_target!(|data: &[u8]| {
    let src = common::MemSource::new(data.to_vec());
    let max = src.len();
    if max == 0 {
        return;
    }
    let mut extents = Vec::new();
    let count = usize::from(u16_at(data, 0)) % 64;
    for i in 0..count {
        let base = 2usize.saturating_add(i.saturating_mul(4));
        let start = u64::from(u16_at(data, base)) % max;
        let len = u64::from(u16_at(data, base + 2)).min(max.saturating_sub(start));
        if len > 0 {
            extents.push(Extent { offset: start, len });
        }
    }
    extents.sort_by_key(|extent| extent.offset);
    let stream = FreeStreamSource::new(&src, &extents);
    let stream_len = stream.len();
    if stream_len == 0 {
        return;
    }
    let logical = u64::from(u16_at(data, data.len().saturating_sub(2))) % stream_len;
    let want = usize::from(u16_at(data, data.len().saturating_sub(4))).min(4096);
    let mut buf = vec![0u8; want];
    let _ = stream.read_at(logical, &mut buf);
    let _ = stream.map_offset(logical);
    let _ = stream.physical_ranges(logical, want as u64);
});
