use crate::fs::FsInfo;
use crate::io::{read_exact_at, ImageSource, RegionLog, UnreadableRegion};
use crate::signatures::{
    analyze, base_validity_checks, build_suffix, validate_deep_detail, validate_repair, Analysis,
    FileType, Need, PdfEofMode, Status, ValidatedRepair, Validation, ValidationCheck,
    ValidationDetail, ValidationLimits, ValidationMode, Validity,
};
use crate::util::{
    complement_extents, hex, human_size, merge_extents, sha256_hex, sha256_hex_with_suffix,
    unix_timestamp, Extent,
};
use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use rayon::prelude::*;
use serde::Serialize;
use sha2::Digest;
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{IsTerminal, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

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

/// A contained panic in one candidate or fragment-join attempt.
#[derive(Debug, Clone, Serialize)]
pub struct InternalError {
    pub offset: u64,
    #[serde(rename = "type")]
    pub file_type: FileType,
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
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
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
            used = self.changed.wait(used).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn release(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        {
            let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanMode {
    Auto,
    Unallocated,
    Raw,
    Allocated,
}

impl ScanMode {
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

#[derive(Clone)]
pub struct ScanOptions {
    pub types: Vec<FileType>,
    pub mode: ScanMode,
    pub min_size: u64,
    pub max_size: u64,
    pub max_truncated_size: u64,
    pub max_memory: u64,
    /// Maximum working memory for one decode validator
    /// (0 = the default validator bound).  The legacy report field
    /// `max_decode_bytes` mirrors this value.
    pub max_validator_memory: u64,
    pub chunk_size: u64,
    pub threads: usize,
    pub min_score: u8,
    pub repair: bool,
    pub complete_only: bool,
    pub pdf_eof: PdfEofMode,
    pub dedupe_hashes: bool,
    pub suppress_nested: bool,
    pub validate: ValidationMode,
    pub align: AlignMode,
    pub force: bool,
    pub quiet: bool,
    pub verbose: bool,
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
    /// Opt in to bounded bifragment gap carving (B-06).
    pub fragment_recovery: bool,
    /// Largest permitted gap between two joined fragments, in clusters.
    pub max_gap: u64,
    /// Maximum number of candidate joins evaluated during a scan.
    pub max_frag_candidates: usize,
    /// Wall-clock budget for the fragment-recovery stage.
    pub frag_time_limit: Duration,
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
            fragment_recovery: true,
            max_gap: DEFAULT_MAX_GAP_CLUSTERS,
            max_frag_candidates: DEFAULT_MAX_FRAG_CANDIDATES,
            frag_time_limit: Duration::from_millis(DEFAULT_FRAG_TIME_LIMIT_MS),
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
            };
            let _ = try_fragment_join(&source, &candidate, &join, &opts, &budget);
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
    pub argv: Vec<String>,
    pub case_id: Option<String>,
    pub examiner: Option<String>,
    pub image_sha256: Option<String>,
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

#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
    pub commit: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InputInfo {
    pub path: String,
    pub size: u64,
    pub sha256: Option<String>,
    pub modified_unix: Option<u64>,
    pub size_after: u64,
    pub modified_unix_after: Option<u64>,
    /// Unknown for stdin, which has no stable backing path (H-25).
    pub unchanged: Option<bool>,
    /// Logical image bytes exposed by the container (H-25).
    pub logical_size: u64,
    pub logical_size_after: u64,
    /// Sum of the physical bytes in the backing files (H-25).
    pub physical_size: u64,
    pub physical_size_after: u64,
    /// Resolved container format, never `auto` (H-25).
    pub format: String,
    /// Backing files with their observed sizes and mtimes (H-25).
    pub files: Vec<crate::io::SourceFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportOptions {
    pub types: Vec<String>,
    pub mode: String,
    pub min_size: u64,
    pub max_size: u64,
    pub max_truncated_size: u64,
    pub max_memory: u64,
    pub max_decode_bytes: u64,
    /// H-05: authoritative name for the shared decode-validator budget.
    pub max_validator_memory: u64,
    pub chunk_size: u64,
    pub threads: usize,
    pub min_score: u8,
    pub repair: bool,
    pub complete_only: bool,
    pub pdf_eof: String,
    pub dedupe_hashes: bool,
    pub suppress_nested: bool,
    pub validate: ValidationMode,
    pub align: AlignMode,
    pub partition: String,
    pub include_gaps: bool,
    pub max_candidates: u64,
    pub max_work_per_candidate: u64,
    pub max_output: u64,
    pub time_budget_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment_recovery: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_gap: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_frag_candidates: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frag_time_limit_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fragment {
    pub offset: u64,
    pub len: u64,
}

/// A structured report warning: a stable `code` plus a human message.
///
/// Codes that mean "the run did not read part of the image" make the CLI
/// exit with code 3 (finished but incomplete, see `docs/DECISIONS.md`).
#[derive(Debug, Clone, Serialize)]
pub struct Warning {
    pub code: String,
    pub message: String,
}

impl Warning {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
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
                | "WORK_LIMIT"
                | "TIME_LIMIT"
                | "OUTPUT_LIMIT"
                | "CANDIDATE_PANIC"
        )
    }
}

/// Compact summary of one detected volume in `volumes[]` (A-07).
#[derive(Debug, Clone, Serialize)]
pub struct VolumeSummary {
    /// 1-based position among the detected volumes.
    pub index: usize,
    pub fs: String,
    pub offset: u64,
    pub length: u64,
    pub cluster_size: u64,
    pub free_bytes: u64,
    pub partition_index: Option<usize>,
    /// Whether this run reads the volume.
    pub scanned: bool,
}

impl VolumeSummary {
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

#[derive(Debug, Clone, Serialize)]
pub struct CarvedFile {
    pub seq: u64,
    #[serde(rename = "type")]
    pub file_type: FileType,
    pub offset: u64,
    pub size: u64,
    pub carved_size: u64,
    pub sha256: String,
    pub status: Status,
    pub score: u8,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub repaired: bool,
    pub path: String,
    pub notes: Vec<String>,
    pub validation: Validation,
    /// Authoritative honesty label; legacy `validation`/`status` are preserved.
    pub validity: Validity,
    pub validated_bytes: u64,
    pub lba: u64,
    pub cluster: Option<u64>,
    pub partition_index: Option<usize>,
    pub extent_id: usize,
    pub allocated: Option<bool>,
    pub source: String,
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
    /// SHA-256 of the bytes read before a repair changed the output.
    /// Absent whenever output bytes are identical to the source bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256_original: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ScanStats {
    pub candidates: usize,
    pub carved: usize,
    /// Number of candidate or join panics contained during this scan.
    pub internal_errors: usize,
    pub rejected: usize,
    pub below_min_size: usize,
    pub below_min_score: usize,
    pub skipped_complete_only: usize,
    pub too_large_truncated: usize,
    pub duplicate_hashes: usize,
    pub suppressed_nested: usize,
    pub truncated: usize,
    pub repaired: usize,
    pub bytes_written: u64,
    pub extents: usize,
    pub bytes_in_extents: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    /// Report schema version; 2 is the current shape.
    pub schema_version: u8,
    pub tool: String,
    pub tool_info: ToolInfo,
    pub image: String,
    pub input: InputInfo,
    pub image_size: u64,
    pub filesystem: Option<FsInfo>,
    pub mode: String,
    pub output_dir: String,
    pub argv: Vec<String>,
    pub options: ReportOptions,
    pub case_id: Option<String>,
    pub examiner: Option<String>,
    /// ISO-8601 UTC timestamp.
    pub started_at: String,
    pub started_at_unix: u64,
    pub duration_ms: u64,
    pub bytes_scanned: u64,
    pub throughput_mib_s: f64,
    pub stats: ScanStats,
    /// Every detected volume with a `scanned` flag (A-07).
    pub volumes: Vec<VolumeSummary>,
    /// Top-level image regions with a `scanned` flag (A-07).
    pub regions: Vec<crate::fs::Region>,
    /// Byte ranges that could not be read from the source (H-22).
    pub unreadable_regions: Vec<UnreadableRegion>,
    pub warnings: Vec<Warning>,
    /// Contained panics, with the source offset and file type of the work item
    /// that caused them. Work on other candidates continues.
    pub internal_errors: Vec<InternalError>,
    /// True when a wall-clock budget stopped the scan; work-unit limits do
    /// not set this flag because their results are deterministic.
    pub nondeterministic_stop: bool,
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
    validation: Validation,
    validity: Validity,
    validation_notes: Vec<String>,
    /// Contiguous bytes from offset zero whose content was actually checked.
    validated_bytes: u64,
    validation_checks: Vec<ValidationCheck>,
    /// Set only for B-06 stitched output (also used for reporting).
    fragments: Vec<Fragment>,
    checks: Option<u64>,
    confidence: Option<u8>,
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
    LowScore(u8),
    NotComplete,
    TooLargeTruncated(u64),
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
    } = *input;
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
    let started = unix_timestamp();
    let t0 = Instant::now();
    let time = TimeBudget::new(t0, opts.time_budget);
    let mut warnings = Vec::new();
    let mut internal_errors = Vec::new();
    if let Ok(value) = std::env::var("CARVEX_TEST_PANIC_AT_OFFSET") {
        if let Ok(offset) = value.parse::<u64>() {
            set_test_panic_at_offset(offset);
        }
    }

    prepare_output_dir(out_dir, opts.force)?;

    let (mode, extents) = plan_extents(opts.mode, volumes, extra_extents, image_len)?;
    let bytes_in_extents: u64 = extents.iter().map(|e| e.len).sum();
    let tasks = build_tasks(&extents, opts.chunk_size);
    let total_to_scan: u64 = tasks.iter().map(|t| t.scan_len).sum();

    let scan_progress = Arc::new(Progress::new(total_to_scan));
    let scan_thread = start_progress(scan_progress.clone(), opts.quiet, "Scanning");

    let thread_count = if opts.threads == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    } else {
        opts.threads
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count.max(1))
        .build()
        .context("cannot build worker pool")?;

    let (mut candidates, candidate_limit_hit) = find_candidates(
        source,
        &tasks,
        &opts.types,
        opts,
        volumes,
        &scan_progress,
        &pool,
        &time,
    )?;
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

    let mut stats = ScanStats {
        candidates: candidates.len(),
        extents: extents.len(),
        bytes_in_extents,
        ..Default::default()
    };

    let carve_progress = Arc::new(Progress::new(candidates.len() as u64));
    let carve_thread = start_progress(carve_progress.clone(), opts.quiet, "Carving");
    let budget = Arc::new(MemoryBudget::new(opts.max_memory));
    let mut outcomes = Vec::with_capacity(candidates.len());
    // Batching is only a scheduling aid now: the budget itself bounds
    // concurrent buffers. A fixed multiple of the thread count keeps result
    // order deterministic (`par_iter().collect()` preserves input order).
    let batch_size = thread_count.max(1).saturating_mul(4);
    for batch in candidates.chunks(batch_size.max(1)) {
        if time.check() {
            break;
        }
        let mut batch_out =
            carve_batch(source, batch, opts, &budget, &pool, &carve_progress, &time)?;
        outcomes.append(&mut batch_out);
    }
    if let Some(h) = carve_thread {
        carve_progress.stop.store(true, Ordering::Relaxed);
        let _ = h.join();
        eprint!("\r{:100}\r", "");
    }

    // Postfilter works on descriptors only; no carved bytes are retained.
    let mut kept = postfilter(
        outcomes,
        opts,
        &mut stats,
        &mut warnings,
        &mut internal_errors,
    );
    if opts.fragment_recovery {
        kept = recover_fragments(
            source,
            &candidates,
            kept,
            &extents,
            volumes,
            opts,
            &budget,
            &mut warnings,
            &time,
            &mut internal_errors,
        )?;
    }
    let unreadable_snapshot = match unreadable_regions.lock() {
        Ok(regions) => regions.clone(),
        Err(_) => Vec::new(),
    };
    // H-22: zero-filled bytes are not evidence.  Mark (or, under `skip`, drop)
    // every output that intersects a region the source could not read.
    if !unreadable_snapshot.is_empty() {
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
        if skip_unreadable && !skipped.is_empty() {
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
    }

    if opts.verbose {
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

    let (files, write_warnings) = write_outputs(
        source, out_dir, &kept, volumes, mode, &mut stats, opts, &budget, &time,
    )?;
    warnings.extend(write_warnings);
    stats.carved = files.len();
    stats.truncated = files
        .iter()
        .filter(|f| f.status != Status::Complete)
        .count();
    stats.repaired = files.iter().filter(|f| f.repaired).count();
    stats.bytes_written = files.iter().map(|f| f.size).sum();
    if opts.manifest {
        write_manifest(out_dir, &files)?;
    }
    if time.expired() && !warnings.iter().any(|w| w.code == "TIME_LIMIT") {
        warnings.push(Warning::new(
            "TIME_LIMIT",
            "scan exceeded --time-budget; stopped early with a partial report",
        ));
    }
    let nondeterministic_stop = warnings.iter().any(|w| w.code == "TIME_LIMIT");
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

    let elapsed = t0.elapsed();
    let bytes_scanned = total_to_scan;
    let throughput = if elapsed.as_secs_f64() > 0.0 {
        bytes_scanned as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0)
    } else {
        0.0
    };

    let input = build_input_info(image, image_len, image_identity, ctx);
    if input.unchanged == Some(false) {
        warnings.push(Warning::new(
            "IMAGE_MODIFIED",
            "source image size or mtime changed during the scan",
        ));
    }

    Ok(ScanReport {
        schema_version: SCHEMA_VERSION,
        tool: format!("CarveX {}", env!("CARGO_PKG_VERSION")),
        tool_info: ToolInfo {
            name: "CarveX".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            commit: option_env!("CARVEX_GIT_COMMIT").unwrap_or("unknown").into(),
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
        files,
    })
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
        files: Vec::new(),
    }
}

fn plan_extents(
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
                    all.extend(fs.free_extents.iter().cloned());
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
                all.extend(fs.free_extents.iter().cloned());
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

fn build_tasks(extents: &[Extent], chunk_size: u64) -> Vec<Task> {
    let mut tasks = Vec::new();
    for (extent_id, e) in extents.iter().enumerate() {
        let mut off = e.offset;
        let end = e.end();
        while off < end {
            let scan_len = chunk_size.min(end - off);
            let read_len = scan_len.saturating_add(16).min(end - off);
            tasks.push(Task {
                start: off,
                scan_len,
                read_len,
                extent_end: end,
                extent_id,
            });
            off = off.saturating_add(scan_len);
            if scan_len == 0 {
                break;
            }
        }
    }
    tasks
}

#[allow(clippy::too_many_arguments)]
fn find_candidates(
    source: &dyn ImageSource,
    tasks: &[Task],
    types: &[FileType],
    opts: &ScanOptions,
    volumes: &[FsInfo],
    progress: &Progress,
    pool: &rayon::ThreadPool,
    time: &TimeBudget,
) -> Result<(Vec<Candidate>, bool)> {
    if time.check() {
        return Ok((Vec::new(), false));
    }
    // First pass: count plausible headers per fixed task. This is a cheap
    // lexical scan; it lets us hand out an exact deterministic global budget
    // without an atomic reservation whose winner would depend on scheduling.
    let counts: Vec<usize> = pool.install(|| {
        tasks
            .par_iter()
            .map(|task| count_task(source, task, types, opts, volumes, time))
            .collect::<Result<Vec<_>>>()
    })?;
    if time.check() {
        return Ok((Vec::new(), false));
    }
    let total: usize = counts.iter().fold(0usize, |sum, &n| sum.saturating_add(n));
    let max = usize::try_from(effective_max_candidates(opts)).unwrap_or(usize::MAX);
    let hit = total > max;
    let mut remaining = total.min(max);
    let quotas: Vec<usize> = counts
        .iter()
        .map(|&count| {
            let quota = count.min(remaining);
            remaining = remaining.saturating_sub(quota);
            quota
        })
        .collect();

    let found: Vec<Vec<Candidate>> = pool.install(|| {
        tasks
            .par_iter()
            .zip(quotas)
            .map(|(task, quota)| {
                if time.check() || quota == 0 {
                    return Ok(Vec::new());
                }
                scan_task(source, task, types, opts, volumes, progress, quota, time)
            })
            .collect::<Result<Vec<_>>>()
    })?;
    let mut out = Vec::with_capacity(total.min(max));
    for candidates in found {
        out.extend(candidates);
    }
    Ok((out, hit))
}

#[allow(clippy::too_many_arguments)]
fn count_task(
    source: &dyn ImageSource,
    task: &Task,
    types: &[FileType],
    opts: &ScanOptions,
    volumes: &[FsInfo],
    time: &TimeBudget,
) -> Result<usize> {
    if time.check() {
        return Ok(0);
    }
    let mut buf = vec![0u8; task.read_len as usize];
    read_exact_at(source, &mut buf, task.start)
        .with_context(|| format!("cannot read scan chunk at offset {}", task.start))?;
    let mut count = 0usize;
    for &kind in types {
        for rel in memchr::memmem::find_iter(&buf, kind.header()) {
            if time.check() {
                return Ok(count);
            }
            if rel as u64 >= task.scan_len || !plausible_header(kind, &buf, rel) {
                continue;
            }
            let offset = task.start + rel as u64;
            if aligned(offset, opts.align, volumes) {
                count = count.saturating_add(1);
            }
        }
    }
    Ok(count)
}

#[allow(clippy::too_many_arguments)]
fn scan_task(
    source: &dyn ImageSource,
    task: &Task,
    types: &[FileType],
    opts: &ScanOptions,
    volumes: &[FsInfo],
    progress: &Progress,
    quota: usize,
    time: &TimeBudget,
) -> Result<Vec<Candidate>> {
    let mut buf = vec![0u8; task.read_len as usize];
    read_exact_at(source, &mut buf, task.start)
        .with_context(|| format!("cannot read scan chunk at offset {}", task.start))?;
    let mut out = Vec::with_capacity(quota);
    'types: for &kind in types {
        for rel in memchr::memmem::find_iter(&buf, kind.header()) {
            if time.check() {
                break 'types;
            }
            if rel as u64 >= task.scan_len || !plausible_header(kind, &buf, rel) {
                continue;
            }
            let offset = task.start + rel as u64;
            if !aligned(offset, opts.align, volumes) {
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
    progress.done.fetch_add(task.scan_len, Ordering::Relaxed);
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
    time: &TimeBudget,
) -> Result<Vec<CarveOutcome>> {
    pool.install(|| {
        batch
            .par_iter()
            .map(|cand| {
                if time.check() {
                    return Ok(CarveOutcome::TimeLimit);
                }
                let out = match catch_unwind(AssertUnwindSafe(|| {
                    carve_one(source, cand, opts, budget, time)
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

fn carve_one(
    source: &dyn ImageSource,
    cand: &Candidate,
    opts: &ScanOptions,
    budget: &MemoryBudget,
    time: &TimeBudget,
) -> Result<CarveOutcome> {
    if time.check() {
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
    let mut guard = match BudgetGuard::reserve(budget, initial_len, "allocating a carve buffer") {
        Ok(guard) => guard,
        Err(_) => {
            return Ok(CarveOutcome::SkippedBudget {
                offset: cand.offset,
                kind: cand.kind,
                needed: initial_len,
            })
        }
    };
    let mut buf = vec![0u8; initial_len as usize];
    let mut buf_len = initial_len;
    let work_limit = effective_max_work(opts);
    let mut work = WorkCounter::new(work_limit);

    loop {
        if time.check() {
            return Ok(CarveOutcome::TimeLimit);
        }
        // Every growth step re-reads the full prefix after dropping the old
        // buffer and its permit. Waiting threads therefore hold no permit and
        // cannot deadlock each other.
        if let Err(e) = read_exact_at(source, &mut buf[..buf_len as usize], cand.offset) {
            return Ok(CarveOutcome::Rejected(format!("read error: {e}")));
        }
        let at_limit = buf_len >= hard_limit;
        let read_len = buf_len as usize;
        if !work.add(read_len as u64) {
            return Ok(CarveOutcome::WorkLimit {
                offset: cand.offset,
                kind: cand.kind,
                examined: work.used().saturating_add(read_len as u64),
                limit: work_limit,
            });
        }
        test_panic_probe(cand.offset, "analyze");
        match analyze(cand.kind, &buf[..read_len], at_limit, opts.pdf_eof) {
            Need::Analyzed(mut analysis) => {
                if analysis.end == 0 || analysis.end > read_len {
                    return Ok(CarveOutcome::Rejected("invalid carved length".into()));
                }
                if !analysis.complete {
                    let trimmed = trim_zero_tail(&buf[..analysis.end]);
                    if trimmed < analysis.end {
                        analysis.notes.push(format!(
                            "trimmed {} trailing zero byte(s)",
                            analysis.end - trimmed
                        ));
                        analysis.end = trimmed;
                    }
                }
                let end = analysis.end;
                if !analysis.complete && (end as u64) > opts.max_truncated_size {
                    return Ok(CarveOutcome::TooLargeTruncated(end as u64));
                }
                if analysis.score < opts.min_score {
                    return Ok(CarveOutcome::LowScore(analysis.score));
                }
                if opts.complete_only && !analysis.complete {
                    return Ok(CarveOutcome::NotComplete);
                }
                let repair_attempted = opts.repair && !analysis.complete;
                // Validator scratch is bounded by its own flag and, for this
                // scan, can never raise the shared working-memory ceiling.
                let limits = ValidationLimits::new(opts.max_validator_memory.min(budget.limit()));
                let mut repair_suffix: Vec<u8> = Vec::new();
                let mut repaired = false;
                let mut sha256_original: Option<String> = None;
                let mut repair_detail: Option<ValidationDetail> = None;
                let mut repair_note: Option<String> = None;
                let mut repair_reason: Option<String> = None;

                // H-21: rebuild and revalidate before claiming `repaired`.
                if repair_attempted {
                    match build_suffix(cand.kind, &buf[..end], limits) {
                        Ok(suffix) => {
                            let rebuilt_len = (end as u64).saturating_add(suffix.len() as u64);
                            match usize::try_from(rebuilt_len) {
                                Ok(rebuilt_len) => {
                                    match BudgetGuard::reserve(
                                        budget,
                                        rebuilt_len as u64,
                                        "validating a repaired file",
                                    ) {
                                        Ok(mut guard) => {
                                            let mut rebuilt = Vec::new();
                                            if rebuilt.try_reserve_exact(rebuilt_len).is_err() {
                                                repair_reason = Some(
                                                    "cannot reserve repair validation memory"
                                                        .into(),
                                                );
                                            } else {
                                                rebuilt.extend_from_slice(&buf[..end]);
                                                rebuilt.extend_from_slice(&suffix);
                                                if !work.add(rebuilt_len as u64) {
                                                    return Ok(CarveOutcome::WorkLimit {
                                                        offset: cand.offset,
                                                        kind: cand.kind,
                                                        examined: work
                                                            .used()
                                                            .saturating_add(rebuilt_len as u64),
                                                        limit: work_limit,
                                                    });
                                                }
                                                test_panic_probe(cand.offset, "validate");
                                                match validate_repair(
                                                    cand.kind,
                                                    &rebuilt,
                                                    limits,
                                                    opts.validate,
                                                    opts.pdf_eof,
                                                ) {
                                                    Ok(ValidatedRepair {
                                                        analysis: rebuilt_analysis,
                                                        detail,
                                                        note,
                                                    }) => {
                                                        repaired = true;
                                                        repair_suffix = suffix;
                                                        repair_detail = Some(detail);
                                                        repair_note = Some(note.clone());
                                                        sha256_original =
                                                            Some(sha256_hex(&buf[..end]));
                                                        // Keep the original score, length and
                                                        // provenance notes; only the footer status
                                                        // becomes complete after revalidation.
                                                        analysis.complete = true;
                                                        analysis
                                                            .notes
                                                            .extend(rebuilt_analysis.notes);
                                                        analysis.notes.push(note);
                                                    }
                                                    Err(reason) => repair_reason = Some(reason),
                                                }
                                            }
                                            guard.release();
                                        }
                                        Err(e) => repair_reason = Some(e.to_string()),
                                    }
                                }
                                Err(_) => {
                                    repair_reason =
                                        Some("repaired length is not addressable".into());
                                }
                            }
                        }
                        Err(reason) => repair_reason = Some(reason),
                    }
                    if !repaired {
                        analysis
                            .notes
                            .push("repair attempted, not validated".into());
                        if let Some(reason) = repair_reason.as_ref() {
                            analysis.notes.push(format!("repair reason: {reason}"));
                        }
                    }
                }

                let output_len = (end as u64).saturating_add(repair_suffix.len() as u64);
                if output_len < opts.min_size {
                    return Ok(CarveOutcome::TooSmall(output_len));
                }

                let validate_bytes = if repair_detail.is_none()
                    && opts.validate == ValidationMode::Decode
                    && analysis.complete
                {
                    end as u64
                } else {
                    0
                };
                test_panic_probe(cand.offset, "validate");
                if validate_bytes > 0 && !work.add(validate_bytes) {
                    return Ok(CarveOutcome::WorkLimit {
                        offset: cand.offset,
                        kind: cand.kind,
                        examined: work.used().saturating_add(validate_bytes),
                        limit: work_limit,
                    });
                }
                let detail = if let Some(detail) = repair_detail {
                    detail
                } else if opts.validate == ValidationMode::Decode {
                    if analysis.complete {
                        validate_deep_detail(cand.kind, &buf[..end], &analysis, limits)
                    } else {
                        ValidationDetail::new(
                            Validation::Partial,
                            vec!["file is truncated; decode validation skipped".into()],
                        )
                    }
                } else if analysis.complete {
                    // The PNG structural parser already recomputes every chunk
                    // CRC. A failure is an integrity failure even in the
                    // default mode, while no decoded byte count is claimed.
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
                };
                let ValidationDetail {
                    validation,
                    notes: mut validation_notes,
                    checks: detail_checks,
                    validated_bytes,
                } = detail;
                // `carved_size` describes bytes read; never claim more checked
                // bytes than were read just because a suffix was appended.
                let validated_bytes = validated_bytes.min(end as u64);
                let status = analysis.status(repaired);
                let validity = Validity::from_legacy(validation, status);
                let mut validation_checks =
                    base_validity_checks(cand.kind, validation, status, &validation_notes);
                validation_checks.extend(detail_checks);
                if repair_attempted {
                    let (ok, note) = if repaired {
                        (
                            Some(true),
                            repair_note.unwrap_or_else(|| "repair revalidated".to_string()),
                        )
                    } else {
                        (
                            Some(false),
                            repair_reason
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
                        return Ok(CarveOutcome::LowScore(analysis.score));
                    }
                }
                let sha256 = sha256_hex_with_suffix(&buf[..end], &repair_suffix);
                return Ok(CarveOutcome::Carved(Box::new(CarvedData {
                    candidate: cand.clone(),
                    payload: PayloadSpec::Range {
                        offset: cand.offset,
                        len: end as u64,
                    },
                    output_len,
                    analysis,
                    sha256,
                    sha256_original,
                    repaired,
                    repair_suffix,
                    validation,
                    validity,
                    validation_notes,
                    validated_bytes,
                    validation_checks,
                    fragments: Vec::new(),
                    checks: None,
                    confidence: None,
                })));
            }
            Need::More => {
                if at_limit {
                    return Ok(CarveOutcome::Rejected(
                        "parser requested more data at the carve limit".into(),
                    ));
                }
                let next = buf_len.saturating_mul(2).min(hard_limit);
                if next <= buf_len {
                    return Ok(CarveOutcome::Rejected("carve buffer cannot grow".into()));
                }
                if next > budget.limit() {
                    return Ok(CarveOutcome::SkippedBudget {
                        offset: cand.offset,
                        kind: cand.kind,
                        needed: next,
                    });
                }
                drop(buf);
                guard.release();
                guard = BudgetGuard::reserve(budget, next, "growing a carve buffer")?;
                buf = vec![0u8; next as usize];
                buf_len = next;
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
}

struct JoinAttempt {
    data: Option<CarvedData>,
    deep_checked: bool,
    skipped_budget: Option<(u64, u64)>,
    work_limit: Option<(u64, FileType, u64, u64)>,
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
    let tasks = build_tasks(extents, 1024 * 1024);
    for task in &tasks {
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

fn candidate_joins(
    cand: &Candidate,
    fs: &FsInfo,
    footers: &[u64],
    extents: &[Extent],
    opts: &ScanOptions,
) -> Vec<FragmentJoin> {
    let mut joins = Vec::new();
    if fs.cluster_size == 0 || !is_cluster_boundary(cand.offset, fs) {
        return joins;
    }
    let available = cand.extent_end.saturating_sub(cand.offset);
    let head_span = opts.max_size.min(available);
    let Some(mut cut) = next_cluster_boundary(cand.offset.saturating_add(1), fs) else {
        return joins;
    };
    let mut cuts = Vec::new();
    while cut <= cand.extent_end && cut - cand.offset <= head_span {
        cuts.push(cut);
        if cuts.len() >= opts.max_frag_candidates {
            break;
        }
        let Some(next) = cut.checked_add(fs.cluster_size) else {
            break;
        };
        cut = next;
    }

    'search: for cut in cuts {
        if !range_covered(extents, cand.offset, cut) {
            continue;
        }
        for gap in 1..=opts.max_gap {
            let Some(gap_bytes) = fs.cluster_size.checked_mul(gap) else {
                break;
            };
            let Some(tail_start) = cut.checked_add(gap_bytes) else {
                break;
            };
            for &footer_end in footers {
                if footer_end <= tail_start {
                    continue;
                }
                let tail_len = footer_end - tail_start;
                if tail_len == 0 || tail_len > opts.max_size {
                    continue;
                }
                if !range_covered(extents, tail_start, footer_end) {
                    continue;
                }
                joins.push(FragmentJoin {
                    cut,
                    tail_start,
                    footer_end,
                    gap_clusters: gap,
                });
                if joins.len() >= opts.max_frag_candidates {
                    break 'search;
                }
            }
        }
    }

    // The guide requires increasing-distance ordering: a smaller skipped gap
    // is tried before a larger one, then the nearest tail and cut offsets.
    joins.sort_by_key(|j| (j.gap_clusters, j.cut, j.tail_start, j.footer_end));
    joins.truncate(opts.max_frag_candidates);
    joins
}

fn try_fragment_join(
    source: &dyn ImageSource,
    cand: &Candidate,
    join: &FragmentJoin,
    opts: &ScanOptions,
    budget: &MemoryBudget,
) -> Result<JoinAttempt> {
    let none = || JoinAttempt {
        data: None,
        deep_checked: false,
        skipped_budget: None,
        work_limit: None,
    };
    let head_len = join.cut.saturating_sub(cand.offset);
    let tail_len = join.footer_end.saturating_sub(join.tail_start);
    if head_len == 0 || tail_len == 0 {
        return Ok(none());
    }
    let total = match head_len.checked_add(tail_len) {
        Some(v) if v <= opts.max_size => v,
        _ => return Ok(none()),
    };
    let _guard = match BudgetGuard::reserve(budget, total, "allocating a B-06 stitch buffer") {
        Ok(guard) => guard,
        Err(_) => {
            return Ok(JoinAttempt {
                data: None,
                deep_checked: false,
                skipped_budget: Some((cand.offset, total)),
                work_limit: None,
            })
        }
    };
    let work_limit = effective_max_work(opts);
    let mut work = WorkCounter::new(work_limit);
    if !work.add(total) {
        return Ok(JoinAttempt {
            data: None,
            deep_checked: false,
            skipped_budget: None,
            work_limit: Some((
                cand.offset,
                cand.kind,
                work.used().saturating_add(total),
                work_limit,
            )),
        });
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

    let Need::Analyzed(mut analysis) = analyze(cand.kind, &bytes, true, opts.pdf_eof) else {
        return Ok(none());
    };
    if !analysis.complete || analysis.end == 0 || analysis.end > bytes.len() {
        return Ok(none());
    }
    if analysis.score < opts.min_score {
        return Ok(none());
    }
    bytes.truncate(analysis.end);
    let used_tail = match analysis.end.checked_sub(head_len as usize) {
        Some(v) if v > 0 && v as u64 <= tail_len => v as u64,
        _ => return Ok(none()),
    };

    let validated = analysis.end as u64;
    if !work.add(validated) {
        return Ok(JoinAttempt {
            data: None,
            deep_checked: true,
            skipped_budget: None,
            work_limit: Some((
                cand.offset,
                cand.kind,
                work.used().saturating_add(validated),
                work_limit,
            )),
        });
    }
    let limits = ValidationLimits::new(opts.max_validator_memory.min(budget.limit()));
    let ValidationDetail {
        validation,
        notes: validation_notes,
        checks: detail_checks,
        validated_bytes,
    } = validate_deep_detail(cand.kind, &bytes, &analysis, limits);
    // B-06 requires deep validation. H-19 stage 2 makes valid PDFs Decoded.
    // Keep accepting the stage-1 lexical-validated Unverified PDF outcome
    // (E-1) for compatibility; every other Unverified result is rejected.
    let accepted = match validation {
        Validation::Decoded => true,
        Validation::Unverified if cand.kind == FileType::Pdf => validation_notes
            .iter()
            .any(|n| n.contains("startxref target validated")),
        _ => false,
    };
    if !accepted {
        return Ok(JoinAttempt {
            data: None,
            deep_checked: true,
            skipped_budget: None,
            work_limit: None,
        });
    }

    analysis.notes.push(format!(
        "B-06 deep-validated 2 fragments with a {} cluster gap",
        join.gap_clusters
    ));
    let confidence = analysis.score;
    let sha256 = sha256_hex(&bytes);
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
    let status = analysis.status(false);
    let validity = Validity::from_legacy(validation, status);
    let mut validation_checks =
        base_validity_checks(cand.kind, validation, status, &validation_notes);
    validation_checks.extend(detail_checks);
    Ok(JoinAttempt {
        data: Some(CarvedData {
            candidate: cand.clone(),
            payload: PayloadSpec::Join {
                fragments: fragments.clone(),
            },
            output_len,
            analysis,
            sha256,
            sha256_original: None,
            repaired: false,
            repair_suffix: Vec::new(),
            validation,
            validity,
            validation_notes,
            validated_bytes,
            validation_checks,
            fragments,
            checks: None,
            confidence: Some(confidence),
        }),
        deep_checked: true,
        skipped_budget: None,
        work_limit: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn recover_fragments(
    source: &dyn ImageSource,
    candidates: &[Candidate],
    mut kept: Vec<CarvedData>,
    extents: &[Extent],
    volumes: &[FsInfo],
    opts: &ScanOptions,
    budget: &Arc<MemoryBudget>,
    warnings: &mut Vec<Warning>,
    time: &TimeBudget,
    internal_errors: &mut Vec<InternalError>,
) -> Result<Vec<CarvedData>> {
    let started = Instant::now();
    if volumes.is_empty() {
        warnings.push(Warning::new(
            "FRAGMENT_RECOVERY_SKIPPED",
            "B-06 needs a detected volume with cluster boundaries; no stitching attempted",
        ));
        return Ok(kept);
    }

    let footers = match find_footers(source, extents, &opts.types, opts) {
        Ok(v) => v,
        Err(e) => {
            warnings.push(Warning::new(
                "FRAGMENT_FOOTER_SCAN_FAILED",
                format!("fragment footer scan failed: {e}"),
            ));
            return Ok(kept);
        }
    };

    let mut successes = Vec::new();
    let mut successful_keys: HashSet<(u64, FileType)> = HashSet::new();
    let mut seen_hashes: HashSet<String> = if opts.dedupe_hashes {
        kept.iter().map(|f| f.sha256.clone()).collect()
    } else {
        HashSet::new()
    };
    let mut attempts = 0usize;
    let mut candidate_limit = false;
    let mut time_limit = false;
    let mut join_work_limits: Vec<(u64, FileType, u64, u64)> = Vec::new();

    for cand in candidates {
        if time.check() {
            break;
        }
        if attempts >= opts.max_frag_candidates {
            candidate_limit = true;
            break;
        }
        if started.elapsed() >= opts.frag_time_limit {
            time_limit = true;
            break;
        }
        let Some(fs) = volume_at(volumes, cand.offset) else {
            continue;
        };
        let Some(footer_hits) = footers.get(&cand.kind) else {
            continue;
        };
        let joins = candidate_joins(cand, fs, footer_hits, extents, opts);
        let mut checks = 0u64;

        for join in joins {
            if time.check() {
                break;
            }
            if attempts >= opts.max_frag_candidates {
                candidate_limit = true;
                break;
            }
            if started.elapsed() >= opts.frag_time_limit {
                time_limit = true;
                break;
            }
            attempts += 1;
            let attempt = match catch_unwind(AssertUnwindSafe(|| {
                try_fragment_join(source, cand, &join, opts, budget)
            })) {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => {
                    if opts.verbose {
                        eprintln!("[-] B-06 join at {} failed: {e}", cand.offset);
                    }
                    // Budget exhaustion is fatal; other errors are recorded as misses.
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
            if let Some((offset, kind, examined, limit)) = attempt.work_limit {
                join_work_limits.push((offset, kind, examined, limit));
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
            if !seen_hashes.insert(data.sha256.clone()) {
                continue;
            }
            data.checks = Some(checks);
            let key = (cand.offset, cand.kind);
            successful_keys.insert(key);
            successes.push(data);
            // Guide: stop at the first verified stitch for a head fragment.
            break;
        }
    }

    if let Some(&(offset, kind, examined, limit)) = join_work_limits.first() {
        warnings.push(Warning::new(
            "WORK_LIMIT",
            format!(
                "{} B-06 join(s) exceeded --max-work-per-candidate {} bytes;                  first at byte {} ({}) examined {} bytes",
                join_work_limits.len(),
                limit,
                offset,
                kind.label(),
                examined
            ),
        ));
    }
    if candidate_limit {
        warnings.push(Warning::new(
            "FRAGMENT_CANDIDATE_LIMIT",
            "B-06 stopped after reaching --max-frag-candidates",
        ));
    }
    if time_limit {
        warnings.push(Warning::new(
            "FRAGMENT_TIME_LIMIT",
            "B-06 stopped after reaching --frag-time-limit",
        ));
    }

    kept.retain(|f| {
        let replaced = successful_keys.contains(&(f.candidate.offset, f.candidate.kind));
        !replaced
    });
    kept.extend(successes);
    kept.sort_by_key(|f| (f.candidate.offset, f.candidate.kind as u8));
    Ok(kept)
}

fn postfilter(
    outcomes: Vec<CarveOutcome>,
    opts: &ScanOptions,
    stats: &mut ScanStats,
    warnings: &mut Vec<Warning>,
    internal_errors: &mut Vec<InternalError>,
) -> Vec<CarvedData> {
    let mut candidates: Vec<CarvedData> = Vec::new();
    let mut work_limited: Vec<(u64, FileType, u64, u64)> = Vec::new();
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
            CarveOutcome::LowScore(score) => {
                stats.below_min_score += 1;
                if opts.verbose {
                    eprintln!("[-] below --min-score {}: {score}", opts.min_score);
                }
            }
            CarveOutcome::NotComplete => {
                stats.skipped_complete_only += 1;
                if opts.verbose {
                    eprintln!("[-] truncated file skipped (--complete-only)");
                }
            }
            CarveOutcome::TooLargeTruncated(size) => {
                stats.too_large_truncated += 1;
                if opts.verbose {
                    eprintln!(
                        "[-] truncated file {size} exceeds --max-truncated-size {}",
                        opts.max_truncated_size
                    );
                }
            }
        }
    }

    if opts.suppress_nested && candidates.len() > 1 {
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
        let mut idx = 0usize;
        candidates.retain(|_c| {
            let keep_this = keep[idx];
            idx += 1;
            keep_this
        });
    }

    if opts.dedupe_hashes {
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
        candidates = kept;
    }
    if let Some(&(offset, kind, examined, limit)) = work_limited.first() {
        warnings.push(Warning::new(
            "WORK_LIMIT",
            format!(
                "{} candidate(s) exceeded --max-work-per-candidate {} bytes;                  first at byte {} ({}) examined {} bytes",
                work_limited.len(),
                limit,
                offset,
                kind.label(),
                examined
            ),
        ));
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

#[allow(clippy::too_many_arguments)]
fn write_outputs(
    source: &dyn ImageSource,
    out_dir: &Path,
    files: &[CarvedData],
    volumes: &[FsInfo],
    mode: ScanMode,
    stats: &mut ScanStats,
    opts: &ScanOptions,
    budget: &MemoryBudget,
    time: &TimeBudget,
) -> Result<(Vec<CarvedFile>, Vec<Warning>)> {
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
        if time.check() {
            break;
        }
        if written.saturating_add(data.output_len) > output_limit {
            skipped_output = files.len().saturating_sub(idx);
            break;
        }
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
                .map(|i| volumes[i].partition_index.unwrap_or(i + 1))
                .unwrap_or(1);
            format!("part{part}/{filename}")
        } else {
            filename.clone()
        };
        let path = out_dir.join(&rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("cannot create output directory '{}'", parent.display())
            })?;
        }
        let tmp = path.with_extension(format!(
            "{}.tmp{}-{}",
            path.extension().and_then(|e| e.to_str()).unwrap_or("bin"),
            std::process::id(),
            seq
        ));

        let digest = (|| -> Result<String> {
            let mut file = File::create(&tmp)
                .with_context(|| format!("cannot create temporary file '{}'", tmp.display()))?;
            let mut hasher = sha2::Sha256::new();
            match &data.payload {
                PayloadSpec::Range { offset, len } => {
                    stream_range(source, &mut file, &mut hasher, *offset, *len, &mut scratch)?;
                }
                PayloadSpec::Join { fragments } => {
                    for fragment in fragments {
                        stream_range(
                            source,
                            &mut file,
                            &mut hasher,
                            fragment.offset,
                            fragment.len,
                            &mut scratch,
                        )?;
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
        })();
        let digest = match digest {
            Ok(digest) => digest,
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(e);
            }
        };
        if digest != data.sha256 {
            let _ = fs::remove_file(&tmp);
            stats.rejected = stats.rejected.saturating_add(1);
            warnings.push(Warning::new(
                "SOURCE_CHANGED",
                format!(
                    "source bytes at offset {} changed before writing {}; expected {}, wrote {}",
                    data.candidate.offset, rel_path, data.sha256, digest
                ),
            ));
            continue;
        }
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("cannot replace '{}'", path.display()))?;
        }
        if let Err(e) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(e).with_context(|| {
                format!("cannot rename '{}' to '{}'", tmp.display(), path.display())
            });
        }

        written = written.saturating_add(data.output_len);
        stats.bytes_written = stats.bytes_written.saturating_add(data.output_len);
        let mut notes = data.analysis.notes.clone();
        notes.extend(data.validation_notes.iter().cloned());
        let (lba, cluster, allocated, partition_index) =
            allocation_metadata(data.candidate.offset, volumes, mode);
        let record = CarvedFile {
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
            sha256_original: data.sha256_original.clone(),
        };
        if !opts.quiet && (opts.verbose || idx < 20) {
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
        warnings.push(Warning::new(
            "OUTPUT_LIMIT",
            format!(
                "--max-output {} reached after {} byte(s); {skipped_output} candidate(s) were not written",
                human_size(output_limit),
                written
            ),
        ));
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

fn write_manifest(out_dir: &Path, files: &[CarvedFile]) -> Result<()> {
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
        + report.stats.too_large_truncated
        + report.stats.duplicate_hashes
        + report.stats.suppressed_nested
        > 0
    {
        println!(
            "Filtered   : min-size={} min-score={} complete-only={} too-large-truncated={} duplicate-hash={} nested={}",
            report.stats.below_min_size,
            report.stats.below_min_score,
            report.stats.skipped_complete_only,
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
