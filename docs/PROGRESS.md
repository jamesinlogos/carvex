# Implementation progress (CarveX 0.2.0)

> Last updated: 2026-11-22. Every status below is verified by the command
> recorded with it; items that could not be run due to missing tools are marked
> `SKIPPED`, with no guessed results. IDs follow the consolidated guide §6
> (A-01…A-15, B-01…B-11, C-01…C-09, D-01…D-03); old findings F-01…F-21 are
> cross-referenced in `docs/REVIEW_RESPONSE.md`.

## Environment

| Component | Value |
|---|---|
| Date | 2026-11-20 |
| OS | Linux x86_64 (Kali), running in a container |
| rustc / cargo | 1.100.0 (0ef1bc386 2026-11-09) / 1.100.0 |
| MSRV | 1.85.0, installed via rustup; `cargo +1.85 check --locked` passes |
| Python | 3.13.12; Pillow available (used to generate JPEG/PNG fixtures and check images) |
| Audit tools | `cargo-audit-audit 0.23.0`, `cargo-deny 0.21.0` (installed locally with `cargo install --locked`) |
| Benchmark tools | foremost, scalpel, photorec, mkfs.vfat, mkfs.exfat, mkntfs, mcopy, ntfs-3g, blkls, fls, istat, tsk_recover, parted, losetup, mount, mtools, 7z |
| Validation tools | `actionlint 1.7.12`, `cargo-fuzz 0.13.2` with `nightly 1.102.0-nightly`, `cargo-llvm-cov 0.9.1` (all installed locally during this validation pass) |
| Missing | `qpdf`, `pdftoppm`, `ewfacquire`, `ewfexport`, wine / a real Windows runner |

### Gate at time of writing — **GATE GREEN (2026-11-20)**

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

