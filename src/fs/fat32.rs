#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
use super::{
    sanitize_name, DeletedEntry, FsInfo, FsKind, MetadataLimits, MetadataPiece, MetadataPieceKind,
    VolumeCtx,
};
use crate::io::ImageSource;
use crate::signatures::FileType;
use crate::util::Extent;
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const FAT_BAD: u32 = 0x0FFF_FFF7;

fn le16(boot: &[u8], off: usize) -> u16 {
    let lo = boot.get(off).copied().unwrap_or(0);
    let hi = boot.get(off + 1).copied().unwrap_or(0);
    u16::from_le_bytes([lo, hi])
}

fn le32(boot: &[u8], off: usize) -> u32 {
    let b = [
        boot.get(off).copied().unwrap_or(0),
        boot.get(off + 1).copied().unwrap_or(0),
        boot.get(off + 2).copied().unwrap_or(0),
        boot.get(off + 3).copied().unwrap_or(0),
    ];
    u32::from_le_bytes(b)
}

struct Fat32Geometry {
    bps: u32,
    spc: u32,
    reserved: u32,
    num_fats: u32,
    fat_size: u64,
    total_from_boot: u64,
    available_sectors: u64,
    total_sectors: u64,
    data_start_sector: u64,
    data_start_byte: u64,
    total_clusters: u64,
    cluster_size: u64,
    fat_bytes_needed: u64,
    fat_offset: u64,
    volume_len: u64,
    root_cluster: u64,
    serial: u32,
}

fn parse_geometry(boot: &[u8], offset: u64, max_len: u64) -> Result<Fat32Geometry> {
    if boot.len() < 512 {
        bail!("FAT32: boot sector is shorter than 512 bytes");
    }
    let bps = le16(boot, 0x0b) as u32;
    let spc = boot.get(0x0d).copied().unwrap_or(0) as u32;
    let reserved = le16(boot, 0x0e) as u32;
    let num_fats = boot.get(0x10).copied().unwrap_or(0) as u32;
    let total16 = le16(boot, 0x13) as u64;
    let total32 = le32(boot, 0x20) as u64;
    let fat_size = le32(boot, 0x24) as u64;
    let root_cluster = le32(boot, 0x2c) as u64;
    let serial = le32(boot, 0x43);

    if !(512..=4096).contains(&bps) || !bps.is_power_of_two() {
        bail!("FAT32: invalid bytes/sector {bps}");
    }
    if spc == 0 || spc > 128 || !spc.is_power_of_two() {
        bail!("FAT32: invalid sectors/cluster {spc}");
    }
    if reserved == 0 || num_fats == 0 || num_fats > 4 {
        bail!("FAT32: invalid reserved/FAT count");
    }
    if fat_size == 0 {
        bail!("FAT32: zero FAT size");
    }

    let cluster_size = bps as u64 * spc as u64;
    let total_from_boot = if total32 != 0 { total32 } else { total16 };
    if total_from_boot == 0 {
        bail!("FAT32: total sector count is zero");
    }
    let available_sectors = max_len / bps as u64;
    let total_sectors = total_from_boot.min(available_sectors);
    let data_start_sector = reserved as u64
        + (num_fats as u64)
            .checked_mul(fat_size)
            .context("FAT32: FAT area overflow")?;
    if data_start_sector >= total_sectors {
        bail!("FAT32: data region starts beyond end of volume");
    }
    let data_sectors = total_sectors - data_start_sector;
    let total_clusters = data_sectors / spc as u64;
    if total_clusters == 0 {
        bail!("FAT32: volume has no data clusters");
    }
    if total_clusters > 0x0FFF_FFF5 {
        bail!("FAT32: cluster count {total_clusters} exceeds FAT32 limit");
    }

    let fat_entries = total_clusters
        .checked_add(2)
        .context("FAT32: cluster count overflow")?;
    let fat_bytes_needed = fat_entries
        .checked_mul(4)
        .context("FAT32: FAT size overflow")?;
    let fat_area_bytes = fat_size
        .checked_mul(bps as u64)
        .context("FAT32: FAT size overflow")?;
    if fat_bytes_needed > fat_area_bytes {
        bail!(
            "FAT32: FAT is too small ({fat_area_bytes} bytes available, {fat_bytes_needed} needed)"
        );
    }
    if fat_bytes_needed > 512 * 1024 * 1024 {
        bail!("FAT32: FAT larger than 512 MiB is not supported");
    }
    let fat_offset = offset
        .checked_add(reserved as u64 * bps as u64)
        .context("FAT32: FAT offset overflow")?;
    let data_start_byte = offset
        .checked_add(data_start_sector * bps as u64)
        .context("FAT32: data offset overflow")?;

    Ok(Fat32Geometry {
        bps,
        spc,
        reserved,
        num_fats,
        fat_size,
        total_from_boot,
        available_sectors,
        total_sectors,
        data_start_sector,
        data_start_byte,
        total_clusters,
        cluster_size,
        fat_bytes_needed,
        fat_offset,
        volume_len: total_sectors * bps as u64,
        root_cluster,
        serial,
    })
}

