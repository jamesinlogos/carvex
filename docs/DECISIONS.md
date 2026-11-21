# Technical decisions (Decision log)

Records decisions when the guide documentation and the actual repo differ, or
when several approaches are all valid. Each item states the context, the
decision, and the consequences.

## D-01. `guide.md` is truncated — use the guide-v1/v2/v3 set

- **Context:** `guide.md` stops at §3.5 but references §4–§12 (task ID, finding
  register, gate, phase). There is no content for the later sections.
- **Decision:** use `guide-v2.md` as the primary source for phase/task/acceptance;
  `guide-v1.md` and `guide-v3.md` for cross-checking findings and baseline. Do
  not extrapolate the missing parts.
- **Consequences:** every task ID in `docs/PROGRESS.md` comes from `guide-v2.md`.
  Parts that cannot be done are recorded as `OPEN`/`SKIPPED` instead of inventing
  new acceptance criteria.

## D-02. MSRV 1.85, no dependency downgrades

- **Context:** `Cargo.toml` 0.1.0 states `rust-version = "1.74"` but the lockfile
  pulls `clap 4.6.8`, `rayon 1.12.0`, `serde 1.0.229` — these versions do not
  guarantee a build on 1.74.
- **Decision:** raise the MSRV to **1.85**, keep the lockfile. 1.85.0 is installed
  and `cargo +1.85 check --locked` passes. CI has a separate MSRV job.
- **Consequences:** users need Rust ≥1.85; `README` and CI reflect this correctly.

## D-03. `panic = "abort"` → unwind; enable `overflow-checks` in release

- **Context (F-03):** a corrupt MFT makes the parser panic; abort turns a data
  error into exit code 134 with no fallback possible. The guide requires
  `catch_unwind`.
- **Decision:** remove `panic = "abort"`; keep the default `panic = "unwind"`; enable
  `overflow-checks = true` for release; `fs::detect_guarded` wraps
  `catch_unwind` and returns `Err`, the CLI falls back to raw with a warning
  (exit 3).
- **Consequences:** the binary is slightly larger and slower; data errors become
  controlled errors. No `unsafe` is used.

## D-04. Report schema v2, backward compatible via `--report-schema 1`

- **Context (F-13):** the 0.1.0 report lacks image hash, argv, options, commit,
  LBA/cluster, ISO timestamps, case/examiner.
- **Decision:** the current schema is **2**; all old fields keep their names and
  meanings. `--report-schema 1` removes the v2 fields from the JSON (and
  `too_large_truncated` from `stats`) so old consumers are not surprised.
- **Consequences:** the default is v2; `docs/report.schema.json` documents v2 and
  notes schema 1. The test `report_schema_one_has_the_v01_shape` locks the key
  set.

## D-05. `status` keeps the meaning "structurally complete"; add `validation`

- **Context (F-04):** `status=complete, score=100` was once read as "the file is
  usable", while a stitched-tail JPEG is still `complete`.
- **Decision:** keep `status` for compatibility; add
  `validation ∈ {structure, decoded, partial, unverified}` and
  `validated_bytes`; add `--validate decode`.
- **Consequences:** `complete` = header/footer found and structure valid, **does
  not** guarantee decodability. The README states this clearly.

## D-06. `FsInfo.data_start` and the real cluster number

- **Context:** the 0.1.0 report derived clusters from `offset - fs.offset`, which
  is wrong for FAT32 (cluster 2 starts after the FAT tables) and NTFS.
- **Decision:** add `data_start` = absolute offset of the first data cluster
  (FAT32: after reserved+FAT; NTFS: start of volume). The reported `cluster` is
  the on-disk number: FAT32 = index + 2, NTFS = index.
- **Consequences:** cluster numbers match `istat`/`fsstat`; the test
  `filesystem_aware_scan_uses_free_extents_only` checks cluster 3.

## D-07. PDF boundary: `%PDF-` only after `%%EOF`

- **Context (F-02):** an uncompressed embedded PDF containing `%PDF-` was treated
  as a new document, cutting off the outer PDF.
- **Decision:** a `%PDF-` starts a new document only when it immediately follows
  `%%EOF` (ignoring whitespace); `%PDF-` inside a stream/attachment belongs to
  the current document.
- **Consequences:** R3 passes; a PDF containing a PDF is carved intact, the inner
  PDF is subject to nested suppression.

