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
  boundaries are enumerated under the stated bounds; candidate joins are scored
  by explained filler/header evidence and bounded by a work-unit budget. A
  stitch is written only when the authoritative `validity` is `verified`.
  Stitched records use `source="bgc"` and add optional `fragments[]`, `checks`,
  `confidence` and `alternatives`; those JSON fields are omitted when the stage
  is disabled. D-53 supersedes the original increasing-gap ordering and
  `validate_deep == Decoded` acceptance rule after H-29.
- **Consequences:** S12-style interleaved JPEG, PNG and PDF fixtures recover both
  source files byte-exactly, and no stitched file is reported as verified
  without the validator. The implementation is limited to two fragments, has no
  metadata/runlist-assisted reassembly, and remains opt-in. A D1-style
  development profile now supplies recall, splice-rate and junk-FP evidence;
  see D-53. The default scan, frozen results, protocol text and dist binaries
  are unchanged.

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

## D-33. Adopt `harness.md` as the remediation baseline (supersedes D-01)

- **Context:** `harness.md` (745 lines, SHA-256 recorded by the user in the
  harness header check) was supplied on 2026-11-21 as the unified remediation
  harness merged from five independent reviews. `docs/guide.md` is not present
  in the repository at the time of adoption; the harness is self-contained for
  the work it requires, and the user was asked to supply `docs/guide.md` (924
  lines, expected sha256 `22ee8122209e9eda11d6f32a1e650e8f8e48f5610bd808309d94f436e5e78265`).
- **Decision:** Place `harness.md` at the repository root and follow its phase
  order. D-01 remains historical context for the pre-harness guide-v1/v2/v3
  work; from this point the harness is the active instruction set. HEAD is
  `0.3.0-dev` (P0.9/ADR-H9) to distinguish it from released 0.2.0.
- **Consequences:** Phase 0 baseline work starts from commit `a7a5c08` (clean
  tree, one commit ahead of `706a12a` with R-01/R-02 already applied). Any
  finding not reproduced is recorded as `NOT-REPRODUCED` with evidence, not
  silently dropped.

## D-34. H-19 stage 1 PDF validation honesty vs B-06 stitch acceptance (E-1)

- **Context:** H-19 stage 1 changes PDF deep validation to return
  `Validation::Unverified` with an explicit lexical-only note, never
  `Validation::Decoded`, because the current check validates startxref/xref
  structure but not PDF content. The B-06 fragment recovery path
  (`try_fragment_join` in `src/scan.rs`) and S12 acceptance test
  (`tests/fragment_recovery.rs`) previously required `Validation::Decoded` for
  all stitched outputs.
- **Decision:** Keep H-19 honest. B-06 accepts `Validation::Decoded`, or for
  PDF only `Validation::Unverified` when the validation notes include
  "startxref target validated" (lexical-validated case). S12 is updated with an
  E-1 comment: PDF BGC stitches assert `Unverified` plus the lexical-validation
  note; JPEG/PNG still assert `Decoded`. No PDF carve is labeled Decoded.
- **Consequences:** B-06 PDF recovery remained possible with an honest label
  during stage 1. D-46 supersedes the stage-1 label for PDFs that pass the new
  structural checks. D-53 later removes the lexical-validated `Unverified`
  B-06 acceptance path entirely: stage 2 permits a clean PDF to be `verified`,
  but a B-06 stitch must still be `verified` after deep validation.

## D-35. E01 chunk cap and read-error policy (H-11, H-22)

- **Context:** an E01 header can declare a chunk size derived from
  `sectors_per_chunk × bytes_per_sector`. A hostile value of 2^27 sectors at
  512 bytes/sector previously drove a 64 GiB allocation in `load_chunk`. Real
  EWF images use 32 KiB or 64 KiB chunks. Separately, a damaged or short final
  chunk aborted the entire scan, losing results already obtained.
- **Decision:** cap declared chunks at `MAX_EWF_CHUNK = 64 MiB`, validate
  `bytes_per_sector` as a plausible power of two in 128…65,536, and allocate
  with `try_reserve_exact`. The default `--on-read-error` is `zero-fill` as
  confirmed by the user in harness §3.6; `abort` restores the old behaviour and
  `skip` is reserved for candidate-level skipping. Unreadable ranges are
  recorded in `OpenedImage.unreadable_regions` for report surfacing.
- **Consequences:** `info` and `scan` reject a 64 GiB-declaring E01 in a few
  milliseconds with a typed error. A short final chunk is read at its logical
  length; both short-checksum and full-chunk-checksum variants are accepted.
  The frozen benchmark, default raw path, output naming and schema 1/2 legacy
  fields are unchanged.

## D-36. Shared decode-validator memory budget (H-05)

- **Context:** R-02 bounded PNG inflation with `--max-decode-bytes`, but the
  harness requires one flag to cover every decode validator, including the
  JPEG frame allocation addressed by H-06. Keeping the old name as the only
  public option would leave the documentation and report semantics split.
- **Decision:** `--max-validator-memory` is the authoritative name (default
  256 MiB). `--max-decode-bytes` stays as a visible alias for compatibility.
  `ValidationLimits` carries the effective value in both fields; report schema
  2 adds `options.max_validator_memory` while retaining
  `options.max_decode_bytes`. Zero selects the default rather than disabling
  the bound.
- **Consequences:** PNG and JPEG validators share one deterministic budget and
  no decode validator may claim more than that budget for one candidate. The
  legacy report field keeps its type and meaning, and schema 1 is unchanged.

## D-37. JPEG frame-claim budget and entropy plausibility (H-06)

- **Context:** `jpeg-decoder` allocates the frame from SOF dimensions before
  consuming entropy data. A v2 runtime test with a 65,535×65,535 SOF was
  killed at 5,948 MiB RSS. The existing `--max-memory` could not prevent it
  because decode validators were outside that budget.
- **Decision:** before invoking the decoder, compute
  `width × height × components` with checked arithmetic and compare it with
  `--max-validator-memory`; an excess is `validation=unverified` with a note
  naming the budget. For Huffman-coded frames only, also reject claims above
  twice the optimistic entropy capacity: each 8×8 block needs at least two
  bits (DC symbol plus EOB), i.e. at most 256 samples per entropy byte, so the
  guard uses 512 samples per input byte. Arithmetic-coded SOF9+ frames are
  exempt from the entropy check because they do not use Huffman tables.
- **Consequences:** locally measured 20,000×20,000 and 65,535×65,535 patched
  JPEGs both finish in ≤ 0.01 s at about 9 MiB RSS and are labelled
  `unverified`; structure mode is unchanged. The plausibility rule is an
  anti-OOM guard, not a general JPEG authenticity test.

## D-38. `--max-memory` as a working-memory budget (H-08)

- **Context:** R-01 retained every carved byte until a post-hoc sum was
  compared with `--max-memory`. Hostile inputs therefore allocated the whole
  output before the check, and an excess exited 1 with no report. The harness
  requires streaming writes, typed coded warnings and exit 3 for limit events.
- **Decision:** the flag now bounds concurrent working buffers only. Carving
  produces `(source ranges, transformation, analysis, SHA-256)` descriptors;
  the write stage re-reads ranges with a bounded scratch buffer, hashes while
  writing to a temporary file, verifies the digest, then atomically renames.
  A digest mismatch yields `SOURCE_CHANGED` and no output file. The budget is
  a blocking permit pool: a request bigger than the whole budget is a
  deterministic `CANDIDATE_SKIPPED_BUDGET`; otherwise the caller waits, and
  growth first drops the old buffer and permit to avoid deadlock. Decode
  validator scratch remains governed by `--max-validator-memory` and cannot
  raise the shared ceiling.
- **Consequences:** cumulative output may exceed `--max-memory` without being
  discarded; no result depends on `--threads` or scheduling. Every budget or
  source-change event is a coded warning in the JSON report and makes the CLI
  exit 3. The double-read cost is measured under G8 rather than hidden behind
  a hybrid retain/stream path.

## D-39. Work-unit and wall-clock budgets (H-09)

- **Context:** the old `--max-candidates` and `--time-budget` aborted with
  exit 1 and no report, and there were no limits on per-candidate analyzer /
  validator work or cumulative output. Reviews require deterministic work-unit
  limits, a flagged wall-clock safety net, coded warnings and a partial report.
- **Decision:** `--max-candidates` (default 1,000,000) uses a two-pass
  deterministic allocation: fixed tasks from `--chunk-size` (never `--threads`)
  are counted in a cheap lexical pass, then the exact global budget is handed
  out in task order before candidate records are built. An over-budget image
  therefore keeps the same prefix regardless of scheduling, while a limit at
  or above the true candidate count never drops candidates spuriously.
  `--max-work-per-candidate`
  (default 256 MiB; `0` = unlimited) counts bytes handed to analyzers and deep
  validators. `--max-output` (default 4 GiB; `0` = unlimited) stops before the
  next output that would exceed the cap. These limits are deterministic and
  emit `CANDIDATE_LIMIT`, `WORK_LIMIT` and `OUTPUT_LIMIT`. `--time-budget SECS`
  remains wall-clock (`0` = unlimited by design); expiry is latched across all
  stages, emits `TIME_LIMIT` and sets `nondeterministic_stop: true`. Every
  event writes a report and exits 3.
- **Measurements (release build, 2026-11-21):** D0 `bench/images` cases used
  27–41 candidates and wrote 6,327,552–14,259,990 bytes; the lowest
  `--max-work-per-candidate` that avoided `WORK_LIMIT` on the largest raw D0
  case was 8,323,072 bytes. S9-4M flood16 produced 262,144 candidates, 0 bytes
  written and needed at least 65,536 bytes of per-candidate work; S10
  flood512/64 MiB produced 129,804 candidates, 0 bytes written and also needed
  65,536 bytes. No default limit triggered on any measured scenario.
- **Residual evidence gap:** CFReDS RM#2 artefacts are not present in the tree
  and were not re-run for this decision; the defaults leave roughly 30× headroom
  over the largest measured per-candidate work and 280× over the largest
  measured candidate count, but no claim is made about that unavailable data.

## D-40. Panic containment boundary (H-10)

- **Context:** the independent reviews found `catch_unwind` only in filesystem
  detection. Candidate analysis/validation inside rayon, B-06 join attempts and
  container opening had no containment. No natural panic fixture was available;
  v2/v5 mutation runs only recorded exit codes, so this was a latent
  defence-in-depth gap rather than a reproduced crash.