Current gate after H-16 (2026-11-22): **GATE GREEN**, 105 tests
(39 unit + 13 cli_contract + 5 evidence_formats + 2 fragment_recovery +
8 fs_detect + 2 jpeg_frame_budget + 4 memory_budget + 3 panic_containment +
4 png_decode_budget + 2 png_validator_budget + 5 properties +
3 regress_e01_safety + 4 regress_vmdk_safety + 6 scan_integration +
1 validity_model + 4 work_budgets), 34 repro scenarios with 0 failures/2 SKIPs/0 XPASS,
G7 and benchmark smoke green. `scripts/perf_gate.py --carvex
target/release/carvex` reported **PERF GREEN**: s9_4m median 0.140 s
(limit 3.0 s), rand64m median 0.032 s.

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
skipped). The distributed Windows binary still has not been executed as
`dist/carvex-windows-x86_64.exe`; GitHub's `test (windows-latest)` job ran the
Rust test suite on Windows.

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
| A-03 | DONE | green | `#![forbid(unsafe_code)]`; deny `indexing_slicing`/`unwrap_used`/`expect_used` for `fs` + `signatures` (test modules have allow with justification); `catch_unwind` around `fs::detect`; release unwind + `overflow-checks=true`; R7 PASS. 7 fuzz targets run 60 s each on nightly (18,520,025 executions, no crash/timeout/OOM, no artifacts). |
| A-04 | DONE | green | `--validate structure` (default) `\| decode`; `validation` + `validated_bytes`; R4c PASS (stitched JPEG → `partial`). On `all-deleted_fat32.img`: structure 0.11–0.20 s; decode 0.49–1.10 s; 30/30 files `decoded`. R-02 bounds PNG validation at IHDR (`--max-decode-bytes`, 256 MiB default) and streams IDAT/filter checks without full IDAT, decoded-image or row-offset buffers; four `png_decode_budget` tests. JPEG/PDF decoder internals remain best-effort. |
| A-05 | DONE | green | R6 PASS: zero-tail trim + `--max-truncated-size 16M`; JPEG/PNG prefix salvage (R4a/R4b) with `status=truncated` + `validated_bytes`. |
| A-06 | DONE | green | R5 0.48–0.79 s for 25,811 fake headers/64 MiB (target ≤1 s). Parallel carving via rayon, deterministic results (1 vs 4 thread test). R-01 fixed: `--max-memory` is a hard budget enforced before buffer allocation/growth and before retaining carved or stitched results; `--max-candidates`/`--time-budget` bound dense or long runs. Best-effort decode-temp reservation is applied, but third-party decoder internals are not fully accounted. |
| A-07 | DONE (steps 1 + 2) | green | `detect_all()` → `VolumeSet{volumes,partitions,unsupported,errors,gaps}`; report schema 2 adds `volumes[]`, `regions[]`, `warnings[]` object; `*_NOT_SCANNED` ⇒ exit 3. **Step 2 approved (D-23):** default `--partition all` + gaps, S1 carves 60 files (30+30, `part1/`/`part2/`) exit 0; `--partition largest --no-gaps` restores 30 files + WARNING + exit 3. `--include-gaps` is still accepted. **Baselines:** red `docs/baseline/A-07_v0.1.0.txt`; step 1 green `docs/baseline/A-07_green_v0.2.0.txt`; step 2 green `docs/baseline/A-07_green_v0.2.0_step2.txt`. Fixture `target/a07/s1.img`. |
| A-08 | PARTIAL | green | `trait ImageSource {len, read_at}` + unchanged `RawFile` default; `seek(End)` fallback when metadata len = 0. B-04 adds split raw, E01, VHD, VMDK and stdin sources under `src/image/`. `--on-read-error` and report `unreadable_regions[]` are now present (H-22); no `BlockDevice`/`--sector-size` yet and real device/Windows not tested. |
| A-09 | PARTIAL | green | Fixed the NTFS sector off-by-one (`field+1`, D-21, unit test `volume_sector_count_includes_the_backup_boot_sector`); FS errors → `FS_DETECTION_FAILED`. No `$ATTRIBUTE_LIST`/multi-extent `$MFT`/`$MFTMirr`/backup boot/FAT #2 yet. |
| A-10 | DONE | green | Lib crate + thin `main.rs`; pipeline `plan_extents → build_tasks → scan_task → carve_batch → postfilter → write_outputs`; format registry; `ScanInput<'a>` groups the input parameters for `run_scan`, with targeted `too_many_arguments` allows only on the two R-01/R-02 helpers that take explicit budget/deadline state; `cargo doc -D warnings` passes. |
| A-11 | PARTIAL | green | **105 tests** in the current gate (39 unit + 13 cli_contract + 5 evidence_formats + 2 fragment_recovery + 8 fs_detect + 2 jpeg_frame_budget + 4 memory_budget + 3 panic_containment + 4 png_decode_budget + 2 png_validator_budget + 5 properties + 3 regress_e01_safety + 4 regress_vmdk_safety + 6 scan_integration + 1 validity_model + 4 work_budgets); `scripts/review_repro.py --assert/--xfail --carvex` 34 scenarios, 0 failures, 2 SKIPs, 0 XPASS; red baseline `docs/baseline/repro_v0.1.0.txt`. Post-R-01/R-02 `cargo-llvm-cov 0.9.1` measured 4456/6391 lines = **69.72%**; CI/local floor set to 60%, not the earlier aspirational 80%. No new coverage run after H-14/H-16. |
| A-12 | PARTIAL | green | MSRV 1.85 (`rust-version`, README corrected from "Rust 1.74"); `cargo +1.85 check` in the gate; `.github/workflows/ci.yml` adds actionlint + coverage and uses a pinned direct actionlint install; `rustfmt.toml`/`clippy.toml`/`deny.toml`; dist 0.2.0 + `SHA256SUMS`. Local `cargo deny check`, `cargo audit` and `actionlint 1.7.12` pass. Real GitHub run `44548142275` on commit `42376a9` succeeded (all jobs; push-event fuzz matrix skipped), but it predates B-04, B-06 and R-01/R-02. Distributed Windows binary not executed; no SBOM/signatures yet; removing binaries from `dist/` is STOP-AND-ASK (kept). |
| A-13 | PARTIAL | green | Exit 0/1/2/3/4 documented + CLI contract tests; coded warnings; `RAW_SCANS_ALLOCATED` informational (D-20). No `thiserror`/leveled logger/`ProgressSink`/partial report on interrupt yet. |
| A-14 | DONE | green | Report schema v2: `schema_version`, `argv`, `options`, `tool_info`, `input` (before–after SHA-256), ISO-8601, `case_id`, `examiner`, `warnings[]` object, `volumes[]`, `regions[]`, `lba`, real `cluster`, `partition_index`, `extent_id`, `allocated`, `source`, `fragments`. `docs/report.schema.json` + key-matching test; atomic write; `--manifest` generates `SHA256SUMS`; `--report-schema 1`. `unreadable_regions[]` belongs to A-08 (not present). |
| A-15 | PARTIAL | green | Hard budget checks for `--max-memory` before allocation/retention plus caps on `--max-size`/`--max-truncated-size`; atomic write; `--force`. No rejection of output inside the source image yet, no `--overwrite`/free-space check/`--max-output`/symlink ban. |
| B-01 | OPEN | - | Does not read `0xE5` dir entries/MFT not-in-use, no `--mode metadata\|hybrid`. |
| B-02 | PARTIAL | green | MBR + GPT primary/backup; `--partition largest\|all\|N`; `info --json` lists `volumes[]`. No EBR chain yet; still assumes 512-byte sectors for the partition table. |
| B-03 | OPEN | - | No exFAT/FAT12/FAT16 support. |
| B-04 | MITIGATED | green | `src/image/` supplies read-only split raw, classic E01 (compressed/uncompressed), fixed/dynamic VHD, flat/sparse VMDK and stdin sources; `open_image()`/`--input-format` select them. Five `evidence_formats` fixture tests verify byte-for-byte reads and stdin report identity (D-29). The raw default, frozen benchmark, figures, lockbox results and dist binaries are unchanged. Recognized EWF2/Ex01, L01/Lx01, LVF, differencing VHD, ESX/COWD and compressed streamOptimized VMDK variants are rejected explicitly; encrypted E01 has no decryption and is not guaranteed to be detected before parsing. |
| B-05 | DONE | green | `--align none\|auto\|cluster\|sector`; default `none` keeps the old behavior; test `alignment_restricts_candidate_starts_without_changing_results_on_aligned_data`; golden TP unchanged. No per-file `aligned` flag recorded, no measurement of candidate reduction (D5). |
| B-06 | MITIGATED | green | Opt-in `--fragment-recovery` adds bounded two-fragment gap carving at cluster boundaries (`--max-gap`, `--max-frag-candidates`, `--frag-time-limit`). Accepted stitches pass the deep validator and record `source="bgc"`, `fragments[]`, `checks` and `confidence`; S12-style JPEG/PNG/PDF fixture recovers both files byte-exactly. Default raw/benchmark path untouched | No D1 recall/splice-rate measurement, no n-fragment path, no metadata hybrid, and no claim that published figures include B-06. |
| B-07 | PARTIAL | green | JPEG/PNG prefix salvage; `status=truncated` + `validated_bytes`; no `--salvage` flag yet; `repaired` does not yet require the validator to reopen the file. |
| B-08 | PARTIAL | green | `--manifest`, `--hash-image`, `--case-id`/`--examiner`, full exit codes. No carve-phase progress/`--resume`/`--dry-run`/`verify`/`check`/`extract`/`--json-lines`/completions/man. |
| B-09 | OPEN | - | Per-file `notes[]` present; no fixture + test per variant (APNG, xref stream, `/Encrypt`…); no format support table. |
| B-10 | OPEN | - | No custom signatures/`--signatures`; the registry (A-10) is the ready foundation. |
| B-11 | OPEN | - | No `posix_fadvise`/`--io-threads`. |
| C-01 | DONE (scoped) | n/a | `docs/BENCHMARK_PROTOCOL.md` frozen at tag `protocol-v1` (SHA-256 `d38607b86850312ec9906acd0dd732f965ca10cd5e78ff567a7076360b8008b6`); pre-execution builder correction tagged `protocol-v1.1`; 20-seed lockbox generated and run once into `bench/results/lockbox-v1` with dataset/result `SHA256SUMS` verified. Execution evidence: `docs/VALIDATION.md §6`. Scope: D1-style profile only, D2–D7 remain OPEN. |
| C-02 | OPEN | - | `bench/lockbox.py` supports `--seeds/--profile` for the 20-seed D1 lockbox; the dev corpus remains synthetic D1 with 30 files. No D2–D7 profiles. |
| C-03 | PARTIAL | - | Ground truth from a Pillow manifest, independent of CarveX; runner checks SHA-256 before each run; metrics do not use CarveX's validator (ADR-007). `qpdf`/`pdftoppm` missing → PDF "openable" check SKIPPED. |
| C-04 | PARTIAL | - | Exact + usable file-level, one-to-one metric, FP classification kept/unknown. **F-17 verified by re-running:** Foremost 40/44 JPEG-in-PDF + 4 truncated PDFs; Scalpel 40/54 + 4 + 10 child spans (8 EXIF thumbnails + 2 `FFD8FFD9` fragments) — matches "40 of 44 / 40 of 54". No block-level `byte_recall`/`splice_rate`; classification not automated in the runner. |
| C-05 | PARTIAL | - | Wilson 95% CI in the REPORT abstract (100/100 → [96.3%; 100%], 30/30 → [88.6%; 100%]) with n/conditions; runner warns `metric_variance`. No McNemar/bootstrap; no CI in every table. |
| C-06 | PARTIAL | - | Competitor configurations stored in `bench/`; search-space confound documented (README, REPORT §6.4 item 9). No same-search-space baseline; no ablation. |
| C-07 | PARTIAL | - | Runner records host/versions/argv/seed, relative to repo root; `--smoke`. No `render_docs.py` yet (figures are still hand-written, with `results.json` as source); no `bench/config.toml`/`--full`. |
| C-08 | PARTIAL | - | CFReDS RM#2 was run (0.1.0) and its real results recorded in `docs/CFREDS.md §5` + binary hashes; DFRWS 2006/2007 not run; 0.2.0 not re-run on CFReDS (documented). |
| C-09 | PARTIAL | - | Median time/throughput table present; full 4-image/3-rep 0.2.0 re-run completed in `bench/results/full-0.2.0` with all checksums verified. No 1–16 GiB scale images, no cold cache + IQR ≥10 repetitions. |
| D-01 | DONE | n/a | Removed absolute paths like `/home/…` (REPORT/CFREDS/README → `$CARVEX`/`$HOME`); fixed "Pillow/ReportLab"; "100%" in the abstract tied to n/CI/conditions; 16 tables numbered `Table 1…16`; SHA-256 of tools (foremost/scalpel/photorec) + explicit note that benchmark binary hashes were not recorded; §6.2 explains the evidenced FP 40/44 and 40/54; README "Rust 1.74" → 1.85; "fairness" conclusion limited; F-16 confound in §6.4; Windows `.exe` noted as untested. |
| D-02 | DONE | n/a | REPORT §1.4 Related Work (5 branches + comparison table); references [11]–[16] had their DOI/publisher pages opened before insertion. |
| D-03 | DONE | n/a | `CHANGELOG.md` (0.2.0), `SECURITY.md`, `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md` (D-01…D-32), `docs/DEPENDENCIES.md`, `docs/REVIEW_RESPONSE.md` (38 IDs), `docs/REPORT.md`, `docs/VALIDATION.md` (execution evidence only). |
| D-24 | DONE | n/a | All docs, comments and reports are English-only; the technical report is now `docs/REPORT.md`; every reference and table caption updated; no CLI/JSON/CSV/benchmark values changed. |
| P5-1 | DONE (scoped) | green | Post-B-04 gate GREEN 2026-11-19 (70 tests); post-B-06 gate GREEN (73 tests, coverage 4111/6001 = 68.51%); post-R-01/R-02 gate GREEN 2026-11-20 (80 tests, coverage 4456/6391 = 69.72%); CHANGELOG 0.2.0 + Unreleased B-04/B-06/R-01/R-02 entries; dist rebuilt for 0.2.0; fuzz runtime complete (18,520,025 executions); post-B-04 coverage 67.50% with 60% floor; GitHub CI green on pre-B-04/B-06 CI-fix commit `42376a9`; lockbox run once; full 4-image/3-rep benchmark re-run. Evidence: `docs/VALIDATION.md`. Scope items remain open as listed below. |
| P5-2 | DONE | n/a | `docs/REVIEW_RESPONSE.md` covers 38 IDs (A-01…D-03) with FIXED/MITIGATED/OPEN and cross-references F-01…F-21. |

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
| H-05 | CRIT | 2026-11-21 source / 2026-11-21 release build | CLOSED | 873ea35 | tests/png_validator_budget.rs; scripts/review_repro.py X02 | R-02 already bounded inflation; the missing shared flag was reproduced red. X02a: 0.009 s / 8.8 MiB RSS, `partial`; X02b rejected at IHDR. |
| H-07 | HIGH | 2026-11-21 source | CLOSED | fa18436 | scripts/review_repro.py X04 | 23-26s -> <1s after single-pass EOF indexing; A-01/R1-R8 and G7 unchanged. |
| H-11 | CRIT | 2026-11-21 source / 2026-11-21 release build | CLOSED | 4b86f2d | tests/regress_e01_safety.rs::hostile_chunk_size_is_rejected; X07 | Before: SIGABRT on a 64 GiB declared chunk (v5). After: typed error `EWF chunk size 68719476736 exceeds the 67108864-byte limit`; `info` 0.005 s / 12 MiB RSS, `scan` 0.005 s / 13 MiB RSS. |
| H-16 | CRIT | 2026-11-22 source | CLOSED | 3f47e51 | tests/validity_model.rs; tests/cli_contract.rs; scripts/review_repro.py X10–X14/X24 | Red test proved `validity` was absent and default `complete/structure/100` rows had no "content not checked" text. Added authoritative `validity` + `checks[]`, validity-aware summary/console/CSV/manifest, and schema conformance; scores and emission unchanged. |
| H-17 | HIGH | 2026-11-21 source | CLOSED | b2a6547 | scripts/review_repro.py X10 | Deep PNG validation now reports CRC mismatches as Validation::Partial; file still emitted. |
| H-20 | MED | 2026-11-21 source | CLOSED | cee916a | scripts/review_repro.py X13 | Trim trailing CR/LF/whitespace/NUL before %%EOF check; EOL variants now identical. |
| H-24 | MED | 2026-11-21 source | CLOSED | 56ee94a | scripts/review_repro.py X18 | Auto-detect now validates VHD footer checksum and requires strong VMDK descriptor markers before classifying. |

