//! Read-only VMware virtual disk (VMDK) image source.
//!
//! The descriptor is parsed for `FLAT`, `SPARSE` and `ZERO` extents, which
//! covers monolithicFlat, splitFlat, monolithicSparse and splitSparse images.
//! ESX-managed extent types and compressed streamOptimized grains are
//! rejected explicitly instead of returning partially correct data.

use super::{le_u16, le_u32, le_u64};
use crate::io::{read_vec_at, ImageSource, RawFile};
use anyhow::{bail, Context, Result};
use std::io;
use std::path::{Path, PathBuf};

const SECTOR: u64 = 512;
const FLAG_ZEROED_GRAIN: u32 = 0x0000_0004;
const FLAG_COMPRESSED: u32 = 0x0001_0000;
const GD_AT_END: u64 = u64::MAX;

enum ExtentKind {
    Flat { file: RawFile, base: u64 },
    Sparse(VmdkSparse),
    Zero,
}

struct Extent {
    start: u64,
    len: u64,
    kind: ExtentKind,
}

/// A read-only VMDK composed of one or more descriptor extents.
pub struct VmdkImage {
    extents: Vec<Extent>,
    paths: Vec<PathBuf>,
    len: u64,
}

impl VmdkImage {
    /// Open a VMDK descriptor (or sparse extent with an embedded descriptor).
    pub fn open(path: &Path) -> Result<Self> {
        let text = read_descriptor(path)?;
        let lines = parse_descriptor(&text)?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));

        let mut extents = Vec::with_capacity(lines.len());
        let mut paths = Vec::with_capacity(lines.len());
        let mut len = 0u64;
        for line in lines {
            let extent_len = line
                .sectors
                .checked_mul(SECTOR)
                .context("VMDK extent size overflow")?;
            let kind = match line.kind.as_str() {
                "ZERO" => ExtentKind::Zero,
                "FLAT" => {
                    let resolved = resolve(parent, &line.filename)?;
                    let file = RawFile::open(&resolved)?;
                    let base = line.offset.saturating_mul(SECTOR);
                    if base
                        .checked_add(extent_len)
                        .context("VMDK extent overflow")?
                        > file.len()
                    {
                        bail!(
                            "VMDK flat extent '{}' is shorter than declared",
                            resolved.display()
                        );
                    }
                    paths.push(resolved);
                    ExtentKind::Flat { file, base }
                }
                "SPARSE" => {
                    let resolved = resolve(parent, &line.filename)?;
                    let image = VmdkSparse::open(&resolved)?;
                    paths.push(resolved);
                    ExtentKind::Sparse(image)
                }
                other => bail!("unsupported VMDK extent type '{other}'"),
            };
            extents.push(Extent {
                start: len,
                len: extent_len,
                kind,
            });
            len = len.checked_add(extent_len).context("VMDK size overflow")?;
        }

        if extents.is_empty() {
            bail!("VMDK descriptor has no extents");
        }
        Ok(Self {
            extents,
            paths,
            len,
        })
    }

    /// Backing files tracked for source-change detection.
    pub(crate) fn source_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.paths.clone();
        paths.sort();
        paths.dedup();
        paths
    }
}

impl ImageSource for VmdkImage {
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
            let extent = self
                .extents
                .iter()
                .find(|e| pos >= e.start && pos < e.start + e.len)
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "VMDK hole"))?;
            let within = pos - extent.start;
            let want = (extent.len - within).min((buf.len() - done) as u64) as usize;
            let n = match &extent.kind {
                ExtentKind::Zero => {
                    buf[done..done + want].fill(0);
                    want
                }
                ExtentKind::Flat { file, base } => {
                    file.read_at(base + within, &mut buf[done..done + want])?
                }
                ExtentKind::Sparse(sparse) => {
                    sparse.read_at(within, &mut buf[done..done + want])?
                }
            };
            if n == 0 {
                break;
            }
            done += n;
        }
        Ok(done)
    }
}

#[derive(Debug)]
struct ExtentLine {
    sectors: u64,
    kind: String,
    filename: String,
    offset: u64,
}

