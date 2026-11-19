# CarveX benchmark results

Generated: `2026-11-17T22:33:47`

Images: **4**, deleted ground-truth files: **100**, kept files: **20**, repetitions: **3**.

Tool versions:
- CarveX: `carvex 0.1.0`
- Foremost: `1.5.7`
- Scalpel: `Scalpel version 1.60`
- PhotoRec: `PhotoRec 7.2, Data Recovery Utility, February 2024`

## Method

The benchmark compares byte-exact recovery and *usable* recovery. An output is an exact true positive only if its SHA-256 equals the original deleted file. For usable matching, bytes after the real footer are ignored: the last JPEG `FF D9`, the PNG IEND chunk, or the last PDF `%%EOF`. This compensates for tools that append a NUL byte or omit a final newline. Every deleted ground-truth file can be matched once; duplicate or extra outputs are false positives. In the mixed images, recovering a file that was never deleted is also a false positive.

CarveX unallocated scans only free clusters from the FAT or NTFS `$Bitmap`; the raw rows and the other tools scan the whole image. All runtime figures are wall-clock medians of the repeated runs.

## Usable results (normalized)

| Image | FS | Scenario | Tool | Space | Out | TP | FP | FN | Prec. | Rec. | F1 | Time (s) | MiB/s |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| all-deleted_fat32 | fat32 | all-deleted | CarveX | unallocated | 30 | 30 | 0 | 0 | 100.00% | 100.00% | 100.00% | 0.123 | 518.9 |
| all-deleted_fat32 | fat32 | all-deleted | CarveX | raw | 30 | 30 | 0 | 0 | 100.00% | 100.00% | 100.00% | 0.123 | 521.6 |
| all-deleted_fat32 | fat32 | all-deleted | Foremost | raw | 39 | 28 | 11 | 2 | 71.79% | 93.33% | 81.16% | 0.484 | 132.3 |
| all-deleted_fat32 | fat32 | all-deleted | Scalpel | raw | 41 | 28 | 13 | 2 | 68.29% | 93.33% | 78.87% | 0.423 | 151.2 |
| all-deleted_fat32 | fat32 | all-deleted | PhotoRec | raw | 28 | 28 | 0 | 2 | 100.00% | 93.33% | 96.55% | 0.524 | 122.2 |
| all-deleted_ntfs | ntfs | all-deleted | CarveX | unallocated | 30 | 30 | 0 | 0 | 100.00% | 100.00% | 100.00% | 0.178 | 358.8 |
| all-deleted_ntfs | ntfs | all-deleted | CarveX | raw | 30 | 30 | 0 | 0 | 100.00% | 100.00% | 100.00% | 0.170 | 375.4 |
| all-deleted_ntfs | ntfs | all-deleted | Foremost | raw | 39 | 28 | 11 | 2 | 71.79% | 93.33% | 81.16% | 0.375 | 170.5 |
| all-deleted_ntfs | ntfs | all-deleted | Scalpel | raw | 42 | 28 | 14 | 2 | 66.67% | 93.33% | 77.78% | 0.525 | 121.8 |
| all-deleted_ntfs | ntfs | all-deleted | PhotoRec | raw | 29 | 29 | 0 | 1 | 100.00% | 96.67% | 98.31% | 0.320 | 199.9 |
| mixed_fat32 | fat32 | mixed | CarveX | unallocated | 20 | 20 | 0 | 0 | 100.00% | 100.00% | 100.00% | 0.069 | 934.0 |
| mixed_fat32 | fat32 | mixed | CarveX | raw | 30 | 20 | 10 | 0 | 66.67% | 100.00% | 80.00% | 0.122 | 524.4 |
| mixed_fat32 | fat32 | mixed | Foremost | raw | 39 | 19 | 20 | 1 | 48.72% | 95.00% | 64.41% | 0.321 | 199.5 |
| mixed_fat32 | fat32 | mixed | Scalpel | raw | 41 | 18 | 23 | 2 | 43.90% | 90.00% | 59.02% | 0.422 | 151.5 |
| mixed_fat32 | fat32 | mixed | PhotoRec | raw | 28 | 18 | 10 | 2 | 64.29% | 90.00% | 75.00% | 0.373 | 171.4 |
| mixed_ntfs | ntfs | mixed | CarveX | unallocated | 20 | 20 | 0 | 0 | 100.00% | 100.00% | 100.00% | 0.073 | 874.5 |
| mixed_ntfs | ntfs | mixed | CarveX | raw | 30 | 20 | 10 | 0 | 66.67% | 100.00% | 80.00% | 0.119 | 536.0 |
| mixed_ntfs | ntfs | mixed | Foremost | raw | 39 | 19 | 20 | 1 | 48.72% | 95.00% | 64.41% | 0.320 | 200.0 |
| mixed_ntfs | ntfs | mixed | Scalpel | raw | 42 | 18 | 24 | 2 | 42.86% | 90.00% | 58.06% | 0.525 | 121.9 |
| mixed_ntfs | ntfs | mixed | PhotoRec | raw | 29 | 19 | 10 | 1 | 65.52% | 95.00% | 77.55% | 0.322 | 199.0 |

## Exact results (SHA-256)

