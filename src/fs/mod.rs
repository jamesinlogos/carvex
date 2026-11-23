//! Filesystem detection and allocation metadata.
//!
//! The parsers are intentionally read-only.  They consume an [`ImageSource`]
//! and return the free extents reported by the FAT or NTFS `$Bitmap`.

#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
/// FAT12/16/32 volume parsing and free-space extent enumeration.
pub mod fat32;
/// NTFS volume parsing and free-space cluster run enumeration.
pub mod ntfs;

use crate::io::{read_vec_at, ImageSource};
use crate::util::{complement_extents, human_size, merge_extents, Extent};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

/// The `FsKind` enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FsKind {
    /// The `Fat32` variant of `FsKind`.
    Fat32,
    /// The `Ntfs` variant of `FsKind`.
    Ntfs,
}

impl std::fmt::Display for FsKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsKind::Fat32 => write!(f, "FAT32"),
            FsKind::Ntfs => write!(f, "NTFS"),
        }
    }
}

/// Parsed filesystem metadata and free-space extents for one volume.
#[derive(Debug, Clone, Serialize)]
pub struct FsInfo {
    /// Filesystem kind (FAT32 or NTFS).
    pub kind: FsKind,
    /// Absolute byte offset of the volume boot sector inside the image.
    pub offset: u64,
    /// Volume length in bytes (clamped to the image).
    pub length: u64,
    /// Bytes per logical sector.
    pub bytes_per_sector: u32,
    /// Sectors per allocation cluster.
    pub sectors_per_cluster: u32,
    /// Cluster size in bytes.
    pub cluster_size: u64,
    /// Absolute offset of the first addressable cluster: cluster 2 for FAT32,
    /// cluster 0 for NTFS.  Used to report the on-disk cluster number.
    pub data_start: u64,
    /// Total addressable clusters in the volume.
    pub total_clusters: u64,
    /// Free clusters available for carving.
    pub free_clusters: u64,
    /// Free-space byte extents, sorted by offset.
    pub free_extents: Vec<Extent>,
    /// Source partition index, when known.
    pub partition_index: Option<usize>,
    /// Partition type name, when known.
    pub partition_type: Option<String>,
    /// Volume label, when present.
    pub label: Option<String>,
    /// Format-specific diagnostic details.
    pub details: BTreeMap<String, String>,
}

impl FsInfo {
    /// Returns total free bytes (`free_clusters * cluster_size`).
    pub fn free_bytes(&self) -> u64 {
        self.free_clusters.saturating_mul(self.cluster_size)
    }

    /// Returns the largest free extent length in bytes.
    pub fn largest_free_extent(&self) -> u64 {
        self.free_extents.iter().map(|e| e.len).max().unwrap_or(0)
    }
}

/// A partition table entry with byte/sector geometry.
#[derive(Debug, Clone)]
pub struct Partition {
    /// Zero-based partition index in the table.
    pub index: usize,
    /// Starting logical block address (512-byte sectors).
    pub start_lba: u64,
    /// Partition length in 512-byte sectors.
    pub sectors: u64,
    /// Human-readable partition type name.
    pub type_name: String,
}

impl Partition {
    /// Returns partition start offset in bytes.
    pub fn byte_offset(&self, sector_size: u64) -> u64 {
        self.start_lba.saturating_mul(sector_size)
    }

    /// Method.
    pub fn byte_len(&self, sector_size: u64) -> u64 {
        self.sectors.saturating_mul(sector_size)
    }
}

/// Which detected volumes a run should scan (A-07).
///
/// The default is [`PartitionSel::All`] (A-07 step 2), which scans every
/// supported volume, each into its own output sub-directory;
/// `--partition largest` restores the historical single-volume behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionSel {
    /// The `Largest` variant of `PartitionSel`.
    Largest,
    /// The `All` variant of `PartitionSel`.
    All,
    /// The `Index` variant of `PartitionSel`.
    Index(usize),
}

impl std::fmt::Display for PartitionSel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartitionSel::Largest => write!(f, "largest"),
            PartitionSel::All => write!(f, "all"),
            PartitionSel::Index(n) => write!(f, "{n}"),
        }
    }
}

impl std::str::FromStr for PartitionSel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "largest" => Ok(PartitionSel::Largest),
            "all" => Ok(PartitionSel::All),
            other => match other.parse::<usize>() {
                Ok(n) if n >= 1 => Ok(PartitionSel::Index(n)),
                _ => Err(format!(
                    "invalid --partition value '{s}': use 'largest', 'all' or a 1-based partition number"
                )),
            },
        }
    }
}

