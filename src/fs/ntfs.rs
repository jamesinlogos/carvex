#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
use super::{
    fat32::SliceSource, sanitize_name, DeletedEntry, FsInfo, FsKind, MetadataLimits, MetadataPiece,
    MetadataPieceKind, VolumeCtx,
};
use crate::io::{read_vec_at, ImageSource};
use crate::signatures::FileType;
use crate::util::Extent;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
struct Run {
    vcn: u64,
    lcn: Option<u64>,
    clusters: u64,
}

#[derive(Debug, Clone)]
struct DataAttr {
    runs: Vec<Run>,
    data_size: u64,
    resident: Option<Vec<u8>>,
}

/// Shared read context for the NTFS parser.
struct NtfsCtx<'a> {
    source: &'a dyn ImageSource,
    volume_offset: u64,
    volume_len: u64,
    cluster_size: u64,
    bytes_per_sector: u32,
}

/// Sector count of an NTFS volume.
///
/// The boot sector stores "sectors in volume − 1": the backup copy of the
/// boot sector occupies the very last sector of the volume.  Verified against
/// `bench/images/all-deleted_ntfs.img`, where the field says 126,975 while the
/// backup boot sector (NTFS OEM signature) sits at partition sector 126,975
/// and the `$Bitmap` covers the matching 15,872 clusters.  The clamp keeps a
/// truncated image safe.  (`// A-07`)
pub(crate) fn volume_sectors(field: u64, available_sectors: u64) -> u64 {
    field.saturating_add(1).min(available_sectors)
}

struct NtfsGeometry {
    bps: u32,
    spc: u32,
    total_sectors_boot: u64,
    available_sectors: u64,
    total_sectors: u64,
    total_clusters: u64,
    volume_len: u64,
    mft_lcn: u64,
    mftmirr_lcn: u64,
    record_size: u64,
    mft_abs: u64,
    serial: u64,
}

fn parse_geometry(boot: &[u8], offset: u64, max_len: u64) -> Result<NtfsGeometry> {
    if boot.len() < 512 {
        bail!("NTFS: boot sector is shorter than 512 bytes");
    }
    let bps = u16_at(boot, 0x0b)? as u32;
    let spc = u8_at(boot, 0x0d)? as u32;
    let total_sectors_boot = u64_at(boot, 0x28)?;
    let mft_lcn = u64_at(boot, 0x30)?;
    let mftmirr_lcn = u64_at(boot, 0x38)?;
    let cpr = u8_at(boot, 0x40)? as i8;
    let serial = u64_at(boot, 0x48)?;

    if !(512..=4096).contains(&bps) || !bps.is_power_of_two() {
        bail!("NTFS: invalid bytes/sector {bps}");
    }
    if spc == 0 || spc > 128 || !spc.is_power_of_two() {
        bail!("NTFS: invalid sectors/cluster {spc}");
    }
    if total_sectors_boot == 0 {
        bail!("NTFS: total sector count is zero");
    }
    let cluster_size = bps as u64 * spc as u64;
    let available_sectors = max_len / bps as u64;
    let total_sectors = volume_sectors(total_sectors_boot, available_sectors);
    let total_clusters = total_sectors / spc as u64;
    if total_clusters == 0 {
        bail!("NTFS: volume has no clusters");
    }
    let volume_len = total_sectors * bps as u64;
    if mft_lcn >= total_clusters {
        bail!("NTFS: $MFT LCN {mft_lcn} is outside the volume");
    }

    let record_size: u64 = if cpr > 0 {
        (cpr as u64)
            .checked_mul(cluster_size)
            .context("NTFS: invalid MFT record size")?
    } else if cpr < 0 {
        1u64.checked_shl(cpr.unsigned_abs() as u32)
            .context("NTFS: invalid MFT record size")?
    } else {
        bail!("NTFS: clusters per MFT record is zero");
    };
    if !(256..=65536).contains(&record_size) || record_size % bps as u64 != 0 {
        bail!("NTFS: implausible MFT record size {record_size}");
    }
    let mft_abs = offset
        .checked_add(
            mft_lcn
                .checked_mul(cluster_size)
                .context("NTFS: $MFT offset overflow")?,
        )
        .context("NTFS: $MFT absolute offset overflow")?;

    Ok(NtfsGeometry {
        bps,
        spc,
        total_sectors_boot,
        available_sectors,
        total_sectors,
        total_clusters,
        volume_len,
        mft_lcn,
        mftmirr_lcn,
        record_size,
        mft_abs,
        serial,
    })
}

