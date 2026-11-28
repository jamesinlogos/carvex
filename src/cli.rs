use crate::fs::{self, FsInfo, PartitionSel};
use crate::io::{
    open_image_with_options, sha256_source, FreeStreamSource, InputFormat, OnReadError,
    OpenOptions, OpenedImage, RegionLog, UnreadableRegion,
};
use crate::report;
use crate::scan::{
    self, AlignMode, Fragment, MetadataPassStats, MetadataResult, ScanContext, ScanMode,
    ScanOptions, ScanReport, VolumeSummary, Warning, DEFAULT_FRAG_TIME_LIMIT_MS,
    DEFAULT_MAX_FRAG_CANDIDATES, DEFAULT_MAX_GAP_CLUSTERS, DEFAULT_MAX_REGIONS, SCHEMA_VERSION,
};
use crate::signatures::{FileType, PdfEofMode, ValidationMode};
use crate::util::{human_size, parse_size, Extent};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// `--partition largest|all|<N>` (A-07).
fn parse_partition_sel(s: &str) -> Result<PartitionSel, String> {
    s.parse()
}

#[derive(Parser)]
#[command(
    name = "carvex",
    version,
    about = "Header/footer file carver for JPEG, PNG and PDF with FAT32/NTFS awareness",
    long_about = "CarveX recovers deleted JPEG, PNG and PDF files from raw disk images.\n\
It combines signature carving (header/footer) with structural validation and,\n\
for FAT32/NTFS, restricts scanning to unallocated clusters reported by the\n\
filesystem allocation metadata (FAT or $Bitmap).",
    after_help = "EXAMPLES:\n  \
carvex info disk.img\n  \
carvex scan disk.img -o recovered --mode unallocated --repair\n  \
carvex scan disk.img -o raw-out --mode raw --types jpg,png,pdf --json-report report.json\n  \
carvex scan disk.img -o out --partition 1 --verbose\n\n\
Note: always work on a forensic copy; CarveX opens the image read-only."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
// A boxed ScanArgs would only move the cost around; the enum lives on the
// stack for one parse.
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Identify the filesystem and print volume/allocation statistics
    Info(InfoArgs),
    /// Scan an image and write carved files plus a report
    Scan(ScanArgs),
    /// Show built-in signatures and validators
    Signatures,
}

