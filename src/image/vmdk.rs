//! Read-only VMware virtual disk (VMDK) image source.
//!
//! The descriptor is parsed for `FLAT`, `SPARSE` and `ZERO` extents, which
//! covers monolithicFlat, splitFlat, monolithicSparse and splitSparse images.
//! ESX-managed extent types and compressed streamOptimized grains are
//! rejected explicitly instead of returning partially correct data.

use super::{le_u16, le_u32, le_u64};
use crate::io::{read_vec_at, ImageSource, RawFile};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const SECTOR: u64 = 512;
const FLAG_ZEROED_GRAIN: u32 = 0x0000_0004;
const FLAG_COMPRESSED: u32 = 0x0001_0000;
const GD_AT_END: u64 = u64::MAX;
/// Hard ceiling on entries in one grain table (256 KiB per table at 4 bytes
/// per entry).  Legitimate VMDKs use 4096 or fewer.
const MAX_GTE_PER_TABLE: usize = 65_536;
/// Hard ceiling on the number of grain-directory entries (4 MiB directory).
const MAX_GD_ENTRIES: usize = 1_048_576;
/// Ceiling on total bytes held by the shared grain-table cache.  Identical
/// table offsets share one allocation, so duplicate directory entries do not
/// multiply memory (H-12).
const MAX_TABLE_CACHE_BYTES: u64 = 64 * 1024 * 1024;
/// Number of distinct grain tables kept in the LRU cache.
const TABLE_CACHE_SLOTS: usize = 16;

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
        Self::open_with(path, false)
    }

    /// Open with explicit permission to follow extents outside the descriptor
    /// directory.  The default (`false`) rejects absolute paths, `..` escapes
    /// and symlink escapes (H-13).
    pub fn open_with(path: &Path, allow_external: bool) -> Result<Self> {
        let text = read_descriptor(path)?;
        let lines = parse_descriptor(&text)?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        // A monolithicSparse extent embeds its descriptor and names itself in
        // the extent line.  When the file is renamed that name is stale; use
        // the file that was actually opened instead of resolving the name
        // (v5:F-23).  This also closes the sibling-swap confusion.
        let self_sparse = RawFile::open(path)
            .ok()
            .and_then(|f| read_vec_at(&f, 0, 4).ok())
            .map(|h| h == b"KDMV")
            .unwrap_or(false);

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
                    let resolved = resolve(parent, &line.filename, allow_external)?;
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
                    let resolved = if self_sparse {
                        path.to_path_buf()
                    } else {
                        resolve(parent, &line.filename, allow_external)?
                    };
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

fn resolve(parent: &Path, name: &str, allow_external: bool) -> Result<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        if !allow_external {
            bail!("VMDK absolute extent path '{}' is not allowed", name);
        }
        return Ok(path.to_path_buf());
    }
    let joined = parent.join(path);
    let canonical_parent = std::fs::canonicalize(parent)
        .with_context(|| format!("cannot canonicalize VMDK directory {}", parent.display()))?;
    let canonical = std::fs::canonicalize(&joined)
        .with_context(|| format!("VMDK extent '{}' cannot be resolved", name))?;
    if !canonical.starts_with(&canonical_parent) && !allow_external {
        bail!(
            "VMDK extent '{}' escapes the descriptor directory (parent {}, resolved {})",
            name,
            canonical_parent.display(),
            canonical.display()
        );
    }
    Ok(canonical)
}

/// LRU of decoded grain tables.  Duplicate grain-directory entries share a
/// single allocation, so a tiny file with thousands of identical entries
/// cannot multiply memory (H-12).
#[derive(Default)]
struct TableCache {
    order: VecDeque<u32>,
    data: HashMap<u32, Arc<Vec<u32>>>,
    bytes: u64,
}

