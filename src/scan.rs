use crate::fs::{
    self as fsx, FsInfo, MetadataLimits, MetadataPieceKind, VolumeCtx, DEFAULT_METADATA_LIMITS,
};
use crate::io::{read_exact_at, ImageSource, RegionLog, UnreadableRegion};
use crate::signatures::png::{
    png_chunk_header_plausible, png_first_crc_gap, png_split_chunk_crc_ok, PngCrcGap,
};
use crate::signatures::{
    analyze_with_limits, base_validity_checks, build_plan, validate_deep_detail, validate_repair,
    Analysis, FileType, Need, PdfEofMode, Status, ValidatedRepair, Validation, ValidationCheck,
    ValidationDetail, ValidationLimits, ValidationMode, Validity,
};
use crate::util::{
    complement_extents, hex, human_size, merge_extents, sha256_hex, sha256_hex_with_suffix,
    unix_timestamp, Extent,
};
use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use crc32fast::Hasher;
use rayon::prelude::*;
use serde::Serialize;
use sha2::Digest;
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{IsTerminal, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

type WorkLimit = (u64, FileType, u64, u64);
/// Offset, type and score of a candidate withheld by `--min-score` (H-02).
type LowScored = (u64, FileType, u8);

/// Test-only panic injection target. Production never sets this; it exists so
/// panic containment can be exercised from integration tests and the Python
/// repro harness without adding a visible CLI flag.
#[doc(hidden)]
pub static TEST_PANIC_AT_OFFSET: AtomicU64 = AtomicU64::new(u64::MAX);

/// Enable the hidden panic seam for one candidate offset. Passing `u64::MAX`
/// disables it.
#[doc(hidden)]
pub fn set_test_panic_at_offset(offset: u64) {
    TEST_PANIC_AT_OFFSET.store(offset, Ordering::SeqCst);
}

fn test_panic_probe(offset: u64, stage: &str) {
    if TEST_PANIC_AT_OFFSET.load(Ordering::SeqCst) == offset {
        panic!("test-only panic injected at offset {offset} during {stage}");
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

#[derive(Debug, Default)]
struct InterruptInner {
    requested: AtomicBool,
    signals: AtomicU8,
}

/// Cooperative SIGINT/Ctrl-C state for one scan (H-28).
///
/// The first signal latches `requested`; every scanner stage polls it and
/// stops taking new work. A second signal exits immediately with status 130,
/// so an operator can still force termination if a stage cannot stop quickly.
#[derive(Debug, Clone, Default)]
pub struct Interrupt {
    inner: Arc<InterruptInner>,
}

impl Interrupt {
    fn new() -> Self {
        Self::default()
    }

    fn request(&self) {
        let previous = self.inner.signals.fetch_add(1, Ordering::SeqCst);
        self.inner.requested.store(true, Ordering::SeqCst);
        if previous >= 1 {
            std::process::exit(130);
        }
    }

    /// Method.
    pub fn requested(&self) -> bool {
        self.inner.requested.load(Ordering::SeqCst)
    }

    fn reset(&self) {
        self.inner.requested.store(false, Ordering::SeqCst);
        self.inner.signals.store(0, Ordering::SeqCst);
    }
}

static INTERRUPT_STATE: OnceLock<Interrupt> = OnceLock::new();
static HANDLER_INSTALLED: Mutex<bool> = Mutex::new(false);

/// Install the process-wide interrupt handler and return this run's state.
///
/// Repeated calls reuse the same state, which keeps library and integration
/// tests from attempting to register a second OS handler. The state is reset
/// for each call because each CLI invocation is a fresh scan.
pub fn install_interrupt_handler() -> Result<Interrupt> {
    let interrupt = INTERRUPT_STATE.get_or_init(Interrupt::new).clone();
    interrupt.reset();
    let Ok(mut installed) = HANDLER_INSTALLED.lock() else {
        bail!("cannot install interrupt handler: handler state is poisoned")
    };
    if !*installed {
        let handler = interrupt.clone();
        ctrlc::set_handler(move || handler.request())
            .context("cannot install SIGINT/Ctrl-C handler")?;
        *installed = true;
    }
    Ok(interrupt)
}

/// A contained panic in one candidate or fragment-join attempt.
#[derive(Debug, Clone, Serialize)]
pub struct InternalError {
    /// Byte offset in the source image.
    pub offset: u64,
    /// Detected file type.
    #[serde(rename = "type")]
    pub file_type: FileType,
    /// Human-readable message.
    pub message: String,
}

/// Report schema version emitted by this build.
pub const SCHEMA_VERSION: u8 = 2;

/// Default bound for B-06 gap carving, in volume clusters.
pub const DEFAULT_MAX_GAP_CLUSTERS: u64 = 16;
/// Default bound on candidate joins evaluated by B-06.
pub const DEFAULT_MAX_FRAG_CANDIDATES: usize = 1024;
/// Default wall-clock budget for B-06 (milliseconds).
pub const DEFAULT_FRAG_TIME_LIMIT_MS: u64 = 5_000;
/// Default hard limit on header candidates evaluated in one run.
pub const DEFAULT_MAX_CANDIDATES: u64 = 1_000_000;
/// Default work limit for one candidate (analyzer + validator bytes).
pub const DEFAULT_MAX_WORK_PER_CANDIDATE: u64 = 256 * 1024 * 1024;
/// Default cap on bytes written by one scan.
pub const DEFAULT_MAX_OUTPUT: u64 = 4 * 1024 * 1024 * 1024;
/// Default overall scan wall-clock budget in seconds (0 = unlimited).
pub const DEFAULT_TIME_BUDGET_SECS: u64 = 0;
/// Default cap on detailed `regions[]` rows (0 = unlimited; H-27).
pub const DEFAULT_MAX_REGIONS: u64 = 4096;

/// H-12b: maximum number of lazy tasks materialised at once. Counting, quota
/// assignment and scanning stay in task order, so batching cannot change the
/// deterministic candidate prefix or the global candidate budget.
const SCAN_TASK_BATCH: usize = 64;

fn effective_max_candidates(opts: &ScanOptions) -> u64 {
    if opts.max_candidates == 0 {
        DEFAULT_MAX_CANDIDATES
    } else {
        opts.max_candidates
    }
}

fn effective_max_work(opts: &ScanOptions) -> u64 {
    if opts.max_work_per_candidate == 0 {
        u64::MAX
    } else {
        opts.max_work_per_candidate
    }
}

fn effective_max_output(opts: &ScanOptions) -> u64 {
    if opts.max_output == 0 {
        u64::MAX
    } else {
        opts.max_output
    }
}

/// Wall-clock safety net. Expiry is latched so every stage sees the same stop
/// decision; `--time-budget` results are flagged as nondeterministic.
#[derive(Debug, Clone)]
struct TimeBudget {
    started: Instant,
    limit: Duration,
    expired: Arc<AtomicBool>,
}

impl TimeBudget {
    fn new(started: Instant, limit: Duration) -> Self {
        Self {
            started,
            limit,
            expired: Arc::new(AtomicBool::new(false)),
        }
    }

    fn check(&self) -> bool {
        if !self.limit.is_zero()
            && !self.expired.load(Ordering::Relaxed)
            && self.started.elapsed() > self.limit
        {
            self.expired.store(true, Ordering::Relaxed);
        }
        self.expired.load(Ordering::Relaxed)
    }

    fn expired(&self) -> bool {
        self.expired.load(Ordering::Relaxed)
    }
}

/// Combined stop controller for the optional wall-clock budget and SIGINT.
/// Work-unit limits remain separate so their results stay deterministic.
#[derive(Debug, Clone)]
struct Stop {
    time: TimeBudget,
    interrupt: Option<Interrupt>,
}

impl Stop {
    fn new(started: Instant, limit: Duration, interrupt: Option<Interrupt>) -> Self {
        Self {
            time: TimeBudget::new(started, limit),
            interrupt,
        }
    }

    fn check(&self) -> bool {
        self.time.check() || self.interrupted()
    }

    fn interrupted(&self) -> bool {
        self.interrupt.as_ref().is_some_and(Interrupt::requested)
    }

    fn expired(&self) -> bool {
        self.time.expired()
    }
}

/// Bytes examined by one candidate's analyzers and validators.
#[derive(Debug)]
struct WorkCounter {
    limit: u64,
    used: u64,
}

impl WorkCounter {
    fn new(limit: u64) -> Self {
        Self { limit, used: 0 }
    }

    fn add(&mut self, bytes: u64) -> bool {
        if bytes > self.limit.saturating_sub(self.used) {
            false
        } else {
            self.used = self.used.saturating_add(bytes);
            true
        }
    }

    fn used(&self) -> u64 {
        self.used
    }
}

/// Shared byte budget for concurrent working buffers.
///
/// H-08 changed `--max-memory` from a post-hoc cap on retained output into a
/// blocking permit pool. A request larger than the whole budget fails
/// immediately (a deterministic per-candidate skip); otherwise the caller
/// waits until enough permits are available. Threads waiting for growth first
/// release and drop their old buffer, so a worker holding no permit can always
/// make progress.
#[derive(Debug)]
struct MemoryBudget {
    limit: u64,
    used: Mutex<u64>,
    changed: Condvar,
}

impl MemoryBudget {
    fn new(limit: u64) -> Self {
        Self {
            limit,
            used: Mutex::new(0),
            changed: Condvar::new(),
        }
    }

    fn limit(&self) -> u64 {
        self.limit
    }

    fn reserve(&self, bytes: u64, what: &str) -> Result<()> {
        if bytes > self.limit {
            bail!(
                "{what} requires {} which exceeds --max-memory {}",
                human_size(bytes),
                human_size(self.limit)
            );
        }
        if bytes == 0 {
            return Ok(());
        }
        let mut used = self
            .used
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            let Some(next) = used.checked_add(bytes) else {
                bail!("{what} would overflow the --max-memory accounting");
            };
            if next <= self.limit {
                *used = next;
                drop(used);
                self.changed.notify_all();
                return Ok(());
            }
            used = self
                .changed
                .wait(used)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn release(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        {
            let mut used = self
                .used
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *used = used.saturating_sub(bytes);
        }
        self.changed.notify_all();
    }
}

/// RAII reservation against a [`MemoryBudget`].
#[derive(Debug)]
struct BudgetGuard<'a> {
    budget: &'a MemoryBudget,
    bytes: u64,
}

impl<'a> BudgetGuard<'a> {
    fn reserve(budget: &'a MemoryBudget, bytes: u64, what: &str) -> Result<Self> {
        budget.reserve(bytes, what)?;
        Ok(Self { budget, bytes })
    }

    fn release(&mut self) {
        if self.bytes != 0 {
            self.budget.release(self.bytes);
            self.bytes = 0;
        }
    }
}

impl Drop for BudgetGuard<'_> {
    fn drop(&mut self) {
        if self.bytes != 0 {
            self.budget.release(self.bytes);
        }
    }
}

/// The `ScanMode` enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanMode {
    /// The `Auto` variant of `ScanMode`.
    Auto,
    /// The `Unallocated` variant of `ScanMode`.
    Unallocated,
    /// The `Raw` variant of `ScanMode`.
    Raw,
    /// The `Allocated` variant of `ScanMode`.
    Allocated,
}

impl ScanMode {
    /// Method.
    pub fn as_str(self) -> &'static str {
        match self {
            ScanMode::Auto => "auto",
            ScanMode::Unallocated => "unallocated",
            ScanMode::Raw => "raw",
            ScanMode::Allocated => "allocated",
        }
    }
}

/// Where a candidate header may start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AlignMode {
    /// Preserve the historical behaviour: every byte is a candidate.
    #[default]
    None,
    /// Cluster boundary for FS-aware modes, sector boundary for raw.
    Auto,
    /// Cluster boundary relative to the volume start.
    Cluster,
    /// 512-byte sector boundary.
    Sector,
}

/// Configuration/data structure.
#[derive(Clone)]
pub struct ScanOptions {
    /// File types enabled for this scan.
    pub types: Vec<FileType>,
    /// Scan mode (raw or filesystem-aware).
    pub mode: ScanMode,
    /// Minimum carved file size in bytes.
    pub min_size: u64,
    /// Maximum carved file size in bytes.
    pub max_size: u64,
    /// Maximum size for truncated/partial carvings in bytes.
    pub max_truncated_size: u64,
    /// Maximum working memory budget in bytes.
    pub max_memory: u64,
    /// Maximum working memory for one decode validator
    /// (0 = the default validator bound).  The legacy report field
    /// `max_decode_bytes` mirrors this value.
    pub max_validator_memory: u64,
    /// Scan chunk size in bytes.
    pub chunk_size: u64,
    /// Number of worker threads (0 = auto).
    pub threads: usize,
    /// Minimum heuristic score to emit.
    pub min_score: u8,
    /// Whether repair attempts are enabled.
    pub repair: bool,
    /// Emit only complete files when true.
    pub complete_only: bool,
    /// Emit only files whose authoritative validity is verified.
    pub verified_only: bool,
    /// PDF EOF handling mode.
    pub pdf_eof: PdfEofMode,
    /// Whether to dedupe by content hash.
    pub dedupe_hashes: bool,
    /// Whether to suppress nested carvings.
    pub suppress_nested: bool,
    /// Validation mode.
    pub validate: ValidationMode,
    /// Alignment requirement in bytes.
    pub align: AlignMode,
    /// Overwrite existing output when true.
    pub force: bool,
    /// Suppress non-essential console output.
    pub quiet: bool,
    /// Enable verbose console output.
    pub verbose: bool,
    /// Manifest output path, if requested.
    pub manifest: bool,
    /// Partition selection in effect (`largest`, `all` or a 1-based number).
    pub partition: String,
    /// Whether `--include-gaps` added the out-of-partition regions.
    pub include_gaps: bool,
    /// Hard limit on header candidates evaluated (0 = use the default limit).
    pub max_candidates: u64,
    /// Analyzer + validator bytes allowed for one candidate (0 = unlimited).
    pub max_work_per_candidate: u64,
    /// Cap on bytes written by one scan (0 = unlimited).
    pub max_output: u64,
    /// Overall scan wall-clock budget (0 = unlimited).
    pub time_budget: Duration,
    /// Cap on detailed `regions[]` rows; summarisation preserves coverage and
    /// scanned-byte accounting (0 = unlimited; H-27).
    pub max_regions: u64,
    /// Opt in to bounded bifragment gap carving (B-06).
    pub fragment_recovery: bool,
    /// Largest permitted gap between two joined fragments, in clusters.
    pub max_gap: u64,
    /// Maximum number of candidate joins evaluated during a scan.
    pub max_frag_candidates: usize,
    /// Wall-clock budget for the fragment-recovery stage.
    pub frag_time_limit: Duration,
    /// H-27: opt in to a second pass over one volume's free-space stream.
    pub free_stream: bool,
    /// H-28/H-29: opt in to deleted filesystem metadata recovery.
    pub metadata: bool,
}

impl ScanOptions {
    /// Serialisable snapshot of the effective options for the report.
    pub fn report_options(&self) -> ReportOptions {
        let max_candidates = effective_max_candidates(self);
        let limits = ValidationLimits::new(self.max_validator_memory);
        ReportOptions {
            types: self.types.iter().map(|t| t.ext().to_string()).collect(),
            mode: self.mode.as_str().to_string(),
            min_size: self.min_size,
            max_size: self.max_size,
            max_truncated_size: self.max_truncated_size,
            max_memory: self.max_memory,
            max_decode_bytes: limits.max_png_decoded_bytes,
            max_validator_memory: limits.max_validator_memory,
            chunk_size: self.chunk_size,
            threads: self.threads,
            min_score: self.min_score,
            repair: self.repair,
            complete_only: self.complete_only,
            verified_only: self.verified_only,
            pdf_eof: self.pdf_eof.to_string(),
            dedupe_hashes: self.dedupe_hashes,
            suppress_nested: self.suppress_nested,
            validate: self.validate,
            align: self.align,
            partition: self.partition.clone(),
            include_gaps: self.include_gaps,
            max_candidates,
            max_work_per_candidate: self.max_work_per_candidate,
            max_output: self.max_output,
            time_budget_ms: self.time_budget.as_millis() as u64,
            max_regions: self.max_regions,
            // B-06 fields are omitted entirely when the stage is disabled so
            // the default report remains byte-for-byte compatible.
            fragment_recovery: self.fragment_recovery.then_some(true),
            max_gap: self.fragment_recovery.then_some(self.max_gap),
            max_frag_candidates: self
                .fragment_recovery
                .then_some(self.max_frag_candidates as u64),
            frag_time_limit_ms: self
                .fragment_recovery
                .then_some(self.frag_time_limit.as_millis() as u64),
            free_stream: self.free_stream.then_some(true),
            metadata: self.metadata.then_some(true),
        }
    }
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            types: FileType::all().to_vec(),
            mode: ScanMode::Auto,
            min_size: 256,
            max_size: 64 * 1024 * 1024,
            max_truncated_size: 16 * 1024 * 1024,
            max_memory: 1024 * 1024 * 1024,
            max_validator_memory: ValidationLimits::default().max_validator_memory,
            chunk_size: 64 * 1024,
            threads: 0,
            min_score: 50,
            repair: false,
            complete_only: false,
            verified_only: false,
            pdf_eof: PdfEofMode::Smart,
            dedupe_hashes: false,
            suppress_nested: true,
            validate: ValidationMode::Structure,
            align: AlignMode::None,
            force: false,
            quiet: true,
            verbose: false,
            manifest: false,
            partition: "largest".to_string(),
            include_gaps: false,
            max_candidates: DEFAULT_MAX_CANDIDATES,
            max_work_per_candidate: DEFAULT_MAX_WORK_PER_CANDIDATE,
            max_output: DEFAULT_MAX_OUTPUT,
            time_budget: Duration::from_secs(DEFAULT_TIME_BUDGET_SECS),
            max_regions: DEFAULT_MAX_REGIONS,
            fragment_recovery: true,
            max_gap: DEFAULT_MAX_GAP_CLUSTERS,
            max_frag_candidates: DEFAULT_MAX_FRAG_CANDIDATES,
            frag_time_limit: Duration::from_millis(DEFAULT_FRAG_TIME_LIMIT_MS),
            free_stream: false,
            metadata: false,
        }
    }
}

/// Hidden fuzz entry point for one B-06 join attempt. It derives bounded
/// candidate offsets from the fuzz bytes and calls the production join path
/// directly; unlike the scan pipeline it deliberately does not catch panics so
/// libFuzzer can report parser defects.
#[doc(hidden)]
pub fn fuzz_fragment_join(data: &[u8]) {
    use crate::image::fuzz_support::{bounded_u64, fragment_fuzz, MemSource};

    let data = fragment_fuzz(data);
    let source = MemSource::new(data.clone());
    let len = source.len();
    if len == 0 {
        return;
    }
    let opts = ScanOptions {
        types: FileType::all().to_vec(),
        fragment_recovery: true,
        max_size: data.len() as u64,
        max_work_per_candidate: 1024 * 1024,
        max_memory: 16 * 1024 * 1024,
        ..ScanOptions::default()
    };
    let budget = Arc::new(MemoryBudget::new(opts.max_memory));

    for (type_index, &kind) in FileType::all().iter().enumerate() {
        let candidate = Candidate {
            offset: 0,
            extent_end: len,
            extent_id: type_index,
            kind,
        };
        for attempt in 0..4usize {
            let base = 64 + (type_index * 4 + attempt) * 32;
            let cut = bounded_u64(fuzz_u64_at(&data, base), 1, len);
            let tail_start = bounded_u64(fuzz_u64_at(&data, base + 8), cut, len);
            let footer_end = bounded_u64(fuzz_u64_at(&data, base + 16), tail_start, len);
            let gap_clusters = fuzz_u64_at(&data, base + 24) % (DEFAULT_MAX_GAP_CLUSTERS + 1);
            let join = FragmentJoin {
                cut,
                tail_start,
                footer_end,
                gap_clusters,
                gap_matches: 0,
                png_oracle: (fuzz_u64_at(&data, base + 28) & 1) == 1,
            };
            let _ = try_fragment_join(&source, &candidate, &join, &opts, &budget);
        }
    }
}

/// Hidden fuzz entry point for the H-30 PNG CRC oracle helpers.
#[doc(hidden)]
pub fn fuzz_png_crc_oracle(data: &[u8]) {
    use crate::image::fuzz_support::fragment_fuzz;

    let data = fragment_fuzz(data);
    let Some(gap) = png_first_crc_gap(&data, data.len()) else {
        let _ = png_chunk_header_plausible(&data);
        return;
    };
    match gap {
        PngCrcGap::Broken {
            ctype,
            data_start,
            chunk_len,
            stored_crc,
            ..
        } => {
            let end = data_start.saturating_add(chunk_len).min(data.len());
            let prefix = data.get(data_start..end).unwrap_or(&[]);
            let stored = stored_crc.unwrap_or_else(|| fuzz_u64_at(&data, 0) as u32);
            let _ = png_split_chunk_crc_ok(&ctype, prefix, &[], stored);
            let _ = png_split_chunk_crc_ok(&ctype, &[], prefix, stored);
        }
        PngCrcGap::Boundary { last_chunk_end } => {
            let _ = png_chunk_header_plausible(data.get(last_chunk_end..).unwrap_or(&[]));
        }
    }
}

fn fuzz_u64_at(data: &[u8], offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = data.get(offset + i).copied().unwrap_or(0);
    }
    u64::from_le_bytes(bytes)
}

fn build_input_info(
    image: &str,
    image_len: u64,
    image_identity: Option<&crate::io::SourceIdentity>,
    ctx: &ScanContext,
) -> InputInfo {
    let (size_after, modified_unix_after, readable) = match image_identity {
        Some(identity) => identity.current(),
        // Stdin has no backing path; the spool is controlled by this process.
        None => (image_len, ctx.image_mtime, true),
    };
    let physical_size = image_identity.map_or(
        ctx.input_physical_size.unwrap_or(image_len),
        crate::io::SourceIdentity::initial_size,
    );
    let physical_size_after = if image_identity.is_some() {
        size_after
    } else {
        physical_size
    };
    // Container logical capacity is fixed when the image is opened. Raw and
    // split sources have no separate physical layer, so their logical size
    // follows the backing files if those change during the run.
    let logical_size_after = if matches!(ctx.input_format.as_str(), "raw" | "split" | "stdin") {
        physical_size_after
    } else {
        image_len
    };
    let unchanged = image_identity.map(|identity| {
        readable
            && size_after == identity.initial_size()
            && modified_unix_after == identity.initial_mtime()
    });
    InputInfo {
        path: image.to_string(),
        size: image_len,
        sha256: ctx.image_sha256.clone(),
        modified_unix: ctx.image_mtime,
        size_after,
        modified_unix_after,
        unchanged,
        logical_size: image_len,
        logical_size_after,
        physical_size,
        physical_size_after,
        format: ctx.input_format.clone(),
        files: image_identity.map_or_else(Vec::new, |identity| identity.files().to_vec()),
    }
}

/// Run-specific metadata that is not a scanning option.
#[derive(Debug, Clone)]
pub struct ScanContext {
    /// Command-line arguments used for the run.
    pub argv: Vec<String>,
    /// Case identifier, if provided.
    pub case_id: Option<String>,
    /// Examiner name, if provided.
    pub examiner: Option<String>,
    /// SHA-256 of the input image before scan.
    pub image_sha256: Option<String>,
    /// Input image mtime (Unix seconds) before scan.
    pub image_mtime: Option<u64>,
    /// Resolved container format emitted in `input.format`.
    pub input_format: String,
    /// Physical backing size for sources without a `SourceIdentity` (stdin).
    pub input_physical_size: Option<u64>,
}

impl Default for ScanContext {
    fn default() -> Self {
        Self {
            argv: Vec::new(),
            case_id: None,
            examiner: None,
            image_sha256: None,
            image_mtime: None,
            input_format: "raw".into(),
            input_physical_size: None,
        }
    }
}

/// Configuration/data structure.
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    /// Name.
    pub name: String,
    /// Version string.
    pub version: String,
    /// Source commit hash, when known.
    pub commit: String,
    /// Rust compiler version used to build this binary.
    pub rustc: String,
}

