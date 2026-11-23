//! Additional read-only image sources.
//!
//! `CarveX` historically read a single raw file.  This module keeps that path
//! byte-for-byte identical while adding the evidence containers requested by
//! B-04.  Every implementation is an [`ImageSource`], so filesystem detection
//! and carving never need to know which container was opened.

// H-11: hostile container fields must be handled with checked access.
#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]

mod e01;
mod split;
mod vhd;
mod vmdk;

/// Hidden fixture builders shared by fuzz targets and parser property tests.
// H-11: fixture-only builders use fixed slices and infallible test data; the
// production parsers above remain subject to the deny list.
#[doc(hidden)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
pub mod fuzz_support;

use crate::io::{read_exact_at, read_vec_at, ImageSource, RawFile};
use crate::util::file_identity;
use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use serde::Serialize;
use std::any::Any;
use std::io::{self, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::NamedTempFile;

pub use e01::E01Image;
pub use split::SplitRaw;
pub use vhd::VhdImage;
pub use vmdk::VmdkImage;

/// Test-only image-open panic seam. Production never sets this.
#[doc(hidden)]
pub static TEST_PANIC_IN_OPEN: AtomicBool = AtomicBool::new(false);

/// Enable or disable the hidden image-open panic seam.
#[doc(hidden)]
pub fn set_test_panic_in_open(enabled: bool) {
    TEST_PANIC_IN_OPEN.store(enabled, Ordering::SeqCst);
}

fn parser_panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// How to react when a byte range of the image cannot be read (A-08, H-22).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OnReadError {
    /// Stop and fail (pre-0.3.0 behaviour).
    Abort,
    /// Treat the unreadable bytes as `0x00` and record the region.
    ZeroFill,
    /// Skip candidates that need the unreadable bytes.
    Skip,
}

#[allow(clippy::derivable_impls)]
impl Default for OnReadError {
    fn default() -> Self {
        Self::ZeroFill
    }
}

/// A byte range that could not be read from the source, with the reason.
#[derive(Debug, Clone, Serialize)]
pub struct UnreadableRegion {
    /// Start offset of the unreadable region.
    pub offset: u64,
    /// Length of the unreadable region in bytes.
    pub len: u64,
    /// Read failure reason.
    pub reason: String,
}

/// Shared log of unreadable ranges discovered while reading an image.
pub type RegionLog = Arc<Mutex<Vec<UnreadableRegion>>>;

pub(crate) fn record_unreadable(log: &RegionLog, offset: u64, len: u64, reason: &str) {
    if let Ok(mut regions) = log.lock() {
        if let Some(last) = regions.last_mut() {
            if last.offset + last.len == offset && last.reason == reason {
                last.len += len;
                return;
            }
        }
        regions.push(UnreadableRegion {
            offset,
            len,
            reason: reason.to_string(),
        });
    }
}

const READ_ERROR_SECTOR: u64 = 512;

/// Applies `--on-read-error` to sources that do not implement it themselves.
///
/// E01 already knows its chunk boundaries, so it is not wrapped. Raw and split
/// sources are retried one sector at a time: good sectors keep their bytes,
/// bad sectors become zeroes (or are recorded and skipped/aborted).
struct PolicySource {
    inner: Box<dyn ImageSource>,
    policy: OnReadError,
    regions: RegionLog,
}

impl PolicySource {
    fn new(inner: Box<dyn ImageSource>, policy: OnReadError, regions: RegionLog) -> Self {
        Self {
            inner,
            policy,
            regions,
        }
    }

    fn handle_error(&self, offset: u64, buf: &mut [u8], error: &io::Error) -> io::Result<usize> {
        let reason = format!("read error at byte {offset}: {error}");
        if self.policy == OnReadError::Abort {
            record_unreadable(&self.regions, offset, buf.len() as u64, &reason);
            return Err(io::Error::new(
                error.kind(),
                format!("read aborted: {reason}"),
            ));
        }

        buf.fill(0);
        let end = offset.saturating_add(buf.len() as u64);
        let mut cursor = offset;
        let mut sector = [0u8; READ_ERROR_SECTOR as usize];
        while cursor < end {
            let sector_start = cursor / READ_ERROR_SECTOR * READ_ERROR_SECTOR;
            let sector_end = sector_start.saturating_add(READ_ERROR_SECTOR).min(end);
            let lo = cursor.max(sector_start);
            let hi = sector_end;
            let want = (hi - lo) as usize;
            let tmp = sector.get_mut(..want).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "read-error sector slice")
            })?;
            let ok = matches!(self.inner.read_at(lo, tmp), Ok(n) if n == want);
            if ok {
                let from = (lo - offset) as usize;
                let end = from.checked_add(want).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "read-error range")
                })?;
                buf.get_mut(from..end)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "read-error destination")
                    })?
                    .copy_from_slice(tmp);
            } else {
                record_unreadable(&self.regions, lo, want as u64, &reason);
            }
            cursor = hi;
        }
        Ok(buf.len())
    }
}

