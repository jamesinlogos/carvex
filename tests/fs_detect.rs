//! Filesystem detection on synthetic FAT32 volumes and MBR layouts.

mod common;

use carvex::fs::{self, FsKind, PartitionSel, RegionKind};
use carvex::io::ImageSource;
use common::{fat32_volume, fat32_volume_sectors, mbr_image, MemSource};

#[test]
fn detects_fat32_superfloppy() {
    let image = fat32_volume();
    let src = MemSource::new(image);
    let info = fs::detect(&src, src.len(), None, None).unwrap();
    assert_eq!(info.kind, FsKind::Fat32);
    assert_eq!(info.offset, 0);
    assert_eq!(info.bytes_per_sector, 512);
    assert_eq!(info.cluster_size, 512);
    assert_eq!(info.partition_index, None);
    assert_eq!(info.label.as_deref(), Some("CARVEX"));
    // Cluster 2 is allocated, so the free area starts at cluster 3.
    assert_eq!(info.free_clusters, info.total_clusters - 1);
    assert!(!info.free_extents.is_empty());
    assert_eq!(info.free_extents[0].offset, 10_240); // data start + cluster 2
}

#[test]
fn detects_fat32_inside_mbr_partition() {
    let volume = fat32_volume();
    let image = mbr_image(&[(1, 0x0C, &volume)]);
    let src = MemSource::new(image);
    let info = fs::detect(&src, src.len(), None, None).unwrap();
    assert_eq!(info.kind, FsKind::Fat32);
    assert_eq!(info.offset, 512);
    assert_eq!(info.partition_index, Some(1));
    assert_eq!(info.partition_type.as_deref(), Some("FAT32 LBA"));
}

#[test]
fn partition_override_selects_the_requested_volume() {
    let small = fat32_volume_sectors(2048);
    let large = fat32_volume_sectors(4096);
    let image = mbr_image(&[(1, 0x0C, &small), (4096, 0x0C, &large)]);
    let src = MemSource::new(image);
    let len = src.len();

    // Historical default: the largest supported volume wins.
    let auto = fs::detect(&src, len, None, None).unwrap();
    assert_eq!(auto.offset, 4096 * 512);
    assert_eq!(auto.partition_index, Some(2));

    let first = fs::detect(&src, len, None, Some(1)).unwrap();
    assert_eq!(first.offset, 512);
    assert_eq!(first.partition_index, Some(1));

    assert!(fs::detect(&src, len, None, Some(9)).is_err());
}

#[test]
fn rejects_images_without_a_supported_filesystem() {
    let src = MemSource::new(vec![0u8; 64 * 1024]);
    let err = fs::detect(&src, src.len(), None, None).unwrap_err();
    assert!(format!("{err:#}").contains("no FAT32/NTFS filesystem"));
    // The guarded wrapper returns an error rather than aborting the process.
    assert!(fs::detect_guarded(&src, src.len(), None, None).is_err());
}

#[test]
fn malformed_fat32_is_an_error_not_a_panic() {
    let mut image = fat32_volume();
    // Break the sectors-per-cluster field and the FAT size.
    image[0x0D] = 0;
    image[0x24..0x28].copy_from_slice(&0u32.to_le_bytes());
    let src = MemSource::new(image);
    assert!(fs::detect_guarded(&src, src.len(), None, None).is_err());
}

#[test]
fn forced_offset_outside_the_image_is_rejected() {
    let src = MemSource::new(fat32_volume());
    let err = fs::detect(&src, src.len(), Some(src.len()), None).unwrap_err();
    assert!(format!("{err:#}").contains("outside the image"));
}

/// A-07: `detect_all` reports every volume, the gaps between partitions and
/// the partitions whose filesystem is unsupported.
#[test]
fn detect_all_reports_volumes_gaps_and_unsupported_partitions() {
    let big = fat32_volume_sectors(4096);
    let small = fat32_volume_sectors(2048);
    let linux = [0u8; 512];
    let image = mbr_image(&[
        (2048, 0x0C, &big),
        (2048 + 4096, 0x0C, &small),
        (2048 + 4096 + 2048, 0x83, &linux),
    ]);
    let src = MemSource::new(image);
    let set = fs::detect_all(&src, src.len()).unwrap();

    assert_eq!(set.volumes.len(), 2);
    assert_eq!(set.volumes[0].partition_index, Some(1));
    assert_eq!(set.volumes[1].partition_index, Some(2));
    assert_eq!(set.unsupported.len(), 1);
    assert_eq!(set.unsupported[0].index, 3);
    assert!(!set.superfloppy);

    // A 1 MiB leading gap before the first partition and nothing else: the
    // partitions are contiguous with the image end.
    assert_eq!(set.gaps.len(), 1);
    assert_eq!(set.gaps[0].offset, 0);
    assert_eq!(set.gaps[0].len, 2048 * 512);

    // Selection semantics (historical default = largest volume).
    let largest = fs::select_volumes(&set, PartitionSel::Largest).unwrap();
    assert_eq!(largest.len(), 1);
    assert_eq!(largest[0].partition_index, Some(1));
    let all = fs::select_volumes(&set, PartitionSel::All).unwrap();
    assert_eq!(all.len(), 2);
    let second = fs::select_volumes(&set, PartitionSel::Index(2)).unwrap();
    assert_eq!(second[0].partition_index, Some(2));
    let missing = fs::select_volumes(&set, PartitionSel::Index(9)).unwrap_err();
    assert!(format!("{missing:#}").contains("partition 9 not found"));

    // `regions[]` covers every range of the image exactly once per kind.
    let regions = set.regions(&all, false, true, false, false);
    let gap = regions
        .iter()
        .filter(|r| r.kind == RegionKind::Gap)
        .collect::<Vec<_>>();
    assert_eq!(gap.len(), 1);
    assert!(!gap[0].scanned, "a plain scan does not read the gap");
    let unsupported = regions
        .iter()
        .filter(|r| r.kind == RegionKind::Unsupported)
        .collect::<Vec<_>>();
    assert_eq!(unsupported.len(), 1);
    assert!(!unsupported[0].scanned);

    // `--include-gaps` marks gaps and unsupported partitions as scanned.
    let with_gaps = set.regions(&all, false, true, false, true);
    assert!(with_gaps
        .iter()
        .filter(|r| r.kind == RegionKind::Gap || r.kind == RegionKind::Unsupported)
        .all(|r| r.scanned));
    let extents = set.unscanned_extents();
    assert_eq!(extents.len(), 2);

    // A raw scan reads everything.
    let raw = set.regions(&all, true, false, false, false);
    assert!(raw.iter().all(|r| r.scanned));
}

/// A-07: a superfloppy image is a single volume with no partition structure,
/// so no gap warnings can exist and every selection returns that volume.
#[test]
fn detect_all_on_a_superfloppy_has_no_gaps() {
    let src = MemSource::new(fat32_volume());
    let set = fs::detect_all(&src, src.len()).unwrap();
    assert!(set.superfloppy);
    assert_eq!(set.volumes.len(), 1);
    assert!(set.gaps.is_empty());
    assert!(set.unsupported.is_empty());
    for sel in [
        PartitionSel::Largest,
        PartitionSel::All,
        PartitionSel::Index(1),
    ] {
        let picked = fs::select_volumes(&set, sel).unwrap();
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].partition_index, None);
    }
}
