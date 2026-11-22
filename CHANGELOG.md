# Changelog

All notable changes to CarveX are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- B-04 read-only evidence sources behind `ImageSource`: concatenated raw
  segments, classic EWF-E01 (compressed and Adler-32 chunks), fixed/dynamic
  VHD, flat/sparse VMDK extents, and stdin via a temporary spool.
- `--input-format auto|raw|split|e01|vhd|vmdk|stdin` for `info` and `scan`;
  `IMAGE` may be `-` to read stdin.
- Five fixture-based tests in `tests/evidence_formats.rs` verify byte-for-byte
  reads, stable source identity for disk-backed containers (no false
  `IMAGE_MODIFIED`), and stdin report identity.
- B-06 bounded bifragment gap carving behind the opt-in
  `--fragment-recovery` flag, with `--max-gap`, `--max-frag-candidates` and
  `--frag-time-limit`. Accepted stitches pass the deep validator and add
  `source="bgc"`, optional `fragments[]`, `fragment_checks` and `confidence` to
  the JSON report; the default report shape and CSV column prefix are unchanged.
- Two `tests/fragment_recovery.rs` tests cover the S12-style interleaved
  `[A1][B1][A2][B2]` scenario for JPEG, PNG and PDF, plus rejection of
  invalid B-06 limits.
- `--max-validator-memory` (default `256M`) bounds the working memory claimed
  by one decode validator (H-05/H-06). `--max-decode-bytes` remains as a
  visible alias; the effective value is recorded in report schema v2 as both
  `options.max_validator_memory` and the legacy `options.max_decode_bytes`.
  Schema 1 is unchanged.
- Report schema 2 adds `unreadable_regions[]` with the offset, length and reason
  for each source range that could not be read. Outputs overlapping such a
  range are labelled as not verified; `skip` drops them and emits
  `CANDIDATE_SKIPPED_UNREADABLE`.
- `--on-read-error abort|zero-fill|skip` controls how unreadable bytes from an
  evidence source are handled; the default is `zero-fill`. `--max-stdin-bytes`
  and `--spool-dir` bound and locate the temporary stdin spool file.
- H-16: every report file carries authoritative `validity` and structured
  `checks[]` (`{name, ok, note}`), with `ok` allowed to be `null` for checks
  that were not performed or are unsupported. CSV, console, summary counts and
  manifest comments show the same label.

### Changed

- H-08: `--max-memory` now bounds concurrent working buffers instead of the
  cumulative size of carved output. Carving keeps source ranges, analysis and
  digests; the write stage re-reads each output from the read-only source,
  verifies its SHA-256, and emits `SOURCE_CHANGED` with exit 3 on a mismatch.
  A candidate whose own buffer requirement exceeds the budget is skipped with
  `CANDIDATE_SKIPPED_BUDGET`; all budget events produce a report rather than an
  exit 1 with no output.
- Auto-detection selects a container from magic bytes and segment naming;
  explicit `--input-format raw` remains available. Ordinary raw images still
  use the unchanged `RawFile` path, so frozen benchmark behavior is preserved.
- Updated test count to 80. Re-measured line coverage after R-01/R-02 is
  4456/6391 lines = 69.72%; the honest CI/local floor remains 60%.
- E01 read errors now follow `--on-read-error`; the default changed from
  failing the whole run to zero-filling the affected chunk and recording it.
  Use `--on-read-error abort` for the previous behaviour. Chunk sizes declared
  in an E01 header are capped at 64 MiB before allocation.
- H-16: default `structure` rows are now labelled `structural` and printed as
  "content not checked" rather than presented as complete/verified. Scores,
  emission decisions and legacy `validation`/`status` values are unchanged.
  The B-06 check-count field is serialized as `fragment_checks` so `checks` can
  be the structured array; schema 1 omits all additive validity fields.

### Fixed

- R-01: `--max-memory` is now enforced as a blocking budget on concurrent
  carve, B-06 join and write buffers. Carving keeps source ranges, analysis and
  digests; the write stage re-reads and verifies each output. See the H-08
  entry below for the final semantics.
- Added `--max-candidates`, `--max-work-per-candidate`, `--max-output` and
  `--time-budget` to bound header floods, per-candidate analyzer/validator
  work, cumulative output and wall-clock time. Work-unit excess emits
  `CANDIDATE_LIMIT`/`WORK_LIMIT`/`OUTPUT_LIMIT`; the wall-clock safety net
  emits `TIME_LIMIT` and sets `nondeterministic_stop: true`. All produce a
  partial report and exit 3. Report schema 2 records the new limits and stop
  flag; schema 1 strips them.