#[derive(Parser)]
struct InfoArgs {
    /// Disk image or volume image (`-` reads stdin)
    image: PathBuf,
    /// Image container: auto, raw, split, e01, vhd, vmdk or stdin
    #[arg(long, value_enum, default_value_t = InputFormat::Auto)]
    input_format: InputFormat,
    /// Allow VMDK extents outside the descriptor directory (unsafe)
    #[arg(long)]
    allow_external_extents: bool,
    /// What to do when a byte range cannot be read (default: zero-fill)
    #[arg(long, value_enum, default_value_t = OnReadError::ZeroFill)]
    on_read_error: OnReadError,
    /// Maximum bytes spooled from stdin (0 = unlimited)
    #[arg(long, value_parser = parse_size, default_value = "0")]
    max_stdin_bytes: u64,
    /// Directory for the stdin spool file
    #[arg(long)]
    spool_dir: Option<PathBuf>,
    /// Select a 1-based MBR slot / GPT partition number
    #[arg(long, conflicts_with = "offset")]
    partition: Option<usize>,
    /// Force the volume boot-sector offset (bytes, suffix K/M/G allowed)
    #[arg(long, value_parser = parse_size, conflicts_with = "partition")]
    offset: Option<u64>,
    /// Print machine-readable JSON
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
struct ScanArgs {
    /// Disk image or volume image (`-` reads stdin; never modified)
    image: PathBuf,
    /// Image container: auto, raw, split, e01, vhd, vmdk or stdin
    #[arg(long, value_enum, default_value_t = InputFormat::Auto)]
    input_format: InputFormat,
    /// Allow VMDK extents outside the descriptor directory (unsafe)
    #[arg(long)]
    allow_external_extents: bool,
    /// What to do when a byte range cannot be read (default: zero-fill)
    #[arg(long, value_enum, default_value_t = OnReadError::ZeroFill)]
    on_read_error: OnReadError,
    /// Maximum bytes spooled from stdin (0 = unlimited)
    #[arg(long, value_parser = parse_size, default_value = "0")]
    max_stdin_bytes: u64,
    /// Directory for the stdin spool file
    #[arg(long)]
    spool_dir: Option<PathBuf>,
    /// Output directory for carved files (must be empty unless --force)
    #[arg(short, long)]
    output: PathBuf,
    /// Comma-separated types: jpeg,jpg,png,pdf
    #[arg(
        short = 't',
        long = "types",
        value_delimiter = ',',
        default_value = "jpeg,png,pdf"
    )]
    types: Vec<FileType>,
    /// Space to scan: auto, unallocated, raw, allocated
    #[arg(long, value_enum, default_value_t = ScanMode::Auto)]
    mode: ScanMode,
    /// Minimum carved file size (bytes, suffix K/M/G allowed)
    #[arg(long, value_parser = parse_size, default_value = "256")]
    min_size: u64,
    /// Maximum bytes to inspect after a header before giving up
    #[arg(long, value_parser = parse_size, default_value = "64M")]
    max_size: u64,
    /// Maximum size of a truncated file written without --repair (0 = unlimited)
    #[arg(long, value_parser = parse_size, default_value = "16M")]
    max_truncated_size: u64,
    /// Maximum concurrent working memory for carve, join and write buffers;
    /// carved output is streamed from the source, not retained in RAM
    #[arg(long, value_parser = parse_size, default_value = "1G")]
    max_memory: u64,
    /// Maximum working memory for one decode validator (0 = default)
    #[arg(long, visible_alias = "max-decode-bytes", value_parser = parse_size, default_value = "256M")]
    max_validator_memory: u64,
    /// Deterministic limit on header candidates evaluated (0 = default limit)
    #[arg(long, value_parser = clap::value_parser!(u64), default_value_t = scan::DEFAULT_MAX_CANDIDATES)]
    max_candidates: u64,
    /// Deterministic analyzer + validator byte budget for one candidate (0 = unlimited)
    #[arg(long, value_parser = parse_size, default_value = "256M")]
    max_work_per_candidate: u64,
    /// Cap on bytes written by one scan (0 = unlimited)
    #[arg(long, value_parser = parse_size, default_value = "4G")]
    max_output: u64,
    /// Overall scan wall-clock budget in seconds (0 = unlimited); stops with a partial report
    #[arg(long, value_parser = clap::value_parser!(u64), default_value_t = scan::DEFAULT_TIME_BUDGET_SECS)]
    time_budget: u64,
    /// Read chunk size for parallel scanning
    #[arg(long, value_parser = parse_size, default_value = "4M")]
    chunk_size: u64,
    /// Worker threads (0 = number of CPUs)
    #[arg(long, default_value_t = 0)]
    threads: usize,
    /// Minimum structural validation score (0-100)
    #[arg(long, default_value_t = 50)]
    min_score: u8,
    /// Repair truncated files only with a deep proof (PDF: xref/trailer rebuild; PNG/JPEG: crop to recovered rows); progressive JPEG and interlaced PNG stay partial
    #[arg(long)]
    repair: bool,
    /// With --repair, score a deep-validated repair on the repaired bytes for --min-score; this is the default (§9 Q13 / D-79)
    #[arg(long, conflicts_with = "no_repair_rescore")]
    repair_rescore: bool,
    /// With --repair, keep the carved-prefix score for --min-score (legacy pre-Q13 behavior)
    #[arg(long, conflicts_with = "repair_rescore")]
    no_repair_rescore: bool,
    /// Only write files whose footer was found, not verified (see validity)
    #[arg(long)]
    complete_only: bool,
    /// Only write files whose authoritative validity is verified
    #[arg(long)]
    verified_only: bool,
    /// PDF %%EOF selection strategy
    #[arg(long, value_enum, default_value_t = PdfEofMode::Smart)]
    pdf_eof: PdfEofMode,
    /// Drop later files with an identical SHA-256
    #[arg(long)]
    dedupe_hashes: bool,
    /// Keep images/PDFs embedded inside a complete carved container
    #[arg(long)]
    keep_nested: bool,
    /// Validate structure only, or also decode the payload
    #[arg(long, value_enum, default_value_t = ValidationMode::Structure)]
    validate: ValidationMode,
    /// Candidate alignment: none (historical), auto, cluster, sector
    #[arg(long, value_enum, default_value_t = AlignMode::None)]
    align: AlignMode,
    /// Allow writing into a non-empty output directory
    #[arg(long)]
    force: bool,
    /// Write SHA256SUMS for the carved files
    #[arg(long)]
    manifest: bool,
    /// Select partitions: 'all' (default), 'largest' (historical) or a 1-based number
    #[arg(long, value_parser = parse_partition_sel, conflicts_with = "offset")]
    partition: Option<PartitionSel>,
    /// Scan image regions outside every supported volume (gaps and
    /// partitions without a supported filesystem); on by default
    #[arg(long, conflicts_with = "no_gaps")]
    include_gaps: bool,
    /// Restore the historical behaviour: do not scan gaps or unsupported
    /// partitions (use with `--partition largest`)
    #[arg(long, conflicts_with = "include_gaps")]
    no_gaps: bool,
    /// Maximum detailed `regions[]` rows before adjacent rows of the same
    /// scanned state are summarised (0 = unlimited; H-27)
    #[arg(long, value_parser = clap::value_parser!(u64), default_value_t = DEFAULT_MAX_REGIONS)]
    max_regions: u64,
    /// Opt in to bounded B-06 gap carving (filler/header gaps; PNG chunk-CRC gaps)
    /// Filler is zero/low-cardinality (<=8 distinct bytes) or a repeated period <=32.
    /// PNG chunk-CRC gaps require both --fragment-recovery and --validate decode;
    /// with --fragment-recovery alone those gaps are not joined. A unique CRC match
    /// is still checked by deep decode validation; ambiguous joins are suspect.
    #[arg(long)]
    fragment_recovery: bool,
    /// Maximum gap between joined fragments, in clusters (B-06)
    #[arg(
        long,
        value_parser = clap::value_parser!(u64).range(1..=1_000_000),
        default_value_t = DEFAULT_MAX_GAP_CLUSTERS
    )]
    max_gap: u64,
    /// Maximum candidate joins evaluated by B-06
    #[arg(
        long,
        value_parser = clap::value_parser!(u64).range(1..=1_000_000),
        default_value_t = DEFAULT_MAX_FRAG_CANDIDATES as u64
    )]
    max_frag_candidates: u64,
    /// Wall-clock budget for B-06, in milliseconds
    #[arg(
        long,
        value_parser = clap::value_parser!(u64).range(1..=600_000),
        default_value_t = DEFAULT_FRAG_TIME_LIMIT_MS
    )]
    frag_time_limit: u64,
    /// H-27: add a second pass over one volume's free-space stream (concatenated free extents).
    /// Only ascending-LBA fragments are recovered; allocated clusters are never read. Requires
    /// exactly one selected volume and filesystem detection.
    #[arg(long)]
    free_stream: bool,
    /// H-28/H-29: recover deleted FAT32 directory entries and NTFS MFT records.
    /// Requires filesystem detection and at least one selected volume.
    #[arg(long)]
    metadata: bool,
    /// Force the volume boot-sector offset (bytes, suffix K/M/G allowed)
    #[arg(long, value_parser = parse_size, conflicts_with = "partition")]
    offset: Option<u64>,
    /// Skip filesystem detection (raw scanning only)
    #[arg(long)]
    no_fs: bool,
    /// Hash the complete source image (streaming, can be slow)
    #[arg(long)]
    hash_image: bool,
    /// Case identifier stored in the report
    #[arg(long)]
    case_id: Option<String>,
    /// Examiner name stored in the report
    #[arg(long)]
    examiner: Option<String>,
    /// Write a machine-readable JSON report (schema 1 or 2)
    #[arg(long)]
    json_report: Option<PathBuf>,
    /// Write a CSV report
    #[arg(long)]
    csv_report: Option<PathBuf>,
    /// JSON report schema: 2 (current) or 1 (legacy shape)
    #[arg(long, default_value_t = SCHEMA_VERSION, value_parser = clap::value_parser!(u8).range(1..=2))]
    report_schema: u8,
    /// Suppress progress and summary output
    #[arg(long, conflicts_with = "verbose")]
    quiet: bool,
    /// Print every candidate/rejection decision
    #[arg(long)]
    verbose: bool,
}