fn bitmap_free_extents(
    bitmap: &[u8],
    total_clusters: u64,
    cluster_size: u64,
    volume_offset: u64,
    volume_len: u64,
) -> Result<(u64, Vec<Extent>)> {
    let mut free_clusters = 0u64;
    let mut extents: Vec<Extent> = Vec::new();
    let mut run: Option<(u64, u64, u64)> = None; // (physical start, first cluster, next cluster)
    for cluster in 0..total_clusters {
        let byte = bitmap
            .get((cluster / 8) as usize)
            .copied()
            .context("NTFS: $Bitmap became shorter than expected")?;
        let used = (byte >> (cluster % 8)) & 1 != 0;
        if !used {
            free_clusters += 1;
            let phys = volume_offset
                .checked_add(cluster.saturating_mul(cluster_size))
                .context("NTFS: cluster offset overflow")?;
            match run {
                Some((start, first, next)) if next == cluster => {
                    run = Some((start, first, cluster + 1));
                }
                Some((start, first, next)) => {
                    extents.push(Extent {
                        offset: start,
                        len: (next - first).saturating_mul(cluster_size),
                    });
                    run = Some((phys, cluster, cluster + 1));
                }
                None => run = Some((phys, cluster, cluster + 1)),
            }
        } else if let Some((start, first, next)) = run.take() {
            extents.push(Extent {
                offset: start,
                len: (next - first).saturating_mul(cluster_size),
            });
        }
    }
    if let Some((start, first, next)) = run.take() {
        extents.push(Extent {
            offset: start,
            len: (next - first).saturating_mul(cluster_size),
        });
    }
    for e in &mut extents {
        let end = e.end().min(volume_offset.saturating_add(volume_len));
        e.len = end.saturating_sub(e.offset);
    }
    extents.retain(|e| e.len > 0);
    Ok((free_clusters, crate::util::merge_extents(extents)))
}

fn ntfs_details(
    geom: &NtfsGeometry,
    mft_data_size: u64,
    mft_records: u64,
    bitmap_len: usize,
) -> BTreeMap<String, String> {
    let mut details = BTreeMap::new();
    details.insert("total_sectors".into(), geom.total_sectors.to_string());
    details.insert("mft_lcn".into(), geom.mft_lcn.to_string());
    details.insert("mft_mirror_lcn".into(), geom.mftmirr_lcn.to_string());
    details.insert("mft_record_size".into(), geom.record_size.to_string());
    details.insert("mft_data_size".into(), mft_data_size.to_string());
    details.insert("mft_records".into(), mft_records.to_string());
    details.insert("bitmap_bytes".into(), bitmap_len.to_string());
    details.insert("volume_serial".into(), format!("{:016X}", geom.serial));
    if geom.total_sectors_boot > geom.available_sectors {
        details.insert(
            "warning".into(),
            format!(
                "total_sectors clamped from {} to {}",
                geom.total_sectors_boot, geom.total_sectors
            ),
        );
    }
    details
}

/// Parse an NTFS volume and return filesystem metadata/extents.
pub fn parse(ctx: &VolumeCtx<'_>, boot: &[u8]) -> Result<FsInfo> {
    let geom = parse_geometry(boot, ctx.offset, ctx.max_len)?;
    let ntfs = NtfsCtx {
        source: ctx.source,
        volume_offset: ctx.offset,
        volume_len: geom.volume_len,
        cluster_size: geom.bps as u64 * geom.spc as u64,
        bytes_per_sector: geom.bps,
    };

    let mut rec0 = read_vec_at(ctx.source, geom.mft_abs, geom.record_size as usize)
        .context("NTFS: cannot read $MFT record 0")?;
    apply_fixup(&mut rec0, geom.bps)?;
    if rec0.get(0..4) != Some(b"FILE") {
        bail!("NTFS: $MFT record 0 has bad signature");
    }
    let mft_attrs = parse_attrs(&rec0)?;
    let mft_data = pick_data(&mft_attrs).context("NTFS: $MFT has no unnamed $DATA attribute")?;
    let mft_runs = mft_data.runs.clone();
    if mft_runs.is_empty() {
        bail!("NTFS: $MFT $DATA has an empty runlist");
    }
    let mft_data_size = mft_data.data_size;
    let mft_records = mft_data_size / geom.record_size;
    if mft_records <= 6 {
        bail!("NTFS: $MFT contains only {mft_records} records; $Bitmap (record 6) missing");
    }

    // $Bitmap is MFT record number 6 and is authoritative for cluster allocation.
    let bitmap_rec = ntfs
        .read_record(&mft_runs, 6, geom.record_size)
        .context("NTFS: cannot read $Bitmap MFT record")?;
    if bitmap_rec.get(0..4) != Some(b"FILE") {
        bail!("NTFS: $Bitmap record has bad signature");
    }
    if bitmap_rec.get(0x16).copied().unwrap_or(0) & 1 == 0 {
        bail!("NTFS: $Bitmap record is not marked in use");
    }
    let bitmap_attrs = parse_attrs(&bitmap_rec)?;
    let bitmap_attr =
        pick_data(&bitmap_attrs).context("NTFS: $Bitmap has no unnamed $DATA attribute")?;
    let bitmap_data_size = bitmap_attr.data_size;
    let bitmap: Vec<u8> = if let Some(res) = &bitmap_attr.resident {
        res.clone()
    } else {
        if bitmap_data_size > 512 * 1024 * 1024 {
            bail!("NTFS: $Bitmap larger than 512 MiB is not supported");
        }
        ntfs.read_logical(&bitmap_attr.runs, 0, bitmap_data_size)?
    };
    let needed_bitmap = geom.total_clusters.div_ceil(8) as usize;
    if bitmap.len() < needed_bitmap {
        bail!(
            "NTFS: $Bitmap is too small ({} bytes, {} required)",
            bitmap.len(),
            needed_bitmap
        );
    }

    let (free_clusters, extents) = bitmap_free_extents(
        &bitmap,
        geom.total_clusters,
        ntfs.cluster_size,
        ctx.offset,
        geom.volume_len,
    )?;

    // Optional nicety: volume label from $Volume (MFT record 3), attribute 0x60.
    let label = ntfs
        .read_volume_label(&mft_runs, geom.record_size)
        .ok()
        .flatten();

    Ok(FsInfo {
        kind: FsKind::Ntfs,
        offset: ctx.offset,
        length: geom.volume_len,
        bytes_per_sector: geom.bps,
        sectors_per_cluster: geom.spc,
        cluster_size: ntfs.cluster_size,
        data_start: ctx.offset,
        total_clusters: geom.total_clusters,
        free_clusters,
        free_extents: extents,
        partition_index: ctx.partition_index,
        partition_type: ctx.partition_type.clone(),
        label,
        details: ntfs_details(&geom, mft_data_size, mft_records, bitmap.len()),
    })
}

