# Implementation progress (CarveX 0.3.1; frozen benchmark figures remain 0.1.0/0.2.0)

> Last updated: 2026-11-23. Every status below is verified by the command
> recorded with it; items that could not be run due to missing tools are marked
> `SKIPPED`, with no guessed results. IDs follow the consolidated guide §6
> (A-01…A-15, B-01…B-11, C-01…C-09, D-01…D-03); old findings F-01…F-21 are
> cross-referenced in `docs/REVIEW_RESPONSE.md`.

## Environment

| Component | Value |
|---|---|
| Date | 2026-11-23 (R2 docs update; earlier gate rows below are dated historical records) |
| OS | Linux x86_64 (Kali), running in a container |
| carvex | source `main` at `bae1033` (0.3.1 release prep/dist rebuilt, docs/evidence updated); `dist/` binaries rebuilt from `v0.3.1` commit `d5be769` (Linux glibc, Windows GNU) and `DIST OK`; CI run `45312234437` green (27/27) |
| rustc / cargo | 1.100.0 (0ef1bc386 2026-11-09) / 1.100.0 |
| MSRV | 1.85.0, installed via rustup; `cargo +1.85 check --locked` passes |
| Python | 3.13.12; Pillow available |
| Audit tools | `cargo-audit-audit 0.23.0`, `cargo-deny 0.21.0` |
| Benchmark tools | foremost, scalpel, photorec, mkfs.vfat, mkfs.exfat, mkntfs, mcopy, ntfs-3g, blkls, fls, istat, tsk_recover, parted, losetup, mount, mtools, 7z |
| Validation tools | `actionlint 1.7.12`, `cargo-fuzz 0.13.2` with `nightly 1.102.0-nightly`, `cargo-llvm-cov 0.9.1` |
| Missing | `qpdf`, `pdftoppm`, `ewfacquire`, `ewfexport`, `qemu-img`, a real Windows runner (Wine 10.0 available for packaged `.exe`) |

### Gate at time of writing — **GATE GREEN (2026-11-20, historical)**

```text
cargo fmt --all -- --check                          OK
cargo clippy --all-targets --all-features -- -D warnings   OK
cargo test --all-features --locked                  80 test pass
    (34 unit + 13 cli_contract + 5 evidence_formats + 8 fs_detect
     + 5 properties + 6 scan_integration + 2 fragment_recovery
     + 3 memory_budget + 4 png_decode_budget)
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps       OK
actionlint .github/workflows/*.yml                   OK (actionlint 1.7.12)
cargo deny check                                     OK (advisories/bans/licenses/sources ok)
cargo audit                                          OK (91 crates, 1555 advisories, 0 vulnerabilities)
cargo +1.85 check --locked                           OK
cargo build --release --locked                       OK (carvex 0.2.0)
scripts/review_repro.py --assert --xfail ...         10/10 PASS, 0 failure, 0 XPASS
bench/run_benchmark.py --smoke                       TP=3/FP=0 (unallocated), TP=3/FP=1 (raw)
```

All of this was run by `scripts/gate.sh` in a single command; final result:
**GATE GREEN**. The gate was re-run after the final A-10 refactor
(`ScanInput<'a>` in the pipeline), after A-07 step 2 (changing the default to
all volumes + gaps), after installing `cargo-audit`/`cargo-deny` and making the
documentation English-only, after the B-04 evidence sources, after the B-06
fragment-recovery stage, and again after R-01/R-02. It remained green with 80
tests, 10/10 repro and both supply-chain checks passing; the benchmark smoke
metrics stayed identical to the frozen 0.2.0 behavior.

`cargo deny check` also emitted four informational
`license-not-encountered` warnings for allow-list entries that no current
dependency uses (`Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`,
`BSD-3-Clause`, `ISC`); there were no advisories, bans, license denials or
untrusted sources. `deny.toml` sets `unmaintained = "none"` for cargo-deny
0.21, which no longer accepts the old `"warn"` value; unmaintained advisories
are informational and are not allowed to fail the gate.

Additional validation completed after that gate run:

```text
actionlint 1.7.12 .github/workflows/ci.yml            OK
scripts/coverage.sh 60                                OK (4456/6391 lines = 69.72%)
scripts/fuzz.sh 60                                   7 targets x 60 s, 18,520,025 runs, no crashes/artifacts
```

Current gate after H-29 (2026-11-22): **GATE GREEN**, 134 tests
(43 unit + 13 cli_contract + 7 evidence_formats + 5 fragment_recovery +
8 fs_detect + 2 jpeg_frame_budget + 4 memory_budget + 3 panic_containment +
2 png_crc_validity + 4 png_decode_budget + 2 png_validator_budget +
8 pdf_structure_validation + 5 properties + 4 regress_e01_safety +
4 regress_vmdk_safety + 3 repair_validation + 6 scan_integration +
1 validity_model + 4 work_budgets + 3 volume_parse_errors + 3 region_coverage),
34 repro scenarios with 0 failures/2 SKIPs/0 XPASS, G7 and benchmark smoke green.
`scripts/perf_gate.py --carvex target/release/carvex` reported **PERF GREEN**:
s9_4m median 0.145 s (limit 3.0 s), rand64m median 0.035 s. The H-30 full-gate
rerun is recorded in the H-30 task below.

### Pre-execution protocol correction (protocol-v1.1)

`protocol-v1` was committed/tagged/pushed before any lockbox artifact was
created (`cbee72d26634be041e48f9256198c1ff344df82f`). The first lockbox
generation attempt then showed that 19 of the fixed seeds 1001–1020 failed the
existing D1 builder's own 4096-byte NTFS non-residency guard for
`pdf_08_small_text.pdf`; seed 1011 was the only pass. No lockbox result was
viewed before fixing this. With user approval, that one builder padding value
was changed from `padding=6` to `padding=10` (minimum value that passes all 20
seeds), committed as a new annotated tag `protocol-v1.1`, and pushed before the
lockbox dataset was regenerated. Carving, scoring, runner commands, configs,
published figures and `docs/BENCHMARK_PROTOCOL.md` were **not** changed.

Execution evidence for the later items is collected in `docs/VALIDATION.md`.
GitHub Actions subsequently ran successfully on the CI-fix commit
(`42376a9`, run `44548142275`: all jobs success; push-event fuzz matrix
skipped). The historical 0.2.0 Windows binary was not executed; the 0.3.0
release binary was executed under Wine (see the release section below), not on
a Windows runner.

### Dist 0.3.0 release rebuild (2026-11-22)

Both release binaries were built from a detached worktree at the local
annotated tag `v0.3.0`, commit `5480f1084f85cc91f68d94c76c703d1348a65b5e`,
with rustc/cargo 1.100.0, the `x86_64-unknown-linux-musl` and
`x86_64-pc-windows-gnu` rustup targets, and `/usr/bin/x86_64-w64-mingw32-gcc`.
The musl binary was stripped after copying; the Windows binary was stripped by
the toolchain.

| File | SHA-256 | Execution |
|---|---|---|
| `dist/carvex-linux-x86_64` | `953dab643ce6628157b416b3588b1da59b84d760df627f82b14529a2cc3f975e` | `--version`, `info`, small `scan`; report version 0.3.0, commit `5480f1084f85` |
| `dist/carvex-windows-x86_64.exe` | `cbcc5343b63881c6c7decc6b218b99742d0d383c3b6415920ad4dd464758f1fa` | same commands under Wine 10.0 on Linux; report version 0.3.0, commit `5480f1084f85` |

`sha256sum -c dist/SHA256SUMS` is OK. Wine execution is **not** a Windows-runner
execution; no Windows runner was available locally, and the CI packaged-Windows
job failed with unavailable logs. `scripts/perf_gate.py` returned absolute
**PERF GREEN** for the musl binary (`s9_4m` median 2.842 s, reported 7.251x
relative baseline regression) and for the Windows binary through a Wine wrapper
(`s9_4m` 0.699 s, `rand64m` 4.752 s, both relative regression notices). The
full repro suite on the musl binary had two failures: X23 (default 2.80 s vs
1 thread 1.26 s) and R5 (5.50 s against the <= 1.0 s target). These are the
previously recorded musl/release-binary H-15/R5 behaviour; the glibc source
gate remains green and no allocator or timing tuning was applied.

### Dist 0.2.0 (rebuilt after A-07 step 2, 2026-11-19)

| File | SHA-256 |
|---|---|
| `dist/carvex-linux-x86_64` (static-pie musl, stripped) | `b9e6c5f4d9cd5dd6ed633acda799e1e1c0880717de5030a5fe61a8ce5f7858af` |
| `dist/carvex-windows-x86_64.exe` (PE32+, stripped) | `503553ea17cd15522ce694ecb5eee0a8daacc8d6fed9ff434fe48262ad17fc9b` |

`sha256sum -c dist/SHA256SUMS` OK; a dist smoke test on S1 with the new default
carves 60 files (`part1/`=30, `part2/`=30) and exits 0; with `--partition largest
--no-gaps` it carves 30 files, with `VOLUME_NOT_SCANNED`/`GAP_NOT_SCANNED`,
exit 3. The Windows `.exe` was newly built + format-checked, **not run on
Windows**.

## Tasks