/// Run the CLI and return the process exit code.
///
/// 0 = success, 1 = runtime error (returned as `Err` by `main`),
/// 2 = clap usage error, 3 = finished but incomplete (warnings that mean
/// evidence was skipped, see `Warning::incomplete`).
pub fn run() -> Result<i32> {
    let cli = Cli::parse();
    match cli.command {
        Command::Info(args) => info(args),
        Command::Scan(args) => scan(args),
        Command::Signatures => {
            crate::signatures::print_signatures();
            Ok(0)
        }
    }
}

/// Default scan accounting used by `info`: all volumes and gaps are scanned
/// (A-07 step 2), free space only inside the supported volumes.
fn default_scan_flags() -> (bool, bool, bool, bool) {
    // (whole_image_scanned, scans_free, scans_allocated, include_gaps)
    (false, true, false, true)
}

fn print_info(
    args: &InfoArgs,
    image_len: u64,
    primary: &FsInfo,
    volumes: Vec<VolumeSummary>,
    regions: Vec<crate::fs::Region>,
) -> Result<i32> {
    if args.json {
        #[derive(Serialize)]
        struct Doc<'a> {
            schema_version: u8,
            image: String,
            image_size: u64,
            filesystem: &'a FsInfo,
            volumes: &'a [VolumeSummary],
            regions: &'a [crate::fs::Region],
        }
        let doc = Doc {
            schema_version: SCHEMA_VERSION,
            image: args.image.display().to_string(),
            image_size: image_len,
            filesystem: primary,
            volumes: &volumes,
            regions: &regions,
        };
        println!("{}", serde_json::to_string_pretty(&doc)?);
        return Ok(0);
    }

    fs::print_info(&args.image, primary);
    println!();
    println!(
        "Unallocated space available for carving: {} ({} clusters)",
        human_size(primary.free_bytes()),
        primary.free_clusters
    );
    if volumes.len() > 1 || !regions.is_empty() {
        println!();
        println!(
            "Volumes detected : {} (default scan selects all volumes and gaps)",
            volumes.len()
        );
        for v in &volumes {
            println!(
                "  [{}] {} at byte {} ({}), default scan: {}",
                v.index,
                v.fs,
                v.offset,
                human_size(v.length),
                if v.scanned { "yes" } else { "NO" }
            );
        }
        println!("Regions:");
        for r in &regions {
            println!(
                "  {:<11} offset={:<12} len={:<12} scanned={}",
                r.kind.to_string(),
                r.offset,
                format!("{} ({})", r.len, human_size(r.len)),
                if r.scanned { "yes" } else { "no" }
            );
        }
    }
    Ok(0)
}

fn info(args: InfoArgs) -> Result<i32> {
    let options = OpenOptions {
        allow_external_extents: args.allow_external_extents,
        on_read_error: args.on_read_error,
        max_stdin_bytes: args.max_stdin_bytes,
        spool_dir: args.spool_dir.clone(),
    };
    let image = open_image_with_options(&args.image, args.input_format, &options)?;
    let len = image.len;
    let source = image.source.as_ref();

    // A forced offset or an explicit `--partition` is a single-volume view:
    // the user opted out of region accounting (same rule as `scan --offset`).
    if args.offset.is_some() || args.partition.is_some() {
        let fs = fs::detect_guarded(source, len, args.offset, args.partition)?;
        let volumes = vec![VolumeSummary::from_fs(1, &fs, true)];
        return print_info(&args, len, &fs, volumes, Vec::new());
    }

    let set = fs::detect_all_guarded(source, len)?;
    let largest = fs::select_volumes(&set, PartitionSel::Largest)?;
    let selected = fs::select_volumes(&set, PartitionSel::All)?;
    // Keep the largest volume as the `filesystem` summary; `volumes[]` and
    // `regions[]` describe the new all-volumes-plus-gaps default.
    let primary = largest
        .first()
        .or_else(|| selected.first())
        .context("no supported FAT32/NTFS filesystem found")?;
    let volumes: Vec<VolumeSummary> = set
        .volumes
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let chosen = selected
                .iter()
                .any(|s| s.offset == v.offset && s.length == v.length);
            VolumeSummary::from_fs(i + 1, v, chosen)
        })
        .collect();
    let (whole, free, alloc, gaps) = default_scan_flags();
    let regions = set.regions(&selected, whole, free, alloc, gaps);
    print_info(&args, len, primary, volumes, regions)
}

fn announce_fs(args: &ScanArgs, f: &FsInfo) {
    if args.quiet {
        return;
    }
    eprintln!(
        "Filesystem: {} at offset {}, {} free ({:.1}% in {} extent(s))",
        f.kind,
        f.offset,
        human_size(f.free_bytes()),
        if f.total_clusters == 0 {
            0.0
        } else {
            f.free_clusters as f64 * 100.0 / f.total_clusters as f64
        },
        f.free_extents.len()
    );
}