fn fat_free_extents(
    fat: &[u8],
    data_start_byte: u64,
    total_clusters: u64,
    cluster_size: u64,
) -> Result<(u64, u64, Vec<Extent>)> {
    let mut free_clusters = 0u64;
    let mut bad_clusters = 0u64;
    let mut extents: Vec<Extent> = Vec::new();
    let mut run: Option<(u64, u64, u64)> = None; // (physical start, first cluster, next cluster)

    for cluster in 2..(total_clusters + 2) {
        let index = (cluster as usize)
            .checked_mul(4)
            .context("FAT32: FAT entry index overflow")?;
        let raw = u32::from_le_bytes(
            fat.get(index..index + 4)
                .context("FAT32: FAT entry outside the table")?
                .try_into()
                .context("FAT32: invalid FAT entry")?,
        ) & 0x0FFF_FFFF;
        if raw == 0 {
            free_clusters += 1;
            let phys = data_start_byte
                .checked_add((cluster - 2).saturating_mul(cluster_size))
                .context("FAT32: cluster offset overflow")?;
            match run {
                Some((start, first, next)) if next == cluster => {
                    run = Some((start, first, cluster + 1));
                }
                Some((start, first, next)) => {
                    let len = (next - first) * cluster_size;
                    extents.push(Extent { offset: start, len });
                    run = Some((phys, cluster, cluster + 1));
                }
                None => run = Some((phys, cluster, cluster + 1)),
            }
        } else if raw == FAT_BAD {
            bad_clusters += 1;
            if let Some((start, first, next)) = run.take() {
                extents.push(Extent {
                    offset: start,
                    len: (next - first) * cluster_size,
                });
            }
        } else if let Some((start, first, next)) = run.take() {
            extents.push(Extent {
                offset: start,
                len: (next - first) * cluster_size,
            });
        }
    }
    if let Some((start, first, next)) = run.take() {
        extents.push(Extent {
            offset: start,
            len: (next - first) * cluster_size,
        });
    }
    Ok((free_clusters, bad_clusters, extents))
}

fn clamp_free_extents(
    mut extents: Vec<Extent>,
    volume_offset: u64,
    volume_len: u64,
) -> Vec<Extent> {
    // Clamp to the image (paranoia around a malformed total-sector field).
    let vol_end = volume_offset.saturating_add(volume_len);
    for e in &mut extents {
        let end = e.end().min(vol_end);
        e.len = end.saturating_sub(e.offset);
    }
    extents.retain(|e| e.len > 0);
    crate::util::merge_extents(extents)
}