## D-08. PDF early-EOF: read more when the tail "may still contain a revision"

- **Context (F-01):** the first read window hit the `%%EOF` of the first part
  (linearized) or of an older version in an incremental update and finalized
  too early.
- **Decision:** in `smart` mode, if the selected `%%EOF` touches the end of the
  buffer, or the tail to the end of the window consists only of ASCII
  whitespace, or the tail contains `xref/startxref/trailer/ obj`, return
  `Need::More` instead of complete. Do not read more if NUL/JPEG/garbage follows
  `%%EOF`.
- **Consequences:** linearized/incremental PDFs are carved fully; the NUL padding
  after a PDF is not swallowed. R1, R2 pass; early EOF + incremental tests.

## D-09. PNG salvage: valid prefix instead of discarding everything

- **Context (F-05):** a PNG missing IEND or with a corrupt chunk was discarded
  entirely, losing the still-good leading part.
- **Decision:** if a valid IHDR + IDAT is present and an invalid chunk type is
  encountered, return the prefix ending at the last valid chunk; status
  `truncated`, write `validated_bytes`.
- **Consequences:** R4b passes; the output can be partially opened. Do not mark
  `decoded` if IEND has not been passed.

## D-10. Check baseline JPEG with a self-written MCU count

- **Context (F-04):** `jpeg-decoder` may forgive a stitched image; splice detection
  is needed.
- **Decision:** write `strict_baseline_scan` that reads the file's own Huffman
  table, counts MCUs and compares with SOF; apply only to single-scan baseline
  sequential. Progressive/arithmetic/multi-scan cases return `None` (no
  conclusion).
- **Consequences:** stitched JPEG → `validation=partial`; valid corpus images
  remain `decoded`. `jpeg-decoder` is used only for `--validate decode`, not used
  as an oracle in the benchmark.

## D-11. Remove `ScanStats.trimmed_tail`

- **Context:** this field was never incremented (dead field) and is not in the
  published schema.
- **Decision:** remove it from the struct and from schema 1; schema 2 does not
  have it either.
- **Consequences:** no behavior change; one less misleading field.

## D-12. Keep historical defaults

- `--align none`, `--validate structure`, `--max-truncated-size 16M`,
  `--min-score 50`, `--chunk-size 4M` (CLI 0.1.0), `--pdf-eof smart`.
- **Rationale:** the requirement is to preserve the CLI and the published results.
  The new improvements (decode, align, schema v2) are optional or additive.

## D-13. `--chunk-size` default 4 MiB

- **Context:** guide A-03 suggests starting at 4–64 KiB; the 0.1.0 code uses 4
  MiB for the parallel scan phase.
- **Decision:** keep 4 MiB so as not to change historical performance/results;
  the parser still reads incrementally within the `--max-size` budget. Tests use
  64 KiB to run faster.
- **Consequences:** default behavior is unchanged; users can lower it to save
  memory.

## D-14. Nested suppression before writing

- **Context (F-14):** 0.1.0 wrote files and then deleted nested files, leaving
  dangling output and wrong logs.
- **Decision:** sort candidates by offset/size, decide nesting in memory before
  `write_outputs`; the console prints only final files.
- **Consequences:** no temporary files are deleted; `stats.suppressed_nested`
  still counts.

## D-15. Do not use a library as an oracle for the benchmark

- **Context:** guide A-04 forbids using the same library both as validator and
  as oracle.
- **Decision:** the benchmark scores by original SHA-256 and footer
  normalization; Pillow is used only for manual checks/CFReDS, not to compute
  CarveX TP/FP.
- **Consequences:** exact/usable metrics are independent of CarveX's validator.

## D-16. Published figures remain unchanged; 0.2.0 does not re-run the full benchmark

- **Context:** `bench/results/final` is the source of every number in
  README/REPORT. Re-running all 5 configurations × 3 times takes time and may
  skew the numbers due to the environment.
- **Decision:** freeze the old baseline (`bench/results/baseline-v0.1/`), **do
  not** modify `final/`. Verify that the carving regression on 4 images (1 run)
  matches ground truth byte-for-byte before release. Do not publish new
  benchmark numbers for 0.2.0.
- **Consequences:** the README states clearly that the figures are from 0.1.0
  and have been verified to leave carving behavior unchanged; all
  report/validation improvements are additive.