| ID | Status | Gate | Notes / evidence |
|----|------------|------|----------------------|
| A-01 | DONE | green | R1, R2, R3 PASS. Heuristic: `%PDF-` is a boundary only when it follows `%%EOF`; early EOF reads more when the tail is only whitespace or looks like a new revision. Unit tests: early EOF, incremental, adjacent PDFs, embedded PDF, fake EOF in a stream. |
| A-02 | DONE | green | R3 PASS; nested suppression before writing; `keep_nested` keeps embedded images. |
| A-03 | PARTIAL | green | `#![forbid(unsafe_code)]`; deny `indexing_slicing`/`unwrap_used`/`expect_used` for `fs` + `signatures` (test modules have allow with justification); `catch_unwind` around `fs::detect`; release unwind + `overflow-checks=true`; R7 PASS. 7 fuzz targets run 60 s each on nightly (18,520,025 executions, no crash/timeout/OOM, no artifacts). Residual: fuzzing is not exhaustive and the H-30 GitHub fuzz jobs failed remotely. |
| A-04 | PARTIAL | green | `--validate structure` (default) `\| decode`; `validation` + `validated_bytes`; R4c PASS (stitched JPEG → `partial`). On `all-deleted_fat32.img`: structure 0.11–0.20 s; decode 0.49–1.10 s; 30/30 files `decoded`. R-02 bounds PNG validation at IHDR (`--max-decode-bytes`, 256 MiB default) and streams IDAT/filter checks without full IDAT, decoded-image or row-offset buffers; four `png_decode_budget` tests. JPEG/PDF decoder internals remain best-effort. |
| A-05 | DONE | green | R6 PASS: zero-tail trim + `--max-truncated-size 16M`; JPEG/PNG prefix salvage (R4a/R4b) with `status=truncated` + `validated_bytes`. |
| A-06 | PARTIAL | green | R5 0.48–0.79 s for 25,811 fake headers/64 MiB (target ≤1 s). Parallel carving via rayon, deterministic results (1 vs 4 thread test). R-01 fixed: `--max-memory` is a hard budget enforced before buffer allocation/growth and before retaining carved or stitched results; `--max-candidates`/`--time-budget` bound dense or long runs. Best-effort decode-temp reservation is applied, but third-party decoder internals are not fully accounted. |
| A-07 | DONE (steps 1 + 2) | green | `detect_all()` → `VolumeSet{volumes,partitions,unsupported,errors,gaps}`; report schema 2 adds `volumes[]`, `regions[]`, `warnings[]` object; `*_NOT_SCANNED` ⇒ exit 3. **Step 2 approved (D-23):** default `--partition all` + gaps, S1 carves 60 files (30+30, `part1/`/`part2/`) exit 0; `--partition largest --no-gaps` restores 30 files + WARNING + exit 3. `--include-gaps` is still accepted. **Baselines:** red `docs/baseline/A-07_v0.1.0.txt`; step 1 green `docs/baseline/A-07_green_v0.2.0.txt`; step 2 green `docs/baseline/A-07_green_v0.2.0_step2.txt`. Fixture `target/a07/s1.img`. |
| A-08 | PARTIAL | green | `trait ImageSource {len, read_at}` + unchanged `RawFile` default; `seek(End)` fallback when metadata len = 0. B-04 adds split raw, E01, VHD, VMDK and stdin sources under `src/image/`. `--on-read-error` and report `unreadable_regions[]` are present (H-22); raw/split reads are retried per 512-byte sector, and abort leaves a partial report. No `BlockDevice`/`--sector-size` yet and real device/Windows not tested. |
| A-09 | PARTIAL | green | Fixed the NTFS sector off-by-one (`field+1`, D-21, unit test `volume_sector_count_includes_the_backup_boot_sector`); FS errors → `FS_DETECTION_FAILED`. No `$ATTRIBUTE_LIST`/multi-extent `$MFT`/`$MFTMirr`/backup boot/FAT #2 yet. |
| A-10 | PARTIAL | green | Lib crate + thin `main.rs`; pipeline `plan_extents → build_tasks → scan_task → carve_batch → postfilter → write_outputs`; format registry; `ScanInput<'a>` groups the input parameters for `run_scan`. H-34 split the remaining long functions into behavior-preserving helpers, enabled public `missing_docs` and pedantic stage 1, and documented every remaining `too_many_arguments` allow; the change is recorded in commit f38a2ed. No non-test function exceeds 120 lines. |
| A-11 | PARTIAL | green | **134 tests** in the current gate (43 unit + 13 cli_contract + 7 evidence_formats + 5 fragment_recovery + 8 fs_detect + 2 jpeg_frame_budget + 4 memory_budget + 3 panic_containment + 2 png_crc_validity + 4 png_decode_budget + 2 png_validator_budget + 8 pdf_structure_validation + 5 properties + 4 regress_e01_safety + 4 regress_vmdk_safety + 3 repair_validation + 6 scan_integration + 1 validity_model + 4 work_budgets + 3 volume_parse_errors + 3 region_coverage); `scripts/review_repro.py --assert/--xfail --carvex` 34 scenarios, 0 failures, 2 SKIPs, 0 XPASS; red baseline `docs/baseline/repro_v0.1.0.txt`. Post-R-01/R-02 `cargo-llvm-cov 0.9.1` measured 4456/6391 lines = **69.72%**; CI/local floor set to 60%, not the earlier aspirational 80%. No new coverage run after H-14/H-16/H-17/H-18/H-19/H-21/H-22/H-25/H-26/H-27/H-29. |
| A-12 | PARTIAL | green | MSRV 1.85 (`rust-version`, README corrected from "Rust 1.74"); `cargo +1.85 check` in the gate; `.github/workflows/ci.yml` adds actionlint + coverage and uses a pinned direct actionlint install; `rustfmt.toml`/`clippy.toml`/`deny.toml`; 0.3.0 release rebuild + regenerated `SHA256SUMS`. Local `cargo deny check`, `cargo audit` and `actionlint 1.7.12` pass. Earlier GitHub run `44548142275` on `42376a9` succeeded but predates B-04/B-06/R-01/R-02; `45095456201` failed, then CI fixes landed and run `45121233267` at `86ae15f` completed success (27/27 jobs, archived in `bench/results/ci_run_45121233267*.json`; job logs 403 via public API). Current-HEAD CI run `45312234437` (`bae1033`, 2026-11-23) completed **success** (27/27 jobs), archived in `bench/results/ci_run_45312234437*.json`; job logs 403 via public API. The 0.3.0 packaged Windows binary was executed under Wine 10.0, not on a Windows runner. SBOM generation is wired in; no signatures; removing binaries from `dist/` is STOP-AND-ASK (kept). |
| A-13 | PARTIAL | green | Exit 0/1/2/3/4 documented + CLI contract tests; coded warnings; `RAW_SCANS_ALLOCATED` informational (D-20); H-28 writes a partial report with `INTERRUPTED` and exit 4. No `thiserror` types, leveled logger or `ProgressSink`/ETA yet. |
| A-14 | PARTIAL | green | Report schema v2: `schema_version`, `argv`, `options`, `tool_info`, `input` (before–after SHA-256), ISO-8601, `case_id`, `examiner`, `warnings[]` object, `volumes[]`, `regions[]`, `lba`, real `cluster`, `partition_index`, `extent_id`, `allocated`, `source`, `fragments`. `docs/report.schema.json` + key-matching test; atomic write; `--manifest` generates `SHA256SUMS`; `--report-schema 1`. `unreadable_regions[]` is emitted with A-08/H-22 read-error handling; schema 1 still strips additive fields. |
| A-15 | PARTIAL | green | Hard budget checks for `--max-memory` before allocation/retention plus caps on `--max-size`/`--max-truncated-size`; atomic write; `--force`. No rejection of output inside the source image yet, no `--overwrite`/free-space check/`--max-output`/symlink ban. |
| B-01 | OPEN | - | Does not read `0xE5` dir entries/MFT not-in-use, no `--mode metadata\|hybrid`. |
| B-02 | PARTIAL | green | MBR + GPT primary/backup; `--partition largest\|all\|N`; `info --json` lists `volumes[]`. No EBR chain yet; still assumes 512-byte sectors for the partition table. |
| B-03 | OPEN | - | No exFAT/FAT12/FAT16 support. |
| B-04 | PARTIAL | green | `src/image/` supplies read-only split raw, classic E01 (compressed/uncompressed), fixed/dynamic VHD, flat/sparse VMDK and stdin sources; `open_image()`/`--input-format` select them. Five `evidence_formats` fixture tests verify byte-for-byte reads and stdin report identity (D-29). The raw default, frozen benchmark, figures, lockbox results and dist binaries are unchanged. Recognized EWF2/Ex01, L01/Lx01, LVF, differencing VHD, ESX/COWD and compressed streamOptimized VMDK variants are rejected explicitly; encrypted E01 has no decryption and is not guaranteed to be detected before parsing. |
| B-05 | PARTIAL | green | `--align none\|auto\|cluster\|sector`; default `none` keeps the old behavior; test `alignment_restricts_candidate_starts_without_changing_results_on_aligned_data`; golden TP unchanged. No per-file `aligned` flag recorded, no measurement of candidate reduction (D5). |
| B-06 | PARTIAL | green | Opt-in `--fragment-recovery` adds bounded two-fragment gap carving at cluster boundaries. Work units are authoritative, ordinary images are not deep-probed, joins are ranked by filler/header evidence, and only `verified` stitches are emitted with `source="bgc"`, `fragments[]`, `checks`, `confidence` and `alternatives`. D1 profile: recall 4/6 = 66.7% [30.0%, 90.3%], splice 0/4, junk FP 0/24; X15 recalls 3/3 real files with no bait stitch. Default raw/benchmark path untouched | n-fragment and metadata/runlist-assisted reassembly remain unimplemented; the small D1 sample has wide CIs, is development evidence only, and published figures exclude B-06. |
| B-07 | PARTIAL | green | JPEG/PNG prefix salvage; `status=truncated` + `validated_bytes`; H-21 now rebuilds PDF xref/trailer, appends real JPEG/PNG footers, and labels `repaired` only after revalidation. No `--salvage` flag; generic marker/chunk truncation is a disclosed `[SHOULD]` skip (D-47). |
| B-08 | PARTIAL | green | `--manifest`, `--hash-image`, `--case-id`/`--examiner`, full exit codes. No carve-phase progress/`--resume`/`--dry-run`/`verify`/`check`/`extract`/`--json-lines`/completions/man. |
| B-09 | OPEN | - | Per-file `notes[]` present; no fixture + test per variant (APNG, xref stream, `/Encrypt`…); no format support table. |
| B-10 | OPEN | - | No custom signatures/`--signatures`; the registry (A-10) is the ready foundation. |
| B-11 | OPEN | - | No `posix_fadvise`/`--io-threads`. |
| C-01 | PARTIAL (scoped) | n/a | `docs/BENCHMARK_PROTOCOL.md` frozen at tag `protocol-v1` (SHA-256 `d38607b86850312ec9906acd0dd732f965ca10cd5e78ff567a7076360b8008b6`); pre-execution builder correction tagged `protocol-v1.1`; 20-seed lockbox generated and run once into `bench/results/lockbox-v1` with dataset/result `SHA256SUMS` verified. Execution evidence: `docs/VALIDATION.md §6`. Scope: D1-style profile only, D2–D7 remain OPEN. |
| C-02 | OPEN | - | `bench/lockbox.py` supports `--seeds/--profile` for the 20-seed D1 lockbox; the dev corpus remains synthetic D1 with 30 files. No D2–D7 profiles. |
| C-03 | PARTIAL | - | Ground truth from a Pillow manifest, independent of CarveX; runner checks SHA-256 before each run; metrics do not use CarveX's validator (ADR-007). `qpdf`/`pdftoppm` missing → PDF "openable" check SKIPPED. |
| C-04 | PARTIAL | - | Exact + usable file-level, one-to-one metric, FP classification kept/unknown. **F-17 verified by re-running:** Foremost 40/44 JPEG-in-PDF + 4 truncated PDFs; Scalpel 40/54 + 4 + 10 child spans (8 EXIF thumbnails + 2 `FFD8FFD9` fragments) — matches "40 of 44 / 40 of 54". No block-level `byte_recall`/`splice_rate`; classification not automated in the runner. |
| C-05 | PARTIAL | - | Wilson 95% CI in the REPORT abstract (100/100 → [96.3%; 100%], 30/30 → [88.6%; 100%]) with n/conditions; runner warns `metric_variance`. No McNemar/bootstrap; no CI in every table. |
| C-06 | PARTIAL | - | Competitor configurations stored in `bench/`; search-space confound documented (README, REPORT §6.4 item 9). No same-search-space baseline; no ablation. |
| C-07 | PARTIAL | - | Runner records host/versions/argv/seed, relative to repo root; `--smoke`. No `render_docs.py` yet (figures are still hand-written, with `results.json` as source); no `bench/config.toml`/`--full`. |
| C-08 | PARTIAL | - | CFReDS RM#2 was run (0.1.0) and its real results recorded in `docs/CFREDS.md §5` + binary hashes; DFRWS 2006/2007 not run; 0.2.0 not re-run on CFReDS (documented). |
| C-09 | PARTIAL | - | Median time/throughput table present; full 4-image/3-rep 0.2.0 re-run completed in `bench/results/full-0.2.0` with all checksums verified. No 1–16 GiB scale images, no cold cache + IQR ≥10 repetitions. |
| D-01 | PARTIAL | n/a | Removed absolute paths like `/home/…` (REPORT/CFREDS/README → `$CARVEX`/`$HOME`); fixed "Pillow/ReportLab"; "100%" in the abstract tied to n/CI/conditions; 16 tables numbered `Table 1…16`; SHA-256 of tools (foremost/scalpel/photorec) + explicit note that benchmark binary hashes were not recorded; §6.2 explains the evidenced FP 40/44 and 40/54; README "Rust 1.74" → 1.85; "fairness" conclusion limited; F-16 confound in §6.4; Windows `.exe` noted as untested. |
| D-02 | DONE | n/a | REPORT §1.4 Related Work (5 branches + comparison table); references [11]–[16] had their DOI/publisher pages opened before insertion. |
| D-03 | PARTIAL | n/a | `CHANGELOG.md` (0.3.0), `SECURITY.md`, `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md` (D-01…D-59), `docs/DEPENDENCIES.md`, `docs/REVIEW_RESPONSE.md` (38 IDs), `docs/REPORT.md`, `docs/VALIDATION.md` (execution evidence only). H-34 quality work is implemented in commit f38a2ed; the evidence manifest and documentation drift are closed. |

## Earlier summary rows (not guide IDs)

| ID | Status | Gate | Notes / evidence |
|----|--------|------|------------------|
| D-24 | DONE | n/a | All docs, comments and reports are English-only; the technical report is now `docs/REPORT.md`; every reference and table caption updated; no CLI/JSON/CSV/benchmark values changed. |
| P5-1 | DONE (scoped) | green | Post-B-04 gate GREEN 2026-11-19 (70 tests); post-B-06 gate GREEN (73 tests, coverage 4111/6001 = 68.51%); post-R-01/R-02 gate GREEN 2026-11-20 (80 tests, coverage 4456/6391 = 69.72%); CHANGELOG 0.2.0 + Unreleased B-04/B-06/R-01/R-02 entries; dist rebuilt for 0.2.0; fuzz runtime complete (18,520,025 executions); post-B-04 coverage 67.50% with 60% floor; GitHub CI green on pre-B-04/B-06 CI-fix commit `42376a9`; lockbox run once; full 4-image/3-rep benchmark re-run. Evidence: `docs/VALIDATION.md`. Scope items remain open as listed below. |
| P5-2 | DONE | n/a | `docs/REVIEW_RESPONSE.md` covers 38 IDs (A-01…D-03) with FIXED/PARTIAL/OPEN and cross-references F-01…F-21. |

## Published regression results (unchanged)

- Carving 0.2.0 on 4 benchmark images: **100/100 files matching SHA-256** with
  the published ground truth (`bench/results/final/reports/*/carvex-unallocated-rep*.json`),
  re-verified after the NTFS fix (D-21) and after the final dist rebuild
  (30+30+20+20 = 100/100). For A-07 step 2, the runner and the regression check
  command explicitly pass `--partition largest --no-gaps` to keep the 0.1.0
  search space (exit 3 due to `GAP_NOT_SCANNED` by design).
- Published benchmark figures remain those of 0.1.0 (D-16): unallocated
  `100/0/0`, raw `100/20/0`; Foremost usable `94/62/6`; Scalpel usable
  `92/74/8`; PhotoRec `94/20/6`.
- Competitor FP classification re-verified by running the commands (C-04):
  Foremost 44, Scalpel 54 `unknown_fp` — see `docs/REPORT.md §6.2`.

### Harness findings (H-xx)

