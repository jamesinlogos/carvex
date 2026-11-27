//! H-27: read-only logical stream over one volume's free extents.
//!
//! The stream concatenates free extents in ascending physical order.  It never
//! reads an allocated byte: [`FreeStreamSource::read_at`] translates each
//! logical range back to a physical range that is wholly inside one extent.

use crate::io::ImageSource;
use crate::util::{merge_extents, Extent};
use std::io;

/// A logical byte stream made only from a volume's free extents.
pub struct FreeStreamSource<'a> {
    backing: &'a dyn ImageSource,
    extents: Vec<Extent>,
    len: u64,
}

impl<'a> FreeStreamSource<'a> {
    /// Build a stream from one volume's free extents. Empty extents are
    /// discarded and adjacent extents are merged before use.
    pub fn new(backing: &'a dyn ImageSource, extents: &[Extent]) -> Self {
        let extents = merge_extents(
            extents
                .iter()
                .copied()
                .filter(|extent| extent.len > 0 && extent.end() > extent.offset)
                .collect(),
        );
        let len = extents
            .iter()
            .fold(0u64, |sum, extent| sum.saturating_add(extent.len));
        Self {
            backing,
            extents,
            len,
        }
    }

    /// Physical free extents contributing to this stream.
    pub fn extents(&self) -> &[Extent] {
        &self.extents
    }

    /// Logical stream length.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the stream is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Locate the extent containing `logical_offset`, returning its index and
    /// the byte offset inside that extent.
    fn locate(&self, logical_offset: u64) -> Option<(usize, u64)> {
        let mut logical = 0u64;
        for (index, extent) in self.extents.iter().enumerate() {
            let extent_end = logical.saturating_add(extent.len);
            if logical_offset >= logical && logical_offset < extent_end {
                return Some((index, logical_offset - logical));
            }
            logical = extent_end;
        }
        None
    }

    /// Translate one logical byte offset into its physical image offset.
    pub fn map_offset(&self, logical_offset: u64) -> Option<u64> {
        let (index, within) = self.locate(logical_offset)?;
        let extent = self.extents.get(index)?;
        Some(extent.offset.saturating_add(within))
    }

    /// Translate a logical range into physical ranges. Each returned range is
    /// wholly inside one free extent, so provenance never spans allocated
    /// clusters. The range is clamped to the end of the logical stream.
    pub fn physical_ranges(&self, logical_offset: u64, len: u64) -> Vec<Extent> {
        let mut out = Vec::new();
        if len == 0 || logical_offset >= self.len {
            return out;
        }
        let mut logical = logical_offset;
        let mut remaining = len.min(self.len.saturating_sub(logical_offset));
        while remaining > 0 {
            let Some((index, within)) = self.locate(logical) else {
                break;
            };
            let Some(extent) = self.extents.get(index) else {
                break;
            };
            let available = extent.len.saturating_sub(within);
            if available == 0 {
                break;
            }
            let take = available.min(remaining);
            out.push(Extent {
                offset: extent.offset.saturating_add(within),
                len: take,
            });
            logical = logical.saturating_add(take);
            remaining -= take;
        }
        out
    }
}

impl ImageSource for FreeStreamSource<'_> {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || offset >= self.len {
            return Ok(0);
        }
        let mut logical = offset;
        let mut written = 0usize;
        while written < buf.len() {
            let Some((index, within)) = self.locate(logical) else {
                break;
            };
            let Some(extent) = self.extents.get(index) else {
                break;
            };
            let available = extent.len.saturating_sub(within);
            if available == 0 {
                break;
            }
            let want_u64 = available.min((buf.len() - written) as u64);
            let Ok(want_usize) = usize::try_from(want_u64) else {
                break;
            };
            let Some(dest) = buf.get_mut(written..written.saturating_add(want_usize)) else {
                break;
            };
            let physical = extent.offset.saturating_add(within);
            let n = self.backing.read_at(physical, dest)?;
            if n == 0 {
                break;
            }
            written = written.saturating_add(n);
            logical = logical.saturating_add(n as u64);
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mem(Vec<u8>);
    impl ImageSource for Mem {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
            let start = usize::try_from(offset).unwrap_or(usize::MAX);
            let Some(src) = self.0.get(start..) else {
                return Ok(0);
            };
            let n = src.len().min(buf.len());
            let (Some(dst), Some(part)) = (buf.get_mut(..n), src.get(..n)) else {
                return Ok(0);
            };
            dst.copy_from_slice(part);
            Ok(n)
        }
    }

    #[test]
    fn stream_skips_allocated_island() {
        let backing = Mem(b"aaaaaBBbbbccccc".to_vec());
        let stream = FreeStreamSource::new(
            &backing,
            &[Extent { offset: 0, len: 5 }, Extent { offset: 7, len: 8 }],
        );
        assert_eq!(stream.len(), 13);
        let mut buf = vec![0u8; 13];
        assert!(matches!(stream.read_at(0, &mut buf), Ok(13)));
        assert_eq!(buf, b"aaaaabbbccccc".to_vec());
        assert_eq!(stream.map_offset(5), Some(7));
        assert_eq!(
            stream.physical_ranges(4, 4),
            vec![Extent { offset: 4, len: 1 }, Extent { offset: 7, len: 3 },]
        );
    }
}