## D-17. Fuzz/coverage/CI: record SKIPPED instead of claiming

> Historical status. Local fuzz, coverage and supply-chain runs and a GitHub
> Actions run were later executed; see D-25/D-26/D-28 and `docs/VALIDATION.md`.

- **Context:** the environment initially had no nightly/cargo-fuzz,
  cargo-llvm-cov, actionlint, cargo-deny or cargo-audit, and has no GitHub
  runner. `cargo-audit` and `cargo-deny` were later installed locally.
- **Decision:** write the full targets/CI/config, compile-check what can be
  checked, but **do not** claim that fuzz/coverage/CI was run. Record SKIPPED in
  PROGRESS and REVIEW_RESPONSE.
- **Consequences:** reviewers can clearly see what has not been verified.
  Local `cargo audit` and `cargo deny check` results are recorded in PROGRESS
  and DEPENDENCIES; no GitHub Actions run is claimed.

## D-18. Do not create a lockbox before a frozen protocol exists

> Historical status. The repository was later tagged `protocol-v1` and
> `protocol-v1.1`, and the lockbox was executed once at D-26/D-28.

- **Context:** C-10 requires tagging the protocol before running the lockbox once.
  The repo is not a git repo so it cannot be tagged; the lockbox dataset (seed
  1001–1020) has not been generated.
- **Decision:** do not run the lockbox, do not publish any lockbox numbers.
  `docs/BENCHMARK_PROTOCOL.md` states the status clearly: "pre-registered, not
  executed".
- **Consequences:** avoid "burning" the lockbox; conclusions are based only on
  the D1 dev-set.

## D-19. A-07 (step 1): keep the default `--partition largest`, coded warnings, exit 3

> Historical status of step 1. Step 2 was approved by the user and implemented
> at **D-23**; the lines below describe the behavior before the default was changed.

- **Context:** guide §6 A-07 (step 1) requires that no volume/gap be skipped
  without notice; changing the default to "scan all" is STOP-AND-ASK (§3.5). At
  that time there was no confirmation to change the default.
- **Decision:**
  - `--partition largest|all|<N>` (default `largest` = historical 0.1.0 behavior).
  - `--include-gaps` raw-scans the gap regions + unsupported partitions.
  - Report schema v2 adds `volumes[]` and `regions[]`
    (`kind = free|allocated|gap|unsupported`, `offset`, `len`, `scanned`,
    `partition_index`) and `warnings` becomes an object `{code, message}`.
  - Coded warnings for "not fully scanned": `VOLUME_NOT_SCANNED`,
    `GAP_NOT_SCANNED`, `VOLUME_UNSUPPORTED`, `FS_DETECTION_FAILED`,
    `IMAGE_MODIFIED` ⇒ exit **3** (finished but incomplete). `--partition all`
    writes to `part1/`, `part2/`, …
  - `--no-fs` / `--offset` are deliberate user choices ⇒ no region computation,
    no warnings.
  - Within a *selected* volume, free/allocated is decided by `--mode` — that is
    mode semantics, not a warning (the principle "warning = evidence skipped",
    not "skipped by design").
- **Consequences:** a default run on the benchmark MBR image (gap 2 MiB) now
  exits 3 with `GAP_NOT_SCANNED`; `bench/run_benchmark.py` accepts codes 0 and 3
  for carvex. The carved file counts remain 30/30/20/20, matching the published
  SHA-256.

## D-20. `RAW_SCANS_ALLOCATED` no longer causes exit 3

- **Context:** 0.1.0 pushed the warning "raw mode also scans allocated clusters"
  into the general list ⇒ exit 3. In A-07, exit 3 means "evidence skipped"; raw
  mode reads *everything*, so nothing is skipped.
- **Decision:** keep the warning (code `RAW_SCANS_ALLOCATED`, informational) in
  stderr/report but **do not** put it in the `incomplete()` group.
- **Consequences:** `carvex scan --mode raw` on an image with no filesystem now
  exits 0 instead of 3. This is a deliberate behavior change from 0.1.0,
  documented here as required by guide §7 (every change must have a decision).

## D-21. NTFS: sector count = field + 1 (off-by-one)