/// Kind of top-level image region reported in `regions[]` (A-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RegionKind {
    /// Unallocated space inside a supported volume.
    Free,
    /// Allocated space inside a supported volume.
    Allocated,
    /// Image bytes outside every partition.
    Gap,
    /// Partition bytes with no supported filesystem.
    Unsupported,
    /// Adjacent extents of different kinds merged by `--max-regions` (H-27).
    /// The `summarized` flag on the region identifies these rows.
    Mixed,
}

impl std::fmt::Display for RegionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegionKind::Free => write!(f, "free"),
            RegionKind::Allocated => write!(f, "allocated"),
            RegionKind::Gap => write!(f, "gap"),
            RegionKind::Unsupported => write!(f, "unsupported"),
            RegionKind::Mixed => write!(f, "mixed"),
        }
    }
}

/// One top-level region of the image: what it is, where it is, and whether
/// this run read it.  `offset`/`len` delimit the containing range; for
/// `free`/`allocated` entries the bytes actually read are the matching
/// extents inside that range.
#[derive(Debug, Clone, Serialize)]
pub struct Region {
    /// Region kind (volume, gap, unreadable, etc.).
    pub kind: RegionKind,
    /// Region start offset in bytes.
    pub offset: u64,
    /// Region length in bytes.
    pub len: u64,
    /// Whether this region was scanned.
    pub scanned: bool,
    /// Source partition index, when applicable.
    pub partition_index: Option<usize>,
    /// True when raw bytes here mix allocated and unallocated content because
    /// a filesystem parser failed (H-26).
    pub may_contain_allocated: bool,
    /// Index of the scanner extent that reads this region (H-27). `None` means
    /// the region was not read by this run, or that a summarised row spans
    /// several scanner extents.
    pub extent_id: Option<usize>,
    /// True when `--max-regions` merged multiple detailed extents into this
    /// row. Coverage and scanned-byte sums remain exact (H-27).
    pub summarized: bool,
}

impl Region {
    /// Exclusive end offset, saturating on overflow.
    pub fn end(&self) -> u64 {
        self.offset.saturating_add(self.len)
    }
}

/// A filesystem probe that recognised a boot sector but could not parse it.
#[derive(Debug, Clone)]
pub struct VolumeParseError {
    /// Partition index where parsing failed.
    pub partition_index: usize,
    /// Byte offset of the volume being parsed.
    pub offset: u64,
    /// Human-readable parse failure reason.
    pub reason: String,
}

impl std::fmt::Display for VolumeParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "partition {} (byte {}): {}",
            self.partition_index, self.offset, self.reason
        )
    }
}

/// Everything a detection pass found: every supported volume, every
/// partition-table entry, the image regions between them, and probe errors.
#[derive(Debug, Clone)]
pub struct VolumeSet {
    /// Supported FAT32/NTFS volumes in partition order (or the single
    /// superfloppy volume found at offset 0).
    pub volumes: Vec<FsInfo>,
    /// Every partition-table entry (MBR/GPT).
    pub partitions: Vec<Partition>,
    /// Table entries without a supported filesystem.
    pub unsupported: Vec<Partition>,
    /// Probe failures for `unsupported` entries, with their partition index,
    /// byte offset and underlying parser reason (one per partition; H-26).
    pub errors: Vec<VolumeParseError>,
    /// Image byte ranges outside every partition (partitioned images only).
    pub gaps: Vec<Extent>,
    /// True when a volume was found directly at offset 0.
    pub superfloppy: bool,
    /// Logical image length. Used to make `regions[]` cover the whole image.
    pub image_len: u64,
}

impl VolumeSet {
    /// Gap and unsupported-partition extents, i.e. everything outside the
    /// supported volumes (used by `--include-gaps`).
    pub fn unscanned_extents(&self) -> Vec<Extent> {
        let mut out = self.gaps.clone();
        for p in &self.unsupported {
            let offset = p.byte_offset(512);
            let len = p.byte_len(512);
            if len > 0 {
                out.push(Extent { offset, len });
            }
        }
        out
    }

