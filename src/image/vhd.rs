//! Read-only Virtual Hard Disk (VHD) image source.
//!
//! Fixed and dynamic VHD images are supported.  Differencing images need a
//! parent chain and are rejected with an explicit error rather than silently
//! returning zeros where a parent would have supplied data.

// H-11: hostile container fields must be handled with checked access.
#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]

use super::{be_u32, be_u64, ones_complement};
use crate::io::{read_vec_at, ImageSource, RawFile};
use anyhow::{bail, Context, Result};
use std::io;

const SECTOR: u64 = 512;
const FIXED: u32 = 2;
const DYNAMIC: u32 = 3;
const DIFFERENCING: u32 = 4;

enum Layout {
    Fixed,
    Dynamic {
        bat: Vec<u32>,
        block_size: u64,
        bitmap_bytes: u64,
    },
}

/// A read-only VHD image.
pub struct VhdImage {
    file: RawFile,
    path: std::path::PathBuf,
    len: u64,
    layout: Layout,
}

impl VhdImage {
    /// Open a fixed or dynamic VHD and expose its guest-visible byte stream.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let file = RawFile::open(path)?;
        let file_len = file.len();
        if file_len < 2 * SECTOR {
            bail!("VHD image '{}' is too small", path.display());
        }
        let footer = read_vec_at(&file, file_len - SECTOR, SECTOR as usize)?;
        if !footer.starts_with(b"conectix") {
            bail!("'{}' is not a VHD image", path.display());
        }
        verify_checksum(&footer, 64, "VHD footer")?;

        let data_offset = be_u64(&footer, 16, "VHD data offset")?;
        let current_size = be_u64(&footer, 48, "VHD current size")?;
        let disk_type = be_u32(&footer, 60, "VHD disk type")?;
        if current_size == 0 {
            bail!("VHD image '{}' declares an empty disk", path.display());
        }
        if disk_type == DIFFERENCING {
            bail!("differencing VHD images are not supported (parent chain required)");
        }
        if disk_type == FIXED {
            // A fixed VHD contains every guest byte before its footer.  A
            // dynamic VHD is sparse, so its logical size is usually larger
            // than the physical file and must not be checked here.
            if current_size > file_len - SECTOR {
                bail!(
                    "VHD image '{}' declares more data than the file contains",
                    path.display()
                );
            }
            return Ok(Self {
                file,
                path: path.to_path_buf(),
                len: current_size,
                layout: Layout::Fixed,
            });
        }
        if disk_type != DYNAMIC {
            bail!("unsupported VHD disk type {disk_type}");
        }

        let header_start = data_offset;
        if header_start
            .checked_add(1024)
            .context("VHD header offset overflow")?
            > file_len - SECTOR
        {
            bail!("VHD dynamic header lies past the end of the file");
        }
        let header = read_vec_at(&file, header_start, 1024)?;
        if !header.starts_with(b"cxsparse") {
            bail!("VHD dynamic header cookie is missing");
        }
        verify_checksum(&header, 36, "VHD dynamic header")?;

        let table_offset = be_u64(&header, 16, "VHD BAT offset")?;
        let entries = be_u32(&header, 28, "VHD BAT entry count")? as usize;
        let block_size = u64::from(be_u32(&header, 32, "VHD block size")?);
        if block_size == 0 || !block_size.is_power_of_two() || block_size % SECTOR != 0 {
            bail!("VHD block size {block_size} is invalid");
        }
        let table_bytes = entries.checked_mul(4).context("VHD BAT size overflow")? as u64;
        if table_offset
            .checked_add(table_bytes)
            .context("VHD BAT offset overflow")?
            > file_len
        {
            bail!("VHD block allocation table lies past the end of the file");
        }
        let raw_bat = read_vec_at(&file, table_offset, table_bytes as usize)?;
        let mut bat = Vec::with_capacity(entries);
        for i in 0..entries {
            bat.push(be_u32(&raw_bat, i * 4, "VHD BAT entry")?);
        }
        let needed = current_size.div_ceil(block_size) as usize;
        if bat.len() < needed {
            bail!(
                "VHD BAT has {} entries but {needed} are required",
                bat.len()
            );
        }
        bat.truncate(needed);

        let bitmap_bytes = bitmap_size(block_size);
        Ok(Self {
            file,
            path: path.to_path_buf(),
            len: current_size,
            layout: Layout::Dynamic {
                bat,
                block_size,
                bitmap_bytes,
            },
        })
    }

    /// Backing files tracked for source-change detection.
    pub(crate) fn source_paths(&self) -> Vec<std::path::PathBuf> {
        vec![self.path.clone()]
    }

    fn read_dynamic(
        &self,
        bat: &[u32],
        block_size: u64,
        bitmap_bytes: u64,
        offset: u64,
        buf: &mut [u8],
    ) -> io::Result<usize> {
        let mut done = 0usize;
        while done < buf.len() {
            let pos = offset + done as u64;
            if pos >= self.len {
                break;
            }
            let block = (pos / block_size) as usize;
            let within = pos % block_size;
            let entry = *bat.get(block).ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "VHD block index out of range")
            })?;
            if entry == u32::MAX {
                let zeros = (block_size - within)
                    .min((buf.len() - done) as u64)
                    .min(self.len - pos) as usize;
                let end = done
                    .checked_add(zeros)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "VHD zero range"))?;
                buf.get_mut(done..end)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "VHD read destination")
                    })?
                    .fill(0);
                done += zeros;
                continue;
            }
            let file_offset = (u64::from(entry) * SECTOR)
                .checked_add(bitmap_bytes)
                .and_then(|n| n.checked_add(within))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "VHD block offset overflow")
                })?;
            let want = (block_size - within).min((buf.len() - done) as u64) as usize;
            let end = done
                .checked_add(want)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "VHD read range"))?;
            let target = buf.get_mut(done..end).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "VHD read destination")
            })?;
            let n = self.file.read_at(file_offset, target)?;
            if n == 0 {
                break;
            }
            done += n;
        }
        Ok(done)
    }
}

impl ImageSource for VhdImage {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.len || buf.is_empty() {
            return Ok(0);
        }
        match &self.layout {
            Layout::Fixed => self.file.read_at(offset, buf),
            Layout::Dynamic {
                bat,
                block_size,
                bitmap_bytes,
            } => self.read_dynamic(bat, *block_size, *bitmap_bytes, offset, buf),
        }
    }
}

/// One bit per sector, rounded up to a whole 512-byte sector.
fn bitmap_size(block_size: u64) -> u64 {
    block_size.div_ceil(SECTOR * 8).div_ceil(SECTOR).max(1) * SECTOR
}

fn verify_checksum(buf: &[u8], checksum_offset: usize, what: &str) -> Result<()> {
    let expected = be_u32(buf, checksum_offset, what)?;
    let mut copy = buf.to_vec();
    let end = checksum_offset
        .checked_add(4)
        .ok_or_else(|| anyhow::anyhow!("{what} checksum offset overflow"))?;
    copy.get_mut(checksum_offset..end)
        .ok_or_else(|| anyhow::anyhow!("{what} checksum field out of range"))?
        .fill(0);
    let actual = ones_complement(&copy);
    if expected != actual {
        bail!("{what} checksum mismatch (expected {expected:08x}, found {actual:08x})");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_size_is_sector_aligned() {
        assert_eq!(bitmap_size(2 * 1024 * 1024), 512);
        assert_eq!(bitmap_size(4 * 1024 * 1024), 1024);
        assert_eq!(bitmap_size(64 * 1024), 512);
    }
}