- **Decision:** each candidate's work is wrapped in
  `catch_unwind(AssertUnwindSafe(…))`; a panic becomes an `InternalError`
  record with source offset, file type and message, increments
  `stats.internal_errors`, adds a `CANDIDATE_PANIC` incomplete warning and exits
  3, while the remaining candidates are still carved. Each B-06 join attempt is
  wrapped separately. `open_image_with_options` wraps all container parsers and
  converts a panic into an `anyhow` error, so `info`/`scan` fail with an error
  instead of unwinding. `Cargo.toml` now explicitly keeps `panic = "unwind"` in
  the dev and release profiles, with a test that reads the manifest.
- **Test seam:** a `#[doc(hidden)]` static and, for the Python repro, the
  `CARVEX_TEST_PANIC_AT_OFFSET` environment variable inject a panic into one
  candidate. There is no visible CLI flag and production never sets the seam.
  X06 now asserts a report with one `internal_errors[]` record, one counted
  error, the `CANDIDATE_PANIC` warning, exit 3 and the healthy candidate still
  written. No claim is made about fuzz execution or natural panic discovery.

## D-41. Parser fuzz coverage and fixtures (H-14)

- **Context:** four independent reviews found fuzz coverage only for the seven
  original targets. The new E01, VHD, VMDK, split-raw, `open_image` dispatch and
  B-06 join paths accept attacker-controlled bytes but had no target, and
  `tests/evidence_formats.rs` contained one-off container builders that could
  not be reused by a fuzzer or by property tests.
- **Decision:** a hidden `src/image/fuzz_support.rs` module owns bounded
  builders and mutators. Fuzz bytes choose small, plausible variants of the
  same on-disk layouts used by the evidence-format tests; all sizes are capped
  before allocation. When fuzz input already has recognised EWF, VHD or VMDK
  magic it is written through unchanged, so committed seeds reach the real
  parser paths. `examples/gen_fuzz_seeds.rs` regenerates the deterministic
  seeds, including the hostile X07 huge-chunk sample and the X08/X09 VMDK
  samples. The global `*.E01` ignore rule is narrowed only for
  `fuzz/corpus/**` so those seeds are committed source fixtures.
- **Boundary:** `fuzz_fragment_join` intentionally calls `try_fragment_join`
  without the production `catch_unwind` wrapper so libFuzzer can report a
  panic. This changes no CLI behaviour; H-10 containment remains at the scan
  boundary. Real-tool E01 and VMDK fixtures were SKIPPED rather than invented
  because `ewfacquire`, `ewfexport` and `qemu-img` are unavailable. No crash
  was found, so no entry was added under `tests/fixtures/crashes/`.
- **Evidence:** six targets ran 60 s each with 235,272 executions and no
  crash, timeout, OOM or artifact; five structured-mutation property tests
  exercise the shared builders. The original seven-target run remains a
  historical result and is not presented as a post-change run. Full counts
  are in `docs/PROGRESS.md` and `docs/VALIDATION.md §4`.

## D-42. H-15 parallel-carve performance scope

- **Context:** the reviewer recorded a ~1.6x parallel slowdown in the 0.2.0
  musl package, while several source-build reviews did not reproduce it. X23
  was re-timed on 2026-11-22 with three runs per arm.
- **Decision:** close H-15 as a measured, packaged-release issue. The current
  glibc source build is faster with the default thread count (0.489 s at
  `--threads 1` versus 0.156 s default on the X23 flood image); only the
  stale 0.2.0 musl dist binary is slower (0.694 s versus 2.436 s, 3.51x).
  That binary predates the streaming-write and buffer-reuse work in H-08, so
  no source change is justified by this evidence.
- **Constraint:** no allocator change is made without STOP-AND-ASK, and no
  performance tuning is done on lockbox or published results. The approved
  end-of-harness 0.3.0 dist rebuild must be timed separately before claiming
  anything about the packaged 0.3.0 binary.

## D-43. Authoritative validity model (H-16)

- **Context:** the legacy `status=complete` and `score=100` fields were being
  read as a content guarantee, while default `--validate structure` never
  checked payload bytes. Existing `validation` has a compatibility contract
  (D-22), so renaming or changing it is not acceptable.
- **Decision:** add required, additive per-file `validity` and `checks[]`
  fields. `validity` follows ADR-H1 (`verified`, `structural`, `suspect`,
  `partial`, `repaired`); checks are `{name, ok: true|false|null, note}` and
  describe only work actually performed. Summaries, console lines, the CSV's
  final column and manifest comments use the new label; `structural` is always
  printed as "content not checked". `validated_bytes` is the contiguous
  prefix whose content was checked, not the carved length.
- **Field conflict:** a B-06 integer field was already named `checks` in the
  unreleased 0.3.0 schema. The Rust field name is retained for source
  compatibility, but its JSON key becomes `fragment_checks`; the H-16 array
  owns `checks`. Schema 1 omits all three additive keys.
- **Boundary:** score and emission are deliberately unchanged. This commit
  does not claim full PNG CRC coverage, strict JPEG scan support or real PDF
  xref validation; those are H-17–H-19. No repair is claimed to re-validate
  until H-21.

## D-44. PNG CRC integrity failure (H-17)

- **Context:** the structural PNG parser already recomputed every chunk CRC
  and only lowered the score; decode mode returned `Partial` but did not say
  which chunk failed, and `validated_bytes` was zero. Reviewers could therefore
  see a bad-CRC PNG as `complete/decoded`.
- **Decision:** a CRC mismatch is an integrity failure in both modes: the file
  keeps its historical score and is still emitted, but `validation=partial`
  and `validity=suspect`. Both modes attach a `chunk_crc` check naming the
  chunk type and byte offset. Decode mode recomputes all CRCs and sets
  `validated_bytes` to the contiguous prefix ending at the last good chunk;
  structural mode still reports zero because it does not claim decoded bytes.
- **Compatibility:** no score, emission, CSV prefix, schema shape or published
  benchmark figure changes. The earlier H-17 partial fix is retained and
  extended rather than reverted.

## D-45. Residual JPEG splice acceptance (H-18)

- **Context:** after the partial fix and the already-present RSTn, MCU-count
  and exact-EOI checks, the 4,000-pair property run still accepted 5 byte-aligned
  tail swaps (0.0013, Wilson 95% [0.0005, 0.0029]). Each accepted output has
  identical SOF/quantisation/Huffman headers to one input and is a completely
  self-consistent baseline bitstream, so no parser error exists to detect.
- **Decision:** close H-18 with the measured point estimate and confidence
  interval disclosed in `docs/baseline/jpeg_splice_v0.2.0.txt`; do not add a
  statistical change-point or suffix heuristic. Such a rule could only be
  calibrated against this corpus and would risk downgrading clean high-contrast
  JPEGs, contradicting the mandatory-check definition of `verified` (ADR-H1).
- **Compatibility:** no score, emission, label mapping or clean-file behaviour
  changes. Unsupported variants remain `unverified`/`structural`; progressive
  JPEGs are never `verified`. The residual rate is a known limitation, not a
  claim that the 0.1% upper-CI boundary was met.

## D-46. H-19 stage 2 bounded PDF structure validation

- **Context:** stage 1 removed the false `decoded` label but left PDF decode
  mode lexical-only. The review oracle showed xref and `/Root` errors that
  `qpdf --check` rejects being accepted by CarveX. `qpdf` and `pdftoppm` are not
  installed here, so their control matrix is SKIPPED and not claimed; `pypdf`
  6.9.2 was used only to sanity-check generated fixtures.
- **Decision:** decode mode runs a bounded structural validator. Classic xref
  entries must point at the matching `N G obj`; xref streams must declare
  `/Type /XRef`, direct `/W`, `/Size` and a direct `/Length` that ends exactly
  before `endstream` (a trailing EOL is outside the stream); `/Root` resolves
  through xref entries or `/ObjStm` to `/Type /Catalog`; `/Prev` is cycle-safe
  with depth 8 and `/XRefStm` is followed; `/Encrypt` is flagged. Inflate work,
  entry count and dictionary scans are bounded by
  `--max-validator-memory`/the shared budget and fixed caps. Implemented
  checks that pass yield `decoded`/`verified`; any mismatch yields
  `partial`/`suspect` while the file and score stay unchanged; unsupported or
  budget-limited checks yield `unverified`/`structural`, never `suspect`.
  `validated_bytes` stays zero because these checks visit scattered offsets
  and are not a contiguous decoded payload.
- **Evidence:** eight red-first tests in
  `tests/pdf_structure_validation.rs` cover a valid minimal PDF, bad xref
  offset, bad `/Root`, xref stream, compressed catalog in `/ObjStm`, bad stream
  length, `/Prev` cycle and a tiny validator budget. All ten corpus PDFs scan
  `verified`/100 with no suspect result. X11 now checks a clean PDF and both
  corruption modes; the `qpdf --check` oracle controls remain SKIPPED.
- **Compatibility:** scores, emission, output names, CLI/JSON/CSV schema,
  warning codes and the default raw path are unchanged. B-06 still requires
  deep validation and now accepts complete stitched PDFs as `Decoded`; the
  stage-1 `Unverified` lexical case is still accepted when present.

## D-47. Repair revalidation and original-byte hash (H-21)

- **Context:** `--repair` appended a static PDF `%%EOF` and labelled the result
  `repaired` without re-opening it. X14's 70%-cut PDF still failed pypdf with
  "startxref not found", and no report field distinguished the repaired bytes
  from the bytes read from the source.
- **Decision:** a candidate is `repaired` only if the reconstructed bytes pass
  revalidation. PDF repair rebuilds a bounded classic xref table and trailer
  from scanned `N G obj` headers plus `/Root` or `/Type /Catalog`; JPEG repair
  appends `FF D9`; PNG repair appends the real length-zero IEND chunk with its
  CRC. PDF revalidation runs the H-19 structural validator even in default
  mode, because a bare `%%EOF` is exactly the false repair being corrected.
  The original score, `carved_size`, output naming and bytes read are
  preserved; only the footer/status changes after successful revalidation.
- **Failure path:** if reconstruction or revalidation cannot prove the repair,
  no suffix is written. The row keeps `status=truncated`, `validation=partial`,
  `validity=partial`, unchanged bytes and the exact note
  `repair attempted, not validated`. A `repair` check records the outcome.