    /// Build the `regions[]` list for a scan: every detected region with a
    /// `scanned` flag describing whether this run reads it.
    ///
    /// * `whole_image_scanned` — the run reads every byte (`--mode raw`).
    /// * `scans_free` / `scans_allocated` — the run reads the free or the
    ///   allocated extents of the *selected* volumes.
    /// * `include_gaps` — `--include-gaps` adds gaps and unsupported
    ///   partitions to the scan space.
    pub fn regions(
        &self,
        selected: &[FsInfo],
        whole_image_scanned: bool,
        scans_free: bool,
        scans_allocated: bool,
        include_gaps: bool,
    ) -> Vec<Region> {
        self.regions_with_plan(
            selected,
            whole_image_scanned,
            scans_free,
            scans_allocated,
            include_gaps,
            &[],
            0,
        )
    }

    /// Build the H-27 coverage map from the same extents the scanner reads.
    ///
    /// `plan` is the output of `scan::plan_extents`; when it is empty (for
    /// `info`) `extent_id` is left unset. `max_regions == 0` keeps every
    /// detailed extent. Adjacent rows with the same `scanned` state may be
    /// summarised, but coverage and the sum of scanned lengths are exact.
    // H-34: region planning keeps its original signature so coverage and summarisation inputs stay explicit.
    #[allow(clippy::too_many_arguments)]
    pub fn regions_with_plan(
        &self,
        selected: &[FsInfo],
        whole_image_scanned: bool,
        scans_free: bool,
        scans_allocated: bool,
        include_gaps: bool,
        plan: &[Extent],
        max_regions: u64,
    ) -> Vec<Region> {
        let mut tagged: Vec<TaggedRegion> = Vec::new();
        let parse_failed = |index: usize| self.errors.iter().any(|e| e.partition_index == index);
        let volume_end = |v: &FsInfo| {
            v.offset
                .saturating_add(v.length)
                .min(self.image_len)
                .max(v.offset.min(self.image_len))
        };

        for v in &self.volumes {
            let chosen = selected
                .iter()
                .any(|s| s.offset == v.offset && s.length == v.length);
            let end = volume_end(v);
            if end <= v.offset {
                continue;
            }
            let free = merge_extents(
                v.free_extents
                    .iter()
                    .filter_map(|e| clamp_extent(*e, v.offset, end))
                    .collect(),
            );
            for extent in &free {
                tagged.push(TaggedRegion {
                    extent: *extent,
                    kind: RegionKind::Free,
                    scanned: whole_image_scanned || (chosen && scans_free),
                    partition_index: v.partition_index,
                    may_contain_allocated: false,
                });
            }
            for extent in complement_extents(v.offset, end - v.offset, &free) {
                tagged.push(TaggedRegion {
                    extent,
                    kind: RegionKind::Allocated,
                    scanned: whole_image_scanned || (chosen && scans_allocated),
                    partition_index: v.partition_index,
                    may_contain_allocated: false,
                });
            }
        }

        for p in &self.unsupported {
            let offset = p.byte_offset(512);
            let end = offset.saturating_add(p.byte_len(512)).min(self.image_len);
            if end > offset {
                tagged.push(TaggedRegion {
                    extent: Extent {
                        offset,
                        len: end - offset,
                    },
                    kind: RegionKind::Unsupported,
                    scanned: whole_image_scanned || include_gaps,
                    partition_index: Some(p.index),
                    may_contain_allocated: parse_failed(p.index),
                });
            }
        }
        for g in &self.gaps {
            if let Some(extent) = clamp_extent(*g, 0, self.image_len) {
                tagged.push(TaggedRegion {
                    extent,
                    kind: RegionKind::Gap,
                    scanned: whole_image_scanned || include_gaps,
                    partition_index: None,
                    may_contain_allocated: false,
                });
            }
        }

        let mut regions = flatten_regions(tagged, self.image_len, whole_image_scanned);
        if !plan.is_empty() {
            for region in &mut regions {
                if region.scanned {
                    region.extent_id = plan
                        .iter()
                        .position(|e| e.offset <= region.offset && region.offset < e.end());
                }
            }
        }
        summarise_regions(regions, max_regions)
    }
}

#[derive(Debug, Clone)]
struct TaggedRegion {
    extent: Extent,
    kind: RegionKind,
    scanned: bool,
    partition_index: Option<usize>,
    may_contain_allocated: bool,
}

fn clamp_extent(extent: Extent, start: u64, end: u64) -> Option<Extent> {
    let from = extent.offset.max(start);
    let to = extent.end().min(end);
    (to > from).then_some(Extent {
        offset: from,
        len: to - from,
    })
}