fn fat32_details(geom: &Fat32Geometry, bad_clusters: u64) -> BTreeMap<String, String> {
    let mut details = BTreeMap::new();
    details.insert("total_sectors".into(), geom.total_sectors.to_string());
    details.insert("reserved_sectors".into(), geom.reserved.to_string());
    details.insert("fat_count".into(), geom.num_fats.to_string());
    details.insert("fat_size_sectors".into(), geom.fat_size.to_string());
    details.insert(
        "data_start_sector".into(),
        geom.data_start_sector.to_string(),
    );
    details.insert("root_cluster".into(), geom.root_cluster.to_string());
    details.insert("bad_clusters".into(), bad_clusters.to_string());
    details.insert("volume_serial".into(), format!("{:08X}", geom.serial));
    if geom.total_from_boot > geom.available_sectors {
        details.insert(
            "warning".into(),
            format!(
                "total_sectors clamped from {} to {}",
                geom.total_from_boot, geom.total_sectors
            ),
        );
    }
    details
}

/// Parse a FAT12/16/32 volume and return filesystem metadata/extents.
pub fn parse(ctx: &VolumeCtx<'_>, boot: &[u8]) -> Result<FsInfo> {
    let geom = parse_geometry(boot, ctx.offset, ctx.max_len)?;
    let fat = ctx
        .read_vec(geom.fat_offset, geom.fat_bytes_needed as usize)
        .with_context(|| format!("FAT32: cannot read FAT at byte {}", geom.fat_offset))?;
    let (free_clusters, bad_clusters, extents) = fat_free_extents(
        &fat,
        geom.data_start_byte,
        geom.total_clusters,
        geom.cluster_size,
    )?;
    let extents = clamp_free_extents(extents, ctx.offset, geom.volume_len);
    let details = fat32_details(&geom, bad_clusters);
    let label = String::from_utf8_lossy(boot.get(0x47..0x52).unwrap_or(&[]))
        .trim_end_matches([' ', '\0'])
        .to_string();

    Ok(FsInfo {
        kind: FsKind::Fat32,
        offset: ctx.offset,
        length: geom.volume_len,
        bytes_per_sector: geom.bps,
        sectors_per_cluster: geom.spc,
        cluster_size: geom.cluster_size,
        data_start: geom.data_start_byte,
        total_clusters: geom.total_clusters,
        free_clusters,
        free_extents: extents,
        partition_index: ctx.partition_index,
        partition_type: ctx.partition_type.clone(),
        label: Some(label),
        details,
    })
}

fn le16_at(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([
        buf.get(off).copied().unwrap_or(0),
        buf.get(off.saturating_add(1)).copied().unwrap_or(0),
    ])
}

fn le32_at(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([
        buf.get(off).copied().unwrap_or(0),
        buf.get(off.saturating_add(1)).copied().unwrap_or(0),
        buf.get(off.saturating_add(2)).copied().unwrap_or(0),
        buf.get(off.saturating_add(3)).copied().unwrap_or(0),
    ])
}

fn fat_value(fat: &[u8], cluster: u64) -> Option<u32> {
    let index = usize::try_from(cluster).ok()?.checked_mul(4)?;
    let end = index.checked_add(4)?;
    let bytes: [u8; 4] = fat.get(index..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes) & 0x0FFF_FFFF)
}

/// Count reused clusters in the contiguous run assumed for a deleted file.
///
/// Every FAT entry is checked, including end-of-chain values: any non-zero
/// entry means the cluster now belongs to another file. The second return value
/// is the number of contiguous reused runs, used for one note per run.
fn reused_clusters(
    fat: &[u8],
    start: u64,
    size: u64,
    cluster_size: u64,
    total_clusters: u64,
) -> (u64, u64) {
    if cluster_size == 0 || start < 2 {
        return (0, 0);
    }
    let wanted = size.div_ceil(cluster_size);
    let available = total_clusters.saturating_add(2).saturating_sub(start);
    let mut overwritten = 0u64;
    let mut runs = 0u64;
    let mut previous = false;
    for index in 0..wanted.min(available) {
        let cluster = start.saturating_add(index);
        let foreign = fat_value(fat, cluster).is_some_and(|value| value != 0);
        if foreign {
            overwritten = overwritten.saturating_add(1);
            if !previous {
                runs = runs.saturating_add(1);
            }
        }
        previous = foreign;
    }
    (overwritten, runs)
}