| P0.7 | - | 2026-11-21 source | DONE | - | scripts/perf_gate.py | Perf baseline recorded (s9_4m median ~0.39s, rand64m ~0.03s); thresholds.toml committed; x04 marked xfail in thresholds. |

| H-06 | CRIT | 2026-11-21 source / 2026-11-21 release build | CLOSED | 3534717 | tests/jpeg_frame_budget.rs; scripts/review_repro.py X03 | Red test proved the decoder ran without a budget check. After: 20,000² and 65,535² SOF claims → `unverified`, 0.010 s / 8.9 MiB and 0.009 s / 9.0 MiB RSS. |
| H-08 | CRIT | 2026-11-21 source / 2026-11-21 release build | CLOSED | e69e68b | tests/memory_budget.rs; scripts/review_repro.py X01 | Red test: 300 x ~60 KiB JPEGs (18 MiB output) with `--max-memory 16M` failed before the fix. After: all 300 written with bounded RSS; `SOURCE_CHANGED` verified by a changing-source test; G8 s9_4m 0.476s before vs 0.164s after. |
| H-09 | HIGH | 2026-11-21 source / 2026-11-21 release build | CLOSED | 48deab5 | tests/work_budgets.rs; scripts/review_repro.py X05 | Red X05 (missing flags) before the fix; after: all four limits trip with coded warning, report and exit 3; time case sets `nondeterministic_stop`. D-39 defaults measured on D0/S9/S10. |
| H-10 | HIGH | 2026-11-21 source / 2026-11-21 release build | CLOSED | 5f794a4 | tests/panic_containment.rs; scripts/review_repro.py X06 | Red test proved an escaping rayon panic before the fix. After: candidate/join panics become `internal_errors[]`, `CANDIDATE_PANIC`, counted in stats, exit 3; other candidates still carved. Open panics become typed errors; panic=unwind profile test added. |
| H-14 | HIGH | 2026-11-22 source / nightly fuzz runtime | CLOSED | e2d3f1d | fuzz/fuzz_targets/fuzz_*.rs; src/image/fuzz_support.rs tests; scripts/fuzz.sh | Static gap: only 7 targets, none for the new parsers. Added 6 named targets, 22 committed seeds and structured property tests; each target ran 60 s clean (235,272 executions total, no crash/artifact). Real-tool E01/VMDK fixtures SKIPPED (`ewfacquire`/`ewfexport`/`qemu-img` missing); all 13 targets are listed in the CI fuzz matrix. |
| H-15 | MED | 2026-11-22 source / 0.2.0 `dist/` | CLOSED (measured; musl/dist-only) | 1c28e2e | scripts/review_repro.py X23; direct X23 timing | Current glibc source: 1 thread median 0.489 s, default 0.156 s (0.32x). Stale 0.2.0 musl dist: 1 thread median 0.694 s, default 2.436 s (3.51x). Only the stale packaged release is slow; buffer reuse already landed in H-08. No allocator change without STOP-AND-ASK; no tuning or dist rebuild in this finding. |
| H-18 | HIGH | 2026-11-21 source | IN_PROGRESS | 3724a06 | scripts/review_repro.py X12; scripts/jpeg_splice_property.py | Baseline splice rate 0.2180 [0.1935,0.2446] at a7a5c08; current rate 0.0010 [0.0002,0.0056]. Strict-scan unsupported variants are Unverified; step 2 is still needed because the current upper CI exceeds 0.1%. |
| H-19 | HIGH | 2026-11-21 source | CLOSED (stage 1) | - | scripts/review_repro.py X11 | PDF decode no longer returns Decoded; returns Unverified with explicit lexical-only note (stage 1 honesty). S12/B-06 acceptance adjusted with E-1 note: PDF stitches accepted as lexical-validated Unverified, not Decoded. |
| H-21 | HIGH | 2026-11-21 source | REPRODUCED | - | scripts/review_repro.py X14 | Repaired PDF missing `sha256_original`. |
| H-22 | CRIT | 2026-11-21 in-memory fixture | IN_PROGRESS | 4b86f2d+97b2b04 | tests/regress_e01_safety.rs::{partial_final_chunk_is_tolerated, damaged_chunk_is_reported_and_outputs_are_not_verified}; X16 (real libewf SKIPPED) | Short final chunks are accepted; damaged chunks follow `--on-read-error`; `unreadable_regions[]`, `UNREADABLE_REGION` and overlap downgrade are reported. Remaining: `abort` must still write a partial report, and real libewf fixtures are unavailable; X16 is now present in the Python harness. |
| H-25 | LOW | 2026-11-21 source | REPRODUCED | - | scripts/review_repro.py X22 | stdin report has `unchanged=true` with null `modified_unix`; missing `logical_size`. |
| H-26 | MED | 2026-11-21 source | REPRODUCED | - | scripts/review_repro.py X19 | Corrupted NTFS in multi-volume disk: exit 0, no parse-failure warning. |
| H-23 | MED | 2026-11-21 source | CLOSED | 080b76a | tests/regress_vmdk_safety.rs | Missing grain table returned "index out of range"; now reads zeros like qemu-img. |