- **Reporting:** optional `sha256_original` is additive in JSON and is appended
  as the final CSV column after `validity`. It is present when output bytes
  differ from bytes read (a successful repair) and absent otherwise. Schema 1
  strips it with the other additive v2 fields. A repaired file never counts as
  exact in the benchmark, and the published figures are untouched.
- **Salvage `[SHOULD]`:** no generic `--salvage` flag was added. The already
  accepted prefix-salvage behavior in B-07/A-05 remains, but cutting arbitrary
  files at the last valid marker/chunk and introducing a new partial-file
  policy is outside X14's contract. The `[SHOULD]` is therefore explicitly
  skipped rather than silently ignored; it should be revisited as a separate
  finding with its own acceptance cases.

## D-48. Abort reports and raw read-error scope (H-22)

- **Context:** `--on-read-error abort` stopped the process with exit 1 before
  the CLI built a report, so the reviewer's requirement for a partial report
  was not met. Raw and split sources also had no read-error policy: a failing
  `ImageSource` returned a plain I/O error without an unreadable range.
- **Decision:** E01 and raw/split readers record the failed range before
  aborting. Raw and split sources are wrapped in a policy source that retries
  the failing request one 512-byte sector at a time; good sectors keep their
  bytes, bad sectors become zeroes under `zero-fill` (the default) or are
  recorded for `skip`. E01 is not wrapped because it already knows its chunk
  granularity. VHD and VMDK retain their container-specific error semantics.
- **Partial report:** when an abort occurs and at least one unreadable range is
  logged, the CLI writes a schema-2 report with no carved files, the recorded
  `unreadable_regions[]`, `UNREADABLE_REGION`, and a new `READ_ABORTED` warning
  naming the first failed offset and reason. It then returns the original error
  so `main` still exits 1. `READ_ABORTED` is marked incomplete for report
  consistency, but the abort exit code is deliberately not converted to 3.
- **Compatibility:** default `zero-fill`, scores, emission, output names, CSV
  columns, warning-code meanings, published figures and the ordinary raw path
  are unchanged. The additive warning only appears on an aborted run with a
  recorded unreadable range. Real libewf and `.E02` controls remain SKIPPED
  because `ewfacquire` is unavailable; synthetic E01 fixtures and mock-source
  unit tests are the executed evidence.

## D-49. Container identity fields (H-25)

- **Context:** schema-2 `input.size` is the logical image length while
  `input.size_after` is the aggregate physical size of the backing files. For
  containers these differ, yet the report had no field naming that distinction,
  no resolved format, and no list of the backing files. Stdin had a null mtime
  but asserted `unchanged: true`, which implied a comparison that did not exist.
- **Decision:** keep every legacy field and its meaning. Add additive schema-2
  fields `input.logical_size`, `logical_size_after`, `physical_size`,
  `physical_size_after`, `format` and `files[]`. `format` is the resolved format
  and never `auto`. `files[]` contains each backing file's path, size and mtime
  as observed when the image was opened. For raw and split sources the logical
  size follows the current backing size; for E01/VHD/VMDK containers it remains
  the capacity fixed when the descriptor was parsed.
- **Stdin:** there is no stable backing path or comparison to perform, so
  `unchanged` is now `null` (JSON), not `true`. Logical and physical sizes both
  describe the temporary spool and `files[]` is empty.
- **Compatibility:** CSV columns, scores, emission, output naming, warning
  codes, published benchmarks and the default raw path are unchanged. Schema 1
  still removes the whole `input` object, so the legacy JSON shape is intact.
- **Evidence:** compressed E01, monolithicSparse VMDK and stdin are exercised
  by the Rust fixture tests and X22. Real libewf-made multi-segment E01/.E02
  containers remain SKIPPED because `ewfacquire` is unavailable.

## D-50. Unparsable volumes are explicit (H-26)

- **Context:** `VolumeSet` already collected one parse error per unrecognised
  or broken partition, but `scan` ignored those errors when another volume
  parsed. The S8 disk therefore exited 0, printed no warning, and reported the
  damaged NTFS range as `unsupported, scanned=true` without saying that raw
  carving there mixes allocated and deleted bytes. With no healthy companion
  volume, the warning and `info` error said only "no FAT32/NTFS filesystem
  found" even though an NTFS boot sector had been recognised.
- **Decision:** store parse failures as structured `VolumeParseError` values
  with `partition_index`, byte `offset` and the underlying `reason`. Wrap
  recognised FAT32/NTFS parse failures with the boot-sector offset and the
  affected metadata, e.g. `NTFS boot sector at byte 1048576 but $MFT
  unreadable`. Every detected parse error becomes a `VOLUME_PARSE_FAILED`
  warning in the report and on stderr; the code is incomplete, so a finished
  scan exits 3 even when other volumes are healthy.
- **Regions:** append an additive `may_contain_allocated` boolean to every
  `regions[]` row. It is true only for an `unsupported` fallback region backed
  by a parse error. The kind, offset, length and scanned flag keep their
  existing meanings; no region is removed or merged.
- **Info:** `info` continues to exit 1 when no supported volume can be selected
  (guide S8b), but `no_volume_message` now uses the structured parse errors
  instead of the misleading generic text. Explicit `--partition` and
  `--offset` views keep their existing error paths.
- **Compatibility:** S8/S8b do not abort. No new filesystem reconstruction,
  backup-boot-sector fallback, score, emission rule, output name, CSV column
  or published benchmark changed. The new warning code and region field are
  additive schema-2 details; schema 1 removes `warnings` and `regions`
  entirely.
- **Evidence:** three red-first CLI tests in `tests/volume_parse_errors.rs`
  and the strengthened X19 control. X19 uses real `mkfs.vfat`/`mkntfs`
  fixtures; the deterministic tests use an in-memory NTFS boot sector whose
  `$MFT` record cannot be parsed.

## D-51. Regions are a plan-backed coverage map (H-27)

- **Context:** `VolumeSet::regions` emitted one `free` and one `allocated` row
  spanning the entire volume for every filesystem. The rows overlapped, the
  sum of scanned row lengths exceeded `bytes_scanned`, and `info --json`
  reported the whole volume as free even when most clusters were allocated.
- **Decision:** make `scan::plan_extents` the single source of read extents and
  pass it to `VolumeSet::regions_with_plan`. Regions are now the merged free
  extents, their allocated complement, gap extents and unsupported partitions,
  sorted and made non-overlapping, with holes in malformed metadata filled as
  unscanned gap rows. The union always covers the logical image. A scanned
  row's `extent_id` is the index of the scanner extent that reads it; unscanned
  rows use `null`.
- **Summarisation:** `--max-regions` defaults to 4096, accepts 0 for unlimited
  detail, and merges adjacent rows that share a `scanned` state. Merged rows
  set `summarized: true`; different kinds merge to the additive `mixed` kind.
  Coverage and `Σ len(regions where scanned) == bytes_scanned` are hard
  invariants. If scanned/unscanned states alternate more often than the
  requested cap, exact accounting takes precedence over the numeric cap and
  the cap is treated as best-effort rather than silently corrupting the map.
- **Compatibility:** carved bytes, scores, emission, output names, CSV columns,
  warning-code meanings, schema 1 and the default raw path are unchanged.
  Schema 2 gains `options.max_regions`, `regions[].extent_id`,
  `regions[].summarized` and the `mixed` kind; all are additive.
- **Evidence:** three red-first CLI tests in `tests/region_coverage.rs` and the
  strengthened X20 control. X20 now checks full-image coverage, exact
  report/stats byte sums, real free extents and a raw-mode cap run; it was
  removed from xfail.

## D-52. Cooperative interruption instead of signal death (H-28)

- **Context:** the documented exit 4 for SIGINT was never implemented. The
  scanner had an unused atomic stop flag; a signal terminated the process with
  `rc -2`, leaving no report and no deterministic boundary between completed
  and half-written outputs.
- **Decision:** add a process-wide `ctrlc` handler (version 3.5.3, MIT OR
  Apache-2.0, MSRV 1.71). The first SIGINT/Ctrl-C latches a shared `Interrupt`
  state; a `Stop` controller folds it into the existing wall-clock stop check
  at candidate counting, candidate carving, fragment recovery and output
  writing. Work-unit limits remain separate, so deterministic budgets keep
  their existing semantics.
- **Exit behaviour:** a completed interrupted scan writes a schema-2 report
  with `interrupted: true` and an `INTERRUPTED` warning, then exits 4. The
  read-error `abort` partial-report path also records the flag if a signal
  arrived before it stopped. A second signal calls `process::exit(130)`
  immediately, preserving the operator's escape hatch. On Windows the same
  handler observes the Ctrl-C event; no Windows binary execution is claimed.
- **Output safety:** output writes remain temp-file-plus-rename. The stop flag
  is checked only between complete atomic renames, so no partially written
  output remains. A signal can omit later outputs, which is why the report is
  marked partial.
- **Compatibility:** no flag, score, emission rule, output name, CSV column,
  warning-code meaning or schema-1 field changed. Schema 2 gains the additive
  `interrupted` boolean; `INTERRUPTED` is a new warning code. The default
  scan path and frozen benchmarks are untouched when no signal is received.
- **Evidence:** the X21 control was red against the release binary with
  `exit -2 after SIGINT (expected 4)`. After the fix it passes with exit 4,
  schema 2, `interrupted: true`, an `INTERRUPTED` warning and no `.tmp` files.
  A direct two-signal control exited 130 without writing a report. X21 was
  removed from `scripts/repro_xfail.txt`.

## D-53. Measured, bounded B-06 fragment recovery (H-29)

- **Context:** the original B-06 acceptance fixtures used one-byte-different
  copies in tiny images. On a realistic 64 MiB FAT32 image with three real
  corpus files split in half and separated by three-cluster gaps, 0/3 files
  were stitched and the 5 s wall-clock budget was consumed. The same budget was
  burned on ordinary non-fragmented images, and a near-footer join could accept
  a plausible but incorrect PDF splice. The stage also accepted a lexical PDF
  label that was weaker than the authoritative `validity` model.
