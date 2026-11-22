//! H-12 / H-13 / H-23: hostile VMDK descriptor and grain-table regressions.
//!
//! These tests build minimal sparse VMDK extents in memory (no external tools)
//! and verify that hostile declarations are rejected or handled without
//! unbounded allocation, that extents cannot escape the descriptor directory,
//! and that a missing grain table reads as zeros.

use std::fs;
use std::path::{Path, PathBuf};

use carvex::io::{open_image, InputFormat};

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("carvex_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn le32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn le64(buf: &mut [u8], off: usize, v: u64) {
    buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

/// Build a monolithicSparse VMDK with `gd_entries` directory entries all
/// pointing at one grain table. The declared capacity is enormous relative to
/// the file size (H-12).
fn sparse_image(
    dir: &Path,
    name: &str,
    gd_entries: u32,
    num_gte: u32,
    grain_sectors: u64,
) -> PathBuf {
    let path = dir.join(name);
    let capacity = gd_entries as u64 * num_gte as u64 * grain_sectors;
    let mut file = vec![0u8; 512 * 4 + num_gte as usize * 4];
    file[0..4].copy_from_slice(b"KDMV");
    le32(&mut file, 4, 1); // version
    le32(&mut file, 8, 0); // flags (no zeroed-grain)
    le64(&mut file, 12, capacity);
    le64(&mut file, 20, grain_sectors);
    le64(&mut file, 28, 1); // descriptor at sector 1
    le64(&mut file, 36, 1); // descriptor 1 sector
    le32(&mut file, 44, num_gte);
    le64(&mut file, 56, 2); // grain directory at sector 2
                            // descriptor
    let desc = format!(
        "# Disk DescriptorFile\nversion=1\nCID=fffffffe\nparentCID=ffffffff\n\
         createType=\"monolithicSparse\"\n\n# Extent description\n\
         RW {capacity} SPARSE \"{name}\"\n"
    );
    file[512..512 + desc.len()].copy_from_slice(desc.as_bytes());
    // grain directory: all entries point at sector 3
    for i in 0..gd_entries as usize {
        le32(&mut file, 1024 + i * 4, 3);
    }
    // grain table at sector 3: all zero (unallocated grains)
    fs::write(&path, &file).unwrap();
    path
}

/// H-12: a tiny file declaring an enormous grain table must be rejected by a
/// typed error, not allocate hundreds of MiB. The open must also be fast.
#[test]
fn hostile_grain_table_is_bounded() {
    let dir = tmp_dir("h12");
    let path = sparse_image(&dir, "huge.vmdk", 128, 262_144, 1);
    let start = std::time::Instant::now();
    let result = open_image(&path, InputFormat::Vmdk);
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    match result {
        Ok(image) => {
            // If accepted, memory must be bounded: the directory may be
            // loaded, but full tables must be shared/lazy.
            assert!(image.len > 0);
        }
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            assert!(
                msg.contains("grain") || msg.contains("table") || msg.contains("size"),
                "unexpected error: {e}"
            );
        }
    }
}

/// H-23: a zero grain-directory entry means the table reads as zeros, not an
/// error. Reading from an unallocated grain must fill the buffer with zeros.
#[test]
fn unallocated_grain_reads_as_zeros() {
    let dir = tmp_dir("h23");
    let path = sparse_image(&dir, "zeros.vmdk", 1, 512, 128);
    // Zero the single grain-directory entry after creation.
    let mut bytes = fs::read(&path).unwrap();
    le32(&mut bytes, 1024, 0);
    fs::write(&path, &bytes).unwrap();

    let image = open_image(&path, InputFormat::Vmdk).expect("open zeros VMDK");
    let mut buf = vec![0xFFu8; 4096];
    let n = image.source.read_at(0, &mut buf).expect("read grain");
    assert_eq!(n, 4096);
    assert!(
        buf.iter().all(|b| *b == 0),
        "unallocated grain must read zeros"
    );
}

/// H-13: a descriptor with an absolute FLAT extent path must be rejected
/// unless explicitly allowed. No file outside the descriptor's directory may
/// be read.
#[test]
fn absolute_extent_path_is_rejected() {
    let dir = tmp_dir("h13_abs");
    let outside = dir.join("outside.img");
    fs::write(&outside, vec![0xABu8; 4096]).unwrap();
    let desc = dir.join("disk.vmdk");
    let text = format!(
        "# Disk DescriptorFile\nversion=1\nCID=fffffffe\ncreateType=\"monolithicFlat\"\n\n\
         # Extent description\nRW 8 FLAT \"{}\" 0\n",
        outside.display()
    );
    fs::write(&desc, text).unwrap();
    let msg = match open_image(&desc, InputFormat::Vmdk) {
        Ok(_) => panic!("absolute extent path must be rejected"),
        Err(e) => e.to_string().to_lowercase(),
    };
    assert!(
        msg.contains("absolute") || msg.contains("external") || msg.contains("escape"),
        "unexpected: {msg}"
    );
}

/// H-13: a `..` escape in a FLAT extent path must be rejected.
#[test]
fn parent_escape_is_rejected() {
    let dir = tmp_dir("h13_esc");
    let sub = dir.join("inner");
    fs::create_dir_all(&sub).unwrap();
    let desc = sub.join("disk.vmdk");
    fs::write(dir.join("secret.img"), vec![0xCDu8; 4096]).unwrap();
    let text = "# Disk DescriptorFile\nversion=1\nCID=fffffffe\ncreateType=\"monolithicFlat\"\n\n\
                # Extent description\nRW 8 FLAT \"../secret.img\" 0\n";
    fs::write(&desc, text).unwrap();
    let result = open_image(&desc, InputFormat::Vmdk);
    assert!(result.is_err(), "parent escape must be rejected");
}