fn read_descriptor(path: &Path) -> Result<String> {
    let file = RawFile::open(path)?;
    let file_len = file.len();
    let head = read_vec_at(&file, 0, file_len.min(512) as usize)?;
    if head.len() >= 4 && &head[..4] == b"KDMV" {
        let sector = le_u64(&head, 28, "VMDK descriptor sector")?;
        let sectors = le_u64(&head, 36, "VMDK descriptor size")?;
        if sector == 0 || sectors == 0 {
            bail!("VMDK sparse extent has no embedded descriptor");
        }
        let byte_offset = sector
            .checked_mul(SECTOR)
            .context("VMDK descriptor offset overflow")?;
        let byte_len = sectors
            .checked_mul(SECTOR)
            .context("VMDK descriptor size overflow")? as usize;
        if byte_offset
            .checked_add(byte_len as u64)
            .context("VMDK descriptor range overflow")?
            > file_len
        {
            bail!("VMDK embedded descriptor lies past the end of the file");
        }
        let bytes = read_vec_at(&file, byte_offset, byte_len)?;
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }

    let prefix_len = file_len.min(64 * 1024) as usize;
    let prefix = read_vec_at(&file, 0, prefix_len)?;
    if !prefix.windows(20).any(|w| w == b"# Extent description") {
        bail!("'{}' is not a VMDK descriptor", path.display());
    }
    Ok(String::from_utf8_lossy(&prefix).into_owned())
}

fn parse_descriptor(text: &str) -> Result<Vec<ExtentLine>> {
    let mut in_extents = false;
    let mut lines = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_matches(|c: char| c == '\0' || c.is_whitespace());
        if line.starts_with("# Extent description") {
            in_extents = true;
            continue;
        }
        if !in_extents || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with("RW") && !line.starts_with("RDONLY") {
            break;
        }
        let tokens = tokenize(line)?;
        if tokens.len() < 3 {
            bail!("malformed VMDK extent line: {line}");
        }
        let sectors: u64 = tokens[1]
            .parse()
            .with_context(|| format!("invalid VMDK extent size in '{line}'"))?;
        let kind = tokens[2].to_ascii_uppercase();
        if kind == "ZERO" {
            lines.push(ExtentLine {
                sectors,
                kind,
                filename: String::new(),
                offset: 0,
            });
            continue;
        }
        if tokens.len() < 4 {
            bail!("VMDK {kind} extent needs a filename: {line}");
        }
        let offset = match tokens.get(4) {
            Some(value) => value
                .parse()
                .with_context(|| format!("invalid VMDK extent offset in '{line}'"))?,
            None => 0,
        };
        lines.push(ExtentLine {
            sectors,
            kind,
            filename: tokens[3].clone(),
            offset,
        });
    }
    Ok(lines)
}

fn tokenize(line: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if quoted {
        bail!("unterminated quote in VMDK descriptor line: {line}");
    }
    if !current.is_empty() {
        out.push(current);
    }
    Ok(out)
}

fn resolve(parent: &Path, name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(parent.join(path))
}

/// One hosted-sparse VMDK extent, addressed in sectors.
struct VmdkSparse {
    file: RawFile,
    len: u64,
    grain_bytes: u64,
    num_gte: usize,
    grain_tables: Vec<Vec<u32>>,
    zeroed_grain: bool,
}