fn write_reports(report: &scan::ScanReport, args: &ScanArgs) -> Result<()> {
    if let Some(path) = &args.json_report {
        report::write_json(report, path, args.report_schema)?;
        if !args.quiet {
            println!("JSON report: {}", path.display());
        }
    }
    if let Some(path) = &args.csv_report {
        report::write_csv(report, path)?;
        if !args.quiet {
            println!("CSV report : {}", path.display());
        }
    }
    Ok(())
}

fn validate_scan_args(args: &ScanArgs) -> Result<()> {
    if !args.fragment_recovery
        && (args.max_gap != DEFAULT_MAX_GAP_CLUSTERS
            || args.max_frag_candidates != DEFAULT_MAX_FRAG_CANDIDATES as u64
            || args.frag_time_limit != DEFAULT_FRAG_TIME_LIMIT_MS)
    {
        bail!("B-06 limits require --fragment-recovery");
    }
    if args.free_stream && args.no_fs {
        bail!("--free-stream requires filesystem detection (remove --no-fs)");
    }
    if args.free_stream && args.mode == ScanMode::Allocated {
        bail!("--free-stream cannot be combined with --mode allocated");
    }
    if args.metadata && args.no_fs {
        bail!("--metadata requires filesystem detection (remove --no-fs)");
    }
    if args.metadata && args.mode == ScanMode::Allocated {
        bail!("--metadata cannot be combined with --mode allocated");
    }
    Ok(())
}

#[derive(Default)]
struct ScanSelection {
    volumes: Vec<FsInfo>,
    extra: Vec<Extent>,
    volume_rows: Vec<VolumeSummary>,
    regions: Vec<crate::fs::Region>,
    warnings: Vec<Warning>,
}

fn forced_offset_selection(
    args: &ScanArgs,
    source: &dyn crate::io::ImageSource,
    len: u64,
    off: u64,
) -> Result<ScanSelection> {
    // Forced boot-sector offset: a single volume, no partition table
    // interpretation, therefore no region accounting (user opt-out).
    let mut selection = ScanSelection::default();
    let f =
        fs::detect_guarded(source, len, Some(off), None).context("filesystem detection failed")?;
    announce_fs(args, &f);
    selection
        .volume_rows
        .push(VolumeSummary::from_fs(1, &f, true));
    selection.volumes.push(f.clone());
    // A forced offset opts out of partition accounting, but H-27 still
    // reports the real allocation extents of that single volume.
    let single = fs::VolumeSet {
        volumes: vec![f],
        partitions: Vec::new(),
        unsupported: Vec::new(),
        errors: Vec::new(),
        gaps: Vec::new(),
        superfloppy: true,
        image_len: len,
    };
    let whole = args.mode == ScanMode::Raw;
    let scans_free = matches!(args.mode, ScanMode::Auto | ScanMode::Unallocated);
    let scans_alloc = args.mode == ScanMode::Allocated;
    let (_, planned) = scan::plan_extents(args.mode, &selection.volumes, &[], len)?;
    selection.regions = single.regions_with_plan(fs::RegionPlanSpec {
        selected: &selection.volumes,
        whole_image_scanned: whole,
        scans_free,
        scans_allocated: scans_alloc,
        include_gaps: false,
        plan: &planned,
        max_regions: args.max_regions,
    });
    Ok(selection)
}

fn detection_failure_selection(
    args: &ScanArgs,
    set: &fs::VolumeSet,
    len: u64,
    include_gaps: bool,
) -> Result<ScanSelection> {
    // The fallback reads the whole image, so every region of the failed
    // detection is still scanned.
    let volume_rows = set
        .volumes
        .iter()
        .enumerate()
        .map(|(i, v)| VolumeSummary::from_fs(i + 1, v, true))
        .collect();
    let (_, planned) = scan::plan_extents(ScanMode::Raw, &[], &[], len)?;
    let regions = set.regions_with_plan(fs::RegionPlanSpec {
        selected: &[],
        whole_image_scanned: true,
        scans_free: false,
        scans_allocated: false,
        include_gaps,
        plan: &planned,
        max_regions: args.max_regions,
    });
    Ok(ScanSelection {
        volume_rows,
        regions,
        ..ScanSelection::default()
    })
}

fn append_not_scanned_warnings(
    selection: &mut ScanSelection,
    set: &fs::VolumeSet,
    include_gaps: bool,
    whole_image: bool,
) {
    // A-07: say out loud what this run does not read.
    if whole_image {
        return;
    }
    for v in &set.volumes {
        let chosen = selection
            .volumes
            .iter()
            .any(|s| s.offset == v.offset && s.length == v.length);
        if !chosen {
            selection.warnings.push(Warning::new(
                "VOLUME_NOT_SCANNED",
                format!(
                    "{} at byte {} ({}) was not scanned; \
                 use --partition all to scan every volume",
                    v.partition_index
                        .map_or_else(|| "volume".into(), |i| format!("partition {i}")),
                    v.offset,
                    human_size(v.length)
                ),
            ));
        }
    }
    if include_gaps {
        return;
    }
    for p in &set.unsupported {
        let detail = set.errors.iter().find(|e| e.partition_index == p.index);
        selection.warnings.push(Warning::new(
            "VOLUME_UNSUPPORTED",
            format!(
                "partition {} (type {}, byte {}) was not scanned: \
             no supported FAT32/NTFS filesystem{}; \
             use --include-gaps to scan it raw",
                p.index,
                p.type_name,
                p.byte_offset(512),
                detail.map(|d| format!(" ({d})")).unwrap_or_default()
            ),
        ));
    }
    if !set.gaps.is_empty() {
        let total: u64 = set.gaps.iter().map(|g| g.len).sum();
        selection.warnings.push(Warning::new(
            "GAP_NOT_SCANNED",
            format!(
                "{} image region(s) outside every partition \
             ({} in total) were not scanned; use --include-gaps",
                set.gaps.len(),
                human_size(total)
            ),
        ));
    }
}