- **Heads and joins:** a contiguous, gap-free carve is not deep-probed; a
  truncated carve, a carve containing filler, or a PDF carve with more than one
  footer is probed. Footer indexing is deferred until a head actually needs a
  join. Skipped clusters are classified once per head as filler or another
  file's header; joins are ranked by unexplained gap, then evidence strength,
  gap size and tail position. Head/tail filler-boundary checks exempt the
  head's own first cluster and short zero-padded tails so compact fixtures
  still work. The contiguous filler scan inspects the same cluster samples
  with bounded block reads instead of one syscall per cluster. At most 32 join
  attempts per head and 16 validated alternatives are counted.
- **Budgets:** `--max-frag-candidates` is authoritative work units across head
  probes and join attempts; reaching it emits `FRAGMENT_CANDIDATE_LIMIT`,
  writes a partial report and exits 3 deterministically. `--frag-time-limit` is
  only a wall-clock safety net; reaching it emits `FRAGMENT_TIME_LIMIT`, sets
  `nondeterministic_stop: true`, writes a partial report and exits 3. Every
  probe and join runs inside the H-10 panic boundary and the H-05/H-06
  validator budgets.
- **Acceptance:** a stitch is emitted only when `validity == verified`; D-34's
  lexical `Unverified` PDF exception is rejected here. Accepted records gain
  the additive `alternatives` count in JSON and CSV; schema 1 strips it and the
  published CSV prefix is preserved.
- **Evidence:** red tests first failed with the missing schema property, the
  CSV header mismatch, S12 `file A not recovered exactly` and X15
  `realistic: 1 stitched output(s) are not the source files`. After the fix the
  D1-style profile measured recall 4/6 (Wilson 95% [0.3000, 0.9032]), splice
  rate 0/4 (Wilson 95% [0, 0.4899]) and junk-FP 0/24 (Wilson 95%
  [0, 0.1380]) in `bench/results/d1_fragment_profile.json`. X15 passes: bait
  produced 0 stitches, the ordinary image had no `FRAGMENT_*` warning, output
  hashes matched the plain scan, runtime stayed within 10%, the realistic
  supported filler-gap layout recalled 3/3 exact source files, and `--threads
  1` matched the default. The profile is development evidence, not the frozen
  lockbox; D-71 records the observed gap classes separately.

## D-54. Enforce G0-G9 locally and in CI (H-30)

- **Context:** G8 and G9 existed only as scripts or prose, fuzz smoke ran on a
  schedule rather than every push/PR, the packaged Windows executable was never
  run, and coverage remained at a 60% floor. The gate had to prove that earlier
  fixes were actually protected.
- **Decision:** `scripts/gate.sh` is the single entry point for G0-G9. It runs
  `scripts/perf_gate.py` (G8), `bench/render_docs.py --check` (G9), coverage at
  70%, fuzz smoke for all 13 targets, G7 golden, X01-X24 repro and benchmark
  smoke. `scripts/perf_gate.py` reports relative baseline regressions by
  default and fails on them only with `--strict`; absolute thresholds remain
  blocking. CI adds full-gate, repro, Windows-packaged, musl, SBOM and coverage
  jobs, and runs fuzz smoke on push and pull request as well as nightly.
- **Documentation check:** the README scan flag table is generated from
  `carvex scan --help` between explicit markers; `bench/render_docs.py --write`
  refreshes it and `--check` fails on any stale flag or default. Other numeric
  claims continue to come from committed generated results; no frozen result is
  regenerated.
- **Efficacy:** reintroducing H-05, H-07, H-08 and H-17 in a scratch worktree
  each turned the gate red at the corresponding Rust test or X control. The
  exact failures are recorded in `docs/PROGRESS.md`.
- **Consequences:** X15 timing now compares the median of nine paired
  B-06/plain runtime ratios after one discarded warm-up pair, because
  independently sorted medians can pair a cold plain run with a warm B-06 run
  and three pairs still produced a false failure on a shared runner. No output, score, default, CLI field,
  CSV column or frozen benchmark changes. A GitHub Actions run ID will be
  appended to `docs/VALIDATION.md` after HEAD is pushed; until then no CI run
  at HEAD was later recorded as `45035152791` and failed (8 success / 18
  failure); the remote failure causes were not observable through the public
  API and are not claimed.

## D-55. Reconcile status labels mechanically (H-31)

- **Context:** several guide items were marked DONE/FIXED in one document while
  residual defects were still described, the two status tables disagreed (for
  example A-03 and C-09), and the duplicate H-xx tables in `PROGRESS.md` were
  incomplete. A reader could not tell which status was authoritative.
- **Decision:** A-03, A-04, A-06, A-10, A-13, A-14, D-01, D-03, B-05 and C-01
  are labelled PARTIAL; C-09 is PARTIAL in both documents. `scripts/check_status.py`
  requires exactly one row per A-01…D-03 and H-01…H-34 in both documents,
  requires the statuses to agree, and permits `DONE`/`FIXED` only when every
  linked H-xx is CLOSED with a commit hash and an existing test/control path.
  The checker runs from `scripts/gate.sh` and a dedicated CI job.
- **Evidence:** the checker failed before the H-xx table was added and failed
  again when a temporary copy incorrectly marked A-10 as fixed; the real tree
  reports `STATUS GREEN` for 38 guide IDs and 34 harness IDs.
- **Consequences:** no scan behavior, score, default, CLI field, CSV column,
  warning code or frozen result changed. H-01 remains BLOCKED pending the
  user-supplied guide. H-34 was later closed in `f38a2ed`; H-04 is PARTIAL
  after the 0.3.0 release only because no Windows runner was available.

## D-56. Publish an evidence manifest and disclose unbacked figures (H-32)

- **Context:** `docs/VALIDATION.md` cited lockbox, benchmark, fuzz, coverage and
  CI results whose raw artefacts were either ignored by `.gitignore` or kept
  only at transient `/tmp` paths. No manifest connected those claims to
  digests, and `bench/lockbox.log` did not exist.
- **Decision:** `bench/results/EVIDENCE.md` and `bench/results/EVIDENCE.sha256`
  list 47 artefact digests, and `bench/results/evidence-bundle-0.2.0.tar.gz`
  prepares them for user publication. The 1.8 GiB lockbox dataset is not
  bundled; its checksum files and one-shot execution results are.
  `bench/lockbox.log` is a documented reconstruction, not an independent
  observation. Historical figures without retained raw logs are labelled
  **author-reported** in `docs/VALIDATION.md`.
- **Evidence:** the static red check showed the ignored paths and missing
  files; after the change all 47 bundled files passed `sha256sum -c` in an
  empty extraction directory. Quoted lockbox, full-benchmark, protocol,
  config and dist digests matched the computed values.
- **Consequences:** no lockbox dataset, benchmark, published figure, score,
  CLI/JSON/CSV field or warning code changed. The bundle is prepared locally;
  publication is the user's action. Remote job logs remain unavailable, so no
  CI failure cause is claimed.

## D-57. Correct README claims and retain historical paths (H-33)

- **Context:** static `grep` found an unsourced speed sentence and a Pillow
  wording that implied pixel verification. Review items about missing flags,
  the PNG CRC limitation, the fragment-recovery test and the missing `validity`
  field had already been fixed, and `docs/REPORT.md` line 853 still
  carries historical Pillow wording. Generated ground truth and frozen benchmark
  reports contain `/home/kali` absolute paths from the original author machine.
- **Decision:** the speed sentence is replaced by a reference to the generated
  timing table; the CFReDS wording now says the files were structurally parsed,
  not verified (see `validity`), and preserves the published counts and hashes.
  The README tree now lists `fuzz/`; the flag table remains generated from
  `carvex scan --help` by `bench/render_docs.py --check`. Corrected or
  `NOT-REPRODUCED` review items are recorded with their grep output in
  `docs/baseline/h33_static_red.txt` and `docs/PROGRESS.md`.
- **Evidence:** the pre-fix grep output is committed as
  `docs/baseline/h33_static_red.txt`. After the change,
  `python3 bench/render_docs.py --check` is green and the full gate remains
  green. No generated result, benchmark figure, score, warning code, CLI field
  or output name was changed.
- **Consequences:** `/home/kali` paths in committed ground truth, logs, reports
  and historical documentation are provenance, not required local paths. They
  are deliberately not rewritten: doing so would alter frozen 0.1.0/0.2.0
  artefacts and their SHA-256 values. Portable rebuild scripts create new
  fixtures with their own paths; frozen results stay byte-identical. The
  historical Pillow sentence in `docs/REPORT.md` is retained as a published
  figure, with an added honesty note that it records a structural check rather
  than pixel verification; no count, hash or conclusion changed.

## D-58. Apply A-10 quality limits as behavior-preserving stages (H-34)

- **Context:** the reviewer's function lengths were heuristic, so H-34 required
  a tool check first. With `too-many-lines-threshold = 120`, clippy reported 15
  non-test functions over the limit (largest `cli::scan` 360 lines) plus one
  121-line test. Enabling `missing_docs`, `pedantic` and `too_many_lines`
  together produced 698 warnings. The repository had no public `missing_docs`
  lint and no staged pedantic configuration.
- **Decision:** long functions are split only by moving code into named helpers;
  no branch, read, budget check, warning, score or output-path logic is
  rewritten. `clippy.toml` fixes the threshold at 120 lines for non-test
  functions. `src/lib.rs` enables `#![warn(missing_docs)]` and documents the
  whole public API. Pedantic linting is staged: stage 1 enables
  `cloned_instead_of_copied`, `doc_markdown`, `field_reassign_with_default`,
  `manual_let_else`, `map_unwrap_or`, `redundant_closure_for_method_calls`,
  `stable_sort_primitive` and `uninlined_format_args`. Stage 2 (numeric casts)
  and stage 3 (API ergonomics such as `must_use` and argument-count redesign)
  are deferred so no cast needs a blanket `allow` and each stage stays
  reviewable. Every retained `too_many_arguments` allow has a comment explaining
  that the argument order preserves provenance; the sole `too_many_lines` allow
  is test-only and justified at the test.
- **Evidence:** the verbatim red inventory is in
  `docs/baseline/h34_static_red.txt` and `docs/baseline/h34_static_red_all.txt`;
  the clean checks are in `docs/baseline/h34_static_green.txt`. The `WORK_LIMIT`
  string was byte-compared with `HEAD:src/scan.rs`, and the multiset of string
  literals in that file is unchanged. `FragmentScanState` takes its start time
  at `recover_fragments` entry so head-classification reads still consume the
  `--frag-time-limit` safety net. G7 golden output and the full G0-G9 gate are
  green after the refactor.
