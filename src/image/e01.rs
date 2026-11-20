//! Expert Witness Compression Format (EWF-E01) reader.
//!
//! This implements the classic EWF-E01 layout used by FTK Imager and EnCase:
//! 13-byte segment headers, 76-byte section descriptors, a volume section,
//! `sectors` data and one or more `table` sections per segment.  Both zlib
//! compressed and Adler-32 protected uncompressed chunks are supported.  EWF2
//! (`.Ex01`), encrypted images and Logical Evidence Files (`.L01`) are not.

use super::{adler32, le_u16, le_u32, le_u64};
use crate::io::{read_vec_at, ImageSource, RawFile};
use anyhow::{bail, Context, Result};
use flate2::read::ZlibDecoder;
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const EWF_MAGIC: &[u8] = b"EVF\x09\x0d\x0a\xff\x00";
const LVF_MAGIC: &[u8] = b"LVF\x09\x0d\x0a\xff\x00";
const CACHE_CHUNKS: usize = 8;

#[derive(Debug, Clone)]
struct ChunkRef {
    segment: usize,
    offset: u64,
    compressed: bool,
}

#[derive(Default)]
struct Cache {
    order: VecDeque<usize>,
    data: HashMap<usize, Arc<Vec<u8>>>,
}

/// A read-only, multi-segment EWF-E01 image.
pub struct E01Image {
    files: Vec<RawFile>,
    paths: Vec<PathBuf>,
    chunks: Vec<ChunkRef>,
    chunk_size: usize,
    len: u64,
    cache: Mutex<Cache>,
}

impl E01Image {
    /// Open the segment set that starts at (or contains) `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let paths = segment_paths(path)?;
        let mut opened = Vec::with_capacity(paths.len());

        for (index, p) in paths.iter().enumerate() {
            let file = RawFile::open(p)?;
            let header = read_vec_at(&file, 0, 13)?;
            if &header[..8] == LVF_MAGIC {
                bail!("logical evidence files (.L01) are not supported");
            }
            if &header[..8] != EWF_MAGIC {
                bail!("'{}' is not an EWF-E01 segment", p.display());
            }
            if header[8] != 1 || le_u16(&header, 11, "EWF header terminator")? != 0 {
                bail!("'{}' has an unsupported EWF header", p.display());
            }
            let number = le_u16(&header, 9, "EWF segment number")?;
            let (chunks, chunk_size, media_len) = parse_sections(&file, index)?;
            opened.push((
                number,
                index,
                p.clone(),
                file,
                chunks,
                chunk_size,
                media_len,
            ));
        }

        opened.sort_by_key(|item| item.0);
        for pair in opened.windows(2) {
            if pair[0].0 == pair[1].0 {
                bail!("duplicate EWF segment number {}", pair[0].0);
            }
        }
        let positions: HashMap<usize, usize> = opened
            .iter()
            .enumerate()
            .map(|(new, item)| (item.1, new))
            .collect();

        let mut files = Vec::with_capacity(opened.len());
        let mut source_paths = Vec::with_capacity(opened.len());
        let mut chunks = Vec::new();
        let mut chunk_size = 0usize;
        let mut len = 0u64;
        for (_, old_index, path, file, mut seg_chunks, seg_chunk_size, seg_len) in opened {
            let segment = *positions
                .get(&old_index)
                .context("EWF segment index was lost")?;
            for chunk in &mut seg_chunks {
                chunk.segment = segment;
            }
            if seg_chunk_size != 0 {
                if chunk_size != 0 && chunk_size != seg_chunk_size {
                    bail!("EWF segments disagree on chunk size");
                }
                chunk_size = seg_chunk_size;
            }
            if seg_len != 0 {
                if len != 0 && len != seg_len {
                    bail!("EWF segments disagree on media size");
                }
                len = seg_len;
            }
            source_paths.push(path);
            files.push(file);
            chunks.extend(seg_chunks);
        }

        if chunk_size == 0 {
            bail!("EWF image does not declare a chunk size");
        }
        if len == 0 {
            bail!("EWF image does not declare a media size");
        }
        let expected = len.div_ceil(chunk_size as u64) as usize;
        if chunks.len() < expected {
            bail!(
                "EWF image declares {} chunks but only {} table entries were found",
                expected,
                chunks.len()
            );
        }
        if chunks.len() > expected {
            bail!(
                "EWF image declares {expected} chunks but its tables contain {} entries",
                chunks.len()
            );
        }