/// Configuration/data structure.
#[derive(Debug, Clone, Serialize)]
pub struct InputInfo {
    /// Path on disk.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// SHA-256 digest.
    pub sha256: Option<String>,
    /// Modification time as Unix seconds.
    pub modified_unix: Option<u64>,
    /// Input size after scan, when rechecked.
    pub size_after: u64,
    /// Input mtime after scan, when rechecked.
    pub modified_unix_after: Option<u64>,
    /// Unknown for stdin, which has no stable backing path (H-25).
    pub unchanged: Option<bool>,
    /// Logical image bytes exposed by the container (H-25).
    pub logical_size: u64,
    /// Logical input size after scan, when rechecked.
    pub logical_size_after: u64,
    /// Sum of the physical bytes in the backing files (H-25).
    pub physical_size: u64,
    /// Physical input size after scan, when rechecked.
    pub physical_size_after: u64,
    /// Resolved container format, never `auto` (H-25).
    pub format: String,
    /// Backing files with their observed sizes and mtimes (H-25).
    pub files: Vec<crate::io::SourceFile>,
}

/// Configuration/data structure.
#[derive(Debug, Clone, Serialize)]
pub struct ReportOptions {
    /// File types enabled for this scan.
    pub types: Vec<String>,
    /// Scan mode (raw or filesystem-aware).
    pub mode: String,
    /// Minimum carved file size in bytes.
    pub min_size: u64,
    /// Maximum carved file size in bytes.
    pub max_size: u64,
    /// Maximum size for truncated/partial carvings in bytes.
    pub max_truncated_size: u64,
    /// Maximum working memory budget in bytes.
    pub max_memory: u64,
    /// Maximum bytes decoded by validators.
    pub max_decode_bytes: u64,
    /// H-05: authoritative name for the shared decode-validator budget.
    pub max_validator_memory: u64,
    /// Scan chunk size in bytes.
    pub chunk_size: u64,
    /// Number of worker threads (0 = auto).
    pub threads: usize,
    /// Minimum heuristic score to emit.
    pub min_score: u8,
    /// Whether repair attempts are enabled.
    pub repair: bool,
    /// Emit only complete files when true.
    pub complete_only: bool,
    /// Emit only files whose authoritative validity is verified.
    pub verified_only: bool,
    /// PDF EOF handling mode.
    pub pdf_eof: String,
    /// Whether to dedupe by content hash.
    pub dedupe_hashes: bool,
    /// Whether to suppress nested carvings.
    pub suppress_nested: bool,
    /// Validation mode.
    pub validate: ValidationMode,
    /// Alignment requirement in bytes.
    pub align: AlignMode,
    /// Partition selection mode.
    pub partition: String,
    /// Whether to include gaps between partitions.
    pub include_gaps: bool,
    /// Maximum number of candidates to process.
    pub max_candidates: u64,
    /// Maximum work units per candidate.
    pub max_work_per_candidate: u64,
    /// Maximum total output bytes.
    pub max_output: u64,
    /// Wall-clock time budget in milliseconds.
    pub time_budget_ms: u64,
    /// Effective `regions[]` detail cap (0 = unlimited; H-27).
    pub max_regions: u64,
    /// Whether fragment recovery is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment_recovery: Option<bool>,
    /// Maximum fragment gap in bytes/clusters as configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_gap: Option<u64>,
    /// Maximum fragment candidates to consider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_frag_candidates: Option<u64>,
    /// Fragment recovery time limit in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frag_time_limit_ms: Option<u64>,
    /// H-27 free-space stream pass was enabled (omitted otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_stream: Option<bool>,
    /// H-28/H-29 deleted-metadata pass was enabled (omitted otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<bool>,
}

/// Configuration/data structure.
#[derive(Debug, Clone, Serialize)]
pub struct Fragment {
    /// Byte offset in the source image.
    pub offset: u64,
    /// Length in bytes.
    pub len: u64,
}

/// A structured report warning: a stable `code` plus a human message.
///
/// Codes that mean "the run did not read part of the image" make the CLI
/// exit with code 3 (finished but incomplete, see `docs/DECISIONS.md`).
#[derive(Debug, Clone, Serialize)]
pub struct Warning {
    /// Warning code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Detected file type when the warning concerns one candidate (H-25).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub file_type: Option<FileType>,
    /// Source offset of the candidate when applicable (H-25).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// Candidate size when applicable (H-25).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

impl Warning {
    /// The `new` associated function.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            file_type: None,
            offset: None,
            size: None,
        }
    }

    /// A warning about one specific truncated candidate (H-25).
    pub fn for_candidate(
        code: &str,
        message: impl Into<String>,
        file_type: FileType,
        offset: u64,
        size: u64,
    ) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            file_type: Some(file_type),
            offset: Some(offset),
            size: Some(size),
        }
    }

    /// True when the warning means evidence was left unscanned or the run was
    /// degraded; such runs exit with code 3.
    pub fn incomplete(&self) -> bool {
        matches!(
            self.code.as_str(),
            "VOLUME_NOT_SCANNED"
                | "GAP_NOT_SCANNED"
                | "VOLUME_UNSUPPORTED"
                | "VOLUME_PARSE_FAILED"
                | "FS_DETECTION_FAILED"
                | "IMAGE_MODIFIED"
                | "UNREADABLE_REGION"
                | "READ_ABORTED"
                | "CANDIDATE_SKIPPED_UNREADABLE"
                | "CANDIDATE_SKIPPED_BUDGET"
                | "SOURCE_CHANGED"
                | "CANDIDATE_LIMIT"
                | "FRAGMENT_CANDIDATE_LIMIT"
                | "WORK_LIMIT"
                | "TIME_LIMIT"
                | "FRAGMENT_TIME_LIMIT"
                | "OUTPUT_LIMIT"
                | "TRUNCATED_TOO_LARGE"
                | "CANDIDATE_PANIC"
                | "METADATA_TIME_LIMIT"
                | "METADATA_PARSE_FAILED"
                | "METADATA_MEMORY_LIMIT"
                | "METADATA_CANDIDATE_LIMIT"
        )
    }
}

/// Compact summary of one detected volume in `volumes[]` (A-07).
#[derive(Debug, Clone, Serialize)]
pub struct VolumeSummary {
    /// 1-based position among the detected volumes.
    pub index: usize,
    /// Filesystem kind/name.
    pub fs: String,
    /// Byte offset in the source image.
    pub offset: u64,
    /// Length in bytes.
    pub length: u64,
    /// Cluster size in bytes.
    pub cluster_size: u64,
    /// Free bytes in the volume.
    pub free_bytes: u64,
    /// Partition index, when applicable.
    pub partition_index: Option<usize>,
    /// Whether this run reads the volume.
    pub scanned: bool,
}

impl VolumeSummary {
    /// The `from_fs` associated function.
    pub fn from_fs(index: usize, fs: &FsInfo, scanned: bool) -> Self {
        Self {
            index,
            fs: fs.kind.to_string(),
            offset: fs.offset,
            length: fs.length,
            cluster_size: fs.cluster_size,
            free_bytes: fs.free_bytes(),
            partition_index: fs.partition_index,
            scanned,
        }
    }
}

/// H-30: provenance for a PNG CRC-guided two-fragment join.
#[derive(Debug, Clone, Serialize)]
pub struct JoinEvidence {
    /// Oracle used to find the gap. Currently always `png_chunk_crc`.
    pub method: String,
    /// Physical cluster boundary where the head fragment ends.
    pub cut: u64,
    /// Physical cluster boundary where the tail fragment starts.
    pub tail_start: u64,
    /// Number of clusters skipped between the two fragments.
    pub gap_clusters: u64,
    /// False when more than one `(cut, gap)` deep-validated for this head.
    pub unique: bool,
}

/// H-30: one deep-validated `(cut, gap)` solution for an ambiguous head.
#[derive(Debug, Clone, Serialize)]
pub struct JoinAlternative {
    /// Physical cluster boundary where the head fragment ends.
    pub cut: u64,
    /// Physical cluster boundary where the tail fragment starts.
    pub tail_start: u64,
    /// Number of clusters skipped between the two fragments.
    pub gap_clusters: u64,
    /// SHA-256 of this deep-validated reconstruction.
    pub sha256: String,
    /// Honesty label of this deep-validated reconstruction.
    pub validity: Validity,
}

/// Configuration/data structure.
#[derive(Debug, Clone, Serialize)]
pub struct CarvedFile {
    /// Sequence number in scan output.
    pub seq: u64,
    /// Detected file type.
    #[serde(rename = "type")]
    pub file_type: FileType,
    /// Byte offset in the source image.
    pub offset: u64,
    /// Size in bytes.
    pub size: u64,
    /// Bytes carved/written for this file.
    pub carved_size: u64,
    /// SHA-256 digest.
    pub sha256: String,
    /// Legacy status label.
    pub status: Status,
    /// Heuristic confidence score (0-100).
    pub score: u8,
    /// Width in pixels, when known.
    pub width: Option<u32>,
    /// Height in pixels, when known.
    pub height: Option<u32>,
    /// Whether repair was applied.
    pub repaired: bool,
    /// Path on disk.
    pub path: String,
    /// Human-readable notes.
    pub notes: Vec<String>,
    /// Legacy validation outcome.
    pub validation: Validation,
    /// Authoritative honesty label; legacy `validation`/`status` are preserved.
    pub validity: Validity,
    /// Short machine-readable reason for `validity`.
    pub validity_reason: String,
    /// Trailing zero bytes removed from a salvaged prefix.
    pub tail_trimmed_bytes: u64,
    /// Whether the candidate offset satisfied the effective alignment mode.
    pub aligned: bool,
    /// Sequence of the containing file when nested output is retained.
    pub nested_in: Option<u64>,
    /// Final output digest when repair changed the carved bytes.
    pub sha256_output: Option<String>,
    /// True only when a repair passed the deep validator and changed the output.
    pub repair_validated: bool,
    /// Rows retained by a validated PNG/JPEG crop; null for other files.
    pub recovered_rows: Option<u32>,
    /// Row count declared by the original PNG/JPEG header; null otherwise.
    pub total_rows: Option<u32>,
    /// Ratio of retained rows to declared rows; null for other files.
    pub recovered_fraction: Option<f64>,
    /// Bytes covered by validation.
    pub validated_bytes: u64,
    /// Logical block address, when known.
    pub lba: u64,
    /// Cluster number, when known.
    pub cluster: Option<u64>,
    /// Partition index, when applicable.
    pub partition_index: Option<usize>,
    /// Extent identifier.
    pub extent_id: usize,
    /// Whether the source region was allocated.
    pub allocated: Option<bool>,
    /// Carving source kind.
    pub source: String,
    /// Fragment ranges composing the carved file.
    pub fragments: Vec<Fragment>,
    /// Structured checks that were actually performed for this file.
    #[serde(rename = "checks")]
    pub validation_checks: Vec<ValidationCheck>,
    /// Number of deep-validation checks made for a B-06 stitch.
    ///
    /// Rust field name is retained for compatibility with existing tests; the
    /// JSON key is `fragment_checks` so `checks` can be the H-16 check array.
    #[serde(rename = "fragment_checks", skip_serializing_if = "Option::is_none")]
    pub checks: Option<u64>,
    /// Confidence in a B-06 stitch (the structural score of the accepted join).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
    /// Number of joins that deep-validated for one B-06 head (H-29).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alternatives: Option<u64>,
    /// H-30: CRC-oracle provenance for a PNG bifragment join.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<JoinEvidence>,
    /// H-30: every deep-validated `(cut, gap)` solution for this head.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join_alternatives: Option<Vec<JoinAlternative>>,
    /// SHA-256 of the bytes read before a repair changed the output.
    /// Absent whenever output bytes are identical to the source bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256_original: Option<String>,
    /// H-28/H-29: recovered display name. Never used to build output paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_name: Option<String>,
    /// H-28/H-29: modified time from the deleted metadata, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<String>,
    /// H-28: true when a FAT file was assumed to occupy contiguous clusters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contiguous_assumed: Option<bool>,
    /// H-28/H-29: number of clusters/runs found overwritten after recovery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overwritten_clusters: Option<u64>,
    /// H-28/H-29: stable notes from the metadata parser and validation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_notes: Option<Vec<String>>,
    /// H-29: zero-filled bytes contributed by sparse runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sparse_bytes: Option<u64>,
}

/// Configuration/data structure.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ScanStats {
    /// Number of candidates examined.
    pub candidates: usize,
    /// Number of files carved.
    pub carved: usize,
    /// Number of candidate or join panics contained during this scan.
    pub internal_errors: usize,
    /// Number of candidates rejected.
    pub rejected: usize,
    /// Candidates rejected below minimum size.
    pub below_min_size: usize,
    /// Candidates rejected below minimum score.
    pub below_min_score: usize,
    /// Candidates skipped by complete-only mode.
    pub skipped_complete_only: usize,
    /// Candidates skipped by verified-only mode.
    pub skipped_verified_only: usize,
    /// Candidates rejected as too-large truncated outputs.
    pub too_large_truncated: usize,
    /// Duplicate outputs suppressed by hash.
    pub duplicate_hashes: usize,
    /// Nested outputs suppressed.
    pub suppressed_nested: usize,
    /// Number of truncated outputs.
    pub truncated: usize,
    /// Whether repair was applied.
    pub repaired: usize,
    /// Total bytes written to output files.
    pub bytes_written: u64,
    /// Number of scanned extents.
    pub extents: usize,
    /// Total bytes in scanned extents.
    pub bytes_in_extents: u64,
}

/// Configuration/data structure.
#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    /// Report schema version; 2 is the current shape.
    pub schema_version: u8,
    /// Tool name (legacy field).
    pub tool: String,
    /// Tool version/build information.
    pub tool_info: ToolInfo,
    /// Input image path (legacy field).
    pub image: String,
    /// Input image metadata.
    pub input: InputInfo,
    /// Input image size in bytes (legacy field).
    pub image_size: u64,
    /// Detected filesystem summary (legacy field).
    pub filesystem: Option<FsInfo>,
    /// Scan mode (raw or filesystem-aware).
    pub mode: String,
    /// Output directory path.
    pub output_dir: String,
    /// Command-line arguments used for the run.
    pub argv: Vec<String>,
    /// Effective scan options.
    pub options: ReportOptions,
    /// Case identifier, if provided.
    pub case_id: Option<String>,
    /// Examiner name, if provided.
    pub examiner: Option<String>,
    /// ISO-8601 UTC timestamp.
    pub started_at: String,
    /// Scan start time as Unix seconds.
    pub started_at_unix: u64,
    /// Scan duration in milliseconds.
    pub duration_ms: u64,
    /// Bytes scanned.
    pub bytes_scanned: u64,
    /// Scan throughput in MiB/s.
    pub throughput_mib_s: f64,
    /// Aggregate scan statistics.
    pub stats: ScanStats,
    /// Every detected volume with a `scanned` flag (A-07).
    pub volumes: Vec<VolumeSummary>,
    /// Top-level image regions with a `scanned` flag (A-07).
    pub regions: Vec<crate::fs::Region>,
    /// Byte ranges that could not be read from the source (H-22).
    pub unreadable_regions: Vec<UnreadableRegion>,
    /// Warnings emitted during scan.
    pub warnings: Vec<Warning>,
    /// Contained panics, with the source offset and file type of the work item
    /// that caused them. Work on other candidates continues.
    pub internal_errors: Vec<InternalError>,
    /// True when a wall-clock budget stopped the scan; work-unit limits do
    /// not set this flag because their results are deterministic.
    pub nondeterministic_stop: bool,
    /// True when SIGINT/Ctrl-C requested a cooperative stop (H-28).
    pub interrupted: bool,
    /// Carved file records.
    pub files: Vec<CarvedFile>,
}

#[derive(Debug, Clone)]
struct Candidate {
    offset: u64,
    extent_end: u64,
    extent_id: usize,
    kind: FileType,
}

#[derive(Debug, Clone)]
struct Task {
    start: u64,
    scan_len: u64,
    read_len: u64,
    extent_end: u64,
    extent_id: usize,
}

struct Progress {
    total: u64,
    done: AtomicU64,
    stop: AtomicBool,
}

impl Progress {
    fn new(total: u64) -> Self {
        Self {
            total,
            done: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        }
    }
}

/// How the write stage reconstructs one output from the read-only source.
#[derive(Debug, Clone)]
enum PayloadSpec {
    /// One contiguous carved range, optionally with a repair suffix appended.
    Range { offset: u64, len: u64 },
    /// A B-06 stitch: read and hash each fragment in order.
    Join { fragments: Vec<Fragment> },
}

impl PayloadSpec {
    fn ranges(&self) -> Vec<(u64, u64)> {
        match self {
            Self::Range { offset, len } => vec![(*offset, *len)],
            Self::Join { fragments } => fragments.iter().map(|f| (f.offset, f.len)).collect(),
        }
    }
}

/// Metadata for one output. H-08: no carved bytes are retained; the write
/// stage re-reads `payload` from the source and verifies the recorded digest.
struct CarvedData {
    candidate: Candidate,
    payload: PayloadSpec,
    output_len: u64,
    analysis: Analysis,
    sha256: String,
    sha256_original: Option<String>,
    repaired: bool,
    /// Dynamic repair bytes appended by the write stage (empty when unchanged).
    repair_suffix: Vec<u8>,
    /// Complete replacement bytes produced by a PNG/JPEG crop repair.
    repair_replacement: Option<Vec<u8>>,
    /// Row coverage of a successful crop repair.
    recovered_rows: Option<u32>,
    total_rows: Option<u32>,
    validation: Validation,
    validity: Validity,
    /// Trailing zero bytes removed from the salvaged prefix.
    tail_trimmed_bytes: u64,
    validation_notes: Vec<String>,
    /// Contiguous bytes from offset zero whose content was actually checked.
    validated_bytes: u64,
    validation_checks: Vec<ValidationCheck>,
    /// Set only for B-06 stitched output (also used for reporting).
    fragments: Vec<Fragment>,
    checks: Option<u64>,
    confidence: Option<u8>,
    alternatives: Option<u64>,
    /// H-30: filled only for a PNG CRC-guided stitch.
    join: Option<JoinEvidence>,
    join_alternatives: Option<Vec<JoinAlternative>>,
}

/// Result of the optional B-06 stage (H-29).
struct FragmentResult {
    files: Vec<CarvedData>,
    /// True only when the wall-clock safety net truncated the stage; the
    /// deterministic work-unit budget does not make a run nondeterministic.
    nondeterministic_stop: bool,
}

enum CarveOutcome {
    Carved(Box<CarvedData>),
    InternalError {
        offset: u64,
        kind: FileType,
        message: String,
    },
    Rejected(String),
    SkippedBudget {
        offset: u64,
        kind: FileType,
        needed: u64,
    },
    WorkLimit {
        offset: u64,
        kind: FileType,
        examined: u64,
        limit: u64,
    },
    TimeLimit,
    TooSmall(u64),
    LowScore {
        offset: u64,
        kind: FileType,
        score: u8,
    },
    NotComplete,
    TooLargeTruncated {
        offset: u64,
        kind: FileType,
        size: u64,
    },
}

/// Inputs describing what to scan and where to write carved files.
pub struct ScanInput<'a> {
    /// Read-only access to the image bytes.
    pub source: &'a dyn ImageSource,
    /// Name used for reporting and display (`-` for stdin).
    pub image: &'a str,
    /// Snapshot of all backing files, when the source has them on disk.
    pub image_identity: Option<&'a crate::io::SourceIdentity>,
    /// Total image size in bytes.
    pub image_len: u64,
    /// Selected filesystem volumes to scan (empty when not filesystem-aware).
    pub volumes: &'a [FsInfo],
    /// Additional raw ranges (e.g. gaps added by `--include-gaps`).
    pub extra_extents: &'a [Extent],
    /// Directory where carved files are written.
    pub out_dir: &'a Path,
    /// Byte ranges reported unreadable by the source reader (H-22).
    pub unreadable_regions: &'a RegionLog,
    /// Drop outputs that intersect an unreadable range instead of zero-filling.
    pub skip_unreadable: bool,
    /// Cooperative interrupt state installed by the CLI (H-28). Library
    /// callers that do not install a handler pass `None`.
    pub interrupt: Option<Interrupt>,
}

fn validate_scan_options(opts: &ScanOptions) -> Result<()> {
    if opts.types.is_empty() {
        bail!("no file types selected");
    }
    if opts.max_size < 64 {
        bail!("--max-size must be at least 64 bytes");
    }
    if opts.max_truncated_size < 64 {
        bail!("--max-truncated-size must be at least 64 bytes");
    }
    if opts.chunk_size < 64 * 1024 {
        bail!("--chunk-size must be at least 64 KiB");
    }
    if opts.max_memory < 16 * 1024 * 1024 {
        bail!("--max-memory must be at least 16 MiB");
    }
    if opts.fragment_recovery {
        if opts.max_gap == 0 {
            bail!("--max-gap must be at least 1 cluster");
        }
        if opts.max_frag_candidates == 0 {
            bail!("--max-frag-candidates must be at least 1");
        }
        if opts.frag_time_limit.is_zero() {
            bail!("--frag-time-limit must be at least 1 millisecond");
        }
    }
    Ok(())
}

fn apply_test_panic_offset() {
    if let Ok(value) = std::env::var("CARVEX_TEST_PANIC_AT_OFFSET") {
        if let Ok(offset) = value.parse::<u64>() {
            set_test_panic_at_offset(offset);
        }
    }
}

struct ScanPlan {
    mode: ScanMode,
    extents: Vec<Extent>,
    bytes_in_extents: u64,
}

fn prepare_scan_plan(
    opts: &ScanOptions,
    volumes: &[FsInfo],
    extra_extents: &[Extent],
    image_len: u64,
) -> Result<ScanPlan> {
    let (mode, extents) = plan_extents(opts.mode, volumes, extra_extents, image_len)?;
    // Extents are disjoint and clamped to `image_len`, so this sum cannot
    // exceed the (u64) image length. It replaces the old per-task sum without
    // materialising a task for every chunk of a declared-size extent.
    let bytes_in_extents = extents
        .iter()
        .fold(0u64, |sum, e| sum.saturating_add(e.len));
    Ok(ScanPlan {
        mode,
        extents,
        bytes_in_extents,
    })
}

/// H-12b: a lazy view of the work implied by resolved extents. No allocation
/// is proportional to a container-declared logical size.
#[derive(Debug, Clone, Copy)]
struct TaskPlan<'a> {
    extents: &'a [Extent],
    chunk_size: u64,
}

/// Shared inputs for the header-candidate discovery workers.
struct CandidateScanCtx<'a> {
    source: &'a dyn ImageSource,
    types: &'a [FileType],
    opts: &'a ScanOptions,
    volumes: &'a [FsInfo],
    stop: &'a Stop,
}

impl<'a> TaskPlan<'a> {
    fn new(extents: &'a [Extent], chunk_size: u64) -> Self {
        Self {
            extents,
            chunk_size,
        }
    }

    fn iter(self) -> TaskIter<'a> {
        TaskIter {
            extents: self.extents,
            extent_id: 0,
            offset: 0,
            end: 0,
            chunk_size: self.chunk_size,
        }
    }
}

/// H-12b: yields one chunk at a time. The caller checks the stop state before
/// each bounded batch, so planning itself remains bounded and cooperative.
struct TaskIter<'a> {
    extents: &'a [Extent],
    extent_id: usize,
    offset: u64,
    end: u64,
    chunk_size: u64,
}

impl Iterator for TaskIter<'_> {
    type Item = Task;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.offset >= self.end {
                let extent = *self.extents.get(self.extent_id)?;
                self.extent_id += 1;
                self.offset = extent.offset;
                self.end = extent.end();
                continue;
            }
            let remaining = self.end - self.offset;
            let scan_len = self.chunk_size.min(remaining);
            let read_len = scan_len.saturating_add(16).min(remaining);
            let task = Task {
                start: self.offset,
                scan_len,
                read_len,
                extent_end: self.end,
                extent_id: self.extent_id - 1,
            };
            self.offset = self.offset.saturating_add(scan_len);
            return Some(task);
        }
    }
}

fn worker_pool(threads: usize) -> Result<(usize, rayon::ThreadPool)> {
    let thread_count = if threads == 0 {
        std::thread::available_parallelism().map_or(4, std::num::NonZero::get)
    } else {
        threads
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count.max(1))
        .build()
        .context("cannot build worker pool")?;
    Ok((thread_count, pool))
}