| ID | Pri | Repro (date, build) | Status | Commit | Test(s) | Before → After / evidence |
|----|----|---------------------|--------|--------|---------|---------------------------|
| H-01 | HIGH | 2026-11-23 user file | CLOSED | 092be33 | docs/guide.md | The 924-line guide was supplied by the user and committed verbatim; `sha256sum docs/guide.md` = `22ee8122209e9eda11d6f32a1e650e8f8e48f5610bd808309d94f436e5e78265`. |
| H-02 | HIGH | 2026-11-21 source | CLOSED | 7129173 | scripts/gate.sh; scripts/check-msrv.sh | The clean-checkout gate was made SKIP-safe; G7 golden and MSRV checks were added. Local gates now run G0-G9 without root; the H-30 CI run is a separate, failed remote observation. |
| H-03 | HIGH | 2026-11-21 source | CLOSED | bbef4bd | scripts/review_repro.py; tests/regress_e01_safety.rs | X01-X24 controls plus Rust regression and property tests were completed; red baseline `docs/baseline/repro_v0.1.0.txt` is retained. |
| H-04 | MED | 2026-11-21 source / 0.3.0 release | PARTIAL | 4d0a6fd; 5480f10 | dist/README.md; dist/SHA256SUMS; scripts/sbom.py | Repository URL, build recipe, version metadata and stale-dist notice were fixed; both binaries were rebuilt at local tag `v0.3.0`, executed, and checksummed. The Windows executable was run under Wine 10.0, not on a Windows runner, so release identity is PARTIAL rather than fully closed. |
| H-05 | CRIT | 2026-11-21 source / 2026-11-21 release build | CLOSED | 873ea35 | tests/png_validator_budget.rs; scripts/review_repro.py X02 | R-02 already bounded inflation; the missing shared flag was reproduced red. X02a: 0.009 s / 8.8 MiB RSS, `partial`; X02b rejected at IHDR. |
| H-06 | CRIT | 2026-11-21 source / 2026-11-21 release build | CLOSED | 3534717 | tests/jpeg_frame_budget.rs; scripts/review_repro.py X03 | Red test proved the decoder ran without a budget check. After: 20,000² and 65,535² SOF claims → `unverified`, 0.010 s / 8.9 MiB and 0.009 s / 9.0 MiB RSS. |
| H-07 | HIGH | 2026-11-21 source | CLOSED | fa18436 | scripts/review_repro.py X04 | 23-26s → <1s after single-pass EOF indexing; A-01/R1-R8 and G7 unchanged. |
| H-08 | CRIT | 2026-11-21 source / 2026-11-21 release build | CLOSED | e69e68b | tests/memory_budget.rs; scripts/review_repro.py X01 | Red test: 300 x ~60 KiB JPEGs (18 MiB output) with `--max-memory 16M` failed before the fix. After: all 300 written with bounded RSS; `SOURCE_CHANGED` verified by a changing-source test; G8 s9_4m 0.476s before vs 0.164s after. |
| H-09 | HIGH | 2026-11-21 source / 2026-11-21 release build | CLOSED | 48deab5 | tests/work_budgets.rs; scripts/review_repro.py X05 | Red X05 (missing flags) before the fix; after: all four limits trip with coded warning, report and exit 3; time case sets `nondeterministic_stop`. D-39 defaults measured on D0/S9/S10. |
| H-10 | HIGH | 2026-11-21 source / 2026-11-21 release build | CLOSED | 5f794a4 | tests/panic_containment.rs; scripts/review_repro.py X06 | Red test proved an escaping rayon panic before the fix. After: candidate/join panics become `internal_errors[]`, `CANDIDATE_PANIC`, counted in stats, exit 3; other candidates still carved. |
| H-11 | CRIT | 2026-11-21 source / 2026-11-23 R2-11 | CLOSED | 4b86f2d, pending | tests/regress_e01_safety.rs; scripts/review_repro.py X07; clippy deny in src/image/{mod,e01,split,vhd,vmdk}.rs | Original E01 allocation bounded; R2-11 denies indexing/unwrap/expect in image parsers and replaces production hostile-field access with checked errors. |
| H-12 | CRIT | 2026-11-23 packaged 0.3.0 + source | CLOSED | pending | tests/regress_vmdk_safety.rs; tests/regress_task_plan.rs; scripts/repro_x08b.py; scripts/repro_x26.py; scripts/review_repro.py X08 | Grain tables stay bounded (X08/X08a) and declared-size scan plans are now lazy. X08b: 512 TiB ZERO → exit 3, 5.01 s, 28.0 MiB RSS with a report; X26 VHD/E01/100k-split → typed errors at ~10 MiB RSS. |
| H-13 | HIGH | 2026-11-21 source | CLOSED | 080b76a | tests/regress_vmdk_safety.rs; scripts/review_repro.py X09 | VMDK descriptors are confined to the containing image; relative and absolute escapes are rejected with typed errors. |
| H-14 | HIGH | 2026-11-22 source / nightly fuzz runtime | CLOSED | e2d3f1d | fuzz/fuzz_targets/fuzz_e01.rs; fuzz/fuzz_targets/fuzz_vhd.rs; fuzz/fuzz_targets/fuzz_vmdk.rs; fuzz/fuzz_targets/fuzz_split_names.rs; fuzz/fuzz_targets/fuzz_open_image.rs; fuzz/fuzz_targets/fuzz_fragment_join.rs; scripts/fuzz.sh | Static gap: only 7 targets, none for the new parsers. Added 6 named targets, 22 committed seeds and structured property tests; each target ran 60 s clean (235,272 executions total, no crash/artifact). R2-08 (2026-11-23): real ewfacquire/qemu-img seeds SKIPPED (`ewfacquire`/`ewfexport`/`qemu-img` missing); 13 fuzz targets x60s with real-container seeds not re-run in this session (cargo-fuzz present, time/opportunistic). |
| H-15 | MED | 2026-11-22 source / 0.2.0 and 0.3.0 `dist/`; 2026-11-23 R2-02 | CLOSED (R2-02 scratch reuse; shipped 0.3.1 glibc dist passes X23/R5) | 1c28e2e, 25b50d0 | scripts/review_repro.py X23,R5; src/scan.rs | R2-02 reuses per-worker scratch buffers in count/scan/carve (ADR-H15/D-66), preserving budget drop-before-reserve. 0.3.0 musl dist baseline: X23 2.80s vs 1.26s (2.22x), R5 5.50s. 0.3.1 musl build measured but not shipped (X23 2.452s vs 1t 1.048s, 2.34x; R5 5.020s). Shipped 0.3.1 Linux glibc (`carvex-linux-x86_64`, min GLIBC 2.34): X23 default 0.158s vs 1t 0.565s (0.28x), R5 0.310s. |
| H-16 | CRIT | 2026-11-22 source | CLOSED | 3f47e51 | tests/validity_model.rs; tests/cli_contract.rs; scripts/review_repro.py X10–X14/X24 | Red test proved `validity` was absent and default `complete/structure/100` rows had no "content not checked" text. Added authoritative `validity` + `checks[]`; scores and emission unchanged. |
| H-17 | HIGH | 2026-11-22 source | CLOSED | 905c9c0 | tests/png_crc_validity.rs; src/signatures/png.rs; scripts/review_repro.py X10 | Bad PNG CRC is now `validation=partial`/`validity=suspect` in structure and decode modes, with a `chunk_crc` check naming type/offset; decode `validated_bytes` stops at the last good chunk. |
| H-18 | HIGH | 2026-11-22 source; 2026-11-23 R2-03 | PARTIAL (R2-03 check added; eval upper bound above 0.1%; accepted for 0.3.1, option A 2026-11-23) | 01ea6b3, pending | src/signatures/jpeg.rs; src/signatures/mod.rs; scripts/review_repro.py X12 | R2-03 adds conservative baseline content-continuity check (D-67, supersedes D-45 disclosure). Clean control 1000: 0 suspects. Dev splices 0/499 verified. Eval splices 2/3997 verified, Wilson 95% upper 0.182% (>0.1% target). User accepted PARTIAL for 0.3.1 release; aggressive heuristic follow-up authorized with FP/upper-bound measurement. |
| H-19 | HIGH | 2026-11-22 source | CLOSED | cb591a3 | tests/pdf_structure_validation.rs; tests/fragment_recovery.rs; tests/scan_integration.rs; scripts/review_repro.py X11 | Stage 1 removed the false Decoded label; stage 2 adds bounded classic/xref-stream/ObjStm/trailer/catalog/length checking. Clean PDFs are `decoded`/`verified`; mismatches are `partial`/`suspect`. R2-08 (2026-11-23): six `qpdf --check` X11 controls SKIPPED (`qpdf` not installed). |
| H-20 | MED | 2026-11-21 source | CLOSED | cee916a | scripts/review_repro.py X13 | Trim trailing CR/LF/whitespace/NUL before %%EOF check; EOL variants now identical. |
| H-21 | HIGH | 2026-11-22 source | CLOSED | 3b0c15c | tests/repair_validation.rs; scripts/review_repro.py X14 | Red tests proved the static footer produced an invalid PDF labelled `repaired` with no `sha256_original`. Every repair is now revalidated before `repaired`; `sha256_original` is emitted whenever output bytes change. |
| H-22 | CRIT | 2026-11-22 in-memory fixture | CLOSED | d7d991e | tests/regress_e01_safety.rs; tests/image/mod.rs; scripts/review_repro.py X16 | Short final chunks accepted; damaged chunks and read errors follow `--on-read-error`; `unreadable_regions[]`, `UNREADABLE_REGION` and overlap downgrade are reported. `abort` exits 1 with a schema-2 partial report. R2-08 (2026-11-23): X16(a) real `ewfacquire` image SKIPPED (`ewfacquire`/`ewfexport` missing; libewf2 package present but no acquire tool). |
| H-23 | MED | 2026-11-21 source | CLOSED | 080b76a | tests/regress_vmdk_safety.rs; scripts/review_repro.py X08 | Missing grain table returned "index out of range"; now reads zeros like qemu-img. R2-08 (2026-11-23): X17 real qemu-made VMDK image SKIPPED (`qemu-img` missing). |
| H-24 | MED | 2026-11-21 source | CLOSED | 56ee94a | scripts/review_repro.py X18 | Auto-detect now validates VHD footer checksum and requires strong VMDK descriptor markers before classifying. |
| H-25 | LOW | 2026-11-22 source | CLOSED | e65f0f2 | tests/evidence_formats.rs; scripts/review_repro.py X22 | Red tests failed with `left: Null, right: "e01"/"vmdk"/"stdin"`. Schema 2 now adds `input.logical_size`, `physical_size`, both `_after` fields, `format` and `files[]`; stdin reports `unchanged: null`. |
| H-26 | MED | 2026-11-22 source | CLOSED | eb649f1 | tests/volume_parse_errors.rs; scripts/review_repro.py X19 | Red tests: multi-volume scan exited 0 with no warning, the single-volume message was generic, and `info` hid the cause. Parse errors now emit `VOLUME_PARSE_FAILED`, exit 3, and flag the fallback region. R2-08 (2026-11-23): X19 re-run with real `mkfs.vfat`/`mkntfs` via `scripts/review_repro.py`: PASS. |
| H-27 | MED | 2026-11-22 source/bench FAT32 | CLOSED | 5ebe851 | tests/region_coverage.rs; scripts/review_repro.py X20 | X20 reported overlapping regions and a 7.4 MiB scanned-length mismatch. Regions now come from the scanner extent plan, cover the image exactly, and preserve `Σ scanned == bytes_scanned`. R2-08 (2026-11-23): X20 re-run with real bench FAT32 image via `scripts/review_repro.py`: PASS; R7/R8 also PASS with real bench images. |
| H-28 | MED | 2026-11-22 source/release binary | CLOSED | c14060d | scripts/review_repro.py X21 | X21 killed the unmodified release binary with `exit -2 after SIGINT (expected 4)`. A cooperative handler now writes an `interrupted`/`INTERRUPTED` schema-2 report and exits 4. |
| H-29 | HIGH | 2026-11-22 source/release binary | CLOSED | b902eed | tests/fragment_recovery.rs; tests/cli_contract.rs; tests/validity_model.rs; scripts/review_repro.py X15; scripts/fragment_profile.py | Red evidence: schema/CSV missing `alternatives`, S12 `file A not recovered exactly`, X15 accepted one incorrect PDF splice. Work units are authoritative; only `verified` stitches are emitted. D1 profile: recall 4/6, splice 0/4, junk FP 0/24; X15 recalls 3/3 real files with no bait stitch. |
| H-30 | HIGH | 2026-11-22 source/CI config | CLOSED (CI green observed on archived run; current-HEAD run pending) | b9c40d3 | scripts/gate.sh; scripts/render_docs.py; scripts/perf_gate.py; scripts/sbom.py; .github/workflows/ci.yml | Red efficacy test: reintroducing H-05, H-07, H-08 and H-17 each turned the gate red. Gate runs G0-G9 at 70% coverage with 13 fuzz targets. Runs `45035152791` and `45095456201` failed; fixes N-35..N-40 landed and observed run `45121233267` at `86ae15f` completed success (27/27). Run JSON/jobs archived in `bench/results/ci_run_45121233267*.json`; job logs 403 via public API. Current-HEAD CI run `45312234437` (`bae1033`, 2026-11-23) completed **success** (27/27 jobs), archived in `bench/results/ci_run_45312234437*.json`. |
| H-31 | MED | 2026-11-22 source/docs | CLOSED | efd6d9f | scripts/check_status.py; scripts/gate.sh; .github/workflows/ci.yml | Static red: the checker failed with `STATUS FAIL: section heading not found` before the reconciliation; a deliberately wrong A-10 label fails after the fix. Status tables now agree across both documents. |
| H-32 | MED | 2026-11-22 docs | CLOSED | bd9ebd0 | bench/results/EVIDENCE.md; bench/results/EVIDENCE.sha256; bench/results/evidence-bundle-0.2.0.tar.gz; bench/lockbox.log; docs/VALIDATION.md | Static red showed ignored evidence paths, no manifest and no reconstructed lockbox log. The manifest covers 47 bundled artefacts; historical figures without retained raw logs are labelled author-reported; no lockbox regeneration. |
| H-33 | MED | 2026-11-22 docs | CLOSED | 240ef42 | README.md; docs/DECISIONS.md; docs/REPORT.md; docs/baseline/h33_static_red.txt; bench/render_docs.py | Static grep red found the speed and Pillow overstatements and a missing `fuzz/` tree entry. Speed sentence removed; Pillow wording corrected; `fuzz/` added; already-fixed items recorded as NOT-REPRODUCED. Frozen results and `/home/kali` paths were not edited. |
| H-34 | LOW | 2026-11-22 source | CLOSED | f38a2ed | clippy.toml; src/lib.rs; src/scan.rs; src/cli.rs; docs/baseline/h34_static_red.txt; docs/baseline/h34_static_green.txt | Static red found 15 non-test functions over 120 lines plus one 121-line test; combined missing_docs/pedantic/too_many_lines inventory had 698 warnings. All non-test functions are now ≤120 lines, public docs are complete, and pedantic stage 1 is clean. One test-only allow is justified in place. No behavior or output change. |

