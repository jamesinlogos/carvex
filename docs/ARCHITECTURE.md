# Architecture (docs/ARCHITECTURE.md)

This document describes the **current structure of the source code** (0.2.0).
The decisions *why* are in [`DECISIONS.md`](DECISIONS.md); business requirements
are in `guide.md`.

## 1. Overview

```text
             ┌──────────────┐
 argv ─────► │ cli.rs       │  parse args, orchestrate, exit code
             └──────┬───────┘
                    │ ImageSource (io.rs)          ← source image: read-only
        ┌───────────┼───────────────────┐
        ▼           ▼                   ▼
   fs::detect*   scan::run_scan      report::write_*
   (partition,   (find candidates →  (JSON schema 2 / 1,
    FAT32/NTFS,   validate → filter → CSV, atomic write)
    free extent)  write files → report)
```

A `carvex scan` run consists of: (1) detecting filesystem/partition
(`fs::detect_all` → `VolumeSet`), (2) selecting volumes by `--partition` (default
`all`) and building the list of extents to read (`plan_extents`: free space, allocated,
gap/unsupported unless `--no-gaps` is used), (3) scanning each extent in parallel
(`scan::run_scan`),
(4) assembling the report consisting of `volumes[]`, `regions[]`, `warnings[]`, (5) exit code
according to the degree of completeness of the evidence.

## 2. Modules

| Module | Responsibility |
|---|---|
| `src/lib.rs` | Library crate; `#![forbid(unsafe_code)]`; exports all public modules. |
| `src/main.rs` | Thin `main()`: calls `cli::run()`, maps `Err` → exit 1, prints error messages. |
| `src/cli.rs` | clap arguments (`info`, `scan`, `signatures`), maps selection → `ScanOptions`, generates coded warnings, decides the exit code. |
| `src/io.rs` | `trait ImageSource { len, read_at }`; `RawFile` (pread by offset, no pointer sharing between threads); `sha256_source` for `--hash-image`. |
| `src/util.rs` | `Extent` (offset+len) + addition/subtraction/merge/complement with overflow checking; `parse_size`, `human_size`, `file_identity`. |
| `src/fs/mod.rs` | MBR/GPT/superfloppy detection; `Partition`, `VolumeSet`, `PartitionSel`, `Region`/`RegionKind`; `detect` (historical, 1 volume) and `detect_all` (complete); `select_volumes`, `gap_regions`, `regions()`. |
| `src/fs/fat32.rs`, `src/fs/ntfs.rs` | Read BPB/FAT table; boot sector + `$MFT` + `$Bitmap` (fixup, runlist); returns `FsInfo` with `free_extents`. |
| `src/signatures/mod.rs` | Format registry (`FileType`), `ValidationMode` modes (`structure`/`decode`), `PdfEofMode`, scoring and `Validation`/`Status` structures. |
| `src/signatures/{jpeg,png,pdf}.rs` | Per-format analyzers: walk the structure sequentially, select the correct footer, write `notes`/`score`; `quick_reject` for cheap candidates. |
| `src/scan.rs` | Pipeline: extent → find header (memmem, with boundary overlap) → structure validation → filter (min-score/size/dedupe/nested) → write files → `ScanReport`; parallel scanning; `Warning`, `VolumeSummary`, `print_summary`. |
| `src/report.rs` | Serde model of `ScanReport`; atomic JSON write (temp + rename); schema 1 = schema 2 without the v2 fields; CSV. |

## 3. Invariants

1. **The source image is opened read-only.** All write paths are inside the `-o` directory.
2. **Reads are bounded by the extent.** A candidate never reads past the end of
   its containing extent — this is the basis for `unallocated` not touching
   allocated data.
3. **No `unsafe`, no panic on image data.** `src/fs/*` and
   `src/signatures/*` deny `unwrap_used`/`expect_used`/`indexing_slicing`;
   corrupt data returns `Err` (caught and turned into a fallback/warning) or
   is discarded at the filtering step.
4. **Allocation derived from the image is capped:** `--max-memory` (total carve
   bytes held in RAM), `--max-size`, `--max-truncated-size`, number of chunks;
   all offset/length operations use checked/saturating arithmetic.
5. **Deterministic results:** same image + same parameters ⇒ same file names
   (`NNNNN_<ext>_<offset16hex>.<ext>`) and same list; candidates are sorted
   by offset before writing; nested suppression runs **before** writing.
6. **Evidence is not silently skipped:** anything not read must appear in
   `regions[]`/`volumes[]` with `scanned=false` and in `warnings[]` with a
   code; exit 3 (A-07, `DECISIONS.md` D-19/D-23).

## 4. Parallelism

- The image is divided by `--chunk-size` (default 4 MiB); each chunk reads an extra 16
  boundary bytes so headers straddling a boundary are not missed, then deduplicates by offset.
- Thread count = `--threads` (0 = number of CPUs); each thread has its own buffer and
  reads only via `ImageSource::read_at` (multithread-safe with `RawFile`).
- Results are merged by offset ⇒ file output order is independent of thread completion
  order.

## 5. Errors and exit codes

- Unrecoverable programming/user errors → `anyhow::Error` → `main` prints
  `error: …` → **exit 1**; clap syntax errors → **exit 2**.
- Completed but evidence was missed (coded warnings `*_NOT_SCANNED`,
  `IMAGE_MODIFIED`, `FS_DETECTION_FAILED`…) → **exit 3**
  (`Warning::incomplete()` in `scan.rs`).
- Interrupted (SIGINT) → **exit 4**.
- Informational warning (`RAW_SCANS_ALLOCATED`) does not change the exit code (D-20).

## 6. Reports

- Schema **2** (default): adds `schema_version`, `tool_info`, `input`,
  `argv`, `options`, `case_id`, `examiner`, `started_at*`, `warnings[]`
  (`{code,message}`), `volumes[]`, `regions[]`; published at
  [`report.schema.json`](report.schema.json).
- Schema **1** (`--report-schema 1`): exactly the 0.1.0 shape — the test
  `report_schema_one_has_the_v01_shape` locks the key list.
- New fields must be additive; renaming/type changes/semantic changes are breaking
  changes (see `CONTRIBUTING.md`).

## 7. Extension points

- **Adding a format:** write an analyzer in `src/signatures/<fmt>.rs`, register it
  in `FileType` (magic, ext, validator, score), add a fixture + CLI contract
  test.
- **Adding a filesystem:** write `probe()` in `src/fs/<fs>.rs` returning `FsInfo`
  (requires `free_extents` + `length` so `regions[]` tiles the image completely),
  register it in `fs::probe`.
- **New data source (E01, block device):** implement `ImageSource` — the pipeline
  is unchanged (A-08/B-04, not present in 0.2.0).
