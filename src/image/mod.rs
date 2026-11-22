//! Additional read-only image sources.
//!
//! CarveX historically read a single raw file.  This module keeps that path
//! byte-for-byte identical while adding the evidence containers requested by
//! B-04.  Every implementation is an [`ImageSource`], so filesystem detection
//! and carving never need to know which container was opened.

mod e01;
mod split;
mod vhd;
mod vmdk;

/// Hidden fixture builders shared by fuzz targets and parser property tests.
#[doc(hidden)]
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
    pub offset: u64,
    pub len: u64,
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
    /// VMware virtual disk descriptor with flat or sparse extents.
    Vmdk,
    /// Read the image from standard input into a temporary spool file.
    Stdin,
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
    size: u64,
    mtime: Option<u64>,
}

impl SourceIdentity {
    /// Observe all backing files before the scan starts.
    pub fn observe(paths: Vec<PathBuf>) -> Result<Self> {
        if paths.is_empty() {
            bail!("image source has no backing files");
        }
        let (size, mtime, ok) = measure(&paths);
        if !ok {
            bail!("cannot stat every file backing the image source");
        }
        Ok(Self { paths, size, mtime })
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

/// An opened image plus the metadata the CLI needs for reporting.
///
/// The temporary file guard is kept alive for stdin even though the scanner
/// only sees the source trait object; dropping it early would unlink the spool
/// underneath an open file handle on some platforms.
pub struct OpenedImage {
    pub source: Box<dyn ImageSource>,
    pub len: u64,
    pub display: String,
    pub identity: Option<SourceIdentity>,
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
    pub allow_external_extents: bool,
    pub on_read_error: OnReadError,
    /// Maximum bytes spooled from stdin; 0 means unlimited.
    pub max_stdin_bytes: u64,
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
        InputFormat::Raw => (Box::new(probe), len, vec![path.to_path_buf()]),
        InputFormat::Split => {
            let image = SplitRaw::open(path)?;
            let len = image.len();
            let paths = image.source_paths();
            (Box::new(image), len, paths)
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
    let actual = u32::from_be_bytes([footer[64], footer[65], footer[66], footer[67]]);
    expected == actual
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
        if &head[..4] == b"KDMV" {
            return Ok(InputFormat::Vmdk);
        }
    }

    if len >= 512 {
        let footer = read_vec_at(probe, len - 512, 512)?;
        if &footer[..8] == b"conectix" && vhd_footer_checksum_valid(&footer) {
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