### Phase 0 support tasks

| ID | Status | Gate | Test(s) | Notes / evidence |
|----|--------|------|---------|------------------|
| P0.7 | DONE | - | scripts/perf_gate.py | Perf baseline recorded (s9_4m median ~0.39s, rand64m ~0.03s); thresholds.toml committed; x04 marked xfail in thresholds. |

### Per-task reports

Task: H-21 — `--repair` labelled unreadable PDFs `repaired`
Reviews: v5:F-15 (RT: `pdf_03_text_2pages.pdf` cut at 70% → `repaired/partial/60`, 4,354 B, pypdf "startxref not found"), guide S15/B-07, v1/v2/v3.
Repro: `tests/repair_validation.rs` was added first against the unmodified
  scanner. All three tests failed on the missing CSV `sha256_original` column
  (`/tmp/opencode/h21_red.log`); the invalid-catalog case also exposed the
  unconditional `repaired` label. X14 reproduced the same real-corpus failure.
Change: `src/signatures/repair.rs` builds a bounded classic xref/trailer from
  `N G obj` headers and `/Root` or `/Type /Catalog`; JPEG gets EOI and PNG
  gets the real IEND chunk. `validate_repair` runs the same deep validator as
  an intact file, with PDF structural checks even in default mode. The scanner
  writes the dynamic suffix only after revalidation, keeps the original score
  and `carved_size`, adds a `repair` check, and emits optional
  `sha256_original` in JSON and CSV. Failed repair keeps `truncated`/`partial`,
  unchanged bytes, and the exact note `repair attempted, not validated`.
Tests: three integration tests cover valid PDF repair, invalid-PDF failed
  repair, and JPEG/PNG footer repair, including hash/CSV/JSON equivalence.
  X14 now asserts the repaired PDF opens in pypdf 6.9.2 non-strict, passes the
  xref/root/repair checks, has a different output hash from the source bytes,
  and carries `sha256_original`. It was removed from `scripts/repro_xfail.txt`.
Gate: green (fmt/clippy/tests/doc/actionlint/deny/audit/MSRV/release/repro/golden/smoke),
  120 tests, 34 repro scenarios, 0 failures/2 SKIPs/0 XPASS, G7 green;
  G8 PERF GREEN (s9_4m median 0.155 s, rand64m median 0.034 s).
Residual risk / next: `--salvage` is not implemented because it is `[SHOULD]`
  and would add a new partial-file policy beyond the accepted X14 contract;
  D-47 records the explicit skip. PDF page content is still not rendered, and
  PDF repair is limited to classic xref reconstruction from surviving object
  headers.

Task: H-19 — PDF `decoded` is a lexical check
Reviews: v1:R-04, v2:R-04, v3:R-04, v4:F-04, v5:F-07.
Repro: stage 1 reproduced the lexical label with the qpdf oracle; stage 2 red
  tests first reported `unverified` for a valid minimal PDF, bad xref offset,
  bad `/Root` and xref-stream fixture. The first implementation attempt also
  failed on the compressed catalog, stream-length and `/Prev` cycle cases.
Change: `src/signatures/pdf_validate.rs` now bounds classic xref parsing, xref
  streams (`/Type /XRef`, `/W`, `/Index`, bounded inflate), `/Size`, direct
  `/Length` vs `endstream`, `/Root` → `/Type /Catalog` (including `/ObjStm`),
  `/Prev` depth/cycle handling, `/XRefStm` and `/Encrypt`. Mismatches are
  `partial`/`suspect`; unsupported or budget-limited checks are
  `unverified`/`structural`. `validated_bytes` remains zero because PDF checks
  are structural. S12/B-06 and scan-integration expectations were updated to
  the new honest `Decoded` result while preserving the E-1 `Unverified`
  compatibility path.
Tests: eight `tests/pdf_structure_validation.rs` integration tests, plus the
  updated B-06 and scan-integration tests. All ten corpus PDFs were verified
  with score 100 and none was suspect. `qpdf` and `pdftoppm` are not installed,
  so their control matrix is SKIPPED; `pypdf` 6.9.2 only opened generated
  fixtures and was not used as a verdict oracle.
Gate: green (fmt/clippy/tests/doc/actionlint/deny/audit/MSRV/release/repro/golden/smoke),
  117 tests, 34 repro scenarios, 0 failures/2 SKIPs/0 XPASS, G7 green;
  G8 PERF GREEN (s9_4m median 0.185 s, rand64m median 0.032 s).
Residual risk / next: PDF page content is not rendered or semantically
  verified; encrypted content is flagged but not decoded. H-21 added repair
  revalidation; the external qpdf/pdftoppm oracle remains SKIPPED.

Task: H-18 — JPEG strict-scan validity and splice rate
Reviews: v1:R-05, v2:R-05, v3:R-05, v4:F-05, v5:F-08
Repro: `scripts/jpeg_splice_property.py` on the unmodified build measured
  218/1,000 top-label splices (0.2180, Wilson 95% [0.1935, 0.2446]).
  The partial fix already made unsupported strict-scan variants return
  `Unverified`; the remaining accepted cases are baseline tail swaps with
  identical headers.
Change: no further parser change was made. The RSTn-sequence (when DRI is
  present), MCU-count-against-SOF and exact-EOI-consumption checks were already
  in `check_baseline_mcus`; two unit tests now pin the MCU/EOI behaviour.
  The property script records accepted case bytes and the final 4,000-pair run.
  D-45 records why no false-positive-prone splice detector was added.
Tests: `src/signatures/jpeg.rs::h18_strict_scan_rejects_extra_entropy_before_eoi`
  and `h18_strict_scan_rejects_truncated_entropy`; X12 for progressive labels.
Gate: green (fmt/clippy/tests/doc/actionlint/deny/audit/MSRV/release/repro/golden/smoke),
  109 tests, 34 repro scenarios, 0 failures/2 SKIPs/0 XPASS, G7 green.
Residual risk / next: 5/4,000 byte-aligned tail swaps still receive `verified`
  (0.0013, Wilson 95% [0.0005, 0.0029]); the upper CI remains above 0.1%.
  H-19 stage 2 and later validity items continue.

Task: H-17 — PNG bad CRC still `complete` / `decoded`
Reviews: v1:R-03, v2:R-03, v3:R-02, v4:F-03, v5:F-06
Repro: `tests/png_crc_validity.rs` corrupted the first IDAT CRC of the embedded
  1x1 PNG. Before the fix, structure mode reported `complete/structure/85`
  with `validity=structural` and no `chunk_crc` check; the red assertion failed
  with `left: String("structure")`, `right: "partial"`. Decode mode reported
  `partial` but `validated_bytes=0` and no check detail. The clean control
  passed before and after.
Change: the structural PNG parser now records a `chunk_crc` check for every
  mismatch and marks `Analysis::integrity_failed()`; the scanner maps that to
  legacy `validation=partial` while keeping `status` and the score. Decode
  validation separately recomputes every CRC and returns a checked prefix that
  stops at the first bad chunk; `validated_bytes` now reports that prefix.
  Added a detailed validator result API without changing the historical tuple
  API. The existing `reports_bad_crc_but_keeps_file` unit test was extended,
  not weakened. README limitation 4 now describes the suspect label.
Tests: `tests/png_crc_validity.rs` covers both modes and the clean control;
  X10 now checks both modes, the check name and the decode prefix.
Gate: green (fmt/clippy/tests/doc/actionlint/deny/audit/MSRV/release/repro/golden/smoke),
  107 tests, 34 repro scenarios, 0 failures/2 SKIPs/0 XPASS, G7 green.
Residual risk / next: only PNG CRC semantics changed. JPEG strict scan (H-18),
  PDF xref validation (H-19) and repair revalidation (H-21) remain open.

Task: H-16 — Validity model absent; default output reads "complete / 100"
Reviews: v1:R-08, v2:R-08, v3:R-08, v4:F-08, v5:F-08 + F-17 (RT×3)
Repro: new `tests/validity_model.rs` built a default-mode spliced JPEG from
  the embedded baseline fixture. Before the fix it failed at
  `assert_eq!(file["validity"], "structural")` with `left: Null`; the same
  row was `complete/structure/100` and the console contained no content-check
  caveat. X24 already covered interleaved fragments but only checked the old
  `validation` field.
Change: added `Validity` and `ValidationCheck` in `src/signatures/mod.rs` with
  the ADR-H1 legacy mapping. `CarvedFile` now exposes required `validity` and
  structured `checks[]`; the B-06 integer count is kept in Rust and serialized
  as additive `fragment_checks` so existing Rust callers still compile.
  Summary counts, per-file console lines, the CSV's final column and
  `SHA256SUMS` comments now show the validity label; structural output is
  printed as "content not checked" and never as complete/verified.
  `validated_bytes` is carried from the validator result and is zero unless
  contiguous content bytes were actually checked. An output overlapping an
  unreadable region is downgraded to `suspect` and gains the
  `overlaps_unreadable_region: false` check. `--complete-only` help now says
  "footer found, not verified". Schema 2 requires `validity`/`checks` and
  documents `fragment_checks`; schema 1 strips all three.
Tests: `tests/validity_model.rs` verifies the default spliced-JPEG label,
  check object, score/emission preservation, CSV, manifest and console.
  `tests/cli_contract.rs` asserts the new schema keys, check-object shape and
  schema-1 stripping. X10–X13/X24 now assert validity, not just legacy
  `validation`; X14 additionally checks the repaired label but remains xfail
  for the missing `sha256_original` (H-21).
Gate: green (fmt/clippy/tests/doc/actionlint/deny/audit/MSRV/release/repro/golden/smoke).
Residual risk / next: this commit establishes the model and honest labels;
  H-17–H-21 still refine CRC, JPEG strict-scan, PDF xref, EOL and repair
  semantics. No scores, emission decisions, default raw path or published
  benchmark figures changed.

Task: H-15 — Parallel carve slower than one thread
Reviews: v2:N-03 (ONE); conflict C5 from v1/v4/v5
Repro: X23 flood image, 4 MiB with a JPEG header every 512 B. Three direct
  runs per arm on 2026-11-22, `--threads 1` versus default.
Measurements: current glibc source build (`target/release/carvex`): medians
  0.489 s (1 thread) and 0.156 s (default), ratio 0.32x; individual runs
  0.480/0.489/0.501 s and 0.150/0.156/0.164 s. Stale 0.2.0 musl dist binary
  (`dist/carvex-linux-x86_64`): medians 0.694 s and 2.436 s, ratio 3.51x;
  individual runs 0.694/0.727/0.669 s and 2.436/2.432/2.471 s. The dist
  binary predates H-08's streaming-write/buffer-reuse work.
Change: none. This is condition (a) of the harness procedure: only the
  packaged musl release is slow, so the finding is closed with measurements.
  An allocator change needs STOP-AND-ASK and was not made; no tuning was done.
Gate: green (fmt/clippy/tests/doc/actionlint/deny/audit/MSRV/release/repro/golden/smoke).
Residual risk / next: the historical 0.2.0 dist binary was not a valid 0.3.0
  performance artifact. The 0.3.0 musl release was re-timed after rebuild:
  X23 default 2.80 s vs 1 thread 1.26 s and R5 5.50 s vs <= 1.0 s. No
  allocator tuning was made; see the release section above.

Task: R2-02 / H-15 — worker scratch reuse (ADR-H15)
Reviews: round-2 R2-02 (harness §R.3); addresses H-15 musl parallel slowdown.
Repro: harness baseline recorded 0.3.0 musl X23 default 2.80s vs 1t 1.26s
  (2.22x) and R5 5.50s; dev source already fast, so fix targets allocation churn.
Change: `src/scan.rs` count/scan/carve workers reuse per-worker scratch buffers
  via `rayon::par_iter().map_init(Vec::new, ...)`. `count_task`/`scan_task`
  resize and read into scratch; `carve_one` reuses scratch for the initial buffer
  and `grow_carve_buffer` drops buffer and releases budget permit before
  reserving/growing, preserving ADR-H15 deadlock avoidance. Retained carve scratch
  capacity is capped at 1 MiB after each candidate.
New tests: existing scan/work_budget/regress_task_plan tests cover behavior;
  no output/ordering/status/score changes.
Gate: green (full `scripts/gate.sh` on 2026-11-23: fmt/clippy/tests/docs/actionlint/deny/audit/MSRV/release/G8/G5/coverage/fuzz-smoke/review-repro/X08b/X26/G7/G9/check_status; `GATE GREEN`).
Measurement: dev release build on this host (not shipped musl): X23 default
  0.168s vs 1-thread 0.591s (0.28x), R5 0.280s, rc 0. Shipped 0.3.1 Linux glibc
  binary passes X23/R5/G8 (X23 0.158s vs 1t 0.565s, 0.28x; R5 0.310s; perf gate
  GREEN). Musl build measured but not shipped (X23 2.34x; R5 5.020s).
Residual risk / next: optional musl allocator follow-up if a static musl binary
  is required; current shipped Linux binary is glibc-linked (min GLIBC 2.34).

Task: R2-03 / H-18 — JPEG content-continuity check (ADR-H18)
Reviews: round-2 R2-03 (harness §R.3); supersedes D-45 disclosure-only closure.
Repro: D-45 recorded 5/4000 accepted byte-aligned tail swaps (0.13%,
  Wilson 95% [0.05,0.29]) as self-consistent baseline bitstreams.