- H-10: candidate analysis/validation and B-06 join attempts now contain
  panics, record `internal_errors[]` with offset/type/message, increment
  `stats.internal_errors`, emit `CANDIDATE_PANIC` and exit 3 while continuing
  other work. Container-open panics become typed errors. All explicit Cargo
  profiles keep `panic = "unwind"`.
- H-11/H-22: hostile E01 chunk sizes are rejected with a typed error before
  allocation; stdin spooling accepts an explicit byte limit and spool directory;
  short final E01 chunks are read at their logical length; checksum/inflate
  failures are handled by the configured read-error policy instead of aborting
  unconditionally. Two `tests/regress_e01_safety.rs` tests cover the hostile
  header and the short final chunk.
- H-06: JPEG decode validation now checks the claimed SOF frame size against
  `--max-validator-memory` and an optimistic entropy capacity before invoking
  the decoder; oversized or implausible claims are `unverified`. Arithmetic
  frames are exempt from the entropy heuristic but not the memory budget.
- R-02: PNG decode validation now checks the expected filtered-scanline size
  at IHDR against the validator budget before inflating anything. Inflation
  streams IDAT chunks through a fixed 64 KiB buffer, checks filter bytes as
  rows arrive, handles Adam7 without a row-offset vector, and never retains
  the full compressed or decoded image. A PNG above the bound is downgraded to
  `validation=partial` rather than inflated. Four `tests/png_decode_budget.rs`
  tests cover extreme dimensions, an explicit budget, a 50,335,744-byte
  expansion under a peak-RSS assertion, and an invalid filter byte.

### Notes

- The frozen benchmark, lockbox results, published figures, protocol text and
  distribution binaries are not modified or rebuilt by this work. B-06 is
  opt-in and its controlled S12 fixture is not a benchmark recall claim.
- E01 segment discovery supports the usual two-digit base-36 suffixes (up to
  `.EZZ`, 1295 segments); sets with longer suffixes are rejected explicitly
  rather than partially discovered.
- H-14 fuzz smoke: the six new targets each ran 60 s locally (235,272
  executions total), with no crash, timeout, OOM or artifact. Real-tool E01
  and VMDK container seeds were SKIPPED because the acquisition tools are not
  installed; the historical seven-target run is not presented as a
  post-change result.
- H-15 X23 re-timing found no source-build slowdown (1 thread median 0.489 s,
  default 0.156 s) and a 3.51x parallel slowdown only in the stale 0.2.0 musl
  dist binary (0.694 s versus 2.436 s). No allocator change or dist rebuild
  was made for this finding.

## [0.2.0] — 2026-11-19

Reliability, forensic-reporting and test release. Carving behaviour for
JPEG/PNG/PDF is preserved on the published benchmark; report schema 2 is
additive and schema 1 remains available.

### Added

- Library crate `carvex` with a thin `src/main.rs`; all parsers, scanning and
  reporting are usable from Rust tests and fuzz targets.
- `ImageSource` abstraction (read-only, checked reads) with a raw-file
  implementation that falls back to seeking for block devices whose metadata
  size is zero.
- Format registry that drives `--types`, the `signatures` command, header
  search and output extensions.
- `--validate structure|decode`; per-file `validation`
  (`structure`/`decoded`/`partial`/`unverified`) and `validated_bytes`.
  Decode mode validates JPEG baseline MCU counts, PNG IDAT inflation and PDF
  xref targets.
- `--align none|auto|cluster|sector` (default `none` preserves history).
- `--max-truncated-size` (default 16 MiB) and `--max-memory`.
- Report schema 2: `schema_version`, `tool_info`, `input` (size/mtime/SHA-256
  before and after), `argv`, `options`, `case_id`, `examiner`, ISO-8601
  `started_at`, `warnings`, and per-file `lba`, `cluster`, `partition_index`,
  `extent_id`, `allocated`, `source`, `fragments`.
- `--report-schema 1` for legacy consumers, `--hash-image`, `--case-id`,
  `--examiner`, `--manifest` (SHA256SUMS for carved files).
- Multi-volume scanning (A-07): `--partition largest|all|N` (default `all`)
  plus gap scanning by default; `--include-gaps` stays accepted, and the new
  `--no-gaps` flag restores the historical skipped-gap behaviour.
  Multi-volume runs write each volume into its own `part1/`, `part2/`, …
  subdirectory.
- Coded warnings and completeness exit code (A-07): every volume, gap or
  partition that was not scanned is reported as a warning object
  `{code, message}` in the report and on stderr — `VOLUME_NOT_SCANNED`,
  `GAP_NOT_SCANNED`, `VOLUME_UNSUPPORTED`, `FS_DETECTION_FAILED`,
  `IMAGE_MODIFIED` — and makes the run exit **3** ("finished but incomplete").
  `RAW_SCANS_ALLOCATED` stays informational and does not change the exit code
  (behaviour change, see below).