fn u8_at(buf: &[u8], off: usize) -> Result<u8> {
    buf.get(off)
        .copied()
        .with_context(|| format!("NTFS: byte at offset {off} is outside the buffer"))
}

fn u16_at(buf: &[u8], off: usize) -> Result<u16> {
    let b = buf
        .get(off..off + 2)
        .with_context(|| format!("NTFS: u16 at offset {off} is outside the buffer"))?;
    Ok(u16::from_le_bytes(
        b.try_into().context("NTFS: invalid u16")?,
    ))
}

fn u32_at(buf: &[u8], off: usize) -> Result<u32> {
    let b = buf
        .get(off..off + 4)
        .with_context(|| format!("NTFS: u32 at offset {off} is outside the buffer"))?;
    Ok(u32::from_le_bytes(
        b.try_into().context("NTFS: invalid u32")?,
    ))
}

fn u64_at(buf: &[u8], off: usize) -> Result<u64> {
    let b = buf
        .get(off..off + 8)
        .with_context(|| format!("NTFS: u64 at offset {off} is outside the buffer"))?;
    Ok(u64::from_le_bytes(
        b.try_into().context("NTFS: invalid u64")?,
    ))
}

fn apply_fixup(rec: &mut [u8], sector_size: u32) -> Result<()> {
    if rec.len() < 8 || sector_size == 0 {
        bail!("NTFS: record too short");
    }
    let usa_off = u16_at(rec, 4)? as usize;
    let usa_cnt = u16_at(rec, 6)? as usize;
    let sectors = rec.len() / sector_size as usize;
    if sectors == 0 || usa_cnt < sectors + 1 {
        bail!("NTFS: invalid update sequence array");
    }
    let usa_end = usa_off
        .checked_add(
            usa_cnt
                .checked_mul(2)
                .context("NTFS: update sequence overflow")?,
        )
        .context("NTFS: update sequence overflow")?;
    if usa_end > rec.len() {
        bail!("NTFS: update sequence array outside record");
    }
    let usn = [
        u8_at(rec, usa_off)?,
        u8_at(rec, usa_off.checked_add(1).context("NTFS: USN overflow")?)?,
    ];
    for i in 0..sectors {
        let end = (i + 1) * sector_size as usize;
        if end > rec.len() {
            break;
        }
        if rec.get(end - 2..end) != Some(&usn) {
            bail!("NTFS: update sequence mismatch in record");
        }
        let src = usa_off
            .checked_add(2 + i * 2)
            .context("NTFS: update sequence index overflow")?;
        let replacement = [
            u8_at(rec, src)?,
            u8_at(rec, src.checked_add(1).context("NTFS: USN overflow")?)?,
        ];
        let dst = rec
            .get_mut(end - 2..end)
            .context("NTFS: fixup target outside record")?;
        dst.copy_from_slice(&replacement);
    }
    Ok(())
}