        Ok(Self {
            files,
            paths: source_paths,
            chunks,
            chunk_size,
            len,
            cache: Mutex::new(Cache::default()),
        })
    }

    /// Backing files tracked for source-change detection.
    pub(crate) fn source_paths(&self) -> Vec<PathBuf> {
        self.paths.clone()
    }

    fn chunk(&self, index: usize) -> Result<Arc<Vec<u8>>> {
        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| anyhow::anyhow!("EWF chunk cache is poisoned"))?;
            if let Some(data) = cache.data.get(&index) {
                return Ok(Arc::clone(data));
            }
        }

        let reference = self
            .chunks
            .get(index)
            .with_context(|| format!("EWF chunk {index} is missing"))?;
        let bytes = self.load_chunk(reference)?;
        let data = Arc::new(bytes);

        let mut cache = self
            .cache
            .lock()
            .map_err(|_| anyhow::anyhow!("EWF chunk cache is poisoned"))?;
        if cache.data.len() >= CACHE_CHUNKS {
            if let Some(old) = cache.order.pop_front() {
                cache.data.remove(&old);
            }
        }
        cache.order.push_back(index);
        cache.data.insert(index, Arc::clone(&data));
        Ok(data)
    }

    fn load_chunk(&self, reference: &ChunkRef) -> Result<Vec<u8>> {
        let file = &self.files[reference.segment];
        let available = file.len().saturating_sub(reference.offset);
        let size = self.chunk_size as u64;
        if reference.compressed {
            // A compressed chunk is shorter than the logical chunk.  Stream
            // directly from the segment so a large E01 does not allocate the
            // remainder of the file for every 64 KiB chunk.
            if available < 2 {
                bail!(
                    "EWF compressed chunk at byte {} is truncated",
                    reference.offset
                );
            }
            let mut out = vec![0u8; self.chunk_size];
            ZlibDecoder::new(EwfChunkReader {
                file,
                offset: reference.offset,
                end: file.len(),
            })
            .read_exact(&mut out)
            .with_context(|| format!("cannot decompress EWF chunk at byte {}", reference.offset))?;
            Ok(out)
        } else {
            if available < size + 4 {
                bail!(
                    "EWF uncompressed chunk at byte {} is truncated",
                    reference.offset
                );
            }
            let raw = read_vec_at(file, reference.offset, (size + 4) as usize)?;
            let stored = adler32(&raw[..self.chunk_size]);
            let expected = le_u32(&raw, self.chunk_size, "EWF chunk checksum")?;
            if stored != expected {
                bail!(
                    "checksum mismatch in EWF chunk at byte {}",
                    reference.offset
                );
            }
            Ok(raw[..self.chunk_size].to_vec())
        }
    }
}

impl ImageSource for E01Image {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let chunk_size = self.chunk_size as u64;
        let mut done = 0usize;
        while done < buf.len() {
            let pos = offset + done as u64;
            if pos >= self.len {
                break;
            }
            let index = (pos / chunk_size) as usize;
            let within = (pos % chunk_size) as usize;
            let chunk = self
                .chunk(index)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            let n = (chunk.len() - within).min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&chunk[within..within + n]);
            done += n;
        }
        Ok(done)
    }
}

/// Streams one compressed chunk without loading the rest of its segment.
struct EwfChunkReader<'a> {
    file: &'a RawFile,
    offset: u64,
    end: u64,
}

impl Read for EwfChunkReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.offset >= self.end {
            return Ok(0);
        }
        let want = (self.end - self.offset).min(buf.len() as u64) as usize;
        let n = self.file.read_at(self.offset, &mut buf[..want])?;
        self.offset += n as u64;
        Ok(n)
    }
}

