# CarveX

**CarveX** is a Rust command-line tool for recovering **JPEG, PNG, and PDF** files from a disk image or partition image. It combines:

- **Signature-based carving** (header/footer) with full **structural validation** rather than merely searching for an end byte;
- **FAT32/NTFS filesystem detection** to scan only **unallocated** space, greatly reducing false positives;
- Parallel scanning per extent, suitable for large disk images;
- JSON/CSV reports and an option to repair missing footers (`--repair`);
- Bundled static Linux and Windows x86_64 binaries.

> **Forensic note:** CarveX only opens images in read-only mode. Always work on a forensic copy and verify the image hash before/after analysis.

Detailed technical report (theoretical basis, design, experiments, limitations): [`docs/REPORT.md`](docs/REPORT.md).

## Table of contents

- [Features](#features)
- [Quick start](#quick-start)
- [Usage](#usage)
- [How it works](#how-it-works)
- [Output format](#output-format)
- [Benchmark](#benchmark)
- [CFReDS dataset](#cfreds-dataset)
- [Limitations and notes](#limitations-and-notes)
- [Project structure](#project-structure)
- [Technical report](docs/REPORT.md)

## Features

| Area | Details |
|---|---|
| Formats | JPEG (baseline, progressive, EXIF thumbnail), PNG (all valid color types, CRC-32), PDF (xref table, incremental update, xref stream detection) |
| Filesystems | FAT32 (reads the FAT table), NTFS (reads `$MFT` and `$Bitmap`), MBR/GPT, superfloppy |
| Scan modes | `auto`, `unallocated`, `raw`, `allocated` |
| Structural validation | JPEG marker/segment walk; PNG chunk walk + CRC; PDF object/xref/`startxref`/`%%EOF` |
| Noise resistance | Structural score threshold, minimum/maximum size, nested file removal, SHA-256 dedupe (optional) |
| Output | Filenames include the offset, JSON/CSV reports, statistics by type |
| Operation | Parallel scanning, per-extent read limits, no writes to the source image |

## Quick start

### 1. Build from source

Requires Rust 1.85 or newer (MSRV).

```bash
cd carvex
cargo build --release
./target/release/carvex --help
cargo test
```

### 2. Inspect a disk image

```bash
./target/release/carvex info /evidence/disk.img
```

Example with a test image:

```text
Image          : bench/images/all-deleted_fat32.img
Filesystem     : FAT32 (62.0 MiB)
Partition      : #1 (FAT32 LBA) at byte 1048576
Volume label   : CARVEXFAT
Geometry       : 512 B/sector x 1 sectors/cluster = 512 B/cluster
Clusters       : 124990
Free clusters  : 124982 (100.0%, 61.0 MiB)
Free extents   : 7 (largest 48.7 MiB)
```

### 3. Recover deleted files

```bash
./target/release/carvex scan /evidence/disk.img \
  -o recovered \
  --mode unallocated \
  --types jpeg,png,pdf \
  --json-report recovered-report.json \
  --csv-report recovered-report.csv
```

If `--mode` is not specified, the default mode is `auto`: use `unallocated` when FAT32/NTFS is detected, otherwise scan the whole image with `raw`.

### 4. Use the bundled binaries

```bash
cd dist
sha256sum -c SHA256SUMS
./carvex-linux-x86_64 --version
```

| File | Platform | Notes |
|---|---|---|
| `dist/carvex-linux-x86_64` | Linux x86_64 | Static PIE (musl), stripped |
| `dist/carvex-windows-x86_64.exe` | Windows x86_64 | PE32+, GNU toolchain, stripped |
| `dist/SHA256SUMS` | — | SHA-256 checksums |

The Windows binary has not been tested on a real Windows machine during development; the core shares source code with the Linux build.

## Usage

### `carvex info`

```text
carvex info IMAGE [--input-format FMT] [--partition N | --offset BYTES] [--json]
```

Displays the filesystem type, partition offset, cluster geometry, number of free clusters, free extents, and BPB/boot sector information. For images with a partition table, `info` also lists all volumes and image regions (`volumes[]`, `regions[]` in `--json`); `--partition N` views only a specific partition.

### `carvex scan`

```text
carvex scan IMAGE -o OUT [OPTIONS]
```

Main options:

<!-- BEGIN GENERATED SCAN FLAG TABLE -->
| Option | Default | Meaning |
|---|---|---|
| `--input-format <INPUT_FORMAT>` | `auto` | Image container: auto, raw, split, e01, vhd, vmdk or stdin |
| `--allow-external-extents` | `off` | Allow VMDK extents outside the descriptor directory (unsafe) |
| `--on-read-error <ON_READ_ERROR>` | `zero-fill` | What to do when a byte range cannot be read (default: zero-fill) |
| `--max-stdin-bytes <MAX_STDIN_BYTES>` | `0` | Maximum bytes spooled from stdin (0 = unlimited) |
| `--spool-dir <SPOOL_DIR>` | `off` | Directory for the stdin spool file |
| `--output <OUTPUT>` | `required` | Output directory for carved files (must be empty unless --force) |
| `--types <TYPES>` | `jpeg,png,pdf` | Comma-separated types: jpeg,jpg,png,pdf |
| `--mode <MODE>` | `auto` | Space to scan: auto, unallocated, raw, allocated |
| `--min-size <MIN_SIZE>` | `256` | Minimum carved file size (bytes, suffix K/M/G allowed) |
| `--max-size <MAX_SIZE>` | `64M` | Maximum bytes to inspect after a header before giving up |
| `--max-truncated-size <MAX_TRUNCATED_SIZE>` | `16M` | Maximum size of a truncated file written without --repair (0 = unlimited) |
| `--max-memory <MAX_MEMORY>` | `1G` | Maximum concurrent working memory for carve, join and write buffers; carved output is streamed from the source, not retained in RAM |
| `--max-validator-memory <MAX_VALIDATOR_MEMORY>` | `256M` | Maximum working memory for one decode validator (0 = default) (alias: `--max-decode-bytes`) |
| `--max-candidates <MAX_CANDIDATES>` | `1000000` | Deterministic limit on header candidates evaluated (0 = default limit) |
| `--max-work-per-candidate <MAX_WORK_PER_CANDIDATE>` | `256M` | Deterministic analyzer + validator byte budget for one candidate (0 = unlimited) |
| `--max-output <MAX_OUTPUT>` | `4G` | Cap on bytes written by one scan (0 = unlimited) |
| `--time-budget <TIME_BUDGET>` | `0` | Overall scan wall-clock budget in seconds (0 = unlimited); stops with a partial report |
| `--chunk-size <CHUNK_SIZE>` | `4M` | Read chunk size for parallel scanning |
| `--threads <THREADS>` | `0` | Worker threads (0 = number of CPUs) |
| `--min-score <MIN_SCORE>` | `50` | Minimum structural validation score (0-100) |
| `--repair` | `off` | Rebuild a truncated footer (PDF xref/trailer, JPEG EOI, PNG IEND) and revalidate before labelling repaired |
| `--complete-only` | `off` | Only write files whose footer was found, not verified (see validity) |
| `--pdf-eof <PDF_EOF>` | `smart` | PDF %%EOF selection strategy |
| `--dedupe-hashes` | `off` | Drop later files with an identical SHA-256 |
| `--keep-nested` | `off` | Keep images/PDFs embedded inside a complete carved container |
| `--validate <VALIDATE>` | `structure` | Validate structure only, or also decode the payload |
| `--align <ALIGN>` | `none` | Candidate alignment: none (historical), auto, cluster, sector |
| `--force` | `off` | Allow writing into a non-empty output directory |
| `--manifest` | `off` | Write SHA256SUMS for the carved files |
| `--partition <PARTITION>` | `all` | Select partitions: 'all' (default), 'largest' (historical) or a 1-based number |
| `--include-gaps` | `on` | Scan image regions outside every supported volume (gaps and partitions without a supported filesystem); on by default |
| `--no-gaps` | `off` | Restore the historical behaviour: do not scan gaps or unsupported partitions (use with `--partition largest`) |
| `--max-regions <MAX_REGIONS>` | `4096` | Maximum detailed `regions[]` rows before adjacent rows of the same scanned state are summarised (0 = unlimited; H-27) |
| `--fragment-recovery` | `off` | Opt in to bounded bifragment gap carving (B-06) |
| `--max-gap <MAX_GAP>` | `16` | Maximum gap between joined fragments, in clusters (B-06) |
| `--max-frag-candidates <MAX_FRAG_CANDIDATES>` | `1024` | Maximum candidate joins evaluated by B-06 |
| `--frag-time-limit <FRAG_TIME_LIMIT>` | `5000` | Wall-clock budget for B-06, in milliseconds |
| `--offset <OFFSET>` | `off` | Force the volume boot-sector offset (bytes, suffix K/M/G allowed) |
| `--no-fs` | `off` | Skip filesystem detection (raw scanning only) |
| `--hash-image` | `off` | Hash the complete source image (streaming, can be slow) |
| `--case-id <CASE_ID>` | `off` | Case identifier stored in the report |
| `--examiner <EXAMINER>` | `off` | Examiner name stored in the report |
| `--json-report <JSON_REPORT>` | `off` | Write a machine-readable JSON report (schema 1 or 2) |
| `--csv-report <CSV_REPORT>` | `off` | Write a CSV report |
| `--report-schema <REPORT_SCHEMA>` | `2` | JSON report schema: 2 (current) or 1 (legacy shape) |
| `--quiet` | `off` | Suppress progress and summary output |
| `--verbose` | `off` | Print every candidate/rejection decision |
<!-- END GENERATED SCAN FLAG TABLE -->

### Evidence image sources

`IMAGE` may be a raw image, concatenated raw segments (`.001`, `.002`, …), a
classic EWF-E01 segment set (`.E01`, `.E02`, …), a fixed or dynamic VHD, a VMDK
descriptor with flat or sparse extents, or `-` for stdin. Auto-detection uses
magic bytes and segment naming; pass `--input-format raw|split|e01|vhd|vmdk` to
force a container. Stdin is spooled to a temporary file because the scanner
needs random access; the spool is removed when the command exits.

Recognized EWF2/Ex01, Lx01, LVF/L01, differencing VHD, ESX/COWD and compressed
streamOptimized VMDK variants are rejected with an explicit error rather than
returning partial data. Encrypted E01 is not supported; there is no decryption,
and no claim is made that every encrypted variant is detected before parsing.
E01 segment discovery handles the usual two-digit base-36 suffixes (up to
`.EZZ`, 1295 segments). The dynamic VHD reader uses the block allocation table
but not the per-sector presence bitmap. Ordinary raw images keep the unchanged
`RawFile` path, so the frozen benchmark behavior is unaffected.

### Bounded fragment recovery (B-06)

`--fragment-recovery` enables an opt-in two-fragment gap-carving stage. Inside a
detected FAT32/NTFS volume it joins a header fragment to a later
cluster-aligned tail fragment. A contiguous, gap-free carve is left alone; a
truncated carve, a carve with filler inside it, or a PDF carve containing more
than one footer is probed with the deep validator. Skipped clusters must look
like filler or another file's header, joins with the best-explained gaps are
tried first, and a stitch is written only when its authoritative `validity` is
`verified` (the earlier lexical PDF exception is no longer accepted here).

`--max-frag-candidates` is the authoritative budget: each head probe and each
join attempt consumes one work unit, and reaching it emits
`FRAGMENT_CANDIDATE_LIMIT` and exits 3 deterministically. `--frag-time-limit` is
only a wall-clock safety net; reaching it emits `FRAGMENT_TIME_LIMIT`, sets
`nondeterministic_stop: true` and also exits 3. Both conditions write a partial
report. Successful records use `source="bgc"` and report `fragments[]`,
`checks`, `confidence` and the additive `alternatives` count (JSON and CSV);
these JSON fields are omitted when the stage is disabled. Limits without
`--fragment-recovery` are a runtime error.

The D1-style development profile in `bench/results/d1_fragment_profile.json`
measured exact recall 4/6 = 66.7% (Wilson 95% [30.0%; 90.3%]), splice rate
0/4 = 0% (Wilson 95% [0%; 49.0%]), and junk-FP 0/24 = 0% (Wilson 95%
[0%; 13.8%]); the X15 realistic fixture recalled 3/3 files, and `--threads 1`
produced identical outputs. The stage is not a runlist/metadata reassembler,
supports at most two fragments, remains opt-in, and is excluded from the
published benchmark figures.

### Multi-volume scanning, warnings, and exit codes

For images with a partition table, CarveX 0.2.0 does not skip any volume or image region without reporting it (A-07):

- By default (A-07 step 2), scan the free space of **all** FAT32/NTFS volumes and raw-scan every gap/unsupported partition; no region is skipped, so exit 0.
- To restore the historical 0.1.0 behavior, use `--partition largest --no-gaps`: every unscanned volume/gap is reported with a **coded warning** on stderr and in `warnings[]` (`VOLUME_NOT_SCANNED`, `GAP_NOT_SCANNED`, `VOLUME_UNSUPPORTED`, `FS_DETECTION_FAILED`, `IMAGE_MODIFIED`), and the program exits 3.
- A recognised but unparsable partition always emits `VOLUME_PARSE_FAILED` with its index, byte offset and underlying reason, even when another volume parses. The scan exits 3 but is not aborted (H-26).
- To skip only gaps but still scan all volumes: `--no-gaps` (can be used with `--partition all`). `--include-gaps` remains accepted for compatibility with old scripts.
- Report schema 2 always lists `volumes[]` in full (which volumes were/were not scanned) and `regions[]` (`free`/`allocated`/`gap`/`unsupported`/`mixed` with a `scanned` flag). Each region also carries `may_contain_allocated`; it is true for an `unsupported` fallback region where a filesystem parse failed, so readers know raw carving mixes allocated and deleted bytes (H-26).
- `regions[]` is a coverage map built from the extents the scanner actually reads: real merged free extents, the allocated complement, gaps and unsupported partitions. Rows are sorted, disjoint and cover the whole image; scanned rows sum exactly to `bytes_scanned`. Each row adds `extent_id` and `summarized`, and `info --json` uses the same free-extent map (H-27). `--max-regions` summarises adjacent rows with the same `scanned` state without changing coverage or totals.

| Exit code | Meaning |
|---|---|
| 0 | Success, no region skipped |
| 1 | Runtime error (no filesystem when required, I/O, …) |
| 2 | Syntax error (clap) |
| 3 | **Finished but incomplete** — there are "not fully scanned" warnings (see above) |
| 4 | Interrupted mid-run: the first SIGINT/Ctrl-C requests a cooperative stop, a schema-2 partial report is written with `interrupted: true` and `INTERRUPTED`, and already-renamed outputs remain (H-28). A second signal exits 130 immediately |

### Examples

Scan the whole image, including allocated space, for comparison:

```bash
carvex scan disk.img -o out-raw --mode raw --no-fs
```

Scan only allocated space:

```bash
carvex scan disk.img -o out-allocated --mode allocated --complete-only
```

Select partition 2 and repair truncated file footers:

```bash
carvex scan disk.img -o out --partition 2 --repair
```

Scanning all volumes and gaps is the default behavior; the command below only states intent explicitly (equivalent to omitting the flag):

```bash
carvex scan multi.img -o out --partition all --include-gaps
```

Restore 0.1.0 results (largest volume only, skip gaps, exit 3 with warnings):

```bash
carvex scan multi.img -o out --partition largest --no-gaps
```

Keep images embedded in PDFs (by default CarveX removes them to avoid duplicate output):

```bash
carvex scan disk.img -o out --keep-nested
```

List signatures and validators:

```bash
carvex signatures
```

## How it works

### 1. Filesystem detection and unallocated space

- **FAT32:** read the BPB and FAT table, mark clusters with entry `0` as free, and merge consecutive free clusters into physical extents. FAT32 uses only the first FAT copy; bad clusters (`0x0FFFFFF7`) are skipped.
- **NTFS:** read the boot sector, MFT record 0, and the `$MFT` runlist, apply update sequence fixups, read record 6 (`$Bitmap`), then take clusters with bit `0` as free space.
- **Partitioning:** try superfloppy at offset 0; if not present, read the MBR (and GPT if there is a protective MBR). By default, select all FAT32/NTFS volumes plus all gaps; use `--partition largest|all|N`, `--no-gaps`, or `--offset` to change the selection. Volumes/gaps that are not scanned (when the user deliberately skips them) are always reported (`volumes[]`, `regions[]`, `warnings[]`) rather than silently omitted.
- `--mode raw` ignores the filesystem and scans the whole image; `--mode allocated` scans the complement of free extents.

### 2. Parallel scanning

The image is split into tasks by `--chunk-size`, with each task reading an extra 16 boundary bytes so no header straddling a boundary is missed. `memchr::memmem` finds JPEG/PNG/PDF headers in each chunk; results are merged, sorted by offset, and duplicate candidates are removed.

Each candidate is bounded by the **end of the extent** containing it, so CarveX does not read into another allocated region. This is the core difference from a raw-scanning carver.

### 3. Structural validation and scoring

| Format | Checks | Score |
|---|---|---|
| JPEG | Walk markers/segments in order; confirm SOF, dimensions, SOS, entropy data, EOI; handle marker stuffing, restart markers, multi-scan progressive; ignore fake EOI inside EXIF thumbnails | 100 if complete; 60 if SOF+SOS present but truncated; lower if structure is missing |
| PNG | Walk each chunk; require IHDR first, check bit depth/color type, PLTE for indexed images, IDAT, IEND; compute CRC-32 for each chunk | 100 minus 15 per bad CRC (max 45); 60 if IHDR+IDAT present but truncated |
| PDF | Check version, object headers, `/Root`, `startxref`, xref/object targets, choose a revision; do not swallow the next `%PDF-` | 50 base + 10 per object + 20 for a valid startxref + 10 for `/Root` + 10 for xref |

Candidates scoring below `--min-score` are discarded. This eliminates most accidental `FF D8 FF` or `%PDF-` sequences in free space.

### 4. Footer selection and truncated-file repair

- JPEG: the real EOI after walking through entropy data, not the thumbnail's EOI.
- PNG: an IEND chunk with length 0 and a correct CRC.
- PDF: the nearest `%%EOF` whose `startxref` points to an xref/object; `smart` prefers the last valid revision. For PDFs with multiple revisions, `--pdf-eof first/last` allows changing the strategy.

When `--repair` is enabled, CarveX reconstructs a candidate suffix: `FF D9` for
JPEG, a real IEND chunk for PNG, or a bounded classic xref table and trailer for
PDF. The result is marked `repaired` only if it passes revalidation; otherwise
the original bytes are written with `status=truncated`, `validity=partial` and
the note `repair attempted, not validated`. A successful repair records
`sha256_original` (the bytes read) alongside `sha256` (the bytes written), and
never counts as an exact recovery. The official benchmark does **not** enable
this option.

### 5. Nested file removal

A complete PDF may contain embedded JPEG/PNG images or PDFs. If left unhandled, the carver creates extra "fake" files for the embedded content. By default CarveX removes candidates located entirely within a larger **complete** container. Use `--keep-nested` if you need to keep them.

## Output format

Files are named in the form:

```text
00001_jpg_00000000001f8800.jpg
```

consisting of a sequence number, type, absolute offset in hex, and extension. Sequence numbers may have gaps after nested files are removed; this is normal.

The JSON report (schema 2, default) has these main fields:

- `schema_version`, `tool`, `tool_info` (version, commit, rustc), `argv`, `options` (all effective parameters), `case_id`, `examiner`, `started_at`/`started_at_unix`;
- `image`, `image_size`, `filesystem`, `mode`, `duration_ms`, `throughput_mib_s`;
- `input`: legacy `size`/`size_after` plus additive schema-2 `logical_size`,
  `logical_size_after`, `physical_size`, `physical_size_after`, resolved
  `format`, and backing `files[]` (`path`, `size`, `modified_unix`). `unchanged`
  is a boolean for disk-backed images and `null` for stdin;
- `stats`: candidate count, carved file count, filtering reasons, extent count, bytes scanned;
- `volumes[]` and `regions[]`: every detected volume/partition/gap as a disjoint full-image coverage map, with `scanned`, the additive `may_contain_allocated` flag (A-07/H-26) and the additive `extent_id`/`summarized` fields (H-27);
- `warnings[]`: a list of `{code, message}`; `*_NOT_SCANNED`-style codes correspond to exit code 3, `READ_ABORTED` accompanies an exit-1 partial report, and `INTERRUPTED` accompanies an exit-4 cooperative stop (H-28);
- `interrupted`: additive schema-2 boolean, true when SIGINT/Ctrl-C requested the stop; removed by `--report-schema 1` (H-28);
- `unreadable_regions[]`: `{offset, len, reason}` for each failed source range (H-22);
- `files[]`: `seq`, `type`, `offset`, `size`, `carved_size`, `sha256`, `status` (`complete`/`truncated`/`repaired`), `score`, `width`, `height`, `notes`, `path`, `validation`, `validity`, `checks[]`, and optional `sha256_original` after a revalidated repair.

`--report-schema 1` emits the legacy 0.1.0 format (omitting v2 fields, including `sha256_original`). The published JSON Schema is at [`docs/report.schema.json`](docs/report.schema.json).

The CSV report has one row per file, suitable for import into a spreadsheet.

## Benchmark

### Setup

- **Corpus:** 30 synthetic files (10 JPEG, 10 PNG, 10 PDF) with realistic variants: baseline/progressive/EXIF, grayscale/RGBA/palette/interlaced, multi-page/incremental-update/scanned PDFs.
- **Test images:** four 64 MiB images, MBR, partition starting at 1 MiB:
  - `all-deleted_fat32`, `all-deleted_ntfs`: 30/30 files deleted;
  - `mixed_fat32`, `mixed_ntfs`: 20 files deleted, 10 kept.
- **Comparison tools:** Foremost 1.5.7, Scalpel 1.60, PhotoRec 7.2.
- **Repetitions:** 3 runs per configuration; times are medians, measured on a 4-CPU machine with a warm cache.
- **CarveX configuration:** `--min-score 50`, no `--repair`, `unallocated` and `raw` scans; other tools scan the whole image.

### Metric definitions

- **Exact:** the recovered file's SHA-256 matches the original file exactly.
- **Usable:** drop the bytes after the true footer (final JPEG EOI, PNG IEND, final PDF `%%EOF`) and then compare SHA-256. This is fair to tools that append NUL/newline bytes.
- **TP/FP/FN:** matched one-to-one; each original file counts as at most one TP. Surplus duplicates, recovered non-deleted files, and extraneous output are all FPs. An unmatched original file is an FN.

### Pooled results (4 images, 100 deleted files)

> **Data source:** the tables below come from the **0.1.0** benchmark run
> (`bench/results/final/results.json`, frozen — see `docs/DECISIONS.md`
> D-16). The 0.2.0 carving behavior was verified byte-for-byte against the
> published ground truth on all 4 images (100/100 files matching SHA-256);
> the 0.2.0 improvements are additive (report/CLI), so the old figures remain valid.

**Usable:**

| Tool | Space | Out | TP | FP | FN | Precision | Recall | F1 |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| CarveX | unallocated | 100 | 100 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| CarveX | raw | 120 | 100 | 20 | 0 | 83.33% | 100.00% | 90.91% |
| Foremost | raw | 156 | 94 | 62 | 6 | 60.26% | 94.00% | 73.44% |
| Scalpel | raw | 166 | 92 | 74 | 8 | 55.42% | 92.00% | 69.17% |
| PhotoRec | raw | 114 | 94 | 20 | 6 | 82.46% | 94.00% | 87.85% |

**Exact:**

| Tool | Space | TP | FP | FN | Precision | Recall | F1 |
|---|---|---:|---:|---:|---:|---:|---:|
| CarveX | unallocated | 100 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| CarveX | raw | 100 | 20 | 0 | 83.33% | 100.00% | 90.91% |
| Foremost | raw | 78 | 78 | 22 | 50.00% | 78.00% | 60.94% |
| Scalpel | raw | 78 | 88 | 22 | 46.99% | 78.00% | 58.65% |
| PhotoRec | raw | 94 | 20 | 6 | 82.46% | 94.00% | 87.85% |

**Pooled time (256 MiB of images; median of 3 repetitions per image, then summed across 4 images):**

| Tool | Total time (s) | Image throughput (MiB/s) |
|---|---:|---:|
| CarveX unallocated | 0.443 | 577.3 |
| CarveX raw | 0.535 | 478.8 |
| Foremost | 1.500 | 170.7 |
| Scalpel | 1.896 | 135.0 |
| PhotoRec | 1.539 | 166.4 |

### Key observations

- On this corpus, CarveX `unallocated` achieves **100/100 exact (0 FP)** on all 4 images, including the mixed images — this is 100 scoring points on 30 unique files (30 × 4 contexts); the 95% Wilson confidence interval for 100/100 is [96.3%; 100%] and for 30/30 is [88.6%; 100%], valid only under these corpus conditions (contiguous files, clean free space). Scanning only free clusters completely eliminates the 20 kept files.
- CarveX `raw` still achieves 100% recall but produces exactly 20 FPs, corresponding to the 20 kept files in the two mixed images. This is quantitative evidence of the benefit of the filesystem-aware approach.
- Foremost and Scalpel have high usable recall but many FPs and drop sharply on exact. For PDFs, the exact-vs-usable gap is due to padding bytes after `%%EOF`; the `unknown_fp` portion (44 for Foremost, 54 for Scalpel — re-verified by re-running the commands) is mainly **JPEGs embedded in the corpus PDFs** and thumbnail sub-spans, not accidental footer errors (`docs/REPORT.md §6.2`).
- PhotoRec produces no extraneous FPs on this corpus but misses 6 usable files; it also recovers the 20 kept files in the mixed images.
- The generated timing table above is the only speed claim: it records the measured runs for this small corpus and does not establish a general speed advantage. No unsourced cross-tool speed statement is made.

> This is a benchmark on a **small, synthetic corpus**. It should not be extrapolated into a 100% guarantee for every real-world disk image. Full per-image results are at [`bench/results/final/BENCHMARK.md`](bench/results/final/BENCHMARK.md).

### Reproducing the benchmark

```bash
# 1. Create the corpus (no root required)
python3 bench/make_corpus.py

# 2. Create FAT32/NTFS images (root required for loop-mount)
sudo python3 bench/make_images.py --carvex target/release/carvex --force

# 3. Run 5 configurations × 4 images × 3 repetitions into a fresh directory
#    bench/results/final is frozen; do not overwrite it.
python3 bench/run_benchmark.py --reps 3 --out bench/results/new-run
```

Required tools: `foremost`, `scalpel`, `photorec` (the `testdisk` package), `parted`, `mkfs.vfat`, `mkntfs`, `7z` (optional for CFReDS).

## CFReDS dataset

CarveX can scan DD images from the **NIST CFReDS Data Leakage Case**. The lightest option is **RM#2 — FAT32, 4 GB DD image**:

```text
https://cfreds-archive.nist.gov/data_leakage_case/images/rm%232/cfreds_2015_data_leakage_rm%232.7z
```

The download, hash verification, extraction, and CarveX run procedure is described in [`docs/CFREDS.md`](docs/CFREDS.md). A helper script is available:

```bash
./scripts/fetch_cfreds.sh
```

The CFReDS trial used a raw/DD image. CarveX now also reads classic E01
segments directly (`--input-format e01`); EWF2/Ex01, encrypted E01 and L01 are
not supported, so those still require conversion with another forensic tool.

**Trial run results on RM#2** (2026-11-17, CarveX 0.1.0):

- Both the archive and the DD image match the SHA-1/MD5 values published by NIST; the SHA-256 is recorded in [`docs/CFREDS.md`](docs/CFREDS.md).
- The image is 3.73 GiB long and contains one quick-formatted 1 GiB FAT32 partition (label `NO NAME`, 261,119/261,120 free clusters).
- `unallocated` scan of 1,020 MiB: 318 files (113 JPEG, 205 PNG), 73 candidates rejected, 38 nested objects blocked, 1.6 s.
- `raw` scan of the whole 3.73 GiB image: the same 318 files in 3.4 s; output identical to unallocated mode.
- 318/318 files are reported `complete`; a historical Pillow check only confirms that the files were structurally parsed, not verified (see `validity`) — Pillow's `Image.verify()` does not decode pixels. The 276 unique SHA-256 values (42 duplicate pairs) are unchanged published figures.
- At least 178/318 files are media embedded in `word/media/` or `ppt/media/` of DOCX/PPTX, not standalone deleted images. There are no PDFs. Since there is no ground truth, this is a structural verification rather than a precision/recall measurement.

## Limitations and notes

1. **FAT32 and NTFS only.** FAT12/FAT16, exFAT, ext4, APFS, and HFS+ are not supported; when encountering them, use `--mode raw`.
2. **No reliance on directory entries.** CarveX does not read original filenames, sizes, or paths from the MFT/directory; output names are only a sequence number + offset.
3. **Fragmented files.** By default the carver relies on header/footer and extent bounds; a fragmented file may be truncated, structurally wrong, or skipped. The opt-in `--fragment-recovery` stage now has a measured D1-style development profile: exact recall 4/6 (66.7%, Wilson 95% [30.0%, 90.3%]), splice rate 0/4 (Wilson 95% [0%, 49.0%]) and junk false-positive rate 0/24 (Wilson 95% [0%, 13.8%]); the larger X15 control recalled 3/3 files. It is not a runlist/metadata reassembler, only two fragments are joined, and the frozen published benchmark excludes it.
4. **Limited payload decoding.** By default, JPEG is checked for marker structure, PNG for chunk CRCs, and PDF structurally rather than by rendering. A PNG chunk-CRC mismatch is now an integrity failure: the file is still written with its unchanged score, but `validation=partial`, `validity=suspect` and a `chunk_crc` check name the chunk and offset. Decode mode recomputes every CRC and sets `validated_bytes` to the contiguous prefix ending at the last good chunk. `--validate decode` and B-06 stitches invoke the deep validator. PNG inflation streams IDAT through a fixed buffer and downgrades a declared image above `--max-decode-bytes` (default 256 MiB) to `partial` before inflating; JPEG and PDF decoder internals are not fully instrumented. A byte-aligned baseline JPEG tail swap between files with identical headers can still pass every implemented check and receive `validity=verified`; the measured residual rate is disclosed in `docs/baseline/jpeg_splice_v0.2.0.txt`. PDF decode mode now verifies bounded xref/trailer/catalog structure and stream lengths, but unsupported or budget-limited checks are `unverified`/`structural`, and no PDF rendering or content verification is performed.
5. **Complex PDFs.** Encrypted PDFs, PDFs with severe structural errors, or fake `%%EOF` inside a stream can cause the parser to choose the wrong boundary. Try `--pdf-eof first/last` or `--keep-nested`.
6. **Noise in free space.** Unallocated space may contain multiple old versions of the same file. The benchmark uses freshly created images, so this phenomenon is not fully simulated.
7. **Performance** depends on cache, disk, and CPU count. The figures in the report are from a 4-CPU machine, 64 MiB images, and a warm cache.
8. **No real-world Windows testing.** The PE binary was built and format-checked but has not been run on Windows in the development environment.
9. **Fixture-level container coverage.** The B-04 readers were verified with generated images, not `ewfacquire`/`ewfexport` output. The dynamic VHD reader uses the block allocation table but does not interpret the per-sector presence bitmap inside an allocated block.
10. **Resource-budget scope.** `--max-memory` bounds concurrent carve, B-06 join and write buffers, not total process RSS. `--max-candidates`, `--max-work-per-candidate` and `--max-output` are deterministic work-unit limits; `--time-budget` is a wall-clock safety net and flags its partial report as `nondeterministic_stop: true`. All four emit coded warnings and exit 3 on excess. Carved bytes are re-read from the read-only source and verified by SHA-256 before an output is renamed into place. Decode validation has its own `--max-validator-memory` bound and streams PNG inflation; JPEG/PDF decoder internals are not fully instrumented.
11. **Panic containment.** A panic in one candidate's analysis/validation or one B-06 join attempt is recorded as an `internal_errors[]` entry (offset, type, message), counted in `stats.internal_errors`, reported by `CANDIDATE_PANIC` and exits 3; other candidates continue. A panic while opening a container becomes a typed error. Profiles keep `panic = "unwind"` so containment can work. This is defence in depth, not a substitute for parser tests, and no fuzzing claim is made here.

## Project structure

```text
carvex/
├── Cargo.toml
├── src/
│   ├── main.rs              # entry point
│   ├── cli.rs               # CLI definitions
│   ├── io.rs                # ImageSource + RawFile
│   ├── image/               # split raw, E01, VHD, VMDK, stdin sources
│   ├── scan.rs              # scanning engine, carving, filtering, reporting
│   ├── report.rs            # JSON/CSV output
│   ├── util.rs              # image reading, extents, hashing, size parsing
│   ├── fs/
│   │   ├── mod.rs           # detect, MBR/GPT, FsInfo
│   │   ├── fat32.rs         # BPB + FAT table
│   │   └── ntfs.rs          # $MFT + $Bitmap
│   └── signatures/
│       ├── mod.rs           # FileType, analyze, scoring
│       ├── jpeg.rs
│       ├── png.rs
│       └── pdf.rs
├── tests/
│   ├── cli_contract.rs
│   ├── evidence_formats.rs  # B-04 container fixtures
│   ├── fragment_recovery.rs # B-06 S12 fixtures
│   ├── fs_detect.rs
│   ├── memory_budget.rs     # R-01 budget/candidate bounds
│   ├── png_decode_budget.rs # R-02 PNG decode bomb fixtures
│   ├── properties.rs
│   └── scan_integration.rs
├── fuzz/                   # libFuzzer targets and committed seeds
├── bench/
│   ├── make_corpus.py
│   ├── make_images.py
│   ├── run_benchmark.py
│   ├── corpus/              # 30 files + manifest
│   ├── images/              # 4 64 MiB images + ground truth
│   └── results/
│       ├── final/           # official benchmark results
│       └── cfreds/          # JSON/CSV reports + CFReDS RM#2 logs
├── dist/                    # Linux/Windows binaries + SHA256SUMS
├── docs/
│   ├── REPORT.md            # sole technical report
│   ├── VALIDATION.md        # executed evidence
│   └── CFREDS.md            # testing guide for CFReDS
└── scripts/
    └── fetch_cfreds.sh
```

### Documentation provenance (H-33)

The scan flag table above is generated from `carvex scan --help` by
`bench/render_docs.py --check`; numbers in this README come from generated
results, not hand-entered estimates. Review items that were already fixed
(`--max-memory`, `--max-truncated-size`, the PNG CRC limitation,
`tests/fragment_recovery.rs` and the `validity` field) were verified by `grep`
and are recorded as `NOT-REPRODUCED` in `docs/PROGRESS.md`. Absolute
`/home/kali` paths remain in committed ground truth, logs and reports as
original-author provenance; they are not required input paths and are preserved
so the frozen artefacts and their hashes stay unchanged (see `docs/DECISIONS.md`
D-57).

## Testing

```bash
cargo test
```

The current suite has 134 tests (43 unit + 13 `cli_contract` +
7 `evidence_formats` + 5 `fragment_recovery` + 8 `fs_detect` +
2 `jpeg_frame_budget` + 4 `memory_budget` + 3 `panic_containment` +
2 `png_crc_validity` + 4 `png_decode_budget` + 2 `png_validator_budget` +
8 `pdf_structure_validation` + 5 `properties` + 4 `regress_e01_safety` +
4 `regress_vmdk_safety` + 3 `repair_validation` + 6 `scan_integration` +
1 `validity_model` + 4 `work_budgets` + 3 `volume_parse_errors` +
3 `region_coverage`).
Run the full G0-G9 release gate with `bash scripts/gate.sh`: it also runs G7
golden hashes, G8 performance checks against committed baselines, a generated
flag-table check (G9), 70% line coverage, a 13-target fuzz smoke, all repro
controls, the benchmark smoke and the H-31 status-label reconciliation check.
CI runs the same gate plus packaged Windows and musl binary execution. The
artefact manifest for the validation claims is `bench/results/EVIDENCE.md`.
The suite covers, among other things:

- JPEG baseline, progressive, EXIF thumbnail, truncated data, fake headers;
- Valid PNG, bad CRC, truncated chunk, indexed PNG missing PLTE, first chunk not IHDR;
- PDF with valid xref, fake `%%EOF` in a stream, incremental revision, truncated PDF, PDF with no objects;
- Extent merge/complement utilities and size parsing;
- Byte-for-byte round-trips for split raw, E01, VHD, VMDK and stdin sources.
- Exact S12-style interleaved JPEG, PNG and PDF recovery with B-06 enabled.
- Full-image `regions[]` coverage, real free extents and exact `bytes_scanned` accounting, including `--max-regions` summarisation.
- Bounded-memory assertions for dense candidate budgets and PNG decompression,
  plus rejection of extreme dimensions and invalid filter bytes.

## License

MIT. See [`LICENSE`](LICENSE).

## References

- NIST CFReDS — Data Leakage Case: <https://cfreds-archive.nist.gov/data_leakage_case/data-leakage-case.html>
- NIST CFReDS — Hacking Case: <https://cfreds.nist.gov/all/NIST/HackingCase>
- Foremost: <https://foremost.sourceforge.net/>
- Scalpel: <https://github.com/sleuthkit/scalpel>
- PhotoRec: <https://www.cgsecurity.org/wiki/PhotoRec>
- JPEG (ITU-T T.81), PNG (ISO/IEC 15948), PDF (ISO 32000-1), FAT32 (Microsoft FAT specification), NTFS (Linux-NTFS documentation).