impl ImageSource for PolicySource {
    fn len(&self) -> u64 {
        self.inner.len()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        match self.inner.read_at(offset, buf) {
            Ok(n) => Ok(n),
            Err(error) => self.handle_error(offset, buf, &error),
        }
    }
}

/// Explicit image container selection for `info` and `scan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InputFormat {
    /// Detect from magic bytes and the file name (default).
    Auto,
    /// Single raw/DD image. This is the historical behaviour.
    Raw,
    /// Concatenated raw segment files (`.001`, `.002`, ...).
    Split,
    /// Expert Witness Compression Format (`.E01`, `.E02`, ...).
    E01,
    /// Virtual Hard Disk, fixed or dynamic.
    Vhd,
    /// `VMware` virtual disk descriptor with flat or sparse extents.
    Vmdk,
    /// Read the image from standard input into a temporary spool file.
    Stdin,
}

impl InputFormat {
    /// Stable report value for this container format.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Raw => "raw",
            Self::Split => "split",
            Self::E01 => "e01",
            Self::Vhd => "vhd",
            Self::Vmdk => "vmdk",
            Self::Stdin => "stdin",
        }
    }
}

/// One physical file backing an image, as recorded in `input.files[]`.
#[derive(Debug, Clone, Serialize)]
pub struct SourceFile {
    /// Source file path.
    pub path: PathBuf,
    /// Source file size in bytes.
    pub size: u64,
    /// Source modification time as Unix seconds.
    pub modified_unix: Option<u64>,
}

/// Size and mtime snapshot of every file backing an opened image.
///
/// Multi-segment containers have a logical length that differs from the size
/// of any one backing file.  The snapshot therefore tracks the backing files,
/// not the logical image length, so `IMAGE_MODIFIED` stays false for a stable
/// E01/VHD/VMDK/split source.
#[derive(Debug, Clone)]
pub struct SourceIdentity {
    paths: Vec<PathBuf>,
    files: Vec<SourceFile>,
    size: u64,
    mtime: Option<u64>,
}

impl SourceIdentity {
    /// Observe all backing files before the scan starts.
    pub fn observe(paths: Vec<PathBuf>) -> Result<Self> {
        if paths.is_empty() {
            bail!("image source has no backing files");
        }
        let files = measure_files(&paths)?;
        let mut size = 0u64;
        let mut mtime = None;
        for file in &files {
            size = size
                .checked_add(file.size)
                .context("backing-file size overflow")?;
            if file.modified_unix > mtime {
                mtime = file.modified_unix;
            }
        }
        Ok(Self {
            paths,
            files,
            size,
            mtime,
        })
    }

    /// Per-file sizes and mtimes recorded when the image was opened.
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// Aggregate backing-file size recorded at open time.
    pub fn initial_size(&self) -> u64 {
        self.size
    }

    /// Mtime recorded at open time for the report's `modified_unix` field.
    pub fn initial_mtime(&self) -> Option<u64> {
        self.mtime
    }

    /// Current aggregate size, newest mtime, and whether all files are readable.
    pub fn current(&self) -> (u64, Option<u64>, bool) {
        measure(&self.paths)
    }
}

fn measure(paths: &[PathBuf]) -> (u64, Option<u64>, bool) {
    let mut size = 0u64;
    let mut mtime = None;
    let mut ok = true;
    for path in paths {
        match file_identity(path) {
            Ok((file_size, file_mtime)) => {
                match size.checked_add(file_size) {
                    Some(total) => size = total,
                    None => ok = false,
                }
                if file_mtime > mtime {
                    mtime = file_mtime;
                }
            }
            Err(_) => ok = false,
        }
    }
    (size, mtime, ok)
}

fn measure_files(paths: &[PathBuf]) -> Result<Vec<SourceFile>> {
    paths
        .iter()
        .map(|path| {
            let (size, modified_unix) = file_identity(path)?;
            Ok(SourceFile {
                path: path.clone(),
                size,
                modified_unix,
            })
        })
        .collect()
}

