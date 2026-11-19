#![allow(dead_code)]
//! Shared helpers for the cargo-fuzz targets.

use carvex::io::ImageSource;
use std::io;
use std::sync::Arc;

#[derive(Clone)]
pub struct MemSource {
    data: Arc<Vec<u8>>,
}

impl MemSource {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data: Arc::new(data),
        }
    }
}

impl ImageSource for MemSource {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if start >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        Ok(n)
    }
}

/// Build an NTFS-looking image whose bytes after the boot sector are the
/// fuzzer-controlled `$MFT` area.  The boot fields are plausible so the parser
/// reaches the MFT/attribute code instead of rejecting at the signature.
pub fn ntfs_image(mft: &[u8]) -> Vec<u8> {
    let mut image = vec![0u8; 4096 + mft.len()];
    image[3..7].copy_from_slice(b"NTFS");
    image[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes()); // bytes/sector
    image[0x0D] = 1; // sectors/cluster
    image[0x28..0x30].copy_from_slice(&8192u64.to_le_bytes()); // total sectors
    image[0x30..0x38].copy_from_slice(&1u64.to_le_bytes()); // $MFT LCN
    image[0x38..0x40].copy_from_slice(&1u64.to_le_bytes()); // $MFTMirr LCN
    image[0x40] = 0xF6; // 1024-byte MFT records (-10)
    image[510..512].copy_from_slice(&[0x55, 0xAA]);
    image[4096..].copy_from_slice(mft);
    image
}
