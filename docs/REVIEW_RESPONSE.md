# Cross-reference by ID of the unified guide (A-01…D-03)

Statuses use the guide's definitions exactly:

- **FIXED** — fixed, with tests/measurements proving it.
- **PARTIAL** — risk reduced but not fully resolved; the remainder is stated
  explicitly. Older reports called this `MITIGATED`.
- **OPEN** — not done; no claim of resolution.
- **WONTFIX** — not doing, with rationale (not used in this table).

Re-run evidence: `scripts/review_repro.py --assert` → **10/10 PASS**, 0 XPASS
(`docs/baseline/repro_v0.1.0.txt` is the red baseline of 0.1.0: 8/9 FAIL);
`scripts/gate.sh` → **GATE GREEN** (fmt, clippy, **134 tests** in the current
H-29 run, doc, MSRV, release, repro, bench `--smoke`). The gate was re-run
after B-04, B-06, R-01/R-02, H-14, H-16, H-17, H-18, H-19, H-21, H-22, H-25,
H-26, H-27, H-28, H-29 and H-30. The only successful recorded GitHub Actions
run predates those changes; the H-30 run at HEAD failed and is recorded without
guessing remote causes.

## Summary table of 38 IDs (guide §6)

| ID | Status | Evidence / test | Remaining notes |
|----|------------|-------------------|-----------------|
| A-01 | **FIXED** | R1, R2 PASS; unit `early_eof_requests_more_when_incremental_update_follows`, `smart_selects_last_incremental_revision`; H-19 adds xref-stream and `/ObjStm` fixtures | No 2–3 MiB linearized PDF compliant with Annex F has been built. |
| A-02 | **FIXED** | R3 PASS; `embedded_pdf_is_not_a_document_boundary`, `adjacent_pdfs_are_split_at_the_second_header`; nested suppression before writing | — |
| A-03 | **PARTIAL** | `#![forbid(unsafe_code)]`; deny `indexing_slicing`/`unwrap`/`expect` in `fs/*`+`signatures/*`; `catch_unwind` around `fs::detect`; release unwind + `overflow-checks`; R7 PASS; historical 7-target run (18,520,025 executions) plus H-14's six new parser targets (235,272 executions), all clean | Fuzzing is not exhaustive; the original seven targets were not rerun after H-14 and no post-change GitHub fuzz run is claimed (`docs/VALIDATION.md §4`). |
| A-04 | **PARTIAL** | `--validate structure\|decode` (default `structure` per A-04's decision rule); legacy `validation`+`validated_bytes` preserved, with authoritative additive `validity`+`checks[]` from H-16; R4c PASS (JPEG with appended tail → `partial`); measured: structure 0.11–0.20 s vs decode 0.49–1.10 s, 30/30 `decoded`; R-02 bounds PNG at IHDR and streams IDAT/filter validation | Field name kept as `validation` (D-22), but H-16 adds the missing `validity` semantics: default `structure` rows are `structural` and read "content not checked". Decode supports only baseline sequential JPEG; progressive → `unverified`/`structural`; PDF stage 2 checks bounded xref/trailer/catalog structure but does not render content. H-21 now requires repair revalidation before the `repaired` label. |
| A-05 | **FIXED** | R6 PASS; zero-tail trim + `--max-truncated-size 16M` (default); salvage JPEG/PNG prefixes (R4a/R4b) with `status=truncated` | No `--salvage`/`--partial-min-fraction` flag (B-07); D-47 records the `[SHOULD]` skip. |
| A-06 | **PARTIAL** | R5 PASS: 0.48–0.79 s for 25,811 fake headers/64 MiB (target ≤1 s); R-01 fixed: `--max-memory` is enforced before carve-buffer allocation/growth and before retaining carved or B-06 stitched results, with budget-aware in-flight batching and `--max-candidates`/`--time-budget`; R-02 bounds the PNG decode path separately; parallel carving remains deterministic (1 vs 4 thread test) | Measured on warm cache, single machine. `--max-memory` is a scan-time budget, not process RSS; JPEG/PDF decoder internals are not fully accounted. |
| A-07 | **FIXED** (step 1 + step 2) | Red baseline `docs/baseline/A-07_v0.1.0.txt` (exit 0, silent misses); step 1 green baseline `docs/baseline/A-07_green_v0.2.0.txt` (30 files + warning + exit 3); step 2 green baseline `docs/baseline/A-07_green_v0.2.0_step2.txt` (default 60 files = 30+30, exit 0, `volumes[]`/`regions[]` both `scanned=true`). `detect_all()` → `VolumeSet`; `warnings[]` coded; `--partition largest --no-gaps` restores old behavior (D-23); H-27 makes `regions[]` a plan-backed, disjoint full-image coverage map | Step 2 was approved by the user (D-23); STOP-AND-ASK §3.5 resolved. Any RUN still missing evidence exits 3. |
| A-08 | **PARTIAL** | `trait ImageSource {len, read_at}` with the unchanged `RawFile` default (pread by offset); fallback `seek(End)` when `metadata().len()==0`; B-04 adds read-only split raw, E01, VHD, VMDK and stdin sources; H-22 adds `--on-read-error`, per-sector raw/split error policy, `unreadable_regions[]` and an abort partial report; `--hash-image` streaming | No `BlockDevice`/`--sector-size`; `info /dev/loopN`, real devices, Windows and libewf-made E01 not tested; VHD/VMDK keep container-specific error semantics. |
| A-09 | **PARTIAL** | Fixed NTFS sector off-by-one (`field+1` clamped to `available`, D-21, unit test `volume_sector_count_includes_the_backup_boot_sector`); all FS parse errors → `FS_DETECTION_FAILED`, and H-26 adds per-partition `VOLUME_PARSE_FAILED` with index/offset/reason plus `may_contain_allocated` on the fallback region | No `$ATTRIBUTE_LIST`, multi-extent `$MFT`, `$MFTMirr` fallback, backup boot fallback, FAT #2/backup BPB yet. |
| A-10 | **PARTIAL** | Lib crate + thin `main.rs`; pipeline `plan_extents→build_tasks→scan_task→carve_batch→postfilter→write_outputs`; format registry; `ScanInput` groups pipeline inputs. H-34 split the remaining long functions into pure helpers, enabled public `#![warn(missing_docs)]` and pedantic stage 1, documented the public API, and justified every retained `too_many_arguments` allow; `cargo doc -D warnings` and deterministic tests are green. | Numeric-cast and API-ergonomics pedantic stages are deliberately deferred to D-58; H-34 is recorded in commit f38a2ed. |
| A-11 | **PARTIAL** | **134 tests** in the current H-29 gate (43 unit + 13 `cli_contract` + 7 `evidence_formats` + 5 `fragment_recovery` + 8 `fs_detect` + 2 `jpeg_frame_budget` + 4 `memory_budget` + 3 `panic_containment` + 2 `png_crc_validity` + 4 `png_decode_budget` + 2 `png_validator_budget` + 8 `pdf_structure_validation` + 5 `properties` + 4 `regress_e01_safety` + 4 `regress_vmdk_safety` + 3 `repair_validation` + 6 `scan_integration` + 1 `validity_model` + 4 `work_budgets` + 3 `volume_parse_errors` + 3 `region_coverage`); `scripts/review_repro.py --assert/--xfail` 34 scenarios, 0 failures, 2 SKIPs, 0 XPASS; red baseline `docs/baseline/repro_v0.1.0.txt`; Critical/High findings have targeted regression tests; post-R-01/R-02 coverage 4456/6391 lines = 69.72%; H-30 ran the gate and coverage at 70% locally | No claim of ≥80% coverage or of a new coverage measurement after H-14/H-16/H-17/H-18/H-19/H-21/H-22/H-25/H-26/H-27/H-29; floor is honestly 60%. Integration tests are named `tests/*.rs` rather than `tests/regress_*.rs`. The H-30 GitHub `review repro assertions` job failed remotely; its logs were not available, so the remote cause is not claimed. |
| A-12 | **PARTIAL** | MSRV **1.85** (`rust-version` + README corrected from "Rust 1.74"); `cargo +1.85 check --locked` in gate; `.github/workflows/ci.yml` (3 OS, MSRV, deny/audit, smoke, push/PR fuzz matrix with 13 targets, packaged Windows and musl smokes, full gate, repro, coverage at 70% and SBOM); `rustfmt.toml`/`clippy.toml`/`deny.toml`; dist 0.2.0 rebuild + `SHA256SUMS`; local `cargo audit` 0.23.0, `cargo deny` 0.21.0 and `actionlint` 1.7.12 pass; earlier GitHub run `44548142275` on `42376a9` succeeded with all jobs green | H-30 pushed run `45035152791` on `d9dcac3` **failed**: 8 jobs succeeded and 18 failed, including the release gate, repro assertions, all 13 fuzz targets, Windows tests and both packaged-binary smokes; logs require repository admin and were not downloaded. No green CI run at HEAD is claimed. Packaged Windows `.exe` **not executed** locally; no signatures; removing binaries from `dist/` is STOP-AND-ASK — kept unchanged. |
| A-13 | **PARTIAL** | Exit codes 0/1/2/3/4 documented (README) and CLI contract tests; coded warnings (`*_NOT_SCANNED` → exit 3, D-19); `RAW_SCANS_ALLOCATED` informational (D-20); H-22 writes a partial report with `READ_ABORTED` on read-error abort (exit 1); H-28 installs a cooperative SIGINT/Ctrl-C handler that writes a partial report with `interrupted: true` and `INTERRUPTED` and exits 4; short error messages with suggested actions | No `thiserror` types, no leveled logger/`--log-format json`, no `ProgressSink`/ETA for the carve phase; a second signal exits 130 without a report by design. |
| A-14 | **PARTIAL** | Schema v2: `schema_version`, `argv`, `options`, `tool_info`, `input` (SHA-256 before/after, logical/physical sizes, resolved format and backing `files[]` from H-25), ISO-8601, `case_id`, `examiner`, `warnings[]` (objects), `volumes[]`, `regions[]`, per-file actual `lba`/`cluster`/`partition_index`/`extent_id`/`allocated`/`source`/`fragments`; H-16 adds required per-file `validity` and structured `checks[]`, renames the B-06 count to additive `fragment_checks`, and H-21 adds optional `sha256_original`; H-27 adds `options.max_regions`, plan-backed `regions[].extent_id`, `regions[].summarized` and the additive `mixed` kind; H-28 adds schema-2 `interrupted` plus the `INTERRUPTED` warning; `docs/report.schema.json` + test `emitted_report_matches_the_published_schema`; `report_schema_one_has_the_v01_shape`; atomic write; `--manifest` comments now show validity | `unreadable_regions[]` and `READ_ABORTED` belong to A-08/H-22; stdin `unchanged` is null (H-25); `tool_info.commit` may be empty outside git. Schema 1 still strips all additive fields. |
| A-15 | **PARTIAL** | Hard `--max-memory` budget checks before allocation/retention plus caps on `--max-size`/`--max-truncated-size`; `--max-candidates`/`--time-budget`; atomic write (temp+rename); `--force` for non-empty directory; nested does not write-then-delete | No rejection of output located inside the source image/source device; no `--overwrite` (force still does not overwrite existing files? — needs test); no free-space/`--max-output` check; symlink following not forbidden. |
| B-01 | **OPEN** | — | No reading of `0xE5` dir entries/not-in-use MFT; no `--mode metadata\|hybrid`, `original_name`/`mtime`. |
| B-02 | **PARTIAL** | MBR + GPT primary/backup; `--partition largest\|all\|N` (A-07); `info --json` lists `volumes[]` | No EBR chain (loop protection); still assumes 512-byte sectors for the partition table; no `--sector-size`. |
| B-03 | **OPEN** | — | No exFAT/FAT12/16 (CFReDS RM#1 is exFAT, so its metadata cannot be scanned). |
| B-04 | **PARTIAL** | Read-only sources in `src/image/` selected by `open_image()`/`--input-format`; five byte-for-byte fixture tests cover split raw, compressed and uncompressed E01, fixed and dynamic VHD, flat and sparse VMDK, and stdin report identity (D-29); H-14 adds six fuzz targets and five structured parser property tests | Recognized EWF2/Ex01, L01/Lx01, LVF, differencing VHD, ESX/COWD and compressed streamOptimized VMDK variants are rejected explicitly; encrypted E01 has no decryption and is not guaranteed to be detected before parsing. Real-tool E01/VMDK seeds were SKIPPED (tools absent). Block devices remain A-08; the raw default and frozen benchmark/figures are unchanged. |
| B-05 | **PARTIAL** | `--align none\|auto\|cluster\|sector` (default `none` preserves history); test `alignment_restricts_candidate_starts_without_changing_results_on_aligned_data`; golden D0 TP unchanged | No per-file `aligned` flag written to the report; no measurement of candidate/nested reduction on D5; not run as an ablation arm. |
| B-06 | **PARTIAL** | Opt-in `--fragment-recovery` adds work-unit-bounded two-fragment gap carving. Ordinary contiguous carves skip deep probing and footer indexing; joins are filler/header-ranked and only `validity == verified` stitches are written (`source="bgc"`, `fragments[]`, `checks`, `confidence`, `alternatives`). D1 profile: recall 4/6 = 66.7% [30.0%, 90.3%], splice 0/4, junk FP 0/24; X15 recalls 3/3 larger real files with no bait stitch | n-fragment and metadata/runlist-assisted reassembly remain unimplemented; the D1 sample has wide CIs and is development evidence only; not enabled by default, so published figures do not include this stage. |
| B-07 | **PARTIAL** | Salvage JPEG/PNG prefixes (R4a/R4b) with `validated_bytes` attached; H-21 rebuilds PDF xref/trailer, appends real JPEG/PNG footers, and requires revalidation before `repaired`; `sha256_original` distinguishes repaired bytes; repaired not counted as exact in the benchmark | No generic `--salvage` flag; D-47 records the explicit `[SHOULD]` skip and the limit of PDF reconstruction. |
| B-08 | **PARTIAL** | `--manifest`, `--hash-image`, `--case-id`/`--examiner`; complete exit codes; README has a flag table | No progress/ETA for the carve phase, `--resume`, `--dry-run`, `carvex verify/check/extract`, `--json-lines`, `completions`, man page, README==`--help` test. |
| B-09 | **OPEN** | Per-file `notes[]` exists and is used when the parser skips unsupported parts | No fixtures+tests for individual variants (APNG, xref stream, `/Encrypt`, trailer after EOI…); no format-support table in docs. |
| B-10 | **OPEN** | Format registry (A-10) is an existing foundation | No `--signatures sig.toml`; no ZIP/OOXML. |
| B-11 | **OPEN** | — | No `posix_fadvise`, no `--io-threads`, no re-measurement of time-vs-size. |
| C-01 | **PARTIAL** | Frozen protocol at `protocol-v1`, builder fix disclosed at `protocol-v1.1`; 20-seed lockbox run once into `bench/results/lockbox-v1` with dataset/result checksums verified (`docs/VALIDATION.md §6`) | D1-style profile only; D2–D7 remain OPEN. Results are single-use evidence and were not used for tuning. |
| C-02 | **OPEN** | `bench/lockbox.py` supports `--seeds/--profile` for the 20-seed D1 lockbox; the dev corpus remains D1 (30 files × 4 images) | No D2–D7 profiles and no fragmentation/overwrite self-check. |
| C-03 | **PARTIAL** | Ground truth from `bench/corpus/manifest.json` (generated by Pillow, independent of CarveX); runner checks SHA-256 before each run; metrics do not use the CarveX validator (ADR-007) | No automated "viewable" PDF check with pypdf/qpdf (`qpdf`/`pdftoppm` missing → SKIPPED); no pixel comparison. |
| C-04 | **PARTIAL** | File-level exact + usable, one-to-one metric; FP classification kept/unknown; **F-17 verified by re-run**: Foremost 44/44 unknown = 40 JPEGs embedded in PDFs + 4 early-truncated incremental PDFs; Scalpel 54 = 40 + 4 + 10 child spans (8 EXIF thumbnails + 2 four-byte `FFD8FFD9` fragments) — matching the guide's "40 of 44 / 40 of 54" | No block-level `byte_recall`/`splice_rate`; classification not automated in the runner (still merges `unknown_fp`). |
| C-05 | **PARTIAL** | REPORT abstract states Wilson 95% CI: 100/100 → [96.3%; 100%], 30/30 → [88.6%; 100%], with n and conditions; runner warns `metric_variance` | No McNemar/bootstrap; no CI for every number table. |
| C-06 | **PARTIAL** | Foremost/Scalpel/PhotoRec configurations stored in `bench/`; search-space confound documented (README, REPORT §6.4 item 9, F-16) | No baseline run with the same search space (unallocated blob); no ablation (validator, align, FS-aware). |
| C-07 | **PARTIAL** | Runner records host/versions/argv/seed, relative to the repo root; `--smoke`; all numbers drawn from `bench/results/final/results.json` | **No `render_docs.py`** to generate numbers into docs → figures still hand-written (with sources); no `bench/config.toml`/`--full`. |
| C-08 | **PARTIAL** | `docs/CFREDS.md §5`: actual RM#2 results (318 files, 276 hashes, 178 embedded OOXML, 0 PDF, CarveX 0.1.0 + hashed binary), explicitly stating these are not precision/recall | DFRWS 2006/2007 not run; 0.2.0 not re-run on CFReDS (noted in doc). |
| C-09 | **PARTIAL** | `bench/run_benchmark.py` has a timing/throughput table (median, `runtime_min/max`); full 4-image/3-rep 0.2.0 re-run completed with checksums verified (`docs/VALIDATION.md §7`) | No 1–4 GiB/sparse 8–16 GiB images; no cold cache + IQR ≥10 repetitions; no per-scenario table in `BENCHMARK.md`. |
| D-01 | **PARTIAL** | Removed absolute paths like `/home/…` from REPORT/CFREDS/README (`$CARVEX`, `$HOME/cfreds-data`); corrected "Pillow/ReportLab" → Pillow + internal PDF script; "100%" in abstract tied to type/n/CI/conditions; **16 numbered tables `Table 1…16`**; tool SHA-256s (foremost/scalpel/photorec) + explicit note that the benchmark binary hash was not recorded; §6.2 explains FPs with evidence (40/44, 40/54); README "Rust 1.74" → 1.85; "fair" conclusion qualified; confound F-16 added to §6.4; Windows `.exe` noted as untested (README + REPORT §6.4) | Figures still hand-written from `results.json` (see C-07). |
| D-02 | **FIXED** | REPORT §1.4 Related Work: 5-branch taxonomy + feature comparison table; references [11]–[16] **DOI/publisher pages opened before insertion** (Garfinkel 10.1016/j.diin.2007.06.017; Richard–Roussev DFRWS 2005 dfrws.org; Pal–Memon 10.1109/MSP.2008.931081; Sencar–Memon 10.1016/j.diin.2009.06.007; Cohen 10.1016/j.diin.2007.10.001; Poisel–Tjoa 10.1109/ARES.2013.62) | Not every claim is cited (some left as "no source"). |
| D-03 | **PARTIAL** | `CHANGELOG.md` (0.2.0, Keep a Changelog, *Changed*/*Behavior changes* sections); `SECURITY.md`; `CONTRIBUTING.md`; `docs/ARCHITECTURE.md`; `docs/DECISIONS.md` (D-01…D-58); `docs/DEPENDENCIES.md`; `docs/REVIEW_RESPONSE.md` (this document); `docs/REPORT.md`; `docs/VALIDATION.md` | Earlier CI run succeeded (`44548142275`); H-30 run `45035152791` at HEAD failed (8 success / 18 failure); no release job or signing is claimed; packaged Windows binary not executed locally (A-12). H-34 quality work is implemented in commit f38a2ed. |

## Harness findings (H-xx)

This table mirrors `docs/PROGRESS.md` so every harness finding has exactly one
status row in each reconciliation document.

| ID | Pri | Repro (date, build) | Status | Commit | Test(s) | Before → After / evidence |
|----|----|---------------------|--------|--------|---------|---------------------------|
| H-01 | HIGH | 2026-11-21 source | BLOCKED | - | docs/guide.md (missing) | User must supply the 924-line guide with the expected sha256; no resolution is claimed. |
| H-02 | HIGH | 2026-11-21 source | CLOSED | 7129173 | scripts/gate.sh; scripts/check-msrv.sh | Clean-checkout gate made SKIP-safe; local G0-G9 now runs without root. |
| H-03 | HIGH | 2026-11-21 source | CLOSED | bbef4bd | scripts/review_repro.py; tests/regress_e01_safety.rs | X01-X24 controls and Rust regressions completed; red baseline retained. |
| H-04 | MED | 2026-11-21 source | PARTIAL | 4d0a6fd | dist/README.md; scripts/sbom.py | Identity metadata fixed; final rebuild/execution/checksums deferred to the approved release. |
| H-05 | CRIT | 2026-11-21 source | CLOSED | 873ea35 | tests/png_validator_budget.rs; scripts/review_repro.py X02 | X02a 0.009 s / 8.8 MiB RSS; X02b rejected at IHDR. |
| H-06 | CRIT | 2026-11-21 source | CLOSED | 3534717 | tests/jpeg_frame_budget.rs; scripts/review_repro.py X03 | SOF bombs become `unverified` within the decoder budget. |
| H-07 | HIGH | 2026-11-21 source | CLOSED | fa18436 | scripts/review_repro.py X04 | 23-26 s → <1 s after single-pass EOF indexing. |
| H-08 | CRIT | 2026-11-21 source | CLOSED | e69e68b | tests/memory_budget.rs; scripts/review_repro.py X01 | `--max-memory` bounds working buffers without losing finished runs. |
| H-09 | HIGH | 2026-11-21 source | CLOSED | 48deab5 | tests/work_budgets.rs; scripts/review_repro.py X05 | Four work limits emit coded warnings, reports and exit 3. |
| H-10 | HIGH | 2026-11-21 source | CLOSED | 5f794a4 | tests/panic_containment.rs; scripts/review_repro.py X06 | Candidate and join panics are contained with `CANDIDATE_PANIC`. |
| H-11 | CRIT | 2026-11-21 source | CLOSED | 4b86f2d | tests/regress_e01_safety.rs; scripts/review_repro.py X07 | Hostile chunk size is a typed error rather than SIGABRT. |
| H-12 | CRIT | 2026-11-21 source | CLOSED | 080b76a | tests/regress_vmdk_safety.rs; scripts/review_repro.py X08 | Grain tables are bounded; zero extents read as zeros. |
| H-13 | HIGH | 2026-11-21 source | CLOSED | 080b76a | tests/regress_vmdk_safety.rs; scripts/review_repro.py X09 | VMDK descriptors are confined to the image. |
| H-14 | HIGH | 2026-11-22 source | CLOSED | e2d3f1d | fuzz/fuzz_targets/fuzz_e01.rs; fuzz/fuzz_targets/fuzz_vmdk.rs; scripts/fuzz.sh | Six parser fuzz targets added; 13 targets ran clean locally. |
| H-15 | MED | 2026-11-22 source / 0.2.0 dist | CLOSED (musl/dist-only) | 1c28e2e | scripts/review_repro.py X23; scripts/perf_gate.py | Slowness measured only in the stale musl dist; no allocator change. |
| H-16 | CRIT | 2026-11-22 source | CLOSED | 3f47e51 | tests/validity_model.rs; tests/cli_contract.rs | Authoritative `validity` plus `checks[]` added. |
| H-17 | HIGH | 2026-11-22 source | CLOSED | 905c9c0 | tests/png_crc_validity.rs; src/signatures/png.rs | Bad CRC lowers validity instead of being presented as decoded. |
| H-18 | HIGH | 2026-11-22 source | CLOSED | 01ea6b3 | src/signatures/jpeg.rs; scripts/jpeg_splice_property.py | Splice acceptance rate reduced; residual accepted cases disclosed. |
| H-19 | HIGH | 2026-11-22 source | CLOSED | cb591a3 | tests/pdf_structure_validation.rs; scripts/review_repro.py X11 | PDF stage-2 structural validation added. |
| H-20 | MED | 2026-11-21 source | CLOSED | cee916a | scripts/review_repro.py X13 | EOL variants normalised before the `%%EOF` check. |
| H-21 | HIGH | 2026-11-22 source | CLOSED | 3b0c15c | tests/repair_validation.rs; scripts/review_repro.py X14 | Repairs are revalidated before the `repaired` label. |
| H-22 | CRIT | 2026-11-22 in-memory fixture | CLOSED | d7d991e | tests/regress_e01_safety.rs; tests/image/mod.rs | Read-error policy, unreadable ranges and abort partial report added. |
| H-23 | MED | 2026-11-21 source | CLOSED | 080b76a | tests/regress_vmdk_safety.rs | Missing VMDK grain table reads as zeros. |
| H-24 | MED | 2026-11-21 source | CLOSED | 56ee94a | scripts/review_repro.py X18 | Container auto-detection validates footer/descriptor evidence. |
| H-25 | LOW | 2026-11-22 source | CLOSED | e65f0f2 | tests/evidence_formats.rs; scripts/review_repro.py X22 | Container identity fields added to schema 2. |
| H-26 | MED | 2026-11-22 source | CLOSED | eb649f1 | tests/volume_parse_errors.rs; scripts/review_repro.py X19 | Parse failures now surface `VOLUME_PARSE_FAILED` and exit 3. |
| H-27 | MED | 2026-11-22 source | CLOSED | 5ebe851 | tests/region_coverage.rs; scripts/review_repro.py X20 | `regions[]` follows the scanner's coverage plan. |
| H-28 | MED | 2026-11-22 source | CLOSED | c14060d | scripts/review_repro.py X21 | Cooperative SIGINT writes an interrupted partial report and exits 4. |
| H-29 | HIGH | 2026-11-22 source | CLOSED | b902eed | tests/fragment_recovery.rs; scripts/review_repro.py X15 | Work-unit-bounded B-06 stitching with verified-only joins. |
| H-30 | HIGH | 2026-11-22 source/CI | CLOSED (local; CI failed) | b9c40d3 | scripts/gate.sh; scripts/render_docs.py; .github/workflows/ci.yml | Local G0-G9 green and gate efficacy proven; remote run `45035152791` failed (8/18 jobs). |
| H-31 | MED | 2026-11-22 source/docs | CLOSED | efd6d9f | scripts/check_status.py; scripts/gate.sh; .github/workflows/ci.yml | Status labels reconciled; checker is green and fails on a deliberately wrong label. |
| H-32 | MED | 2026-11-22 docs | CLOSED | bd9ebd0 | bench/results/EVIDENCE.md; bench/results/EVIDENCE.sha256; bench/lockbox.log | Manifest and prepared bundle cover the cited artefacts; historical unbacked figures are labelled author-reported. |
| H-33 | MED | 2026-11-22 docs | CLOSED | 240ef42 | README.md; docs/DECISIONS.md; docs/REPORT.md; docs/baseline/h33_static_red.txt | Speed sentence removed; Pillow wording corrected; `fuzz/` tree entry added; NOT-REPRODUCED items recorded with grep output; frozen results unchanged. |
| H-34 | LOW | 2026-11-22 source | CLOSED | f38a2ed | clippy.toml; src/lib.rs; src/scan.rs; src/cli.rs; docs/baseline/h34_static_red.txt; docs/baseline/h34_static_green.txt | 15 non-test functions were over 120 lines and the combined lint inventory had 698 warnings. Non-test functions are now ≤120 lines, public API docs are complete and pedantic stage 1 is clean; one test-only allow is justified. No behavior or output change. |

## Cross-reference of old findings (F-01…F-21) → new IDs

Per guide Appendix D (`v2:F-xx` → new ID):

| Old finding | New ID | Old finding | New ID |
|---|---|---|---|
| F-01 | A-01 | F-12 | B-06 |
| F-02 | A-02 | F-13 | A-14 |
| F-03 | A-03 | F-14 | A-10 |
| F-04 | A-04 | F-15 | A-11, A-12 |
| F-05 | B-07 | F-16 | C-06 |
| F-06 | A-06 | F-17 | C-04, D-01 |
| F-07 | A-05 | F-18 | C-02 |
| F-08 | A-07, B-02 | F-19 | C-06 |
| F-09 | A-08, B-03, B-04 | F-20 | C-04, C-09 |
| F-10 | B-01 | F-21 | C-08, D-01, D-02 |
| F-11 | B-05 | | |

For the status of each old finding, see the table at the top of this document in previous versions;
the 38-ID table above is the current source of truth.

## Details of fixed items

### A-01 / A-02 — PDF (F-01, F-02)

- `document_boundaries()` only treats `%PDF-` as a boundary when it follows `%%EOF`
  (ignoring whitespace). A header inside a stream/attachment does not split the
  outer document.
- `analyze()` in `smart` mode returns `Need::More` when `%%EOF` touches the end
  of the buffer, when the tail is only ASCII whitespace to the end of the window,
  or when the tail contains `xref/startxref/trailer/ obj`. NUL/JPEG/garbage after
  `%%EOF` does not trigger an additional read.
- Tests: R1, R2, R3; 4 related PDF unit tests.

### A-03 — No panics (F-03)

- `#![forbid(unsafe_code)]` crate-wide; `fs/*` + `signatures/*` deny
  `indexing_slicing`, `unwrap_used`, `expect_used` (test modules have `allow` with
  justification).
- `fs::detect_guarded` wraps `catch_unwind`; CLI auto-mode falls back to raw + exit 3.
- Release: unwind + `overflow-checks = true`.
- Fuzz targets: `jpeg`, `png`, `pdf`, `fat32_boot`, `ntfs_boot`, `ntfs_mft`,
  `partition_table`; seeds from corpus/benchmark. `scripts/fuzz.sh 60` ran each
  target for 60 s on nightly: 18,520,025 executions, no crash, timeout, OOM or
  artifact. Logs are in `bench/results/fuzz/`; see `docs/VALIDATION.md §4`.

### A-04 — Deep validation (F-04)

- `--validate decode`: JPEG baseline uses custom Huffman/MCU; PNG inflates IDAT
  and checks filter/row; PDF stage 2 checks xref tables/streams, object headers,
  `/Root` → `/Catalog`, `/Size`, `/Prev` and stream lengths without rendering
  page content.
- Report adds `validation`, `validated_bytes`; H-16 adds authoritative
  `validity` and structured `checks[]` while preserving legacy field meanings.
- Measured on `all-deleted_fat32.img`: structure 0.11–0.20 s; decode 0.49–1.10 s;
  30/30 `decoded`.
- R4c: a JPEG with an appended tail is `partial`, not `decoded`.

### A-06 / A-05 — Resources (F-06, F-07)

- R5: 25,811 fake headers in 64 MiB, 3 measurements 0.48/0.52/0.79 s.
- R6: zero-tail trim + 16 MiB cap; no more 64 MiB output.
- Parallel carve, deterministic; test comparing JSON + byte output between 1 and
  4 threads.

### A-07 — No silent data misses (F-08)

- `detect_all()` returns all supported volumes + `gaps` + `unsupported` + `errors`;
  `PartitionSel{Largest, All, Index(n)}` via `--partition largest|all|N` (default
  `all`); by default the raw scan also covers areas outside volumes, and
  `--no-gaps` skips them (the `--include-gaps` flag remains accepted).
- Report schema 2 adds `volumes[]`, `regions[]`
  (`kind = free|allocated|gap|unsupported`, `offset`, `len`, `scanned`,
  `partition_index`) and `warnings[]` as objects `{code, message}`.
- Coded warnings `VOLUME_NOT_SCANNED`, `GAP_NOT_SCANNED`, `VOLUME_UNSUPPORTED`,
  `FS_DETECTION_FAILED`, `IMAGE_MODIFIED` ⇒ **exit 3**; `--partition all` writes
  to `part1/`, `part2/`, …; `--no-fs`/`--offset` are not counted as regions.
- Evidence: `docs/baseline/A-07_v0.1.0.txt` (red),
  `docs/baseline/A-07_green_v0.2.0.txt` (step 1 green) and
  `docs/baseline/A-07_green_v0.2.0_step2.txt` (step 2 green) on fixture S1
  (`target/a07/s1.img`, 131,072,000 B); decisions D-19…D-23 in
  `docs/DECISIONS.md`.

### A-14 / A-10 — Report and architecture (F-13, F-14)

- Report schema 2; atomic write; `--report-schema 1` compatible.
- `FsInfo.data_start`: FAT32 clusters start at 2, NTFS at 0; test checks cluster 3.
- Pipeline split into functions; nested decided before writing; outputs numbered
  after sorting by offset.
- H-34: `clippy.toml` sets `too-many-lines-threshold = 120`. `cli::scan`,
  `fs::{fat32,ntfs}::parse`, `signatures::{jpeg,png,pdf}::analyze`,
  `pdf_validate`, and the long `scan.rs` passes were split into pure helpers;
  `FragmentScanState` captures the fragment deadline at function entry, so the
  wall-clock safety net still includes head-classification reads. Public API
  items are documented, pedantic stage 1 is enabled explicitly, and every
  retained `too_many_arguments` allow carries a justification comment. The only
  `too_many_lines` allow is on one test and is justified in place; D-58 records
  the deferred numeric-cast and API-ergonomics stages.

### A-07 / H-27 — faithful coverage regions

- Red evidence: `tests/region_coverage.rs` failed before the fix with
  `region 0 starts at 1048576, expected 0`, an `info --json` row calling the
  whole FAT32 volume free, and clap rejecting `--max-regions`. X20 had already
  shown overlapping rows whose scanned lengths exceeded `bytes_scanned`.
- `scan::plan_extents` is now the single source of read extents and feeds
  `VolumeSet::regions_with_plan`. Regions are the merged free extents, their
  allocated complement, gaps and unsupported partitions: sorted, disjoint and
  covering the entire logical image. Holes in malformed metadata become
  unscanned gap rows.
- Every scanned region carries the `extent_id` of the scanner extent that reads
  it. `--max-regions` (default 4096, `0` = unlimited) summarises adjacent rows
  sharing a `scanned` state; `summarized: true` and the additive `mixed` kind
  record the merge. Coverage and `Σ len(scanned regions) == bytes_scanned`
  take precedence over the soft cap.
- X20 now asserts full-image coverage, all additive fields, exact
  report/stats byte totals, real free extents and a raw-mode cap run; it was
  removed from `scripts/repro_xfail.txt`. No carving behaviour, score,
  emission, output name, CSV column or schema-1 field changed.

### A-08 / H-22 — read-error policy

- Red evidence: `abort_writes_partial_report_and_exits_one` failed on the
  unmodified scanner because exit 1 was reached with no report file. The fixed
  scanner writes a schema-2 report with `READ_ABORTED`, `UNREADABLE_REGION` and
  the recorded 4,096-byte E01 chunk before returning the original error.
- E01 checksum/inflate failures and raw/split read errors share the policy:
  `zero-fill` (default) records the region and keeps unreadable bytes out of
  `verified` outputs; `skip` drops overlapping candidates; `abort` exits 1 with
  a partial report. Raw/split reads are retried per 512-byte sector, verified by
  two failing-mock `ImageSource` unit tests.
- Residual: no libewf-made E01, `.E02`, real device or Windows control was
  available; VHD/VMDK retain their container-specific error semantics.

### A-09 / H-26 — unparsable volumes

- Red evidence: the multi-volume X19-style test exited 0 with no warning, the
  single-volume fallback said "no FAT32/NTFS filesystem found" despite a
  recognised NTFS boot sector, and `info` gave the same generic message.
- `VolumeSet.errors` now carries partition index, byte offset and the parser
  reason. Each failure becomes a `VOLUME_PARSE_FAILED` warning in the report and
  on stderr; the code is incomplete, so the scan exits 3 without aborting other
  healthy volumes.
- Each `regions[]` row gains `may_contain_allocated`. It is true for the
  `unsupported` fallback region of a parse failure, declaring that raw carving
  there mixes allocated and deleted bytes.
- `info` still exits 1 when no volume can be selected, but now names the
  boot-sector offset and failed metadata (for example `$MFT unreadable`).
- X19 was strengthened to assert exit 3, the warning, console output, the
  fallback region flag and a separate single-volume `info` cause; it was
  removed from `scripts/repro_xfail.txt`.

### A-14 / H-25 — container identity

- Red evidence: compressed-E01, VMDK and stdin report tests failed before the
  fix with `left: Null` for `format` and each new `input` field. X22 originally
  only showed that stdin had `unchanged=true` with a null mtime.
- Schema 2 now adds `logical_size`, `logical_size_after`, `physical_size`,
  `physical_size_after`, `format` and `files[]` under `input`. Legacy `size`
  and `size_after` keep their 0.2.0 meanings; schema 1 removes the whole
  `input` object.
- Stdin reports equal logical/physical spool sizes, no backing files, and
  `unchanged: null`. X22 exercises stdin, a compressed E01 and a
  monolithicSparse VMDK and was removed from the xfail list.
- Residual: no real libewf-made multi-segment E01/.E02 fixture was available;
  `input.files[]` lists backing files, not byte-level grain/extent maps.

### A-13 / H-28 — cooperative interruption

- Red evidence: the strengthened X21 control killed the unmodified release
  binary with `exit -2 after SIGINT (expected 4)`; no report was written.
- `ctrlc` installs one process-wide handler. The first SIGINT/Ctrl-C latches a
  shared state polled by the candidate-count, carve, fragment and write stages;
  the second exits 130 immediately. Completed outputs stay because writes are
  finished and atomically renamed before the next stop check.
- An interrupted scan writes a schema-2 report with `interrupted: true` and an
  `INTERRUPTED` warning and exits 4. Schema 1 removes the new field. The
  `abort` partial-report path records a signal that arrived before it stopped.
- X21 now asserts exit 4, schema 2, the flag, the warning and no `.tmp` files;
  it was removed from `scripts/repro_xfail.txt`. A direct two-signal control
  exited 130 without a report. No packaged Windows binary was executed.

### B-07 / H-21 — repair validation

- Red evidence: `tests/repair_validation.rs` failed on the unmodified scanner
  because CSV had no `sha256_original` column; the invalid-catalog PDF was
  also written as `repaired`. X14 reproduced the real-corpus failure where
  pypdf could not open the statically repaired PDF.
- Response: PDF repair rebuilds a bounded classic xref/trailer, JPEG/PNG use
  their real footers, and every repaired byte stream is revalidated before the
  label changes. Failed repair leaves bytes unchanged and emits the exact note
  `repair attempted, not validated`. `sha256_original` is additive in JSON and
  CSV, stripped by schema 1, and absent when output bytes are unchanged.
- Residual: no generic `--salvage`; PDF reconstruction is limited to surviving
  object headers and a resolvable catalog, and repaired output is never exact.

### A-11 / A-12 — Test/CI (F-15)

- **134 tests** in the current H-29 gate (43 unit + 13 CLI contract +
  7 evidence formats + 5 fragment recovery + 8 FS detection + 2 JPEG frame
  budget + 4 memory budget + 3 panic containment + 2 PNG CRC validity +
  4 PNG decode budget + 2 PNG validator budget + 8 PDF structure validation +
  5 property + 4 E01 safety + 4 VMDK safety + 3 repair validation +
  6 integration + 1 validity model + 4 work budget + 3 volume parse errors +
  3 region coverage); `scripts/gate.sh` runs fmt, clippy, test, doc,
  actionlint, MSRV, release, repro, smoke and supply-chain checks →
  **GATE GREEN 2026-11-22**, re-run after B-04, B-06, R-01/R-02, H-14, H-16,
  H-17, H-18, H-19, H-21, H-22, H-25, H-26, H-27, H-28 and H-29.
- CI 3 OS + MSRV + security + smoke + nightly fuzz matrix is declared in
  `.github/workflows/ci.yml`; after H-14 the fuzz matrix lists all 13 targets
  and after H-30 it runs on push/PR alongside the full gate, repro assertions,
  packaged-binary smokes, 70% coverage and SBOM. Real run `44548142275` on
  `42376a9` succeeded with every job green, but its push-event fuzz matrix was
  skipped by its schedule/workflow_dispatch trigger. H-30 pushed run
  `45035152791` on `d9dcac3` completed **failure**: 8 jobs succeeded and 18
  failed (all 13 fuzz targets, repro assertions, release gate, Windows tests,
  Windows packaged smoke and Linux musl packaged smoke). The log endpoint
  returned `403 Must have admin rights to Repository.`, so remote failure
  causes are not observed or guessed; see `docs/VALIDATION.md §3`. No green CI
  run at HEAD is claimed.
- Local `cargo audit` (0 vulnerabilities in 91 crates against 1555 advisories)
  and `cargo deny check` (advisories/bans/licenses/sources ok) both pass; the
  four unused `license-not-encountered` warnings are informational.
  `actionlint 1.7.12` passes locally and in CI.
- After R-01/R-02, `scripts/coverage.sh 60` measured 4456/6391 lines =
  **69.72%** (post-B-06: 4111/6001 = 68.51%; post-B-04: 3802/5633 = 67.50%;
  pre-B-04: 2859/4508 = 63.42%); the CI/local floor is honestly 60%, not 80%.
  Full evidence: `docs/VALIDATION.md §§2–5`.

### C-04 / D-01 — FP classification (F-17) — **verified**

- Re-ran the exact benchmark commands for Foremost/Scalpel on all 4 images; total
  `unknown_fp` matches `results.json`: **Foremost 44, Scalpel 54**.
- Classification (comparing output bytes with the corpus): **Foremost 40/44** are
  JPEG runs inside the corpus PDFs, **4/44** are early-truncated incremental PDFs
  (`pdf_04_incremental`, truncated at the first `%%EOF`, 6,450 B). **Scalpel**
  likewise **40/54** JPEGs inside PDFs + **4/54** early-truncated PDFs + **10/54**
  child JPEG spans (8 spans from the EXIF thumbnail
  `jpeg_08_exif_thumb_900x700.jpg` × 4 images, 2 four-byte `FFD8 FFD9` fragments
  in the NTFS image) — matching guide D-01's "40 of 44 / 40 of 54" finding.
- Recorded in `docs/REPORT.md §6.2`; classification not yet automated in the runner.

### D-01 / D-02 — Documentation (F-21)

- `docs/CFREDS.md`: actual 0.1.0 results (318 files, 276 hashes, 178 embedded
  OOXML, 0 PDF), explicitly stating these are not a precision/recall benchmark;
  `$CARVEX`/`$HOME/cfreds-data` paths.
- `docs/REPORT.md`: Related Work §1.4 with DOI/URLs opened before insertion;
  corrected corpus description (Pillow + internal PDF script, not ReportLab);
  §6.2 explains FPs with counts 40/44 and 40/54; 16 numbered tables; tool
  SHA-256s; abstract "100%" tied to n/CI/conditions; confound F-16 in §6.4.
- `README.md`: MSRV 1.85; new flag table (`--partition`, `--include-gaps`,
  `--no-gaps`, `--validate`, `--align`, `--report-schema`…); section
  "Multi-volume scanning, warnings, and exit codes"; Windows limitations.
- All docs, comments and reports are English-only (D-24); the technical report
  is now `docs/REPORT.md` with all references updated. No CLI, JSON/CSV,
  warning-code or benchmark value was changed.
- Figures still hand-written from `bench/results/final/results.json` — no
  `render_docs.py` (C-07), recorded as OPEN.

## What is not claimed

- No claim of exhaustive or "clean" fuzzing: 7 targets ran for 60 s each
  (18,520,025 executions, no crashes); no broader guarantee is made
  (`docs/VALIDATION.md §4`).
- No claim of ≥80% coverage: post-R-01/R-02 line coverage is 69.72% with a 60%
  floor.
- No claim that `--max-memory` caps process RSS or that JPEG/PDF decoder
  internals are fully instrumented; the explicit PNG decode bound covers the
  PNG path only.
- No claim of universal container support: the B-04 readers cover only the
  variants listed above and reject unsupported formats explicitly. The only
  successful GitHub CI run observed here predates B-04, B-06 and R-01/R-02;
  the H-30 run at HEAD failed remotely.
- B-06 recall and `splice_rate` are now measured on a small D1-style
  development profile (4/6 recall, 0/4 splices, 0/24 junk FP with Wilson 95%
  intervals in `bench/results/d1_fragment_profile.json`) and a 3/3 X15 real-file
  control. These are not lockbox results or published benchmark claims; no
  n-fragment path is claimed.
- No claim of a "fair" benchmark per C-06: the same search space has not been run
  for every tool; no ablations or advanced statistics are claimed.
- No claim of lockbox generalization: 20 seeds were run exactly once on the
  D1-style profile, D2–D7 remain OPEN, and no result was used for tuning.
- No claim that the packaged Windows executable was executed locally: the old
  `dist/` binary was format-checked only. H-30 added a CI job that builds and
  executes a Windows release exe, but that job failed in the observed H-30 run
  and its logs were not available; no packaged Windows execution is claimed.
- No claim of a green CI run at HEAD: run `44548142275` succeeded on an older
  commit with the fuzz matrix skipped by design, while H-30 run
  `45035152791` on `d9dcac3` failed (8 success / 18 failure).
- Historical fuzz execution counts, early audit/coverage percentages and
  remote failure causes without a retained raw artefact are labelled
  **author-reported** in `docs/VALIDATION.md` and
  `bench/results/EVIDENCE.md`; they are not treated as independently observed
  evidence.
- No claim of new benchmark numbers for 0.2.0: published figures are from 0.1.0.
  The full 3-repetition re-run is validation evidence only; all 20 rows are
  metric-identical to the frozen `bench/results/final/results.json`.
- No claim that the protocol `viewable` PDF metric was computed: `qpdf` and
  `pdftoppm` were unavailable.