- **Context:** the NTFS boot sector (offset 0x28) stores "number of sectors of
  the volume − 1" because the backup boot sector is in the last sector. The old
  parser used the value directly ⇒ the volume was 1 sector short, `$Bitmap`
  (1,984 B = 15,872 clusters) was larger than the computed cluster count
  (15,871) and `regions[]` did not tile the image completely.
- **Evidence:** on `bench/images/all-deleted_ntfs.img`: field = 126,975,
  partition = 126,976 sectors; the backup boot sector (OEM `NTFS`) is at sector
  126,975 of the partition, sector 126,974 is zeros.
- **Decision:** `volume_sectors(field, available) = field + 1` clamped by
  `available` (a truncated image must not be read past its end). There is a unit
  test (`volume_sector_count_includes_the_backup_boot_sector`).
- **Consequences:** `regions[]` tiles the image completely; FAT32/NTFS carving is
  unchanged (verified 100/100 files matching the published SHA-256).

## D-22. Naming deviations from guide §8.1/§8.4 (trusted repository)

- **Context:** guide §0.5 allows keeping a trusted repository's conventions
  instead of forcing mass renames; §8.1/§8.4 are architecture suggestions.
- **Decision — keep the current names:**
  - `--validate structure|decode` instead of `structure|deep`; the default mode
    remains `structure` (D-05/A-04).
  - Field `validation` (0.1.0) instead of renaming it to `validity` — renaming
    would break JSON compatibility, contrary to "additive only".
  - Flat module layout (`src/fs`, `src/signatures`, `src/scan.rs`,
    `src/report.rs`, `src/cli.rs`) instead of `volume/format/engine/report/cli`.
    Behavior and the public API do not depend on file paths.
  - `unreadable_regions[]`, `checks[]`, `src/volume/` do not exist yet: they
    belong to A-08 (`--on-read-error`) and A-04 (validity model), which are not
    at that stage yet.
- **Consequences:** REVIEW_RESPONSE records these items as OPEN/on-schedule, not
  omitted.

## D-23. A-07 step 2: change the default to "all volumes + all gaps" (approved)

- **Context:** guide §6 A-07 step 2 and §3.5/§3.4 require user confirmation, a
  **Changed** entry in the CHANGELOG, a flag to restore the old behavior, and a
  minor bump. The user answered "Flip default to all+gaps" when asked directly
  on 2026-11-19; D-19 records the step 1 status before this decision.
- **Decision:**
  - Default `scan`: `--partition all` + gap/unsupported extents are included in
    `plan_extents`; `info` also reports all volumes/gaps as `scanned=true`.
  - New flag `--no-gaps` to exclude gap/unsupported; `--include-gaps` remains
    accepted (meaning unchanged) for compatibility with old scripts. The two
    flags conflict with each other.
  - `--partition largest --no-gaps` restores exactly the 0.1.0/step 1 results:
    one volume, coded warnings, exit 3.
  - Coded warnings and exit 3 still apply to every run that *actually* skips
    evidence (including deliberate exclusions).
  - The minor version is already **0.2.0** (0.1.0 → 0.2.0); the CHANGELOG
    **Changed** entry documents the behavior change and the restore flag.
- **Benchmark consequences:** `bench/run_benchmark.py` explicitly passes
  `--partition largest --no-gaps` to every CarveX command to keep the search
  space and the published 0.1.0 figures unchanged; no `--full` re-run, no figure
  changes. `scripts/review_repro.py` R8 checks all four modes (default, restore,
  `all --no-gaps`, `--include-gaps`). New green baseline:
  `docs/baseline/A-07_green_v0.2.0_step2.txt`.
- **Other consequences:** `docs/REVIEW_RESPONSE.md`, README, ARCHITECTURE and
  PROGRESS are updated; the CLI contract tests add a case for restoring the old
  behavior. No subcommand/field/file-name format names are changed.

## D-24. English-only documentation and report rename

- **Context:** the user requested that CarveX use English for all documentation,
  comments and reports, and that the technical report be renamed to
  `docs/REPORT.md`.
- **Decision:** translate the remaining prose in README, release docs, `docs/`
  and `bench/results/cfreds/README.md`; keep every command, flag, ID, hash,
  version, date, count, benchmark figure, URL, DOI and citation unchanged; and
  update all references to the report path. Source comments and user-facing
  report strings were already English.