fn find_scan_candidates(
    source: &dyn ImageSource,
    plan: &ScanPlan,
    opts: &ScanOptions,
    volumes: &[FsInfo],
    pool: &rayon::ThreadPool,
    stop: &Stop,
    warnings: &mut Vec<Warning>,
) -> Result<(Vec<Candidate>, Arc<Progress>)> {
    let task_plan = TaskPlan::new(&plan.extents, opts.chunk_size);
    let scan_progress = Arc::new(Progress::new(plan.bytes_in_extents));
    let scan_thread = start_progress(scan_progress.clone(), opts.quiet, "Scanning");
    let find_ctx = CandidateScanCtx {
        source,
        types: &opts.types,
        opts,
        volumes,
        stop,
    };
    let (mut candidates, candidate_limit_hit) =
        find_candidates(&find_ctx, task_plan, &scan_progress, pool)?;
    candidates.sort_by_key(|c| (c.offset, c.kind as u8));
    candidates.dedup_by(|a, b| a.offset == b.offset && a.kind == b.kind);
    if candidate_limit_hit {
        warnings.push(Warning::new(
            "CANDIDATE_LIMIT",
            format!(
                "candidate work limit {} reached; additional header candidates were not evaluated",
                effective_max_candidates(opts)
            ),
        ));
    }
    if let Some(h) = scan_thread {
        scan_progress.stop.store(true, Ordering::Relaxed);
        let _ = h.join();
        eprint!("\r{:100}\r", "");
    }
    Ok((candidates, scan_progress))
}

fn carve_candidates(
    source: &dyn ImageSource,
    candidates: &[Candidate],
    opts: &ScanOptions,
    thread_count: usize,
    pool: &rayon::ThreadPool,
    stop: &Stop,
) -> Result<(Vec<CarveOutcome>, Arc<MemoryBudget>)> {
    let carve_progress = Arc::new(Progress::new(candidates.len() as u64));
    let carve_thread = start_progress(carve_progress.clone(), opts.quiet, "Carving");
    let budget = Arc::new(MemoryBudget::new(opts.max_memory));
    let mut outcomes = Vec::with_capacity(candidates.len());
    // Batching is only a scheduling aid now: the budget itself bounds
    // concurrent buffers. A fixed multiple of the thread count keeps result
    // order deterministic (`par_iter().collect()` preserves input order).
    let batch_size = thread_count.max(1).saturating_mul(4);
    for batch in candidates.chunks(batch_size.max(1)) {
        if stop.check() {
            break;
        }
        let mut batch_out = carve_batch(source, batch, opts, &budget, pool, &carve_progress, stop)?;
        outcomes.append(&mut batch_out);
    }
    if let Some(h) = carve_thread {
        carve_progress.stop.store(true, Ordering::Relaxed);
        let _ = h.join();
        eprint!("\r{:100}\r", "");
    }
    Ok((outcomes, budget))
}

fn snapshot_unreadable_regions(regions: &RegionLog) -> Vec<UnreadableRegion> {
    match regions.lock() {
        Ok(regions) => regions.clone(),
        Err(_) => Vec::new(),
    }
}

fn apply_unreadable_regions(
    kept: &mut Vec<CarvedData>,
    unreadable_snapshot: &[UnreadableRegion],
    skip_unreadable: bool,
    stats: &mut ScanStats,
    warnings: &mut Vec<Warning>,
) {
    // H-22: zero-filled bytes are not evidence.  Mark (or, under `skip`, drop)
    // every output that intersects a region the source could not read.
    if unreadable_snapshot.is_empty() {
        return;
    }
    let mut skipped = Vec::new();
    for (idx, data) in kept.iter_mut().enumerate() {
        let ranges = data.payload.ranges();
        let overlapping = unreadable_snapshot.iter().find(|region| {
            ranges.iter().any(|&(start, len)| {
                let end = start.saturating_add(len);
                start < region.offset.saturating_add(region.len) && region.offset < end
            })
        });
        if let Some(region) = overlapping {
            if skip_unreadable {
                skipped.push(idx);
            } else {
                let note = format!(
                    "output overlaps unreadable region at byte {} ({} bytes): not verified",
                    region.offset, region.len
                );
                data.validation = Validation::Partial;
                data.validity =
                    Validity::from_legacy(data.validation, data.analysis.status(data.repaired));
                data.validated_bytes = 0;
                data.validation_checks.push(ValidationCheck::new(
                    "overlaps_unreadable_region",
                    Some(false),
                    note.clone(),
                ));
                data.validation_notes.push(note);
            }
        }
    }
    if !skip_unreadable || skipped.is_empty() {
        return;
    }
    let count = skipped.len();
    for idx in skipped.into_iter().rev() {
        let removed = kept.remove(idx);
        stats.rejected = stats.rejected.saturating_add(1);
        warnings.push(Warning::new(
            "CANDIDATE_SKIPPED_UNREADABLE",
            format!(
                "candidate at byte {} was skipped because it overlaps an unreadable region",
                removed.candidate.offset
            ),
        ));
    }
    if count > 0 {
        // Keep the warning list deterministic: one warning per skipped
        // candidate is useful in verbose runs but noisy otherwise.
        warnings.dedup_by(|a, b| a.code == b.code && a.message == b.message);
    }
}

fn print_verbose_candidates(kept: &[CarvedData], opts: &ScanOptions) {
    if !opts.verbose || opts.quiet {
        return;
    }
    for (idx, file) in kept.iter().enumerate() {
        eprintln!(
            "[+] {:<4} offset={:>12} size={:>9} score={:>3} {} {:05}_{}_{:016x}.{}",
            file.candidate.kind.label(),
            file.candidate.offset,
            file.output_len,
            file.analysis.score,
            file.validity.display_label(),
            idx + 1,
            file.candidate.kind.ext(),
            file.candidate.offset,
            file.candidate.kind.ext()
        );
    }
}

fn update_output_stats(stats: &mut ScanStats, files: &[CarvedFile]) {
    stats.carved = files.len();
    stats.truncated = files
        .iter()
        .filter(|f| f.status != Status::Complete)
        .count();
    stats.repaired = files.iter().filter(|f| f.repaired).count();
    stats.bytes_written = files.iter().map(|f| f.size).sum();
}

fn finish_scan_warnings(
    stop: &Stop,
    fragment_nondeterministic: bool,
    internal_errors: &[InternalError],
    stats: &mut ScanStats,
    warnings: &mut Vec<Warning>,
) -> (bool, bool) {
    if stop.expired() && !warnings.iter().any(|w| w.code == "TIME_LIMIT") {
        warnings.push(Warning::new(
            "TIME_LIMIT",
            "scan exceeded --time-budget; stopped early with a partial report",
        ));
    }
    let interrupted = stop.interrupted();
    if interrupted && !warnings.iter().any(|w| w.code == "INTERRUPTED") {
        warnings.push(Warning::new(
            "INTERRUPTED",
            "SIGINT requested a cooperative stop; the report is partial",
        ));
    }
    let nondeterministic_stop =
        fragment_nondeterministic || warnings.iter().any(|w| w.code == "TIME_LIMIT");
    stats.internal_errors = internal_errors.len();
    if !internal_errors.is_empty() {
        warnings.push(Warning::new(
            "CANDIDATE_PANIC",
            format!(
                "{} candidate or fragment-join panic(s) were contained; affected work items were skipped",
                internal_errors.len()
            ),
        ));
    }
    (interrupted, nondeterministic_stop)
}

fn scan_throughput(
    t0: Instant,
    interrupted: bool,
    scan_progress: &Progress,
    total_to_scan: u64,
) -> (Duration, u64, f64) {
    let elapsed = t0.elapsed();
    // For an interrupted run, report only finished scan tasks; an in-flight
    // chunk is a lower bound rather than claiming the whole image was read.
    let bytes_scanned = if interrupted {
        scan_progress
            .done
            .load(Ordering::Relaxed)
            .min(total_to_scan)
    } else {
        total_to_scan
    };
    let throughput = if elapsed.as_secs_f64() > 0.0 {
        bytes_scanned as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0)
    } else {
        0.0
    };
    (elapsed, bytes_scanned, throughput)
}

/// Inputs to the final report assembly, grouped for H-22 without changing the
/// order in which report fields and warnings are produced.
struct ScanReportAssembly<'a> {
    started: u64,
    elapsed: Duration,
    bytes_scanned: u64,
    throughput: f64,
    image: &'a str,
    image_len: u64,
    input: InputInfo,
    mode: ScanMode,
    out_dir: &'a Path,
    ctx: &'a ScanContext,
    opts: &'a ScanOptions,
    volumes: &'a [FsInfo],
    unreadable_snapshot: Vec<UnreadableRegion>,
    warnings: Vec<Warning>,
    internal_errors: Vec<InternalError>,
    nondeterministic_stop: bool,
    interrupted: bool,
    files: Vec<CarvedFile>,
    stats: ScanStats,
}

fn assemble_scan_report(assembly: ScanReportAssembly<'_>) -> ScanReport {
    let ScanReportAssembly {
        started,
        elapsed,
        bytes_scanned,
        throughput,
        image,
        image_len,
        input,
        mode,
        out_dir,
        ctx,
        opts,
        volumes,
        unreadable_snapshot,
        warnings,
        internal_errors,
        nondeterministic_stop,
        interrupted,
        files,
        stats,
    } = assembly;
    ScanReport {
        schema_version: SCHEMA_VERSION,
        tool: format!("CarveX {}", env!("CARGO_PKG_VERSION")),
        tool_info: ToolInfo {
            name: "CarveX".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            commit: option_env!("CARVEX_GIT_COMMIT").unwrap_or("unknown").into(),
            rustc: env!("CARVEX_RUSTC").into(),
        },
        image: image.to_string(),
        input,
        image_size: image_len,
        filesystem: volumes.first().cloned(),
        mode: mode.as_str().to_string(),
        output_dir: out_dir.display().to_string(),
        argv: ctx.argv.clone(),
        options: opts.report_options(),
        case_id: ctx.case_id.clone(),
        examiner: ctx.examiner.clone(),
        started_at: crate::util::iso8601_utc(started),
        started_at_unix: started,
        duration_ms: elapsed.as_millis() as u64,
        bytes_scanned,
        throughput_mib_s: throughput,
        stats,
        volumes: volumes
            .iter()
            .enumerate()
            .map(|(i, fs)| VolumeSummary::from_fs(i + 1, fs, true))
            .collect(),
        regions: Vec::new(),
        unreadable_regions: unreadable_snapshot,
        warnings,
        internal_errors,
        nondeterministic_stop,
        interrupted,
        files,
    }
}

/// Scan an image and write carved files plus a report.
///
/// `input.volumes` are the selected filesystem volumes to scan (empty when the
/// scan is not filesystem-aware); `input.extra_extents` are additional raw
/// ranges (gaps / unsupported partitions added by `--include-gaps`).
pub fn run_scan(
    input: &ScanInput<'_>,
    opts: &ScanOptions,
    ctx: &ScanContext,
) -> Result<ScanReport> {
    validate_scan_options(opts)?;
    let interrupt = input.interrupt.clone();
    let ScanInput {
        source,
        image,
        image_identity,
        image_len,
        volumes,
        extra_extents,
        out_dir,
        unreadable_regions,
        skip_unreadable,
        interrupt: _,
    } = *input;
    let started = unix_timestamp();
    let t0 = Instant::now();
    let stop = Stop::new(t0, opts.time_budget, interrupt);
    let mut warnings = Vec::new();
    let mut internal_errors = Vec::new();
    apply_test_panic_offset();
    prepare_output_dir(out_dir, opts.force)?;
    let plan = prepare_scan_plan(opts, volumes, extra_extents, image_len)?;
    let (thread_count, pool) = worker_pool(opts.threads)?;
    let (candidates, scan_progress) =
        find_scan_candidates(source, &plan, opts, volumes, &pool, &stop, &mut warnings)?;
    let mut stats = ScanStats {
        candidates: candidates.len(),
        extents: plan.extents.len(),
        bytes_in_extents: plan.bytes_in_extents,
        ..Default::default()
    };
    let (outcomes, budget) =
        carve_candidates(source, &candidates, opts, thread_count, &pool, &stop)?;
    // Postfilter works on descriptors only; no carved bytes are retained.
    let mut kept = postfilter(
        outcomes,
        opts,
        &mut stats,
        &mut warnings,
        &mut internal_errors,
    );
    let mut fragment_nondeterministic = false;
    if opts.fragment_recovery {
        let fragment_ctx = FragmentRecoveryCtx {
            source,
            candidates: &candidates,
            extents: &plan.extents,
            volumes,
            opts,
            budget: &budget,
            stop: &stop,
        };
        let fragments =
            recover_fragments(&fragment_ctx, kept, &mut warnings, &mut internal_errors)?;
        kept = fragments.files;
        fragment_nondeterministic = fragments.nondeterministic_stop;
    }
    let unreadable_snapshot = snapshot_unreadable_regions(unreadable_regions);
    apply_unreadable_regions(
        &mut kept,
        &unreadable_snapshot,
        skip_unreadable,
        &mut stats,
        &mut warnings,
    );
    print_verbose_candidates(&kept, opts);
    let write_ctx = WriteCtx(
        source, out_dir, &kept, volumes, plan.mode, opts, &budget, &stop,
    );
    let (files, write_warnings) = write_outputs(write_ctx, &mut stats)?;
    warnings.extend(write_warnings);
    update_output_stats(&mut stats, &files);
    if opts.manifest {
        write_manifest(out_dir, &files)?;
    }
    let (interrupted, nondeterministic_stop) = finish_scan_warnings(
        &stop,
        fragment_nondeterministic,
        &internal_errors,
        &mut stats,
        &mut warnings,
    );
    let (elapsed, bytes_scanned, throughput) =
        scan_throughput(t0, interrupted, &scan_progress, plan.bytes_in_extents);
    let input_info = build_input_info(image, image_len, image_identity, ctx);
    if input_info.unchanged == Some(false) {
        warnings.push(Warning::new(
            "IMAGE_MODIFIED",
            "source image size or mtime changed during the scan",
        ));
    }
    Ok(assemble_scan_report(ScanReportAssembly {
        started,
        elapsed,
        bytes_scanned,
        throughput,
        image,
        image_len,
        input: input_info,
        mode: plan.mode,
        out_dir,
        ctx,
        opts,
        volumes,
        unreadable_snapshot,
        warnings,
        internal_errors,
        nondeterministic_stop,
        interrupted,
        files,
        stats,
    }))
}

/// Build the schema-valid report left behind when a read-error `abort` stops
/// the scan. It intentionally contains no carved files: work stopped before a
/// complete candidate set could be produced.
pub fn partial_report(
    input: &ScanInput<'_>,
    opts: &ScanOptions,
    ctx: &ScanContext,
    warnings: Vec<Warning>,
) -> ScanReport {
    let started = unix_timestamp();
    let unreadable_regions = match input.unreadable_regions.lock() {
        Ok(regions) => regions.clone(),
        Err(_) => Vec::new(),
    };
    let mut warnings = warnings;
    let interrupted = input.interrupt.as_ref().is_some_and(Interrupt::requested);
    if interrupted && !warnings.iter().any(|w| w.code == "INTERRUPTED") {
        warnings.push(Warning::new(
            "INTERRUPTED",
            "SIGINT requested a cooperative stop; the report is partial",
        ));
    }
    let input_info = build_input_info(input.image, input.image_len, input.image_identity, ctx);
    if input_info.unchanged == Some(false) {
        warnings.push(Warning::new(
            "IMAGE_MODIFIED",
            "source image size or mtime changed during the scan",
        ));
    }
    let mode = if opts.mode == ScanMode::Auto && input.volumes.is_empty() {
        ScanMode::Raw
    } else {
        opts.mode
    };
    ScanReport {
        schema_version: SCHEMA_VERSION,
        tool: format!("CarveX {}", env!("CARGO_PKG_VERSION")),
        tool_info: ToolInfo {
            name: "CarveX".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            commit: option_env!("CARVEX_GIT_COMMIT").unwrap_or("unknown").into(),
            rustc: env!("CARVEX_RUSTC").into(),
        },
        image: input.image.to_string(),
        input: input_info,
        image_size: input.image_len,
        filesystem: input.volumes.first().cloned(),
        mode: mode.as_str().to_string(),
        output_dir: input.out_dir.display().to_string(),
        argv: ctx.argv.clone(),
        options: opts.report_options(),
        case_id: ctx.case_id.clone(),
        examiner: ctx.examiner.clone(),
        started_at: crate::util::iso8601_utc(started),
        started_at_unix: started,
        duration_ms: 0,
        bytes_scanned: 0,
        throughput_mib_s: 0.0,
        stats: ScanStats::default(),
        volumes: input
            .volumes
            .iter()
            .enumerate()
            .map(|(i, fs)| VolumeSummary::from_fs(i + 1, fs, true))
            .collect(),
        regions: Vec::new(),
        unreadable_regions,
        warnings,
        internal_errors: Vec::new(),
        nondeterministic_stop: false,
        interrupted,
        files: Vec::new(),
    }
}

/// Resolve the exact byte extents a scan will read.
///
/// This is public because `regions[]` must be built from the same plan;
/// carving itself is unchanged by the coverage report (H-27).
pub fn plan_extents(
    mode: ScanMode,
    volumes: &[FsInfo],
    extra: &[Extent],
    image_len: u64,
) -> Result<(ScanMode, Vec<Extent>)> {
    let (resolved, mut extents) = match mode {
        ScanMode::Raw => (
            ScanMode::Raw,
            vec![Extent {
                offset: 0,
                len: image_len,
            }],
        ),
        ScanMode::Auto => {
            if volumes.is_empty() {
                (
                    ScanMode::Raw,
                    vec![Extent {
                        offset: 0,
                        len: image_len,
                    }],
                )
            } else {
                let mut all = Vec::new();
                for fs in volumes {
                    all.extend(fs.free_extents.iter().copied());
                }
                all.extend_from_slice(extra);
                (ScanMode::Unallocated, all)
            }
        }
        ScanMode::Unallocated => {
            if volumes.is_empty() {
                bail!(
                    "--mode unallocated requires a supported FAT32/NTFS filesystem \
                     (use --mode raw to force)"
                );
            }
            let mut all = Vec::new();
            for fs in volumes {
                all.extend(fs.free_extents.iter().copied());
            }
            all.extend_from_slice(extra);
            (ScanMode::Unallocated, all)
        }
        ScanMode::Allocated => {
            if volumes.is_empty() {
                bail!(
                    "--mode allocated requires a supported FAT32/NTFS filesystem \
                     (use --mode raw to force)"
                );
            }
            let mut all = Vec::new();
            for fs in volumes {
                all.extend(complement_extents(fs.offset, fs.length, &fs.free_extents));
            }
            all.extend_from_slice(extra);
            (ScanMode::Allocated, all)
        }
    };

    for e in &mut extents {
        let end = e.end().min(image_len);
        e.len = end.saturating_sub(e.offset);
    }
    extents.retain(|e| e.len > 0);
    Ok((resolved, merge_extents(extents)))
}

// H-22: candidate discovery groups its shared worker inputs in `CandidateScanCtx`;
// source reads, quota assignment and warning order are unchanged.
fn find_candidates(
    ctx: &CandidateScanCtx<'_>,
    task_plan: TaskPlan<'_>,
    progress: &Progress,
    pool: &rayon::ThreadPool,
) -> Result<(Vec<Candidate>, bool)> {
    if ctx.stop.check() {
        return Ok((Vec::new(), false));
    }
    let max_u64 = effective_max_candidates(ctx.opts);
    let max = usize::try_from(max_u64).unwrap_or(usize::MAX);
    let mut out = Vec::new();
    // Only the initial capacity is capped; the vector still grows to the
    // configured limit if that many candidates really exist.
    out.try_reserve_exact(max.min(SCAN_TASK_BATCH * 64))
        .context("cannot reserve candidate result vector")?;
    let mut total: u64 = 0;
    let mut hit = false;
    let mut remaining = max;
    let mut tasks = task_plan.iter();

    // H-12b: count, allocate quotas and scan one bounded batch at a time. The
    // old all-at-once version materialised one Task per chunk (about 10 MiB of
    // plan per declared TiB) before any budget could be checked. Quotas are
    // still assigned in task order, so the result is byte-for-byte identical.
    loop {
        if ctx.stop.check() {
            break;
        }
        let batch: Vec<Task> = tasks.by_ref().take(SCAN_TASK_BATCH).collect();
        if batch.is_empty() {
            break;
        }
        // First pass: count plausible headers in this batch. This is a cheap
        // lexical scan; it lets us hand out an exact deterministic global
        // budget without an atomic reservation whose winner depends on
        // scheduling.
        let counts: Vec<usize> = pool.install(|| {
            batch
                .par_iter()
                .map_init(Vec::<u8>::new, |scratch, task| {
                    count_task(ctx, task, scratch)
                })
                .collect::<Result<Vec<_>>>()
        })?;
        if ctx.stop.check() {
            break;
        }
        for &count in &counts {
            total = total.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
            if total > max_u64 {
                hit = true;
            }
        }
        let quotas: Vec<usize> = counts
            .iter()
            .map(|&count| {
                let quota = count.min(remaining);
                remaining = remaining.saturating_sub(quota);
                quota
            })
            .collect();

        let found: Vec<Vec<Candidate>> = pool.install(|| {
            batch
                .par_iter()
                .zip(quotas)
                .map_init(Vec::<u8>::new, |scratch, (task, quota)| {
                    if ctx.stop.check() || quota == 0 {
                        return Ok(Vec::new());
                    }
                    scan_task(ctx, task, progress, quota, scratch)
                })
                .collect::<Result<Vec<_>>>()
        })?;
        for candidates in found {
            out.extend(candidates);
        }
    }
    Ok((out, hit))
}

// H-22: the counting worker keeps chunk accounting identical; scratch reuse
// is a per-worker allocation optimization (R2-02, ADR-H15).
fn count_task(ctx: &CandidateScanCtx<'_>, task: &Task, scratch: &mut Vec<u8>) -> Result<usize> {
    if ctx.stop.check() {
        return Ok(0);
    }
    scratch.resize(task.read_len as usize, 0);
    read_exact_at(ctx.source, scratch, task.start)
        .with_context(|| format!("cannot read scan chunk at offset {}", task.start))?;
    let mut count = 0usize;
    for &kind in ctx.types {
        for rel in memchr::memmem::find_iter(scratch.as_slice(), kind.header()) {
            if ctx.stop.check() {
                return Ok(count);
            }
            if rel as u64 >= task.scan_len || !plausible_header(kind, scratch.as_slice(), rel) {
                continue;
            }
            let offset = task.start + rel as u64;
            if aligned(offset, ctx.opts.align, ctx.volumes) {
                count = count.saturating_add(1);
            }
        }
    }
    Ok(count)
}

// H-22: the scanning worker keeps chunk order and panic containment
// identical; scratch reuse is a per-worker allocation optimization (R2-02).
fn scan_task(
    ctx: &CandidateScanCtx<'_>,
    task: &Task,
    progress: &Progress,
    quota: usize,
    scratch: &mut Vec<u8>,
) -> Result<Vec<Candidate>> {
    scratch.resize(task.read_len as usize, 0);
    read_exact_at(ctx.source, scratch, task.start)
        .with_context(|| format!("cannot read scan chunk at offset {}", task.start))?;
    let mut out = Vec::with_capacity(quota);
    let mut examined = task.scan_len;
    'types: for &kind in ctx.types {
        for rel in memchr::memmem::find_iter(scratch.as_slice(), kind.header()) {
            if ctx.stop.check() {
                examined = (rel as u64).min(task.scan_len);
                break 'types;
            }
            if rel as u64 >= task.scan_len || !plausible_header(kind, scratch.as_slice(), rel) {
                continue;
            }
            let offset = task.start + rel as u64;
            if !aligned(offset, ctx.opts.align, ctx.volumes) {
                continue;
            }
            if out.len() >= quota {
                break 'types;
            }
            out.push(Candidate {
                offset,
                extent_end: task.extent_end,
                extent_id: task.extent_id,
                kind,
            });
        }
    }
    progress.done.fetch_add(examined, Ordering::Relaxed);
    Ok(out)
}

/// Cheap pre-filter applied before the full parser.
fn plausible_header(kind: FileType, buf: &[u8], rel: usize) -> bool {
    match kind {
        FileType::Jpeg => buf
            .get(rel + 3)
            .is_some_and(|&m| (0xC0..=0xFE).contains(&m) && m != 0xD8 && m != 0xD9),
        FileType::Pdf => {
            let Some(v) = buf.get(rel + 5..rel + 8) else {
                return false;
            };
            v[0].is_ascii_digit() && v[1] == b'.' && v[2].is_ascii_digit()
        }
        FileType::Png => true,
    }
}

/// The volume containing `offset`, used for alignment and metadata.
fn volume_at(volumes: &[FsInfo], offset: u64) -> Option<&FsInfo> {
    volumes
        .iter()
        .find(|v| offset >= v.offset && offset < v.offset.saturating_add(v.length))
}

fn aligned(offset: u64, align: AlignMode, volumes: &[FsInfo]) -> bool {
    let cluster_ok = |fs: &FsInfo| {
        offset
            .checked_sub(fs.offset)
            .is_some_and(|rel| fs.cluster_size != 0 && rel % fs.cluster_size == 0)
    };
    match align {
        AlignMode::None => true,
        AlignMode::Sector => offset % 512 == 0,
        AlignMode::Auto => match volume_at(volumes, offset) {
            Some(fs) => cluster_ok(fs),
            None => offset % 512 == 0,
        },
        AlignMode::Cluster => volume_at(volumes, offset).is_some_and(cluster_ok),
    }
}

