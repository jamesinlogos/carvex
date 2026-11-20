# Cross-reference by ID of the unified guide (A-01…D-03)

Statuses use the guide's definitions exactly:

- **FIXED** — fixed, with tests/measurements proving it.
- **MITIGATED** — risk reduced but not fully resolved; the remainder is stated explicitly.
- **OPEN** — not done; no claim of resolution.
- **WONTFIX** — not doing, with rationale (not used in this table).

Re-run evidence: `scripts/review_repro.py --assert` → **10/10 PASS**, 0 XPASS
(`docs/baseline/repro_v0.1.0.txt` is the red baseline of 0.1.0: 8/9 FAIL);
`scripts/gate.sh` → **GATE GREEN** (fmt, clippy, **70 tests**, doc, MSRV,
release, repro, bench `--smoke`). The gate was re-run after B-04; the recorded
GitHub Actions runs predate B-04.

## Summary table of 38 IDs (guide §6)

| ID | Status | Evidence / test | Remaining notes |
|----|------------|-------------------|-----------------|
| A-01 | **FIXED** | R1, R2 PASS; unit `early_eof_requests_more_when_incremental_update_follows`, `smart_selects_last_incremental_revision` | No 2–3 MiB linearized PDF compliant with Annex F has been built; no fixtures for xref stream/ObjStm. |
| A-02 | **FIXED** | R3 PASS; `embedded_pdf_is_not_a_document_boundary`, `adjacent_pdfs_are_split_at_the_second_header`; nested suppression before writing | — |
| A-03 | **MITIGATED** | `#![forbid(unsafe_code)]`; deny `indexing_slicing`/`unwrap`/`expect` in `fs/*`+`signatures/*`; `catch_unwind` around `fs::detect`; release unwind + `overflow-checks`; R7 PASS; 7 fuzz targets run 60 s each (18,520,025 executions, no crashes/artifacts) | Fuzzing is not exhaustive; no claim beyond the executed 7×60 s run (`docs/VALIDATION.md §4`). |
| A-04 | **FIXED** | `--validate structure\|decode` (default `structure` per A-04's decision rule); `validation`+`validated_bytes`; R4c PASS (JPEG with appended tail → `partial`); measured: structure 0.11–0.20 s vs decode 0.49–1.10 s, 30/30 `decoded` | Field name kept as `validation` (0.1.0) instead of `validity` — see D-22; decode supports only baseline sequential; progressive → `unverified`. |
| A-05 | **FIXED** | R6 PASS; zero-tail trim + `--max-truncated-size 16M` (default); salvage JPEG/PNG prefixes (R4a/R4b) with `status=truncated` | No `--salvage`/`--partial-min-fraction` flag (B-07). |
| A-06 | **FIXED** | R5 PASS: 0.48–0.79 s for 25,811 fake headers/64 MiB (target ≤1 s); `--max-memory`/`--max-size` capped; parallel carvex, deterministic (1 vs 4 thread test) | Measured on warm cache, single machine. |
| A-07 | **FIXED** (step 1 + step 2) | Red baseline `docs/baseline/A-07_v0.1.0.txt` (exit 0, silent misses); step 1 green baseline `docs/baseline/A-07_green_v0.2.0.txt` (30 files + warning + exit 3); step 2 green baseline `docs/baseline/A-07_green_v0.2.0_step2.txt` (default 60 files = 30+30, exit 0, `volumes[]`/`regions[]` both `scanned=true`). `detect_all()` → `VolumeSet`; `warnings[]` coded; `--partition largest --no-gaps` restores old behavior (D-23) | Step 2 was approved by the user (D-23); STOP-AND-ASK §3.5 resolved. Any RUN still missing evidence exits 3. |
| A-08 | **MITIGATED** | `trait ImageSource {len, read_at}` with the unchanged `RawFile` default (pread by offset); fallback `seek(End)` when `metadata().len()==0`; B-04 adds read-only split raw, E01, VHD, VMDK and stdin sources; `--hash-image` streaming | No `BlockDevice`/`--sector-size`/`--on-read-error`/`unreadable_regions[]`; `info /dev/loopN` not tested; Windows multithread safety not verified. |
| A-09 | **MITIGATED** | Fixed NTFS sector off-by-one (`field+1` clamped to `available`, D-21, unit test `volume_sector_count_includes_the_backup_boot_sector`); all FS parse errors → `FS_DETECTION_FAILED` in `warnings[]` | No `$ATTRIBUTE_LIST`, multi-extent `$MFT`, `$MFTMirr` fallback, backup boot fallback, FAT #2/backup BPB yet. |
| A-10 | **FIXED** | Lib crate + thin `main.rs`; pipeline `plan_extents→build_tasks→scan_task→carve_candidates→postfilter→write_outputs`; format registry; no more `allow(too_many_arguments)`; `cargo doc -D warnings` green; deterministic tests | No full public `#![warn(missing_docs)]`; pedantic cleanup not done by group. |
| A-11 | **MITIGATED** | **70 tests** (34 unit + 12 `cli_contract` + 5 `evidence_formats` + 8 `fs_detect` + 5 `properties` + 6 `scan_integration`); `scripts/review_repro.py --assert/--xfail` (10/10); red baseline `docs/baseline/repro_v0.1.0.txt`; every Critical/High finding has a red-before/green-after test; post-B-04 coverage 3802/5633 lines = 67.50% | No claim of ≥80% coverage; floor is honestly 60%. Integration tests are named `tests/*.rs` rather than `tests/regress_*.rs`. |
| A-12 | **MITIGATED** | MSRV **1.85** (`rust-version` + README corrected from "Rust 1.74"); `cargo +1.85 check --locked` in gate; `.github/workflows/ci.yml` (3 OS, MSRV, deny/audit, smoke, conditional nightly fuzz); `rustfmt.toml`/`clippy.toml`/`deny.toml`; dist 0.2.0 rebuild + `SHA256SUMS`; local `cargo audit` 0.23.0, `cargo deny` 0.21.0 and `actionlint` 1.7.12 pass; GitHub run `44548142275` on `42376a9` succeeded with all jobs green | The recorded GitHub runs predate B-04; no post-B-04 CI run is claimed. Packaged Windows `.exe` **not executed**; GitHub's push-event fuzz matrix was skipped; no SBOM/signatures; removing binaries from `dist/` is STOP-AND-ASK — kept unchanged. |
| A-13 | **MITIGATED** | Exit codes 0/1/2/3/4 documented (README) and CLI contract tests; coded warnings (`*_NOT_SCANNED` → exit 3, D-19); `RAW_SCANS_ALLOCATED` informational (D-20); short error messages with suggested actions | No `thiserror` types, no leveled logger/`--log-format json`, no `ProgressSink`/ETA for the carve phase, no partial report written on interruption (exit 4). |
| A-14 | **FIXED** | Schema v2: `schema_version`, `argv`, `options`, `tool_info`, `input` (SHA-256 before/after), ISO-8601, `case_id`, `examiner`, `warnings[]` (objects), `volumes[]`, `regions[]`, per-file actual `lba`/`cluster`/`partition_index`/`extent_id`/`allocated`/`source`/`fragments`; `docs/report.schema.json` + test `emitted_report_matches_the_published_schema`; `report_schema_one_has_the_v01_shape`; atomic write; `--manifest` | `unreadable_regions[]` belongs to A-08 (not present); field `validity` kept as `validation` (D-22); `tool_info.commit` may be empty when built outside git. |
| A-15 | **MITIGATED** | Caps on `--max-memory`/`--max-size`/`--max-truncated-size`; atomic write (temp+rename); `--force` for non-empty directory; nested does not write-then-delete | No rejection of output located inside the source image/source device; no `--overwrite` (force still does not overwrite existing files? — needs test); no free-space/`--max-output` check; symlink following not forbidden. |
| B-01 | **OPEN** | — | No reading of `0xE5` dir entries/not-in-use MFT; no `--mode metadata\|hybrid`, `original_name`/`mtime`. |
| B-02 | **MITIGATED** | MBR + GPT primary/backup; `--partition largest\|all\|N` (A-07); `info --json` lists `volumes[]` | No EBR chain (loop protection); still assumes 512-byte sectors for the partition table; no `--sector-size`. |
| B-03 | **OPEN** | — | No exFAT/FAT12/16 (CFReDS RM#1 is exFAT, so its metadata cannot be scanned). |
| B-04 | **MITIGATED** | Read-only sources in `src/image/` selected by `open_image()`/`--input-format`; five byte-for-byte fixture tests cover split raw, compressed and uncompressed E01, fixed and dynamic VHD, flat and sparse VMDK, and stdin report identity (D-29) | Recognized EWF2/Ex01, L01/Lx01, LVF, differencing VHD, ESX/COWD and compressed streamOptimized VMDK variants are rejected explicitly; encrypted E01 has no decryption and is not guaranteed to be detected before parsing. Block devices remain A-08; the raw default and frozen benchmark/figures are unchanged. |
| B-05 | **MITIGATED** | `--align none\|auto\|cluster\|sector` (default `none` preserves history); test `alignment_restricts_candidate_starts_without_changing_results_on_aligned_data`; golden D0 TP unchanged | No per-file `aligned` flag written to the report; no measurement of candidate/nested reduction on D5; not run as an ablation arm. |
| B-06 | **OPEN** | — | No BGC/`--max-gap`/fragment joining. |
| B-07 | **MITIGATED** | Salvage JPEG/PNG prefixes (R4a/R4b) with `validated_bytes` attached; `--repair` retains its old meaning; repaired not counted as exact in the benchmark | No `--salvage` flag; `repaired` does not yet require the validator to reopen the file (PDF xref rebuild). |
| B-08 | **MITIGATED** | `--manifest`, `--hash-image`, `--case-id`/`--examiner`; complete exit codes; README has a flag table | No progress/ETA for the carve phase, `--resume`, `--dry-run`, `carvex verify/check/extract`, `--json-lines`, `completions`, man page, README==`--help` test. |
| B-09 | **OPEN** | Per-file `notes[]` exists and is used when the parser skips unsupported parts | No fixtures+tests for individual variants (APNG, xref stream, `/Encrypt`, trailer after EOI…); no format-support table in docs. |
| B-10 | **OPEN** | Format registry (A-10) is an existing foundation | No `--signatures sig.toml`; no ZIP/OOXML. |
| B-11 | **OPEN** | — | No `posix_fadvise`, no `--io-threads`, no re-measurement of time-vs-size. |
| C-01 | **MITIGATED** | Frozen protocol at `protocol-v1`, builder fix disclosed at `protocol-v1.1`; 20-seed lockbox run once into `bench/results/lockbox-v1` with dataset/result checksums verified (`docs/VALIDATION.md §6`) | D1-style profile only; D2–D7 remain OPEN. Results are single-use evidence and were not used for tuning. |
| C-02 | **OPEN** | `bench/lockbox.py` supports `--seeds/--profile` for the 20-seed D1 lockbox; the dev corpus remains D1 (30 files × 4 images) | No D2–D7 profiles and no fragmentation/overwrite self-check. |
| C-03 | **MITIGATED** | Ground truth from `bench/corpus/manifest.json` (generated by Pillow, independent of CarveX); runner checks SHA-256 before each run; metrics do not use the CarveX validator (ADR-007) | No automated "viewable" PDF check with pypdf/qpdf (`qpdf`/`pdftoppm` missing → SKIPPED); no pixel comparison. |
| C-04 | **MITIGATED** | File-level exact + usable, one-to-one metric; FP classification kept/unknown; **F-17 verified by re-run**: Foremost 44/44 unknown = 40 JPEGs embedded in PDFs + 4 early-truncated incremental PDFs; Scalpel 54 = 40 + 4 + 10 child spans (8 EXIF thumbnails + 2 four-byte `FFD8FFD9` fragments) — matching the guide's "40 of 44 / 40 of 54" | No block-level `byte_recall`/`splice_rate`; classification not automated in the runner (still merges `unknown_fp`). |
| C-05 | **MITIGATED** | REPORT abstract states Wilson 95% CI: 100/100 → [96.3%; 100%], 30/30 → [88.6%; 100%], with n and conditions; runner warns `metric_variance` | No McNemar/bootstrap; no CI for every number table. |
| C-06 | **MITIGATED** | Foremost/Scalpel/PhotoRec configurations stored in `bench/`; search-space confound documented (README, REPORT §6.4 item 9, F-16) | No baseline run with the same search space (unallocated blob); no ablation (validator, align, FS-aware). |
| C-07 | **MITIGATED** | Runner records host/versions/argv/seed, relative to the repo root; `--smoke`; all numbers drawn from `bench/results/final/results.json` | **No `render_docs.py`** to generate numbers into docs → figures still hand-written (with sources); no `bench/config.toml`/`--full`. |
| C-08 | **MITIGATED** | `docs/CFREDS.md §5`: actual RM#2 results (318 files, 276 hashes, 178 embedded OOXML, 0 PDF, CarveX 0.1.0 + hashed binary), explicitly stating these are not precision/recall | DFRWS 2006/2007 not run; 0.2.0 not re-run on CFReDS (noted in doc). |
| C-09 | **OPEN** | `bench/run_benchmark.py` has a timing/throughput table (median, `runtime_min/max`); full 4-image/3-rep 0.2.0 re-run completed with checksums verified (`docs/VALIDATION.md §7`) | No 1–4 GiB/sparse 8–16 GiB images; no cold cache + IQR ≥10 repetitions; no per-scenario table in `BENCHMARK.md`. |
| D-01 | **FIXED** | Removed absolute paths like `/home/…` from REPORT/CFREDS/README (`$CARVEX`, `$HOME/cfreds-data`); corrected "Pillow/ReportLab" → Pillow + internal PDF script; "100%" in abstract tied to type/n/CI/conditions; **16 numbered tables `Table 1…16`**; tool SHA-256s (foremost/scalpel/photorec) + explicit note that the benchmark binary hash was not recorded; §6.2 explains FPs with evidence (40/44, 40/54); README "Rust 1.74" → 1.85; "fair" conclusion qualified; confound F-16 added to §6.4; Windows `.exe` noted as untested (README + REPORT §6.4) | Figures still hand-written from `results.json` (see C-07). |
| D-02 | **FIXED** | REPORT §1.4 Related Work: 5-branch taxonomy + feature comparison table; references [11]–[16] **DOI/publisher pages opened before insertion** (Garfinkel 10.1016/j.diin.2007.06.017; Richard–Roussev DFRWS 2005 dfrws.org; Pal–Memon 10.1109/MSP.2008.931081; Sencar–Memon 10.1016/j.diin.2009.06.007; Cohen 10.1016/j.diin.2007.10.001; Poisel–Tjoa 10.1109/ARES.2013.62) | Not every claim is cited (some left as "no source"). |
| D-03 | **FIXED** | `CHANGELOG.md` (0.2.0, Keep a Changelog, *Changed*/*Behavior changes* sections); `SECURITY.md`; `CONTRIBUTING.md`; `docs/ARCHITECTURE.md`; `docs/DECISIONS.md` (D-01…D-29); `docs/DEPENDENCIES.md`; `docs/REVIEW_RESPONSE.md` (this document); `docs/REPORT.md`; `docs/VALIDATION.md` | Real CI run succeeded (`44548142275`); no release job, SBOM or signing is claimed; packaged Windows binary not executed (A-12). |

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
  and checks filter/row; PDF checks xref target/`/Root`.
- Report adds `validation`, `validated_bytes`.
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

### A-11 / A-12 — Test/CI (F-15)

- **70 tests** (34 unit + 12 CLI contract + 5 evidence formats + 8 FS detection
  + 5 property + 6 integration); `scripts/gate.sh` runs fmt, clippy, test, doc,
  actionlint, MSRV, release, repro, smoke and supply-chain checks → **GATE GREEN
  2026-11-19**, re-run after B-04.
- CI 3 OS + MSRV + security + smoke + conditional nightly fuzz matrix is
  declared in `.github/workflows/ci.yml`. Real run `44548142275` on `42376a9`
  succeeded with every job green; the push-event fuzz matrix was skipped by its
  schedule/workflow_dispatch trigger. These GitHub runs predate B-04; no
  post-B-04 CI result is claimed here.
- Local `cargo audit` (0 vulnerabilities in 91 crates) and `cargo deny check`
  (advisories/bans/licenses/sources ok) both pass; the four unused
  `license-not-encountered` warnings are informational. `actionlint 1.7.12`
  passes locally and in CI.
- After B-04, `scripts/coverage.sh 60` measured 3802/5633 lines = **67.50%**
  (pre-B-04: 2859/4508 = 63.42%); the CI/local floor is honestly 60%, not 80%.
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
- No claim of ≥80% coverage: post-B-04 line coverage is 67.50% with a 60% floor.
- No claim of universal container support: the B-04 readers cover only the
  variants listed above and reject unsupported formats explicitly. GitHub CI
  evidence predates B-04.
- No claim of a "fair" benchmark per C-06: the same search space has not been run
  for every tool; no ablations or advanced statistics are claimed.
- No claim of lockbox generalization: 20 seeds were run exactly once on the
  D1-style profile, D2–D7 remain OPEN, and no result was used for tuning.
- No claim that the packaged Windows executable was executed: it was built and
  format-checked only. GitHub's `test (windows-latest)` job ran the Rust test
  suite, not `dist/carvex-windows-x86_64.exe`.
- No claim of CI beyond the observed run `44548142275`: all its jobs succeeded,
  while the push-event fuzz matrix was skipped by design.
- No claim of new benchmark numbers for 0.2.0: published figures are from 0.1.0.
  The full 3-repetition re-run is validation evidence only; all 20 rows are
  metric-identical to the frozen `bench/results/final/results.json`.
- No claim that the protocol `viewable` PDF metric was computed: `qpdf` and
  `pdftoppm` were unavailable.