impl VmdkSparse {
    fn open(path: &Path) -> Result<Self> {
        let file = RawFile::open(path)?;
        let file_len = file.len();
        let header = read_vec_at(&file, 0, 512)?;
        if &header[..4] != b"KDMV" {
            bail!("'{}' is not a VMDK sparse extent", path.display());
        }
        let version = le_u32(&header, 4, "VMDK version")?;
        if !(1..=3).contains(&version) {
            bail!("unsupported VMDK sparse version {version}");
        }
        let flags = le_u32(&header, 8, "VMDK flags")?;
        if flags & FLAG_COMPRESSED != 0 {
            bail!("compressed streamOptimized VMDK extents are not supported");
        }
        let capacity = le_u64(&header, 12, "VMDK capacity")?;
        let grain_size = le_u64(&header, 20, "VMDK grain size")?;
        let num_gte = le_u32(&header, 44, "VMDK grain table entries")? as usize;
        let mut gd_offset = le_u64(&header, 56, "VMDK grain directory offset")?;
        let compression = le_u16(&header, 77, "VMDK compression method")?;
        if compression != 0 {
            bail!("VMDK grain compression is not supported");
        }
        if grain_size == 0 || !grain_size.is_power_of_two() {
            bail!("invalid VMDK grain size {grain_size}");
        }
        if num_gte == 0 || capacity == 0 {
            bail!("VMDK sparse extent has no capacity");
        }
        if gd_offset == GD_AT_END {
            if file_len < 1024 {
                bail!("VMDK stream-optimized extent is truncated");
            }
            let tail = read_vec_at(&file, file_len - 1024, 512)?;
            if &tail[..4] != b"KDMV" {
                bail!("VMDK secondary extent header is missing");
            }
            gd_offset = le_u64(&tail, 56, "VMDK grain directory offset")?;
        }
        if gd_offset == 0 {
            bail!("VMDK sparse extent has no grain directory");
        }

        let per_table = (num_gte as u64)
            .checked_mul(grain_size)
            .context("VMDK grain table coverage overflow")?;
        let gd_count = capacity.div_ceil(per_table) as usize;
        let gd_bytes = gd_count
            .checked_mul(4)
            .context("VMDK grain directory size overflow")?;
        let gd_start = gd_offset
            .checked_mul(SECTOR)
            .context("VMDK grain directory offset overflow")?;
        if gd_start
            .checked_add(gd_bytes as u64)
            .context("VMDK grain directory range overflow")?
            > file_len
        {
            bail!("VMDK grain directory lies past the end of the file");
        }
        let raw_gd = read_vec_at(&file, gd_start, gd_bytes)?;
        let zeroed_grain = flags & FLAG_ZEROED_GRAIN != 0;
        let mut grain_tables = Vec::with_capacity(gd_count);
        for i in 0..gd_count {
            let sector = u64::from(le_u32(&raw_gd, i * 4, "VMDK grain directory entry")?);
            if sector == 0 || (zeroed_grain && sector == 1) {
                grain_tables.push(Vec::new());
                continue;
            }
            let table_start = sector
                .checked_mul(SECTOR)
                .context("VMDK grain table offset overflow")?;
            let table_bytes = num_gte
                .checked_mul(4)
                .context("VMDK grain table size overflow")?;
            if table_start
                .checked_add(table_bytes as u64)
                .context("VMDK grain table range overflow")?
                > file_len
            {
                bail!("VMDK grain table lies past the end of the file");
            }
            let raw = read_vec_at(&file, table_start, table_bytes)?;
            let mut table = Vec::with_capacity(num_gte);
            for j in 0..num_gte {
                table.push(le_u32(&raw, j * 4, "VMDK grain table entry")?);
            }
            grain_tables.push(table);
        }

        Ok(Self {
            file,
            len: capacity
                .checked_mul(SECTOR)
                .context("VMDK extent size overflow")?,
            grain_bytes: grain_size
                .checked_mul(SECTOR)
                .context("VMDK grain size overflow")?,
            num_gte,
            grain_tables,
            zeroed_grain,
        })
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
            let grain = pos / self.grain_bytes;
            let within = pos % self.grain_bytes;
            let gd_index = (grain / self.num_gte as u64) as usize;
            let gte_index = (grain % self.num_gte as u64) as usize;
            let sector = match self
                .grain_tables
                .get(gd_index)
                .and_then(|t| t.get(gte_index))
            {
                Some(sector) => *sector,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "VMDK grain index out of range",
                    ))
                }
            };
            if sector == 0 || (self.zeroed_grain && sector == 1) {
                let zeros = (self.grain_bytes - within)
                    .min((buf.len() - done) as u64)
                    .min(self.len - pos) as usize;
                buf[done..done + zeros].fill(0);
                done += zeros;
                continue;
            }
            let file_offset = (u64::from(sector) * SECTOR)
                .checked_add(within)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "VMDK grain offset overflow")
                })?;
            let want = (self.grain_bytes - within).min((buf.len() - done) as u64) as usize;
            let n = self
                .file
                .read_at(file_offset, &mut buf[done..done + want])?;
            if n == 0 {
                break;
            }
            done += n;
        }
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_tokens_respect_quotes() {
        let tokens = tokenize("RW 1024 SPARSE \"my disk-s001.vmdk\" 0").unwrap();
        assert_eq!(tokens, ["RW", "1024", "SPARSE", "my disk-s001.vmdk", "0"]);
    }
}