fn carve_batch(
    source: &dyn ImageSource,
    batch: &[Candidate],
    opts: &ScanOptions,
    budget: &MemoryBudget,
    pool: &rayon::ThreadPool,
    progress: &Progress,
    stop: &Stop,
) -> Result<Vec<CarveOutcome>> {
    pool.install(|| {
        batch
            .par_iter()
            .map_init(Vec::<u8>::new, |scratch, cand| {
                if stop.check() {
                    return Ok(CarveOutcome::TimeLimit);
                }
                let out = match catch_unwind(AssertUnwindSafe(|| {
                    carve_one(source, cand, opts, budget, stop, scratch)
                })) {
                    Ok(result) => result?,
                    Err(payload) => CarveOutcome::InternalError {
                        offset: cand.offset,
                        kind: cand.kind,
                        message: panic_message(payload.as_ref()),
                    },
                };
                progress.done.fetch_add(1, Ordering::Relaxed);
                Ok(out)
            })
            .collect()
    })
}

fn work_limit_outcome(cand: &Candidate, examined: u64, limit: u64) -> CarveOutcome {
    CarveOutcome::WorkLimit {
        offset: cand.offset,
        kind: cand.kind,
        examined,
        limit,
    }
}

fn prepare_analysis(
    cand: &Candidate,
    prefix: &[u8],
    analysis: &mut Analysis,
    opts: &ScanOptions,
) -> Result<u64, CarveOutcome> {
    if analysis.end == 0 || analysis.end > prefix.len() {
        return Err(CarveOutcome::Rejected("invalid carved length".into()));
    }
    let mut tail_trimmed_bytes = 0;
    if !analysis.complete {
        let (trimmed, notes) = trim_incomplete_tail(&prefix[..analysis.end]);
        if trimmed < analysis.end {
            tail_trimmed_bytes = (analysis.end - trimmed) as u64;
            analysis.notes.extend(notes);
            analysis.end = trimmed;
        }
    }
    let end = analysis.end;
    if !analysis.complete && (end as u64) > opts.max_truncated_size {
        return Err(CarveOutcome::TooLargeTruncated {
            offset: cand.offset,
            kind: cand.kind,
            size: end as u64,
        });
    }
    if analysis.score < opts.min_score {
        return Err(CarveOutcome::LowScore {
            offset: cand.offset,
            kind: cand.kind,
            score: analysis.score,
        });
    }
    if opts.complete_only && !analysis.complete {
        return Err(CarveOutcome::NotComplete);
    }
    Ok(tail_trimmed_bytes)
}

struct RepairState {
    attempted: bool,
    suffix: Vec<u8>,
    replacement: Option<Vec<u8>>,
    repaired: bool,
    sha256_output: Option<String>,
    sha256_original: Option<String>,
    recovered_rows: Option<u32>,
    total_rows: Option<u32>,
    detail: Option<ValidationDetail>,
    note: Option<String>,
    reason: Option<String>,
}

impl RepairState {
    fn new(attempted: bool) -> Self {
        Self {
            attempted,
            suffix: Vec::new(),
            replacement: None,
            repaired: false,
            sha256_output: None,
            sha256_original: None,
            recovered_rows: None,
            total_rows: None,
            detail: None,
            note: None,
            reason: None,
        }
    }
}

/// Inputs to one repair attempt, grouped for H-22 without changing validation
/// order or budget accounting.
struct RepairAttempt<'a> {
    cand: &'a Candidate,
    analysis: &'a mut Analysis,
    prefix: &'a [u8],
    end: usize,
    opts: &'a ScanOptions,
    limits: ValidationLimits,
    budget: &'a MemoryBudget,
    work: &'a mut WorkCounter,
    work_limit: u64,
    attempted: bool,
}

fn attempt_repair(ctx: RepairAttempt<'_>) -> Result<RepairState, CarveOutcome> {
    let RepairAttempt {
        cand,
        analysis,
        prefix,
        end,
        opts,
        limits,
        budget,
        work,
        work_limit,
        attempted,
    } = ctx;
    let mut state = RepairState::new(attempted);
    if !attempted {
        return Ok(state);
    }
    // H-21/H-04: build, revalidate, and only then expose the changed bytes.
    // A crop repair is a complete replacement; a PDF repair is still a suffix.
    let plan = match build_plan(cand.kind, &prefix[..end], limits) {
        Ok(plan) => Some(plan),
        Err(reason) => {
            state.reason = Some(reason);
            None
        }
    };
    if let Some(plan) = plan {
        let suffix = plan.suffix;
        let rows = plan.recovered_rows.zip(plan.total_rows);
        let has_replacement = plan.replacement.is_some();
        let replacement = plan.replacement;
        let rebuilt_len = if has_replacement {
            replacement.as_ref().map_or(0, Vec::len) as u64
        } else {
            (end as u64).saturating_add(suffix.len() as u64)
        };
        match usize::try_from(rebuilt_len) {
            Ok(rebuilt_len) => {
                match BudgetGuard::reserve(budget, rebuilt_len as u64, "validating a repaired file")
                {
                    Ok(mut guard) => {
                        let mut rebuilt = Vec::new();
                        if rebuilt.try_reserve_exact(rebuilt_len).is_err() {
                            state.reason = Some("cannot reserve repair validation memory".into());
                        } else {
                            if has_replacement {
                                rebuilt = replacement.unwrap_or_default();
                            } else {
                                rebuilt.extend_from_slice(&prefix[..end]);
                                rebuilt.extend_from_slice(&suffix);
                            }
                            if !work.add(rebuilt_len as u64) {
                                return Err(work_limit_outcome(
                                    cand,
                                    work.used().saturating_add(rebuilt_len as u64),
                                    work_limit,
                                ));
                            }
                            test_panic_probe(cand.offset, "validate");
                            match validate_repair(cand.kind, &rebuilt, limits, opts.pdf_eof, rows) {
                                Ok(ValidatedRepair {
                                    analysis: rebuilt_analysis,
                                    detail,
                                    note,
                                }) => {
                                    state.sha256_output = Some(sha256_hex(&rebuilt));
                                    state.repaired = true;
                                    state.suffix = suffix;
                                    state.replacement =
                                        if has_replacement { Some(rebuilt) } else { None };
                                    state.recovered_rows = rows.map(|(kept, _)| kept);
                                    state.total_rows = rows.map(|(_, total)| total);
                                    state.detail = Some(detail);
                                    state.note = Some(note.clone());
                                    state.sha256_original = Some(sha256_hex(&prefix[..end]));
                                    // Keep the original score and provenance
                                    // notes; only the footer status becomes
                                    // complete after revalidation.
                                    analysis.complete = true;
                                    analysis.notes.extend(rebuilt_analysis.notes);
                                    analysis.notes.push(note);
                                }
                                Err(reason) => state.reason = Some(reason),
                            }
                        }
                        guard.release();
                    }
                    Err(e) => state.reason = Some(e.to_string()),
                }
            }
            Err(_) => state.reason = Some("repaired length is not addressable".into()),
        }
    }
    if !state.repaired {
        analysis
            .notes
            .push("repair attempted, not validated".into());
        if let Some(reason) = state.reason.as_ref() {
            analysis.notes.push(format!("repair reason: {reason}"));
        }
    }
    Ok(state)
}

fn validation_detail_for(
    cand: &Candidate,
    repair: &mut RepairState,
    analysis: &Analysis,
    prefix: &[u8],
    end: usize,
    opts: &ScanOptions,
    limits: ValidationLimits,
) -> ValidationDetail {
    if let Some(detail) = repair.detail.take() {
        return detail;
    }
    if opts.validate == ValidationMode::Decode {
        if analysis.complete {
            validate_deep_detail(cand.kind, &prefix[..end], analysis, limits)
        } else {
            ValidationDetail::new(
                Validation::Partial,
                vec!["file is truncated; decode validation skipped".into()],
            )
        }
    } else if analysis.complete {
        // The PNG structural parser already recomputes every chunk CRC. A
        // failure is an integrity failure even in the default mode, while no
        // decoded byte count is claimed.
        let notes = analysis
            .integrity_checks
            .iter()
            .map(|check| check.note.clone())
            .collect();
        ValidationDetail {
            validation: if analysis.integrity_failed() {
                Validation::Partial
            } else {
                Validation::Structure
            },
            notes,
            checks: analysis.integrity_checks.clone(),
            validated_bytes: 0,
        }
    } else {
        ValidationDetail::new(Validation::Partial, vec!["file is truncated".into()])
    }
}

/// Inputs to the final carved-data assembly, grouped for H-22 without
/// changing validation labels, notes or output lengths.
struct CarveFinalize<'a> {
    cand: &'a Candidate,
    analysis: Analysis,
    detail: ValidationDetail,
    repair: RepairState,
    prefix: &'a [u8],
    end: usize,
    output_len: u64,
    tail_trimmed_bytes: u64,
    opts: &'a ScanOptions,
}

fn finalize_carved(ctx: CarveFinalize<'_>) -> Result<CarvedData, CarveOutcome> {
    let CarveFinalize {
        cand,
        mut analysis,
        detail,
        repair,
        prefix,
        end,
        output_len,
        tail_trimmed_bytes,
        opts,
    } = ctx;
    let ValidationDetail {
        validation,
        notes: mut validation_notes,
        checks: detail_checks,
        validated_bytes,
    } = detail;
    // `carved_size` describes bytes read; a crop repair emits fewer bytes than
    // the carved prefix, so it must not claim more checked bytes than it wrote.
    let checked_len = repair
        .replacement
        .as_ref()
        .map_or(end as u64, |v: &Vec<u8>| v.len() as u64);
    let validated_bytes = validated_bytes.min(checked_len);
    let status = analysis.status(repair.repaired);
    let validity = Validity::from_legacy(validation, status);
    let mut validation_checks =
        base_validity_checks(cand.kind, validation, status, &validation_notes);
    validation_checks.extend(detail_checks);
    if repair.attempted {
        let (ok, note) = if repair.repaired {
            (
                Some(true),
                repair
                    .note
                    .unwrap_or_else(|| "repair revalidated".to_string()),
            )
        } else {
            (
                Some(false),
                repair
                    .reason
                    .unwrap_or_else(|| "repair attempted, not validated".to_string()),
            )
        };
        validation_checks.push(ValidationCheck::new("repair", ok, note));
    }
    if analysis.complete && validation == Validation::Partial {
        if opts.validate == ValidationMode::Decode {
            analysis.score = analysis.score.saturating_sub(20);
        }
        analysis.notes.append(&mut validation_notes);
        if analysis.score < opts.min_score {
            return Err(CarveOutcome::LowScore {
                offset: cand.offset,
                kind: cand.kind,
                score: analysis.score,
            });
        }
    }
    let sha256 = match repair.sha256_output.clone() {
        Some(digest) => digest,
        None => sha256_hex_with_suffix(&prefix[..end], &repair.suffix),
    };
    let replacement_len = repair.replacement.as_ref().map(|v| v.len() as u64);
    let output_len = replacement_len.unwrap_or(output_len);
    Ok(CarvedData {
        candidate: cand.clone(),
        tail_trimmed_bytes,
        payload: PayloadSpec::Range {
            offset: cand.offset,
            len: end as u64,
        },
        output_len,
        analysis,
        sha256,
        sha256_original: repair.sha256_original,
        repaired: repair.repaired,
        repair_suffix: repair.suffix,
        repair_replacement: repair.replacement,
        recovered_rows: repair.recovered_rows,
        total_rows: repair.total_rows,
        validation,
        validity,
        validation_notes,
        validated_bytes,
        validation_checks,
        fragments: Vec::new(),
        checks: None,
        confidence: None,
        alternatives: None,
        join: None,
        join_alternatives: None,
    })
}

const CARVE_SCRATCH_RETAIN: usize = 1024 * 1024;

fn cap_carve_scratch(scratch: &mut Vec<u8>) {
    if scratch.capacity() > CARVE_SCRATCH_RETAIN {
        *scratch = Vec::new();
    } else {
        scratch.clear();
    }
}

fn grow_carve_buffer<'a>(
    scratch: &mut Vec<u8>,
    guard: &mut BudgetGuard<'a>,
    buf_len: &mut u64,
    hard_limit: u64,
    budget: &'a MemoryBudget,
    cand: &Candidate,
) -> Result<Option<CarveOutcome>> {
    if *buf_len >= hard_limit {
        return Ok(Some(CarveOutcome::Rejected(
            "parser requested more data at the carve limit".into(),
        )));
    }
    let next = buf_len.saturating_mul(2).min(hard_limit);
    if next <= *buf_len {
        return Ok(Some(CarveOutcome::Rejected(
            "carve buffer cannot grow".into(),
        )));
    }
    if next > budget.limit() {
        return Ok(Some(CarveOutcome::SkippedBudget {
            offset: cand.offset,
            kind: cand.kind,
            needed: next,
        }));
    }
    // Drop old buffer and permit before reserving the next one so waiting
    // threads hold no permit and cannot deadlock (ADR-H15/R2-02).
    *scratch = Vec::new();
    guard.release();
    // A reserve failure here is an authoritative --max-memory error and is
    // propagated to the caller exactly as before the refactor.
    *guard = BudgetGuard::reserve(budget, next, "growing a carve buffer")?;
    scratch.resize(next as usize, 0);
    *buf_len = next;
    Ok(None)
}

/// Inputs to the repair/validate/finalize stage after a candidate has been
/// fully analyzed. Kept separate so `carve_one_with_scratch` stays within the
/// A-10 line limit without changing the order of work accounting or probes.
struct AnalyzedCandidateCtx<'a> {
    cand: &'a Candidate,
    analysis: Analysis,
    prefix: &'a [u8],
    tail_trimmed_bytes: u64,
    opts: &'a ScanOptions,
    limits: ValidationLimits,
    budget: &'a MemoryBudget,
    work: &'a mut WorkCounter,
    work_limit: u64,
}

fn finalize_analyzed_candidate(ctx: AnalyzedCandidateCtx<'_>) -> Result<CarveOutcome> {
    let AnalyzedCandidateCtx {
        cand,
        mut analysis,
        prefix,
        tail_trimmed_bytes,
        opts,
        limits,
        budget,
        work,
        work_limit,
    } = ctx;
    let end = analysis.end;
    // Validator scratch is bounded by its own flag and, for this scan, can
    // never raise the shared working-memory ceiling.
    let repair_attempted = opts.repair && !analysis.complete;
    let mut repair = match attempt_repair(RepairAttempt {
        cand,
        analysis: &mut analysis,
        prefix,
        end,
        opts,
        limits,
        budget,
        work: &mut *work,
        work_limit,
        attempted: repair_attempted,
    }) {
        Ok(state) => state,
        Err(outcome) => return Ok(outcome),
    };

    let output_len = repair.replacement.as_ref().map_or_else(
        || (end as u64).saturating_add(repair.suffix.len() as u64),
        |bytes| bytes.len() as u64,
    );
    if output_len < opts.min_size {
        return Ok(CarveOutcome::TooSmall(output_len));
    }

    let validate_bytes = if repair.detail.is_none()
        && opts.validate == ValidationMode::Decode
        && analysis.complete
    {
        end as u64
    } else {
        0
    };
    test_panic_probe(cand.offset, "validate");
    if validate_bytes > 0 && !work.add(validate_bytes) {
        return Ok(work_limit_outcome(
            cand,
            work.used().saturating_add(validate_bytes),
            work_limit,
        ));
    }
    let detail = validation_detail_for(cand, &mut repair, &analysis, prefix, end, opts, limits);
    match finalize_carved(CarveFinalize {
        cand,
        analysis,
        detail,
        repair,
        prefix,
        end,
        output_len,
        tail_trimmed_bytes,
        opts,
    }) {
        Ok(data) => Ok(CarveOutcome::Carved(Box::new(data))),
        Err(outcome) => Ok(outcome),
    }
}

fn carve_one(
    source: &dyn ImageSource,
    cand: &Candidate,
    opts: &ScanOptions,
    budget: &MemoryBudget,
    stop: &Stop,
    scratch: &mut Vec<u8>,
) -> Result<CarveOutcome> {
    let result = carve_one_with_scratch(source, cand, opts, budget, stop, scratch);
    cap_carve_scratch(scratch);
    result
}

fn carve_one_with_scratch(
    source: &dyn ImageSource,
    cand: &Candidate,
    opts: &ScanOptions,
    budget: &MemoryBudget,
    stop: &Stop,
    scratch: &mut Vec<u8>,
) -> Result<CarveOutcome> {
    if stop.check() {
        return Ok(CarveOutcome::TimeLimit);
    }
    let available = cand.extent_end.saturating_sub(cand.offset);
    let hard_limit = available.min(opts.max_size);
    if hard_limit < 16 {
        return Ok(CarveOutcome::Rejected(
            "file candidate too close to extent end".into(),
        ));
    }
    let initial_len = hard_limit.min(64 * 1024);
    let Ok(mut guard) = BudgetGuard::reserve(budget, initial_len, "allocating a carve buffer")
    else {
        return Ok(CarveOutcome::SkippedBudget {
            offset: cand.offset,
            kind: cand.kind,
            needed: initial_len,
        });
    };
    scratch.clear();
    scratch.resize(initial_len as usize, 0);
    let mut buf_len = initial_len;
    let work_limit = effective_max_work(opts);
    let mut work = WorkCounter::new(work_limit);
    // H-25: structural truncation detection may inflate a bounded IDAT/Flate
    // prefix, so it shares the same decode budget as deep validation.
    let limits = ValidationLimits::new(opts.max_validator_memory.min(budget.limit()));

    loop {
        if stop.check() {
            return Ok(CarveOutcome::TimeLimit);
        }
        // Every growth step re-reads the full prefix after dropping the old
        // buffer and its permit. Waiting threads therefore hold no permit and
        // cannot deadlock each other.
        if let Err(e) = read_exact_at(source, &mut scratch[..buf_len as usize], cand.offset) {
            return Ok(CarveOutcome::Rejected(format!("read error: {e}")));
        }
        let at_limit = buf_len >= hard_limit;
        let read_len = buf_len as usize;
        if !work.add(read_len as u64) {
            return Ok(work_limit_outcome(
                cand,
                work.used().saturating_add(read_len as u64),
                work_limit,
            ));
        }
        test_panic_probe(cand.offset, "analyze");
        match analyze_with_limits(
            cand.kind,
            &scratch[..read_len],
            at_limit,
            opts.pdf_eof,
            limits,
        ) {
            Need::Analyzed(mut analysis) => {
                let tail_trimmed_bytes =
                    match prepare_analysis(cand, &scratch[..read_len], &mut analysis, opts) {
                        Ok(trimmed) => trimmed,
                        Err(outcome) => return Ok(outcome),
                    };
                return finalize_analyzed_candidate(AnalyzedCandidateCtx {
                    cand,
                    analysis,
                    prefix: &scratch[..read_len],
                    tail_trimmed_bytes,
                    opts,
                    limits,
                    budget,
                    work: &mut work,
                    work_limit,
                });
            }
            Need::More => {
                if let Some(outcome) =
                    grow_carve_buffer(scratch, &mut guard, &mut buf_len, hard_limit, budget, cand)?
                {
                    return Ok(outcome);
                }
            }
            Need::Reject(reason) => return Ok(CarveOutcome::Rejected(reason)),
        }
    }
}

fn trim_zero_tail(data: &[u8]) -> usize {
    let mut end = data.len();
    while end > 0 && data.get(end - 1) == Some(&0) {
        end -= 1;
    }
    // Keep a short floor so an all-zero candidate does not become empty.
    end.max(16.min(data.len()))
}

/// H-25: trim one repeated byte from the end of an incomplete candidate.
/// Zero trimming keeps its historical behaviour. A long `0xFF` run (a fill
/// pattern, never entropy-coded data because a literal FF is stuffed as FF 00)
/// is trimmed only at the very end, never before a marker or inside entropy
/// data. The zero trim runs first so trailing zero padding cannot hide an
/// adjacent FF run; a short stuffed FF 00 is left alone.
const MIN_FF_TAIL_RUN: usize = 512;

fn trailing_ff_run(data: &[u8], end: usize) -> usize {
    let mut start = end;
    while start > 0 && data.get(start - 1) == Some(&0xFF) {
        start -= 1;
    }
    end - start
}

fn trim_incomplete_tail(data: &[u8]) -> (usize, Vec<String>) {
    let mut notes = Vec::new();
    let zero_end = trim_zero_tail(data);
    if zero_end < data.len() {
        notes.push(format!(
            "trimmed {} trailing zero byte(s)",
            data.len() - zero_end
        ));
    }
    let mut end = zero_end;
    let run = trailing_ff_run(data, end);
    if run >= MIN_FF_TAIL_RUN {
        let floor = 16.min(data.len());
        let trimmed_end = end.saturating_sub(run).max(floor);
        if trimmed_end < end {
            notes.push(format!(
                "trimmed {} trailing 0xFF byte(s)",
                end - trimmed_end
            ));
            end = trimmed_end;
        }
    }
    (end, notes)
}

// ---------------------------------------------------------------------------
// B-06: bounded bifragment gap carving
//
// A stitch is attempted only between two volume-cluster-aligned fragments.
// The head comes from a normal header candidate; the tail starts at a later
// cluster boundary and ends at a format footer.  Every structurally complete
// join must pass the deep validator before it is accepted, so a stitched file
// can never be reported as verified on structural evidence alone.

#[derive(Debug, Clone, Copy)]
struct FragmentJoin {
    /// Exclusive end of the head fragment (a cluster boundary).
    cut: u64,
    /// Start of the tail fragment (a cluster boundary).
    tail_start: u64,
    /// Exclusive end of the footer bytes.
    footer_end: u64,
    /// Number of clusters skipped between the two fragments.
    gap_clusters: u64,
    /// How many skipped clusters look like filler or another file's header.
    gap_matches: u64,
    /// H-30: this gap was found by the PNG chunk-CRC/chunk-header oracle and
    /// must be tried before the weaker filler/header-only candidates.
    png_oracle: bool,
}

impl FragmentJoin {
    /// Clusters between the fragments with no simple filler/header signal.
    /// Lower is better: a real split leaves filler, not ordinary file bytes.
    fn unexplained_gap(&self) -> u64 {
        self.gap_clusters.saturating_sub(self.gap_matches)
    }
}

struct JoinAttempt {
    data: Option<CarvedData>,
    deep_checked: bool,
    skipped_budget: Option<(u64, u64)>,
    work_limit: Option<WorkLimit>,
}

fn footer_needle(kind: FileType) -> &'static [u8] {
    match kind {
        FileType::Jpeg => &[0xFF, 0xD9],
        FileType::Png => &[
            0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
        ],
        FileType::Pdf => b"%%EOF",
    }
}

fn find_footers(
    source: &dyn ImageSource,
    extents: &[Extent],
    kinds: &[FileType],
    opts: &ScanOptions,
) -> Result<HashMap<FileType, Vec<u64>>> {
    let mut found: HashMap<FileType, Vec<u64>> = kinds.iter().map(|&k| (k, Vec::new())).collect();
    // H-12b: fragment footer scanning also avoids a per-chunk task vector.
    for task in TaskPlan::new(extents, 1024 * 1024).iter() {
        let mut buf = vec![0u8; task.read_len as usize];
        read_exact_at(source, &mut buf, task.start)
            .with_context(|| format!("cannot read fragment scan chunk at {}", task.start))?;
        let searchable = buf.get(..task.scan_len as usize).unwrap_or(&buf);
        for &kind in kinds {
            let needle = footer_needle(kind);
            let Some(hits) = found.get_mut(&kind) else {
                continue;
            };
            for rel in memchr::memmem::find_iter(searchable, needle) {
                let mut footer_len = needle.len();
                // PDF %%EOF is followed by an optional line ending that is
                // part of the original file.  Include it so a stitch can be
                // byte-identical rather than one byte short.
                if kind == FileType::Pdf {
                    let after = rel + footer_len;
                    if buf.get(after) == Some(&b'\r') && buf.get(after + 1) == Some(&b'\n') {
                        footer_len += 2;
                    } else if buf.get(after).is_some_and(|b| *b == b'\n' || *b == b'\r') {
                        footer_len += 1;
                    }
                }
                let end = task.start + rel as u64 + footer_len as u64;
                if end <= task.start + task.scan_len && hits.last() != Some(&end) {
                    hits.push(end);
                }
            }
        }
    }
    let cap = opts.max_frag_candidates.saturating_mul(4).min(8192);
    for hits in found.values_mut() {
        hits.sort_unstable();
        hits.dedup();
        hits.truncate(cap);
    }
    Ok(found)
}