- Report schema 2 adds `volumes[]` (every detected volume and whether it was
  scanned) and `regions[]` (`kind = free|allocated|gap|unsupported`, `offset`,
  `len`, `scanned`, `partition_index`), so a reader can see exactly which
  bytes were read.
- New documentation: `SECURITY.md`, `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`;
  `docs/DECISIONS.md` (D-01…D-28) records every consequential decision, and
  `docs/VALIDATION.md` records the execution evidence.
- Rebuilt distribution binaries: `dist/carvex-linux-x86_64` (static-pie musl,
  stripped) and `dist/carvex-windows-x86_64.exe` (PE32+, stripped), both
  `carvex 0.2.0`, with `dist/SHA256SUMS` regenerated after the final internal
  `ScanInput` refactor. The Windows binary is built and format-checked but has
  not been executed on Windows.
- `docs/report.schema.json` plus a CLI test that checks the emitted key sets
  against the schema.
- Integration tests (`tests/fs_detect.rs`, `tests/scan_integration.rs`,
  `tests/cli_contract.rs`), property tests (`tests/properties.rs`) and thirteen
  cargo-fuzz targets: the seven original targets with seeds from the benchmark
  images plus six H-14 parser targets with deterministic evidence-format and
  hostile-input seeds.
- Hidden, bounded fixture builders shared by `tests/evidence_formats.rs`,
  `examples/gen_fuzz_seeds.rs`, the six new fuzz targets and five structured
  parser property tests. The CI fuzz matrix lists all thirteen targets.
- `.github/workflows/ci.yml` (3-OS fmt/clippy/test/doc/build, MSRV,
  cargo-deny/audit, benchmark smoke, conditional nightly fuzz matrix) with a
  pinned direct actionlint 1.7.12 install and lint step.
- `bench/run_benchmark.py --smoke`: builds an 8 MiB synthetic FAT32 image
  without root/loop devices and runs CarveX unallocated + raw.
- `scripts/gate.sh`, `scripts/fuzz.sh`, `scripts/coverage.sh`,
  `scripts/lockbox_run.sh`, `scripts/benchmark_full.sh`, `deny.toml`,
  `docs/{PROGRESS,DECISIONS,DEPENDENCIES,BENCHMARK_PROTOCOL,
  REVIEW_RESPONSE,VALIDATION}.md`.
- Local supply-chain verification with `cargo-audit 0.23.0` and
  `cargo-deny 0.21.0`; `deny.toml` updated for the current `unmaintained`
  configuration field.

### Changed

- MSRV raised to **1.85** to match the locked dependency set; the CI workflow
  is configured to check it.
- Release profile uses `panic = "unwind"` and `overflow-checks = true`.
  Filesystem detection is wrapped in `catch_unwind`; auto mode falls back to a
  raw scan with a warning (exit 3) instead of aborting.
- PDF document boundaries: a `%PDF-` header starts a new document only when it
  follows `%%EOF`; headers inside uncompressed streams/attachments stay part
  of the outer document.
- PDF completion read-ahead: an early `%%EOF` requests more data when the rest
  of the current window is whitespace or looks like a new revision, so
  linearized and incremental-update PDFs are carved whole without swallowing
  NUL padding.
- PNG salvage: a complete IHDR+IDAT followed by an invalid chunk type yields a
  truncated prefix instead of rejecting the whole candidate.
- Baseline JPEG validation uses a strict Huffman/MCU count; spliced files are
  reported `validation=partial`.
- Zero-tail trimming preserves payload bytes and enforces a 16-byte floor;
  truncated files are capped by `--max-truncated-size`.
- Nested candidates are resolved in memory before writing; output ordering and
  naming are deterministic across thread counts and chunk sizes.
- FAT32/NTFS `cluster` is the real on-disk cluster number (FAT32 starts at 2);
  `FsInfo.data_start` records the first data cluster.
- `status=complete` is documented as structural completeness, not “the file
  opens”.
- **Behaviour change:** `carvex scan --mode raw` no longer exits 3 because of
  the informational `RAW_SCANS_ALLOCATED` warning (raw mode reads everything,
  so no evidence is omitted). Runs that genuinely omit volumes/gaps still exit
  3 via the new coded warnings above.
- **Behaviour change (A-07 step 2, user-approved):** the default scan now
  reads the free space of **all** supported volumes plus all gaps and
  unsupported partitions. A default run of an MBR image therefore carves more
  files and exits 0 instead of 3. Pass `--partition largest --no-gaps` to
  restore the v0.1.0/step-1 view; coded warnings and exit 3 still apply to any
  run that actually leaves evidence unscanned.
- **Behaviour change:** the benchmark runner pins `--partition largest
  --no-gaps` for CarveX so the published v0.1.0 search space and results remain
  reproducible; exit codes 0 and 3 are both accepted.