fn selected_volumes_selection(
    args: &ScanArgs,
    set: &fs::VolumeSet,
    selected: Vec<FsInfo>,
    len: u64,
    include_gaps: bool,
) -> Result<ScanSelection> {
    let mut selection = ScanSelection {
        volumes: selected,
        ..ScanSelection::default()
    };
    for f in &selection.volumes {
        announce_fs(args, f);
    }
    if include_gaps {
        selection.extra = set.unscanned_extents();
    }
    let whole_image = args.mode == ScanMode::Raw;
    let scans_free = matches!(args.mode, ScanMode::Auto | ScanMode::Unallocated);
    let scans_alloc = args.mode == ScanMode::Allocated;
    selection.volume_rows = set
        .volumes
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let chosen = selection
                .volumes
                .iter()
                .any(|s| s.offset == v.offset && s.length == v.length);
            VolumeSummary::from_fs(i + 1, v, whole_image || chosen)
        })
        .collect();
    let (_, planned) = scan::plan_extents(args.mode, &selection.volumes, &selection.extra, len)?;
    selection.regions = set.regions_with_plan(fs::RegionPlanSpec {
        selected: &selection.volumes,
        whole_image_scanned: whole_image,
        scans_free,
        scans_allocated: scans_alloc,
        include_gaps,
        plan: &planned,
        max_regions: args.max_regions,
    });
    append_not_scanned_warnings(&mut selection, set, include_gaps, whole_image);
    Ok(selection)
}

fn detect_all_selection(
    args: &ScanArgs,
    source: &dyn crate::io::ImageSource,
    len: u64,
    needs_fs: bool,
    include_gaps: bool,
) -> Result<ScanSelection> {
    let selection_kind = args.partition.unwrap_or(PartitionSel::All);
    let explicit = args.partition.is_some();
    match fs::detect_all_guarded(source, len) {
        Err(e) => {
            if explicit || needs_fs {
                return Err(e).context("filesystem detection failed");
            }
            let mut selection = ScanSelection::default();
            selection.warnings.push(Warning::new(
                "FS_DETECTION_FAILED",
                format!("filesystem detection failed; used raw scan: {e:#}"),
            ));
            Ok(selection)
        }
        Ok(set) => {
            let mut selection = ScanSelection::default();
            // H-26: a recognised but unparsable volume is a degraded run,
            // even when another volume parsed and the scan can continue.
            for error in &set.errors {
                selection.warnings.push(Warning::new(
                    "VOLUME_PARSE_FAILED",
                    format!(
                        "partition {} at byte {} could not be parsed: {}",
                        error.partition_index, error.offset, error.reason
                    ),
                ));
            }
            match fs::select_volumes(&set, selection_kind) {
                Err(e) => {
                    if explicit || needs_fs {
                        return Err(e).context("filesystem detection failed");
                    }
                    let mut failed = detection_failure_selection(args, &set, len, include_gaps)?;
                    let mut warnings = std::mem::take(&mut selection.warnings);
                    warnings.push(Warning::new(
                        "FS_DETECTION_FAILED",
                        format!("filesystem detection failed; used raw scan: {e:#}"),
                    ));
                    failed.warnings = warnings;
                    Ok(failed)
                }
                Ok(selected) => {
                    let mut selected =
                        selected_volumes_selection(args, &set, selected, len, include_gaps)?;
                    let mut warnings = std::mem::take(&mut selection.warnings);
                    warnings.append(&mut selected.warnings);
                    selected.warnings = warnings;
                    Ok(selected)
                }
            }
        }
    }
}

fn select_scan_regions(
    args: &ScanArgs,
    source: &dyn crate::io::ImageSource,
    len: u64,
) -> Result<ScanSelection> {
    let needs_fs = matches!(args.mode, ScanMode::Unallocated | ScanMode::Allocated);
    // A-07 step 2: gaps are included by default; `--no-gaps` restores the
    // historical step-1 behaviour. `--include-gaps` remains accepted for
    // scripts written against the old CLI.
    let include_gaps = args.include_gaps || !args.no_gaps;

    let mut selection = if args.no_fs {
        if needs_fs {
            bail!("--no-fs is incompatible with --mode unallocated/allocated");
        }
        // No detection: the user opted out of filesystem and region accounting.
        ScanSelection::default()
    } else if let Some(off) = args.offset {
        forced_offset_selection(args, source, len, off)?
    } else {
        detect_all_selection(args, source, len, needs_fs, include_gaps)?
    };

    if args.mode == ScanMode::Raw && !selection.volumes.is_empty() {
        selection.warnings.push(Warning::new(
            "RAW_SCANS_ALLOCATED",
            "raw mode also scans allocated clusters",
        ));
    }

    // No filesystem metadata (or detection failed outright): the scanner
    // reads whole-image extents, so report them as scanned gap regions rather
    // than leaving `regions[]` empty (H-27).
    if selection.regions.is_empty() {
        let (_, planned) =
            scan::plan_extents(args.mode, &selection.volumes, &selection.extra, len)?;
        selection.regions = planned
            .iter()
            .enumerate()
            .map(|(extent_id, extent)| fs::Region {
                kind: fs::RegionKind::Gap,
                offset: extent.offset,
                len: extent.len,
                scanned: true,
                partition_index: None,
                may_contain_allocated: false,
                extent_id: Some(extent_id),
                summarized: false,
            })
            .collect();
    }
    Ok(selection)
}

