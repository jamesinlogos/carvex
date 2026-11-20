//! Concatenated raw image segments (`.001`, `.002`, ...).

use crate::io::{ImageSource, RawFile};
use anyhow::{bail, Context, Result};
use std::io;
use std::path::{Path, PathBuf};

/// Several raw files presented as one contiguous image.
pub struct SplitRaw {
    files: Vec<RawFile>,
    paths: Vec<PathBuf>,
    starts: Vec<u64>,
    len: u64,
}

impl SplitRaw {
    /// Discover the segment set containing `path` and open it read-only.
    ///
    /// The segment number is the numeric file extension.  Discovery walks
    /// both backwards and forwards so the caller may name any segment.
    pub fn open(path: &Path) -> Result<Self> {
        let paths = discover(path)?;
        let mut files = Vec::with_capacity(paths.len());
        let mut starts = Vec::with_capacity(paths.len());
        let mut len = 0u64;
        for p in &paths {
            let file = RawFile::open(p)?;
            starts.push(len);
            len = len
                .checked_add(file.len())
                .context("split image size overflow")?;
            files.push(file);
        }
        if len == 0 {
            bail!("split image '{}' is empty", path.display());
        }
        Ok(Self {
            files,
            paths,
            starts,
            len,
        })
    }

    /// Backing files tracked for source-change detection.
    pub(crate) fn source_paths(&self) -> Vec<PathBuf> {
        self.paths.clone()
    }
}

impl ImageSource for SplitRaw {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let mut done = 0usize;
        while done < buf.len() {
            let pos = offset + done as u64;
            if pos >= self.len {
                break;
            }
            let idx = match self.starts.binary_search(&pos) {
                Ok(i) => i,
                Err(i) => i - 1,
            };
            let within = pos - self.starts[idx];
            let n = self.files[idx].read_at(within, &mut buf[done..])?;
            if n == 0 {
                break;
            }
            done += n;
        }
        Ok(done)
    }
}

/// Whether `path` uses the conventional numeric split-file extension.
pub(crate) fn looks_like_split(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => !ext.is_empty() && ext.len() <= 6 && ext.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

fn discover(path: &Path) -> Result<Vec<PathBuf>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .context("split image needs a numeric segment extension")?;
    if !looks_like_split(path) {
        bail!(
            "split image '{}' needs a numeric extension such as .001",
            path.display()
        );
    }
    let width = ext.len();
    let start: u64 = ext.parse().context("invalid split segment number")?;

    let mut paths = Vec::new();
    let mut n = start;
    loop {
        let candidate = with_number(path, n, width);
        if !candidate.is_file() {
            break;
        }
        paths.push(candidate);
        if n == 0 {
            break;
        }
        n -= 1;
    }
    paths.reverse();

    let mut n = start + 1;
    loop {
        let candidate = with_number(path, n, width);
        if !candidate.is_file() {
            break;
        }
        paths.push(candidate);
        n += 1;
    }

    if paths.is_empty() {
        bail!("split image segment '{}' does not exist", path.display());
    }
    Ok(paths)
}

fn with_number(path: &Path, n: u64, width: usize) -> PathBuf {
    path.with_extension(format!("{n:0width$}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_extension_is_detected() {
        assert!(looks_like_split(Path::new("disk.001")));
        assert!(looks_like_split(Path::new("disk.00012")));
        assert!(!looks_like_split(Path::new("disk.img")));
        assert!(!looks_like_split(Path::new("disk.E01")));
    }
}