fn dos_datetime(date: u16, time: u16) -> Option<String> {
    let year = 1980u32.saturating_add((date >> 9) as u32);
    let month = (date >> 5) & 0x0F;
    let day = date & 0x1F;
    let hour = (time >> 11) & 0x1F;
    let minute = (time >> 5) & 0x3F;
    let second = (time & 0x1F).saturating_mul(2);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

fn lfn_checksum(short: &[u8]) -> u8 {
    let mut sum = 0u8;
    for byte in short.iter().take(11) {
        sum = sum.rotate_left(1).wrapping_add(*byte);
    }
    sum
}

/// Legal first bytes of an 8.3 name at the moment it was deleted.
///
/// `0xE5` is the deletion marker itself and `0x00` marks an empty slot.
/// Restricting the brute-force inversion to this set keeps a corrupted LFN
/// checksum from being "explained" by an impossible short-name byte.
fn plausible_short_first(byte: u8) -> bool {
    byte.is_ascii_uppercase()
        || byte.is_ascii_lowercase()
        || byte.is_ascii_digit()
        || matches!(
            byte,
            b'$' | b'%' | b'\'' | b'-' | b'_' | b'@' | b'~' | b'`' | b'{' | b'}'
        )
        || byte >= 0x80
}

/// Recover the original 8.3 name from the bytes left after deletion.
///
/// FAT replaces byte 0 with `0xE5` but keeps the other ten bytes. The LFN
/// checksum is over the original eleven bytes; the rotate/add checksum is
/// invertible in byte 0, so a plausible candidate is unique when the LFN is
/// genuine. An implausible or ambiguous candidate means the LFN is unbound.
fn original_short_name(stored: &[u8], lfn_sum: u8) -> Option<Vec<u8>> {
    let rest = stored.get(1..11)?;
    if stored.first().copied() != Some(0xE5) {
        return Some(stored.to_vec());
    }
    let mut found = None;
    let mut matches = 0usize;
    for first in 0u8..=u8::MAX {
        if !plausible_short_first(first) {
            continue;
        }
        let mut candidate = Vec::with_capacity(11);
        candidate.push(first);
        candidate.extend_from_slice(rest);
        if lfn_checksum(&candidate) == lfn_sum {
            matches = matches.saturating_add(1);
            found = Some(candidate);
            if matches > 1 {
                return None;
            }
        }
    }
    found
}

/// Decode LFN slots in physical order.
///
/// FAT stores long-name slots in descending sequence (`0x40` marks the first,
/// highest slot), so reversing the physical order yields the original UTF-16
/// string. Deleted slots have byte 0 overwritten with `0xE5`, which destroys
/// their sequence byte; physical order is the only reliable ordering left.
fn decode_lfn(parts: &[(u8, Vec<u16>, u8)], limit: usize) -> Option<(String, u8)> {
    let checksum = parts.last().map(|part| part.2)?;
    let mut units = Vec::new();
    for (_, chars, _) in parts.iter().rev() {
        units.extend(chars.iter().copied());
    }
    units.retain(|u| *u != 0 && *u != 0xFFFF);
    if units.is_empty() {
        return None;
    }
    let raw = String::from_utf16_lossy(&units);
    sanitize_name(&raw, limit).map(|name| (name, checksum))
}

fn short_display_name(raw: &[u8], limit: usize) -> Option<String> {
    let mut base = Vec::new();
    for byte in raw.iter().take(8) {
        if *byte == 0 || *byte == 0x20 {
            break;
        }
        base.push(if *byte == 0xE5 { b'_' } else { *byte });
    }
    let mut ext = Vec::new();
    for byte in raw.iter().skip(8).take(3) {
        if *byte == 0 || *byte == 0x20 {
            break;
        }
        ext.push(*byte);
    }
    let stem = String::from_utf8_lossy(&base);
    let suffix = String::from_utf8_lossy(&ext);
    let text = if suffix.is_empty() {
        stem.into_owned()
    } else {
        format!("{stem}.{suffix}")
    };
    sanitize_name(&text, limit)
}

fn signature_match(head: &[u8]) -> Option<&'static str> {
    FileType::all()
        .into_iter()
        .find(|kind| head.starts_with(kind.header()))
        .map(FileType::ext)
}