Change: added conservative pixel-domain content-continuity check in
  `src/signatures/jpeg.rs` and wired it in `src/signatures/mod.rs` for baseline
  sequential JPEGs that decode and pass strict scan. Failed check returns
  `validation=partial`, `validity=suspect`, `status` unchanged, score unchanged,
  and `content_continuity ok=false`. Unsupported/progressive/arithmetic variants
  report not applicable.
New tests: existing JPEG/corpus tests pass; clean corpus 0 suspects.
Gate: green (full `scripts/gate.sh` on 2026-11-23: fmt/clippy/tests/docs/actionlint/deny/audit/MSRV/release/G8/G5/coverage/fuzz-smoke/review-repro/X08b/X26/G7/G9/check_status; `GATE GREEN`).
Measurement: clean-file control 1000 generated baseline JPEGs: 0 false suspects.
  Dev splice set (disjoint, 500, 499 carved files): 0 verified. Eval splice set
  (disjoint, 4000 generated, 3997 carved files): 2 verified, 3987 suspect,
  8 partial; Wilson 95% upper bound 0.182% (above 0.1% target). R2-03
  upper-bound acceptance criterion not met; residual is documented in D-67.
Residual risk / next: conservative threshold avoids clean false positives but
  misses some low-contrast/flat self-consistent splices; accepted PARTIAL for
  0.3.1 release with measured evidence (option A, 2026-11-23). Aggressive
  content-continuity heuristic authorized as measured follow-up prototype.

Task: R2-08 — Scenarios needing absent tools
Reviews: round-2 R2-08 (harness §R.3).
Repro: N/A (tool availability/evidence task).
Change: Ran available scenarios and recorded SKIPPED reasons for missing tools.
Measurement (2026-11-23, debug build `target/debug/carvex`): X19 PASS with real
  `mkfs.vfat`/`mkntfs` (`scripts/review_repro.py case_x19`); X20 PASS with real
  bench FAT32 image (`case_x20`); R7 PASS with real bench NTFS image
  (`case_r7`); R8 PASS with real bench FAT32/NTFS images (`case_r8`).
SKIPPED: X16(a)/X17 real `ewfacquire`/`ewfexport`/`qemu-img` images (tools not
  installed; `libewf2` package present but no acquire/export CLI); six `qpdf
  --check` X11 controls (`qpdf` not installed); 13 fuzz targets x60s with
  real-container seeds not re-run (cargo-fuzz present, opportunistic/not
  executed in session); coverage ratchet re-measurement not run
  (cargo-llvm-cov present, opportunistic).
Residual risk / next: real-tool E01/VMDK/qpdf controls remain unexecuted until
  tools/images are available; record as SKIPPED, not pass.

Task: H-14 — No fuzz targets for the new parsers
Reviews: v1:R-19, v2:N-06, v4:F-14, v5:F-10 (SRC×4)
Repro: REPRODUCED statically before the change: `fuzz/fuzz_targets`
  contained only the seven original targets; there was no target for E01,
  VHD, VMDK, split-raw name parsing, `open_image` dispatch or B-06 fragment
  joins despite the new attacker-controlled parsers in `src/image/`.
Change: added `fuzz_e01`, `fuzz_vhd`, `fuzz_vmdk`, `fuzz_split_names`,
  `fuzz_open_image` and `fuzz_fragment_join`. A hidden `fuzz_support` module
  shares bounded builders with `tests/evidence_formats.rs`, an example
  generator writes deterministic seeds, and raw passthrough keeps seed magic
  intact. Seeds include fixture-derived containers and hostile X07–X09
  samples. Real-tool containers were SKIPPED because `ewfacquire`,
  `ewfexport` and `qemu-img` are not installed. The 13 targets are listed in
  the CI fuzz matrix; the job itself remains scheduled/manual as before
  (H-30 covers push/PR wiring).
New tests: five structured-mutation property tests in
  `src/image/fuzz_support.rs` cover E01, VHD (including a hole), VMDK,
  split-raw names and `open_image` dispatch. The fuzz entry point for
  fragment joins calls `try_fragment_join` directly so libFuzzer sees panics
  that the production H-10 containment wrapper would otherwise convert.
Fuzz runtime (2026-11-22, cargo-fuzz 0.13.2, nightly
  `1.102.0-nightly`): `scripts/fuzz.sh 60 fuzz_e01 fuzz_vhd fuzz_vmdk
  fuzz_split_names fuzz_open_image fuzz_fragment_join`; six targets × 60 s,
  235,272 executions, no crash, timeout, OOM or artifact. Per-target:
  `fuzz_e01` 968, `fuzz_vhd` 79,089, `fuzz_vmdk` 48,460,
  `fuzz_split_names` 87,028, `fuzz_open_image` 6,249 and
  `fuzz_fragment_join` 13,478. No crash fixture was required because no
  failure was found. Logs are in `bench/results/fuzz/` and the counts are
  recorded in `docs/VALIDATION.md §4`.
Gate: green (fmt/clippy/tests/doc/actionlint/deny/audit/MSRV/release/repro/golden/smoke).
  G8 recheck on the release binary: s9_4m median 0.140 s, rand64m median
  0.032 s, both PASS.
Residual risk / next: fuzzing is sampling, not proof of parser safety. No
  real-tool E01/VMDK container seeds, post-change GitHub Actions run,
  Windows/musl packaged execution or new coverage measurement is claimed.
  The historical 7×60 s run was not repeated after this change.

Task: H-10 — No per-candidate or container panic containment
Reviews: v1:R-07, v2:R-07, v3:R-07, v4:F-07, v5:F-05
Repro: REPRODUCED red with a hidden test seam: the first
  `tests/panic_containment.rs` run unwound out of the rayon worker and failed
  the test; X06 previously skipped because no seam existed.
Change: candidate analyse/validate and B-06 join attempts use
  `catch_unwind(AssertUnwindSafe(…))` and produce `internal_errors[]` records
  (offset, type, message), increment `stats.internal_errors`, emit the
  incomplete `CANDIDATE_PANIC` warning and exit 3. Other candidates continue.
  `open_image_with_options` converts container-parser panics into typed
  errors. Explicit dev/release profiles keep `panic = "unwind"` (D-40).
New tests: `tests/panic_containment.rs` covers candidate containment, typed
  open errors and the profile invariant; X06 is active and verifies the CLI
  report and exit code.
Gate: green (fmt/clippy/tests/doc/MSRV/repro/golden/smoke). G8 after the
  containment wrappers: s9_4m median 0.150 s, rand64m 0.031 s.
Residual risk / next: no natural panic fixture and no fuzz runtime were used;
  containment is defence in depth only. The panic hook still prints a panic
  message to stderr before containment.

Task: H-09 — Required work budgets
Reviews: v5:F-04, v2 RT, v1/v3 R-01
Repro: REPRODUCED red before the fix: X05 reported missing flags
  `--max-work-per-candidate` and `--max-output`; `--max-candidates` and
  `--time-budget` aborted rather than producing a partial report.
Change: added a two-pass deterministic candidate budget (count per fixed
  chunk task, then allocate the exact global prefix), a per-candidate
  analyzer/validator work counter, a cumulative output cap and a latched
  wall-clock stop. Limit events emit CANDIDATE_LIMIT, WORK_LIMIT, OUTPUT_LIMIT
  or TIME_LIMIT, write a partial report and exit 3. Only TIME_LIMIT sets
  `nondeterministic_stop: true`. Report schema 2 records the new limits and
  stop flag; schema 1 strips them (D-39).
New tests: `tests/work_budgets.rs` covers all four limits, including the
  nondeterministic flag and warning incompleteness. X05 was removed from the
  expected-failure list.
Gate: green (fmt/clippy/tests/doc/MSRV/repro/golden/smoke).
Measurement: D0 used 27–41 candidates and wrote 6.3–14.3 MiB; S9-4M flood16
  used 262,144 candidates; S10 flood512/64 MiB used 129,804 candidates. Lowest
  non-triggering work limit was 8,323,072 bytes on D0 and 65,536 on the floods.
  Chosen defaults (1M candidates, 256 MiB work, 4 GiB output) did not trigger.
  G8 after the second lexical pass: s9_4m median 0.162 s, rand64m 0.031 s.
Residual risk / next: CFReDS RM#2 was not available; no claim is made for it.
  Wall-clock results are explicitly flagged as nondeterministic.

Task: H-08 — `--max-memory` must bound working memory, not output
Reviews: v1:R-01, v2:R-01, v3:R-01, v4:F-01, v5:F-01
Repro: REPRODUCED with an in-memory fixture (300 x 60 KiB JPEGs, 18 MiB of
  output against a 16 MiB cap). The original red test failed with the old
  non-blocking retention budget; the Python X01 fixture is smaller than the
  reviewer's and did not trip the old post-hoc exit by itself.
Change: carved results are now source-range descriptors plus SHA-256 and
  analysis metadata. Workers use a blocking permit budget, release and drop a
  buffer before growing it, and the write stage re-reads each range into a
  fixed scratch buffer, hashes while writing a temporary file, verifies the
  digest, then atomically renames (D-38). Mismatches emit SOURCE_CHANGED;
  oversized per-candidate requirements emit CANDIDATE_SKIPPED_BUDGET. Both are
  coded warnings with exit 3 and a report.
New tests: `tests/memory_budget.rs::{dense_successful_carves_complete_within_a_working_memory_budget,
  changed_source_is_detected_before_a_file_is_written}` (first test red).
Gate: green (fmt/clippy/tests/doc/MSRV/repro/golden/smoke).
Measurement: the 18 MiB fixture completes with all 300 files and no
  incomplete warnings. G8 s9_4m median 0.476 s before H-08 vs 0.164 s after;
  rand64m 0.033 s in both runs. The double-read path did not regress on this
  workload.
Residual risk / next: exit-3 reporting is covered by the CLI warning mapping;
  H-09 will add work/output budgets on the same warning/report path.

Task: H-06 — JPEG SOF dimensions must not drive unbounded allocation
Reviews: v2:N-02 (ONE); local source-build reproduction was weak
Repro: the OOM was not re-observed locally, but the red test proved that an
  oversized SOF reached the decoder even with a 1 KiB validator budget.
Change: `jpeg::frame_claim` reads SOF dimensions/components without decoding;
  `validate_jpeg` checks the claimed sample count against
  `--max-validator-memory` and, for Huffman frames only, against an optimistic
  entropy capacity of 512 samples per input byte (D-37).
New tests: `tests/jpeg_frame_budget.rs::{claimed_frame_above_the_validator_budget_is_not_decoded,
  implausible_entropy_claim_is_not_decoded_with_a_large_budget}` (first red).
Gate: green (fmt/clippy/tests/doc/MSRV/repro/golden/smoke).
Measurement: patched baseline JPEG at 20,000x20,000 → `unverified`, 0.010 s,
  8.9 MiB RSS; 65,535x65,535 → `unverified`, 0.009 s, 9.0 MiB RSS.
Residual risk / next: arithmetic-coded JPEGs are exempt from the entropy
  plausibility check but still obey the memory budget; structure mode is
  intentionally unchanged.

Task: H-05 — PNG decode inflation must be bounded
Reviews: v1:R-02, v2:R-02, v3:R-03, v4:F-02, v5:F-02
Repro: NOT-REPRODUCED for the OOM itself on 0.3.0-dev (R-02 already streams
  IDAT and checks IHDR first); the missing common budget flag was reproduced
  red by `tests/png_validator_budget.rs` before the fix.
Change: added `--max-validator-memory` (visible alias `--max-decode-bytes`),
  shared by PNG and later JPEG validators; report schema 2 records both the
  authoritative and legacy field names; README/CHANGELOG updated.
New tests: `tests/png_validator_budget.rs::{max_validator_memory_is_a_cli_flag,
  png_bomb_is_not_verified_with_a_small_validator_budget}` (red → green).
Gate: green (fmt/clippy/tests/doc/MSRV/repro/golden/smoke).
Measurement: X02a 1 MiB inflate claim → `partial`, 0.009 s, 8.8 MiB RSS.
  X02b 32000x32000 (1,024,032,000 raw bytes) → rejected before inflation at
  both the 256 MiB default and an explicit 1 KiB budget.
Residual risk / next: H-06 must consume the same budget for JPEG frame claims.

Task: H-11 — E01 allocations from untrusted header fields; stdin spool
Reviews: v5:F-21, v1:R-19, v2:N-06 (CONFLICT C3)
Repro: REPRODUCED — a 64 GiB-declaring E01 aborted in v5; local in-memory
  fixture now fails in 0.005 s with a typed error (`info` 12 MiB RSS, `scan`
  13 MiB RSS).
Change: `src/image/e01.rs` caps declared chunks at 64 MiB and validates
  sector size; `src/image/mod.rs` bounds stdin spooling via `--max-stdin-bytes`
  and `--spool-dir`; `src/cli.rs` exposes the new flags.
New tests: `tests/regress_e01_safety.rs::hostile_chunk_size_is_rejected`
  (red before / green after).
Gate: green (SKIPPED: real E01 tool scenarios without `ewfacquire`).
Measurement: SIGABRT → typed error, 0.005 s, ≤ 13 MiB RSS.
Residual risk / next: real libewf fixtures remain unavailable; stdin limit is
  exercised through unit-level code, not a full-pipe integration test.