/// An opened image plus the metadata the CLI needs for reporting.
///
/// The temporary file guard is kept alive for stdin even though the scanner
/// only sees the source trait object; dropping it early would unlink the spool
/// underneath an open file handle on some platforms.
pub struct OpenedImage {
    /// Opened image source implementation.
    pub source: Box<dyn ImageSource>,
    /// Logical image length in bytes.
    pub len: u64,
    /// Display name/path for reports.
    pub display: String,
    /// Identity metadata captured at open time.
    pub identity: Option<SourceIdentity>,
    /// Detected container format.
    pub format: InputFormat,
    /// Byte ranges that could not be read (H-22).  Filled by container
    /// readers and copied into the scan report by the CLI.
    pub unreadable_regions: RegionLog,
    _temp_guard: Option<NamedTempFile>,
}

/// Open `path` as a read-only image source.
///
/// A path equal to `-` always means stdin.  With [`InputFormat::Auto`] the
/// remaining containers are identified by magic bytes; the file name is only
/// used for split raw and E01 segment discovery.  An explicit format bypasses
/// detection so unusual files can still be opened as raw.
pub fn open_image(path: &Path, format: InputFormat) -> Result<OpenedImage> {
    open_image_with_options(path, format, &OpenOptions::default())
}

/// Options controlling how an image is opened (H-11, H-13, H-22).
pub struct OpenOptions {
    /// Allow container extents that reference external files.
    pub allow_external_extents: bool,
    /// Policy for unreadable sectors/chunks.
    pub on_read_error: OnReadError,
    /// Maximum bytes spooled from stdin; 0 means unlimited.
    pub max_stdin_bytes: u64,
    /// Optional spool directory for container materialization.
    pub spool_dir: Option<PathBuf>,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            allow_external_extents: false,
            on_read_error: OnReadError::ZeroFill,
            max_stdin_bytes: 0,
            spool_dir: None,
        }
    }
}

/// Open an image with the supplied options.  The default [`open_image`] never
/// allows external VMDK extents and zero-fills unreadable regions.
pub fn open_image_with_options(
    path: &Path,
    format: InputFormat,
    options: &OpenOptions,
) -> Result<OpenedImage> {
    // A malformed container must produce a typed error, never an unwind that
    // escapes the CLI. The closure is unwind-safe by construction: it only
    // builds owned values and returns them after all parsers have finished.
    catch_unwind(AssertUnwindSafe(|| open_image_inner(path, format, options))).map_err(
        |payload| {
            anyhow!(
                "image parser panicked: {}",
                parser_panic_message(payload.as_ref())
            )
        },
    )?
}

fn open_image_inner(
    path: &Path,
    format: InputFormat,
    options: &OpenOptions,
) -> Result<OpenedImage> {
    if TEST_PANIC_IN_OPEN.load(Ordering::SeqCst) {
        panic!("test-only panic injected in image open");
    }
    if path == Path::new("-") {
        if !matches!(format, InputFormat::Auto | InputFormat::Stdin) {
            bail!("an explicit container cannot be used with stdin; use auto or stdin");
        }
        return open_stdin(options);
    }
    if format == InputFormat::Stdin {
        bail!("--input-format stdin requires the image path '-'");
    }

    let probe = RawFile::open(path)?;
    let len = probe.len();
    let chosen = if format == InputFormat::Auto {
        detect(path, &probe, len)?
    } else {
        format
    };

    let regions: RegionLog = Arc::new(Mutex::new(Vec::new()));
    let (source, len, paths): (Box<dyn ImageSource>, u64, Vec<PathBuf>) = match chosen {
        InputFormat::Auto => unreachable!("auto was resolved above"),
        InputFormat::Raw => (
            Box::new(PolicySource::new(
                Box::new(probe),
                options.on_read_error,
                Arc::clone(&regions),
            )),
            len,
            vec![path.to_path_buf()],
        ),
        InputFormat::Split => {
            let image = SplitRaw::open(path)?;
            let len = image.len();
            let paths = image.source_paths();
            (
                Box::new(PolicySource::new(
                    Box::new(image),
                    options.on_read_error,
                    Arc::clone(&regions),
                )),
                len,
                paths,
            )
        }
        InputFormat::E01 => {
            let image = E01Image::open_with(path, options.on_read_error, Arc::clone(&regions))?;
            let len = image.len();
            let paths = image.source_paths();
            (Box::new(image), len, paths)
        }
        InputFormat::Vhd => {
            let image = VhdImage::open(path)?;
            let len = image.len();
            let paths = image.source_paths();
            (Box::new(image), len, paths)
        }
        InputFormat::Vmdk => {
            let image = VmdkImage::open_with(path, options.allow_external_extents)?;
            let len = image.len();
            let paths = image.source_paths();
            (Box::new(image), len, paths)
        }
        InputFormat::Stdin => bail!("stdin cannot be opened from a path"),
    };
    let identity = SourceIdentity::observe(paths)?;

    Ok(OpenedImage {
        source,
        len,
        display: path.display().to_string(),
        identity: Some(identity),
        format: chosen,
        unreadable_regions: regions,
        _temp_guard: None,
    })
}