fn parse_attrs(rec: &[u8]) -> Result<Vec<DataAttr>> {
    let first_attr = u16_at(rec, 0x14)? as usize;
    if first_attr >= rec.len() {
        bail!("NTFS: attribute offset outside record");
    }
    let mut pos = first_attr;
    let mut out = Vec::new();
    let mut guard = 0;
    while pos.checked_add(8).is_some_and(|end| end <= rec.len()) {
        guard += 1;
        if guard > 4096 {
            bail!("NTFS: attribute loop guard exceeded");
        }
        let type_id = u32_at(rec, pos)?;
        if type_id == 0xFFFF_FFFF {
            break;
        }
        let len = u32_at(rec, pos + 4)? as usize;
        let attr_end = pos
            .checked_add(len)
            .context("NTFS: attribute length overflow")?;
        if len < 0x18 || attr_end > rec.len() {
            bail!("NTFS: invalid attribute length {len}");
        }
        let non_resident = u8_at(rec, pos + 8)?;
        let name_len = u8_at(rec, pos + 9)? as usize;
        if type_id == 0x80 && name_len == 0 {
            if non_resident == 0 {
                let value_len = u32_at(rec, pos + 0x10)? as usize;
                let value_off = u16_at(rec, pos + 0x14)? as usize;
                let value_end = value_off
                    .checked_add(value_len)
                    .context("NTFS: resident $DATA length overflow")?;
                if value_end > len {
                    bail!("NTFS: resident $DATA value outside attribute");
                }
                let value = rec
                    .get(pos + value_off..pos + value_end)
                    .context("NTFS: resident $DATA outside record")?
                    .to_vec();
                out.push(DataAttr {
                    runs: Vec::new(),
                    data_size: value_len as u64,
                    resident: Some(value),
                });
            } else {
                let start_vcn = u64_at(rec, pos + 0x10)?;
                let run_off = u16_at(rec, pos + 0x20)? as usize;
                let data_size = u64_at(rec, pos + 0x30)?;
                if run_off >= len {
                    bail!("NTFS: runlist offset outside attribute");
                }
                let run_data = rec
                    .get(pos + run_off..attr_end)
                    .context("NTFS: runlist outside record")?;
                let mut runs = parse_runlist(run_data)?;
                for r in &mut runs {
                    r.vcn = r.vcn.saturating_add(start_vcn);
                }
                out.push(DataAttr {
                    runs,
                    data_size,
                    resident: None,
                });
            }
        }
        // Volume label lives in a resident attribute 0x60 (handled separately).
        if type_id == 0x60 && non_resident == 0 {
            let value_len = u32_at(rec, pos + 0x10)? as usize;
            let value_off = u16_at(rec, pos + 0x14)? as usize;
            let value_end = value_off
                .checked_add(value_len)
                .context("NTFS: volume label length overflow")?;
            if value_end <= len {
                let value = rec
                    .get(pos + value_off..pos + value_end)
                    .context("NTFS: volume label outside record")?
                    .to_vec();
                out.push(DataAttr {
                    runs: Vec::new(),
                    data_size: value_len as u64,
                    resident: Some(value),
                });
            }
        }
        pos = attr_end;
    }
    Ok(out)
}

/// Pick the (first) unnamed $DATA attribute. Multiple extents are not merged here;
/// small/forensic images normally keep $MFT in a single attribute.
fn pick_data(attrs: &[DataAttr]) -> Option<&DataAttr> {
    attrs.iter().find(|a| a.data_size > 0 || !a.runs.is_empty())
}

/// Bounded runlist parser entry point for regression/property tests.
#[doc(hidden)]
pub fn parse_runlist_public(data: &[u8]) -> Result<Vec<(u64, Option<u64>, u64)>> {
    parse_runlist(data).map(|runs| {
        runs.into_iter()
            .map(|run| (run.vcn, run.lcn, run.clusters))
            .collect()
    })
}

fn parse_runlist(data: &[u8]) -> Result<Vec<Run>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut vcn = 0u64;
    let mut lcn = 0i64;
    while pos < data.len() {
        let Some(&header) = data.get(pos) else {
            break;
        };
        pos += 1;
        if header == 0 {
            break;
        }
        let len_bytes = (header & 0x0F) as usize;
        let off_bytes = (header >> 4) as usize;
        if len_bytes == 0 || len_bytes > 8 || off_bytes > 8 {
            bail!("NTFS: invalid runlist header 0x{header:02x}");
        }
        let end = pos
            .checked_add(len_bytes + off_bytes)
            .context("NTFS: runlist entry overflow")?;
        if end > data.len() {
            bail!("NTFS: truncated runlist");
        }
        let clusters = read_uint_le(
            data.get(pos..pos + len_bytes)
                .context("NTFS: truncated run length")?,
        );
        pos += len_bytes;
        if clusters == 0 {
            bail!("NTFS: zero-length run");
        }
        let physical = if off_bytes == 0 {
            None
        } else {
            let delta = read_int_le(
                data.get(pos..pos + off_bytes)
                    .context("NTFS: truncated run offset")?,
            );
            pos += off_bytes;
            lcn = lcn
                .checked_add(delta)
                .context("NTFS: runlist LCN overflow")?;
            if lcn < 0 {
                bail!("NTFS: negative runlist LCN");
            }
            Some(lcn as u64)
        };
        out.push(Run {
            vcn,
            lcn: physical,
            clusters,
        });
        vcn = vcn
            .checked_add(clusters)
            .context("NTFS: runlist VCN overflow")?;
    }
    Ok(out)
}

fn read_uint_le(b: &[u8]) -> u64 {
    let mut v = 0u64;
    for (i, &byte) in b.iter().enumerate() {
        v |= (byte as u64) << (i * 8);
    }
    v
}