### Per-task reports

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
Residual risk / next: the stale 0.2.0 dist binary is not a valid 0.3.0
  performance artifact. The approved end-of-harness release rebuild must be
  re-timed before any claim about the 0.3.0 musl package; this finding does
  not claim that the future binary is fast.

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

Task: H-22 — E01 partial/damaged chunk handling
Reviews: v5:F-20 (ONE, real libewf)
Repro: REPRODUCED (in-memory) — short final chunks and damaged-chunk policy
  were demonstrated with generated EWF sections. Real `ewfacquire` fixtures
  are SKIPPED because the tool is not installed.
Change: `src/image/e01.rs` reads the logical final-chunk length, accepts both
  checksum variants and routes inflate/checksum failures to
  `--on-read-error`; `src/image/mod.rs` records unreadable ranges.
New tests: `tests/regress_e01_safety.rs::partial_final_chunk_is_tolerated`
  (red before / green after).
Gate: green (SKIPPED: real E01 tool scenarios).
Measurement: hard failure → short tail readable as zeros; damaged chunks no
  longer abort under the default policy.
Residual risk / next: `abort` still exits 1 without a partial report, the
  Python harness lacks X16, and real libewf fixtures are unavailable. The
  report now carries `unreadable_regions[]`, `UNREADABLE_REGION` and a
  non-verified label for overlapping outputs; `skip` drops such outputs.