- **Consequences:** no CLI flag, default, JSON/CSV field, warning code or
  meaning, score, emission, output name, frozen benchmark, published figure or
  raw-image path changed. The H-34 commit-hash backfill was committed
  separately; D-03 remains PARTIAL because H-01 is blocked and H-04 is only
  PARTIAL for Windows-runner execution.
- **H-22 closure (last item):** all 13 `#[allow(clippy::too_many_arguments)]`
  attributes were removed by local stage/parameter structs: `RegionPlanSpec`
  for region planning, `CandidateScanCtx` for header discovery, a report
  assembly spec, repair/finalize specs, three B-06 stage contexts, and an
  output-writing context. Two pre-existing clippy `too_many_lines` overflows
  were split into `finalize_analyzed_candidate` and `pdf::complete_analysis` in
  the same pure refactor. No test file was edited for H-22. clippy's code-line
  metric is the gate of record and reports 0 findings; `scripts/fn_len.py`
  remains informational and reports 3 raw-line findings because it also counts
  comments, blank lines and struct literals. G7 golden output is byte-identical
  to the pre-refactor log, and G8 is `PERF GREEN` with `rand64m` at 1.051x of
  the committed baseline.

## D-59. Release 0.3.0 with local tag and disclosed execution limits

- **Context:** harness §7 asks for a Windows-runner execution of the packaged
  executable and a green CI run at HEAD. The final source commit `360f154`
  had a green local G0-G9 gate, but public GitHub API run `45095456201` at
  that HEAD completed `failure` (9 success/18 failure) and the job-log
  endpoint returned `403 Must have admin rights to Repository.`. A real
  Windows runner was not available locally; Wine 10.0 was installed and could
  execute the PE binary on Linux.
- **Decision:** version `0.3.0` was committed as `5480f10` and tagged
  `v0.3.0` locally only; the tag was not pushed. Both `dist/` binaries were
  rebuilt from that tag with `--locked`, executed, stripped as appropriate,
  and checksummed. The musl binary ran natively and the Windows binary ran
  under Wine 10.0; no Windows-runner or CI-green claim is made. The release
  step changes no source after the tag: publish changes are limited to
  `dist/` and documentation.
- **Evidence:** `dist/README.md`, `dist/SHA256SUMS` and `docs/VALIDATION.md §8`
  record the build environment, commands, both binary SHA-256 digests, the
  `--version`/report commit `5480f1084f85`, the absolute G8 results, and the
  full repro result on the musl binary (2 failures: X23 and R5). CI run
  `45095456201` is recorded in `docs/VALIDATION.md §3`.
- **Consequences:** H-04 remains PARTIAL solely because the packaged Windows
  binary was not executed on a Windows runner. H-15 remains CLOSED as a
  measured musl/release-binary behaviour with no allocator change. H-30 is
  not green remotely; no fuzz, coverage, lockbox, full-benchmark or Windows
  result is inferred from the failed remote jobs. No binary, source flag,
  score, warning code, frozen benchmark or published figure was tuned.

## D-60. CI-only fixes after the 0.3.0 publish push

- **Context:** pushing `a1eaf41` produced public Actions run `45102520898`,
  which failed with the same five job classes as the previous final-head run.
  Job logs remained unavailable (`403 Must have admin rights to Repository.`).
  Three causes were reproduced locally: the pinned `1.100.0` toolchain rejects
  cargo-fuzz's `-Z` flags unless `+nightly` is explicit; the GitHub Ubuntu image
  does not list `musl-tools`; and `bench/images/*.img` is git-ignored, so the
  packaged smokes and repro job passed paths to files that a clean checkout
  never contains.
- **Decision:** change only `.github/workflows/ci.yml` and add the no-root
  fixture generator `scripts/make_smoke_image.py`. Fuzz jobs use
  `cargo +nightly fuzz run`; the musl job installs `musl-tools`; the musl and
  Windows packaged smokes and the repro job generate the committed-corpus FAT32
  smoke fixture instead of depending on ignored images. The Windows `Tests`,
  `release gate` and `review repro` steps tee their logs and emit failure-tail
  annotations so the next public run can expose details through the API. No
  source, CLI default, output name, JSON/CSV field, score, warning code, frozen
  benchmark, published figure or binary is changed, and the local-only `v0.3.0`
  tag stays at `5480f10`.
- **Evidence:** local red `cargo fuzz run` log and green
  `cargo +nightly fuzz run` control; public runner-image package list;
  `git check-ignore` on `bench/images/mixed_fat32.img`; generated-fixture `info`
  and `scan` runs on both the Linux release binary and the Windows binary under
  Wine 10.0; and a green clean-checkout `scripts/gate.sh` (`GATE GREEN`, 34
  scenarios/0 failures/5 SKIPs). Run `45102520898` and the step conclusions are
  recorded in `docs/VALIDATION.md §3`.
- **Consequences:** these are CI/tooling changes only. The remaining three
  failure classes are still unresolved and are tracked as N-38/N-39; no green
  CI claim is made until a new run is observed.

## D-61. CI annotations, byte-exact fixtures, and explicit musl target

- **Context:** run `45110520472` at `80155cd` still had four failing jobs.
  Public annotations exposed `error[E0463]: can't find crate for 'core'` in
  the musl build, four Windows `pdf_structure_validation` failures, and
  generic `PASS` lines in the gate/repro annotations that obscured the real
  failing control.
- **Decision:** install the musl target explicitly for the pinned `1.100.0`
  toolchain (`7c107bb`, N-40); mark `*.pdf` as `-text` so PDF byte offsets
  survive Windows checkout (`53ca337`, N-39); and emit only `FAIL`/`XPASS`
  lines plus the scenario summary from gate/repro failures (`7ba1557`).
  The X05 flood fixture was also grown from 64 MiB to 256 MiB and streamed
  (`700221f`) so the one-second time-budget control could expire on faster
  hosted runners. These are workflow, checkout-attribute and repro-harness
  changes only.
- **Evidence:** local CRLF conversion reproduced exactly the four Windows PDF
  test failures, and a simulated `core.autocrlf=true` checkout with
  `*.pdf -text` restored all eight tests. `case_x05` returned no error against
  a fresh 256 MiB fixture in 1.672 s. A full `RUSTFLAGS='-D warnings'
  bash scripts/gate.sh` was green before each commit (`GATE GREEN`, 34
  scenarios/0 failures). Public run `45116318679` then showed 25 successes
  and two failures, both annotated with the X05 time-budget error, and run
  `45118790273` showed 26 successes with only the repro job failing.
- **Consequences:** musl and Windows `Tests` became green remotely. The X05
  failure remained, so no green CI claim was made at this step.

## D-62. Repro fixture cache isolation and observed green CI

- **Context:** the 256 MiB X05 fixture in `700221f` did not change the remote
  failure in `45118790273`. The repro workdir was `target/repro-ci`, which
  `Swatinem/rust-cache@v2` restores; `case_x05` only generates the flood
  fixture when it is absent, so CI kept reusing an older 64 MiB fixture.
- **Decision:** move the CI repro workdir to `$RUNNER_TEMP/carvex-repro` and
  delete/recreate it before the smoke fixture is generated (`86ae15f`, N-38).
  Backfill the public run IDs, commit hashes, N-35…N-40 and the observed
  conclusions in `docs/VALIDATION.md`, `docs/PROGRESS.md` and this file. No
  source, CLI default, output name, JSON/CSV field, score, warning-code
  meaning, frozen benchmark, published figure or binary is changed.
- **Evidence:** public run `45121233267` at `86ae15f` is `completed` /
  `success` with 27 of 27 jobs successful: all 13 fuzz targets, both packaged
  smokes, `MSRV`, `SBOM`, `actionlint`, `benchmark smoke`, `coverage (≥70%)`,
  `dependencies`, `release gate (G0-G9)`, `review repro assertions`,
  `status labels (H-31)`, `test (macos-latest)`, `test (ubuntu-latest)` and
  `test (windows-latest)`. Job-log bodies remained unavailable (`403`), but
  run status, step conclusions and annotations were observed directly.
- **Consequences:** N-35…N-40 are closed as CI-green. The local annotated
  `v0.3.0` tag remains at `5480f10`; it was later pushed to origin (remote tag
  `271026578c532cfe4fb7e8600205558caa818c78` points at commit
  `5480f1084f85cc91f68d94c76c703d1348a65b5e`, confirmed 2026-11-23). This is
  the first observed green public run and the sole basis for a CI-green claim.

## D-63. Acceptance guide added verbatim (H-01)

- **Context:** R2-05/H-01 required `docs/guide.md` to be added to the repository
  exactly as supplied by the user: 924 lines, SHA-256
  `22ee8122209e9eda11d6f32a1e650e8f8e48f5610bd808309d94f436e5e78265`. The
  pre-fix state was `docs/guide.md` absent (`/tmp/opencode/h01_red.txt`).
- **Decision:** copy `/home/kali/guide.md` to `docs/guide.md` without editing,
  reflowing or reconstructing it; verify line count, digest and byte equality;
  close H-01. D-01 remains the historical record of the truncated pre-harness
  guide, and D-33 remains the record of the earlier missing-file state; the
  complete guide is now the acceptance baseline the harness refers to.
- **Evidence:** `wc -l docs/guide.md` = 924; `sha256sum` matches the expected
  value; `cmp docs/guide.md /home/kali/guide.md` is silent (byte-for-byte
  identical). No source, CLI, score, warning code, frozen result or binary
  changed.

## D-64. Lazy scan task planning (ADR-H11, H-12b)

- **Context:** round-2 X08b reproduced a CRIT regression in the released
  0.3.0 code: a 512 TiB VMDK `ZERO` extent made `prepare_scan_plan` push one
  `Task` per 4 MiB chunk before any budget was consulted. The plan grew by
  about 10 MiB/TiB, reaching 5.9 GiB and SIGKILL; `--time-budget 20` was
  ignored during planning. The same path is reached by every container whose
  logical size is declared in a header.
- **Decision:** replace the eager `Vec<Task>` with a `TaskPlan`/`TaskIter` view
  over resolved extents. Candidate counting, quota assignment and scanning
  proceed in bounded 64-task batches; totals and `bytes_in_extents` come from
  extent arithmetic, and fragment footer scanning uses the same iterator.
  The order of tasks and the deterministic global candidate quota are
  unchanged, so output bytes, scores, warnings and G7 results are unchanged.
  The existing wall-clock stop is now checked while planning and produces the
  usual `TIME_LIMIT`/`nondeterministic_stop` partial report instead of an
  abort. No default, threshold, frozen result or scoring rule changed.