Task: H-11b (R2-11) — deny indexing/unwrap/expect in image parsers
Reviews: round-2 R2-11 (harness §R.3); extends H-11.
Repro: NOT REPRODUCED as a crash; static audit found production indexing and
  error-unwrap patterns in `src/image/{mod,e01,split,vhd,vmdk}.rs`.
Change: add `#![deny(clippy::indexing_slicing, clippy::unwrap_used,
  clippy::expect_used)]` to the five image parser modules; replace production
  indexing with checked access and typed errors; keep `fuzz_support` and test
  modules under narrow justified `#[allow(...)]`.
New tests: none (lint/compile-time enforcement); existing image and regress
  tests cover behavior.
Gate: green (full `scripts/gate.sh` on 2026-11-23: fmt/clippy/tests/docs/actionlint/deny/audit/MSRV/release/G8/G5/coverage/fuzz-smoke/review-repro/X08b/X26/G7/G9/check_status; `GATE GREEN`).
Measurement: `cargo clippy --all-targets --all-features --locked -- -D warnings`
  passes; `cargo test --all-features --locked` passes.
Residual risk / next: none for R2-11.

Task: H-22 — E01 partial/damaged chunk handling
Reviews: v5:F-20 (ONE, real libewf)
Repro: REPRODUCED (in-memory) — short final chunks and damaged-chunk policy
  were demonstrated with generated EWF sections. Real `ewfacquire` fixtures
  are SKIPPED because the tool is not installed.
Change: `src/image/e01.rs` reads the logical final-chunk length, accepts both
  checksum variants and routes inflate/checksum failures to
  `--on-read-error`; `src/image/mod.rs` records unreadable ranges and wraps raw
  and split sources in a per-sector error policy. On `abort`, E01 and raw
  readers record the failed range before returning an error, and the CLI writes
  a schema-2 partial report containing `READ_ABORTED` before propagating exit 1.
New tests: `tests/regress_e01_safety.rs::partial_final_chunk_is_tolerated`
  (red before / green after), `damaged_chunk_is_reported_and_outputs_are_not_verified`,
  and `abort_writes_partial_report_and_exits_one` (red first: no report was
  written). Two `src/image/mod.rs` unit tests use a failing mock `ImageSource`.
Gate: green, 123 tests, 34 repro scenarios, 0 failures/2 SKIPs/0 XPASS, G7 green;
  G8 PERF GREEN (s9_4m median 0.151 s, rand64m median 0.034 s). X16 now checks
  zero-fill, abort and skip against the same damaged E01 fixture.
Measurement: hard failure → short tail readable as zeros; damaged chunks no
  longer abort under the default policy; abort exits 1 with a partial report.
Residual risk / next: real libewf and multi-segment `.E02` fixtures remain
  SKIPPED because `ewfacquire` is unavailable; Windows, real block devices and
  raw-device error injection are not tested. VHD/VMDK readers keep their own
  container-specific error semantics and are not wrapped by `PolicySource`.

Task: H-25 — Container identity in reports
Reviews: v5:F-26 (RT/SRC)
Repro: REPRODUCED — X22 showed stdin `unchanged=true` with null
  `modified_unix`, no `logical_size`, and a logical `size` paired with a
  physical `size_after` for containers.
Change: `SourceIdentity` now records per-file size and mtime; `ScanContext`
  carries the resolved format and physical size; schema-2 `input` adds
  `logical_size`, `logical_size_after`, `physical_size`, `physical_size_after`,
  `format` and `files[]`. Legacy `size`, `size_after`, mtime fields and schema 1
  are unchanged. Stdin `unchanged` is now null because there is no stable
  backing path.
New tests: `tests/evidence_formats.rs` compressed-E01 and VMDK identity tests,
  plus the updated stdin test (red first: `left: Null` for `format` and the new
  fields). `tests/cli_contract.rs::emitted_report_matches_the_published_schema`
  checks the new `input` and `input_file` definitions.
Gate: green, 125 tests, 34 repro scenarios, 0 failures/2 SKIPs/0 XPASS, G7 green;
  G8 PERF GREEN (s9_4m median 0.146 s, rand64m median 0.033 s). X22 now builds
  a compressed E01 and a monolithicSparse VMDK and was removed from xfail.
Measurement: compressed E01 and sparse VMDK report logical capacity from the
  container header and physical bytes from their backing files; `unchanged`
  stays true when those files are stable. Stdin reports equal logical/physical
  spool sizes, no backing files, and `unchanged: null`.
Residual risk / next: `input.files[]` lists backing files, not byte-level
  resolved grain/extent maps; no libewf-made multi-segment `.E01`/`.E02`
  fixture was available. The logical size is fixed at open time for containers
  and is not re-read from a changed descriptor during the run.

Task: H-26 — Surface unparsable volumes
Reviews: v5:F-13 (RT)
Repro: REPRODUCED — X19 built a 98 MiB FAT32+NTFS disk with a corrupted
  NTFS `$MFT`. The scan exited 0 with no warning; `info`/`volumes[]` showed
  only one volume. Red tests also showed the single-volume fallback saying
  "no FAT32/NTFS filesystem found" although an NTFS boot sector was present.
Change: `VolumeSet.errors` is now a structured `VolumeParseError` with
  partition index, byte offset and underlying reason. `probe()` wraps NTFS and
  FAT32 parser failures with the recognised boot sector and byte offset.
  `scan` emits `VOLUME_PARSE_FAILED` warnings (coded as incomplete ⇒ exit 3)
  even when another volume parses, and adds `may_contain_allocated: true` to
  the fallback `unsupported` region. `info` still exits 1 but now reports the
  specific cause.
New tests: `tests/volume_parse_errors.rs` has three red-first cases covering
  the multi-volume warning/region contract, the specific
  `FS_DETECTION_FAILED` message, and `info` exit 1 with the cause. Before the
  fix the multi-volume case exited 0 (`left: Some(0)`, `right: Some(3)`), the
  fallback message was generic, and `info` hid the reason. X19 now asserts the
  same contract and was removed from xfail.
Gate: green, 128 tests, 34 repro scenarios, 0 failures/2 SKIPs/0 XPASS, G7 green;
  G8 PERF GREEN (s9_4m median 0.138 s, rand64m median 0.033 s). S8/S8b still
  do not abort: a degraded scan finishes with exit 3 and `info` fails with
  exit 1 and an explicit reason.
Measurement: the real X19 fixture uses `mkfs.vfat`/`mkntfs` and the corrupted
  NTFS partition at byte 34,603,008. The deterministic Rust fixture uses a
  synthetic NTFS boot sector at byte 1,048,576 for the single-volume path.
Residual risk / next: recovery of allocated metadata inside an unparsable
  volume remains unavailable; the run only declares that the raw fallback
  mixes allocated and deleted bytes. No backup-boot-sector, `$MFTMirr` or
  `$ATTRIBUTE_LIST` fallback was added.

Task: H-27 — Faithful `regions[]` coverage
Reviews: v5:F-16 (RT + SRC)
Repro: REPRODUCED — the baseline X20 on `bench/images/all-deleted_fat32.img`
  returned `regions overlap or are unsorted`. The v5 report summed scanned
  rows to 102,760,448 B against 95,367,168 B actually scanned; `info --json`
  reported a 64 MiB free region when only 59.4 MiB was free.
Change: `scan::plan_extents` is public and supplies the exact scanner extents
  to `VolumeSet::regions_with_plan`. Regions now contain the merged free
  extents, the allocated complement, gaps and unsupported partitions as
  disjoint, sorted, whole-image-covering rows. Every scanned row carries the
  scanner `extent_id`; additive `summarized` identifies rows merged by
  `--max-regions` (default 4096, 0 = unlimited). A `mixed` kind is used only
  when summarisation joins different row kinds. Summarisation never changes
  coverage or the sum of scanned lengths; when scanned/unscanned states
  alternate more often than the requested cap, exact accounting wins over
  the soft cap.
New tests: `tests/region_coverage.rs` has three red-first CLI cases. Before
  the fix they failed with `region 0 starts at 1048576, expected 0`, a
  whole-volume `info` free row, and clap rejecting `--max-regions`. X20 was
  strengthened to assert full coverage, all additive fields, real free
  extents, exact report/stats byte sums and a raw-mode cap control; it was
  removed from xfail.
Gate: green, 131 tests, 34 repro scenarios, 0 failures/2 SKIPs/0 XPASS, G7 green;
  G8 PERF GREEN (s9_4m median 0.167 s, rand64m median 0.032 s). Carving output,
  scores, emission, output names, CSV and schema 1 are unchanged.
Measurement: on the FAT32 benchmark image the new report has 16 disjoint
  regions covering all 67,108,864 bytes; auto mode sums 66,087,936 scanned
  bytes, allocated mode 3,118,080, raw mode 67,108,864, and
  `--partition largest --no-gaps` 63,990,784 — each equal to `bytes_scanned`.
Residual risk / next: the merged `mixed` kind is additive schema-2 metadata;
  consumers that reject an unknown enum value must stay on schema 1 or avoid
  `--max-regions` values that force summarisation. No output or scan behavior
  changed.

Task: H-28 — Cooperative SIGINT/Ctrl-C stop
Reviews: v5:F-14 (RT), v2:A-13 (SRC), docs `ARCHITECTURE.md`
Repro: REPRODUCED — X21 signalled the unmodified `target/release/carvex`
  directly and failed with `exit -2 after SIGINT (expected 4)`; no report was
  written. The test note about non-interactive shell background jobs was
  followed by spawning the child directly.
Change: `ctrlc` 3.5.3 installs one process-wide handler. The first signal
  latches shared state that joins the existing wall-clock stop checks at
  candidate counting, carving, fragment recovery and output writing; the
  second signal exits 130. `run_scan` emits schema-2 `interrupted: true` and
  an `INTERRUPTED` warning, and `cli::scan` returns 4. `partial_report`
  carries the flag when a signal precedes a read-error abort. Schema 1 drops
  the new field.
Evidence: after the fix X21 passed with exit 4, schema 2, `interrupted: true`,
  an `INTERRUPTED` warning and no `.tmp` files. A direct control with two
  signals 0.02 s apart exited 130 and wrote no report. The full gate reported
  131 tests, 34 scenarios, 0 failures/2 SKIPs/0 XPASS, GOLDEN GREEN and
  **GATE GREEN** (`/tmp/opencode/gate_h28.log`); `scripts/perf_gate.py`
  reported **PERF GREEN**, s9_4m median 0.156 s and rand64m median 0.039 s
  (`/tmp/opencode/perf_h28.log`). X21 was removed from xfail.
Residual risk / next: the handler is process-wide, so library callers that do
  not install it pass `interrupt: None`. No packaged Windows binary was
  available to execute; Windows Ctrl-C support is a source/dependency claim
  only. No lockbox, full benchmark or `dist/` rebuild was run.

Task: H-29 — B-06 burned the budget and accepted a false splice
Reviews: v1:R-20, v2:N-07, v4:F-16, v5:F-24 (SRC/RT); harness §H-29.
Repro: REPRODUCED on the 0.3.0-dev source and release binary. Red evidence
  retained in `/tmp/opencode`: schema validation lacked `alternatives`, the CSV
  header lacked the trailing column, S12 failed with `file A not recovered
  exactly`/`file B not recovered exactly`, and strengthened X15 failed with
  `realistic: 1 stitched output(s) are not the source files`. The false PDF
  stitch joined tail `4085248`; the planted tail was at `3773952`.
Change: `--max-frag-candidates` now counts head probes and join attempts, while
  `--frag-time-limit` is a flagged wall-clock safety net. Contiguous, gap-free
  carves are not deep-probed except when truncated, containing filler, or PDFs
  with multiple footers; footer indexing is deferred until a head needs it.
  Per-head filler/header flags score the skipped gap, joins are ranked by
  unexplained gap then evidence strength, and only `validity == verified`
  stitches are emitted under the H-10/H-05/H-06 boundaries. Accepted stitches
  record an additive `alternatives` count in JSON and CSV.
Measurement: `scripts/fragment_profile.py --carvex target/release/carvex
  --corpus bench/corpus --work target/fragment-profile --out
  bench/results/d1_fragment_profile.json` produced recall 4/6 = 0.667, Wilson
  95% [0.299993, 0.903229]; splice rate 0/4 = 0.0, Wilson 95% [0.0, 0.489891];
  junk-FP 0/24 = 0.0, Wilson 95% [0.0, 0.137976]. The release binary SHA-256 is
  recorded in the JSON file. No tuning followed this measurement.
Evidence: strengthened X15 passed: bait 0 stitches; ordinary image had no
  `FRAGMENT_*` warning, 20 identical carved outputs and runtime within 10% of
  the plain scan; realistic layout recalled all 3 exact source files (JPEG,
  PNG, PDF); `--threads 1` matched the default report and output hashes.
  Final gate: 134 tests, 34 scenarios, 0 failures/2 SKIPs/0 XPASS, GOLDEN GREEN
  and **GATE GREEN** (`/tmp/opencode/gate_h29.log`); `scripts/perf_gate.py`
  reported **PERF GREEN**, s9_4m median 0.145 s and rand64m median 0.035 s
  (`/tmp/opencode/perf_h29.log`). X15 was removed from xfail.
Residual risk / next: the D1-style profile is small and its confidence
  intervals are wide; D2–D7 and the frozen lockbox were not touched. Only two
  fragments and no metadata/runlist-assisted reassembly are supported. No new
  coverage measurement, full benchmark, `dist/` rebuild or tag was run.

Task: H-30 — Gates G7–G9 absent and CI did not enforce the guide
Reviews: v1:R-09, v2:R-09, v3:R-09, v4:F-09, v5:F-11 (SRC×5).
Repro: static evidence before the change — `scripts/gate.sh` had G0-G6 plus G7
  golden but no G8/G9, no coverage or fuzz invocation, and `.github/workflows/ci.yml`
  ran fuzz only on `schedule`/`workflow_dispatch`; the packaged Windows exe was
  built but never executed and no SBOM was produced.