/// A complete contiguous carve that contains more than one footer for its own
/// type is likely an interleaved splice, not one intact file. This is the
/// second cheap B-06 signal: the first is filler inside a contiguous carve.
fn contiguous_range_has_multiple_footers(
    source: &dyn ImageSource,
    kind: FileType,
    start: u64,
    len: u64,
) -> bool {
    let needle = footer_needle(kind);
    let overlap = needle.len().saturating_sub(1);
    let mut buf = vec![0u8; 1024 * 1024];
    let mut carry = Vec::new();
    let mut offset = start;
    let end = start.saturating_add(len);
    let mut count = 0usize;
    while offset < end {
        let want = (buf.len() - carry.len()) as u64;
        let take = (end - offset).min(want) as usize;
        buf[..carry.len()].copy_from_slice(&carry);
        if read_exact_at(source, &mut buf[carry.len()..carry.len() + take], offset).is_err() {
            return false;
        }
        let filled = carry.len() + take;
        count += memchr::memmem::find_iter(&buf[..filled], needle).count();
        if count > 1 {
            return true;
        }
        carry.clear();
        if filled > overlap {
            carry.extend_from_slice(&buf[filled - overlap..filled]);
        }
        offset += take as u64;
    }
    false
}

fn cluster_base(fs: &FsInfo) -> u64 {
    fs.data_start.max(fs.offset)
}

fn is_cluster_boundary(offset: u64, fs: &FsInfo) -> bool {
    fs.cluster_size != 0
        && offset
            .checked_sub(cluster_base(fs))
            .is_some_and(|rel| rel % fs.cluster_size == 0)
}

fn next_cluster_boundary(offset: u64, fs: &FsInfo) -> Option<u64> {
    if fs.cluster_size == 0 {
        return None;
    }
    let base = cluster_base(fs);
    let floor = offset.checked_sub(offset.checked_sub(base)? % fs.cluster_size)?;
    if floor == offset {
        offset.checked_add(fs.cluster_size)
    } else {
        floor.checked_add(fs.cluster_size)
    }
}

fn range_covered(extents: &[Extent], start: u64, end: u64) -> bool {
    extents
        .iter()
        .any(|e| start >= e.offset && end <= e.end() && start < end)
}

fn starts_with_file_header(window: &[u8]) -> bool {
    const HEADERS: [&[u8]; 3] = [
        &[0xFF, 0xD8, 0xFF],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        b"%PDF-",
    ];
    HEADERS.iter().any(|header| window.starts_with(header))
}

/// Cheap prefilter for a prospective tail fragment. A B-06 tail is the
/// remainder of a file, not another header and not an empty gap. Rejecting
/// those here keeps the expensive deep-validation work-unit budget focused on
/// joins that can plausibly be real fragments (H-29).
fn tail_window_plausible(source: &dyn ImageSource, tail_start: u64) -> bool {
    let mut window = [0u8; 32];
    if read_exact_at(source, &mut window, tail_start).is_err() {
        return false;
    }
    !window.iter().all(|&byte| byte == 0) && !starts_with_file_header(&window)
}

/// Cheaply recognise the cluster immediately before a tail start as filler
/// left by a split (all zero, a small repeated alphabet, or a short repeated
/// pattern). Real compressed file bytes are not low-entropy in this way, so
/// this rejects the many tail windows that merely sit inside one fragment.
fn window_is_gap_like(window: &[u8]) -> bool {
    if window.len() < 8 {
        return false;
    }
    if window.iter().all(|&byte| byte == 0) {
        return true;
    }
    let mut seen = [false; 256];
    let unique = window
        .iter()
        .filter(|&&byte| {
            let slot = &mut seen[byte as usize];
            let was_seen = *slot;
            *slot = true;
            !was_seen
        })
        .count();
    if unique <= 8 {
        return true;
    }
    (1..=32).any(|period| {
        window
            .iter()
            .enumerate()
            .all(|(index, &byte)| byte == window[index % period])
    })
}

/// Detect a filler run inside an otherwise complete contiguous carve. This is
/// the cheap signal that a structurally complete file may actually be two
/// fragments separated by a gap; such a candidate must be deep-validated even
/// when `--validate` is the default structural mode (H-29).
fn contiguous_range_has_gap(
    source: &dyn ImageSource,
    cluster_size: u64,
    start: u64,
    len: u64,
) -> bool {
    if cluster_size == 0 || len <= cluster_size.saturating_mul(2) {
        return false;
    }
    let sample_len = cluster_size.min(64) as usize;
    // One small read per cluster made ordinary images pay hundreds of syscalls
    // for a stage that finds no head. Read a bounded block and inspect every
    // cluster boundary inside it, preserving the exact same samples.
    let clusters_per_block = (256 * 1024u64 / cluster_size).max(1);
    let span = cluster_size.saturating_mul(clusters_per_block);
    let mut buf = vec![0u8; span.saturating_add(sample_len as u64) as usize];
    let step = usize::try_from(cluster_size).unwrap_or(usize::MAX);
    let mut offset = start.saturating_add(cluster_size);
    let end = start.saturating_add(len).saturating_sub(cluster_size);
    while offset < end {
        let block_end = end.min(offset.saturating_add(span));
        let take = (block_end - offset) as usize;
        let read_len = take + sample_len;
        if read_exact_at(source, &mut buf[..read_len], offset).is_err() {
            return false;
        }
        let mut rel = 0usize;
        while rel < take {
            if window_is_gap_like(&buf[rel..rel + sample_len]) {
                return true;
            }
            rel = rel.saturating_add(step);
        }
        offset = block_end;
    }
    false
}

/// Precompute which clusters begin with filler (zeros, a repeated byte/pattern)
/// or another file's header. Footer-only joins can reassemble a contiguous
/// carve with a few bytes cut out of its tail; those gaps contain ordinary file
/// bytes and score worse than a real split. One pass per head keeps the cheap
/// signal cheap even when many tail/cut combinations are enumerated.
fn gap_cluster_flags(
    source: &dyn ImageSource,
    base: u64,
    cluster_size: u64,
    extent_end: u64,
) -> (Vec<bool>, Vec<bool>) {
    let sample_len = cluster_size.min(64) as usize;
    let mut window = vec![0u8; sample_len];
    let clusters = extent_end.saturating_sub(base) / cluster_size;
    let mut filler = Vec::with_capacity(clusters.saturating_add(1) as usize);
    let mut header = Vec::with_capacity(clusters.saturating_add(1) as usize);
    for index in 0..=clusters {
        let Some(offset) = base.checked_add(index.saturating_mul(cluster_size)) else {
            filler.push(false);
            header.push(false);
            continue;
        };
        let readable = read_exact_at(source, &mut window, offset).is_ok();
        filler.push(readable && window_is_gap_like(&window));
        header.push(readable && starts_with_file_header(&window));
    }
    (filler, header)
}

fn cluster_flag(flags: &[bool], base: u64, cluster_size: u64, offset: u64) -> bool {
    offset
        .checked_sub(base)
        .map(|delta| delta / cluster_size)
        .and_then(|index| flags.get(index as usize))
        .copied()
        .unwrap_or(false)
}

fn gap_cluster_matches(
    filler: &[bool],
    header: &[bool],
    base: u64,
    cluster_size: u64,
    cut: u64,
    tail_start: u64,
) -> u64 {
    if tail_start <= cut {
        return 0;
    }
    let first = cut.saturating_sub(base) / cluster_size;
    let last = tail_start.saturating_sub(base) / cluster_size;
    (first..last)
        .filter(|&index| {
            filler.get(index as usize).copied().unwrap_or(false)
                || header.get(index as usize).copied().unwrap_or(false)
        })
        .count() as u64
}

fn join_rank(join: &FragmentJoin) -> (u8, u64, std::cmp::Reverse<u64>, u64, u64, u64, u64) {
    (
        // H-30: the PNG CRC/chunk-header oracle explains a gap directly, so it
        // is tried before filler-only candidates even though the gap bytes may
        // be high entropy. Existing joins keep their relative ordering.
        u8::from(!join.png_oracle),
        join.unexplained_gap(),
        // Among fully explained gaps, more filler evidence is stronger than a
        // merely smaller gap; a three-cluster zero run is a better split than
        // one zero cluster cut out of a contiguous file.
        std::cmp::Reverse(join.gap_matches),
        join.gap_clusters,
        join.tail_start,
        join.footer_end,
        join.cut,
    )
}

/// H-30: derive PNG bifragment candidates from the chunk CRC-32 oracle.
///
/// The oracle is only a candidate generator. Every join returned here still
/// goes through the unchanged structural analysis and deep decode validator in
/// [`try_fragment_join`]. All reads are checked against scanned extents, so an
/// allocated island cannot be sampled unless `--mode raw` selected it.
const PNG_ORACLE_HEAD_CAP: u64 = 8 * 1024 * 1024;
const PNG_ORACLE_CHUNK_CAP: usize = 1024 * 1024;
const PNG_ORACLE_CUT_CAP: usize = 64;
const PNG_ORACLE_FOOTERS: usize = 8;

/// Compute a CRC-32 over `ctype` and a byte range split across two sources.
/// `prefix` is the head slice, `tail` the first bytes at the candidate tail
/// offset. Each hasher carries an explicit byte count so `Hasher::combine`
/// reproduces a contiguous CRC without concatenating the bytes.
fn png_split_crc(ctype: &[u8; 4], prefix: &[u8], tail: &[u8]) -> u32 {
    let mut head_crc = Hasher::new();
    head_crc.update(ctype);
    head_crc.update(prefix);
    let mut tail_crc = Hasher::new();
    tail_crc.update(tail);
    head_crc.combine(&tail_crc);
    head_crc.finalize()
}

