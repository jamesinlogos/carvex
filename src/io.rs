//! Read-only image sources.
//!
//! Every filesystem parser and the scanner work against [`ImageSource`] rather
//! than a raw [`std::fs::File`].  That keeps the source read-only by
//! construction and makes it possible to test the same code against an
//! in-memory image, a split raw image, or a block device later.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Seek, SeekFrom};
use std::path::Path;

pub use crate::image::{
    open_image, open_image_with_options, InputFormat, OnReadError, OpenOptions, OpenedImage,
    SourceIdentity, UnreadableRegion,
};

/// A random-access, read-only byte source.
pub trait ImageSource: Send + Sync {
    /// Total length of the source in bytes.
    fn len(&self) -> u64;

    /// Read up to `buf.len()` bytes at `offset`.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize>;

    /// Whether the source is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A regular file opened read-only.
pub struct RawFile {
    file: File,
    len: u64,
}

impl RawFile {
    /// Open `path` read-only and determine its length.
    ///
    /// `metadata().len()` returns zero for block devices on some platforms, so
    /// fall back to seeking to the end.  The fallback is only used when the
    /// metadata length is zero; normal files keep their existing behaviour.
    pub fn open(path: &Path) -> Result<Self> {
        let mut file =
            File::open(path).with_context(|| format!("cannot open image '{}'", path.display()))?;
        let mut len = file
            .metadata()
            .with_context(|| format!("cannot stat image '{}'", path.display()))?
            .len();
        if len == 0 {
            let current = file
                .stream_position()
                .with_context(|| format!("cannot query position in '{}'", path.display()))?;
            len = file
                .seek(SeekFrom::End(0))
                .with_context(|| format!("cannot determine size of '{}'", path.display()))?;
            file.seek(SeekFrom::Start(current))
                .with_context(|| format!("cannot rewind '{}'", path.display()))?;
        }
        if len == 0 {
            bail!("image '{}' is empty", path.display());
        }
        Ok(Self { file, len })
    }
}

impl ImageSource for RawFile {
    fn len(&self) -> u64 {
        self.len
    }

    #[cfg(unix)]
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        use std::os::unix::fs::FileExt;
        self.file.read_at(buf, offset)
    }

    #[cfg(windows)]
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        use std::os::windows::fs::FileExt;
        self.file.seek_read(buf, offset)
    }
}

/// Read exactly `buf.len()` bytes at `offset`.
pub fn read_exact_at(
    source: &dyn ImageSource,
    mut buf: &mut [u8],
    mut offset: u64,
) -> io::Result<()> {
    if offset > source.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "offset lies beyond the end of the image",
        ));
    }
    while !buf.is_empty() {
        let n = source.read_at(offset, buf)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected end of image while reading",
            ));
        }
        let tmp = buf;
        buf = &mut tmp[n..];
        offset = offset.saturating_add(n as u64);
    }
    Ok(())
}

/// Read exactly `len` bytes at `offset`.
pub fn read_vec_at(source: &dyn ImageSource, offset: u64, len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    read_exact_at(source, &mut buf, offset)
        .with_context(|| format!("cannot read {len} bytes at offset {offset}"))?;
    Ok(buf)
}

/// Compute the SHA-256 of the whole source without loading it into memory.
pub fn sha256_source(source: &dyn ImageSource) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut offset = 0u64;
    let mut buf = vec![0u8; 8 * 1024 * 1024];
    while offset < source.len() {
        let want = (source.len() - offset).min(buf.len() as u64) as usize;
        read_exact_at(source, &mut buf[..want], offset)
            .with_context(|| format!("cannot hash image at offset {offset}"))?;
        hasher.update(&buf[..want]);
        offset += want as u64;
    }
    Ok(crate::util::hex(&hasher.finalize()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Arc;

    /// In-memory source used by regression tests.
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
            let start = offset as usize;
            if start >= self.data.len() {
                return Ok(0);
            }
            let n = buf.len().min(self.data.len() - start);
            buf[..n].copy_from_slice(&self.data[start..start + n]);
            Ok(n)
        }
    }

    #[test]
    fn reads_exactly_and_hashes() {
        let src = MemSource::new(b"carvex".to_vec());
        let mut buf = [0u8; 3];
        read_exact_at(&src, &mut buf, 1).unwrap();
        assert_eq!(&buf, b"arv");
        assert_eq!(
            sha256_source(&src).unwrap(),
            "e6dc2ba3e17e0c6bc3f9cbf04d4f136502eefdf735a6ad7081d61e60a6eceaf5"
        );
        assert!(read_exact_at(&src, &mut buf, 99).is_err());
    }
}