fn build_scan_options(
    args: &ScanArgs,
    include_gaps: bool,
    image_sha256: Option<String>,
    image_mtime: Option<u64>,
    input_format: String,
    input_physical_size: u64,
) -> (ScanOptions, ScanContext) {
    let opts = ScanOptions {
        types: args.types.clone(),
        mode: args.mode,
        min_size: args.min_size,
        max_size: args.max_size,
        max_truncated_size: if args.max_truncated_size == 0 {
            u64::MAX
        } else {
            args.max_truncated_size
        },
        max_memory: args.max_memory,
        max_validator_memory: args.max_validator_memory,
        chunk_size: args.chunk_size,
        threads: args.threads,
        min_score: args.min_score,
        repair: args.repair,
        score_repaired: args.repair_rescore || !args.no_repair_rescore,
        complete_only: args.complete_only,
        verified_only: args.verified_only,
        pdf_eof: args.pdf_eof,
        dedupe_hashes: args.dedupe_hashes,
        suppress_nested: !args.keep_nested,
        validate: args.validate,
        align: args.align,
        force: args.force,
        quiet: args.quiet,
        verbose: args.verbose,
        manifest: args.manifest,
        partition: args.partition.unwrap_or(PartitionSel::All).to_string(),
        include_gaps,
        max_candidates: args.max_candidates,
        max_work_per_candidate: args.max_work_per_candidate,
        max_output: args.max_output,
        time_budget: std::time::Duration::from_secs(args.time_budget),
        max_regions: args.max_regions,
        fragment_recovery: args.fragment_recovery,
        max_gap: args.max_gap,
        max_frag_candidates: args.max_frag_candidates as usize,
        frag_time_limit: std::time::Duration::from_millis(args.frag_time_limit),
        free_stream: args.free_stream,
        metadata: args.metadata,
    };
    let ctx = ScanContext {
        argv: std::env::args().collect(),
        case_id: args.case_id.clone(),
        examiner: args.examiner.clone(),
        image_sha256,
        image_mtime,
        input_format,
        input_physical_size: Some(input_physical_size),
    };
    (opts, ctx)
}

fn print_scan_banner(args: &ScanArgs, len: u64) {
    if args.quiet {
        return;
    }
    eprintln!(
        "CarveX {} | image {} | types {} | mode {}{}",
        env!("CARGO_PKG_VERSION"),
        human_size(len),
        args.types
            .iter()
            .map(|t| t.label())
            .collect::<Vec<_>>()
            .join(","),
        args.mode.as_str(),
        if args.repair { " | repair" } else { "" }
    );
}

fn unreadable_warning(regions: &[UnreadableRegion]) -> Option<Warning> {
    if regions.is_empty() {
        return None;
    }
    Some(Warning::new(
        "UNREADABLE_REGION",
        format!(
            "{} source byte range(s) could not be read; affected outputs are not verified",
            regions.len()
        ),
    ))
}

/// Inputs to the schema-valid partial report left behind by a read-error
/// abort, grouped for H-22 without changing warning or write order.
struct AbortReportSpec<'a, 'b> {
    input: &'a scan::ScanInput<'b>,
    opts: &'a ScanOptions,
    ctx: &'a ScanContext,
    args: &'a ScanArgs,
    warnings: Vec<Warning>,
    volume_rows: Vec<VolumeSummary>,
    regions: Vec<crate::fs::Region>,
    unreadable_regions: &'a [UnreadableRegion],
}

fn write_abort_partial_report(spec: AbortReportSpec<'_, '_>) -> Result<()> {
    let AbortReportSpec {
        input,
        opts,
        ctx,
        args,
        mut warnings,
        volume_rows,
        regions,
        unreadable_regions,
    } = spec;
    // `abort` intentionally returns exit 1, but the run must still leave a
    // schema-valid partial report when unreadable bytes were the reason it
    // stopped.
    if args.on_read_error != OnReadError::Abort || unreadable_regions.is_empty() {
        return Ok(());
    }
    let first = &unreadable_regions[0];
    warnings.push(Warning::new(
        "READ_ABORTED",
        format!(
            "scan aborted at byte {} after an unreadable source range: {}",
            first.offset, first.reason
        ),
    ));
    let mut partial = scan::partial_report(input, opts, ctx, warnings);
    partial.volumes = volume_rows;
    partial.regions = regions;
    write_reports(&partial, args)
}

fn finish_scan_report(
    mut report: scan::ScanReport,
    args: &ScanArgs,
    volume_rows: Vec<VolumeSummary>,
    regions: Vec<crate::fs::Region>,
    warnings: Vec<Warning>,
) -> Result<i32> {
    // A-07: every detected volume and region, with whether this run read it.
    report.volumes = volume_rows;
    report.regions = regions;
    report.warnings.extend(warnings);
    if !args.quiet {
        scan::print_summary(&report);
    }
    for warning in &report.warnings {
        eprintln!("warning[{}]: {}", warning.code, warning.message);
    }
    write_reports(&report, args)?;
    // H-28 takes precedence over the generic incomplete exit: SIGINT always
    // reports as interrupted, even when other warnings are also present.
    if report.interrupted {
        return Ok(4);
    }
    // Exit 3 = finished but incomplete: evidence was left unscanned (A-07).
    Ok(
        if report.warnings.iter().any(super::scan::Warning::incomplete) {
            3
        } else {
            0
        },
    )
}

/// H-27: run the unchanged scan pipeline over one volume's free-space stream.
/// The logical source is a read-only adapter; coordinate translation happens
/// when the results are merged back into the physical report.
fn run_free_stream_pass<'a>(
    physical: &'a dyn crate::io::ImageSource,
    volume: &FsInfo,
    base_opts: &ScanOptions,
    ctx: &ScanContext,
    interrupt: scan::Interrupt,
    temp_dir: &'a Path,
) -> Result<(FreeStreamSource<'a>, Option<ScanReport>)> {
    let stream = FreeStreamSource::new(physical, &volume.free_extents);
    if stream.is_empty() {
        return Ok((stream, None));
    }
    let mut opts = base_opts.clone();
    opts.mode = ScanMode::Raw;
    opts.free_stream = false;
    let unreadable: RegionLog = Arc::new(Mutex::new(Vec::new()));
    let input = scan::ScanInput {
        source: &stream,
        image: "free-stream",
        image_identity: None,
        image_len: stream.len(),
        volumes: &[],
        extra_extents: &[],
        out_dir: temp_dir,
        unreadable_regions: &unreadable,
        skip_unreadable: false,
        interrupt: Some(interrupt),
    };
    let report = scan::run_scan(&input, &opts, ctx)?;
    Ok((stream, Some(report)))
}

