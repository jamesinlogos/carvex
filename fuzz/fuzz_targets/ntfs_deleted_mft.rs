#![no_main]
use carvex::fs::{ntfs, MetadataLimits, DEFAULT_METADATA_LIMITS};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = MetadataLimits {
        max_depth: 4,
        max_entries: 64,
        max_bytes: 64 * 1024,
        max_name_bytes: 128,
        max_runlist_entries: 64,
        min_size: 32,
        max_size: 64 * 1024,
        max_data_bytes: 64 * 1024,
        max_work_bytes: 64 * 1024,
        max_mft_scan_bytes: 64 * 1024,
        ..DEFAULT_METADATA_LIMITS
    };
    let boot = data.get(..data.len().min(512)).unwrap_or(data);
    let _ = ntfs::deleted_entries_fuzz(boot, data, &limits);
});