#[allow(clippy::too_many_arguments)]
fn png_crc_joins(
    source: &dyn ImageSource,
    cand: &Candidate,
    fs: &FsInfo,
    footers: &[u64],
    extents: &[Extent],
    opts: &ScanOptions,
    _budget: &MemoryBudget,
    stop: &Stop,
) -> Vec<FragmentJoin> {
    let mut out = Vec::new();
    if cand.kind != FileType::Png || footers.is_empty() || opts.validate != ValidationMode::Decode {
        return out;
    }
    let Ok(cluster) = usize::try_from(fs.cluster_size) else {
        return out;
    };
    if cluster == 0 {
        return out;
    }
    let full_span = cand.extent_end.saturating_sub(cand.offset);
    let head_span = opts.max_size.min(full_span);
    let cap = PNG_ORACLE_HEAD_CAP.min(head_span).max(16);
    let Ok(head_len) = usize::try_from(cap) else {
        return out;
    };
    if head_len < 16 || !range_covered(extents, cand.offset, cand.offset + cap) {
        return out;
    }
    let mut head = Vec::new();
    if head.try_reserve_exact(head_len).is_err() {
        return out;
    }
    head.resize(head_len, 0u8);
    if read_exact_at(source, &mut head, cand.offset).is_err() {
        return out;
    }
    let Some(gap_kind) = png_first_crc_gap(&head, head.len()) else {
        return out;
    };

    let limit = opts.max_frag_candidates.saturating_mul(2).max(64);
    let mut seen: HashSet<(u64, u64, u64)> = HashSet::new();

    let push_join = |out: &mut Vec<FragmentJoin>,
                     seen: &mut HashSet<(u64, u64, u64)>,
                     cut_rel: usize,
                     tail_start: u64,
                     footer_end: u64,
                     gap_clusters: u64| {
        let Some(cut_abs) = cand.offset.checked_add(cut_rel as u64) else {
            return;
        };
        if tail_start <= cut_abs || !seen.insert((cut_abs, tail_start, footer_end)) {
            return;
        }
        out.push(FragmentJoin {
            cut: cut_abs,
            tail_start,
            footer_end,
            gap_clusters,
            gap_matches: gap_clusters,
            png_oracle: true,
        });
    };

    match gap_kind {
        PngCrcGap::Broken {
            data_start,
            chunk_len,
            ctype,
            stored_crc,
            ..
        } => {
            if chunk_len > PNG_ORACLE_CHUNK_CAP {
                return out;
            }
            let chunk_data_end = data_start.saturating_add(chunk_len);
            // The split may fall inside the chunk data *or* inside its four
            // trailing CRC bytes. In the latter case the prefix already holds
            // the whole payload, so any cut after `data_start` is still valid.
            let chunk_end = chunk_data_end.saturating_add(4);
            let first_rel = data_start.div_ceil(cluster).saturating_mul(cluster);
            let last_rel = chunk_end.min(head.len()) / cluster * cluster;
            let mut cut_rel = first_rel;
            let mut cuts = 0usize;
            while cut_rel <= last_rel && cuts < PNG_ORACLE_CUT_CAP {
                cuts += 1;
                if cut_rel >= data_start && cut_rel < chunk_end {
                    let prefix = head.get(data_start..cut_rel).unwrap_or(&[]);
                    let suffix_len = chunk_data_end.saturating_sub(cut_rel);
                    let crc_rel = chunk_end.saturating_sub(cut_rel);
                    for gap in 1..=opts.max_gap {
                        if stop.check() || out.len() >= limit {
                            return out;
                        }
                        let Some(gap_bytes) = fs.cluster_size.checked_mul(gap) else {
                            break;
                        };
                        let Some(cut_abs) = cand.offset.checked_add(cut_rel as u64) else {
                            break;
                        };
                        let Some(tail_start) = cut_abs.checked_add(gap_bytes) else {
                            break;
                        };
                        let Some(footer_end) = footers
                            .iter()
                            .copied()
                            .filter(|&end| end > tail_start)
                            .take(PNG_ORACLE_FOOTERS)
                            .find(|&end| end.saturating_sub(tail_start) <= opts.max_size)
                        else {
                            continue;
                        };
                        let need = crc_rel;
                        let need_u64 = u64::try_from(need).unwrap_or(u64::MAX);
                        let tail_end = tail_start.saturating_add(need_u64);
                        if need == 0
                            || need > PNG_ORACLE_CHUNK_CAP.saturating_add(4)
                            || tail_end.saturating_sub(tail_start) != need_u64
                            || !range_covered(extents, cand.offset, cut_abs)
                            || !range_covered(extents, tail_start, tail_end)
                        {
                            continue;
                        }
                        let mut tail = vec![0u8; need];
                        if read_exact_at(source, &mut tail, tail_start).is_err() {
                            continue;
                        }
                        let Some(stored) = stored_crc.or_else(|| {
                            tail.get(suffix_len..crc_rel)
                                .and_then(|b| <[u8; 4]>::try_from(b).ok())
                                .map(u32::from_be_bytes)
                        }) else {
                            continue;
                        };
                        let suffix = tail.get(..suffix_len).unwrap_or(&[]);
                        if png_split_crc(&ctype, prefix, suffix) == stored {
                            push_join(&mut out, &mut seen, cut_rel, tail_start, footer_end, gap);
                        }
                    }
                }
                let Some(next) = cut_rel.checked_add(cluster) else {
                    break;
                };
                cut_rel = next;
            }
        }
        PngCrcGap::Boundary { last_chunk_end } => {
            // A cut exactly on a chunk boundary leaves no failing chunk. The
            // next header must parse, be plausibly sized and fit before IEND.
            let mut cut_rel = last_chunk_end / cluster * cluster;
            for _ in 0..2usize {
                for gap in 1..=opts.max_gap {
                    if stop.check() || out.len() >= limit {
                        return out;
                    }
                    let Some(gap_bytes) = fs.cluster_size.checked_mul(gap) else {
                        break;
                    };
                    let Some(cut_abs) = cand.offset.checked_add(cut_rel as u64) else {
                        break;
                    };
                    let Some(tail_start) = cut_abs.checked_add(gap_bytes) else {
                        break;
                    };
                    if tail_start.saturating_sub(cand.offset) > head_span {
                        continue;
                    }
                    let Some(footer_end) = footers
                        .iter()
                        .copied()
                        .filter(|&end| end > tail_start)
                        .take(PNG_ORACLE_FOOTERS)
                        .find(|&end| end.saturating_sub(tail_start) <= opts.max_size)
                    else {
                        continue;
                    };
                    let mut header = [0u8; 12];
                    if !range_covered(extents, cand.offset, cut_abs)
                        || !range_covered(extents, tail_start, tail_start.saturating_add(12))
                        || read_exact_at(source, &mut header, tail_start).is_err()
                        || !png_chunk_header_plausible(&header)
                    {
                        continue;
                    }
                    let len = u32::from_be_bytes(
                        header
                            .get(0..4)
                            .and_then(|b| <[u8; 4]>::try_from(b).ok())
                            .unwrap_or([0; 4]),
                    ) as u64;
                    if footer_end.saturating_sub(tail_start) < len.saturating_add(12) {
                        continue;
                    }
                    push_join(&mut out, &mut seen, cut_rel, tail_start, footer_end, gap);
                }
                if cut_rel < cluster {
                    break;
                }
                cut_rel -= cluster;
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn candidate_joins(
    source: &dyn ImageSource,
    cand: &Candidate,
    fs: &FsInfo,
    footers: &[u64],
    extents: &[Extent],
    opts: &ScanOptions,
    budget: &MemoryBudget,
    stop: &Stop,
) -> Vec<FragmentJoin> {
    let mut joins = Vec::new();
    if fs.cluster_size == 0 || !is_cluster_boundary(cand.offset, fs) {
        return joins;
    }
    let base = cluster_base(fs);
    let available = cand.extent_end.saturating_sub(cand.offset);
    let head_span = opts.max_size.min(available);
    let max_tail_clusters = (opts.max_size / fs.cluster_size).max(1);
    let join_pool_limit = opts.max_frag_candidates.saturating_mul(4).max(1024);
    let (gap_filler, gap_headers) =
        gap_cluster_flags(source, base, fs.cluster_size, cand.extent_end);

    for &footer_end in footers {
        if footer_end <= cand.offset.saturating_add(fs.cluster_size) {
            continue;
        }
        let end_before_footer = footer_end.saturating_sub(1);
        if end_before_footer < base {
            continue;
        }
        let mut tail_start = base
            .checked_add((end_before_footer - base) / fs.cluster_size * fs.cluster_size)
            .unwrap_or(base);
        for _tail_clusters in 1..=max_tail_clusters {
            if tail_start <= cand.offset.saturating_add(fs.cluster_size)
                || tail_start.saturating_sub(cand.offset) > head_span
            {
                break;
            }
            let tail_len = footer_end - tail_start;
            if tail_len > opts.max_size {
                break;
            }
            if tail_window_plausible(source, tail_start)
                && range_covered(extents, tail_start, footer_end)
            {
                for gap in 1..=opts.max_gap {
                    let Some(gap_bytes) = fs.cluster_size.checked_mul(gap) else {
                        break;
                    };
                    let Some(cut) = tail_start.checked_sub(gap_bytes) else {
                        break;
                    };
                    if cut <= cand.offset || cut.saturating_sub(cand.offset) > head_span {
                        continue;
                    }
                    if !range_covered(extents, cand.offset, cut) {
                        continue;
                    }
                    // A real split has file data on both sides of the gap. A
                    // sub-gap of filler extends the head into zeros, and a tail
                    // that begins with filler is not a file remainder. The
                    // head's own first cluster and a short tail's zero padding
                    // are exempt: compact fixtures use repeated-byte markers.
                    let previous = cut.saturating_sub(fs.cluster_size);
                    let head_has_room = previous >= cand.offset.saturating_add(fs.cluster_size);
                    let tail_spans_cluster =
                        footer_end.saturating_sub(tail_start) >= fs.cluster_size;
                    if (head_has_room && cluster_flag(&gap_filler, base, fs.cluster_size, previous))
                        || (tail_spans_cluster
                            && cluster_flag(&gap_filler, base, fs.cluster_size, tail_start))
                    {
                        continue;
                    }
                    joins.push(FragmentJoin {
                        cut,
                        tail_start,
                        footer_end,
                        gap_clusters: gap,
                        gap_matches: gap_cluster_matches(
                            &gap_filler,
                            &gap_headers,
                            base,
                            fs.cluster_size,
                            cut,
                            tail_start,
                        ),
                        png_oracle: false,
                    });
                    // Keep the pool bounded without discarding good joins that
                    // are enumerated late (a tail far from its footer).
                    if joins.len() >= join_pool_limit {
                        joins.sort_by_key(join_rank);
                        joins.truncate(join_pool_limit / 2);
                    }
                }
            }
            let Some(previous) = tail_start.checked_sub(fs.cluster_size) else {
                break;
            };
            tail_start = previous;
        }
    }

    if opts.fragment_recovery {
        // H-30: high-entropy gaps are outside the original filler/header
        // classes but are explained by a PNG chunk CRC. These candidates are
        // ranked ahead of the weaker local signals above.
        joins.extend(png_crc_joins(
            source, cand, fs, footers, extents, opts, budget, stop,
        ));
    }

    // A real split leaves filler (or another file's header) in the gap, while
    // a footer-only splice cuts ordinary bytes out of one contiguous file.
    // Rank the explained gaps first, then prefer small gaps and early tails so
    // the deterministic work-unit budget reaches the real split.
    joins.sort_by_key(join_rank);
    joins.truncate(opts.max_frag_candidates);
    joins
}

/// Cheap rejection before the expensive head probe: if no footer can possibly
/// follow a tail within the configured gap and size bounds, the candidate is
/// not a B-06 head.
fn has_plausible_footer(
    cand: &Candidate,
    fs: &FsInfo,
    footers: &[u64],
    opts: &ScanOptions,
) -> bool {
    if footers.is_empty() || fs.cluster_size == 0 || !is_cluster_boundary(cand.offset, fs) {
        return false;
    }
    let Some(first_cut) = next_cluster_boundary(cand.offset.saturating_add(1), fs) else {
        return false;
    };
    let max_footer = cand
        .offset
        .saturating_add(opts.max_size)
        .saturating_add(opts.max_gap.saturating_mul(fs.cluster_size))
        .saturating_add(opts.max_size);
    footers
        .iter()
        .any(|&end| end > first_cut.saturating_add(fs.cluster_size) && end <= max_footer)
}

/// Deep-validation result for a candidate before it is used as a B-06 head.
///
/// H-29 makes the work-unit budget authoritative, so head selection is a
/// counted work item like a join attempt. The probe never changes the normal
/// carve output; it only decides whether the contiguous bytes already decode.
#[derive(Debug)]
struct HeadProbe {
    deep_valid: bool,
    skipped_budget: Option<(u64, u64)>,
    work_limit: Option<WorkLimit>,
}

fn head_probe_miss() -> HeadProbe {
    HeadProbe {
        deep_valid: false,
        skipped_budget: None,
        work_limit: None,
    }
}

fn head_probe_skipped_budget(offset: u64, needed: u64) -> HeadProbe {
    HeadProbe {
        deep_valid: false,
        skipped_budget: Some((offset, needed)),
        work_limit: None,
    }
}

fn head_probe_work_limit(cand: &Candidate, examined: u64, limit: u64) -> HeadProbe {
    HeadProbe {
        deep_valid: false,
        skipped_budget: None,
        work_limit: Some((cand.offset, cand.kind, examined, limit)),
    }
}

fn head_quality_ok(analysis: &Analysis, bytes_len: usize, opts: &ScanOptions) -> bool {
    analysis.end != 0
        && analysis.end <= bytes_len
        && analysis.complete
        && analysis.score >= opts.min_score
        && (analysis.end as u64) >= opts.min_size
}

fn head_deep_valid(detail: &ValidationDetail, analysis: &Analysis) -> bool {
    detail.validation == Validation::Decoded
        && Validity::from_legacy(detail.validation, analysis.status(false)) == Validity::Verified
}

/// H-30: a PNG head split inside a chunk fails deep validation but still has a
/// strong candidate oracle. Probe only the bytes already read for the head; a
/// matching gap marks this head as requiring a join attempt instead of being
/// discarded as an ordinary truncated carve.
fn png_head_needs_crc_join(source: &dyn ImageSource, cand: &Candidate, opts: &ScanOptions) -> bool {
    if !opts.fragment_recovery
        || cand.kind != FileType::Png
        || opts.validate != ValidationMode::Decode
    {
        return false;
    }
    let available = cand.extent_end.saturating_sub(cand.offset);
    let span = available.min(opts.max_size).clamp(16, PNG_ORACLE_HEAD_CAP);
    let Ok(len) = usize::try_from(span) else {
        return false;
    };
    let mut head = vec![0u8; len];
    if read_exact_at(source, &mut head, cand.offset).is_err() {
        return false;
    }
    matches!(
        png_first_crc_gap(&head, len),
        Some(PngCrcGap::Broken { .. }) | Some(PngCrcGap::Boundary { .. })
    )
}

fn deep_validate_head(
    source: &dyn ImageSource,
    cand: &Candidate,
    opts: &ScanOptions,
    budget: &MemoryBudget,
    stop: &Stop,
    validated_heads: &mut HashMap<String, bool>,
    known_hash: Option<&str>,
) -> Result<HeadProbe> {
    let available = cand.extent_end.saturating_sub(cand.offset);
    let hard_limit = available.min(opts.max_size);
    if hard_limit < 16 {
        return Ok(head_probe_miss());
    }
    let work_limit = effective_max_work(opts);
    let mut work = WorkCounter::new(work_limit);
    let limits = ValidationLimits::new(opts.max_validator_memory.min(budget.limit()));
    let mut buf_len = hard_limit.min(64 * 1024);
    let Ok(mut guard) = BudgetGuard::reserve(budget, buf_len, "deep-validating a B-06 head") else {
        return Ok(head_probe_skipped_budget(cand.offset, buf_len));
    };
    let mut bytes = Vec::new();
    let len = usize::try_from(buf_len).unwrap_or(usize::MAX);
    if bytes.try_reserve_exact(len).is_err() {
        return Ok(head_probe_miss());
    }
    bytes.resize(len, 0u8);

    loop {
        if stop.check() {
            return Ok(head_probe_miss());
        }
        read_exact_at(source, &mut bytes[..buf_len as usize], cand.offset)
            .with_context(|| format!("cannot read B-06 head at {}", cand.offset))?;
        let read_len = buf_len;
        if !work.add(read_len) {
            return Ok(head_probe_work_limit(
                cand,
                work.used().saturating_add(read_len),
                work_limit,
            ));
        }
        let at_limit = buf_len >= hard_limit;
        test_panic_probe(cand.offset, "bgc_head_analyze");
        match analyze_with_limits(
            cand.kind,
            &bytes[..buf_len as usize],
            at_limit,
            opts.pdf_eof,
            limits,
        ) {
            Need::Reject(_) => return Ok(head_probe_miss()),
            Need::More => {
                if at_limit {
                    return Ok(head_probe_miss());
                }
                let next = buf_len.saturating_mul(2).min(hard_limit);
                if next <= buf_len {
                    return Ok(head_probe_miss());
                }
                if next > budget.limit() {
                    return Ok(head_probe_skipped_budget(cand.offset, next));
                }
                drop(bytes);
                guard.release();
                guard = BudgetGuard::reserve(budget, next, "growing a B-06 head buffer")?;
                bytes = Vec::new();
                let next_len = usize::try_from(next).unwrap_or(usize::MAX);
                if bytes.try_reserve_exact(next_len).is_err() {
                    return Ok(head_probe_miss());
                }
                bytes.resize(next_len, 0u8);
                buf_len = next;
            }
            Need::Analyzed(analysis) => {
                if !head_quality_ok(&analysis, bytes.len(), opts) {
                    return Ok(head_probe_miss());
                }
                let validated = analysis.end as u64;
                let digest =
                    known_hash.map_or_else(|| sha256_hex(&bytes[..analysis.end]), str::to_string);
                if let Some(&deep_valid) = validated_heads.get(&digest) {
                    return Ok(HeadProbe {
                        deep_valid,
                        skipped_budget: None,
                        work_limit: None,
                    });
                }
                if !work.add(validated) {
                    return Ok(head_probe_work_limit(
                        cand,
                        work.used().saturating_add(validated),
                        work_limit,
                    ));
                }
                let limits = ValidationLimits::new(opts.max_validator_memory.min(budget.limit()));
                test_panic_probe(cand.offset, "bgc_head_validate");
                let detail =
                    validate_deep_detail(cand.kind, &bytes[..analysis.end], &analysis, limits);
                let deep_valid = head_deep_valid(&detail, &analysis);
                validated_heads.insert(digest, deep_valid);
                return Ok(HeadProbe {
                    deep_valid,
                    skipped_budget: None,
                    work_limit: None,
                });
            }
        }
    }
}

fn join_none() -> JoinAttempt {
    JoinAttempt {
        data: None,
        deep_checked: false,
        skipped_budget: None,
        work_limit: None,
    }
}

fn join_skipped_budget(offset: u64, total: u64) -> JoinAttempt {
    JoinAttempt {
        data: None,
        deep_checked: false,
        skipped_budget: Some((offset, total)),
        work_limit: None,
    }
}

fn join_work_limit(cand: &Candidate, examined: u64, limit: u64, deep_checked: bool) -> JoinAttempt {
    JoinAttempt {
        data: None,
        deep_checked,
        skipped_budget: None,
        work_limit: Some((cand.offset, cand.kind, examined, limit)),
    }
}

fn junction_analysis(
    analysis: &Analysis,
    bytes_len: usize,
    tail_len: u64,
    min_score: u8,
    head_len: u64,
) -> Option<(usize, u64)> {
    if !analysis.complete || analysis.end == 0 || analysis.end > bytes_len {
        return None;
    }
    if analysis.score < min_score {
        return None;
    }
    match analysis.end.checked_sub(head_len as usize) {
        Some(v) if v > 0 && v as u64 <= tail_len => Some((analysis.end, v as u64)),
        _ => None,
    }
}

/// Inputs to one B-06 stitch record, grouped for H-22 without changing the
/// order in which fragment notes, hashes and validation checks are assembled.
struct JoinedCarvedSpec<'a> {
    cand: &'a Candidate,
    join: &'a FragmentJoin,
    analysis: Analysis,
    bytes: &'a [u8],
    detail: ValidationDetail,
    validation: Validation,
    status: Status,
    validity: Validity,
    head_len: u64,
    used_tail: u64,
}

fn joined_carved_data(spec: JoinedCarvedSpec<'_>) -> CarvedData {
    let JoinedCarvedSpec {
        cand,
        join,
        mut analysis,
        bytes,
        detail,
        validation,
        status,
        validity,
        head_len,
        used_tail,
    } = spec;
    analysis.notes.push(format!(
        "B-06 deep-validated 2 fragments with a {} cluster gap",
        join.gap_clusters
    ));
    let confidence = analysis.score;
    let sha256 = sha256_hex(bytes);
    let fragments = vec![
        Fragment {
            offset: cand.offset,
            len: head_len,
        },
        Fragment {
            offset: join.tail_start,
            len: used_tail,
        },
    ];
    let output_len = analysis.end as u64;
    let ValidationDetail {
        notes: validation_notes,
        checks: detail_checks,
        validated_bytes,
        ..
    } = detail;
    let mut validation_checks =
        base_validity_checks(cand.kind, validation, status, &validation_notes);
    validation_checks.extend(detail_checks);
    CarvedData {
        candidate: cand.clone(),
        tail_trimmed_bytes: 0,
        payload: PayloadSpec::Join {
            fragments: fragments.clone(),
        },
        output_len,
        analysis,
        sha256,
        sha256_original: None,
        repaired: false,
        repair_suffix: Vec::new(),
        repair_replacement: None,
        recovered_rows: None,
        total_rows: None,
        validation,
        validity,
        validation_notes,
        validated_bytes,
        validation_checks,
        fragments,
        checks: None,
        confidence: Some(confidence),
        alternatives: None,
        join: None,
        join_alternatives: None,
    }
}

fn try_fragment_join(
    source: &dyn ImageSource,
    cand: &Candidate,
    join: &FragmentJoin,
    opts: &ScanOptions,
    budget: &MemoryBudget,
) -> Result<JoinAttempt> {
    let head_len = join.cut.saturating_sub(cand.offset);
    let tail_len = join.footer_end.saturating_sub(join.tail_start);
    if head_len == 0 || tail_len == 0 {
        return Ok(join_none());
    }
    let total = match head_len.checked_add(tail_len) {
        Some(v) if v <= opts.max_size => v,
        _ => return Ok(join_none()),
    };
    let Ok(_guard) = BudgetGuard::reserve(budget, total, "allocating a B-06 stitch buffer") else {
        return Ok(join_skipped_budget(cand.offset, total));
    };
    let work_limit = effective_max_work(opts);
    let mut work = WorkCounter::new(work_limit);
    let limits = ValidationLimits::new(opts.max_validator_memory.min(budget.limit()));
    if !work.add(total) {
        return Ok(join_work_limit(
            cand,
            work.used().saturating_add(total),
            work_limit,
            false,
        ));
    }
    let capacity = usize::try_from(total).unwrap_or(usize::MAX);
    let mut bytes = vec![0u8; capacity];
    read_exact_at(source, &mut bytes[..head_len as usize], cand.offset)
        .with_context(|| format!("cannot read B-06 head at {}", cand.offset))?;
    read_exact_at(
        source,
        &mut bytes[head_len as usize..total as usize],
        join.tail_start,
    )
    .with_context(|| format!("cannot read B-06 tail at {}", join.tail_start))?;

    let Need::Analyzed(analysis) =
        analyze_with_limits(cand.kind, &bytes, true, opts.pdf_eof, limits)
    else {
        return Ok(join_none());
    };
    let Some((end, used_tail)) =
        junction_analysis(&analysis, bytes.len(), tail_len, opts.min_score, head_len)
    else {
        return Ok(join_none());
    };
    bytes.truncate(end);
    let validated = end as u64;
    if !work.add(validated) {
        return Ok(join_work_limit(
            cand,
            work.used().saturating_add(validated),
            work_limit,
            true,
        ));
    }
    let limits = ValidationLimits::new(opts.max_validator_memory.min(budget.limit()));
    let detail = validate_deep_detail(cand.kind, &bytes, &analysis, limits);
    // H-29 supersedes the stage-1 PDF exception from E-1 for B-06: a stitch
    // is emitted only when the authoritative honesty label is `verified`.
    let status = analysis.status(false);
    let validation = detail.validation;
    let validity = Validity::from_legacy(validation, status);
    if validity != Validity::Verified {
        return Ok(JoinAttempt {
            data: None,
            deep_checked: true,
            skipped_budget: None,
            work_limit: None,
        });
    }
    let data = joined_carved_data(JoinedCarvedSpec {
        cand,
        join,
        analysis,
        bytes: &bytes,
        detail,
        validation,
        status,
        validity,
        head_len,
        used_tail,
    });
    Ok(JoinAttempt {
        data: Some(data),
        deep_checked: true,
        skipped_budget: None,
        work_limit: None,
    })
}

const MAX_FRAG_ALTERNATIVES: u64 = 16;
const MAX_JOIN_ATTEMPTS_PER_HEAD: usize = 32;

struct FragmentHeadClassification {
    required: HashMap<(u64, FileType), bool>,
    force: HashMap<(u64, FileType), bool>,
    contiguous_hashes: HashMap<(u64, FileType), String>,
    /// H-30: PNG heads whose already-read bytes contain a chunk-CRC oracle gap.
    png_crc_heads: HashSet<(u64, FileType)>,
}

fn classify_fragment_heads(
    source: &dyn ImageSource,
    candidates: &[Candidate],
    kept: &[CarvedData],
    volumes: &[FsInfo],
    opts: &ScanOptions,
) -> FragmentHeadClassification {
    let mut required: HashMap<(u64, FileType), bool> = candidates
        .iter()
        .map(|cand| ((cand.offset, cand.kind), true))
        .collect();
    // A suspicious contiguous carve is still a B-06 head even if the legacy
    // deep validator labels it `verified`: an interleaved splice can contain a
    // valid selected revision and a valid startxref while its bytes are wrong.
    let mut force: HashMap<(u64, FileType), bool> = candidates
        .iter()
        .map(|cand| ((cand.offset, cand.kind), true))
        .collect();
    for file in kept {
        let key = (file.candidate.offset, file.candidate.kind);
        let cluster_size =
            volume_at(volumes, file.candidate.offset).map_or(0, |fs| fs.cluster_size);
        let (needs_probe, forced) = if !file.fragments.is_empty() || file.repaired {
            (false, false)
        } else if file.analysis.status(file.repaired) != Status::Complete {
            (true, true)
        } else {
            let suspicious = contiguous_range_has_gap(
                source,
                cluster_size,
                file.candidate.offset,
                file.output_len,
            ) || (file.candidate.kind == FileType::Pdf
                && contiguous_range_has_multiple_footers(
                    source,
                    file.candidate.kind,
                    file.candidate.offset,
                    file.output_len,
                ));
            (suspicious, suspicious)
        };
        required.insert(key, needs_probe);
        force.insert(key, forced);
    }
    // Reuse the digest already computed by the carve stage for unchanged
    // contiguous outputs. Duplicate copies then need no second read, hash or
    // decode, which keeps an ordinary image cheap while still deep-validating
    // every distinct head.
    let contiguous_hashes: HashMap<(u64, FileType), String> = kept
        .iter()
        .filter(|file| {
            !file.repaired
                && file.fragments.is_empty()
                && matches!(
                    file.payload,
                    PayloadSpec::Range { offset, len }
                        if offset == file.candidate.offset && len == file.output_len
                )
        })
        .map(|file| {
            (
                (file.candidate.offset, file.candidate.kind),
                file.sha256.clone(),
            )
        })
        .collect();
    let png_crc_heads = if opts.fragment_recovery {
        candidates
            .iter()
            .filter(|cand| png_head_needs_crc_join(source, cand, opts))
            .map(|cand| (cand.offset, cand.kind))
            .collect()
    } else {
        HashSet::new()
    };
    FragmentHeadClassification {
        required,
        force,
        contiguous_hashes,
        png_crc_heads,
    }
}

struct FragmentScanState {
    started: Instant,
    panicked: HashSet<(u64, FileType)>,
    successes: Vec<CarvedData>,
    successful_keys: HashSet<(u64, FileType)>,
    seen_hashes: HashSet<String>,
    work_units: usize,
    candidate_limit: bool,
    time_limit: bool,
    work_limits: Vec<WorkLimit>,
    validated_heads: HashMap<String, bool>,
}

impl FragmentScanState {
    fn new(
        started: Instant,
        kept: &[CarvedData],
        opts: &ScanOptions,
        internal_errors: &[InternalError],
    ) -> Self {
        // A candidate that already panicked in the carve stage is not probed a
        // second time; the single contained panic is enough evidence for the run.
        let panicked = internal_errors
            .iter()
            .map(|error| (error.offset, error.file_type))
            .collect();
        let seen_hashes = if opts.dedupe_hashes {
            kept.iter().map(|f| f.sha256.clone()).collect()
        } else {
            HashSet::new()
        };
        Self {
            started,
            panicked,
            successes: Vec::new(),
            successful_keys: HashSet::new(),
            seen_hashes,
            work_units: 0,
            candidate_limit: false,
            time_limit: false,
            work_limits: Vec::new(),
            validated_heads: HashMap::new(),
        }
    }

    fn work_budget(&self, opts: &ScanOptions) -> usize {
        // H-29: work units are authoritative. One head probe and one join
        // attempt each consume one unit of `--max-frag-candidates`. Wall-clock
        // time is only a safety net and is reported as nondeterministic.
        opts.max_frag_candidates
    }

    fn work_exhausted(&mut self, budget: usize) -> bool {
        if self.work_units >= budget {
            self.candidate_limit = true;
            true
        } else {
            false
        }
    }

    fn time_exhausted(&mut self, opts: &ScanOptions) -> bool {
        if self.started.elapsed() >= opts.frag_time_limit {
            self.time_limit = true;
            true
        } else {
            false
        }
    }
}

/// Shared inputs for the first B-06 pass. Mutable outputs stay explicit at
/// the call site so work-budget and warning provenance remain auditable.
#[derive(Clone, Copy)]
struct FragmentProbeCtx<'a> {
    source: &'a dyn ImageSource,
    candidates: &'a [Candidate],
    volumes: &'a [FsInfo],
    opts: &'a ScanOptions,
    budget: &'a Arc<MemoryBudget>,
    stop: &'a Stop,
    classification: &'a FragmentHeadClassification,
}

fn probe_fragment_heads<'a>(
    ctx: &FragmentProbeCtx<'a>,
    state: &mut FragmentScanState,
    warnings: &mut Vec<Warning>,
    internal_errors: &mut Vec<InternalError>,
) -> Result<Vec<(&'a Candidate, &'a FsInfo)>> {
    let FragmentProbeCtx {
        source,
        candidates,
        volumes,
        opts,
        budget,
        stop,
        classification,
    } = *ctx;
    let mut heads = Vec::new();
    let work_budget = state.work_budget(opts);
    // Pass one: classify heads. Footer scanning is deliberately deferred until
    // at least one candidate fails deep validation, so an ordinary image never
    // pays for the fragment footer index.
    for cand in candidates {
        if stop.check() {
            break;
        }
        if state.work_exhausted(work_budget) {
            break;
        }
        if state.time_exhausted(opts) {
            break;
        }
        if state.panicked.contains(&(cand.offset, cand.kind)) {
            continue;
        }
        let Some(fs) = volume_at(volumes, cand.offset) else {
            continue;
        };
        if !is_cluster_boundary(cand.offset, fs) {
            continue;
        }
        let key = (cand.offset, cand.kind);
        if !classification.required.get(&key).copied().unwrap_or(true)
            && !classification.png_crc_heads.contains(&key)
        {
            continue;
        }
        let known_hash = classification.contiguous_hashes.get(&key);
        let cached_probe = known_hash
            .and_then(|hash| state.validated_heads.get(hash))
            .copied()
            .map(|deep_valid| HeadProbe {
                deep_valid,
                skipped_budget: None,
                work_limit: None,
            });
        if cached_probe.is_none() {
            state.work_units += 1;
        }
        let probe = match cached_probe {
            Some(probe) => probe,
            None => match catch_unwind(AssertUnwindSafe(|| {
                deep_validate_head(
                    source,
                    cand,
                    opts,
                    budget,
                    stop,
                    &mut state.validated_heads,
                    known_hash.map(String::as_str),
                )
            })) {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => {
                    if opts.verbose {
                        eprintln!("[-] B-06 head at {} failed: {e}", cand.offset);
                    }
                    if e.to_string().contains("--max-memory") {
                        return Err(e);
                    }
                    continue;
                }
                Err(payload) => {
                    internal_errors.push(InternalError {
                        offset: cand.offset,
                        file_type: cand.kind,
                        message: panic_message(payload.as_ref()),
                    });
                    continue;
                }
            },
        };
        if let Some((offset, needed)) = probe.skipped_budget {
            warnings.push(Warning::new(
                "CANDIDATE_SKIPPED_BUDGET",
                format!(
                    "B-06 head at byte {} needs {} which exceeds --max-memory {}",
                    offset,
                    human_size(needed),
                    human_size(budget.limit())
                ),
            ));
        }
        if let Some(limit) = probe.work_limit {
            state.work_limits.push(limit);
        }
        if probe.skipped_budget.is_some()
            || probe.work_limit.is_some()
            || (probe.deep_valid
                && !classification.force.get(&key).copied().unwrap_or(false)
                && !classification.png_crc_heads.contains(&key))
        {
            // A clean contiguous copy that deep-validates is not a fragmented
            // head; a budget-limited probe cannot safely be classified either
            // way. A suspicious splice remains a head even when deep validation
            // accepts its selected revision.
            continue;
        }
        heads.push((cand, fs));
    }
    Ok(heads)
}

fn scan_fragment_footers(
    source: &dyn ImageSource,
    extents: &[Extent],
    heads: &[(&Candidate, &FsInfo)],
    opts: &ScanOptions,
    state: &FragmentScanState,
    warnings: &mut Vec<Warning>,
) -> (HashMap<FileType, Vec<u64>>, bool) {
    let mut found: HashMap<FileType, Vec<u64>> =
        opts.types.iter().map(|&kind| (kind, Vec::new())).collect();
    let mut footer_failed = false;
    if !heads.is_empty() && !state.time_limit {
        match find_footers(source, extents, &opts.types, opts) {
            Ok(footers) => found = footers,
            Err(e) => {
                warnings.push(Warning::new(
                    "FRAGMENT_FOOTER_SCAN_FAILED",
                    format!("fragment footer scan failed: {e}"),
                ));
                footer_failed = true;
            }
        }
    }
    (found, footer_failed)
}

/// Shared inputs for one head's bounded B-06 join attempts. Mutable outputs
/// remain explicit so the attempt, warning and panic order is unchanged.
#[derive(Clone, Copy)]
struct FragmentJoinCtx<'a> {
    source: &'a dyn ImageSource,
    cand: &'a Candidate,
    fs: &'a FsInfo,
    found: &'a HashMap<FileType, Vec<u64>>,
    extents: &'a [Extent],
    opts: &'a ScanOptions,
    budget: &'a Arc<MemoryBudget>,
    stop: &'a Stop,
}

fn attempt_head_joins(
    ctx: &FragmentJoinCtx<'_>,
    state: &mut FragmentScanState,
    warnings: &mut Vec<Warning>,
    internal_errors: &mut Vec<InternalError>,
) -> Result<Option<CarvedData>> {
    let FragmentJoinCtx {
        source,
        cand,
        fs,
        found,
        extents,
        opts,
        budget,
        stop,
    } = *ctx;
    let Some(footer_hits) = found.get(&cand.kind) else {
        return Ok(None);
    };
    if !has_plausible_footer(cand, fs, footer_hits, opts) {
        return Ok(None);
    }
    let joins = candidate_joins(source, cand, fs, footer_hits, extents, opts, budget, stop);
    // H-30: a PNG CRC hit is only an oracle, not proof. Keep every distinct
    // deep-validated `(cut, gap)` solution so two good-looking joins cannot be
    // collapsed into a silent first-success `verified` result.
    let oracle_joins = joins.iter().filter(|join| join.png_oracle).count();
    let mut oracle_results: Vec<(FragmentJoin, JoinAlternative)> = Vec::new();
    let mut oracle_hashes: HashSet<(u64, u64, String)> = HashSet::new();
    let mut checks = 0u64;
    let mut alternatives = 0u64;
    let mut head_attempts = 0usize;
    let mut first_success: Option<CarvedData> = None;
    let work_budget = state.work_budget(opts);

    for join in joins {
        if stop.check() {
            break;
        }
        if state.work_exhausted(work_budget) {
            break;
        }
        if head_attempts >= MAX_JOIN_ATTEMPTS_PER_HEAD {
            break;
        }
        if state.time_exhausted(opts) {
            break;
        }
        state.work_units += 1;
        head_attempts += 1;
        let attempt = match catch_unwind(AssertUnwindSafe(|| {
            try_fragment_join(source, cand, &join, opts, budget)
        })) {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                if opts.verbose {
                    eprintln!("[-] B-06 join at {} failed: {e}", cand.offset);
                }
                if e.to_string().contains("--max-memory") {
                    return Err(e);
                }
                continue;
            }
            Err(payload) => {
                internal_errors.push(InternalError {
                    offset: cand.offset,
                    file_type: cand.kind,
                    message: panic_message(payload.as_ref()),
                });
                continue;
            }
        };
        if attempt.deep_checked {
            checks += 1;
        }
        if let Some(limit) = attempt.work_limit {
            state.work_limits.push(limit);
        }
        if let Some((offset, needed)) = attempt.skipped_budget {
            warnings.push(Warning::new(
                "CANDIDATE_SKIPPED_BUDGET",
                format!(
                    "B-06 candidate at byte {} needs {} which exceeds --max-memory {}",
                    offset,
                    human_size(needed),
                    human_size(budget.limit())
                ),
            ));
        }
        let Some(mut data) = attempt.data else {
            continue;
        };
        alternatives = alternatives.saturating_add(1);
        if join.png_oracle && data.validity == Validity::Verified {
            // Record the geometry keyed by the generated bytes, not by the
            // cluster gap alone: a different cut and gap can produce the same
            // output and must not count as a second solution.
            let key = (join.cut, join.gap_clusters, data.sha256.clone());
            if oracle_hashes.insert(key) && oracle_results.len() < MAX_FRAG_ALTERNATIVES as usize {
                oracle_results.push((
                    join,
                    JoinAlternative {
                        cut: join.cut,
                        tail_start: join.tail_start,
                        gap_clusters: join.gap_clusters,
                        sha256: data.sha256.clone(),
                        validity: data.validity,
                    },
                ));
            }
        }
        if !state.seen_hashes.insert(data.sha256.clone()) {
            continue;
        }
        data.checks = Some(checks);
        data.alternatives = Some(alternatives);
        if first_success.is_none() {
            first_success = Some(data);
        }
        if alternatives >= MAX_FRAG_ALTERNATIVES || head_attempts >= MAX_JOIN_ATTEMPTS_PER_HEAD {
            break;
        }
    }
    if oracle_joins > 0 {
        return Ok(finish_oracle_join(
            oracle_results,
            first_success,
            alternatives,
            checks,
        ));
    }
    if let Some(mut data) = first_success {
        data.alternatives = Some(alternatives);
        Ok(Some(data))
    } else {
        Ok(None)
    }
}