/// Recover deleted FAT32 directory entries. Bounded in depth, entries and bytes.
pub fn deleted_entries(
    ctx: &VolumeCtx<'_>,
    boot: &[u8],
    limits: &MetadataLimits,
) -> Result<Vec<DeletedEntry>> {
    let geom = parse_geometry(boot, ctx.offset, ctx.max_len)?;
    let fat = ctx.read_vec(geom.fat_offset, geom.fat_bytes_needed as usize)?;
    let cluster_bytes = usize::try_from(geom.cluster_size).context("FAT32: cluster too large")?;
    let volume_end = ctx.offset.saturating_add(ctx.max_len);
    let mut out = Vec::new();
    let mut queue = VecDeque::from([(geom.root_cluster, 0usize, false)]);
    let mut visited = BTreeSet::new();

    while let Some((cluster, depth, deleted_chain)) = queue.pop_front() {
        if depth > limits.max_depth
            || out.len() >= limits.max_entries
            || queue.len() >= limits.max_entries
        {
            break;
        }
        if cluster < 2
            || cluster >= geom.total_clusters.saturating_add(2)
            || !visited.insert(cluster)
        {
            continue;
        }
        let mut current = cluster;
        let mut steps = 0usize;
        loop {
            if steps >= limits.max_depth.saturating_mul(16).max(16)
                || out.len() >= limits.max_entries
            {
                break;
            }
            steps = steps.saturating_add(1);
            let phys = geom
                .data_start_byte
                .checked_add(current.saturating_sub(2).saturating_mul(geom.cluster_size))
                .context("FAT32: directory cluster offset overflow")?;
            if phys >= volume_end {
                break;
            }
            let data = ctx.read_vec(phys, cluster_bytes)?;
            let mut lfn_parts: Vec<(u8, Vec<u16>, u8)> = Vec::new();

            for (entry_index, entry) in data.chunks_exact(32).enumerate() {
                if out.len() >= limits.max_entries {
                    break;
                }
                let first = entry.first().copied().unwrap_or(0);
                let attr = entry.get(11).copied().unwrap_or(0);
                if attr == 0x0F {
                    let seq = first & 0x1F;
                    // A deleted LFN slot has no sequence byte left; its physical
                    // position in `lfn_parts` still carries the ordering.
                    if (first != 0xE5 && seq == 0) || lfn_parts.len() >= 32 {
                        lfn_parts.clear();
                        continue;
                    }
                    let mut chars = Vec::new();
                    for off in [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30] {
                        let lo = entry.get(off).copied().unwrap_or(0);
                        let hi = entry.get(off.saturating_add(1)).copied().unwrap_or(0);
                        chars.push(u16::from_le_bytes([lo, hi]));
                    }
                    let checksum = entry.get(13).copied().unwrap_or(0);
                    lfn_parts.push((seq, chars, checksum));
                    continue;
                }

                let start_high = u64::from(le16_at(entry, 0x14));
                let start_low = u64::from(le16_at(entry, 0x1A));
                let start = (start_high << 16 | start_low) & 0x0FFF_FFFF;
                if first != 0xE5 {
                    // Live subdirectories are traversed too: deleted files are
                    // often left inside a directory whose own entry is intact.
                    if attr & 0x10 != 0
                        && start >= 2
                        && start < geom.total_clusters.saturating_add(2)
                    {
                        queue.push_back((start, depth.saturating_add(1), false));
                    }
                    lfn_parts.clear();
                    continue;
                }

                // For a deleted entry byte 0 has been overwritten with 0xE5;
                // the LFN checksum must be checked against the original name.
                let short: Vec<u8> = entry.iter().take(11).copied().collect();
                let is_dir = attr & 0x10 != 0;
                let size = u64::from(le32_at(entry, 0x1C));

                if is_dir && start >= 2 && start < geom.total_clusters.saturating_add(2) {
                    queue.push_back((start, depth.saturating_add(1), true));
                    lfn_parts.clear();
                    continue;
                }
                if size < limits.min_size || size > limits.max_size || size > limits.max_bytes {
                    lfn_parts.clear();
                    continue;
                }
                if start < 2 || start >= geom.total_clusters.saturating_add(2) {
                    lfn_parts.clear();
                    continue;
                }

                let file_phys = geom
                    .data_start_byte
                    .checked_add(start.saturating_sub(2).saturating_mul(geom.cluster_size))
                    .context("FAT32: file cluster offset overflow")?;
                let head_len = usize::try_from(size.min(512)).unwrap_or(512);
                let head = ctx.read_vec(file_phys, head_len).unwrap_or_default();
                // An allocated FAT chain plus mismatching content is a guess,
                // not evidence of recovery: never emit a partial row for it.
                if signature_match(&head).is_none() {
                    lfn_parts.clear();
                    continue;
                }
                let (overwritten, overwritten_runs) =
                    reused_clusters(&fat, start, size, geom.cluster_size, geom.total_clusters);

                let (name, mismatch, lfn_recovered) =
                    if let Some((name, lfn_sum)) = decode_lfn(&lfn_parts, limits.max_name_bytes) {
                        match original_short_name(&short, lfn_sum) {
                            Some(original) if lfn_checksum(&original) == lfn_sum => {
                                (Some(name), false, true)
                            }
                            _ => (
                                short_display_name(&short, limits.max_name_bytes),
                                true,
                                true,
                            ),
                        }
                    } else {
                        (
                            short_display_name(&short, limits.max_name_bytes),
                            false,
                            false,
                        )
                    };

                let mut entry_out = DeletedEntry::new("fat32_dir_entry");
                entry_out.name = name;
                entry_out.mtime = dos_datetime(le16_at(entry, 0x18), le16_at(entry, 0x16));
                entry_out.lfn_checksum_mismatch = mismatch;
                entry_out.lfn_recovered = lfn_recovered;
                entry_out.record_offset = Some(phys.saturating_add(
                    u64::try_from(entry_index.saturating_mul(32)).unwrap_or(u64::MAX),
                ));
                if mismatch {
                    entry_out.notes.push("lfn_checksum_mismatch".to_string());
                }
                if overwritten > 0 {
                    entry_out.overwritten_clusters = overwritten;
                    for _ in 0..overwritten_runs.min(64) {
                        entry_out.notes.push("fat_cluster_reallocated".to_string());
                    }
                }
                let readable = volume_end
                    .saturating_sub(file_phys)
                    .min(size)
                    .min(limits.max_bytes);
                if readable < size {
                    entry_out.notes.push("truncated_by_volume".to_string());
                }
                if readable > 0 {
                    entry_out.pieces.push(MetadataPiece {
                        kind: MetadataPieceKind::Physical,
                        physical_start: Some(file_phys),
                        volume_offset: 0,
                        length: readable,
                        resident_bytes: None,
                    });
                }
                if !entry_out.pieces.is_empty() {
                    out.push(entry_out);
                }
                lfn_parts.clear();
            }

            let next = if deleted_chain {
                current.saturating_add(1)
            } else {
                match fat_value(&fat, current) {
                    Some(value)
                        if value >= 2
                            && u64::from(value) < geom.total_clusters.saturating_add(2) =>
                    {
                        u64::from(value)
                    }
                    _ => 0,
                }
            };
            if next < 2 || next >= geom.total_clusters.saturating_add(2) || visited.contains(&next)
            {
                break;
            }
            current = next;
        }
    }
    Ok(out)
}

