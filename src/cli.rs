use crate::fs::{self, FsInfo, PartitionSel};
use crate::io::{
    open_image_with_options, sha256_source, InputFormat, OnReadError, OpenOptions, OpenedImage,
    UnreadableRegion,
};
use crate::report;
use crate::scan::{
    self, AlignMode, ScanContext, ScanMode, ScanOptions, VolumeSummary, Warning,
    DEFAULT_FRAG_TIME_LIMIT_MS, DEFAULT_MAX_FRAG_CANDIDATES, DEFAULT_MAX_GAP_CLUSTERS,
    SCHEMA_VERSION,
};
use crate::signatures::{FileType, PdfEofMode, ValidationMode};
use crate::util::{human_size, parse_size, Extent};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::path::PathBuf;

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
    /// Rebuild a truncated footer (PDF xref/trailer, JPEG EOI, PNG IEND) and revalidate before labelling repaired
    #[arg(long)]
    repair: bool,
    /// Only write files whose footer was found, not verified (see validity)
    #[arg(long)]
    complete_only: bool,
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
    /// Opt in to bounded bifragment gap carving (B-06)
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

fn scan(args: ScanArgs) -> Result<i32> {
    if !args.fragment_recovery
        && (args.max_gap != DEFAULT_MAX_GAP_CLUSTERS
            || args.max_frag_candidates != DEFAULT_MAX_FRAG_CANDIDATES as u64
            || args.frag_time_limit != DEFAULT_FRAG_TIME_LIMIT_MS)
    {
        bail!("B-06 limits require --fragment-recovery");
    }
    let options = OpenOptions {
        allow_external_extents: args.allow_external_extents,
        on_read_error: args.on_read_error,
        max_stdin_bytes: args.max_stdin_bytes,
        spool_dir: args.spool_dir.clone(),
    };
    let image: OpenedImage = open_image_with_options(&args.image, args.input_format, &options)?;
    let len = image.len;
    let source = image.source.as_ref();

    // Selected volumes actually scanned, plus raw ranges added by
    // `--include-gaps`; the full detection result feeds `volumes[]`,
    // `regions[]` and the "not scanned" warnings (A-07).
    let mut volumes: Vec<FsInfo> = Vec::new();
    let mut extra: Vec<Extent> = Vec::new();
    let mut volume_rows: Vec<VolumeSummary> = Vec::new();
    let mut regions: Vec<crate::fs::Region> = Vec::new();
    let mut warnings: Vec<Warning> = Vec::new();

    let needs_fs = matches!(args.mode, ScanMode::Unallocated | ScanMode::Allocated);
    // A-07 step 2: gaps are included by default; `--no-gaps` restores the
    // historical step-1 behaviour. `--include-gaps` remains accepted for
    // scripts written against the old CLI.
    let include_gaps = args.include_gaps || !args.no_gaps;

    if args.no_fs {
        if needs_fs {
            bail!("--no-fs is incompatible with --mode unallocated/allocated");
        }
        // No detection: the user opted out of filesystem and region accounting.
    } else if let Some(off) = args.offset {
        // Forced boot-sector offset: a single volume, no partition table
        // interpretation, therefore no region accounting (user opt-out).
        let f = fs::detect_guarded(source, len, Some(off), None)
            .context("filesystem detection failed")?;
        announce_fs(&args, &f);
        volume_rows.push(VolumeSummary::from_fs(1, &f, true));
        volumes.push(f);
    } else {
        let selection = args.partition.unwrap_or(PartitionSel::All);
        let explicit = args.partition.is_some();
        match fs::detect_all_guarded(source, len) {
            Err(e) => {
                if explicit || needs_fs {
                    return Err(e).context("filesystem detection failed");
                }
                warnings.push(Warning::new(
                    "FS_DETECTION_FAILED",
                    format!("filesystem detection failed; used raw scan: {e:#}"),
                ));
            }
            Ok(set) => {
                // H-26: a recognised but unparsable volume is a degraded run,
                // even when another volume parsed and the scan can continue.
                for error in &set.errors {
                    warnings.push(Warning::new(
                        "VOLUME_PARSE_FAILED",
                        format!(
                            "partition {} at byte {} could not be parsed: {}",
                            error.partition_index, error.offset, error.reason
                        ),
                    ));
                }
                match fs::select_volumes(&set, selection) {
                    Err(e) => {
                        if explicit || needs_fs {
                            return Err(e).context("filesystem detection failed");
                        }
                        warnings.push(Warning::new(
                            "FS_DETECTION_FAILED",
                            format!("filesystem detection failed; used raw scan: {e:#}"),
                        ));
                        // The fallback reads the whole image, so every region of
                        // the failed detection is still scanned.
                        volume_rows = set
                            .volumes
                            .iter()
                            .enumerate()
                            .map(|(i, v)| VolumeSummary::from_fs(i + 1, v, true))
                            .collect();
                        regions = set.regions(&[], true, false, false, include_gaps);
                    }
                    Ok(selected) => {
                        volumes = selected;
                        for f in &volumes {
                            announce_fs(&args, f);
                        }
                        if include_gaps {
                            extra = set.unscanned_extents();
                        }
                        let whole_image = args.mode == ScanMode::Raw;
                        let scans_free =
                            matches!(args.mode, ScanMode::Auto | ScanMode::Unallocated);
                        let scans_alloc = args.mode == ScanMode::Allocated;
                        volume_rows = set
                            .volumes
                            .iter()
                            .enumerate()
                            .map(|(i, v)| {
                                let chosen = volumes
                                    .iter()
                                    .any(|s| s.offset == v.offset && s.length == v.length);
                                VolumeSummary::from_fs(i + 1, v, whole_image || chosen)
                            })
                            .collect();
                        regions = set.regions(
                            &volumes,
                            whole_image,
                            scans_free,
                            scans_alloc,
                            include_gaps,
                        );

                        // A-07: say out loud what this run does not read.
                        if !whole_image {
                            for v in &set.volumes {
                                let chosen = volumes
                                    .iter()
                                    .any(|s| s.offset == v.offset && s.length == v.length);
                                if !chosen {
                                    warnings.push(Warning::new(
                                        "VOLUME_NOT_SCANNED",
                                        format!(
                                            "{} at byte {} ({}) was not scanned; \
                                         use --partition all to scan every volume",
                                            v.partition_index
                                                .map(|i| format!("partition {i}"))
                                                .unwrap_or_else(|| "volume".into()),
                                            v.offset,
                                            human_size(v.length)
                                        ),
                                    ));
                                }
                            }
                            if !include_gaps {
                                for p in &set.unsupported {
                                    let detail =
                                        set.errors.iter().find(|e| e.partition_index == p.index);
                                    warnings.push(Warning::new(
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
                                    warnings.push(Warning::new(
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
                        }
                    }
                }
            }
        }
    }

    if args.mode == ScanMode::Raw && !volumes.is_empty() {
        warnings.push(Warning::new(
            "RAW_SCANS_ALLOCATED",
            "raw mode also scans allocated clusters",
        ));
    }

    let image_mtime = image
        .identity
        .as_ref()
        .and_then(|identity| identity.initial_mtime());
    let image_sha256 = if args.hash_image {
        if !args.quiet {
            eprintln!("Hashing source image (SHA-256)…");
        }
        Some(sha256_source(source)?)
    } else {
        None
    };

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
        complete_only: args.complete_only,
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
        fragment_recovery: args.fragment_recovery,
        max_gap: args.max_gap,
        max_frag_candidates: args.max_frag_candidates as usize,
        frag_time_limit: std::time::Duration::from_millis(args.frag_time_limit),
    };
    let ctx = ScanContext {
        argv: std::env::args().collect(),
        case_id: args.case_id.clone(),
        examiner: args.examiner.clone(),
        image_sha256,
        image_mtime,
        input_format: image.format.as_str().to_string(),
        input_physical_size: Some(
            image
                .identity
                .as_ref()
                .map_or(len, crate::io::SourceIdentity::initial_size),
        ),
    };

    if !args.quiet {
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

    let input = scan::ScanInput {
        source,
        image: &image.display,
        image_identity: image.identity.as_ref(),
        image_len: len,
        volumes: &volumes,
        extra_extents: &extra,
        out_dir: &args.output,
        unreadable_regions: &image.unreadable_regions,
        skip_unreadable: args.on_read_error == OnReadError::Skip,
    };
    let run_result = scan::run_scan(&input, &opts, &ctx);
    let unreadable_regions: Vec<UnreadableRegion> = match image.unreadable_regions.lock() {
        Ok(regions) => regions.clone(),
        Err(_) => Vec::new(),
    };
    if !unreadable_regions.is_empty() {
        warnings.push(Warning::new(
            "UNREADABLE_REGION",
            format!(
                "{} source byte range(s) could not be read; affected outputs are not verified",
                unreadable_regions.len()
            ),
        ));
    }

    let mut report = match run_result {
        Ok(report) => report,
        Err(err) => {
            // `abort` intentionally returns exit 1, but the run must still
            // leave a schema-valid partial report when unreadable bytes were
            // the reason it stopped.
            if args.on_read_error == OnReadError::Abort && !unreadable_regions.is_empty() {
                let first = &unreadable_regions[0];
                let mut partial_warnings = warnings;
                partial_warnings.push(Warning::new(
                    "READ_ABORTED",
                    format!(
                        "scan aborted at byte {} after an unreadable source range: {}",
                        first.offset, first.reason
                    ),
                ));
                let mut partial = scan::partial_report(&input, &opts, &ctx, partial_warnings);
                partial.volumes = volume_rows;
                partial.regions = regions;
                write_reports(&partial, &args)?;
            }
            return Err(err);
        }
    };
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
    write_reports(&report, &args)?;
    // Exit 3 = finished but incomplete: evidence was left unscanned (A-07).
    Ok(if report.warnings.iter().any(|w| w.incomplete()) {
        3
    } else {
        0
    })
}