### Conflict measurements (C1-C13)

- **C1 (JPEG splice rate).** `scripts/jpeg_splice_property.py` on the
  unmodified `a7a5c08` build: 218/1,000 splices received `validation=decoded`,
  rate 0.2180, Wilson 95% [0.1935, 0.2446] (`docs/baseline/jpeg_splice_v0.2.0.txt`).
  After the partial H-18 fix the same protocol gave 1/1,000, rate 0.0010,
  Wilson 95% [0.0002, 0.0056]. The point estimate meets the 0.1% boundary but
  the upper CI does not; H-18 step 2 remains open.
- **C2 (PDF `%%EOF` cost).** Current source build, `--threads 1`, canonical
  revision-like fixture at 61,550,637 B: E=1 0.005 s, E=4,096 0.331 s,
  `%%EOX` control 0.139 s (median of 3). Reviewer figures were 18.05-19.2 s;
  v4's milder fixture was always fast. H-07 is closed; outputs unchanged.
- **C5 (parallel carve).** 4 MiB JPEG-header flood, median of 3: glibc source
  build 1 thread 0.542 s vs default 0.364 s; packaged 0.2.0 musl binary
  1 thread 0.764 s vs default 2.491 s (3.26x slower). This confirms the v2
  `dist/`-only hypothesis; allocator changes are out of scope without
  STOP-AND-ASK. H-15 remains measured/musl-only, not tuned.