/// Remove `covered` byte ranges from `base`. Used only to make pathological
/// overlapping partition metadata produce a non-overlapping coverage map.
fn subtract_extents(base: Extent, covered: &[Extent]) -> Vec<Extent> {
    let mut parts = vec![base];
    for cut in covered {
        let cut_start = cut.offset;
        let cut_end = cut.end();
        let mut next = Vec::new();
        for part in parts {
            if cut_start >= part.end() || cut_end <= part.offset {
                next.push(part);
                continue;
            }
            if cut_start > part.offset {
                next.push(Extent {
                    offset: part.offset,
                    len: cut_start - part.offset,
                });
            }
            if cut_end < part.end() {
                next.push(Extent {
                    offset: cut_end,
                    len: part.end() - cut_end,
                });
            }
        }
        parts = next;
    }
    parts
}

fn flatten_regions(
    mut tagged: Vec<TaggedRegion>,
    image_len: u64,
    whole_image_scanned: bool,
) -> Vec<Region> {
    // Lower priority wins when malformed partition metadata overlaps.
    tagged.sort_by_key(|r| (r.extent.offset, r.kind != RegionKind::Free));
    let mut covered: Vec<Extent> = Vec::new();
    let mut out = Vec::new();
    for item in tagged {
        for extent in subtract_extents(item.extent, &covered) {
            covered.push(extent);
            out.push(Region {
                kind: item.kind,
                offset: extent.offset,
                len: extent.len,
                scanned: item.scanned,
                partition_index: item.partition_index,
                may_contain_allocated: item.may_contain_allocated,
                extent_id: None,
                summarized: false,
            });
        }
    }

    // Any hole in the metadata is still image bytes. Mark it unscanned unless
    // this run really read the whole image.
    let mut cursor = 0u64;
    for extent in merge_extents(covered) {
        if extent.offset > cursor {
            out.push(Region {
                kind: RegionKind::Gap,
                offset: cursor,
                len: extent.offset - cursor,
                scanned: whole_image_scanned,
                partition_index: None,
                may_contain_allocated: false,
                extent_id: None,
                summarized: false,
            });
        }
        cursor = cursor.max(extent.end());
    }
    if cursor < image_len {
        out.push(Region {
            kind: RegionKind::Gap,
            offset: cursor,
            len: image_len - cursor,
            scanned: whole_image_scanned,
            partition_index: None,
            may_contain_allocated: false,
            extent_id: None,
            summarized: false,
        });
    }
    out.sort_by_key(|r| r.offset);
    out
}

fn summarise_regions(regions: Vec<Region>, max_regions: u64) -> Vec<Region> {
    if max_regions == 0 || regions.len() <= max_regions as usize {
        return regions;
    }
    let mut out = Vec::new();
    let mut index = 0usize;
    while let Some(first) = regions.get(index) {
        let scanned = first.scanned;
        let mut end = index.saturating_add(1);
        while regions.get(end).is_some_and(|r| r.scanned == scanned) {
            end = end.saturating_add(1);
        }
        let Some(group) = regions.get(index..end) else {
            break;
        };
        if group.len() == 1 {
            out.push(first.clone());
        } else if let (Some(first), Some(last)) = (group.first(), group.last()) {
            let same_kind = group.iter().all(|r| r.kind == first.kind);
            let same_partition = group
                .iter()
                .all(|r| r.partition_index == first.partition_index);
            let same_extent = group.iter().all(|r| r.extent_id == first.extent_id);
            out.push(Region {
                kind: if same_kind {
                    first.kind
                } else {
                    RegionKind::Mixed
                },
                offset: first.offset,
                len: last.end().saturating_sub(first.offset),
                scanned: first.scanned,
                partition_index: if same_partition {
                    first.partition_index
                } else {
                    None
                },
                may_contain_allocated: group.iter().any(|r| r.may_contain_allocated),
                extent_id: if same_extent { first.extent_id } else { None },
                summarized: true,
            });
        }
        index = end;
    }
    out
}

/// Everything a filesystem parser needs to read one volume.
pub struct VolumeCtx<'a> {
    /// Image source to read from.
    pub source: &'a dyn ImageSource,
    /// Volume start offset within the source.
    pub offset: u64,
    /// Maximum readable length from `offset`.
    pub max_len: u64,
    /// Partition index, when known.
    pub partition_index: Option<usize>,
    /// Partition type name, when known.
    pub partition_type: Option<String>,
}

impl VolumeCtx<'_> {
    /// Method.
    pub fn read_vec(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let end = offset
            .checked_add(len as u64)
            .context("volume read overflow")?;
        if end > self.offset.saturating_add(self.max_len) {
            bail!("volume read at {offset} (+{len}) lies outside the volume");
        }
        read_vec_at(self.source, offset, len)
    }
}