/// One hosted-sparse VMDK extent, addressed in sectors.
struct VmdkSparse {
    file: RawFile,
    len: u64,
    grain_bytes: u64,
    num_gte: usize,
    /// Grain directory entries (table sector numbers).  Loaded once and
    /// bounded by `MAX_GD_ENTRIES` and the file length.
    directory: Vec<u32>,
    /// Lazily loaded, shared grain tables.
    tables: Mutex<TableCache>,
    table_bytes: usize,
    file_len: u64,
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
        if num_gte > MAX_GTE_PER_TABLE {
            bail!("VMDK grain table declares {num_gte} entries (limit {MAX_GTE_PER_TABLE})");
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
        let gd_count_u64 = capacity.div_ceil(per_table);
        if gd_count_u64 > MAX_GD_ENTRIES as u64 {
            bail!("VMDK grain directory declares {gd_count_u64} entries (limit {MAX_GD_ENTRIES})");
        }
        let gd_count = gd_count_u64 as usize;
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
        let mut directory = Vec::with_capacity(gd_count);
        let table_bytes = num_gte
            .checked_mul(4)
            .context("VMDK grain table size overflow")?;
        // A bounded number of distinct tables may exist; verify the total
        // declared table footprint before loading any of them.
        let mut unique: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for i in 0..gd_count {
            let sector = le_u32(&raw_gd, i * 4, "VMDK grain directory entry")?;
            if sector != 0 && !(zeroed_grain && sector == 1) {
                unique.insert(sector);
            }
        }
        if (unique.len() as u64).saturating_mul(table_bytes as u64) > MAX_TABLE_CACHE_BYTES {
            bail!(
                "VMDK declares {} distinct grain tables of {} bytes (limit {} bytes)",
                unique.len(),
                table_bytes,
                MAX_TABLE_CACHE_BYTES
            );
        }
        for i in 0..gd_count {
            directory.push(le_u32(&raw_gd, i * 4, "VMDK grain directory entry")?);
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
            directory,
            tables: Mutex::new(TableCache::default()),
            table_bytes,
            file_len,
            zeroed_grain,
        })
    }

    /// Fetch a grain table, loading it from the file on first use.  Duplicate
    /// directory entries share one cached allocation.
    fn table(&self, sector: u32) -> Option<Arc<Vec<u32>>> {
        {
            let cache = self.tables.lock().ok()?;
            if let Some(table) = cache.data.get(&sector) {
                return Some(Arc::clone(table));
            }
        }
        let table_start = (sector as u64)
            .checked_mul(SECTOR)?
            .checked_add(self.table_bytes as u64)?;
        if table_start > self.file_len {
            return None;
        }
        let raw = read_vec_at(&self.file, (sector as u64) * SECTOR, self.table_bytes).ok()?;
        let mut table = Vec::with_capacity(self.num_gte);
        for chunk in raw.chunks_exact(4) {
            table.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        table.resize(self.num_gte, 0);
        let table = Arc::new(table);
        let mut cache = self.tables.lock().ok()?;
        if cache.data.len() >= TABLE_CACHE_SLOTS
            || cache.bytes + self.table_bytes as u64 > MAX_TABLE_CACHE_BYTES
        {
            if let Some(old) = cache.order.pop_front() {
                if let Some(removed) = cache.data.remove(&old) {
                    cache.bytes = cache.bytes.saturating_sub((removed.len() * 4) as u64);
                }
            }
        }
        cache.order.push_back(sector);
        cache.bytes += self.table_bytes as u64;
        cache.data.insert(sector, Arc::clone(&table));
        Some(table)
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
            // Missing or out-of-range directory entries read as zeros (H-23),
            // matching qemu-img behaviour for unallocated thin-provisioned
            // grains.  Only an explicit grain sector is dereferenced.
            let table_sector = match self.directory.get(gd_index) {
                Some(s) => *s,
                None => {
                    let zeros = (self.grain_bytes - within)
                        .min((buf.len() - done) as u64)
                        .min(self.len - pos) as usize;
                    buf[done..done + zeros].fill(0);
                    done += zeros;
                    continue;
                }
            };
            if table_sector == 0 || (self.zeroed_grain && table_sector == 1) {
                let zeros = (self.grain_bytes - within)
                    .min((buf.len() - done) as u64)
                    .min(self.len - pos) as usize;
                buf[done..done + zeros].fill(0);
                done += zeros;
                continue;
            }
            let sector = match self
                .table(table_sector)
                .and_then(|t| t.get(gte_index).copied())
            {
                Some(s) => s,
                None => {
                    let zeros = (self.grain_bytes - within)
                        .min((buf.len() - done) as u64)
                        .min(self.len - pos) as usize;
                    buf[done..done + zeros].fill(0);
                    done += zeros;
                    continue;
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