- **C13 (function length / README drift).** `clippy::too_many_lines` at
  threshold 120 reports 10 non-test functions over the limit (largest
  `cli::scan` 269, `scan::run_scan` 267, `signatures::png::analyze` 235).
  `README.md` still contains the speed claim at line 374 and the Pillow
  "verified" claim at line 418; `--max-memory` and `--max-truncated-size`
  are already in the flag table (lines 127-128). H-33/H-34 remain open.
- **Repro baseline.** `docs/baseline/harness_repro_v0.2.0.txt` preserves the
  reviewer-reported v0.2.0 values and the locally executed 0.3.0-dev red
  run: 34 scenarios, 0 failures, 3 SKIP, 0 XPASS; xfail file
  `X05,X14,X19,X22,X15,X20,X21`.

## Blockers / open questions

- GitHub Actions has now run successfully on the pre-B-04/B-06 CI-fix commit
  `42376a9` (`44548142275`); the two earlier failed runs were caused by the
  invalid actionlint action reference and an older queued pair is superseded.
  No post-B-04 or post-B-06 GitHub run is claimed. GitHub's fuzz matrix remains
  conditional and is skipped on push; local fuzz runtime evidence is in
  `docs/VALIDATION.md §4`.
- The packaged Windows binary has not been executed; the GitHub Windows job ran
  the Rust test suite, which is a different artifact.