Change: `scripts/gate.sh` now runs fmt, clippy, tests, rustdoc, actionlint,
  deny/audit, MSRV, release build, G9 (`bench/render_docs.py --check`), G8
  (`scripts/perf_gate.py` with committed baseline ratios), coverage (floor 70),
  fuzz smoke (13 targets, 60 s each by default), G7 golden, X01-X24 repro and
  benchmark smoke. CI adds a full-gate job, an explicit repro job, push/PR fuzz
  smoke, packaged Windows and musl execution jobs, coverage at 70% and an SPDX
  SBOM artifact. `scripts/perf_gate.py` gained `--strict`; relative regressions
  are reported by default because shared runners are noisy. X15 compares the
  median of nine paired B-06/plain runtime ratios after one discarded warm-up
  pair, instead of independently sorted medians or three pairs. Both earlier
  forms were sensitive to scheduler noise on clean builds: a three-pair run
  during the final gate produced a false X15 failure (ratio 1.176) despite
  identical outputs and no fragment warning.
Gate-efficacy test: performed in detached worktree `/tmp/opencode/h30-efficacy`
  with `CARVEX_FUZZ_SECONDS=1 CARVEX_COVERAGE_MIN=0` to keep the test short.
  Every reintroduction was applied to a clean worktree and the gate was run:
  H-05 (`let max_decoded = u64::MAX`) failed `tests/png_decode_budget.rs` with
  `expected a validator-budget note, got: ["PNG zlib stream did not terminate"]`
  (`/tmp/opencode/gate_efficacy_h05.log`); H-07 (per-`%%EOF` full-prefix rescans
  and a 4 KiB look-ahead) failed X04 with `took 25.38s for 4096 EOF markers`
  (`gate_efficacy_h07.log`); H-08 (retaining carved outputs against
  `--max-memory`) failed `dense_successful_carves_complete_within_a_working_memory_budget`
  with `retaining carved output would exceed --max-memory 16.0 MiB`
  (`gate_efficacy_h08.log`); H-17 (CRC check reported ok and no Partial return)
  failed `signatures::png::tests::reports_bad_crc_but_keeps_file` at
  `assertion failed: a.integrity_failed()` (`gate_efficacy_h17.log`). All four
  gates exited nonzero; the worktree was removed afterwards.
Local evidence: `CARVEX_FUZZ_SECONDS=1 scripts/gate.sh` reported G9 GREEN for
  48 scan options, G7 GOLDEN GREEN, PERF GREEN (s9_4m 0.158 s / 0.403x baseline;
  rand64m 0.030 s / 1.001x), coverage complete at 70%, 13 fuzz targets × 1 s,
  34 scenarios with 0 failures/2 SKIPs/0 XPASS and GATE GREEN
  (`/tmp/opencode/gate_h30_smoke.log`). A full `scripts/gate.sh` run with the
  default 60 s fuzz budget reported G9 GREEN, G7 GOLDEN GREEN, PERF GREEN
  (s9_4m 0.154 s / 0.392x baseline, rand64m 0.030 s / 1.001x), coverage
  complete at 70%, 18,942,273 fuzz executions, 34 scenarios with 0
  failures/2 SKIPs/0 XPASS and **GATE GREEN** (`/tmp/opencode/gate_h30_final.log`).
  The pushed HEAD's GitHub run ID is recorded in `docs/VALIDATION.md`.
Residual risk / next: GitHub run `45035152791` on `d9dcac3` completed **failure**
  (8 jobs success, 18 failure), including the release gate, repro assertions,
  all 13 fuzz targets, Windows tests and both packaged-binary smokes. The
  final-head run `45095456201` on `360f154` also completed **failure** (9 jobs
  success, 18 failure), with the same 18 failing job categories. The log
  endpoint returned `403 Must have admin rights to Repository.`, so no remote
  failure cause is claimed. The 0.3.0 packaged binaries were rebuilt from local
  tag `v0.3.0`; the Windows binary was executed under Wine only. No green CI at
  final HEAD, lockbox regeneration or full-benchmark rerun is claimed; the
  `v0.3.0` tag was pushed to origin (confirmed 2026-11-23). `v0.3.1` release
  commit `d5be769` is tagged locally; tag push is pending after green CI
  evidence is recorded (R2-10).

Task: H-31 — Status labels overstated or inconsistent
Reviews: v1:R-15, v2:N-04 and v5:F-12.
Repro: static evidence before the change — A-03 was DONE in `PROGRESS.md` but
  MITIGATED in `REVIEW_RESPONSE.md`; C-09 was PARTIAL vs OPEN; A-03, A-04,
  A-06, A-13, A-14, A-10, D-01 and D-03 were overstated; `PROGRESS.md` had a
  second, incomplete H-xx table and `REVIEW_RESPONSE.md` had no H-xx table.
Change: `scripts/check_status.py` parses the guide table in both documents,
  both H-xx tables, and the explicit links from DONE/FIXED guide items to
  CLOSED H-xx findings with a commit and an existing test/control path. It is
  wired into `scripts/gate.sh` and a CI job. The statuses were relabelled, the
  duplicate table was removed, and one H-01…H-34 row now exists in each
  document.
Red evidence: before documentation changes the checker exited 1 with
  `STATUS FAIL: section heading not found`
  (`/tmp/opencode/status_red_before_docs.txt`). After the fix, temporary copies
  with A-10 marked `DONE`/`FIXED` exited 1 with `A-10: linked H-34 is OPEN, not
  CLOSED` and `A-10: linked H-34 has no commit hash ('-')`
  (`/tmp/opencode/status_red_wrong_label.txt`); the real documents were
  untouched by the control. The final command printed
  `STATUS GREEN: 38 guide IDs and 34 harness IDs agree between documents;
  DONE/FIXED links verified`.
Gate: the final H-31 tree ran `scripts/gate.sh` with the default 60 s fuzz
  budget: G9 GREEN for 48 options, STATUS GREEN, G7 GOLDEN GREEN, 70% coverage,
  13 fuzz targets × 60 s with 16,964,405 executions, 34 scenarios with 0
  failures/2 SKIPs/0 XPASS and **GATE GREEN**
  (`/tmp/opencode/gate_h31_final.log`). G8 reported s9_4m 0.147 s / 0.376x
  baseline and rand64m 0.035 s / advisory 1.161x; no threshold was changed.
Residual risk / next: H-01 remains BLOCKED until the user supplies
  `docs/guide.md`; H-04, H-33 and H-34 remain open in their own findings.
  This is a documentation/process fix: no CLI, JSON/CSV, score, warning code,
  default or frozen result changed.

Task: H-32 — Evidence cited in the documents is not in the tree
Reviews: v1:R-14, v2:N-08, v3:R-09 and v4:F-15.
Repro: static red evidence before the change — `git check-ignore -v` showed
  `bench/results/fuzz/`, `bench/results/coverage/`, `bench/results/lockbox-v1/`,
  `bench/results/full-0.2.0/`, `bench/lockbox/` and `bench/results/*.log` were
  excluded; `bench/results/EVIDENCE.md` and `bench/lockbox.log` did not exist;
  `docs/VALIDATION.md` did not cite a manifest
  (`/tmp/opencode/h32_static_red.txt`).
Change: `bench/results/EVIDENCE.md` lists 47 artefact digests and the bundle
  checksums; `bench/results/EVIDENCE.sha256` is a `sha256sum`-compatible
  manifest; `bench/results/evidence-bundle-0.2.0.tar.gz` prepares those
  artefacts for the user to publish. `bench/lockbox.log` is explicitly
  reconstructed from `docs/VALIDATION.md`, not independently observed.
  `docs/VALIDATION.md` now cites the manifest and labels historical fuzz,
  audit and coverage figures without retained raw logs as **author-reported**.
Red evidence: extracting the bundle into an empty directory and running
  `sha256sum -c bench/results/EVIDENCE.sha256` verified all 47 entries
  (`/tmp/opencode/h32_bundle_verify.txt`). No lockbox dataset or result was
  regenerated; the 1.8 GiB dataset itself is not bundled, only its checksum
  files and one-shot execution results.
Gate: the H-32 candidate tree ran `scripts/gate.sh` with the default 60 s fuzz
  budget: G9 GREEN, STATUS GREEN, G7 GOLDEN GREEN, 70% coverage, 13 fuzz targets
  with 17,940,194 executions, 34 scenarios with 0 failures/2 SKIPs/0 XPASS and
  **GATE GREEN** (`/tmp/opencode/gate_h32_final.log`). G8 reported s9_4m
  0.152 s / 0.387x baseline and rand64m 0.027 s / 0.887x baseline; no threshold
  was changed.
Residual risk / next: the bundle is prepared but not published; only the user
  can attach it to a release or data repository. Remote GitHub job logs remain
  unavailable, so no failure cause is claimed. No CLI, JSON/CSV, score,
  warning code, default or frozen benchmark value changed.

Task: H-33 — Documentation drift
Reviews: v1:R-14, v2:N-10 and v5:F-17.
Repro: a pre-fix `grep` run was committed verbatim as
  `docs/baseline/h33_static_red.txt`. Its key lines were
  `README.md:416: - CarveX is significantly faster in the unallocated
  configuration ...`, `README.md:460: - 318/318 files 'complete', successfully
  opened and verified with Pillow ...`, and `--- fuzz tree entry absent ---
  ABSENT`. It also recorded `git grep -n "/home/kali" ... | wc -l` as 4879 lines
  in generated ground truth, frozen results and working documents.
Change: the speed sentence was deleted and replaced with a reference to the
  generated timing table; the CFReDS line now says the files were structurally
  parsed, not verified (see `validity`), with published counts and hashes
  unchanged; `fuzz/` was added to the README tree; `docs/REPORT.md` keeps the
  published figure and gains an honesty note that "verified" was a historical
  structural check, not pixel decoding; D-57 records the `/home/kali` caveat
  without editing frozen artefacts.
NOT-REPRODUCED: the pre-fix grep output records
  `README.md:132 --max-truncated-size`, `README.md:133 --max-memory`,
  `README.md:468 "A PNG chunk-CRC mismatch is now an integrity failure"`,
  `README.md:502 fragment_recovery.rs`, and F-17 evidence at
  `src/report.rs:57 "validation"` plus
  `docs/report.schema.json:324 "validity"`. These review items were already
  corrected before H-33; no further code change was made for them.
Red evidence: `docs/baseline/h33_static_red.txt` is the unchanged pre-fix
  output. Green evidence: `python3 bench/render_docs.py --check` reports
  `G9 GREEN: 48 scan options match README` and `python3 scripts/check_status.py`
  reports `STATUS GREEN` after the status rows were updated; `actionlint` exited
  0 with no findings.
Gate: the final H-33 tree ran `scripts/gate.sh` with the default 60 s fuzz
  budget: G9 GREEN, STATUS GREEN, G7 GOLDEN GREEN, 70% coverage, 13 fuzz targets
  with 17,365,967 executions, 34 scenarios with 0 failures/2 SKIPs/0 XPASS and
  **GATE GREEN** (`/tmp/opencode/gate_h33_final.log`). G8 reported s9_4m
  0.164 s / 0.419x baseline and rand64m 0.046 s / advisory 1.531x; no threshold
  was changed.
Residual risk / next: the historical `/home/kali` paths remain as provenance by
  decision D-57; regenerating them would change frozen hashes, so no edit was
  made. No CLI, JSON/CSV, score, warning code, default, warning meaning,
  emission, output name or frozen benchmark value changed.

### Task: H-12b — lazy scan task planning (round 2)

Reviews: round-2 verification of H-12 (`harness.md` §R.3 R2-01).
Repro: REPRODUCED on the packaged `dist/carvex-linux-x86_64` and on the
  source-built library. `python3 /tmp/opencode/x08b_red.py dist/carvex-linux-x86_64`
  (512 TiB `ZERO` extent, `RLIMIT_AS=1 GiB`, `--time-budget 5`) exited `-6`
  after 0.47 s with `memory allocation of 1342177280 bytes failed` and no report
  (`/tmp/opencode/x08b_red.txt`). The red Rust test
  `tests/regress_task_plan.rs::declared_size_plan_is_lazy_and_honours_the_time_budget`
  aborted with the same allocation message when run under a 1 GiB address-space
  limit (`/tmp/opencode/r2_01_red_rust.txt`). Cause confirmed at
  `prepare_scan_plan`/`build_tasks`: one `Task` per 4 MiB chunk is pushed into a
  `Vec` before any budget or interrupt can be checked (~10 MiB per TiB).
Change: `src/scan.rs` now plans from `TaskPlan`/`TaskIter` over resolved extents,
  counts/quota-scans 64 tasks per batch, computes totals arithmetically, checks
  the stop state during planning, and reuses the iterator for fragment footer
  scanning. Added `scripts/repro_x08b.py`, `scripts/repro_x26.py` and wired both
  into `scripts/gate.sh`; added D-64 and an Unreleased CHANGELOG entry.
New tests: `tests/regress_task_plan.rs::declared_size_plan_is_lazy_and_honours_the_time_budget`
  aborted before the fix and passes after; X08b and X26 are red/green as
  recorded above and in `docs/DECISIONS.md` D-64.