fn read_int_le(b: &[u8]) -> i64 {
    let raw = read_uint_le(b);
    let bits = (b.len() * 8) as u32;
    if bits == 64 {
        raw as i64
    } else {
        let shift = 64 - bits;
        ((raw << shift) as i64) >> shift
    }
}

impl NtfsCtx<'_> {
    fn read_logical(&self, runs: &[Run], byte_off: u64, len: u64) -> Result<Vec<u8>> {
        if len > 512 * 1024 * 1024 {
            bail!("NTFS: logical read larger than 512 MiB is not supported");
        }
        let mut out: Vec<u8> = Vec::with_capacity(len as usize);
        let end = byte_off
            .checked_add(len)
            .context("NTFS: logical read overflow")?;
        let mut cur = byte_off;
        for run in runs {
            let run_start = run
                .vcn
                .checked_mul(self.cluster_size)
                .context("NTFS: run start overflow")?;
            let run_end = run_start
                .checked_add(
                    run.clusters
                        .checked_mul(self.cluster_size)
                        .context("NTFS: run length overflow")?,
                )
                .context("NTFS: run end overflow")?;
            if run_end <= cur {
                continue;
            }
            if run_start > cur {
                let gap = (run_start.min(end) - cur) as usize;
                out.resize(out.len().saturating_add(gap), 0);
                cur += gap as u64;
                if cur >= end {
                    break;
                }
            }
            let within = cur - run_start;
            let take = (run_end - cur).min(end - cur);
            if take == 0 {
                continue;
            }
            match run.lcn {
                Some(lcn) => {
                    let phys = self
                        .volume_offset
                        .checked_add(
                            lcn.checked_mul(self.cluster_size)
                                .context("NTFS: run physical offset overflow")?,
                        )
                        .and_then(|v| v.checked_add(within))
                        .context("NTFS: run physical offset overflow")?;
                    let phys_end = phys.checked_add(take).context("NTFS: run read overflow")?;
                    if phys_end > self.volume_offset.saturating_add(self.volume_len) {
                        bail!("NTFS: run points outside the volume");
                    }
                    out.extend_from_slice(&read_vec_at(self.source, phys, take as usize)?);
                }
                None => out.resize(out.len().saturating_add(take as usize), 0),
            }
            cur += take;
            if cur >= end {
                break;
            }
        }
        if (out.len() as u64) < len {
            bail!(
                "NTFS: runlist does not cover requested range ({} of {} bytes)",
                out.len(),
                len
            );
        }
        out.truncate(len as usize);
        Ok(out)
    }

    fn read_record(&self, mft_runs: &[Run], record_no: u64, record_size: u64) -> Result<Vec<u8>> {
        let byte_off = record_no
            .checked_mul(record_size)
            .context("NTFS: MFT record offset overflow")?;
        let mut rec = self.read_logical(mft_runs, byte_off, record_size)?;
        apply_fixup(&mut rec, self.bytes_per_sector)?;
        Ok(rec)
    }

    fn read_volume_label(&self, mft_runs: &[Run], record_size: u64) -> Result<Option<String>> {
        let rec = self.read_record(mft_runs, 3, record_size)?;
        if rec.get(0..4) != Some(b"FILE") {
            return Ok(None);
        }
        let attrs = parse_attrs(&rec)?;
        for a in attrs {
            if let Some(data) = a.resident {
                if data.len() >= 2 && data.len() % 2 == 0 {
                    let units: Vec<u16> = data
                        .chunks_exact(2)
                        .map_while(|c| <[u8; 2]>::try_from(c).ok().map(u16::from_le_bytes))
                        .take_while(|&c| c != 0)
                        .collect();
                    let s = String::from_utf16_lossy(&units);
                    if !s.is_empty() && s.chars().all(|c| !c.is_control()) {
                        return Ok(Some(s));
                    }
                }
            }
        }
        Ok(None)
    }
}

/// An attribute as needed by deleted-record recovery.
#[derive(Debug, Clone)]
struct DeletedAttr {
    type_id: u32,
    name: Option<String>,
    flags: u16,
    non_resident: bool,
    resident: Option<Vec<u8>>,
    runs: Vec<Run>,
    data_size: u64,
    initialized_size: u64,
    lowest_vcn: u64,
}