/// Choose the single H-30 report row for one PNG head. A unique deep-validated
/// `(cut, gap)` result remains `verified`; two or more are downgraded to
/// `suspect` and keep equally ranked alternatives without guessing a winner.
fn finish_oracle_join(
    results: Vec<(FragmentJoin, JoinAlternative)>,
    first_success: Option<CarvedData>,
    alternatives: u64,
    checks: u64,
) -> Option<CarvedData> {
    let mut results = results;
    if results.is_empty() {
        return None;
    }
    let unique = results.len() == 1;
    let alternative_rows: Vec<JoinAlternative> =
        results.iter().map(|(_, row)| row.clone()).collect();
    // Ordered candidate generation is deterministic; do not guess a winner
    // among equally deep-validated geometries.
    results.sort_by_key(|(join, row)| {
        (
            join.cut,
            join.gap_clusters,
            join.tail_start,
            row.sha256.clone(),
        )
    });
    let (join, _) = results.remove(0);
    let mut data = first_success?;
    // H-30 never changes a result that was not deeply decoded. In particular,
    // a structure-mode legacy join must keep its old `verified` contract.
    if data.validation != Validation::Decoded || data.validity != Validity::Verified {
        return Some(data);
    }
    data.join = Some(JoinEvidence {
        method: "png_chunk_crc".to_string(),
        cut: join.cut,
        tail_start: join.tail_start,
        gap_clusters: join.gap_clusters,
        unique,
    });
    if !unique {
        let count = alternative_rows.len();
        data.join_alternatives = Some(alternative_rows);
        data.validity = Validity::Suspect;
        data.validation = Validation::Structure;
        data.validation_checks.push(ValidationCheck::new(
            "png_crc_join_ambiguous",
            Some(false),
            format!("{count} deep-validated PNG chunk-CRC joins"),
        ));
    }
    data.checks = Some(checks);
    data.alternatives = Some(alternatives);
    Some(data)
}

fn append_fragment_warnings(state: &FragmentScanState, warnings: &mut Vec<Warning>) {
    if let Some(&(offset, kind, examined, limit)) = state.work_limits.first() {
        warnings.push(Warning::new(
            "WORK_LIMIT",
            format!(
                "{} B-06 probe(s)/join(s) exceeded --max-work-per-candidate {} bytes; first at byte {} ({}) examined {} bytes",
                state.work_limits.len(),
                limit,
                offset,
                kind.label(),
                examined
            ),
        ));
    }
    if state.candidate_limit {
        warnings.push(Warning::new(
            "FRAGMENT_CANDIDATE_LIMIT",
            "B-06 stopped after reaching the authoritative --max-frag-candidates work-unit budget",
        ));
    }
    if state.time_limit {
        warnings.push(Warning::new(
            "FRAGMENT_TIME_LIMIT",
            "B-06 stopped after reaching the wall-clock --frag-time-limit safety net",
        ));
    }
}

fn finalize_fragment_files(mut kept: Vec<CarvedData>, state: FragmentScanState) -> FragmentResult {
    kept.retain(|f| {
        let replaced = state
            .successful_keys
            .contains(&(f.candidate.offset, f.candidate.kind));
        !replaced
    });
    kept.extend(state.successes);
    kept.sort_by_key(|f| (f.candidate.offset, f.candidate.kind as u8));
    FragmentResult {
        files: kept,
        nondeterministic_stop: state.time_limit,
    }
}

/// Shared inputs for the whole B-06 pass. The owned `kept` vector and mutable
/// diagnostics stay explicit so replacement and warning order are unchanged.
#[derive(Clone, Copy)]
struct FragmentRecoveryCtx<'a> {
    source: &'a dyn ImageSource,
    candidates: &'a [Candidate],
    extents: &'a [Extent],
    volumes: &'a [FsInfo],
    opts: &'a ScanOptions,
    budget: &'a Arc<MemoryBudget>,
    stop: &'a Stop,
}

fn recover_fragments(
    ctx: &FragmentRecoveryCtx<'_>,
    kept: Vec<CarvedData>,
    warnings: &mut Vec<Warning>,
    internal_errors: &mut Vec<InternalError>,
) -> Result<FragmentResult> {
    let FragmentRecoveryCtx {
        source,
        candidates,
        extents,
        volumes,
        opts,
        budget,
        stop,
    } = *ctx;
    let started = Instant::now();
    if volumes.is_empty() {
        warnings.push(Warning::new(
            "FRAGMENT_RECOVERY_SKIPPED",
            "B-06 needs a detected volume with cluster boundaries; no stitching attempted",
        ));
        return Ok(FragmentResult {
            files: kept,
            nondeterministic_stop: false,
        });
    }
    let classification = classify_fragment_heads(source, candidates, &kept, volumes, opts);
    let mut state = FragmentScanState::new(started, &kept, opts, internal_errors);
    let probe_ctx = FragmentProbeCtx {
        source,
        candidates,
        volumes,
        opts,
        budget,
        stop,
        classification: &classification,
    };
    let heads = probe_fragment_heads(&probe_ctx, &mut state, warnings, internal_errors)?;
    let (found, footer_failed) =
        scan_fragment_footers(source, extents, &heads, opts, &state, warnings);

    // Pass two: bounded join attempts for heads that actually failed deep
    // validation. The work-unit budget stays authoritative across both passes.
    if !footer_failed && !state.time_limit {
        for (cand, fs) in heads {
            if stop.check() {
                break;
            }
            if state.work_exhausted(state.work_budget(opts)) {
                break;
            }
            if state.time_exhausted(opts) {
                break;
            }
            let join_ctx = FragmentJoinCtx {
                source,
                cand,
                fs,
                found: &found,
                extents,
                opts,
                budget,
                stop,
            };
            if let Some(data) =
                attempt_head_joins(&join_ctx, &mut state, warnings, internal_errors)?
            {
                state.successful_keys.insert((cand.offset, cand.kind));
                state.successes.push(data);
            }
        }
    }

    append_fragment_warnings(&state, warnings);
    Ok(finalize_fragment_files(kept, state))
}

fn collect_outcomes(
    outcomes: Vec<CarveOutcome>,
    opts: &ScanOptions,
    stats: &mut ScanStats,
    warnings: &mut Vec<Warning>,
    internal_errors: &mut Vec<InternalError>,
) -> (Vec<CarvedData>, Vec<WorkLimit>, Vec<LowScored>) {
    let mut candidates: Vec<CarvedData> = Vec::new();
    let mut work_limited: Vec<WorkLimit> = Vec::new();
    let mut low_scored: Vec<LowScored> = Vec::new();
    for outcome in outcomes {
        match outcome {
            CarveOutcome::Carved(data) => candidates.push(*data),
            CarveOutcome::InternalError {
                offset,
                kind,
                message,
            } => internal_errors.push(InternalError {
                offset,
                file_type: kind,
                message,
            }),
            CarveOutcome::SkippedBudget {
                offset,
                kind,
                needed,
            } => {
                warnings.push(Warning::new(
                    "CANDIDATE_SKIPPED_BUDGET",
                    format!(
                        "candidate at byte {} ({}) needs {} which exceeds --max-memory",
                        offset,
                        kind.label(),
                        human_size(needed)
                    ),
                ));
            }
            CarveOutcome::WorkLimit {
                offset,
                kind,
                examined,
                limit,
            } => work_limited.push((offset, kind, examined, limit)),
            CarveOutcome::TimeLimit => {}
            CarveOutcome::Rejected(reason) => {
                stats.rejected += 1;
                if opts.verbose {
                    eprintln!("[-] reject: {reason}");
                }
            }
            CarveOutcome::TooSmall(size) => {
                stats.below_min_size += 1;
                if opts.verbose {
                    eprintln!("[-] below --min-size {}: {size}", opts.min_size);
                }
            }
            CarveOutcome::LowScore {
                offset,
                kind,
                score,
            } => {
                stats.below_min_score += 1;
                low_scored.push((offset, kind, score));
                if opts.verbose {
                    eprintln!(
                        "[-] below --min-score {}: {score} at byte {offset} ({})",
                        opts.min_score,
                        kind.label()
                    );
                }
            }
            CarveOutcome::NotComplete => {
                stats.skipped_complete_only += 1;
                if opts.verbose {
                    eprintln!("[-] truncated file skipped (--complete-only)");
                }
            }
            CarveOutcome::TooLargeTruncated { offset, kind, size } => {
                stats.too_large_truncated += 1;
                warnings.push(Warning::for_candidate(
                    "TRUNCATED_TOO_LARGE",
                    format!(
                        "truncated {} at byte {offset} is {size} byte(s), exceeding --max-truncated-size {}",
                        kind.label(),
                        opts.max_truncated_size
                    ),
                    kind,
                    offset,
                    size,
                ));
                if opts.verbose {
                    eprintln!(
                        "[-] truncated file {size} at byte {offset} exceeds --max-truncated-size {}",
                        opts.max_truncated_size
                    );
                }
            }
        }
    }
    (candidates, work_limited, low_scored)
}

fn suppressed_flags(
    candidates: &[CarvedData],
    stats: &mut ScanStats,
    opts: &ScanOptions,
) -> Vec<bool> {
    let mut keep = vec![true; candidates.len()];
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by_key(|&i| {
        (
            candidates[i].candidate.offset,
            std::cmp::Reverse(candidates[i].analysis.end),
        )
    });
    let mut stack: Vec<usize> = Vec::new();
    for i in order {
        let child_start = candidates[i].candidate.offset;
        let child_end = child_start.saturating_add(candidates[i].analysis.end as u64);
        while let Some(&top) = stack.last() {
            let parent_end = candidates[top]
                .candidate
                .offset
                .saturating_add(candidates[top].analysis.end as u64);
            if parent_end <= child_start {
                stack.pop();
            } else {
                break;
            }
        }
        let parent = stack.iter().rev().copied().find(|&j| {
            let p = &candidates[j];
            let p_start = p.candidate.offset;
            let p_end = p_start.saturating_add(p.analysis.end as u64);
            p.analysis.complete
                && p.analysis.end > candidates[i].analysis.end
                && p_start <= child_start
                && p_end >= child_end
        });
        if let Some(j) = parent {
            keep[i] = false;
            stats.suppressed_nested += 1;
            if opts.verbose {
                eprintln!(
                    "[-] nested object {} at offset {} suppressed inside {} at offset {}",
                    candidates[i].candidate.kind.label(),
                    child_start,
                    candidates[j].candidate.kind.label(),
                    candidates[j].candidate.offset
                );
            }
            continue;
        }
        stack.push(i);
    }
    keep
}

fn dedupe_candidates(
    candidates: Vec<CarvedData>,
    stats: &mut ScanStats,
    opts: &ScanOptions,
) -> Vec<CarvedData> {
    let mut seen: HashMap<String, u64> = HashMap::new();
    let mut kept = Vec::with_capacity(candidates.len());
    for data in candidates {
        if let Some(prev) = seen.get(&data.sha256) {
            stats.duplicate_hashes += 1;
            if opts.verbose {
                eprintln!(
                    "[-] duplicate hash of #{prev:05} at offset {}",
                    data.candidate.offset
                );
            }
        } else {
            let seq = kept.len() as u64 + 1;
            seen.insert(data.sha256.clone(), seq);
            kept.push(data);
        }
    }
    kept
}

fn work_limit_warning(first: &WorkLimit, count: usize) -> Warning {
    let (offset, kind, examined, limit) = *first;
    Warning::new(
        "WORK_LIMIT",
        format!(
            "{} candidate(s) exceeded --max-work-per-candidate {} bytes;                  first at byte {} ({}) examined {} bytes",
            count,
            limit,
            offset,
            kind.label(),
            examined
        ),
    )
}

/// H-02: a low score must never silently discard evidence. This warning is
/// deliberately not in `Warning::incomplete()`: the run still finished, and
/// the default `--min-score` semantics stay unchanged (§9 Q3).
fn low_score_warning(low: &[LowScored], min_score: u8) -> Warning {
    let offsets: Vec<String> = low
        .iter()
        .take(3)
        .map(|(offset, kind, score)| format!("{offset} ({} score {score})", kind.label()))
        .collect();
    Warning::new(
        "BELOW_MIN_SCORE",
        format!(
            "{} candidate(s) scored below --min-score {min_score} and were not written; first offsets: {}",
            low.len(),
            offsets.join(", ")
        ),
    )
}

fn postfilter(
    outcomes: Vec<CarveOutcome>,
    opts: &ScanOptions,
    stats: &mut ScanStats,
    warnings: &mut Vec<Warning>,
    internal_errors: &mut Vec<InternalError>,
) -> Vec<CarvedData> {
    let (mut candidates, work_limited, mut low_scored) =
        collect_outcomes(outcomes, opts, stats, warnings, internal_errors);
    if opts.suppress_nested && candidates.len() > 1 {
        let keep = suppressed_flags(&candidates, stats, opts);
        let mut idx = 0usize;
        candidates.retain(|_c| {
            let keep_this = keep[idx];
            idx += 1;
            keep_this
        });
    }
    if opts.dedupe_hashes {
        candidates = dedupe_candidates(candidates, stats, opts);
    }
    if opts.verified_only {
        let before = candidates.len();
        candidates.retain(|data| data.validity == Validity::Verified);
        stats.skipped_verified_only += before - candidates.len();
        if opts.verbose && before > candidates.len() {
            eprintln!(
                "[-] {} candidate(s) skipped (--verified-only)",
                before - candidates.len()
            );
        }
    }
    if let Some(first) = work_limited.first() {
        warnings.push(work_limit_warning(first, work_limited.len()));
    }
    if !low_scored.is_empty() {
        low_scored.sort_by_key(|(offset, kind, _)| (*offset, *kind as u8));
        warnings.push(low_score_warning(&low_scored, opts.min_score));
    }
    candidates
}

/// Stream one source range into a file and a live SHA-256 context.
fn stream_range(
    source: &dyn ImageSource,
    file: &mut File,
    hasher: &mut sha2::Sha256,
    offset: u64,
    len: u64,
    scratch: &mut [u8],
) -> Result<()> {
    let mut remaining = len;
    let mut at = offset;
    while remaining > 0 {
        let chunk = usize::try_from(remaining.min(scratch.len() as u64)).unwrap_or(scratch.len());
        if chunk == 0 {
            bail!("write buffer is empty");
        }
        read_exact_at(source, &mut scratch[..chunk], at)
            .with_context(|| format!("cannot re-read carved bytes at offset {at}"))?;
        file.write_all(&scratch[..chunk])
            .with_context(|| "cannot stream carved bytes to a temporary file")?;
        hasher.update(&scratch[..chunk]);
        at = at.saturating_add(chunk as u64);
        remaining -= chunk as u64;
    }
    Ok(())
}

struct OutputPaths {
    seq: u64,
    rel_path: String,
    path: PathBuf,
    tmp: PathBuf,
}

fn output_paths(
    idx: usize,
    data: &CarvedData,
    volumes: &[FsInfo],
    multi: bool,
    out_dir: &Path,
) -> OutputPaths {
    let seq = idx as u64 + 1;
    let filename = format!(
        "{:05}_{}_{:016x}.{}",
        seq,
        data.candidate.kind.ext(),
        data.candidate.offset,
        data.candidate.kind.ext()
    );
    let rel_path = if multi {
        let part = volumes
            .iter()
            .position(|v| {
                data.candidate.offset >= v.offset
                    && data.candidate.offset < v.offset.saturating_add(v.length)
            })
            .map_or(1, |i| volumes[i].partition_index.unwrap_or(i + 1));
        format!("part{part}/{filename}")
    } else {
        filename.clone()
    };
    let path = out_dir.join(&rel_path);
    let tmp = path.with_extension(format!(
        "{}.tmp{}-{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or("bin"),
        std::process::id(),
        seq
    ));
    OutputPaths {
        seq,
        rel_path,
        path,
        tmp,
    }
}

fn stream_file_digest(
    source: &dyn ImageSource,
    tmp: &Path,
    data: &CarvedData,
    scratch: &mut [u8],
) -> Result<String> {
    let mut file = File::create(tmp)
        .with_context(|| format!("cannot create temporary file '{}'", tmp.display()))?;
    let mut hasher = sha2::Sha256::new();
    if let Some(bytes) = data.repair_replacement.as_deref() {
        file.write_all(bytes)
            .with_context(|| "cannot write cropped repair output")?;
        hasher.update(bytes);
    } else {
        match &data.payload {
            PayloadSpec::Range { offset, len } => {
                stream_range(source, &mut file, &mut hasher, *offset, *len, scratch)?;
            }
            PayloadSpec::Join { fragments } => {
                for fragment in fragments {
                    stream_range(
                        source,
                        &mut file,
                        &mut hasher,
                        fragment.offset,
                        fragment.len,
                        scratch,
                    )?;
                }
            }
        }
    }
    if !data.repair_suffix.is_empty() {
        file.write_all(&data.repair_suffix)
            .with_context(|| "cannot append repair suffix")?;
        hasher.update(&data.repair_suffix);
    }
    file.sync_all()
        .with_context(|| format!("cannot flush temporary file '{}'", tmp.display()))?;
    drop(file);
    Ok(hex(&hasher.finalize()))
}

fn source_changed_warning(data: &CarvedData, rel_path: &str, digest: &str) -> Warning {
    Warning::new(
        "SOURCE_CHANGED",
        format!(
            "source bytes at offset {} changed before writing {}; expected {}, wrote {}",
            data.candidate.offset, rel_path, data.sha256, digest
        ),
    )
}

fn validity_reason_code(data: &CarvedData) -> String {
    if data.join.as_ref().is_some_and(|join| !join.unique) {
        return "ambiguous_png_crc_join".into();
    }
    match data.validity {
        Validity::Verified => "validator_passed".into(),
        Validity::Structural => {
            if data.validation == Validation::Decoded {
                "decoded_without_full_proof".into()
            } else {
                "structure_only".into()
            }
        }
        Validity::Suspect => data
            .validation_checks
            .iter()
            .find(|check| check.ok == Some(false))
            .map_or_else(
                || "integrity_check_failed".into(),
                |check| format!("failed_check:{}", check.name),
            ),
        Validity::Partial => "truncated_prefix".into(),
        Validity::Repaired => "repair_revalidated".into(),
    }
}

/// Counters for the optional deleted-metadata pass.
#[derive(Debug, Default, Clone, Copy)]
pub struct MetadataPassStats {
    /// Metadata candidates examined.
    pub candidates: usize,
    /// New rows written by the pass.
    pub added: usize,
    /// Existing rows annotated with additive metadata fields.
    pub attached: usize,
    /// Candidates suppressed because their content hash was already known.
    pub duplicates: usize,
    /// Candidates that could not be parsed or validated.
    pub parse_failed: usize,
    /// Candidates using unsupported metadata features.
    pub unsupported: usize,
    /// True when the pass stopped at its time limit.
    pub time_limited: bool,
    /// True when the total materialisation budget stopped the pass.
    pub memory_limited: bool,
    /// True when the candidate budget stopped the pass.
    pub candidate_limited: bool,
    /// True when `--max-output` stopped the pass.
    pub output_limited: bool,
}

/// One metadata result, either a new row or additive fields for an existing one.
#[derive(Debug, Clone)]
pub struct MetadataResult {
    /// The reconstructed file row (new or additive only).
    pub file: CarvedFile,
    /// Index of an existing row to annotate, or `None` for a new row.
    pub attach_to: Option<usize>,
}

/// Bundled inputs for [`run_metadata_pass`].
///
/// The pass has grown beyond a readable argument list; grouping the inputs
/// also keeps the output-budget baseline together with the write path.
pub struct MetadataPassSpec<'a> {
    /// Physical image source, shared with the normal scan.
    pub source: &'a dyn ImageSource,
    /// Volumes selected for this run.
    pub volumes: &'a [FsInfo],
    /// Rows produced by the normal and free-stream passes.
    pub existing: &'a [CarvedFile],
    /// Effective scan options (all CLI filters apply here too).
    pub opts: &'a ScanOptions,
    /// Directory for newly written metadata rows.
    pub out_dir: &'a Path,
    /// First sequence number for newly written rows.
    pub start_seq: u64,
    /// Hashes already present in the report, updated in place.
    pub known: &'a mut HashSet<String>,
    /// Bytes already written by earlier passes, charged to `--max-output`.
    pub initial_output_bytes: u64,
}

fn metadata_limits(opts: &ScanOptions) -> MetadataLimits {
    let mut limits = DEFAULT_METADATA_LIMITS;
    // `0` is the existing CLI spelling of "unlimited" for memory/output;
    // candidate limits use the scan-wide effective default.
    let memory = if opts.max_memory == 0 {
        u64::MAX
    } else {
        opts.max_memory
    };
    let output = effective_max_output(opts);
    limits.min_size = opts.min_size.max(limits.min_size);
    limits.max_memory = memory;
    limits.max_total_bytes = memory;
    limits.max_output_bytes = output;
    limits.max_candidates = effective_max_candidates(opts);
    limits.max_entries = limits
        .max_entries
        .min(usize::try_from(limits.max_candidates).unwrap_or(usize::MAX));
    limits.max_bytes = limits.max_bytes.min(memory).min(output);
    limits.max_data_bytes = limits.max_data_bytes.min(memory).min(output);
    limits.max_work_bytes = limits.max_work_bytes.min(memory);
    limits.max_mft_scan_bytes = limits.max_mft_scan_bytes.min(memory);
    limits.max_size = limits.max_size.min(memory).min(opts.max_size);
    limits.time_budget = opts.time_budget;
    limits
}

fn metadata_cluster(offset: u64, volume: &FsInfo) -> Option<u64> {
    let rel = offset.checked_sub(volume.data_start)?;
    if volume.cluster_size == 0 || rel % volume.cluster_size != 0 {
        return None;
    }
    let index = rel / volume.cluster_size;
    Some(match volume.kind {
        crate::fs::FsKind::Ntfs => index,
        _ => index.saturating_add(2),
    })
}

fn metadata_checks(entry: &fsx::DeletedEntry) -> Vec<ValidationCheck> {
    let mut checks = Vec::new();
    checks.push(ValidationCheck::new(
        "metadata_source",
        Some(true),
        entry.metadata_source,
    ));
    match entry.metadata_source {
        "fat32_dir_entry" => {
            if entry.lfn_recovered || entry.lfn_checksum_mismatch {
                checks.push(ValidationCheck::new(
                    "lfn_checksum",
                    Some(!entry.lfn_checksum_mismatch),
                    if entry.lfn_checksum_mismatch {
                        "LFN checksum did not bind to the recovered 8.3 name; the long name was dropped"
                    } else {
                        "LFN checksum bound to the original 8.3 name"
                    },
                ));
            }
        }
        _ => {
            checks.push(ValidationCheck::new(
                "fixup",
                Some(true),
                "update-sequence fixups applied; mismatching records skipped",
            ));
            checks.push(ValidationCheck::new(
                "runlist_bounds",
                Some(!entry.notes.iter().any(|n| n == "runlist_out_of_bounds")),
                "runlist cluster bounds checked",
            ));
        }
    }
    if entry.metadata_source == "fat32_dir_entry" || entry.metadata_source == "ntfs_deleted_mft" {
        checks.push(ValidationCheck::new(
            "overwritten_clusters",
            Some(entry.overwritten_clusters == 0),
            format!(
                "{} cluster(s)/run(s) marked allocated after deletion",
                entry.overwritten_clusters
            ),
        ));
    }
    checks
}

fn materialize_metadata(
    source: &dyn ImageSource,
    entry: &fsx::DeletedEntry,
    limits: &MetadataLimits,
    budget: u64,
) -> (Vec<u8>, Vec<Fragment>, bool, bool) {
    let mut data = Vec::new();
    let mut fragments = Vec::new();
    let mut read_failed = false;
    let mut budget_limited = false;
    let mut remaining = budget;
    for piece in &entry.pieces {
        let allowed = piece.length.min(limits.max_bytes).min(remaining);
        if piece.length > allowed {
            budget_limited = true;
        }
        let length = usize::try_from(allowed).unwrap_or(usize::MAX);
        if length == 0 {
            continue;
        }
        let added = match piece.kind {
            MetadataPieceKind::Physical => {
                let Some(offset) = piece.physical_start else {
                    read_failed = true;
                    continue;
                };
                let mut buf = vec![0u8; length];
                if read_exact_at(source, &mut buf, offset).is_err() {
                    read_failed = true;
                }
                let added = buf.len();
                fragments.push(Fragment {
                    offset,
                    len: added as u64,
                });
                data.extend_from_slice(&buf);
                added
            }
            MetadataPieceKind::Sparse => {
                let end = data.len().saturating_add(length);
                data.resize(end, 0);
                length
            }
            MetadataPieceKind::Resident => {
                if let Some(bytes) = &piece.resident_bytes {
                    let added = bytes.len().min(length);
                    if let Some(prefix) = bytes.get(..added) {
                        data.extend_from_slice(prefix);
                    }
                    added
                } else {
                    read_failed = true;
                    continue;
                }
            }
        };
        remaining = remaining.saturating_sub(added as u64);
    }
    (data, fragments, read_failed, budget_limited)
}