fn open_stdin(options: &OpenOptions) -> Result<OpenedImage> {
    let mut temp = match &options.spool_dir {
        Some(dir) => NamedTempFile::new_in(dir).context("cannot create stdin spool file")?,
        None => NamedTempFile::new().context("cannot create stdin spool file")?,
    };
    let mut stdin = io::stdin().lock();
    let copied = if options.max_stdin_bytes > 0 {
        let mut limited = stdin.take(options.max_stdin_bytes + 1);
        io::copy(&mut limited, &mut temp).context("cannot read image from stdin")?
    } else {
        io::copy(&mut stdin, &mut temp).context("cannot read image from stdin")?
    };
    if options.max_stdin_bytes > 0 && copied > options.max_stdin_bytes {
        bail!(
            "stdin image exceeds --max-stdin-bytes ({})",
            options.max_stdin_bytes
        );
    }
    temp.flush().context("cannot flush stdin spool file")?;
    let len = temp
        .as_file()
        .metadata()
        .context("cannot stat stdin spool file")?
        .len();
    if len == 0 {
        bail!("stdin image is empty");
    }
    let source = RawFile::open(temp.path())?;
    Ok(OpenedImage {
        source: Box::new(source),
        len,
        display: "-".to_string(),
        identity: None,
        format: InputFormat::Stdin,
        unreadable_regions: Arc::new(Mutex::new(Vec::new())),
        _temp_guard: Some(temp),
    })
}

fn vhd_footer_checksum_valid(footer: &[u8]) -> bool {
    if footer.len() < 512 {
        return false;
    }
    let mut sum: u32 = 0;
    for (i, b) in footer.iter().enumerate() {
        if (64..68).contains(&i) {
            continue;
        }
        sum = sum.wrapping_add(u32::from(*b));
    }
    let expected = !sum;
    field::<4>(footer, 64, "VHD footer checksum")
        .is_ok_and(|bytes| u32::from_be_bytes(bytes) == expected)
}

fn looks_like_vmdk_descriptor(prefix: &[u8]) -> bool {
    if !prefix.windows(20).any(|w| w == b"# Extent description") {
        return false;
    }
    // Require additional VMDK descriptor markers to avoid false positives on
    // arbitrary raw data (H-24).
    let has_ver = prefix.windows(8).any(|w| w == b"version=");
    let has_cid = prefix.windows(4).any(|w| w == b"CID=");
    let has_create = prefix.windows(10).any(|w| w == b"createType");
    let has_extent = prefix
        .windows(5)
        .any(|w| w == b"\nRW " || w == b"\nRD " || w == b"\nNOAC");
    has_ver || has_cid || has_create || has_extent
}

fn detect(path: &Path, probe: &RawFile, len: u64) -> Result<InputFormat> {
    const EWF_MAGIC: &[u8] = b"EVF\x09\x0d\x0a\xff\x00";
    const LVF_MAGIC: &[u8] = b"LVF\x09\x0d\x0a\xff\x00";
    const EWF2_MAGIC: &[u8] = b"EVF2\r\n\x81\x00";
    const LEF2_MAGIC: &[u8] = b"LEF2\r\n\x81\x00";

    let mut head = [0u8; 8];
    if len >= head.len() as u64 {
        read_exact_at(probe, &mut head, 0)?;
        if head == EWF2_MAGIC || head == LEF2_MAGIC {
            bail!("EWF2/Ex01 and Lx01 images are not supported");
        }
        if head == LVF_MAGIC {
            bail!("logical evidence files (.L01) are not supported");
        }
        if head == EWF_MAGIC {
            return Ok(InputFormat::E01);
        }
        if head.starts_with(b"KDMV") {
            return Ok(InputFormat::Vmdk);
        }
    }

    if len >= 512 {
        let footer = read_vec_at(probe, len - 512, 512)?;
        if footer.starts_with(b"conectix") && vhd_footer_checksum_valid(&footer) {
            return Ok(InputFormat::Vhd);
        }
    }

    let prefix = read_vec_at(probe, 0, len.min(64 * 1024) as usize)?;
    if looks_like_vmdk_descriptor(&prefix) {
        return Ok(InputFormat::Vmdk);
    }

    // Content-based detection wins over the numeric-extension heuristic so a
    // container named `disk.001` is not mistaken for a raw segment.
    if split::looks_like_split(path) {
        return Ok(InputFormat::Split);
    }

    Ok(InputFormat::Raw)
}