/// Detect a supported filesystem.
///
/// `--offset` overrides partition parsing; `--partition` selects a 1-based MBR
/// slot / GPT partition number. The CLI default scans all supported volumes
/// (A-07 step 2); `PartitionSel::Largest` remains available to restore the
/// historical single-volume view.
pub fn detect(
    source: &dyn ImageSource,
    len: u64,
    forced_offset: Option<u64>,
    wanted_partition: Option<usize>,
) -> Result<FsInfo> {
    if let Some(off) = forced_offset {
        if off >= len {
            bail!("volume offset {off} is outside the image ({len} bytes)");
        }
        let fs = probe(source, off, len, None, None)?
            .with_context(|| format!("no supported FAT32/NTFS volume at offset {off}"))?;
        return Ok(fs);
    }

    // Superfloppy / directly formatted volume image.
    if let Ok(Some(mut fs)) = probe(source, 0, len, None, None) {
        fs.partition_index = None;
        return Ok(fs);
    }

    let parts = read_partition_table(source, len).unwrap_or_default();
    if parts.is_empty() {
        bail!("no FAT32/NTFS filesystem found at offset 0 and no usable MBR/GPT partition table");
    }

    if let Some(want) = wanted_partition {
        let p = parts.iter().find(|p| p.index == want).with_context(|| {
            format!(
                "partition {want} not found (available: {})",
                parts
                    .iter()
                    .map(|p| p.index.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        let offset = p.byte_offset(512);
        let fs = probe(
            source,
            offset,
            len,
            Some(p.index),
            Some(p.type_name.clone()),
        )?
        .with_context(|| {
            format!(
                "partition {} (type {}, offset {}) is not a supported FAT32/NTFS volume",
                p.index, p.type_name, offset
            )
        })?;
        return Ok(fs);
    }

    let mut candidates: Vec<FsInfo> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for p in &parts {
        match probe(
            source,
            p.byte_offset(512),
            len,
            Some(p.index),
            Some(p.type_name.clone()),
        ) {
            Ok(Some(fs)) => candidates.push(fs),
            Ok(None) => {}
            Err(e) => errors.push(format!("partition {}: {e:#}", p.index)),
        }
    }
    candidates.sort_by_key(|c| std::cmp::Reverse(c.length));
    if let Some(fs) = candidates.into_iter().next() {
        return Ok(fs);
    }
    let mut msg = String::from("no FAT32/NTFS filesystem found in the partition table");
    if !errors.is_empty() {
        msg.push_str(":\n  - ");
        msg.push_str(&errors.join("\n  - "));
    }
    bail!("{msg}");
}

/// Detect while containing parser panics.
///
/// A panic in a malformed-filesystem parser must not abort a long carving run.
/// The scanner catches this error in `auto` mode and falls back to a raw scan
/// with a warning; `unallocated`/`allocated` modes return the error.
pub fn detect_guarded(
    source: &dyn ImageSource,
    len: u64,
    forced_offset: Option<u64>,
    wanted_partition: Option<usize>,
) -> Result<FsInfo> {
    match catch_unwind(AssertUnwindSafe(|| {
        detect(source, len, forced_offset, wanted_partition)
    })) {
        Ok(result) => result,
        Err(_) => bail!(
            "filesystem parser panicked on malformed metadata; \
             use --mode raw or --no-fs to carve without allocation metadata"
        ),
    }
}

/// Detect **every** supported volume plus the image regions that belong to no
/// supported volume (A-07: never skip data silently).
///
/// Unlike [`detect`], which collapses the partition table to a single volume,
/// this returns the full picture so callers can warn about everything they do
/// not scan and report it in `regions[]`.
pub fn detect_all(source: &dyn ImageSource, len: u64) -> Result<VolumeSet> {
    // Superfloppy / directly formatted volume image: the whole image is one
    // volume and there is no partition structure to report (same precedence
    // as `detect`).
    if let Ok(Some(mut fs)) = probe(source, 0, len, None, None) {
        fs.partition_index = None;
        return Ok(VolumeSet {
            volumes: vec![fs],
            partitions: Vec::new(),
            unsupported: Vec::new(),
            errors: Vec::new(),
            gaps: Vec::new(),
            superfloppy: true,
            image_len: len,
        });
    }

    let parts = read_partition_table(source, len).unwrap_or_default();
    if parts.is_empty() {
        bail!("no FAT32/NTFS filesystem found at offset 0 and no usable MBR/GPT partition table");
    }

    let mut volumes = Vec::new();
    let mut unsupported = Vec::new();
    let mut errors = Vec::new();
    for p in &parts {
        match probe(
            source,
            p.byte_offset(512),
            len,
            Some(p.index),
            Some(p.type_name.clone()),
        ) {
            Ok(Some(fs)) => volumes.push(fs),
            Ok(None) => unsupported.push(p.clone()),
            Err(e) => {
                unsupported.push(p.clone());
                errors.push(VolumeParseError {
                    partition_index: p.index,
                    offset: p.byte_offset(512),
                    reason: format!("{e:#}"),
                });
            }
        }
    }
    let gaps = gap_regions(&parts, len);
    Ok(VolumeSet {
        volumes,
        partitions: parts,
        unsupported,
        errors,
        gaps,
        superfloppy: false,
        image_len: len,
    })
}

/// [`detect_all`] with panics from malformed metadata converted to errors.
pub fn detect_all_guarded(source: &dyn ImageSource, len: u64) -> Result<VolumeSet> {
    match catch_unwind(AssertUnwindSafe(|| detect_all(source, len))) {
        Ok(result) => result,
        Err(_) => bail!(
            "filesystem parser panicked on malformed metadata; \
             use --mode raw or --no-fs to carve without allocation metadata"
        ),
    }
}

/// Pick the volumes a run should scan, preserving the historical selection
/// semantics of [`detect`] (largest volume first, ties keep partition order;
/// a superfloppy volume ignores the partition selection).
pub fn select_volumes(set: &VolumeSet, sel: PartitionSel) -> Result<Vec<FsInfo>> {
    if set.superfloppy {
        return Ok(set.volumes.clone());
    }
    match sel {
        PartitionSel::All => {
            if set.volumes.is_empty() {
                bail!(no_volume_message(set));
            }
            Ok(set.volumes.clone())
        }
        PartitionSel::Largest => {
            if set.volumes.is_empty() {
                bail!(no_volume_message(set));
            }
            let mut sorted = set.volumes.clone();
            sorted.sort_by_key(|f| std::cmp::Reverse(f.length));
            match sorted.into_iter().next() {
                Some(fs) => Ok(vec![fs]),
                None => bail!(no_volume_message(set)),
            }
        }
        PartitionSel::Index(n) => {
            if let Some(fs) = set.volumes.iter().find(|f| f.partition_index == Some(n)) {
                return Ok(vec![fs.clone()]);
            }
            if let Some(p) = set.unsupported.iter().find(|p| p.index == n) {
                bail!(
                    "partition {n} (type {}, offset {}) is not a supported FAT32/NTFS volume",
                    p.type_name,
                    p.byte_offset(512)
                );
            }
            let available = set
                .partitions
                .iter()
                .map(|p| p.index.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            if available.is_empty() {
                bail!(no_volume_message(set));
            }
            bail!("partition {n} not found (available: {available})");
        }
    }
}

fn no_volume_message(set: &VolumeSet) -> String {
    if set.errors.is_empty() {
        return String::from("no FAT32/NTFS filesystem found in the partition table");
    }
    let details = set
        .errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    format!("filesystem metadata could not be parsed: {details}")
}

/// Image byte ranges that lie outside every partition (A-07 `regions[]`).
fn gap_regions(parts: &[Partition], len: u64) -> Vec<Extent> {
    let mut spans: Vec<(u64, u64)> = parts
        .iter()
        .filter_map(|p| {
            let start = p.start_lba.saturating_mul(512);
            let end = start.saturating_add(p.byte_len(512)).min(len).max(start);
            (start < len && end > start).then_some((start, end))
        })
        .collect();
    spans.sort_unstable();
    let mut gaps = Vec::new();
    let mut cur = 0u64;
    for (start, end) in spans {
        if start > cur {
            gaps.push(Extent {
                offset: cur,
                len: start - cur,
            });
        }
        cur = cur.max(end);
    }
    if cur < len {
        gaps.push(Extent {
            offset: cur,
            len: len - cur,
        });
    }
    gaps.retain(|g| g.len > 0);
    gaps
}

/// Probe one absolute offset. Returns `Ok(None)` if the sector is not FAT32/NTFS.
pub fn probe(
    source: &dyn ImageSource,
    offset: u64,
    image_len: u64,
    partition_index: Option<usize>,
    partition_type: Option<String>,
) -> Result<Option<FsInfo>> {
    if offset.saturating_add(512) > image_len {
        return Ok(None);
    }
    let boot = read_vec_at(source, offset, 512)?;
    let max_len = image_len.saturating_sub(offset);
    let ctx = VolumeCtx {
        source,
        offset,
        max_len,
        partition_index,
        partition_type,
    };

    if boot.get(3..7) == Some(b"NTFS") && boot.get(510..512) == Some(&[0x55, 0xAA]) {
        let fs = ntfs::parse(&ctx, &boot)
            .with_context(|| format!("NTFS boot sector at byte {offset} but $MFT unreadable"))?;
        return Ok(Some(fs));
    }

    let is_fat32 = boot.get(510..512) == Some(&[0x55, 0xAA])
        && (boot.get(0x52..0x57) == Some(b"FAT32")
            || (boot.get(0x0b..0x0d) != Some(&[0, 0])
                && boot.get(0x10).copied().unwrap_or(0) != 0
                && u16::from_le_bytes([
                    boot.get(0x11).copied().unwrap_or(0),
                    boot.get(0x12).copied().unwrap_or(0),
                ]) == 0
                && u16::from_le_bytes([
                    boot.get(0x16).copied().unwrap_or(0),
                    boot.get(0x17).copied().unwrap_or(0),
                ]) == 0
                && u32::from_le_bytes([
                    boot.get(0x24).copied().unwrap_or(0),
                    boot.get(0x25).copied().unwrap_or(0),
                    boot.get(0x26).copied().unwrap_or(0),
                    boot.get(0x27).copied().unwrap_or(0),
                ]) > 0));
    if is_fat32 {
        let fs = fat32::parse(&ctx, &boot).with_context(|| {
            format!("FAT32 boot sector at byte {offset} but metadata unreadable")
        })?;
        return Ok(Some(fs));
    }

    Ok(None)
}

/// Read MBR (and, when the protective 0xEE type is present, GPT) partitions.
pub fn read_partition_table(source: &dyn ImageSource, image_len: u64) -> Result<Vec<Partition>> {
    if image_len < 512 {
        return Ok(Vec::new());
    }
    let mbr = read_vec_at(source, 0, 512)?;
    if mbr.get(510..512) != Some(&[0x55, 0xAA]) {
        return Ok(Vec::new());
    }

    let protective = (0..4).any(|i| {
        let e = 446 + i * 16;
        mbr.get(e + 4).copied() == Some(0xEE)
    });
    if protective {
        if let Ok(parts) = read_gpt(source, image_len) {
            if !parts.is_empty() {
                return Ok(parts);
            }
        }
    }

    let mut out = Vec::new();
    for i in 0..4 {
        let e = 446 + i * 16;
        let Some(status) = mbr.get(e).copied() else {
            break;
        };
        let Some(type_id) = mbr.get(e + 4).copied() else {
            break;
        };
        let start = u32::from_le_bytes([
            mbr.get(e + 8).copied().unwrap_or(0),
            mbr.get(e + 9).copied().unwrap_or(0),
            mbr.get(e + 10).copied().unwrap_or(0),
            mbr.get(e + 11).copied().unwrap_or(0),
        ]) as u64;
        let sectors = u32::from_le_bytes([
            mbr.get(e + 12).copied().unwrap_or(0),
            mbr.get(e + 13).copied().unwrap_or(0),
            mbr.get(e + 14).copied().unwrap_or(0),
            mbr.get(e + 15).copied().unwrap_or(0),
        ]) as u64;
        if (status != 0x00 && status != 0x80) || type_id == 0 || sectors == 0 {
            continue;
        }
        if start.saturating_mul(512) >= image_len {
            continue;
        }
        out.push(Partition {
            index: i + 1,
            start_lba: start,
            sectors,
            type_name: mbr_type_name(type_id).to_string(),
        });
    }
    Ok(out)
}

fn read_gpt(source: &dyn ImageSource, image_len: u64) -> Result<Vec<Partition>> {
    if image_len < 1024 {
        return Ok(Vec::new());
    }
    let hdr = read_vec_at(source, 512, 92)?;
    if hdr.get(0..8) != Some(b"EFI PART") {
        return Ok(Vec::new());
    }
    let entry_lba = u64::from_le_bytes(
        hdr.get(72..80)
            .context("GPT header is truncated")?
            .try_into()
            .context("GPT entry LBA")?,
    );
    let num_entries = u32::from_le_bytes(
        hdr.get(80..84)
            .context("GPT header is truncated")?
            .try_into()
            .context("GPT entry count")?,
    ) as usize;
    let entry_size = u32::from_le_bytes(
        hdr.get(84..88)
            .context("GPT header is truncated")?
            .try_into()
            .context("GPT entry size")?,
    ) as usize;
    if num_entries == 0 || num_entries > 4096 || !(128..=4096).contains(&entry_size) {
        bail!("invalid GPT partition entry geometry");
    }
    let table_off = entry_lba
        .checked_mul(512)
        .context("GPT entry table offset overflow")?;
    let table_len = num_entries
        .checked_mul(entry_size)
        .context("GPT entry table too large")?;
    if table_off.saturating_add(table_len as u64) > image_len {
        bail!("GPT entry table lies outside the image");
    }
    let table = read_vec_at(source, table_off, table_len)?;
    let mut out = Vec::new();
    for i in 0..num_entries {
        let start = i
            .checked_mul(entry_size)
            .context("GPT entry index overflow")?;
        let end = start
            .checked_add(entry_size)
            .context("GPT entry end overflow")?;
        let Some(e) = table.get(start..end) else {
            break;
        };
        if e.get(0..16).is_none_or(|g| g.iter().all(|&b| b == 0)) {
            continue;
        }
        let first = u64::from_le_bytes(
            e.get(32..40)
                .context("GPT entry is truncated")?
                .try_into()
                .context("GPT first LBA")?,
        );
        let last = u64::from_le_bytes(
            e.get(40..48)
                .context("GPT entry is truncated")?
                .try_into()
                .context("GPT last LBA")?,
        );
        if last < first || first == 0 {
            continue;
        }
        if first.saturating_mul(512) >= image_len {
            continue;
        }
        out.push(Partition {
            index: i + 1,
            start_lba: first,
            sectors: last.saturating_sub(first).saturating_add(1),
            type_name: gpt_type_name(e.get(0..16).unwrap_or(&[])).to_string(),
        });
    }
    Ok(out)
}

fn mbr_type_name(t: u8) -> &'static str {
    match t {
        0x01 => "FAT12",
        0x04 => "FAT16 <32M",
        0x05 => "Extended",
        0x06 => "FAT16",
        0x07 => "NTFS/exFAT",
        0x0b => "FAT32 CHS",
        0x0c => "FAT32 LBA",
        0x0e => "FAT16 LBA",
        0x0f => "Extended LBA",
        0x82 => "Linux swap",
        0x83 => "Linux",
        0xee => "GPT protective",
        0xef => "EFI System",
        _ => "other",
    }
}

fn gpt_type_name(g: &[u8]) -> &'static str {
    const BASIC_DATA: [u8; 16] = [
        0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99,
        0xc7,
    ];
    const EFI_SYSTEM: [u8; 16] = [
        0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9,
        0x3b,
    ];
    const LINUX_FS: [u8; 16] = [
        0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d,
        0xe4,
    ];
    if g == BASIC_DATA {
        "Microsoft basic data"
    } else if g == EFI_SYSTEM {
        "EFI System"
    } else if g == LINUX_FS {
        "Linux filesystem"
    } else {
        "GPT partition"
    }
}

/// Print a human-readable summary of one detected filesystem.
pub fn print_info(image: &Path, fs: &FsInfo) {
    let pct = if fs.total_clusters == 0 {
        0.0
    } else {
        fs.free_clusters as f64 * 100.0 / fs.total_clusters as f64
    };
    println!("Image          : {}", image.display());
    println!("Filesystem     : {} ({})", fs.kind, human_size(fs.length));
    match fs.partition_index {
        Some(i) => println!(
            "Partition      : #{} ({}) at byte {}",
            i,
            fs.partition_type.as_deref().unwrap_or("unknown"),
            fs.offset
        ),
        None => println!("Partition      : none (superfloppy/volume image)"),
    }
    if let Some(l) = &fs.label {
        if !l.is_empty() {
            println!("Volume label   : {l}");
        }
    }
    println!(
        "Geometry       : {} B/sector x {} sectors/cluster = {} B/cluster",
        fs.bytes_per_sector, fs.sectors_per_cluster, fs.cluster_size
    );
    println!("Clusters       : {}", fs.total_clusters);
    println!(
        "Free clusters  : {} ({:.1}%, {})",
        fs.free_clusters,
        pct,
        human_size(fs.free_bytes())
    );
    println!(
        "Free extents   : {} (largest {})",
        fs.free_extents.len(),
        human_size(fs.largest_free_extent())
    );
    for (k, v) in &fs.details {
        println!("{k:<15}: {v}");
    }
}
