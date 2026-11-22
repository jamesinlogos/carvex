# Implementation progress (CarveX 0.2.0)

> Last updated: 2026-11-20. Every status below is verified by the command
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
| A-08 | PARTIAL | green | `trait ImageSource {len, read_at}` + unchanged `RawFile` default; `seek(End)` fallback when metadata len = 0. B-04 adds split raw, E01, VHD, VMDK and stdin sources under `src/image/`. No `BlockDevice`/`--sector-size`/`--on-read-error`/`unreadable_regions[]` yet; real device/Windows not tested. |
| A-09 | PARTIAL | green | Fixed the NTFS sector off-by-one (`field+1`, D-21, unit test `volume_sector_count_includes_the_backup_boot_sector`); FS errors → `FS_DETECTION_FAILED`. No `$ATTRIBUTE_LIST`/multi-extent `$MFT`/`$MFTMirr`/backup boot/FAT #2 yet. |
| A-10 | DONE | green | Lib crate + thin `main.rs`; pipeline `plan_extents → build_tasks → scan_task → carve_batch → postfilter → write_outputs`; format registry; `ScanInput<'a>` groups the input parameters for `run_scan`, with targeted `too_many_arguments` allows only on the two R-01/R-02 helpers that take explicit budget/deadline state; `cargo doc -D warnings` passes. |
| A-11 | PARTIAL | green | **80 tests** (34 unit + 13 cli_contract + 5 evidence_formats + 8 fs_detect + 5 properties + 6 scan_integration + 2 fragment_recovery + 3 memory_budget + 4 png_decode_budget); `scripts/review_repro.py --assert/--xfail` 10/10; red baseline `docs/baseline/repro_v0.1.0.txt`. Post-R-01/R-02 `cargo-llvm-cov 0.9.1` measured 4456/6391 lines = **69.72%**; CI/local floor set to 60%, not the earlier aspirational 80%. |
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

| H-05 | CRIT | 2026-11-21 source | NOT-REPRODUCED (current) | - | scripts/review_repro.py X02 | X02 did not reproduce on 0.3.0-dev; likely mitigated by R-02 bounded PNG decode. Needs adversarial confirmation per harness recipe. |
| H-07 | HIGH | 2026-11-21 source | CLOSED | - | scripts/review_repro.py X04 | 23-26s -> <1s after single-pass EOF indexing; A-01/R1-R8 and G7 unchanged. |
| H-11 | CRIT | 2026-11-21 source | STATIC-ONLY | - | scripts/review_repro.py X07 | Crafted minimal E01 did not crash; fixture likely insufficient. SRC confirms unbounded chunk allocation risk. |
| H-12 | CRIT | 2026-11-21 source | STATIC-ONLY | - | scripts/review_repro.py X08 | Crafted VMDK completed quickly; RSS not measured. Needs proper RSS harness. |
| H-16 | CRIT | 2026-11-21 source | OPEN | - | scripts/review_repro.py X24 | X24 interleaved fixture not flagged decoded/complete; default structural-as-complete issue still present per SRC. Needs proper validity-model check. |
| H-17 | HIGH | 2026-11-21 source | CLOSED | - | scripts/review_repro.py X10 | Deep PNG validation now reports CRC mismatches as Validation::Partial; file still emitted. |
| H-20 | MED | 2026-11-21 source | CLOSED | - | scripts/review_repro.py X13 | Trim trailing CR/LF/whitespace/NUL before %%EOF check; EOL variants now identical. |
| H-24 | MED | 2026-11-21 source | CLOSED | - | scripts/review_repro.py X18 | Auto-detect now validates VHD footer checksum and requires strong VMDK descriptor markers before classifying. |

| P0.7 | - | 2026-11-21 source | DONE | - | scripts/perf_gate.py | Perf baseline recorded (s9_4m median ~0.39s, rand64m ~0.03s); thresholds.toml committed; x04 marked xfail in thresholds. |

| H-06 | CRIT | 2026-11-21 source | NOT-REPRODUCED (weak) | - | scripts/review_repro.py X03 | Oversized-SOF JPEG not reported decoded/complete and did not timeout; RSS/kill not measured. |
| H-08 | CRIT | 2026-11-21 source | NOT-REPRODUCED (weak) | - | scripts/review_repro.py X01 | Small repro did not show exit-1/no-report discard; needs RSS-bound and large-payload confirmation. |
| H-09 | HIGH | 2026-11-21 source | REPRODUCED (partial) | - | scripts/review_repro.py X05 | Missing `--max-work-per-candidate`, `--max-output`; `--max-candidates` and `--time-budget` already present. |
| H-13 | HIGH | 2026-11-21 source | NOT-REPRODUCED (weak) | - | scripts/review_repro.py X09 | Absolute-path VMDK descriptor did not carve outside file in simple test; needs symlink/sibling/rename cases. |
| H-14 | HIGH | - | OPEN | - | - | No new fuzz targets for image parsers yet. |
| H-15 | MED | 2026-11-21 source | NOT-REPRODUCED | - | scripts/review_repro.py X23 | Default threads not slower than 1 thread on 4M flood source build. |
| H-18 | HIGH | 2026-11-21 source | CLOSED (partial) | - | scripts/review_repro.py X12 | Unsupported JPEG strict-scan variants now return Validation::Unverified; splice property/rate not yet measured. |
| H-19 | HIGH | 2026-11-21 source | REPRODUCED | - | scripts/review_repro.py X11 | Broken-xref PDF reported decoded (lexical-only validation). |
| H-21 | HIGH | 2026-11-21 source | REPRODUCED | - | scripts/review_repro.py X14 | Repaired PDF missing `sha256_original`. |
| H-22 | CRIT | - | OPEN | - | - | E01 partial/damaged chunk handling not reproduced (ewfacquire missing). |
| H-25 | LOW | 2026-11-21 source | REPRODUCED | - | scripts/review_repro.py X22 | stdin report has `unchanged=true` with null `modified_unix`; missing `logical_size`. |
| H-26 | MED | 2026-11-21 source | REPRODUCED | - | scripts/review_repro.py X19 | Corrupted NTFS in multi-volume disk: exit 0, no parse-failure warning. |
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
| H-03 | HIGH | - | OPEN | - | - | xfail scaffolding started in review_repro.py; full Rust regress_* builders not yet added |
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