/// Recover deleted FAT32/NTFS entries and turn them into report rows.
///
/// The pass is deliberately additive: it never rewrites results from the normal
/// scan, and metadata alone never promotes a file to `verified`.
pub fn run_metadata_pass(
    spec: MetadataPassSpec<'_>,
) -> Result<(Vec<MetadataResult>, MetadataPassStats, Vec<Warning>)> {
    let MetadataPassSpec {
        source,
        volumes,
        existing,
        opts,
        out_dir,
        start_seq,
        known,
        initial_output_bytes,
    } = spec;
    let limits = metadata_limits(opts);
    let deadline = if limits.time_budget.is_zero() {
        None
    } else {
        Instant::now().checked_add(limits.time_budget)
    };
    let mut stats = MetadataPassStats::default();
    let mut warnings = Vec::new();
    let mut files = Vec::new();
    let mut seq = start_seq;
    let mut written = initial_output_bytes;
    let mut materialized = 0u64;
    let vlimits = ValidationLimits::new(opts.max_validator_memory);
    if opts.verified_only {
        warnings.push(Warning::new(
            "METADATA_VERIFIED_ONLY",
            "deleted metadata cannot establish verified validity, so no metadata rows were written",
        ));
        return Ok((files, stats, warnings));
    }

    for volume in volumes {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            stats.time_limited = true;
            break;
        }
        let max_len = source
            .len()
            .saturating_sub(volume.offset)
            .min(volume.length);
        let ctx = VolumeCtx {
            source,
            offset: volume.offset,
            max_len,
            partition_index: volume.partition_index,
            partition_type: volume.partition_type.clone(),
        };
        let entries = match catch_unwind(AssertUnwindSafe(|| fsx::deleted_entries(&ctx, &limits))) {
            Ok(Ok(entries)) => entries,
            Ok(Err(_)) => {
                stats.parse_failed = stats.parse_failed.saturating_add(1);
                continue;
            }
            Err(_) => {
                stats.parse_failed = stats.parse_failed.saturating_add(1);
                warnings.push(Warning::new(
                    "METADATA_PARSE_FAILED",
                    "deleted-metadata parser panic was contained for one volume",
                ));
                continue;
            }
        };

        for entry in entries {
            if deadline.is_some_and(|d| Instant::now() >= d) {
                stats.time_limited = true;
                break;
            }
            if stats.candidates as u64 >= limits.max_candidates {
                stats.candidate_limited = true;
                break;
            }
            if limits.max_output_bytes != u64::MAX && written >= limits.max_output_bytes {
                stats.output_limited = true;
                warnings.push(output_limit_warning(
                    stats.candidates,
                    written,
                    limits.max_output_bytes,
                ));
                return Ok((files, stats, warnings));
            }
            let candidate_budget = limits.max_total_bytes.saturating_sub(materialized);
            if candidate_budget == 0 {
                stats.memory_limited = true;
                warnings.push(Warning::new(
                    "METADATA_MEMORY_LIMIT",
                    format!(
                        "deleted-metadata pass reached its {} total materialisation limit",
                        human_size(limits.max_total_bytes)
                    ),
                ));
                return Ok((files, stats, warnings));
            }
            stats.candidates = stats.candidates.saturating_add(1);
            if entry.unsupported {
                stats.unsupported = stats.unsupported.saturating_add(1);
            }
            let (data, fragments, read_failed, budget_limited) =
                materialize_metadata(source, &entry, &limits, candidate_budget);
            materialized = materialized.saturating_add(data.len() as u64);
            if data.len() < limits.min_size as usize {
                stats.parse_failed = stats.parse_failed.saturating_add(1);
                continue;
            }
            let kind = FileType::all()
                .into_iter()
                .find(|kind| data.starts_with(kind.header()));
            let Some(kind) = kind else {
                stats.parse_failed = stats.parse_failed.saturating_add(1);
                continue;
            };
            if !opts.types.contains(&kind) {
                continue;
            }
            // A deleted-entry size is authoritative: the reconstructed buffer
            // is exact, so footer parsers must not ask for trailing bytes.
            let need = analyze_with_limits(kind, &data, true, opts.pdf_eof, vlimits);
            let analysis = match need {
                Need::Analyzed(analysis) => analysis,
                Need::More | Need::Reject(_) => {
                    stats.parse_failed = stats.parse_failed.saturating_add(1);
                    continue;
                }
            };
            let prefix = analysis.end.min(data.len());
            if !analysis.complete && prefix as u64 > opts.max_truncated_size {
                stats.parse_failed = stats.parse_failed.saturating_add(1);
                warnings.push(Warning::for_candidate(
                    "TRUNCATED_TOO_LARGE",
                    format!(
                        "deleted metadata at byte {} is {prefix} byte(s), exceeding --max-truncated-size {}",
                        entry.record_offset.unwrap_or(0),
                        opts.max_truncated_size
                    ),
                    kind,
                    entry.record_offset.unwrap_or(0),
                    prefix as u64,
                ));
                continue;
            }
            if analysis.score < opts.min_score || (opts.complete_only && !analysis.complete) {
                continue;
            }
            let checked = data.get(..prefix).unwrap_or(&data);
            let detail = validate_deep_detail(kind, checked, &analysis, vlimits);
            let hash = sha256_hex(&data);
            // Resident and sparse bytes have no stable physical start. Never
            // annotate an unrelated offset-0 carve with those fields.
            let attachable = !entry.has_nonphysical_pieces() && !fragments.is_empty();
            let physical = fragments
                .first()
                .map(|f| f.offset)
                .or(entry.record_offset)
                .unwrap_or(0);
            let size = entry.total_length().max(data.len() as u64);
            // Annotation is decided by location, not content hash: when the normal
            // pass already carved the same bytes, keep its row and add only the
            // metadata fields instead of dropping the candidate as a duplicate.
            let attach_to = attachable
                .then(|| {
                    existing
                        .iter()
                        .position(|row| row.offset == physical && row.size == size)
                })
                .flatten();
            let duplicate = !known.insert(hash.clone());
            if attach_to.is_none() && duplicate {
                stats.duplicates = stats.duplicates.saturating_add(1);
                continue;
            }

            let downgrade = entry.unsupported
                || entry.overwritten_clusters > 0
                || entry.lfn_checksum_mismatch
                || read_failed
                || budget_limited
                || entry.has_nonphysical_pieces()
                || entry.notes.iter().any(|n| {
                    matches!(
                        n.as_str(),
                        "uninitialized_tail" | "truncated_by_volume" | "runlist_short"
                    )
                });
            let (mut validation, mut status, mut validity) = (
                detail.validation,
                analysis.status(false),
                Validity::from_legacy(detail.validation, analysis.status(false)),
            );
            if validation == Validation::Decoded {
                validation = Validation::Structure;
            }
            if validity == Validity::Verified {
                validity = Validity::Structural;
            }
            let reason = if downgrade {
                validation = Validation::Partial;
                status = Status::Truncated;
                validity = Validity::Partial;
                if entry.unsupported {
                    "unsupported"
                } else if entry.overwritten_clusters > 0 {
                    "overwritten_clusters"
                } else if read_failed {
                    "metadata_read_failed"
                } else {
                    "metadata_validation_incomplete"
                }
                .to_string()
            } else {
                validity_reason_code_raw(validation, status, validity, &detail.checks)
            };

            let mut notes = entry.notes.clone();
            notes.extend(detail.notes.iter().cloned());
            if budget_limited {
                notes.push("truncated_by_limits".to_string());
            }
            if downgrade && !notes.iter().any(|n| n == "metadata_downgraded") {
                notes.push("metadata_downgraded".to_string());
            }
            // Reserve the whole write before touching the filesystem: a single
            // write must never push the run past --max-output.
            if attach_to.is_none() && limits.max_output_bytes != u64::MAX {
                let needed = written.saturating_add(data.len() as u64);
                if needed > limits.max_output_bytes {
                    stats.output_limited = true;
                    warnings.push(output_limit_warning(
                        stats.candidates,
                        written,
                        limits.max_output_bytes,
                    ));
                    return Ok((files, stats, warnings));
                }
                written = needed;
            }
            let (row_seq, path) = match attach_to {
                Some(index) => (
                    existing.get(index).map_or(seq, |row| row.seq),
                    existing
                        .get(index)
                        .map_or_else(String::new, |row| row.path.clone()),
                ),
                None => {
                    let filename =
                        format!("{:05}_{}_{:016x}.{}", seq, kind.ext(), physical, kind.ext());
                    fs::write(out_dir.join(&filename), &data).with_context(|| {
                        format!(
                            "cannot write metadata result '{filename}' to '{}'",
                            out_dir.display()
                        )
                    })?;
                    (seq, filename)
                }
            };
            let (lba, cluster, partition_index) = (
                physical / u64::from(volume.bytes_per_sector.max(1)),
                metadata_cluster(physical, volume),
                volume.partition_index,
            );
            files.push(MetadataResult {
                attach_to,
                file: CarvedFile {
                    seq: row_seq,
                    file_type: kind,
                    offset: physical,
                    size,
                    carved_size: data.len() as u64,
                    sha256: hash,
                    status,
                    score: analysis.score,
                    width: analysis.width,
                    height: analysis.height,
                    repaired: false,
                    path,
                    notes,
                    validation,
                    validity,
                    validity_reason: reason,
                    tail_trimmed_bytes: 0,
                    aligned: aligned(physical, opts.align, volumes),
                    nested_in: None,
                    sha256_output: None,
                    repair_validated: false,
                    recovered_rows: None,
                    total_rows: None,
                    recovered_fraction: None,
                    validated_bytes: detail.validated_bytes,
                    lba,
                    cluster,
                    partition_index,
                    extent_id: 0,
                    allocated: Some(false),
                    source: "metadata".to_string(),
                    fragments,
                    validation_checks: detail
                        .checks
                        .into_iter()
                        .chain(metadata_checks(&entry))
                        .collect(),
                    checks: None,
                    confidence: None,
                    alternatives: None,
                    join: None,
                    join_alternatives: None,
                    sha256_original: None,
                    original_name: entry.name.clone(),
                    mtime: entry.mtime.clone(),
                    contiguous_assumed: (entry.metadata_source == "fat32_dir_entry" && attachable)
                        .then_some(true),
                    overwritten_clusters: Some(entry.overwritten_clusters),
                    metadata_notes: Some(entry.notes.clone()),
                    sparse_bytes: Some(entry.sparse_bytes()),
                },
            });
            if attach_to.is_some() {
                stats.attached = stats.attached.saturating_add(1);
            } else {
                seq = seq.saturating_add(1);
                stats.added = stats.added.saturating_add(1);
            }
        }
    }
    if stats.time_limited {
        warnings.push(Warning::new(
            "METADATA_TIME_LIMIT",
            "deleted-metadata pass stopped at its time limit",
        ));
    }
    if stats.candidate_limited {
        warnings.push(Warning::new(
            "METADATA_CANDIDATE_LIMIT",
            format!(
                "deleted-metadata pass stopped at its {} candidate limit",
                limits.max_candidates
            ),
        ));
    }
    if stats.parse_failed > 0 {
        warnings.push(Warning::new(
            "METADATA_PARSE_FAILED",
            format!(
                "{} deleted metadata candidate(s) could not be parsed",
                stats.parse_failed
            ),
        ));
    }
    if stats.unsupported > 0 {
        warnings.push(Warning::new(
            "METADATA_UNSUPPORTED",
            format!(
                "{} deleted metadata candidate(s) used unsupported features and were not verified",
                stats.unsupported
            ),
        ));
    }
    Ok((files, stats, warnings))
}

fn validity_reason_code_raw(
    validation: Validation,
    status: Status,
    validity: Validity,
    checks: &[ValidationCheck],
) -> String {
    if status == Status::Truncated {
        return "truncated_prefix".to_string();
    }
    if validity == Validity::Suspect {
        return checks
            .iter()
            .find(|check| check.ok == Some(false))
            .map_or_else(
                || "integrity_check_failed".to_string(),
                |check| format!("failed_check:{}", check.name),
            );
    }
    match validation {
        Validation::Decoded => "decoded".to_string(),
        Validation::Structure => "structural".to_string(),
        Validation::Partial => "partial_validation".to_string(),
        Validation::Unverified => "unverified".to_string(),
    }
}

fn build_carved_file(
    seq: u64,
    data: &CarvedData,
    rel_path: String,
    volumes: &[FsInfo],
    mode: ScanMode,
    opts: &ScanOptions,
) -> CarvedFile {
    let mut notes = data.analysis.notes.clone();
    notes.extend(data.validation_notes.iter().cloned());
    let (lba, cluster, allocated, partition_index) =
        allocation_metadata(data.candidate.offset, volumes, mode);
    CarvedFile {
        seq,
        file_type: data.candidate.kind,
        offset: data.candidate.offset,
        size: data.output_len,
        carved_size: data.analysis.end as u64,
        sha256: data.sha256.clone(),
        status: data.analysis.status(data.repaired),
        score: data.analysis.score,
        width: data.analysis.width,
        height: data.analysis.height,
        repaired: data.repaired,
        path: rel_path,
        notes,
        validation: data.validation,
        validity: data.validity,
        validity_reason: validity_reason_code(data),
        tail_trimmed_bytes: data.tail_trimmed_bytes,
        aligned: aligned(data.candidate.offset, opts.align, volumes),
        nested_in: None,
        sha256_output: data.repaired.then(|| data.sha256.clone()),
        repair_validated: data.repaired,
        recovered_rows: data.recovered_rows,
        total_rows: data.total_rows,
        recovered_fraction: data
            .recovered_rows
            .zip(data.total_rows)
            .map(|(kept, total)| {
                if total == 0 {
                    0.0
                } else {
                    f64::from(kept) / f64::from(total)
                }
            }),
        validated_bytes: data.validated_bytes,
        lba,
        cluster,
        partition_index,
        extent_id: data.candidate.extent_id,
        allocated,
        source: if data.fragments.len() > 1 {
            "bgc".into()
        } else {
            "carve".into()
        },
        fragments: data.fragments.clone(),
        validation_checks: data.validation_checks.clone(),
        checks: data.checks,
        confidence: data.confidence,
        alternatives: data.alternatives,
        join: data.join.clone(),
        join_alternatives: data.join_alternatives.clone(),
        sha256_original: data.sha256_original.clone(),
        original_name: None,
        mtime: None,
        contiguous_assumed: None,
        overwritten_clusters: None,
        metadata_notes: None,
        sparse_bytes: None,
    }
}

fn output_limit_warning(skipped_output: usize, written: u64, output_limit: u64) -> Warning {
    Warning::new(
        "OUTPUT_LIMIT",
        format!(
            "--max-output {} reached after {} byte(s); {skipped_output} candidate(s) were not written",
            human_size(output_limit),
            written
        ),
    )
}

/// Shared inputs for output writing. The tuple form keeps the call site short
/// while the mutable stats object stays explicit; byte and rejection counts are
/// updated at the same points as before.
#[derive(Clone, Copy)]
struct WriteCtx<'a>(
    &'a dyn ImageSource,
    &'a Path,
    &'a [CarvedData],
    &'a [FsInfo],
    ScanMode,
    &'a ScanOptions,
    &'a MemoryBudget,
    &'a Stop,
);

fn write_outputs(
    ctx: WriteCtx<'_>,
    stats: &mut ScanStats,
) -> Result<(Vec<CarvedFile>, Vec<Warning>)> {
    let WriteCtx(source, out_dir, files, volumes, mode, opts, budget, stop) = ctx;
    // With more than one scanned volume each file goes into its own
    // `part<N>/` sub-directory so outputs of different partitions cannot be
    // confused (A-07 `--partition all`).
    let multi = volumes.len() > 1;
    let write_buf_len = 64 * 1024u64;
    let _write_guard = BudgetGuard::reserve(budget, write_buf_len, "allocating the write buffer")?;
    let mut scratch = vec![0u8; write_buf_len as usize];
    let mut records = Vec::with_capacity(files.len());
    let mut warnings = Vec::new();
    let output_limit = effective_max_output(opts);
    let mut written = 0u64;
    let mut skipped_output = 0usize;

    for (idx, data) in files.iter().enumerate() {
        if stop.check() {
            // H-28: the current output, if any, is always finished and
            // atomically renamed before the next iteration checks the flag.
            // No partially written file is left behind.
            break;
        }
        if written.saturating_add(data.output_len) > output_limit {
            skipped_output = files.len().saturating_sub(idx);
            break;
        }
        let paths = output_paths(idx, data, volumes, multi, out_dir);
        if let Some(parent) = paths.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("cannot create output directory '{}'", parent.display())
            })?;
        }

        let digest = match stream_file_digest(source, &paths.tmp, data, &mut scratch) {
            Ok(digest) => digest,
            Err(e) => {
                let _ = fs::remove_file(&paths.tmp);
                return Err(e);
            }
        };
        if digest != data.sha256 {
            let _ = fs::remove_file(&paths.tmp);
            stats.rejected = stats.rejected.saturating_add(1);
            warnings.push(source_changed_warning(data, &paths.rel_path, &digest));
            continue;
        }
        if paths.path.exists() {
            fs::remove_file(&paths.path)
                .with_context(|| format!("cannot replace '{}'", paths.path.display()))?;
        }
        if let Err(e) = fs::rename(&paths.tmp, &paths.path) {
            let _ = fs::remove_file(&paths.tmp);
            return Err(e).with_context(|| {
                format!(
                    "cannot rename '{}' to '{}'",
                    paths.tmp.display(),
                    paths.path.display()
                )
            });
        }

        written = written.saturating_add(data.output_len);
        stats.bytes_written = stats.bytes_written.saturating_add(data.output_len);
        let record = build_carved_file(paths.seq, data, paths.rel_path, volumes, mode, opts);
        if !opts.quiet && !opts.verbose && idx < 20 {
            eprintln!(
                "[+] {:<4} offset={:>12} size={:>9} score={:>3} {} {}",
                record.file_type.label(),
                record.offset,
                record.size,
                record.score,
                record.validity.display_label(),
                record.path
            );
        }
        records.push(record);
    }
    if skipped_output > 0 {
        warnings.push(output_limit_warning(skipped_output, written, output_limit));
    }
    Ok((records, warnings))
}

fn allocation_metadata(
    offset: u64,
    volumes: &[FsInfo],
    mode: ScanMode,
) -> (u64, Option<u64>, Option<bool>, Option<usize>) {
    match volume_at(volumes, offset) {
        Some(fs) => {
            let lba = offset / fs.bytes_per_sector.max(1) as u64;
            let cluster = offset.checked_sub(fs.data_start).and_then(|rel| {
                if fs.cluster_size == 0 || rel % fs.cluster_size != 0 {
                    return None;
                }
                let index = rel / fs.cluster_size;
                Some(match fs.kind {
                    crate::fs::FsKind::Fat32 => index.saturating_add(2),
                    crate::fs::FsKind::Ntfs => index,
                })
            });
            let allocated = match mode {
                ScanMode::Unallocated => Some(false),
                ScanMode::Allocated => Some(true),
                _ => None,
            };
            (lba, cluster, allocated, fs.partition_index)
        }
        None => (offset / 512, None, None, None),
    }
}

fn write_file_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or("bin"),
        std::process::id()
    ));
    fs::write(&tmp, data)
        .with_context(|| format!("cannot write temporary file '{}'", tmp.display()))?;
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("cannot replace '{}'", path.display()))?;
    }
    fs::rename(&tmp, path)
        .with_context(|| format!("cannot rename '{}' to '{}'", tmp.display(), path.display()))?;
    Ok(())
}

/// Rewrite the manifest after an additive metadata pass so metadata-only rows
/// are included. Normal scans still write it from `run_scan` only.
pub fn write_manifest(out_dir: &Path, files: &[CarvedFile]) -> Result<()> {
    let path = out_dir.join("SHA256SUMS");
    let mut text = String::new();
    for f in files {
        // Comment lines are ignored by sha256sum, so the historical manifest
        // format remains verifiable while carrying the authoritative label.
        text.push_str(&format!(
            "# {} validity={}\n",
            f.path,
            f.validity.display_label()
        ));
        text.push_str(&format!("{}  {}\n", f.sha256, f.path));
    }
    write_file_atomic(&path, text.as_bytes())
}

fn start_progress(
    progress: Arc<Progress>,
    quiet: bool,
    label: &'static str,
) -> Option<std::thread::JoinHandle<()>> {
    if quiet || !std::io::stderr().is_terminal() {
        return None;
    }
    Some(std::thread::spawn(move || {
        while !progress.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
            let done = progress.done.load(Ordering::Relaxed);
            let pct = if progress.total == 0 {
                100.0
            } else {
                done as f64 * 100.0 / progress.total as f64
            };
            eprint!(
                "\r{label} {:>6.2}%  {:>10} / {}",
                pct,
                human_size(done),
                human_size(progress.total)
            );
            let _ = std::io::stderr().flush();
        }
    }))
}

fn prepare_output_dir(out_dir: &Path, force: bool) -> Result<()> {
    if out_dir.exists() {
        if !out_dir.is_dir() {
            bail!(
                "output path '{}' exists and is not a directory",
                out_dir.display()
            );
        }
        let non_empty = fs::read_dir(out_dir)
            .with_context(|| format!("cannot list '{}'", out_dir.display()))?
            .next()
            .is_some();
        if non_empty && !force {
            bail!(
                "output directory '{}' is not empty (use --force to write anyway)",
                out_dir.display()
            );
        }
    } else {
        fs::create_dir_all(out_dir)
            .with_context(|| format!("cannot create '{}'", out_dir.display()))?;
    }
    Ok(())
}

/// Returns a summary line for the CLI and prints per-type numbers.
pub fn print_summary(report: &ScanReport) {
    println!();
    println!(
        "Scanned {} in {} across {} extent(s) [{}]",
        human_size(report.bytes_scanned),
        crate::util::human_duration(report.duration_ms as u128),
        report.stats.extents,
        report.mode
    );
    println!(
        "Throughput: {:.1} MiB/s | candidates: {} | carved: {} | rejected: {}",
        report.throughput_mib_s,
        report.stats.candidates,
        report.stats.carved,
        report.stats.rejected
    );
    if report.stats.below_min_size
        + report.stats.below_min_score
        + report.stats.skipped_complete_only
        + report.stats.skipped_verified_only
        + report.stats.too_large_truncated
        + report.stats.duplicate_hashes
        + report.stats.suppressed_nested
        > 0
    {
        println!(
            "Filtered   : min-size={} min-score={} complete-only={} verified-only={} too-large-truncated={} duplicate-hash={} nested={}",
            report.stats.below_min_size,
            report.stats.below_min_score,
            report.stats.skipped_complete_only,
            report.stats.skipped_verified_only,
            report.stats.too_large_truncated,
            report.stats.duplicate_hashes,
            report.stats.suppressed_nested
        );
    }
    if report.stats.carved == 0 {
        println!("No files were carved.");
        return;
    }
    println!();
    println!(
        "{:<6} {:>7} {:>8} {:>11} {:>8} {:>8} {:>8} {:>12}",
        "TYPE", "FILES", "VERIFIED", "STRUCTURAL", "SUSPECT", "PARTIAL", "REPAIRED", "BYTES"
    );
    for kind in FileType::all() {
        let files: Vec<&CarvedFile> = report
            .files
            .iter()
            .filter(|f| f.file_type == kind)
            .collect();
        if files.is_empty() {
            continue;
        }
        let count = |validity| files.iter().filter(|f| f.validity == validity).count();
        let bytes: u64 = files.iter().map(|f| f.size).sum();
        println!(
            "{:<6} {:>7} {:>8} {:>11} {:>8} {:>8} {:>8} {:>12}",
            kind.label(),
            files.len(),
            count(Validity::Verified),
            count(Validity::Structural),
            count(Validity::Suspect),
            count(Validity::Partial),
            count(Validity::Repaired),
            human_size(bytes)
        );
    }
    println!("Validity: verified = content checked; structural = content not checked");
    println!("          suspect = integrity check failed; partial = salvaged prefix");
    println!("Output: {}", report.output_dir);
}