/// A read-only slice source used only by the fuzz harness (never by the CLI).
#[doc(hidden)]
pub struct SliceSource<'a> {
    pub data: &'a [u8],
}

impl ImageSource for SliceSource<'_> {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if start >= self.data.len() {
            return Ok(0);
        }
        let available = self.data.len().saturating_sub(start);
        let n = buf.len().min(available);
        if let (Some(dst), Some(src)) = (buf.get_mut(..n), self.data.get(start..start + n)) {
            dst.copy_from_slice(src);
        }
        Ok(n)
    }
}

/// Bounded parser entry point for the `fat32_dir` fuzz target.
#[doc(hidden)]
pub fn deleted_entries_fuzz(
    boot: &[u8],
    image: &[u8],
    limits: &MetadataLimits,
) -> Vec<DeletedEntry> {
    let source = SliceSource { data: image };
    let ctx = VolumeCtx {
        source: &source,
        offset: 0,
        max_len: image.len() as u64,
        partition_index: None,
        partition_type: None,
    };
    deleted_entries(&ctx, boot, limits).unwrap_or_default()
}

#[cfg(test)]
// Test fixtures use fixed slices and expect valid layouts; the production
// parser above remains subject to the deny list.
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::io::tests::MemSource;

    fn boot() -> Vec<u8> {
        let mut b = vec![0u8; 512];
        b[0x0b..0x0d].copy_from_slice(&512u16.to_le_bytes());
        b[0x0d] = 1;
        b[0x0e..0x10].copy_from_slice(&1u16.to_le_bytes());
        b[0x10] = 1;
        b[0x13..0x15].copy_from_slice(&0u16.to_le_bytes());
        b[0x20..0x24].copy_from_slice(&16u32.to_le_bytes());
        b[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
        b[0x2c..0x30].copy_from_slice(&2u32.to_le_bytes());
        b[0x47..0x52].copy_from_slice(b"TESTVOL    ");
        b[510] = 0x55;
        b[511] = 0xAA;
        b
    }

    #[test]
    fn rejects_boot_sector_shorter_than_512() {
        let src = MemSource::new(vec![0u8; 1024]);
        let ctx = VolumeCtx {
            source: &src,
            offset: 0,
            max_len: 1024,
            partition_index: None,
            partition_type: None,
        };
        assert!(parse(&ctx, &[0u8; 100]).is_err());
    }

    #[test]
    fn parses_minimal_fat32() {
        // reserved=1 sector, one FAT of 1 sector, 16 total sectors, 512 B cluster.
        // Cluster 2 is the root directory (allocated); clusters 3.. are free.
        let mut img = vec![0u8; 16 * 512];
        img[..512].copy_from_slice(&boot());
        // FAT entry 0/1 are reserved; entry 2 = end-of-chain; entries 3.. = free.
        let fat = 512usize;
        img[fat..fat + 4].copy_from_slice(&0x0FFF_FFF8u32.to_le_bytes());
        img[fat + 4..fat + 8].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
        img[fat + 8..fat + 12].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
        let src = MemSource::new(img);
        let ctx = VolumeCtx {
            source: &src,
            offset: 0,
            max_len: 16 * 512,
            partition_index: Some(1),
            partition_type: Some("FAT32".into()),
        };
        let fs = parse(&ctx, &boot()).unwrap();
        assert_eq!(fs.kind, FsKind::Fat32);
        assert_eq!(fs.cluster_size, 512);
        assert_eq!(fs.total_clusters, 14);
        assert_eq!(fs.free_clusters, 13);
        assert_eq!(fs.label.as_deref(), Some("TESTVOL"));
    }
}