/// H-27: translate the free-stream report into physical image coordinates and
/// merge it into the physical report. Results already produced by the normal
/// pass are removed by SHA-256, not by name or offset.
fn merge_free_stream_report(
    report: &mut ScanReport,
    stream_report: ScanReport,
    stream: &FreeStreamSource<'_>,
    volume: &FsInfo,
    mode: ScanMode,
    temp_dir: &Path,
    out_dir: &Path,
) -> Result<()> {
    use std::collections::HashSet;

    let mut known = report
        .files
        .iter()
        .map(|file| file.sha256.clone())
        .collect::<HashSet<_>>();
    let mut seq = report.files.len() as u64 + 1;
    let mut added = 0usize;
    let mut duplicates = 0usize;
    let mut copied_bytes = 0u64;

    for mut file in stream_report.files {
        if !known.insert(file.sha256.clone()) {
            duplicates += 1;
            continue;
        }
        // The temporary output was named from the logical free-stream offset;
        // preserve that name for the source path while reporting physical
        // coordinates to the examiner.
        let stream_name = file.path.clone();
        let physical_offset = stream.map_offset(file.offset).unwrap_or(file.offset);
        let physical_fragments: Vec<Extent> = if file.source == "bgc" && !file.fragments.is_empty()
        {
            file.fragments
                .iter()
                .flat_map(|fragment| {
                    stream
                        .physical_ranges(fragment.offset, fragment.len)
                        .into_iter()
                })
                .collect()
        } else {
            stream.physical_ranges(file.offset, file.carved_size.max(1))
        };
        file.fragments = physical_fragments
            .into_iter()
            .map(|extent| Fragment {
                offset: extent.offset,
                len: extent.len,
            })
            .collect();
        file.offset = physical_offset;
        file.source = "free-stream".to_string();
        let (lba, cluster, partition_index) =
            free_stream_allocation_metadata(physical_offset, volume);
        file.lba = lba;
        file.cluster = cluster;
        file.allocated = match mode {
            ScanMode::Allocated => Some(true),
            _ => Some(false),
        };
        file.partition_index = partition_index;

        let filename = format!(
            "{:05}_{}_{:016x}.{}",
            seq,
            file.file_type.ext(),
            physical_offset,
            file.file_type.ext()
        );
        let src = temp_dir.join(&stream_name);
        let dst = out_dir.join(&filename);
        std::fs::copy(&src, &dst).with_context(|| {
            format!("cannot copy free-stream result '{stream_name}' to '{filename}'")
        })?;
        copied_bytes = copied_bytes.saturating_add(file.size);
        file.path = filename;
        file.seq = seq;
        report.files.push(file);
        added += 1;
        seq += 1;
    }

    report.bytes_scanned = report
        .bytes_scanned
        .saturating_add(stream_report.bytes_scanned);
    report.duration_ms = report.duration_ms.saturating_add(stream_report.duration_ms);
    report.stats.candidates = report
        .stats
        .candidates
        .saturating_add(stream_report.stats.candidates);
    report.stats.carved = report.stats.carved.saturating_add(added);
    report.stats.duplicate_hashes = report
        .stats
        .duplicate_hashes
        .saturating_add(duplicates + usize::from(stream_report.stats.duplicate_hashes > 0));
    report.stats.bytes_written = report.stats.bytes_written.saturating_add(copied_bytes);
    report.stats.truncated = report
        .stats
        .truncated
        .saturating_add(stream_report.stats.truncated);
    report.stats.repaired = report
        .stats
        .repaired
        .saturating_add(stream_report.stats.repaired);
    report.warnings.extend(stream_report.warnings);
    report.internal_errors.extend(stream_report.internal_errors);
    report.nondeterministic_stop |= stream_report.nondeterministic_stop;
    report.interrupted |= stream_report.interrupted;
    report.warnings.push(Warning::new(
        "FREE_STREAM",
        format!(
            "free-space stream pass added {added} file(s) and suppressed {duplicates} duplicate(s)"
        ),
    ));
    Ok(())
}

fn free_stream_allocation_metadata(
    offset: u64,
    volume: &FsInfo,
) -> (u64, Option<u64>, Option<usize>) {
    let lba = offset / u64::from(volume.bytes_per_sector.max(1));
    let cluster = offset.checked_sub(volume.data_start).and_then(|rel| {
        if volume.cluster_size == 0 || rel % volume.cluster_size != 0 {
            return None;
        }
        let index = rel / volume.cluster_size;
        Some(match volume.kind {
            fs::FsKind::Ntfs => index,
            _ => index.saturating_add(2),
        })
    });
    (lba, cluster, volume.partition_index)
}

/// H-28/H-29: attach additive metadata fields to matching rows and append new
/// metadata-only rows. Existing rows and their output order are never changed.
fn merge_metadata_report(
    report: &mut ScanReport,
    results: Vec<MetadataResult>,
    stats: MetadataPassStats,
    warnings: Vec<Warning>,
) {
    let mut added_bytes = 0u64;
    for result in results {
        match result
            .attach_to
            .and_then(|index| report.files.get_mut(index))
        {
            Some(row) => {
                row.original_name = result.file.original_name;
                row.mtime = result.file.mtime;
                row.contiguous_assumed = result.file.contiguous_assumed;
                row.overwritten_clusters = result.file.overwritten_clusters;
                row.metadata_notes = result.file.metadata_notes;
                row.sparse_bytes = result.file.sparse_bytes;
                row.notes.extend(result.file.notes);
                row.validation_checks.extend(result.file.validation_checks);
            }
            None => {
                added_bytes = added_bytes.saturating_add(result.file.carved_size);
                report.files.push(result.file);
            }
        }
    }
    report.stats.candidates = report.stats.candidates.saturating_add(stats.candidates);
    report.stats.carved = report.stats.carved.saturating_add(stats.added);
    report.stats.bytes_written = report.stats.bytes_written.saturating_add(added_bytes);
    report.warnings.extend(warnings);
    report.warnings.push(Warning::new(
        "METADATA",
        format!(
            "deleted-metadata pass examined {} candidate(s), added {} file(s), \
             annotated {} existing row(s) and suppressed {} duplicate(s)",
            stats.candidates, stats.added, stats.attached, stats.duplicates
        ),
    ));
}