fn parse_deleted_attrs(rec: &[u8], limits: &MetadataLimits) -> Result<Vec<DeletedAttr>> {
    let first = u16_at(rec, 0x14)? as usize;
    if first >= rec.len() {
        bail!("NTFS: attribute offset outside record");
    }
    let mut pos = first;
    let mut out = Vec::new();
    let mut guard = 0usize;
    while pos.checked_add(8).is_some_and(|end| end <= rec.len()) {
        guard = guard.saturating_add(1);
        if guard > 4096 || out.len() >= 512 {
            break;
        }
        let type_id = u32_at(rec, pos)?;
        if type_id == 0xFFFF_FFFF {
            break;
        }
        let len = u32_at(rec, pos.saturating_add(4))? as usize;
        let attr_end = pos.checked_add(len).context("NTFS: attribute overflow")?;
        if len < 0x18 || attr_end > rec.len() {
            bail!("NTFS: invalid attribute length {len}");
        }
        let non_resident = u8_at(rec, pos.saturating_add(8))? != 0;
        let name_len = u8_at(rec, pos.saturating_add(9))? as usize;
        let name_off = u16_at(rec, pos.saturating_add(0x0A))? as usize;
        let flags = u16_at(rec, pos.saturating_add(0x0C))?;
        let name = if name_len == 0 {
            None
        } else {
            let bytes = name_len.saturating_mul(2);
            let start = pos
                .checked_add(name_off)
                .context("NTFS: name offset overflow")?;
            let end = start
                .checked_add(bytes)
                .context("NTFS: name end overflow")?;
            if end > attr_end {
                bail!("NTFS: attribute name outside attribute");
            }
            let units: Vec<u16> = rec
                .get(start..end)
                .context("NTFS: attribute name outside record")?
                .chunks_exact(2)
                .filter_map(|c| <[u8; 2]>::try_from(c).ok().map(u16::from_le_bytes))
                .collect();
            sanitize_name(&String::from_utf16_lossy(&units), limits.max_name_bytes)
        };

        let (resident, runs, data_size, initialized_size, lowest_vcn) = if non_resident {
            let lowest_vcn = u64_at(rec, pos.saturating_add(0x28))?;
            let data_size = u64_at(rec, pos.saturating_add(0x30))?;
            let initialized_size = u64_at(rec, pos.saturating_add(0x38))?;
            let run_off = u16_at(rec, pos.saturating_add(0x20))? as usize;
            let start = pos
                .checked_add(run_off)
                .context("NTFS: runlist offset overflow")?;
            if start > attr_end {
                bail!("NTFS: runlist offset outside attribute");
            }
            let mut runs = parse_runlist(rec.get(start..attr_end).unwrap_or(&[]))?;
            if runs.len() > limits.max_runlist_entries {
                bail!("NTFS: runlist exceeds bound");
            }
            let start_vcn = u64_at(rec, pos.saturating_add(0x10))?;
            for r in &mut runs {
                r.vcn = r.vcn.saturating_add(start_vcn);
            }
            (None, runs, data_size, initialized_size, lowest_vcn)
        } else {
            let value_len = u32_at(rec, pos.saturating_add(0x10))? as usize;
            let value_off = u16_at(rec, pos.saturating_add(0x14))? as usize;
            let start = pos
                .checked_add(value_off)
                .context("NTFS: resident value offset overflow")?;
            let end = start
                .checked_add(value_len)
                .context("NTFS: resident value end overflow")?;
            if end > attr_end {
                bail!("NTFS: resident value outside attribute");
            }
            let bytes = rec
                .get(start..end)
                .context("NTFS: resident value outside record")?
                .to_vec();
            (
                Some(bytes),
                Vec::new(),
                value_len as u64,
                value_len as u64,
                0,
            )
        };
        out.push(DeletedAttr {
            type_id,
            name,
            flags,
            non_resident,
            resident,
            runs,
            data_size,
            initialized_size,
            lowest_vcn,
        });
        pos = attr_end;
    }
    Ok(out)
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days.saturating_add(719_468);
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era.saturating_mul(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_000) / 365;
    let year = yoe.saturating_add(era.saturating_mul(400));
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year, month, day)
}

fn filetime_iso(raw: u64) -> Option<String> {
    if raw == 0 || raw == u64::MAX {
        return None;
    }
    let unix = (raw / 10_000_000) as i64 - 11_644_473_600;
    if unix < 0 {
        return None;
    }
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    ))
}

fn ntfs_signature_match(head: &[u8]) -> Option<&'static str> {
    FileType::all()
        .into_iter()
        .find(|kind| head.starts_with(kind.header()))
        .map(FileType::ext)
}

fn read_volume_bitmap(
    ntfs: &NtfsCtx<'_>,
    mft_runs: &[Run],
    geom: &NtfsGeometry,
    limits: &MetadataLimits,
) -> Option<Vec<u8>> {
    let rec = ntfs.read_record(mft_runs, 6, geom.record_size).ok()?;
    if rec.get(0..4) != Some(b"FILE") {
        return None;
    }
    let attrs = parse_deleted_attrs(&rec, limits).ok()?;
    let attr = attrs
        .iter()
        .find(|a| a.type_id == 0xB0 || a.type_id == 0x80)?;
    let needed = usize::try_from(geom.total_clusters.div_ceil(8)).ok()?;
    if needed as u64 > limits.max_work_bytes {
        return None;
    }
    if let Some(bytes) = &attr.resident {
        return bytes.get(..needed).map(<[u8]>::to_vec);
    }
    ntfs.read_logical(&attr.runs, 0, needed as u64).ok()
}

fn bitmap_used(bitmap: &[u8], lcn: u64) -> bool {
    let Some(index) = lcn.checked_div(8).and_then(|v| usize::try_from(v).ok()) else {
        return false;
    };
    bitmap
        .get(index)
        .is_some_and(|byte| (byte >> (lcn % 8)) & 1 != 0)
}