- **Evidence:** red observations are in `docs/PROGRESS.md` (task H-12b). After
  the change, `scripts/repro_x08b.py --carvex target/release/carvex` reported
  `exit=3`, `5.01 s`, `28.0 MiB` peak RSS with a report under `RLIMIT_AS=1 GiB`;
  `scripts/repro_x26.py` reported typed errors and ~10 MiB RSS for the VHD,
  E01 and 100,000-segment split declarations; the Rust regression test aborted
  before the fix and passes afterwards. `G7`/`G8` are rechecked by the commit's
  full gate run.

## D-65. Image parser deny-lints (ADR-H11, R2-11)

- **Context:** round-2 R2-11 required `src/image/{mod,e01,split,vhd,vmdk}.rs`
  to deny `clippy::indexing_slicing`, `clippy::unwrap_used` and
  `clippy::expect_used`, because hostile container fields must not panic on
  unchecked indexing/unwrap. The hidden `fuzz_support` fixture module inherits
  module lints and is not production parsing code.
- **Decision:** add `#![deny(...)]` to the five image parser modules. Replace
  production indexing with checked access (`get/get_mut`, `starts_with`,
  `try_into`, iterator-safe patterns) and return typed I/O or parse errors.
  Keep `fuzz_support` behind a narrow module-level `#[allow(...)]` with an
  H-11 justification, and keep test modules under justified `#[allow(...)]`
  where fixed fixtures intentionally use direct indexing. No parser behavior,
  default, score, warning code, or accepted output changes on valid inputs.
- **Evidence:** `cargo clippy --all-targets --all-features --locked -- -D warnings`
  passes with the denies enabled; `cargo test --all-features --locked` passes.
  The full gate run for the R2-11 commit records G0-G9.

## D-66. Worker scratch reuse for scan/carve (ADR-H15, R2-02)

- **Context:** R2-02/H-15 reported musl parallel carve slowdown. Profiling and
  harness evidence indicated per-task/per-candidate buffer allocation churn in
  count, scan and carve workers. The user approved allocator/glibc remedy work
  under R2-02 conditions.
- **Decision:** reuse per-worker scratch buffers in scan count/scan passes and
  carve workers via `rayon::map_init`. `count_task` and `scan_task` take
  `&mut Vec<u8>` scratch and `resize/read_exact_at` into it; `carve_one`
  reuses scratch for the initial buffer and `grow_carve_buffer` preserves the
  drop-buffer/release-permit/reserve/resize sequence so waiting workers hold no
  permit during growth. Retained carve scratch is capped at 1 MiB after each
  candidate.
- **Evidence:** `cargo test --all-features --locked` passes. Dev release
  measurement on this host (not a shipped musl binary): X23 default 0.168s vs
  1-thread 0.591s (0.28x), R5 0.280s, rc 0. Shipped Linux/musl 0.3.1 binaries
  must be measured at R2-10 before claiming the harness X23/R5 targets.

## D-67. JPEG content-continuity check (ADR-H18, R2-03, supersedes D-45 disclosure)

- **Context:** R2-03 required a deterministic content-continuity check for
  baseline JPEGs, superseding D-45's disclosure-only closure. D-45 remains
  historical record. R2-03 target: eval Wilson 95% upper bound <= 0.1% and
  clean-file control >=1000 with 0 false `suspect`.
- **Decision:** add a conservative pixel-domain seam check in
  `src/signatures/jpeg.rs::content_continuity_check`, invoked from
  `validate_jpeg_detail` only for baseline sequential JPEGs that decode and
  pass strict scan. It computes 8-row boundary edge scores and 8-row band-mean
  steps, flags only strong outliers, and returns `content_continuity` checks
  (`ok true/false/null`). Failed check maps to `validation=partial`,
  `validity=suspect`, `status` unchanged, score unchanged. Progressive/
  arithmetic/unsupported variants remain not applicable.
- **Evidence:** clean-file control 1000 generated baseline JPEGs: 0 suspects.
  Corpus baseline JPEGs: 0 suspects, progressive JPEGs remain structural. Dev
  splice set (disjoint, 500 generated, same-dim/quality compatible header
  tail swaps): 0/499 verified. Eval splice set (disjoint, 4000 generated):
  2/3997 verified (`suspect` 3987, `partial` 8), Wilson 95% upper bound
  0.182% (> 0.1% target). The check improves over D-45 disclosure but does
  not meet the R2-03 upper-bound acceptance criterion on this eval set; the
  residual accepted splices are low-contrast/flat boundary cases where the
  conservative threshold avoids clean false positives.
- **Compatibility:** no score, status, emitted bytes, or benign fixture
  validity changes observed on corpus/clean control; G7 must remain green.
- **User decision (2026-11-23, option A):** accept R2-03 as PARTIAL for 0.3.1
  release with measured evidence recorded honestly; authorize aggressive
  content-continuity heuristic as a measured follow-up prototype (not a
  default change without FP/upper-bound evidence). R2-10 may proceed.
- **Follow-up investigation (2026-11-23):** D-67-style clean control
  (1000 baseline JPEGs, gradient/noise/photo, 256x256, q90) re-confirmed 0
  suspects. A harder synthetic hard-edge/text control (1010 images including
  corpus JPEGs) produced 44/1010 suspects, all in hard-edges/text class; this
  is not the D-67 control class and indicates FP sensitivity to sharp synthetic
  edges. No aggressive threshold/configuration was promoted to default because
  no measured configuration met both clean-control and eval Wilson 95% upper
  <= 0.1% criteria without FP risk. R2-03 remains PARTIAL.
- **Re-measurement (2026-11-24):** deterministic eval on current release binary
  (`jpeg_splice_property.py --pairs 4000`, seeds 20261121/70612, same pool
  classes): 3/4000 verified, Wilson 95% upper 0.22%. Accepted residuals
  (cases 940, 1131, 2006) were all gradient_noise low-contrast/flat boundary
  splices; edge/variance pixel-domain signals showed no clean separation.
  The clean-control criterion is met; the upper-bound criterion remains unmet.
- **External H-09 reopen (2026-11-26):** the authority file
  `/home/kali/harness.md` scopes H-09 to the point-estimate accepted fraction
  (`<= 0.1%`) plus static confirmation of the unsupported-variant fall-through,
  which is weaker than R2-03/D-67's Wilson upper-bound rule. Re-measurement on
  HEAD `0f3522a` gave 3/4,000 = 0.0008 (upper 0.0022); the assertion passes the
  external rule and `tests/jpeg_unsupported_validation.rs` pins progressive
  JPEGs to `unverified`. This does not change D-67 acceptance: repo H-18 stays
  PARTIAL. Evidence: `docs/baseline/jpeg_splice_h09_reopen.txt`.

## D-68. `--repair` supports PDF, PNG and JPEG footers (harness §9 Q11)

- **Context:** the honesty fix stopped labelling 60 %-cut PNG/JPEG files
  `repaired`, but footer-only PNG/JPEG repairs still pass decode revalidation
  (`tests/repair_validation.rs`). Harness §9 Q11 offered "PDF only" versus a
  decode-proven PNG/JPEG repair.
- **User decision (2026-11-25):** document all three formats as supported.
  `--help`, README and REPORT now state that only a rebuilt footer that passes
  revalidation is labelled `repaired`; a cut through the payload stays
  `partial`. No output bytes, thresholds or benchmark results change.
- **Regression pin:** `mid_stream_png_and_jpeg_repair_stays_partial` covers the
  S-REPAIR-style 60 % rows for both formats; the existing footer-repair test
  covers the successful PNG/JPEG cases.

## D-69. Low-score drops emit `BELOW_MIN_SCORE` (harness H-02)

- **Context:** with `--validate decode`, three bad PNG IDAT CRCs score 35,
  below the default `--min-score 50`; the candidate was discarded with only a
  stats counter as evidence.
- **Decision:** aggregate withheld candidates into one `BELOW_MIN_SCORE`
  warning carrying the count and first offsets. The code is intentionally not
  in `Warning::incomplete()`, so the run still exits 0 and `--min-score`
  semantics are unchanged. Whether `suspect` files should be exempt from the
  threshold remains open (§9 Q3); the repository already emits CRC-bad PNGs as
  `suspect` when they pass the threshold.

## D-70. Last verified byte ends a truncated file (harness H-25)

- **Context:** zero padding was trimmed, but `0xFF`/random/text padding after a
  60 % cut was swallowed by JPEG segment lengths, PNG IDAT declared lengths and
  PDF `search_end`. A truncated JPEG next to a complete JPEG was rejected
  outright, and a 48 MiB `0xFF` run pushed the candidate over
  `--max-truncated-size` with no warning.
- **Decision:** for an incomplete candidate, end at the last byte the parser
  can prove. JPEG ends at the first invalid continuation once SOF is present.
  Between entropy scans a real DHT/DQT/DNL/DRI/SOS marker is resumed only when
  its payload is structurally valid; a random `FF xx` ends the file there.
  Requiring the whole remainder to parse to EOI was rejected because it wrongly
  cut progressive JPEGs at their first intermediate marker when trailing
  padding was present. A trailing `0xFF` run is never entropy data because a
  literal FF is stuffed as `FF 00`. PNG ends at the last IDAT byte whose
  inflated scanline has a valid filter byte; inflation is bounded by the
  validator budget and fed in 64-byte pieces so zlib lookahead is not claimed
  as verified data. CRC-failing complete chunks remain `suspect` (D-67/Q4)
  and are not silently promoted. PDF ends after the last complete top-level
  `endobj` (keeping its own line terminator), or at the bounded Flate or
  `DCTDecode` prefix of an open stream; with no complete object only the
  header/object header is kept. Long zero/FF fill is left to the generic
  O(tail) trimmer so zero-padded rows stay byte-identical. `tail_trimmed_bytes`
  records the removed bytes.
- **Warning shape:** `TRUNCATED_TOO_LARGE` is a structured warning with `type`,
  `offset` and `size`, is in the exit-3 set, and keeps the existing stat. All
  other warnings keep their two-field shape via `skip_serializing_if`.
