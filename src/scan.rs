use crate::fs::FsInfo;
use crate::io::{read_exact_at, ImageSource};
use crate::signatures::{
    analyze, validate_deep, Analysis, FileType, Need, PdfEofMode, Status, Validation,
    ValidationMode,
};
use crate::util::{
    complement_extents, human_size, merge_extents, sha256_hex, unix_timestamp, Extent,
};
use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Report schema version emitted by this build.
pub const SCHEMA_VERSION: u8 = 2;

/// Default bound for B-06 gap carving, in volume clusters.
pub const DEFAULT_MAX_GAP_CLUSTERS: u64 = 16;
/// Default bound on candidate joins evaluated by B-06.
pub const DEFAULT_MAX_FRAG_CANDIDATES: usize = 1024;
/// Default wall-clock budget for B-06 (milliseconds).
pub const DEFAULT_FRAG_TIME_LIMIT_MS: u64 = 5_000;

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
        ReportOptions {
            types: self.types.iter().map(|t| t.ext().to_string()).collect(),
            mode: self.mode.as_str().to_string(),
            min_size: self.min_size,
            max_size: self.max_size,
            max_truncated_size: self.max_truncated_size,
            max_memory: self.max_memory,
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

/// Run-specific metadata that is not a scanning option.
#[derive(Debug, Clone, Default)]
pub struct ScanContext {
    pub argv: Vec<String>,
    pub case_id: Option<String>,
    pub examiner: Option<String>,
    pub image_sha256: Option<String>,
    pub image_mtime: Option<u64>,
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
    pub unchanged: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportOptions {
    pub types: Vec<String>,
    pub mode: String,
    pub min_size: u64,
    pub max_size: u64,
    pub max_truncated_size: u64,
    pub max_memory: u64,
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
                | "FS_DETECTION_FAILED"
                | "IMAGE_MODIFIED"
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
    pub validated_bytes: u64,
    pub lba: u64,
    pub cluster: Option<u64>,
    pub partition_index: Option<usize>,
    pub extent_id: usize,
    pub allocated: Option<bool>,
    pub source: String,
    pub fragments: Vec<Fragment>,
    /// Number of deep-validation checks made for a B-06 stitch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checks: Option<u64>,
    /// Confidence in a B-06 stitch (the structural score of the accepted join).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ScanStats {
    pub candidates: usize,
    pub carved: usize,
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
    pub warnings: Vec<Warning>,
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

struct CarvedData {
    candidate: Candidate,
    bytes: Vec<u8>,
    analysis: Analysis,
    sha256: String,
    repaired: bool,
    validation: Validation,
    validation_notes: Vec<String>,
    /// Set only for B-06 stitched output.
    fragments: Vec<Fragment>,
    checks: Option<u64>,
    confidence: Option<u8>,
}

enum CarveOutcome {
    Carved(CarvedData),
    Rejected(String),
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

    let mut candidates = find_candidates(
        source,
        &tasks,
        &opts.types,
        opts,
        volumes,
        &scan_progress,
        &pool,
    )?;
    candidates.sort_by_key(|c| (c.offset, c.kind as u8));
    candidates.dedup_by(|a, b| a.offset == b.offset && a.kind == b.kind);

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
    let outcomes = carve_candidates(source, &candidates, opts, &pool, &carve_progress)?;
    if let Some(h) = carve_thread {
        carve_progress.stop.store(true, Ordering::Relaxed);
        let _ = h.join();
        eprint!("\r{:100}\r", "");
    }

    let mut kept = postfilter(outcomes, opts, &mut stats);
    let mut warnings = Vec::new();
    if opts.fragment_recovery {
        kept = recover_fragments(
            source,
            &candidates,
            kept,
            &extents,
            volumes,
            opts,
            &mut warnings,
        );
    }
    if opts.verbose {
        for (idx, file) in kept.iter().enumerate() {
            eprintln!(
                "[+] {:<4} offset={:>12} size={:>9} score={:>3} {:<9} {:05}_{}_{:016x}.{}",
                file.candidate.kind.label(),
                file.candidate.offset,
                file.bytes.len(),
                file.analysis.score,
                format!("{:?}", file.analysis.status(file.repaired)).to_lowercase(),
                idx + 1,
                file.candidate.kind.ext(),
                file.candidate.offset,
                file.candidate.kind.ext()
            );
        }
    }

    let total_carved: u64 = kept.iter().map(|f| f.bytes.len() as u64).sum();
    if total_carved > opts.max_memory {
        bail!(
            "carved output would use {} which exceeds --max-memory {}; \
             reduce --max-size/--max-truncated-size or raise --max-memory",
            human_size(total_carved),
            human_size(opts.max_memory)
        );
    }

    let files = write_outputs(out_dir, &kept, volumes, mode, &mut stats, opts)?;
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

    let elapsed = t0.elapsed();
    let bytes_scanned = total_to_scan;
    let throughput = if elapsed.as_secs_f64() > 0.0 {
        bytes_scanned as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0)
    } else {
        0.0
    };

    let (size_after, mtime_after, unchanged) = match image_identity {
        Some(identity) => {
            let (size, mtime, readable) = identity.current();
            (
                size,
                mtime,
                readable && size == identity.initial_size() && mtime == identity.initial_mtime(),
            )
        }
        // Stdin has no backing path; the spool is controlled by this process.
        None => (image_len, ctx.image_mtime, true),
    };
    let input = InputInfo {
        path: image.to_string(),
        size: image_len,
        sha256: ctx.image_sha256.clone(),
        modified_unix: ctx.image_mtime,
        size_after,
        modified_unix_after: mtime_after,
        unchanged,
    };
    if !input.unchanged {
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
        warnings,
        files,
    })
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

fn find_candidates(
    source: &dyn ImageSource,
    tasks: &[Task],
    types: &[FileType],
    opts: &ScanOptions,
    volumes: &[FsInfo],
    progress: &Progress,
    pool: &rayon::ThreadPool,
) -> Result<Vec<Candidate>> {
    let found: Vec<Vec<Candidate>> = pool.install(|| {
        tasks
            .par_iter()
            .map(|task| scan_task(source, task, types, opts, volumes, progress))
            .collect::<Result<Vec<_>>>()
    })?;
    Ok(found.into_iter().flatten().collect())
}

fn scan_task(
    source: &dyn ImageSource,
    task: &Task,
    types: &[FileType],
    opts: &ScanOptions,
    volumes: &[FsInfo],
    progress: &Progress,
) -> Result<Vec<Candidate>> {
    let mut buf = vec![0u8; task.read_len as usize];
    read_exact_at(source, &mut buf, task.start)
        .with_context(|| format!("cannot read scan chunk at offset {}", task.start))?;
    let mut out = Vec::new();
    for &kind in types {
        let needle = kind.header();
        for rel in memchr::memmem::find_iter(&buf, needle) {
            if rel as u64 >= task.scan_len {
                continue;
            }
            if !plausible_header(kind, &buf, rel) {
                continue;
            }
            let offset = task.start + rel as u64;
            if !aligned(offset, opts.align, volumes) {
                continue;
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

fn carve_candidates(
    source: &dyn ImageSource,
    candidates: &[Candidate],
    opts: &ScanOptions,
    pool: &rayon::ThreadPool,
    progress: &Progress,
) -> Result<Vec<CarveOutcome>> {
    // Keep memory bounded and preserve candidate order: batches are carved in
    // parallel but appended in the original order.
    let mut outcomes = Vec::with_capacity(candidates.len());
    for batch in candidates.chunks(64) {
        let mut batch_out: Vec<CarveOutcome> = pool.install(|| {
            batch
                .par_iter()
                .map(|cand| {
                    let out = carve_one(source, cand, opts);
                    progress.done.fetch_add(1, Ordering::Relaxed);
                    out
                })
                .collect()
        });
        outcomes.append(&mut batch_out);
    }
    Ok(outcomes)
}

fn carve_one(source: &dyn ImageSource, cand: &Candidate, opts: &ScanOptions) -> CarveOutcome {
    let available = cand.extent_end.saturating_sub(cand.offset);
    let hard_limit = available.min(opts.max_size);
    if hard_limit < 16 {
        return CarveOutcome::Rejected("file candidate too close to extent end".into());
    }
    let mut buf_len = hard_limit.min(64 * 1024);
    let mut buf = vec![0u8; buf_len as usize];
    if let Err(e) = read_exact_at(source, &mut buf, cand.offset) {
        return CarveOutcome::Rejected(format!("read error: {e}"));
    }
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 64 {
            return CarveOutcome::Rejected("carve read growth guard exceeded".into());
        }
        let at_limit = buf_len >= hard_limit;
        let read_len = buf_len.min(buf.len() as u64) as usize;
        match analyze(cand.kind, &buf[..read_len], at_limit, opts.pdf_eof) {
            Need::Analyzed(mut analysis) => {
                if analysis.end == 0 || analysis.end > read_len {
                    return CarveOutcome::Rejected("invalid carved length".into());
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
                let mut bytes = buf[..end].to_vec();
                if !analysis.complete && bytes.len() as u64 > opts.max_truncated_size {
                    return CarveOutcome::TooLargeTruncated(bytes.len() as u64);
                }
                if analysis.score < opts.min_score {
                    return CarveOutcome::LowScore(analysis.score);
                }
                if opts.complete_only && !analysis.complete {
                    return CarveOutcome::NotComplete;
                }
                let (validation, mut validation_notes) = if opts.validate == ValidationMode::Decode
                {
                    if analysis.complete {
                        validate_deep(cand.kind, &bytes, &analysis)
                    } else {
                        (
                            Validation::Partial,
                            vec!["file is truncated; decode validation skipped".into()],
                        )
                    }
                } else if analysis.complete {
                    (Validation::Structure, Vec::new())
                } else {
                    (Validation::Partial, vec!["file is truncated".into()])
                };
                if opts.validate == ValidationMode::Decode
                    && analysis.complete
                    && validation == Validation::Partial
                {
                    analysis.score = analysis.score.saturating_sub(20);
                    analysis.notes.append(&mut validation_notes);
                    validation_notes = Vec::new();
                    if analysis.score < opts.min_score {
                        return CarveOutcome::LowScore(analysis.score);
                    }
                }
                let mut repaired = false;
                if opts.repair && !analysis.complete {
                    bytes.extend_from_slice(analysis.repair_suffix);
                    repaired = true;
                }
                let size = bytes.len() as u64;
                if size < opts.min_size {
                    return CarveOutcome::TooSmall(size);
                }
                let sha256 = sha256_hex(&bytes);
                return CarveOutcome::Carved(CarvedData {
                    candidate: cand.clone(),
                    bytes,
                    analysis,
                    sha256,
                    repaired,
                    validation,
                    validation_notes,
                    fragments: Vec::new(),
                    checks: None,
                    confidence: None,
                });
            }
            Need::More => {
                if at_limit {
                    return CarveOutcome::Rejected(
                        "parser requested more data at the carve limit".into(),
                    );
                }
                let next = buf_len.saturating_mul(2).min(hard_limit);
                if next <= buf_len {
                    return CarveOutcome::Rejected("carve buffer cannot grow".into());
                }
                buf.resize(next as usize, 0);
                if let Err(e) =
                    read_exact_at(source, &mut buf[buf_len as usize..], cand.offset + buf_len)
                {
                    return CarveOutcome::Rejected(format!("read error: {e}"));
                }
                buf_len = next;
            }
            Need::Reject(reason) => return CarveOutcome::Rejected(reason),
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
) -> Result<JoinAttempt> {
    let head_len = join.cut.saturating_sub(cand.offset);
    let tail_len = join.footer_end.saturating_sub(join.tail_start);
    if head_len == 0 || tail_len == 0 {
        return Ok(JoinAttempt {
            data: None,
            deep_checked: false,
        });
    }
    let total = match head_len.checked_add(tail_len) {
        Some(v) if v <= opts.max_size => v,
        _ => {
            return Ok(JoinAttempt {
                data: None,
                deep_checked: false,
            })
        }
    };
    let capacity = usize::try_from(total).unwrap_or(usize::MAX);
    let mut bytes = Vec::with_capacity(capacity);
    let mut head = vec![0u8; head_len as usize];
    read_exact_at(source, &mut head, cand.offset)
        .with_context(|| format!("cannot read B-06 head at {}", cand.offset))?;
    let mut tail = vec![0u8; tail_len as usize];
    read_exact_at(source, &mut tail, join.tail_start)
        .with_context(|| format!("cannot read B-06 tail at {}", join.tail_start))?;
    bytes.extend_from_slice(&head);
    bytes.extend_from_slice(&tail);

    let Need::Analyzed(mut analysis) = analyze(cand.kind, &bytes, true, opts.pdf_eof) else {
        return Ok(JoinAttempt {
            data: None,
            deep_checked: false,
        });
    };
    if !analysis.complete || analysis.end == 0 || analysis.end > bytes.len() {
        return Ok(JoinAttempt {
            data: None,
            deep_checked: false,
        });
    }
    if analysis.score < opts.min_score {
        return Ok(JoinAttempt {
            data: None,
            deep_checked: false,
        });
    }
    bytes.truncate(analysis.end);
    let used_tail = match analysis.end.checked_sub(head_len as usize) {
        Some(v) if v > 0 && v as u64 <= tail_len => v as u64,
        _ => {
            return Ok(JoinAttempt {
                data: None,
                deep_checked: false,
            })
        }
    };

    let (validation, validation_notes) = validate_deep(cand.kind, &bytes, &analysis);
    if validation != Validation::Decoded {
        return Ok(JoinAttempt {
            data: None,
            deep_checked: true,
        });
    }

    analysis.notes.push(format!(
        "B-06 deep-validated 2 fragments with a {} cluster gap",
        join.gap_clusters
    ));
    let confidence = analysis.score;
    let sha256 = sha256_hex(&bytes);
    Ok(JoinAttempt {
        data: Some(CarvedData {
            candidate: cand.clone(),
            bytes,
            analysis,
            sha256,
            repaired: false,
            validation,
            validation_notes,
            fragments: vec![
                Fragment {
                    offset: cand.offset,
                    len: head_len,
                },
                Fragment {
                    offset: join.tail_start,
                    len: used_tail,
                },
            ],
            checks: None,
            confidence: Some(confidence),
        }),
        deep_checked: true,
    })
}

fn recover_fragments(
    source: &dyn ImageSource,
    candidates: &[Candidate],
    mut kept: Vec<CarvedData>,
    extents: &[Extent],
    volumes: &[FsInfo],
    opts: &ScanOptions,
    warnings: &mut Vec<Warning>,
) -> Vec<CarvedData> {
    let started = Instant::now();
    if volumes.is_empty() {
        warnings.push(Warning::new(
            "FRAGMENT_RECOVERY_SKIPPED",
            "B-06 needs a detected volume with cluster boundaries; no stitching attempted",
        ));
        return kept;
    }

    let footers = match find_footers(source, extents, &opts.types, opts) {
        Ok(v) => v,
        Err(e) => {
            warnings.push(Warning::new(
                "FRAGMENT_FOOTER_SCAN_FAILED",
                format!("fragment footer scan failed: {e}"),
            ));
            return kept;
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

    for cand in candidates {
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
            if attempts >= opts.max_frag_candidates {
                candidate_limit = true;
                break;
            }
            if started.elapsed() >= opts.frag_time_limit {
                time_limit = true;
                break;
            }
            attempts += 1;
            let attempt = match try_fragment_join(source, cand, &join, opts) {
                Ok(v) => v,
                Err(e) => {
                    if opts.verbose {
                        eprintln!("[-] B-06 join at {} failed: {e}", cand.offset);
                    }
                    continue;
                }
            };
            if attempt.deep_checked {
                checks += 1;
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

    kept.retain(|f| !successful_keys.contains(&(f.candidate.offset, f.candidate.kind)));
    kept.extend(successes);
    kept.sort_by_key(|f| (f.candidate.offset, f.candidate.kind as u8));
    kept
}

fn postfilter(
    outcomes: Vec<CarveOutcome>,
    opts: &ScanOptions,
    stats: &mut ScanStats,
) -> Vec<CarvedData> {
    let mut candidates: Vec<CarvedData> = Vec::new();
    for outcome in outcomes {
        match outcome {
            CarveOutcome::Carved(data) => candidates.push(data),
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
        candidates.retain(|_| {
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
    candidates
}

fn write_outputs(
    out_dir: &Path,
    files: &[CarvedData],
    volumes: &[FsInfo],
    mode: ScanMode,
    stats: &mut ScanStats,
    opts: &ScanOptions,
) -> Result<Vec<CarvedFile>> {
    // With more than one scanned volume each file goes into its own
    // `part<N>/` sub-directory so outputs of different partitions cannot be
    // confused (A-07 `--partition all`).
    let multi = volumes.len() > 1;
    let mut records = Vec::with_capacity(files.len());
    for (idx, data) in files.iter().enumerate() {
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
        write_file_atomic(&path, &data.bytes)?;
        stats.bytes_written = stats.bytes_written.saturating_add(data.bytes.len() as u64);
        let mut notes = data.analysis.notes.clone();
        notes.extend(data.validation_notes.iter().cloned());
        if data.repaired {
            notes.push("repaired by appending the signature footer".into());
        }
        let (lba, cluster, allocated, partition_index) =
            allocation_metadata(data.candidate.offset, volumes, mode);
        let record = CarvedFile {
            seq,
            file_type: data.candidate.kind,
            offset: data.candidate.offset,
            size: data.bytes.len() as u64,
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
            validated_bytes: if data.validation == Validation::Decoded {
                data.analysis.end as u64
            } else {
                0
            },
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
            checks: data.checks,
            confidence: data.confidence,
        };
        if !opts.quiet && (opts.verbose || idx < 20) {
            eprintln!(
                "[+] {:<4} offset={:>12} size={:>9} score={:>3} {:<9} {}",
                record.file_type.label(),
                record.offset,
                record.size,
                record.score,
                format!("{:?}", record.status).to_lowercase(),
                record.path
            );
        }
        records.push(record);
    }
    Ok(records)
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
        "{:<6} {:>7} {:>9} {:>10} {:>9} {:>12}",
        "TYPE", "FILES", "COMPLETE", "TRUNCATED", "REPAIRED", "BYTES"
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
        let complete = files
            .iter()
            .filter(|f| f.status == Status::Complete)
            .count();
        let truncated = files
            .iter()
            .filter(|f| f.status != Status::Complete)
            .count();
        let repaired = files.iter().filter(|f| f.repaired).count();
        let bytes: u64 = files.iter().map(|f| f.size).sum();
        println!(
            "{:<6} {:>7} {:>9} {:>10} {:>9} {:>12}",
            kind.label(),
            files.len(),
            complete,
            truncated,
            repaired,
            human_size(bytes)
        );
    }
    println!("Output: {}", report.output_dir);
}