/// Recover deleted NTFS `$MFT` records, bounded by `limits`.
pub fn deleted_entries(
    ctx: &VolumeCtx<'_>,
    boot: &[u8],
    limits: &MetadataLimits,
) -> Result<Vec<DeletedEntry>> {
    let geom = parse_geometry(boot, ctx.offset, ctx.max_len)?;
    let ntfs = NtfsCtx {
        source: ctx.source,
        volume_offset: ctx.offset,
        volume_len: geom.volume_len,
        cluster_size: geom.bps as u64 * geom.spc as u64,
        bytes_per_sector: geom.bps,
    };
    let mut rec0 = read_vec_at(ctx.source, geom.mft_abs, geom.record_size as usize)
        .context("NTFS: cannot read $MFT record 0")?;
    apply_fixup(&mut rec0, geom.bps)?;
    if rec0.get(0..4) != Some(b"FILE") {
        bail!("NTFS: $MFT record 0 has bad signature");
    }
    let mft_attrs = parse_attrs(&rec0)?;
    let mft_data = pick_data(&mft_attrs).context("NTFS: $MFT has no unnamed $DATA attribute")?;
    let mft_runs = mft_data.runs.clone();
    let count = mft_data
        .data_size
        .checked_div(geom.record_size)
        .unwrap_or(0)
        .min(limits.max_mft_scan_bytes / geom.record_size.max(1))
        .min(limits.max_entries as u64);
    let volume_end = ctx.offset.saturating_add(ctx.max_len);
    let bitmap = read_volume_bitmap(&ntfs, &mft_runs, &geom, limits).unwrap_or_default();
    let mut out = Vec::new();

    for index in 0..count {
        if out.len() >= limits.max_entries {
            break;
        }
        let Ok(rec) = ntfs.read_record(&mft_runs, index, geom.record_size) else {
            continue;
        };
        if rec.get(0..4) != Some(b"FILE") {
            continue;
        }
        if u16_at(&rec, 0x10).unwrap_or(0) == 0 {
            continue;
        }
        if u8_at(&rec, 0x16).unwrap_or(0) & 1 != 0 {
            continue;
        }
        let Ok(attrs) = parse_deleted_attrs(&rec, limits) else {
            continue;
        };
        let Some(data) = attrs.iter().find(|a| a.type_id == 0x80 && a.name.is_none()) else {
            continue;
        };
        let seq = u16_at(&rec, 0x10).unwrap_or(0);
        let record_phys = ctx
            .offset
            .checked_add(index.saturating_mul(geom.record_size))
            .unwrap_or(ctx.offset);
        let mut notes: Vec<String> = Vec::new();
        let mut unsupported = attrs.iter().any(|a| {
            a.type_id == 0x20
                || (a.flags & 0x0001) != 0
                || (a.flags & 0x4000) != 0
                || a.lowest_vcn > 0
        });
        if unsupported {
            notes.push("unsupported_mft_feature".to_string());
        }
        if (data.flags & 0x8000) != 0 {
            notes.push("sparse_attribute".to_string());
        }
        if data.initialized_size < data.data_size {
            notes.push("uninitialized_tail".to_string());
        }

        let std_info = attrs.iter().find(|a| a.type_id == 0x10 && !a.non_resident);
        let mtime = std_info
            .and_then(|a| a.resident.as_ref())
            .and_then(|v| u64_at(v, 0x08).ok())
            .and_then(filetime_iso);
        let file_name = attrs.iter().find(|a| a.type_id == 0x30 && !a.non_resident);
        let name = file_name.and_then(|a| {
            let value = a.resident.as_ref()?;
            let len = usize::from(u8_at(value, 0x40).ok()?);
            let units: Vec<u16> = value
                .get(0x42..)?
                .chunks_exact(2)
                .take(len)
                .filter_map(|c| <[u8; 2]>::try_from(c).ok().map(u16::from_le_bytes))
                .collect();
            sanitize_name(&String::from_utf16_lossy(&units), limits.max_name_bytes)
        });
        if let Some(parent) = file_name
            .and_then(|a| a.resident.as_ref())
            .and_then(|v| u64_at(v, 0).ok())
        {
            let parent_seq = (parent >> 48) as u16;
            let parent_no = parent & 0x0000_FFFF_FFFF_FFFF;
            if parent_seq != 0 && parent_seq != seq {
                notes.push("parent_sequence_mismatch".to_string());
                unsupported = true;
            }
            if parent_no >= count && parent_no != 0 {
                notes.push("parent_out_of_range".to_string());
                unsupported = true;
            }
        }

        let covered: u64 = data.runs.iter().fold(0u64, |acc, run| {
            let offset = run.vcn.saturating_mul(ntfs.cluster_size);
            if offset >= data.data_size {
                return acc;
            }
            let length = run
                .clusters
                .saturating_mul(ntfs.cluster_size)
                .min(data.data_size.saturating_sub(offset));
            acc.saturating_add(length)
        });
        if covered < data.data_size {
            notes.push("runlist_short".to_string());
            if data.initialized_size >= data.data_size {
                unsupported = true;
            }
        }

        let mut entry = DeletedEntry::new("ntfs_deleted_mft");
        entry.name = name;
        entry.mtime = mtime;
        entry.notes = notes;
        entry.unsupported = unsupported;
        entry.record_offset = Some(record_phys);

        if let Some(bytes) = &data.resident {
            if bytes.len() > limits.max_data_bytes as usize {
                continue;
            }
            if ntfs_signature_match(bytes).is_none() && !unsupported {
                continue;
            }
            entry.pieces.push(MetadataPiece {
                kind: MetadataPieceKind::Resident,
                physical_start: None,
                volume_offset: 0,
                length: bytes.len() as u64,
                resident_bytes: Some(bytes.clone()),
            });
            out.push(entry);
            continue;
        }

        let mut total = 0u64;
        let mut head: Vec<u8> = Vec::new();
        let mut bad_run = false;
        for run in &data.runs {
            let offset = run
                .vcn
                .checked_mul(ntfs.cluster_size)
                .context("NTFS: run VCN overflow")?;
            if offset >= data.data_size {
                break;
            }
            let raw_len = run.clusters.saturating_mul(ntfs.cluster_size);
            let length = raw_len.min(data.data_size.saturating_sub(offset));
            if length == 0 {
                continue;
            }
            total = total.saturating_add(length);
            if total > limits.max_data_bytes {
                entry.notes.push("truncated_by_limits".to_string());
                entry.unsupported = true;
                break;
            }
            match run.lcn {
                Some(lcn) => {
                    if lcn >= geom.total_clusters {
                        bad_run = true;
                        break;
                    }
                    let phys = ctx
                        .offset
                        .checked_add(lcn.saturating_mul(ntfs.cluster_size))
                        .context("NTFS: run physical offset overflow")?;
                    if phys.saturating_add(length) > volume_end {
                        bad_run = true;
                        break;
                    }
                    if bitmap_used(&bitmap, lcn) {
                        entry.overwritten_clusters = entry.overwritten_clusters.saturating_add(1);
                    }
                    if head.is_empty() {
                        let want = usize::try_from(length.min(512)).unwrap_or(512);
                        head = read_vec_at(ctx.source, phys, want).unwrap_or_default();
                    }
                    entry.pieces.push(MetadataPiece {
                        kind: MetadataPieceKind::Physical,
                        physical_start: Some(phys),
                        volume_offset: offset,
                        length,
                        resident_bytes: None,
                    });
                }
                None => {
                    entry.pieces.push(MetadataPiece {
                        kind: MetadataPieceKind::Sparse,
                        physical_start: None,
                        volume_offset: offset,
                        length,
                        resident_bytes: None,
                    });
                }
            }
        }
        if bad_run {
            entry.notes.push("runlist_out_of_bounds".to_string());
            entry.unsupported = true;
            entry.pieces.clear();
        }
        if entry.overwritten_clusters > 0 {
            entry.notes.push("bitmap_cluster_allocated".to_string());
        }
        if entry.pieces.is_empty() {
            if unsupported || bad_run {
                out.push(entry);
            }
            continue;
        }
        if ntfs_signature_match(&head).is_none() && !unsupported && entry.overwritten_clusters == 0
        {
            continue;
        }
        out.push(entry);
    }
    Ok(out)
}