- Post-R-01/R-02 coverage is 69.72% (4456/6391), below the earlier aspirational
  80%. The CI/local floor is honestly set to 60%; no claim of 80% coverage is
  made.
- Scientific benchmarks C-02, C-09 are incomplete; C-06/C-07 are partial; D2–D7
  remain OPEN/PARTIAL. All README conclusions apply only to D1 and are scoped.
- A-07 step 2 was approved by the user and implemented (D-23); STOP-AND-ASK
  §3.5 has no items awaiting confirmation.

## Harness baseline (2026-11-21)

- Harness: `harness.md` copied to repo root (745 lines, `HARNESS-EOF` verified).
- `docs/guide.md`: **MISSING** in repo; user asked to supply the 924-line file
  (sha256 `22ee8122209e9eda11d6f32a1e650e8f8e48f5610bd808309d94f436e5e78265`).
  Continuing with harness (self-contained) per H-01/§3.5-8.
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
  version set to `0.3.0-dev`; repository URL fixed; `dist/README.md` marked
  stale (binaries not replaced).

## Findings (H-xx)

| ID | Pri | Repro (date, build) | Status | Commit | Test(s) | Before → After (± CI) / evidence |
|----|-----|---------------------|--------|--------|---------|----------------------------------|
| H-01 | HIGH | 2026-11-21 source | IN_PROGRESS | - | - | harness placed; guide.md missing (user requested); D-33 added; gate header fixed |
| H-02 | HIGH | 2026-11-21 source | IN_PROGRESS | - | - | SKIP semantics added to repro; check-msrv.sh added; G7 golden script added; full clean-checkout gate not yet proven |
| H-03 | HIGH | 2026-11-21 source | IN_PROGRESS | 0473d8a+pending | scripts/review_repro.py X01-X24; tests/regress_* | Python harness has all X01-X24 scenarios; Rust stub builders remain incomplete. |
| H-04 | MED | 2026-11-21 source | IN_PROGRESS | - | - | URL/build.rs/version/dist notice done; dist rebuild deferred to release |

## Open xfail tests / scenarios


## Blockers / open questions

- `docs/guide.md` missing from repo; user must supply the 924-line file with the
  expected sha256 before H-01 can be closed (§3.5 item 8).
- Harness scope is large (Phases 0-4, 34 findings); this session begins Phase 0.

## New findings outside this harness (N-xx + evidence + proposed priority)

(none)

## Decisions → docs/DECISIONS.md

- D-33 supersedes D-01 for active remediation.

## Decisions

See `docs/DECISIONS.md` (D-01…D-33).