- **Consequences:** `docs/REPORT.md` is the sole technical report. Table
  captions use `Table 1…16`, and numbers use English separators. CLI behavior,
  JSON/CSV fields, warning codes and frozen benchmark results are unchanged.

## D-25. Fuzz, coverage and CI: replace SKIPPED with measured evidence

- **Context:** D-17 recorded fuzz runtime, coverage and GitHub CI as SKIPPED
  when the tools or runner were unavailable.
- **Decision:** install `cargo-fuzz` with nightly, `cargo-llvm-cov` and
  `actionlint`; run seven fuzz targets for 60 seconds each, measure line
  coverage with `scripts/coverage.sh`, and record the real GitHub Actions runs.
  Because measured coverage is 63.42%, set the honest CI/local floor to 60%
  rather than the aspirational 80%.
- **Consequences:** 18,520,025 fuzz executions with no crashes; 2859/4508 lines
  covered; GitHub run `44548142275` succeeded. Evidence is in
  `docs/VALIDATION.md`; no 80% coverage claim is made. GitHub's scheduled fuzz
  matrix remains skipped on push events, so local fuzz evidence is the actual
  runtime evidence.

## D-26. Freeze protocol-v1, then disclose the pre-execution v1.1 builder fix

- **Context:** the protocol required a tag before any lockbox result was
  viewed. The first generation attempt showed that 19 of the fixed seeds
  1001–1020 failed the existing corpus builder's own 4096-byte NTFS
  non-residency guard for `pdf_08_small_text.pdf`.
- **Decision:** keep `protocol-v1` as the frozen protocol record; with user
  approval, change only that builder padding value from 6 to 10, create
  `protocol-v1.1` before regenerating or viewing the dataset, and disclose the
  correction in PROGRESS and VALIDATION. No carving, scoring, runner, config or
  protocol-text change was made.
- **Consequences:** all 20 seeds pass the guard; the lockbox was generated from
  the v1.1 builder. Results are single-use evidence, never a tuning signal.

## D-27. Replace invalid actionlint GitHub Action with a pinned direct install

- **Context:** the first two GitHub runs failed because
  `rhysd/actionlint-action@v1` resolves to a non-existent repository.
- **Decision:** install actionlint 1.7.12 from its release tarball in the
  workflow and run `actionlint .github/workflows/*.yml` directly.
- **Consequences:** the CI-fix commit's GitHub run `44548142275` completed with
  all jobs successful. The local gate uses the same actionlint version.

## D-28. Run the lockbox and full benchmark into new result directories

- **Context:** protocol execution evidence was still missing, and the 0.1.0
  figures in `bench/results/final` had to remain frozen.
- **Decision:** run `scripts/lockbox_run.sh` exactly once into
  `bench/results/lockbox-v1`, and `scripts/benchmark_full.sh` into a fresh
  `bench/results/full-0.2.0`; verify dataset and result checksums; compare the
  full run with `bench/results/final` but do not change published figures or
  code.
- **Consequences:** lockbox 100/100 runs and full benchmark 60/60 runs were
  successful; all 20 full-run rows are metric-identical to 0.1.0. Frozen result
  directories are untouched; D2–D7, same-search-space baselines, ablations,
  block metrics and advanced statistics remain OPEN.

## D-29. Add B-04 evidence sources without changing the raw path

- **Context:** B-04 required E01, split raw, VHD, VMDK and stdin support, while
  the frozen benchmark/lockbox evidence and the published raw-image behavior
  had to remain untouched.
- **Decision:** keep `RawFile` as the default for ordinary raw images and add
  new read-only `ImageSource` implementations under `src/image/`. `open_image()`
  selects a container by magic bytes and segment naming, and
  `--input-format` forces an explicit choice. Stdin is spooled to a temporary
  file because the scanner requires random access. Unsupported container
  variants fail explicitly instead of returning partial data.
- **Consequences:** five fixture tests verify byte-for-byte reads, stable source
  identity for disk-backed containers, and stdin report identity; the full gate
  and benchmark smoke stay green. Recognized EWF2/Ex01, L01/Lx01, LVF,
  differencing VHD, ESX/COWD and compressed streamOptimized VMDK variants fail
  explicitly. Encrypted E01 is not supported and is not guaranteed to be
  detected before parsing; the dynamic VHD reader ignores the per-sector
  presence bitmap inside an allocated block. Frozen results, figures, protocol
  text and dist binaries are unchanged.