/// Read a fixed-size byte array with a useful error when a container is cut.
pub(crate) fn field<const N: usize>(buf: &[u8], offset: usize, what: &str) -> Result<[u8; N]> {
    buf.get(offset..offset.saturating_add(N))
        .and_then(|s| s.try_into().ok())
        .with_context(|| format!("truncated {what}"))
}

pub(crate) fn le_u16(buf: &[u8], offset: usize, what: &str) -> Result<u16> {
    Ok(u16::from_le_bytes(field(buf, offset, what)?))
}

pub(crate) fn le_u32(buf: &[u8], offset: usize, what: &str) -> Result<u32> {
    Ok(u32::from_le_bytes(field(buf, offset, what)?))
}

pub(crate) fn le_u64(buf: &[u8], offset: usize, what: &str) -> Result<u64> {
    Ok(u64::from_le_bytes(field(buf, offset, what)?))
}

pub(crate) fn be_u32(buf: &[u8], offset: usize, what: &str) -> Result<u32> {
    Ok(u32::from_be_bytes(field(buf, offset, what)?))
}

pub(crate) fn be_u64(buf: &[u8], offset: usize, what: &str) -> Result<u64> {
    Ok(u64::from_be_bytes(field(buf, offset, what)?))
}

/// Adler-32 used by the uncompressed chunks in EWF-E01 images.
pub(crate) fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a = 1u32;
    let mut b = 0u32;
    for chunk in data.chunks(5552) {
        for byte in chunk {
            a += u32::from(*byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// One's-complement checksum used by VHD footers and dynamic headers.
pub(crate) fn ones_complement(buf: &[u8]) -> u32 {
    let sum: u64 = buf.iter().map(|b| u64::from(*b)).sum();
    !(sum as u32)
}

#[cfg(test)]
// Test fixtures use fixed slices and expect valid layouts; the production
// parser above remains subject to the deny list.
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    struct FailingSource {
        data: Vec<u8>,
        fail_at: u64,
    }

    impl ImageSource for FailingSource {
        fn len(&self) -> u64 {
            self.data.len() as u64
        }

        fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
            if (offset..offset + buf.len() as u64).contains(&self.fail_at) {
                return Err(io::Error::other("mock read failure"));
            }
            let start = offset as usize;
            let n = buf.len().min(self.data.len().saturating_sub(start));
            if n == 0 {
                return Ok(0);
            }
            buf[..n].copy_from_slice(&self.data[start..start + n]);
            Ok(n)
        }
    }

    fn source() -> FailingSource {
        FailingSource {
            data: vec![0xA5; READ_ERROR_SECTOR as usize * 3],
            fail_at: READ_ERROR_SECTOR * 2,
        }
    }

    #[test]
    fn raw_read_error_policy_zero_fills_one_sector() {
        let regions: RegionLog = Arc::new(Mutex::new(Vec::new()));
        let source = PolicySource::new(
            Box::new(source()),
            OnReadError::ZeroFill,
            Arc::clone(&regions),
        );
        let mut buf = vec![1u8; READ_ERROR_SECTOR as usize];
        assert_eq!(
            source.read_at(READ_ERROR_SECTOR * 2, &mut buf).unwrap(),
            buf.len()
        );
        assert!(buf.iter().all(|b| *b == 0));
        let regions = regions.lock().unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].offset, READ_ERROR_SECTOR * 2);
        assert_eq!(regions[0].len, READ_ERROR_SECTOR);
    }

    #[test]
    fn raw_read_error_policy_abort_records_the_region() {
        let regions: RegionLog = Arc::new(Mutex::new(Vec::new()));
        let source =
            PolicySource::new(Box::new(source()), OnReadError::Abort, Arc::clone(&regions));
        let mut buf = vec![0u8; READ_ERROR_SECTOR as usize];
        assert!(source.read_at(READ_ERROR_SECTOR * 2, &mut buf).is_err());
        let regions = regions.lock().unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].offset, READ_ERROR_SECTOR * 2);
        assert_eq!(regions[0].len, READ_ERROR_SECTOR);
    }
}