fn parse_sections(file: &RawFile, segment: usize) -> Result<(Vec<ChunkRef>, usize, u64)> {
    let file_len = file.len();
    let mut pos = 13u64;
    let mut chunks = Vec::new();
    let mut chunk_size = 0usize;
    let mut media_len = 0u64;

    while let Some(descriptor_end) = pos.checked_add(76) {
        if descriptor_end > file_len {
            break;
        }
        let descriptor = read_vec_at(file, pos, 76)?;
        let kind = section_name(&descriptor[..16]);
        let next = le_u64(&descriptor, 16, "EWF section link")?;
        let size = le_u64(&descriptor, 24, "EWF section size")?;
        let data_offset = descriptor_end;

        if matches!(kind.as_str(), "volume" | "data" | "disk") {
            let want = (size.min(1052) as usize).max(24);
            let data = read_vec_at(file, data_offset, want)?;
            let sectors_per_chunk = le_u32(&data, 8, "EWF sectors per chunk")?;
            let bytes_per_sector = le_u32(&data, 12, "EWF bytes per sector")?;
            if sectors_per_chunk != 0 && bytes_per_sector != 0 {
                let bytes = (sectors_per_chunk as u64)
                    .checked_mul(bytes_per_sector as u64)
                    .context("EWF chunk size overflow")?;
                let bytes =
                    usize::try_from(bytes).context("EWF chunk size exceeds address space")?;
                if chunk_size != 0 && chunk_size != bytes {
                    bail!("EWF sections disagree on chunk size");
                }
                chunk_size = bytes;
            }
            let sectors = if data.len() >= 24 {
                le_u64(&data, 16, "EWF sector count")?
            } else {
                u64::from(le_u32(&data, 16, "EWF sector count")?)
            };
            if sectors != 0 {
                let bytes = sectors
                    .checked_mul(bytes_per_sector as u64)
                    .context("EWF media size overflow")?;
                if media_len != 0 && media_len != bytes {
                    bail!("EWF sections disagree on media size");
                }
                media_len = bytes;
            }
        }

        if kind == "table" {
            let header = read_vec_at(file, data_offset, 24)?;
            let count = le_u32(&header, 0, "EWF table entry count")? as usize;
            let base = le_u64(&header, 8, "EWF table base offset")?;
            if count != 0 {
                let entries_offset = data_offset
                    .checked_add(24)
                    .context("EWF table offset overflow")?;
                let entries_len = count.checked_mul(4).context("EWF table size overflow")?;
                if entries_offset
                    .checked_add(entries_len as u64)
                    .context("EWF table range overflow")?
                    > file_len
                {
                    bail!("EWF table lies past the end of the segment");
                }
                let entries = read_vec_at(file, entries_offset, entries_len)?;
                for i in 0..count {
                    let raw = le_u32(&entries, i * 4, "EWF table entry")?;
                    let compressed = (raw & 0x8000_0000) != 0;
                    let relative = u64::from(raw & 0x7fff_ffff);
                    let offset = base
                        .checked_add(relative)
                        .context("EWF chunk offset overflow")?;
                    if offset >= file_len {
                        bail!("EWF chunk offset {offset} lies past the segment end");
                    }
                    chunks.push(ChunkRef {
                        segment,
                        offset,
                        compressed,
                    });
                }
            }
        }

        let next_end = match next.checked_add(76) {
            Some(value) => value,
            None => break,
        };
        if kind == "done" || next <= pos || next_end > file_len {
            break;
        }
        pos = next;
    }

    if chunks.is_empty() {
        bail!("EWF segment contains no data table");
    }
    Ok((chunks, chunk_size, media_len))
}

fn section_name(raw: &[u8]) -> String {
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end])
        .trim()
        .to_ascii_lowercase()
}

fn segment_paths(path: &Path) -> Result<Vec<PathBuf>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .context("E01 image needs a three-character segment extension")?;
    if ext.len() != 3 {
        bail!(
            "E01 image '{}' needs an extension such as .E01",
            path.display()
        );
    }
    let prefix = &ext[..1];
    let number = base36_decode(&ext[1..])
        .with_context(|| format!("invalid EWF segment extension '.{ext}'"))?;
    if number == 0 {
        bail!("EWF segment numbers start at 01");
    }

    let mut paths = Vec::new();
    let mut n = number;
    loop {
        let candidate = path.with_extension(format!("{prefix}{}", base36_encode(n)?));
        if !candidate.is_file() {
            break;
        }
        paths.push(candidate);
        if n == 1 {
            break;
        }
        n -= 1;
    }
    paths.reverse();

    let mut n = number + 1;
    loop {
        let candidate = path.with_extension(format!("{prefix}{}", base36_encode(n)?));
        if !candidate.is_file() {
            break;
        }
        paths.push(candidate);
        n += 1;
    }
    Ok(paths)
}

fn base36_decode(text: &str) -> Option<u32> {
    let bytes = text.as_bytes();
    if bytes.len() != 2 {
        return None;
    }
    let mut value = 0u32;
    for b in bytes {
        let digit = match b {
            b'0'..=b'9' => u32::from(b - b'0'),
            b'a'..=b'z' => u32::from(b - b'a') + 10,
            b'A'..=b'Z' => u32::from(b - b'A') + 10,
            _ => return None,
        };
        value = value.checked_mul(36)?.checked_add(digit)?;
    }
    Some(value)
}

fn base36_encode(value: u32) -> Result<String> {
    const DIGITS: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    if value > 35 * 36 + 35 {
        bail!("EWF segment number {value} exceeds the two-character extension limit");
    }
    let high = DIGITS[(value / 36) as usize] as char;
    let low = DIGITS[(value % 36) as usize] as char;
    Ok(format!("{high}{low}"))
}
