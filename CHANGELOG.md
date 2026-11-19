# Changelog

All notable changes to CarveX are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
  `docs/DECISIONS.md` (D-01…D-23) records every consequential decision.
- Rebuilt distribution binaries: `dist/carvex-linux-x86_64` (static-pie musl,
  stripped) and `dist/carvex-windows-x86_64.exe` (PE32+, stripped), both
  `carvex 0.2.0`, with `dist/SHA256SUMS` regenerated after the final internal
  `ScanInput` refactor. The Windows binary is built and format-checked but has
  not been executed on Windows.
- `docs/report.schema.json` plus a CLI test that checks the emitted key sets
  against the schema.
- Integration tests (`tests/fs_detect.rs`, `tests/scan_integration.rs`,
  `tests/cli_contract.rs`), property tests (`tests/properties.rs`) and seven
  cargo-fuzz targets with seeds from the benchmark images.
- `.github/workflows/ci.yml` (3-OS fmt/clippy/test/doc/build, MSRV,
  cargo-deny/audit, benchmark smoke, nightly fuzz matrix).
- `bench/run_benchmark.py --smoke`: builds an 8 MiB synthetic FAT32 image
  without root/loop devices and runs CarveX unallocated + raw.
- `scripts/gate.sh`, `deny.toml`, `docs/{PROGRESS,DECISIONS,DEPENDENCIES,
  BENCHMARK_PROTOCOL,REVIEW_RESPONSE}.md`.
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

### Known gaps

See `docs/REVIEW_RESPONSE.md` for the status of every guide ID (A-01…D-03).
Scientific benchmark items C-02 and C-09 are open and C-06/C-07 are partial;
no lockbox result is claimed. Fuzz, coverage, cargo-deny/audit and Windows
execution were not run locally (tools or hardware unavailable) and are marked
`SKIPPED` in `docs/PROGRESS.md`.

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