- **Consequences:** complete files, zero-padded results and scores/labels are
  unchanged. A cut before the first complete object is conservative and may be
  shorter than the raw prefix; a cut through a compressed stream keeps only the
  provably inflatable prefix. `--repair` appends at the new end and still
  revalidates (D-68). Regression guards are in `tests/regress_tail_padding.rs`
  and `tests/properties.rs`; frozen benchmark artifacts were not touched.

## D-71. B-06 keeps precision over recall on unsupported gaps (harness H-17)

- **Context:** the S-B06 residual asked for a final decision on random and
  other-file gaps. On `ab725a6`, a false PDF stitch made with raised limits was
  accepted without a decode proof; the current join path rejects that stitch,
  but a random gap still leaves the three S-B06 files unrecovered. The two B-06
  limit flags also did not bind on that fixture because the stage stopped
  before consuming a work unit.
- **Decision:** precision over recall is the intended final design. Supported
  gaps are (1) filler — all-zero bytes, at most eight distinct byte values, or
  a repeated byte pattern with period ≤ 32 — and (2) an embedded file header
  at the gap start, which is only ranking evidence. Random/high-entropy gaps
  and arbitrary other-file bytes are not supported gap classes. A stitch is
  never `verified` without the deep validator; the lexical PDF exception is
  permanently rejected here. This closes the H-17 scope without changing the
  join algorithm, defaults or frozen benchmark.
- **Evidence:** `scripts/h17_gap_matrix.py` regenerates the S-B06 fixture and
  writes `bench/results/h17_gap_matrix.json`. The current release binary
  recovered 3/3 zero-gap files, 0/3 random-gap files and 2/3 embedded-header
  files, with zero spliced stitches and no `FRAGMENT_*` warning. The current
  A.2 row is retained as the historical `ab725a6` observation; the generated
  matrix is the current measurement. `tests/fragment_recovery.rs::h17_fragment_limits_bind_on_a_hostile_layout`
  plants 64 distinct truncated heads and valid footers: `--max-frag-candidates
  1` emits `FRAGMENT_CANDIDATE_LIMIT` deterministically, a 1 ns
  `--frag-time-limit` emits `FRAGMENT_TIME_LIMIT` with
  `nondeterministic_stop=true`, and raised limits emit neither. The existing
  X15/S12 controls and the exit-3 Q4 answer are unchanged.
- **Documentation:** `scan --help` and the README now name the supported gap
  classes; the harness A.2 row carries the closure note. No test result,
  report value or output byte from the frozen benchmark changed.

## D-72. Repairs require a deep proof in both validation modes (external H-04 item 0)

- **Context:** the external harness revision for `f5bda4a` re-measured the
  S-REPAIR rows and found a remaining over-claim: a JPEG cut mid-scan was
  labelled `repaired`/`repaired` in default `--validate structure` mode after
  appending EOI. Pillow could open the result, but rows ≥ 271 of 480 were grey
  decoder fill; the structural parser could not distinguish those invented
  pixels from recovered content. The acceptance test uses the full
  `jpeg_01_baseline_640x480.jpg` corpus file cut at 30/60/90 %.
- **Decision:** a repair changes output bytes, so both `--validate structure`
  and `--validate decode` now run the same deep validator on the rebuilt
  bytes. `Validation::Decoded` is the only accepted outcome;
  `Validation::Unverified` is not a proof. A failed repair keeps the carved
  prefix byte-identical, stays `truncated`/`partial`, and records
  `repair attempted, not validated`. This narrows D-68's footer-repair
  acceptance but does not change any default scan path.
- **Evidence:** `tests/repair_validation.rs::jpeg_mid_stream_repair_structure_mode_stays_partial`
  was red before the fix (the 60 % cut was reported `repaired`) and green
  after it. The footer controls and the PDF repair rows remain green.
- **Residual:** this decision removed the false label; the remaining gap was
  recovery of mid-stream PNG/JPEG cuts. That gap is addressed by D-73 and
  closed by the user's 2026-11-27 confirmation of §9 Q12.

## D-73. `--repair` crops PNG/JPEG to the recovered rows (external H-04 items 1–7 / §9 Q12)

- **Context:** the user answered §9 Q11 with "repair PNG and JPEG too, with
  verification". D-68 implemented the narrower footer-only reading, which is
  honest but leaves a truncated PNG/JPEG — the common case — unrecoverable.
  External H-04 items 1–7 define a crop-to-recovered-rows design; Appendix C.3
  (`scripts/repair_ref.py`) demonstrates it on the corpus. §9 Q12 asked which
  reading is intended; the user confirmed the recommended crop design on
  2026-11-27.
- **Decision:** implement the crop design behind the existing `--repair` flag.
  PDF keeps the xref/trailer rebuild. Non-interlaced PNG keeps complete
  scanlines and rewrites IHDR; baseline sequential, interleaved, single-scan
  JPEG without restart intervals keeps complete MCU rows and rewrites SOF.
  Progressive/non-interleaved/restart-interval JPEG and interlaced PNG stay
  `partial`. Missing pixels are never invented: full-canvas fill is rejected,
  no `--repair-fill` flag is added, and no output is labelled `repaired` without
  a deep validator proof in both `--validate` modes (D-72). This supersedes the
  narrower "mid-stream cuts stay partial" wording of D-68; the user confirmed
  this choice under §9 Q12 on 2026-11-27.
- **Evidence:** `src/signatures/repair_image.rs` ports the Appendix C.3
  algorithm with checked arithmetic and decode budgets. `tests/regress_repair_images.rs`
  checks at least three corpus files per format at 30/60/90 %, both validation
  modes on two formats, a cut in the next PNG chunk header, padding after the
  cut, unsupported variants and intact controls. The independent Pillow oracle
  verifies output and compares retained rows with the original (JPEG ignores
  the last two rows for chroma edges). `tests/repair_validation.rs` retains the
  item-0 regression name but now asserts the no-invented-rows invariant. A full
  corpus sweep repairs the same 9/10 PNG and 8/10 JPEG files as Appendix C.3 at
  all three cut points; the interlaced PNG and two progressive JPEGs stay
  `partial`.
- **Report fields:** validated crops emit `repair_validated`, `recovered_rows`,
  `total_rows`, `recovered_fraction`, `sha256_original` and `sha256_output`,
  plus the note `repair cropped to N of M rows`. Schema 1 omits these additive
  fields. CSV columns and the frozen benchmark inputs and metrics are unchanged.
- **Residual:** none for the confirmed crop target. The user rejected the
  footer-only alternative under §9 Q12 on 2026-11-27, so H-04 is closed with
  this implementation; unsupported image variants remain honestly `partial`.

## D-74. H-27 is an opt-in complement, not a replacement for content oracles

- **Context:** Appendix C.4 E1 showed that a deleted file split around a live
  allocated island is invisible to default carving and to B-06, while the same
  layout is 40/40 recoverable when the free extents are concatenated into one
  logical stream. The opposite layout, where the island is the interior of
  another deleted file, produced 0/40 exact results with that stream: adjacency
  in free space is not proof of content.
- **Decision:** add `--free-stream` as a second, opt-in pass over the free
  extents of exactly one selected volume. The pass uses the unchanged scanner
  on a read-only logical `ImageSource`; physical provenance is restored by
  translating logical ranges back to free extents before reporting. Results
  already produced by the normal pass are suppressed by SHA-256. Files split
  backwards (second fragment at a lower LBA) are explicitly unsupported.
  `--no-fs` and `--mode allocated` are rejected with this flag.
- **Evidence:** `tests/free_stream.rs` proves exact recovery past a live island,
  physical `fragments[]` containment, no false `verified` on a dead island, and
  an unchanged default report. `scripts/part_t/h27_free_stream.py` records
  80/80 exact live-island trials (Wilson lower 0.954), 0/80 dead-island false
  `verified` trials, and the measured extra pass cost. The `free_stream` fuzz
  target exercises hostile extent maps.
- **Consequences:** default runs, golden D0, schema-1 output, CSV header and
  frozen benchmark metrics are byte-identical. Only `options.free_stream` and
  a `FREE_STREAM` warning are added; accepted rows use `source="free-stream"`
  with physical `fragments[]`. H-27 complements, and does not replace, B-06 and
  the PNG/JPEG content-oracle items H-30/H-32.

## D-75. H-28/H-29 metadata is opt-in evidence, never a validity shortcut

- **Context:** deleted FAT directory entries and NTFS MFT records can supply a
  name, a start extent or an exact runlist, but a deleted entry proves neither
  that the data survived nor that a reused record still belongs to the same
  file.
- **Decision:** one `--metadata` flag enables both parsers. FAT entries require
  a plausible data-area cluster, bounded size and a signature match; LFN names
  are bound by the checksum of the original 8.3 name and a mismatch drops only
  the name. Live subdirectories and bounded deleted directories are traversed.
  NTFS records require `FILE`, a clear in-use bit, a plausible sequence/parent,
  valid fixups and in-volume runlists. `$ATTRIBUTE_LIST`, extension records,
  compressed or encrypted data, sparse/zero tails and overwritten clusters are
  reported as unsupported or partial. Metadata never sets `verified`; the deep
  validator still does. Rows that match an existing carved row by offset and
  exact size are annotated, not duplicated. Output filenames never use
  recovered names. All normal CLI filters and `--max-memory`,
  `--max-candidates`, `--max-output` and `--time-budget` bound the pass;
  `--verified-only --metadata` writes no rows. A zero time budget is unlimited.
- **Evidence:** `tests/metadata.rs` (22 tests, including a 10,000-case runlist
  round-trip property, fixup/hostile-record paths, deleted/out-of-order LFN
  slots and checksum failure, live-subdirectory traversal, reallocated FAT
  cluster, CLI filters/bounds, manifest coverage and unchanged default output)
  and `bench/results/h28_h29_metadata.txt` (100/100 FAT exact, Wilson lower
  0.963; 10/10 NTFS exact, Wilson lower 0.722). New fuzz targets `fat32_dir`
  and `ntfs_deleted_mft` cover both parsers. CI exposed the mkntfs dependency:
  the five mkntfs-dependent tests now fail loudly when the tool is missing on
  Linux and are visibly `#[ignore]`d with a reason on Windows/macOS; CI run
  `45945194205` completed success (30/30) after the portability fix and `dist/`
  rebuild.