| Image | FS | Scenario | Tool | Space | Out | TP | FP | FN | Prec. | Rec. | F1 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| all-deleted_fat32 | fat32 | all-deleted | CarveX | unallocated | 30 | 30 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| all-deleted_fat32 | fat32 | all-deleted | CarveX | raw | 30 | 30 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| all-deleted_fat32 | fat32 | all-deleted | Foremost | raw | 39 | 23 | 16 | 7 | 58.97% | 76.67% | 66.67% |
| all-deleted_fat32 | fat32 | all-deleted | Scalpel | raw | 41 | 24 | 17 | 6 | 58.54% | 80.00% | 67.61% |
| all-deleted_fat32 | fat32 | all-deleted | PhotoRec | raw | 28 | 28 | 0 | 2 | 100.00% | 93.33% | 96.55% |
| all-deleted_ntfs | ntfs | all-deleted | CarveX | unallocated | 30 | 30 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| all-deleted_ntfs | ntfs | all-deleted | CarveX | raw | 30 | 30 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| all-deleted_ntfs | ntfs | all-deleted | Foremost | raw | 39 | 23 | 16 | 7 | 58.97% | 76.67% | 66.67% |
| all-deleted_ntfs | ntfs | all-deleted | Scalpel | raw | 42 | 24 | 18 | 6 | 57.14% | 80.00% | 66.67% |
| all-deleted_ntfs | ntfs | all-deleted | PhotoRec | raw | 29 | 29 | 0 | 1 | 100.00% | 96.67% | 98.31% |
| mixed_fat32 | fat32 | mixed | CarveX | unallocated | 20 | 20 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| mixed_fat32 | fat32 | mixed | CarveX | raw | 30 | 20 | 10 | 0 | 66.67% | 100.00% | 80.00% |
| mixed_fat32 | fat32 | mixed | Foremost | raw | 39 | 16 | 23 | 4 | 41.03% | 80.00% | 54.24% |
| mixed_fat32 | fat32 | mixed | Scalpel | raw | 41 | 15 | 26 | 5 | 36.59% | 75.00% | 49.18% |
| mixed_fat32 | fat32 | mixed | PhotoRec | raw | 28 | 18 | 10 | 2 | 64.29% | 90.00% | 75.00% |
| mixed_ntfs | ntfs | mixed | CarveX | unallocated | 20 | 20 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| mixed_ntfs | ntfs | mixed | CarveX | raw | 30 | 20 | 10 | 0 | 66.67% | 100.00% | 80.00% |
| mixed_ntfs | ntfs | mixed | Foremost | raw | 39 | 16 | 23 | 4 | 41.03% | 80.00% | 54.24% |
| mixed_ntfs | ntfs | mixed | Scalpel | raw | 42 | 15 | 27 | 5 | 35.71% | 75.00% | 48.39% |
| mixed_ntfs | ntfs | mixed | PhotoRec | raw | 29 | 19 | 10 | 1 | 65.52% | 95.00% | 77.55% |

## Pooled by tool

| Tool | Space | Deleted | Out | TP | FP | FN | Prec. | Rec. | F1 | Total time (s) | MiB/s |
|---|---|---|---|---|---|---|---|---|---|---|---|
| CarveX | unallocated | 100 | 100 | 100 | 0 | 0 | 100.00% | 100.00% | 100.00% | 0.443 | 577.3 |
| CarveX | raw | 100 | 120 | 100 | 20 | 0 | 83.33% | 100.00% | 90.91% | 0.535 | 478.8 |
| Foremost | raw | 100 | 156 | 94 | 62 | 6 | 60.26% | 94.00% | 73.44% | 1.500 | 170.7 |
| Scalpel | raw | 100 | 166 | 92 | 74 | 8 | 55.42% | 92.00% | 69.17% | 1.896 | 135.0 |
| PhotoRec | raw | 100 | 114 | 94 | 20 | 6 | 82.46% | 94.00% | 87.85% | 1.539 | 166.4 |

### Pooled exact results

| Tool | Space | TP | FP | FN | Prec. | Rec. | F1 |
|---|---|---|---|---|---|---|---|
| CarveX | unallocated | 100 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| CarveX | raw | 100 | 20 | 0 | 83.33% | 100.00% | 90.91% |
| Foremost | raw | 78 | 78 | 22 | 50.00% | 78.00% | 60.94% |
| Scalpel | raw | 78 | 88 | 22 | 46.99% | 78.00% | 58.65% |
| PhotoRec | raw | 94 | 20 | 6 | 82.46% | 94.00% | 87.85% |

## Commands

```text
carvex scan IMAGE -o OUT --mode unallocated --types jpeg,png,pdf --min-score 50 --quiet
carvex scan IMAGE -o OUT --mode raw --no-fs --types jpeg,png,pdf --min-score 50 --quiet
foremost -Q -c bench/foremost.conf -t jpg,png,pdf -i IMAGE -o OUT
scalpel -c bench/scalpel.conf -o OUT IMAGE
photorec /log /d OUT /cmd IMAGE partition_i386,1,fileopt,everything,disable,jpg,enable,png,enable,pdf,enable,search
```

## Notes

- `Out` counts every recognized target file in the tool output, including duplicates and invalid files with a target extension.
- `kept_outputs` (in the CSV/JSON) separates false positives caused by recovering allocated files from other false positives.
- Foremost's built-in signatures are used (its packaged config file has no enabled types). Adding custom JPEG/PDF entries to the config registers a second signature and produces duplicate carvings.
- The 64 MiB images are small, warm-cache benchmark media. Runtime is reported for comparison, not as a claim about large disks.