Gate: green (full `scripts/gate.sh` on 2026-11-23: fmt/clippy/tests/docs/actionlint/deny/audit/MSRV/release/G8/G5/coverage/fuzz-smoke/review-repro/X08b/X26/G7/G9/check_status; `GATE GREEN`).
Measurement: before 512 TiB → SIGABRT at 641.6 MiB RSS in 0.47 s with no report;
  after → exit 3, 5.01 s, 28.0 MiB peak RSS with a valid report under
  `RLIMIT_AS=1 GiB`. X26 → typed errors at ~10 MiB RSS for VHD, E01 and the
  100,000-segment split set.
Residual risk / next: none for H-12b. Wired X08b/X26 into the gate so a return
  to eager planning fails automatically; `dist/` remains 0.3.0 until R2-10.

### Task: H-01 — 924-line acceptance guide committed

Reviews: v1:R-12, v2:R-12, v3:R-13, v4:F-10, v5:F-18.
Repro: `docs/guide.md` was absent; the red check is preserved in
  `/tmp/opencode/h01_red.txt`. The supplied file had the expected size and
  digest before any repository change.
Change: copied `/home/kali/guide.md` verbatim to `docs/guide.md`; added D-63,
  closed the H-01 rows in both status documents, and removed the two now-stale
  "guide missing" blocker bullets from this file.
New tests: `test -f docs/guide.md` failed before and passes after; `wc -l` and
  `sha256sum` are the acceptance checks, and `cmp` proves byte equality.
Gate: green (see the full `scripts/gate.sh` output recorded by this commit).
Measurement: before absent → after 924 lines, SHA-256
  `22ee8122209e9eda11d6f32a1e650e8f8e48f5610bd808309d94f436e5e78265`.
Residual risk / next: none for H-01. The guide is a document only; no CLI,
  JSON/CSV, score, warning code, default, frozen result or binary changed.

### Task: H-34 — A-10 quality limits

Reviews: v2:N-05 (heuristic counts near `706a12a`), with v1/v3/v5 calling A-10
mostly or confirmed. The review asked for a tool check before acting.

Repro: `clippy.toml` had no `too-many-lines-threshold`. With an explicit
  `-W clippy::too_many_lines` and threshold 120, clippy reported 15 non-test
  functions over the limit (largest `cli::scan` 360 lines, `scan::run_scan` 307,
  `scan::recover_fragments` 329) plus one 121-line test. Enabling
  `missing_docs`, `pedantic` and `too_many_lines` together reported 698 warnings.
  The verbatim inventories are `docs/baseline/h34_static_red.txt` and
  `docs/baseline/h34_static_red_all.txt`.

Change: long functions were split into behavior-preserving helpers. `cli::scan`,
  `scan::run_scan`, `carve_one`, `postfilter`, `write_outputs`, `recover_fragments`,
  `fs::{fat32,ntfs}::parse`, `signatures::{jpeg,png,pdf}::analyze` and
  `pdf_validate` now delegate to named phases. `FragmentScanState` captures
  `Instant::now()` at `recover_fragments` entry, so classification reads remain
  inside the `--frag-time-limit` safety net exactly as before. The public API
  gained `#![warn(missing_docs)]` and complete documentation; pedantic stage 1
  enables eight low-risk readability lints. Numeric-cast and API-ergonomics
  stages are deferred in D-58 rather than hidden behind blanket allows. Every
  retained `too_many_arguments` allow has a justification comment; the sole
  `too_many_lines` allow is test-only and justified in the test file.

Preservation checks: the `WORK_LIMIT` message was byte-compared with
  `HEAD:src/scan.rs`; the full set of string literals in `src/scan.rs` is an
  exact multiset match with `HEAD`; G7 golden output, warning codes and
  insertion order, budget/work-unit semantics, panic-probe sites, source-read
  order, SIGINT handling and report fields were unchanged in the full gate.

Green evidence: `docs/baseline/h34_static_green.txt` records zero warnings for
  the explicit 120-line check, `missing_docs`, staged pedantic clippy and
  `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked`, plus the passing
  `cargo test --all-features --locked` summary.

Gate: the final H-34 tree ran `scripts/gate.sh` with the default 60 s fuzz
  budget: G9 GREEN, STATUS GREEN, G7 GOLDEN GREEN, 70% coverage, 13 fuzz targets
  with no crashes, 34 repro scenarios with 0 failures and 0 XPASS, and
  **GATE GREEN**. G8 thresholds were not changed.

Residual risk / next: pedantic stage 2 (numeric casts) and stage 3 (API
  ergonomics) remain deliberately unapplied and are recorded as deferred in
  D-58. No non-test function exceeds 120 lines. No CLI, JSON/CSV, score,
  warning code, default, warning meaning, emission, output name or frozen
  benchmark value changed.

### Conflict measurements (C1-C13)

- **C1 (JPEG splice rate).** `scripts/jpeg_splice_property.py` on the
  unmodified `a7a5c08` build: 218/1,000 splices received `validation=decoded`,
  rate 0.2180, Wilson 95% [0.1935, 0.2446] (`docs/baseline/jpeg_splice_v0.2.0.txt`).
  After the partial H-18 fix the same protocol gave 1/1,000, rate 0.0010,
  Wilson 95% [0.0002, 0.0056]. The final 4,000-pair run gave 5/4,000, rate
  0.0013, Wilson 95% [0.0005, 0.0029]. D-45 closes the finding by disclosing
  the residual accepted cases rather than adding an unreliable detector; the
  reviewer's 21.8% conflict is resolved, but the 0.1% boundary is not met at
  the upper CI.
- **C2 (PDF `%%EOF` cost).** Current source build, `--threads 1`, canonical
  revision-like fixture at 61,550,637 B: E=1 0.005 s, E=4,096 0.331 s,
  `%%EOX` control 0.139 s (median of 3). Reviewer figures were 18.05-19.2 s;
  v4's milder fixture was always fast. H-07 is closed; outputs unchanged.
- **C5 (parallel carve).** 4 MiB JPEG-header flood, median of 3: glibc source
  build 1 thread 0.542 s vs default 0.364 s; packaged 0.2.0 musl binary
  1 thread 0.764 s vs default 2.491 s (3.26x slower). This confirms the v2
  `dist/`-only hypothesis; allocator changes are out of scope without
  STOP-AND-ASK. H-15 remains measured/musl-only, not tuned.
- **C13 (function length / README drift).** The reviewer's heuristic counts
  (for example `cli::scan` ≈270) and the tool check at threshold 120 agreed that
  long functions existed; the observed pre-fix tool inventory was larger
  (`cli::scan` 360, 15 non-test functions over the limit). `README.md` had the
  speed and Pillow "verified" claims and lacked the two flag rows; all four
  items are recorded in `docs/baseline/h33_static_red.txt`, with already-fixed
  items marked NOT-REPRODUCED. H-33 and H-34 are closed; no frozen figure changed.
- **Repro baseline.** `docs/baseline/harness_repro_v0.2.0.txt` preserves the
  reviewer-reported v0.2.0 values and the locally executed 0.3.0-dev red
  run: 34 scenarios, 0 failures, 3 SKIP, 0 XPASS; xfail file
  `X05,X14,X19,X22,X15,X20,X21`.

## Blockers / open questions (current)

- Current-HEAD CI run `45312234437` at `bae1033` (2026-11-23) completed **success**
  (27/27 jobs), archived in `bench/results/ci_run_45312234437*.json`; checksums
  in `bench/results/EVIDENCE.sha256`. Job logs unavailable via public API (403);
  status/conclusions/annotations only. Prior green reference: `45121233267` at
  `86ae15f` (27/27).
- R2-03/H-18: content-continuity check added, but eval splice upper bound
  0.182% is above the 0.1% target (D-67). **Accepted PARTIAL for 0.3.1 release
  by user (2026-11-23, option A)** with measured evidence recorded honestly;
  aggressive continuity heuristic authorized as measured follow-up prototype
  (not a default change without FP/upper-bound evidence).
- R2-08: X19/X20/R7/R8 re-run with available real tools/images PASS
  (2026-11-23). Remaining R2-08 items SKIPPED with reasons: X16(a)/X17
  (ewfacquire/ewfexport/qemu-img missing), qpdf X11 controls (qpdf missing),
  13 fuzz targets x60s real-container seeds not re-run (cargo-fuzz present,
  opportunistic), coverage ratchet not re-measured (cargo-llvm-cov present,
  opportunistic).
- 0.3.1 dist binaries rebuilt from `d5be769` and `DIST OK`; shipped Linux is
  glibc-linked (min GLIBC 2.34) because musl failed X23/R5. Green CI
  `45312234437` (`bae1033`, 27/27) recorded; `v0.3.1` tag pushed to origin. The
  0.3.1 Windows binary was executed under Wine, not a Windows runner.
- Post-R-01/R-02 coverage 69.72% (4456/6391); CI/local floor 60%. No 80% claim.
  Coverage has not been re-measured after H-14/H-16/H-17/H-18/H-19/H-21/H-22/H-25/H-26/H-27/H-29/R2 work.
- Scientific benchmarks C-02, C-09 incomplete; C-06/C-07 partial; D2–D7 remain
  OPEN/PARTIAL. README conclusions apply only to D1.

## Harness baseline (2026-11-21)

- Harness: `harness.md` copied to repo root (745 lines, `HARNESS-EOF` verified).
- `docs/guide.md`: supplied by the user in round 2 and committed verbatim
  (924 lines, sha256 `22ee8122209e9eda11d6f32a1e650e8f8e48f5610bd808309d94f436e5e78265`);
  H-01 is closed. This bullet is historical context for the 2026-11-21 baseline.
- Baseline: `a7a5c08` (clean tree) = `706a12a` + R-01/R-02 commit.
- P0.1 environment (2026-11-21): cargo/rustc 1.100.0; rustup auto-installed
  toolchain from `rust-toolchain.toml`; tools present: mkfs.vfat, mkntfs, mcopy,
  mdel, ntfs-3g, foremost, scalpel, photorec, cargo-fuzz, cargo-deny,
  cargo-audit, cargo-llvm-cov, python3/Pillow/pypdf/numpy; missing:
  ewfacquire, ewfverify, qemu-img, qpdf, pdftoppm.
- P0.2/P0.9 started: harness placed in root; gate header updated;
  `scripts/check-msrv.sh`, `scripts/check_commit_ids.sh` added;
  `review_repro.py` now treats SKIP as non-failure unless `--require-all`;
  `rust-toolchain.toml` added; `build.rs` emits `CARVEX_GIT_COMMIT`; Cargo
  version set to `0.3.0-dev`; repository URL fixed; `dist/README.md` was
  marked stale then, and the 0.3.0 release rebuild above supersedes it.

## Open xfail tests / scenarios

- None currently recorded. R2-03 eval upper-bound target is not met; tracked as
  PARTIAL H-18/D-67 rather than xfail.

## New findings outside this harness (N-xx + evidence + proposed priority)

| ID | Pri | Evidence | Status | Proposed fix |
|---|---|---|---|---|
| N-35 | HIGH | Run `45102520898`: all 13 fuzz jobs failed at `Fuzz smoke`. Local `cargo fuzz run jpeg` on the pinned `1.100.0` toolchain exited 1 with `error: the option 'Z' is only accepted on the nightly compiler`; `cargo +nightly fuzz run jpeg -- -max_total_time=2 -rss_limit_mb=4096` exited 0. | CLOSED (CI green) | `cargo +nightly fuzz run` in CI (D-60); all 13 fuzz jobs are green in observed run `45121233267`. |
| N-36 | HIGH | musl smoke failed at the release build on CI. Public `actions/runner-images` Ubuntu 24.04 package list contains `gcc`/`binutils` but not `musl-tools`; the local musl build succeeded only because `musl-gcc` is installed here. | CLOSED (CI green) | Install `musl-tools` in the CI job (D-60); Linux musl packaged smoke is green in `45121233267`. |
| N-37 | HIGH | `.gitignore:6` ignores `bench/images/*.img`. A clean checkout has no `mixed_fat32.img`; `carvex info` on that path exits 1. Both packaged smokes and the repro job referenced those ignored files. | CLOSED (CI green) | Generate the no-root FAT32 smoke fixture via `scripts/make_smoke_image.py` (D-60); both packaged smokes and the repro job are green in `45121233267`. |
| N-38 | MED | `release gate (G0-G9)` and `review repro assertions` failed remotely through `45118790273`. Failure-only annotations exposed `FAIL X05 ... time: exit 0, expected 3`; the 256 MiB fixture in `700221f` was not used remotely because `rust-cache` restored a stale 64 MiB fixture under `target/repro-ci`. | CLOSED (CI green) | Narrow annotations (`80155cd`, `7ba1557`), grow/stream the X05 fixture (`700221f`), and move the repro workdir to `$RUNNER_TEMP` (`86ae15f`, D-62). Both jobs are green in `45121233267`. |
| N-39 | MED | `test (windows-latest)` failed remotely at `Tests`. Converting the four NUL-free PDF fixtures to CRLF locally reproduced exactly four `pdf_structure_validation` failures; `*.pdf -text` restored all eight. | CLOSED (CI green) | Add `.gitattributes` with `*.pdf -text` (`53ca337`, D-61); Windows `Tests` is green in `45121233267`. |
| N-40 | MED | Run `45110520472` exposed `error[E0463]: can't find crate for 'core'` in the musl build because `rust-toolchain.toml` pins `1.100.0`, so the stable action target was not the active toolchain. | CLOSED (CI green) | Run `rustup target add x86_64-unknown-linux-musl` for the pinned toolchain (`7c107bb`, D-61); musl smoke is green in `45121233267`. |

## Decisions → docs/DECISIONS.md

- D-33 supersedes D-01 for active remediation.

## Decisions

See `docs/DECISIONS.md` (D-01…D-62).
