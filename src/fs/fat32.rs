#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
use super::{FsInfo, FsKind, VolumeCtx};
use crate::util::Extent;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

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

pub fn parse(ctx: &VolumeCtx<'_>, boot: &[u8]) -> Result<FsInfo> {
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
    let available_sectors = ctx.max_len / bps as u64;
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
    let fat_offset = ctx
        .offset
        .checked_add(reserved as u64 * bps as u64)
        .context("FAT32: FAT offset overflow")?;
    let fat = ctx
        .read_vec(fat_offset, fat_bytes_needed as usize)
        .with_context(|| format!("FAT32: cannot read FAT at byte {fat_offset}"))?;

    let data_start_byte = ctx
        .offset
        .checked_add(data_start_sector * bps as u64)
        .context("FAT32: data offset overflow")?;
    let volume_len = total_sectors * bps as u64;
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
        let free = raw == 0;
        if free {
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

    // Clamp to the image (paranoia around a malformed total-sector field).
    let vol_end = ctx.offset.saturating_add(volume_len);
    for e in &mut extents {
        let end = e.end().min(vol_end);
        e.len = end.saturating_sub(e.offset);
    }
    extents.retain(|e| e.len > 0);
    let extents = crate::util::merge_extents(extents);

    let label = String::from_utf8_lossy(boot.get(0x47..0x52).unwrap_or(&[]))
        .trim_end_matches([' ', '\0'])
        .to_string();
    let mut details = BTreeMap::new();
    details.insert("total_sectors".into(), total_sectors.to_string());
    details.insert("reserved_sectors".into(), reserved.to_string());
    details.insert("fat_count".into(), num_fats.to_string());
    details.insert("fat_size_sectors".into(), fat_size.to_string());
    details.insert("data_start_sector".into(), data_start_sector.to_string());
    details.insert("root_cluster".into(), root_cluster.to_string());
    details.insert("bad_clusters".into(), bad_clusters.to_string());
    details.insert("volume_serial".into(), format!("{serial:08X}"));
    if total_from_boot > available_sectors {
        details.insert(
            "warning".into(),
            format!("total_sectors clamped from {total_from_boot} to {total_sectors}"),
        );
    }

    Ok(FsInfo {
        kind: FsKind::Fat32,
        offset: ctx.offset,
        length: volume_len,
        bytes_per_sector: bps,
        sectors_per_cluster: spc,
        cluster_size,
        data_start: data_start_byte,
        total_clusters,
        free_clusters,
        free_extents: extents,
        partition_index: ctx.partition_index,
        partition_type: ctx.partition_type.clone(),
        label: Some(label),
        details,
    })
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