## D-30. B-06 stays opt-in and deep-validates every stitch

- **Context:** B-06 required bounded bifragment gap carving, while the frozen
  0.1.0 benchmark results, published figures and default raw-image path had to
  remain unchanged.
- **Decision:** add `--fragment-recovery` (off by default) plus `--max-gap`,
  `--max-frag-candidates` and `--frag-time-limit`. A stitch is attempted only
  between two cluster-aligned fragments inside a detected volume. Head and tail
  boundaries are enumerated under the stated bounds, ordered by increasing gap,
  and a join is accepted only when `validate_deep` returns `Decoded`. Stitched
  records use `source="bgc"` and add optional `fragments[]`, `checks` and
  `confidence`; those JSON fields are omitted when the stage is disabled.
- **Consequences:** S12-style interleaved JPEG, PNG and PDF fixtures recover both
  source files byte-exactly and no stitched file is reported as deep-validated
  without the validator. The implementation is limited to two fragments and has
  no D1 recall/splice-rate measurement, so B-06 is MITIGATED rather than FIXED.
  The default scan, frozen results, protocol text and dist binaries are
  unchanged.

## D-31. Enforce `--max-memory` before allocation and retention (R-01)

- **Context:** `--max-memory` was previously compared with the summed size of
  retained outputs only after carving had completed. A dense or hostile image
  could therefore allocate many carve buffers and retain every carved result
  before the limit was checked.
- **Decision:** introduce a shared `MemoryBudget`/`BudgetGuard`. Carve buffers
  reserve their bytes before allocation and before each growth step; a carved
  result keeps its reservation while retained and is released when
  post-filtering discards it; kept results stay charged through output writing.
  B-06 join buffers and accepted stitches use the same budget. Carve
  work is batched by budget and worker count; a shared counter stops candidate
  collection as soon as `--max-candidates` (default 1,000,000) is exceeded, and
  `--time-budget` bounds long-running scans.
  The new options are additive and recorded in report schema v2; schema 1 is
  unchanged.
- **Consequences:** budget exhaustion aborts the scan instead of allowing
  unbounded retention, and three `tests/memory_budget.rs` tests cover early
  failure and the candidate bound. Carving, scoring, output naming and the
  frozen raw-image path are unchanged. `--max-memory` is a scan-time budget for
  carve buffers, retained outputs and B-06 stitches, not a total process-RSS
  allocator cap; JPEG/PDF decoder internals are not fully instrumented.

## D-32. Bound PNG decode validation before inflation (R-02)

- **Context:** the PNG decode validator copied every IDAT payload into one
  `Vec`, inflated into a second `Vec` with `read_to_end`, compared the decoded
  length only after decompression, and built a row-offset `Vec<usize>`. A
  compressed image with hostile dimensions or a high expansion ratio could
  exhaust memory even when `--max-memory` was set, because that budget did not
  cover the validator.
- **Decision:** derive the expected filtered-scanline size from IHDR and check
  it against a validator budget before creating an inflater. The default is
  `DEFAULT_MAX_PNG_DECODE_BYTES` = 256 MiB; `--max-decode-bytes` configures it
  and `ValidationLimits::new(0)` selects the default rather than disabling the
  guard. A larger image is downgraded to `Validation::Partial` before any IDAT
  byte is inflated. Accepted images are inflated incrementally with
  `flate2::Decompress` into a fixed 64 KiB buffer, with filter bytes checked by
  a lazy row cursor that also handles Adam7 without a row-offset vector. The
  old `validate_deep` signature remains and uses the default limit;
  `validate_deep_with_limits` is used by the scan and B-06 paths.
- **Consequences:** four `tests/png_decode_budget.rs` tests cover extreme
  dimensions, an explicit small budget, a 50,335,744-byte expansion with a
  peak-RSS assertion, and an invalid filter byte. Report schema v2 records the
  effective `options.max_decode_bytes`. The PNG path no longer retains the full
  IDAT copy, decoded image or row offsets. JPEG/PDF decoders are outside this
  bound and are not claimed to be fully instrumented. `--validate structure`
  remains the default; the frozen benchmark, lockbox results, figures, protocol
  text and dist binaries are unchanged.
