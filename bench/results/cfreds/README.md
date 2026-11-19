# CFReDS RM#2 results

Results of running CarveX 0.1.0 on the NIST CFReDS Data Leakage Case RM#2 image
on 17/11/2026. Full instructions and discussion are in
[`docs/CFREDS.md`](../../../docs/CFREDS.md).

## Data

| Object | Size | SHA-1 | SHA-256 |
|---|---:|---|---|
| 7z archive | 229,899,285 B | `DDFE97AA3D8D0B33CC6092123090A8154945F38E` | `ade9fb60ba1f700b93c6b8b1f538c72000411e5b30037dc95c300c5a0aeafd65` |
| DD image | 4,004,511,744 B | `048961A85CA3ECED8CC73F1517442D31D4DCA0A3` | `d930afa23f8905e88a28041786edbe3791b5bf41b01468027ee27a19cd59d407` |

MD5 of the DD image: `B4644902ACAB4583A1D0F9F1A08FAA77`. SHA-1 and MD5 match the
values published by NIST.

The image has an MBR with one 1 GiB FAT32 partition at offset 65,536 bytes, label
`NO NAME`, 261,119/261,120 free clusters. The partition was quick-formatted;
about 2.7 GiB remains outside the partition.

## Results

| Mode | Scanned area | Time | Throughput | Candidates | Carve | Rejected | Nested | JPEG | PNG | PDF |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `unallocated` | 1,020 MiB | 1.609 s | 633.5 MiB/s | 429 | 318 | 73 | 38 | 113 | 205 | 0 |
| `raw --no-fs` | 3.73 GiB | 3.418 s | 1,117.2 MiB/s | 429 | 318 | 73 | 38 | 113 | 205 | 0 |

The two modes produced identical output. Total data written: 92,272,113 bytes.

## Checks

- 318/318 files `complete`, score 100, `--repair` not used.
- `file` correctly identifies 113 JPEG and 205 PNG; Pillow `verify()` succeeded
  for 318/318.
- The 318 files have 276 unique SHA-256 values; 42 duplicate groups, 2 copies each.
- 73 JPEG candidates rejected; 38 JPEGs nested in metadata blocked.
- At least 178/318 files are media stored in `word/media/` or `ppt/media/` of
  DOCX/PPTX. These are not 318 independent user files, and CFReDS has no per-file
  ground truth, so these results cannot be used to compute precision/recall.

## Files

- `cfreds-report.json`, `cfreds-report.csv`: unallocated scan reports.
- `cfreds-raw-report.json`: whole-image raw scan report.
- `cfreds-unallocated.log`, `cfreds-raw.log`: console logs.
- `SHA256SUMS`: checksums of the files in this directory.

Reproduction commands:

```bash
./scripts/fetch_cfreds.sh ~/cfreds-data

./dist/carvex-linux-x86_64 scan \
  ~/cfreds-data/'cfreds_2015_data_leakage_rm#2.dd' \
  -o ~/cfreds-data/recovered --mode unallocated --types jpeg,png,pdf \
  --json-report cfreds-report.json --csv-report cfreds-report.csv

./dist/carvex-linux-x86_64 scan \
  ~/cfreds-data/'cfreds_2015_data_leakage_rm#2.dd' \
  -o ~/cfreds-data/recovered-raw --mode raw --no-fs --types jpeg,png,pdf \
  --json-report cfreds-raw-report.json
```