struct FreeStreamRunSpec<'a> {
    physical: &'a dyn crate::io::ImageSource,
    volumes: &'a [FsInfo],
    opts: &'a ScanOptions,
    ctx: &'a ScanContext,
    mode: ScanMode,
    out_dir: &'a Path,
}

fn apply_free_stream_pass(
    report: &mut ScanReport,
    spec: FreeStreamRunSpec<'_>,
    interrupt: scan::Interrupt,
) -> Result<()> {
    if spec.volumes.len() != 1 {
        bail!("--free-stream requires exactly one selected volume; use --partition <N>");
    }
    let temp = tempfile::tempdir().context("cannot create free-stream work directory")?;
    let volume = spec
        .volumes
        .first()
        .context("--free-stream has no selected volume")?;
    let (stream, stream_report) = run_free_stream_pass(
        spec.physical,
        volume,
        spec.opts,
        spec.ctx,
        interrupt,
        temp.path(),
    )?;
    if let Some(stream_report) = stream_report {
        merge_free_stream_report(
            report,
            stream_report,
            &stream,
            volume,
            spec.mode,
            temp.path(),
            spec.out_dir,
        )?;
    }
    Ok(())
}

fn apply_metadata_pass(
    report: &mut ScanReport,
    source: &dyn crate::io::ImageSource,
    volumes: &[FsInfo],
    opts: &ScanOptions,
    out_dir: &Path,
) -> Result<()> {
    if volumes.is_empty() {
        bail!("--metadata requires at least one selected volume");
    }
    let start_seq = report.files.len() as u64 + 1;
    let mut known = report
        .files
        .iter()
        .map(|file| file.sha256.clone())
        .collect::<std::collections::HashSet<_>>();
    let initial_output_bytes = report.stats.bytes_written;
    let (results, stats, metadata_warnings) = scan::run_metadata_pass(scan::MetadataPassSpec {
        source,
        volumes,
        existing: &report.files,
        opts,
        out_dir,
        start_seq,
        known: &mut known,
        initial_output_bytes,
    })?;
    merge_metadata_report(report, results, stats, metadata_warnings);
    if opts.manifest {
        scan::write_manifest(out_dir, &report.files)?;
    }
    Ok(())
}

fn scan(args: ScanArgs) -> Result<i32> {
    validate_scan_args(&args)?;
    let interrupt = scan::install_interrupt_handler()?;
    let options = OpenOptions {
        allow_external_extents: args.allow_external_extents,
        on_read_error: args.on_read_error,
        max_stdin_bytes: args.max_stdin_bytes,
        spool_dir: args.spool_dir.clone(),
    };
    let image: OpenedImage = open_image_with_options(&args.image, args.input_format, &options)?;
    let len = image.len;
    let source = image.source.as_ref();
    let selection = select_scan_regions(&args, source, len)?;

    let image_mtime = image
        .identity
        .as_ref()
        .and_then(super::image::SourceIdentity::initial_mtime);
    let image_sha256 = if args.hash_image {
        if !args.quiet {
            eprintln!("Hashing source image (SHA-256)…");
        }
        Some(sha256_source(source)?)
    } else {
        None
    };

    let input_format = image.format.as_str().to_string();
    let input_physical_size = image
        .identity
        .as_ref()
        .map_or(len, crate::io::SourceIdentity::initial_size);
    let (opts, ctx) = build_scan_options(
        &args,
        args.include_gaps || !args.no_gaps,
        image_sha256,
        image_mtime,
        input_format,
        input_physical_size,
    );
    print_scan_banner(&args, len);

    let input = scan::ScanInput {
        source,
        image: &image.display,
        image_identity: image.identity.as_ref(),
        image_len: len,
        volumes: &selection.volumes,
        extra_extents: &selection.extra,
        out_dir: &args.output,
        unreadable_regions: &image.unreadable_regions,
        skip_unreadable: args.on_read_error == OnReadError::Skip,
        interrupt: Some(interrupt.clone()),
    };
    let run_result = scan::run_scan(&input, &opts, &ctx);
    let unreadable_regions: Vec<UnreadableRegion> = match image.unreadable_regions.lock() {
        Ok(regions) => regions.clone(),
        Err(_) => Vec::new(),
    };
    let mut warnings = selection.warnings;
    if let Some(warning) = unreadable_warning(&unreadable_regions) {
        warnings.push(warning);
    }

    let report = match run_result {
        Ok(report) => report,
        Err(err) => {
            write_abort_partial_report(AbortReportSpec {
                input: &input,
                opts: &opts,
                ctx: &ctx,
                args: &args,
                warnings,
                volume_rows: selection.volume_rows,
                regions: selection.regions,
                unreadable_regions: &unreadable_regions,
            })?;
            return Err(err);
        }
    };
    let mut report = report;
    if args.free_stream {
        apply_free_stream_pass(
            &mut report,
            FreeStreamRunSpec {
                physical: source,
                volumes: &selection.volumes,
                opts: &opts,
                ctx: &ctx,
                mode: args.mode,
                out_dir: &args.output,
            },
            interrupt,
        )?;
    }
    if args.metadata {
        apply_metadata_pass(&mut report, source, &selection.volumes, &opts, &args.output)?;
    }
    finish_scan_report(
        report,
        &args,
        selection.volume_rows,
        selection.regions,
        warnings,
    )
}