- Documentation is now English-only across README, release docs, `docs/`,
  source comments and reports; the technical report is now `docs/REPORT.md`.

### Fixed

- F-01: early `%%EOF` in a PDF no longer truncates linearized/incremental
  documents (R1).
- F-02: embedded `%PDF-` no longer cuts the outer PDF (R2/R3).
- F-03: corrupt MFT/attributes return errors instead of panicking; auto mode
  falls back to raw with exit 3 (R7).
- F-04: spliced JPEGs are detected under `--validate decode` (R4c).
- F-05: truncated JPEG/PNG heads are salvaged (R4a/R4b) where structurally
  possible.
- F-06: 25,811 fake headers in 64 MiB scan in under one second (R5).
- F-07: zero-tail candidates no longer produce 64 MiB truncated files (R6).
- F-13: reports carry run parameters, source hash, tool version, LBA/cluster
  and ISO timestamps.
- F-14: pipeline split into small functions; no `too_many_arguments` allows;
  no write-then-delete for nested files.
- Parser hardening: `fs/*` and `signatures/*` deny `indexing_slicing`,
  `unwrap_used`, `expect_used`; `#![forbid(unsafe_code)]` crate-wide.
- NTFS volume size off-by-one: the boot-sector field at offset 0x28 stores
  *(sectors − 1)* because the backup boot sector lives in the last sector.
  The parser now adds 1 (clamped to the partition size), so `regions[]` tile
  the image exactly and `$Bitmap` is not larger than the computed cluster
  count. Unit test
  `volume_sector_count_includes_the_backup_boot_sector` locks it in.

### Removed

- `panic = "abort"` from the release profile.
- `ScanStats.trimmed_tail` (never incremented; not part of any published
  schema).
- All `#[allow(clippy::too_many_arguments)]`.

### Compatibility

- CLI subcommands, flags, output naming, JSON/CSV field names and meanings of
  existing fields are unchanged. New fields are additive.
- Report schema 1 can be emitted with `--report-schema 1`.
- Historical defaults remain: `--align none`, `--validate structure`,
  `--max-truncated-size 16M`.
- Published benchmark numbers remain those of 0.1.0; the 0.2.0 carve results
  were verified byte-for-byte identical on all four benchmark images.

### Validation

All execution evidence is collected in `docs/VALIDATION.md`; published figures
remain those of 0.1.0.

- `scripts/gate.sh` finished **GATE GREEN**: 62 tests, 10/10 repro PASS, smoke
  TP=3/FP=0 unallocated and TP=3/FP=1 raw, plus fmt/clippy/doc/MSRV/release,
  actionlint, cargo-deny and cargo-audit.
- `scripts/fuzz.sh 60` ran 7 targets × 60 s: 18,520,025 executions, no crash,
  timeout, OOM or artifact.
- `scripts/coverage.sh 60` measured 2859/4508 covered lines = **63.42%**; the
  CI/local floor is honestly 60%, not 80%.
- GitHub Actions run `44548142275` on `42376a9` succeeded with all jobs green;
  the push-event fuzz matrix was skipped by its schedule/workflow_dispatch
  trigger.
- The 20-seed lockbox was executed exactly once into `bench/results/lockbox-v1`
  with dataset and result checksums verified; results were not used for tuning.
- The full 4-image/3-repetition benchmark was re-run into
  `bench/results/full-0.2.0` with checksums verified. All 20 rows are
  metric-identical to the frozen 0.1.0 `bench/results/final/results.json`.

### Known gaps

See `docs/REVIEW_RESPONSE.md` for the status of every guide ID (A-01…D-03).
Scientific benchmark items C-02 and C-09 are open and C-06/C-07 are partial;
lockbox-v1 covers only the D1-style profile and D2–D7 remain OPEN. The packaged
Windows binary, the `viewable` PDF check (`qpdf`/`pdftoppm` unavailable),
same-search-space baselines, ablations, block-level metrics and advanced
statistics are not claimed. Fuzz, coverage, cargo-deny/audit and GitHub CI were
executed as recorded in `docs/VALIDATION.md`; no claim is made beyond that
evidence.

## [0.1.0] — 2026-11-17

Initial release.

### Added

- Header/footer carving for JPEG, PNG and PDF from raw images.
- FAT32 and NTFS detection, unallocated extents, cluster-aware accounting.
- Parallel scanning, structural scoring, truncation repair, nested
  suppression, JSON/CSV reports and documentation.
- 30-file synthetic corpus, four 64 MiB benchmark images, comparison against
  Foremost, Scalpel and PhotoRec, and a CFReDS RM#2 run.

[0.2.0]: https://example.invalid/carvex/compare/v0.1.0...v0.2.0
[0.1.0]: https://example.invalid/carvex/releases/tag/v0.1.0