/// Bounded parser entry point for the `ntfs_deleted_mft` fuzz target.
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

    // A-07: the boot sector counts sectors minus one (the backup boot sector
    // lives in the last sector), and the count never exceeds the image.
    #[test]
    fn volume_sector_count_includes_the_backup_boot_sector() {
        assert_eq!(volume_sectors(126_975, 1_000_000), 126_976);
        assert_eq!(volume_sectors(0, 1_000_000), 1);
        // A truncated image clamps instead of reading past the end.
        assert_eq!(volume_sectors(126_975, 126_975), 126_975);
        assert_eq!(volume_sectors(10, 4), 4);
    }

    #[test]
    fn corrupt_attribute_header_is_an_error_not_a_panic() {
        let mut rec = vec![0u8; 0x100];
        rec[0x14..0x16].copy_from_slice(&0xF8u16.to_le_bytes());
        rec[0xF8..0xFC].copy_from_slice(&0x80u32.to_le_bytes());
        rec[0xFC..0x100].copy_from_slice(&0x18u32.to_le_bytes());
        assert!(parse_attrs(&rec).is_err());
    }

    #[test]
    fn attribute_offset_outside_record_is_an_error() {
        let mut rec = vec![0u8; 0x40];
        rec[0x14..0x16].copy_from_slice(&0xFFFFu16.to_le_bytes());
        assert!(parse_attrs(&rec).is_err());
    }

    #[test]
    fn rejects_short_boot_sector() {
        let src = crate::io::tests::MemSource::new(vec![0u8; 1024]);
        let ctx = VolumeCtx {
            source: &src,
            offset: 0,
            max_len: 1024,
            partition_index: None,
            partition_type: None,
        };
        assert!(parse(&ctx, &[0u8; 100]).is_err());
    }
}