- **Consequences:** default reports, schema-1 output, CSV header, golden D0 and
  frozen benchmark metrics stay byte-identical. Only `options.metadata` and
  additive per-file fields appear when the flag is enabled; B-01 is now
  PARTIAL (FAT32/NTFS only; no exFAT metadata).

## D-76. H-31/H-32 prototypes do not authorize production promotion

- **Context:** the classic-xref PDF prototype recovered 100/100 random-gap
  trials but only 39/100 other-PDF-interior trials, with 61 false accepts
  (Wilson 95% [0.300, 0.488]). The JPEG seam experiment recovered 12/100
  random-gap and 0/100 other-JPEG-interior trials, with 19 and 22 false accepts;
  the X1 prefilter itself remained 572/572 on baseline JPEG clusters but 0/2293
  PNG, 0/2047 random and 0/255 zero clusters.
- **Decision:** no H-31 or H-32 production source change is promoted. Xref
  offsets alone are not a unique oracle for another PDF's interior, and seam
  continuity with a margin is insufficiently reliable for JPEG. The archived
  scripts and evidence remain design/experiment records only.
- **Evidence:** `bench/results/h31_pdf_xref_prototype.txt`,
  `bench/results/h32_jpeg_join_experiment.txt`,
  `bench/results/x1_cluster_rule_current.txt` and the summary appended to
  `bench/results/part_t_prototypes.txt`.
- **Consequences:** no new join method, report field or fuzz target is added for
  H-31/H-32. Any later production proposal needs a new accepted experiment
  with n, Wilson bounds and zero false `verified` results for each gap kind.

## D-77. H-30 PNG chunk-CRC joins are a bounded exception to D-71

- **Context:** the archived prototype found a unique PNG chunk-CRC stitch in
  200/200 random-gap and 200/200 other-PNG-interior trials, while the existing
  filler/header-only B-06 recorder missed both classes. D-71 deliberately kept
  precision over recall and excluded random/high-entropy gaps.
- **Decision:** a single bounded exception is promoted: under
  `--fragment-recovery --validate decode`, a PNG head split inside a chunk may
  be joined when a CRC-32 over `type || head-prefix || tail-suffix` matches the
  stored CRC. The oracle only proposes candidates; deep decode validation is
  still mandatory. A unique `(cut, gap)` keeps `verified`; multiple solutions
  are reported once as `suspect` with `join_alternatives[]`.
- **Evidence:** `scripts/part_t/h30_png_crc.py`: 200/200 exact (100 random,
  100 other-PNG-interior), Wilson 95% [0.963, 1.0] for both kinds, 0 false
  `verified`, controls clean, mean 0.085–0.088 s per scan inside the 5 s
  fragment time limit. New coverage lives in `tests/fragment_recovery.rs` and
  `fuzz/fuzz_targets/fuzz_fragment_join.rs`.
- **Consequences:** no other format, entropy class or metadata source gains a
  join oracle. Allocated clusters are never read in unallocated/auto mode;
  schema 1 strips `join`/`join_alternatives` and CSV columns are unchanged.

## D-78. H-33 coverage work remains design-only

- **Context:** external harness §7.4 lists H-33 (exFAT/FAT12/16 and ZIP/OOXML,
  guide B-03/B-10) as NEW and **[NOT PROTOTYPED]**. No fixture image, ZIP/OOXML
  corpus, independent validator or decompression-cap test was run in Phase H5.
- **Decision:** no H-33 production source change is promoted. The external design
  sketch is retained as a future experiment only; B-03 and B-10 remain
  unimplemented and no recall number is claimed.
- **Evidence:** external harness §7.4; the H-33 record and Phase H5 report in
  `docs/PROGRESS.md`.
- **Consequences:** no parser, CLI flag, report field, fuzz target, fixture or
  `dist/` rebuild is added for H-33. A later proposal needs per-filesystem
  fixtures where a deleted JPEG is carved exactly, `regions[]` tiling, no
  FAT32/NTFS or golden D0 regression, plus independent OOXML content-type
  validation and ZIP-bomb caps.

## D-79. H-04 R1 scores validated repairs by default after §9 Q13 (2026-11-28)

- **Context:** the external harness measured eight of 30 truncated PNG cuts
  scoring 40 before repair (`ihdr` found, IDAT not yet complete), so the
  historical `--min-score 50` gate dropped them before `--repair` ran; with
  `--min-score 0` all eight crop-repair correctly. §9 Q13 asked whether to
  score the validated repair instead, which changes the repaired output set
  from 19/30 to 27/30 without changing `--min-score`.
- **Decision:** the user answered Q13 **yes** on 2026-11-28. `ScanOptions::score_repaired`
  now defaults to `true`: with `--repair`, the pre-repair score gate is deferred
  for a candidate that will be repaired and applied to the rebuilt bytes' score
  (`rebuilt_analysis.score`), or to the original score when no repair validates.
  The historical behaviour is available as `--no-repair-rescore`; the earlier
  `--repair-rescore` flag is accepted for compatibility and is a no-op. The
  report's `analysis.score` and all report/CSV fields stay unchanged; no
  threshold or metric definition was altered. **Q13 is closed.**
- **Evidence:** `tests/regress_repair_images.rs`:
  `low_pre_repair_score_is_recovered_by_default` (red before the default flip,
  green after; `png_04` cut at 30 %, default `--min-score`),
  `low_pre_repair_score_is_recovered_with_opt_in_rescoring` (compatibility flag
  still accepted), and `no_repair_rescore_keeps_the_legacy_drop` (red before
  the new flag, green after; `BELOW_MIN_SCORE` plus
  `stats.below_min_score >= 1`). The pre-Q13 default-drop test was adapted to
  this opt-out; its assertions were not loosened.
- **Consequences:** default `--repair` runs now recover small truncated PNGs that
  are independently Pillow-verified, without lowering `--min-score`. Users who
  need byte-for-byte historical selection can pass `--no-repair-rescore`.
  Golden D0 holds only intact files and is unaffected; q1 did not use
  `--repair`, so no archived benchmark cell is re-run or edited.

## D-80. A rebuilt PDF must still have a readable page (H-04 R2, 2026-11-28)

- **Context:** a 30 % cut of `pdf_01_image_1page.pdf` keeps the catalog and
  `/Pages` node but removes the only `/Type /Page` object. Rebuilding the
  trailer produced a file that passed structural checks but had zero readable
  pages (`pypdf`: 0 pages), and CarveX labelled it `repaired`/`decoded` — the
  over-claim of H-04 R2. The control `pdf_02_image_3pages.pdf` cut at 30 %
  retains a page and is a genuine repair.
- **Decision:** the repair revalidation path now walks the catalog's page tree
  with hard limits (64 nodes, 256 kids per node, depth 8) and requires at least
  one resolvable `/Type /Page` when `/Count > 0`. If none resolves, the repair
  is rejected: the carved prefix is written byte-identically with
  `status=truncated`, `validation=partial`, `validity=partial`, `repaired=false`
  and the existing note `repair attempted, not validated`. A tree declaring
  `/Count 0` claims no page and is exempt; this preserves the synthetic
  `/Count 0` repair fixtures without rewriting any test. Ordinary decode
  validation remains purely structural, so no non-repair `Decoded` result
  changes.
- **Evidence:** `tests/repair_validation.rs`:
  `pdf_repair_with_no_readable_page_stays_partial` (red with the old dispatch,
  green after) and `pdf_repair_with_a_readable_page_stays_repaired` (control).
- **Consequences:** no new report field, threshold or CLI flag; the stricter
  check is confined to `--repair --validate decode` PDF revalidation. Golden D0
  and G8 are unchanged because no intact file or benchmark profile is affected.

## D-81. FAT reuse is reported, not silently cut (H-28, 2026-11-28)

- **Context:** the deleted-entry recovery assumed a contiguous run but checked
  only the start cluster's FAT entry, treating end-of-chain as free. A deleted
  file whose middle or last cluster had been reused was emitted as `complete`
  with `overwritten_clusters = 0`, even though live bytes were inside the
  output.
- **Decision:** check every cluster of the assumed run. Any non-zero FAT value,
  end-of-chain included, counts as reused; `overwritten_clusters` is the
  number of such clusters and each contiguous reused run adds one
  `fat_cluster_reallocated` note (hard-capped at 64 notes). The existing
  metadata downgrade turns the row into `partial` with
  `validity_reason = overwritten_clusters`. The output is deliberately not
  truncated at the first foreign cluster: the directory entry is evidence of
  an extent, not an oracle for its bytes, and the additive provenance fields
  must describe the whole recovered candidate.
- **Evidence:** `tests/metadata.rs` covers index 1, middle and last for JPEG,
  PNG and PDF, an end-of-chain value at the start cluster, and five foreign
  clusters in three runs. All three new tests failed before the fix with
  `overwritten_clusters = 0` and pass after it.
- **Consequences:** no default change, no new CLI flag, no report schema
  change, and no golden D0 change; metadata rows are never `verified` (D-75).

## D-82. NTFS fixture tests are explicit SKIPs by default (H-29, 2026-11-28)

- **Context:** the five end-to-end NTFS metadata tests require `mkntfs`. On a
  machine without `ntfs-3g` they used to panic, turning `cargo test` red for an
  environmental reason; an earlier version printed `SKIP` and returned, which
  was a silent pass. The harness (H-26 and H-29) forbids both.
- **Decision:** `build.rs` declares `cfg(carvex_has_mkntfs)` and enables it only
  when `CARVEX_REQUIRE_MKNTFS=1` on Linux. The five dependent tests carry an
  `#[ignore]` reason naming the env var, so a normal `cargo test` prints a
  visible SKIP; CI installs `ntfs-3g`, sets the variable and therefore fails if
  the tool is missing. Three new synthetic `$MFT` tests (resident bytes,
  fragmented runlist with a negative delta, fixup mismatch) run on every
  runner and exercise the same recovery path without external tools.
- **Evidence:** `bench/results/h29_ntfs_mkntfs.txt` records the default run
  (`23 passed; 5 ignored` for `tests/metadata.rs`) and the opt-in run with
  `CARVEX_REQUIRE_MKNTFS=1` (`28 passed; 0 ignored`), including all five
  `mkntfs` fixtures passing locally.
- **Consequences:** no production recovery path, CLI flag, report field or
  default changes; golden D0 and G8 are unaffected. The repository's n = 10
  recall claim remains [REPORTED]; these tests do not enlarge that corpus.
