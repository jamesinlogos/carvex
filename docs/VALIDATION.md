# Validation execution evidence (CarveX 0.2.0)

> Last updated: 2026-11-22. This file records commands that were actually run
> and the resulting artifacts. It is **execution evidence only**, not a second
> technical report: `docs/REPORT.md` remains the sole technical report, and the
> published 0.1.0 figures in `README.md`, `docs/REPORT.md` and
> `bench/results/final/` are unchanged.

## 1. Freeze and provenance

| Item | Value |
|---|---|
| Local gate commit | `cbee72d26634be041e48f9256198c1ff344df82f` |
| Frozen protocol tag | `protocol-v1` on `cbee72d` (created before any lockbox artifact was viewed) |
| Pre-execution builder correction | `protocol-v1.1` on `32d1714338ef281916ab25d201c9e3687d467bbb` |
| Final CI-fix commit | `42376a9d0a00adb0c0e812927a1d6871a94996ac` |
| `docs/BENCHMARK_PROTOCOL.md` SHA-256 | `d38607b86850312ec9906acd0dd732f965ca10cd5e78ff567a7076360b8008b6` |
| `bench/foremost.conf` SHA-256 | `a091c518d256b182ef0efa3320535f38102b14cdc7fc44903fc2966ab3df096d` |
| `bench/scalpel.conf` SHA-256 | `9c55a406f8911167c912b790c06fcb1c54247b59b32fecc4ef541c466df653e0` |
| Linux dist SHA-256 | `b9e6c5f4d9cd5dd6ed633acda799e1e1c0880717de5030a5fe61a8ce5f7858af` |
| Windows dist SHA-256 | `503553ea17cd15522ce694ecb5eee0a8daacc8d6fed9ff434fe48262ad17fc9b` |

The first lockbox generation attempt failed at seed 1001 because 19 of the
fixed seeds produced a `pdf_08_small_text.pdf` below the corpus builder's own
4096-byte NTFS non-residency guard (seed 1011 was the only pass). No lockbox
result had been viewed. With user approval, only that builder padding value was
changed from `padding=6` to `padding=10`, disclosed in `docs/PROGRESS.md`,
committed and tagged `protocol-v1.1` before the dataset was regenerated. No
carving code, scoring, runner command, tool config, protocol text or published
figure was changed. The CI workflow was later fixed separately to replace the
non-existent `rhysd/actionlint-action` reference with a pinned direct
actionlint install.

## 2. Local gate

Command: `scripts/gate.sh`

Result: **GATE GREEN**. The original recorded run in this section executed 80
tests (34 unit + 13 CLI contract + 5 evidence formats + 8 filesystem detection
+ 5 property + 6 scan integration + 2 fragment recovery + 3 memory budget +
4 PNG decode budget); the current H-14 re-run is recorded below. The gate runs
format check, clippy, tests, rustdoc with `-D warnings`, `actionlint`,
`cargo deny check`, `cargo audit`, MSRV 1.85 check, release build,
`scripts/review_repro.py` (10/10 PASS, 0 XPASS in the original run; 34 scenarios
with 0 failures, 2 SKIPs and 0 XPASS in the H-14 run) and the benchmark smoke
test (TP=3/FP=0 unallocated, TP=3/FP=1 raw). `cargo audit` reported 0
vulnerabilities in 91 crates against 1555 advisories; `cargo deny check`
reported advisories, bans, licenses and sources all OK.

The gate was re-run after the B-04 source additions, after B-06, and after
R-01/R-02. The five `tests/evidence_formats.rs` fixture tests verify
byte-for-byte reads for split raw, compressed and uncompressed E01, fixed and
dynamic VHD, flat and sparse VMDK, and stdin report identity. The two
`tests/fragment_recovery.rs` tests verify exact S12-style JPEG, PNG and PDF
recovery from interleaved fragments and rejection of invalid B-06 limits. The
three `tests/memory_budget.rs` tests cover early `--max-memory` failure,
candidate-count enforcement and a finite default candidate limit. The four
`tests/png_decode_budget.rs` tests cover extreme dimensions, an explicit small
decode budget, a 50,335,744-byte expansion with a peak-RSS assertion, and an
invalid filter byte. Smoke output remained metric-identical to the frozen 0.2.0
behavior.

The gate was re-run after H-14, H-16, H-17, H-18, H-19 stage 2, H-21, H-22,
H-25, H-26 and H-27 on 2026-11-22 and remained green.
The current test count is 131: 43 unit, 13 CLI contract, 7 evidence-format,
2 fragment recovery, 8 filesystem detection, 2 JPEG frame budget, 4 memory
budget, 3 panic containment, 2 PNG CRC validity, 4 PNG decode budget,
2 PNG validator budget, 8 PDF structure validation, 5 property, 4 E01 safety,
4 VMDK safety, 3 repair validation, 6 scan integration, 1 validity-model,
4 work-budget, 3 volume-parse-error and 3 region-coverage tests.
The H-16 red test
`tests/validity_model.rs::default_structure_rows_are_labeled_content_not_checked`
failed before the fix with `left: Null` for `validity`; the H-17 red test
`tests/png_crc_validity.rs::bad_png_crc_is_suspect_in_structure_and_decode_modes`
failed with `left: String("structure")`, `right: "partial"`. Both now pass.
H-18 adds two `src/signatures/jpeg.rs` unit tests that pin the pre-existing
MCU-count/exact-EOI checks; the 4,000-pair property run and its five residual
accepted splices are recorded in `docs/baseline/jpeg_splice_v0.2.0.txt` and
D-45. H-19 stage 2 added eight red-first tests in
`tests/pdf_structure_validation.rs`; the initial valid PDF and broken-xref/root
fixtures reported `unverified` before the structural validator was wired in.
All ten corpus PDFs now scan `verified`/100 with no suspect result. `qpdf` and
`pdftoppm` are unavailable, so the X11 external-oracle control matrix is
SKIPPED; `pypdf` 6.9.2 opened generated fixtures for H-19 and is the X14
non-strict oracle for a real corpus PDF cut. H-21 added three red-first tests
in `tests/repair_validation.rs`: before the fix all three failed on the missing
CSV `sha256_original` column, and the invalid-catalog case was also mislabelled
`repaired`. After the fix, valid PDF/JPEG/PNG repairs revalidate and emit the
source hash; an invalid PDF keeps its original bytes and reports
`repair attempted, not validated`. X14 now passes against
`pdf_03_text_2pages.pdf` cut at 70%: pypdf opens the rebuilt output, the xref,
root and repair checks pass, and `sha256_original` differs from `sha256`. X14
was removed from `scripts/repro_xfail.txt`. H-22 added the red-first test
`tests/regress_e01_safety.rs::abort_writes_partial_report_and_exits_one`: before
the fix the aborted scan exited 1 but wrote no report; after the fix it writes a
schema-2 report with `READ_ABORTED`, `UNREADABLE_REGION` and the one 4,096-byte
EWF chunk range. Two `src/image/mod.rs` unit tests use a failing mock
`ImageSource` to verify per-sector zero-fill and abort recording. X16 now checks
zero-fill, abort and skip on the same damaged synthetic E01. H-25 added two
red-first container-identity tests in `tests/evidence_formats.rs` plus the
updated stdin expectations; before the fix all three failed with `left: Null`
for `format` and the new `input` fields. The schema contract test now checks the
new `input` and `input_file` definitions. X22 builds a compressed E01 and a
monolithicSparse VMDK in addition to the stdin case and was removed from
`scripts/repro_xfail.txt`. H-26 added three red-first CLI tests in
`tests/volume_parse_errors.rs`: the multi-volume case exited 0 with no warning
(`left: Some(0)`, `right: Some(3)`), the single-volume fallback hid the NTFS
cause behind "no FAT32/NTFS filesystem found", and `info` gave the same generic
message. After the fix, X19 asserts exit 3, a `VOLUME_PARSE_FAILED` warning with
partition 2/offset/`$MFT`, a scanned `unsupported` region with
`may_contain_allocated: true`, and a single-volume `info` exit 1 with the
specific cause; X19 was removed from `scripts/repro_xfail.txt`. H-27 added three
red-first CLI tests in `tests/region_coverage.rs`. Before the fix they failed
with `region 0 starts at 1048576, expected 0`, an `info --json` free region
spanning the whole volume, and clap rejecting `--max-regions`. After the fix,
regions are disjoint, sorted and cover the image; free rows equal the
filesystem's real `free_extents`; and scanned lengths equal both
`bytes_scanned` and `stats.bytes_in_extents` in auto, allocated, raw,
largest/no-gaps and no-filesystem modes. X20 now asserts that contract plus a
raw-mode `--max-regions 1` run and was removed from `scripts/repro_xfail.txt`.
The repro harness ran 34 scenarios with 0 failures, 2 SKIPs and 0 XPASS; G7 and
the benchmark smoke were green. `scripts/perf_gate.py --carvex
target/release/carvex` reported **PERF GREEN**: s9_4m median 0.140 s and
rand64m median 0.039 s. No new coverage measurement was made for
H-14/H-16/H-17/H-18/H-19/H-21/H-22/H-25/H-26/H-27.

## 3. GitHub Actions

The first two runs on `cbee72d` failed only because `rhysd/actionlint-action@v1`
resolves to a non-existent repository (`repository not found`). After replacing
it with a direct install of actionlint 1.7.12, the CI-fix commit produced:

- Run ID: `44548142275`
- URL: <https://github.com/jamesinlogos/carvex/actions/runs/44548142275>
- Head SHA: `42376a9d0a00adb0c0e812927a1d6871a94996ac`
- Conclusion: **success**

Job conclusions in that run: `dependencies`, `actionlint`, `test
(ubuntu-latest)`, `test (windows-latest)`, `test (macos-latest)`, `MSRV`,
`coverage` and `benchmark smoke` all succeeded. The `fuzz` matrix job was
skipped because its trigger condition is `schedule` or `workflow_dispatch`, and
this was a push event. Two older queued runs on `32d1714` were still queued at
the final API check and are superseded by the successful run above. Local
fuzz runtime evidence is recorded in §4 because it is the actual executed fuzz
evidence.

These GitHub runs predate the B-04 source additions, B-06 and R-01/R-02. The
post-B-04, post-B-06 and post-R-01/R-02 evidence in §2 is the local gate only;
no corresponding GitHub run is claimed.

## 4. Fuzz runtime

### Historical seven-target run (before H-14)

Command: `scripts/fuzz.sh 60` (7 targets × 60 seconds, nightly
`1.102.0-nightly`, cargo-fuzz `0.13.2`).

| Target | Executions |
|---|---:|
| `fat32_boot` | 3,627,815 |
| `jpeg` | 2,126,329 |
| `ntfs_boot` | 4,004,758 |
| `ntfs_mft` | 1,960,355 |
| `partition_table` | 4,444,996 |
| `pdf` | 84,558 |
| `png` | 2,271,214 |
| **Total** | **18,520,025** |

No crash, timeout, OOM or panic was reported; `fuzz/artifacts/` remained
empty. Full logs are in `bench/results/fuzz/*.log`. These are historical
results and were not repeated after H-14.

### H-14 parser targets (2026-11-22)

Command: `scripts/fuzz.sh 60 fuzz_e01 fuzz_vhd fuzz_vmdk fuzz_split_names
fuzz_open_image fuzz_fragment_join` (6 targets × 60 seconds, nightly
`1.102.0-nightly`, cargo-fuzz `0.13.2`).

| Target | Executions |
|---|---:|
| `fuzz_e01` | 968 |
| `fuzz_vhd` | 79,089 |
| `fuzz_vmdk` | 48,460 |
| `fuzz_split_names` | 87,028 |
| `fuzz_open_image` | 6,249 |
| `fuzz_fragment_join` | 13,478 |
| **Total** | **235,272** |

Each target printed `Done ... in 61 second(s)` with no crash, timeout, OOM or
panic. No files were written under `fuzz/artifacts/`, so no minimised crash
fixture or crash regression test was required. Real-tool E01/VMDK container
seeds were SKIPPED because `ewfacquire`, `ewfexport` and `qemu-img` are not
installed. The expanded fuzzer corpora were reset to the 22 deterministic
committed seeds after the run. Logs are in `bench/results/fuzz/`; the CI
matrix lists all 13 targets, but no post-change GitHub run is claimed.

## 5. Coverage

Command: `scripts/coverage.sh 60` (cargo-llvm-cov `0.9.1`).

Result: **COVERAGE COMPLETE: minimum 60% lines**. After R-01/R-02 the lcov
report contains 4456/6391 covered lines = **69.72%**. The post-B-06
measurement was 4111/6001 lines = 68.51%, the post-B-04 measurement was
3802/5633 lines = 67.50%, and the pre-B-04 0.2.0 measurement was 2859/4508
lines = 63.42%; those figures are historical. All measurements are below the
earlier aspirational 80% target, so the CI and local floor is honestly set to
60%; no 80% coverage is claimed. Artifacts:
`bench/results/coverage/lcov.info` and
`bench/results/coverage/coverage.log`.

## 6. Lockbox-v1 (single use)

Dataset command: `python3 bench/lockbox.py` after `protocol-v1.1`.

- 20 seeds 1001–1020, balanced 5/5/5/5: all-deleted/FAT32, all-deleted/NTFS,
  mixed/FAT32, mixed/NTFS.
- 600 unique corpus files and 20 64 MiB images; strict hash dedupe against the
  dev corpus and earlier lockbox seeds.
- `bench/lockbox/SHA256SUMS` contains 1302 entries and has SHA-256
  `a5724038df4d76bdf1691dc429c86292f58f41d15b19c3ab6cdab1b66446f68f`.
- `bench/lockbox/lockbox.json` SHA-256:
  `3c98caaa693791a2198bb2dfdabf9797c5e25a6328785042b299ab176148b93c`.
- `sha256sum -c SHA256SUMS` in `bench/lockbox/` passed for every entry.

Execution command: `scripts/lockbox_run.sh`, run exactly once into
`bench/results/lockbox-v1`. It ran 100 tool/image combinations (20 images ×
5 tool/mode specs × 1 repetition) with the unchanged runner, configs and
carving binary. All 100 rows have `ok=true`, empty error strings and no
`metric_variance` warnings. The frozen dataset checksum file was verified
again and copied as `bench/results/lockbox-v1/DATASET_SHA256SUMS`; both copies
hash to `a5724038…46f68f`. The result `SHA256SUMS` hashes to
`9f8b4e5d475ccd38587f14abb127ef56b2d6b0c967eb1c07722487bb04f03734` and
`sha256sum -c SHA256SUMS` passed inside the result directory.

Pooled lockbox metrics (exact unless labelled usable):

| Tool / mode | Deleted | Kept | Outputs | TP | FP | FN | P | R | F1 | TP usable | FP usable | Runtime |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| CarveX unallocated | 500 | 100 | 500 | 500 | 0 | 0 | 100.0% | 100.0% | 100.0% | 500 | 0 | 3.524 s |
| CarveX raw | 500 | 100 | 600 | 500 | 100 | 0 | 83.3% | 100.0% | 90.9% | 500 | 100 | 2.173 s |
| Foremost | 500 | 100 | 780 | 390 | 390 | 110 | 50.0% | 78.0% | 60.9% | 470 | 310 | 7.660 s |
| Scalpel | 500 | 100 | 845 | 390 | 455 | 110 | 46.2% | 78.0% | 58.0% | 460 | 385 | 9.877 s |
| PhotoRec | 500 | 100 | 579 | 479 | 100 | 21 | 82.7% | 95.8% | 88.8% | 479 | 100 | 10.745 s |

By design group, CarveX unallocated recovered 150/150 deleted files with 0 FP
in each of the four groups (all-deleted/FAT32, all-deleted/NTFS,
mixed/FAT32, mixed/NTFS). CarveX raw also recovered every deleted file but
reported 50 FP in each mixed group and 0 FP in each all-deleted group. Wilson
95% CI for 500/500 exact true positives is [99.24%, 100%]. These numbers were
not used to change code, thresholds, configs or the runner.

## 7. Full 0.2.0 dev benchmark

Command: `scripts/benchmark_full.sh`, writing to the fresh directory
`bench/results/full-0.2.0`. It ran 4 images × 5 tool/mode specs × 3
repetitions = 60 runs, all successful. The result `SHA256SUMS` hashes to
`01b6881bc42b8927a7ac600d5f484b17f942007c9545c74328858beb1f15844b` and
`sha256sum -c SHA256SUMS` passed inside the result directory.

| Tool / mode | Deleted | Kept | Outputs | TP | FP | FN | P | R | F1 | Runtime |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| CarveX unallocated | 100 | 20 | 100 | 100 | 0 | 0 | 100.0% | 100.0% | 100.0% | 0.275 s |
| CarveX raw | 100 | 20 | 120 | 100 | 20 | 0 | 83.3% | 100.0% | 90.9% | 0.329 s |
| Foremost | 100 | 20 | 156 | 78 | 78 | 22 | 50.0% | 78.0% | 60.9% | 1.085 s |
| Scalpel | 100 | 20 | 166 | 78 | 88 | 22 | 47.0% | 78.0% | 58.6% | 1.895 s |
| PhotoRec | 100 | 20 | 114 | 94 | 20 | 6 | 82.5% | 94.0% | 87.9% | 1.537 s |

A row-by-row comparison of all 20 image/tool combinations against the frozen
0.1.0 `bench/results/final/results.json` found **zero differences** in
`outputs`, `tp_exact`, `fp_exact`, `fn_exact`, `tp_usable`, `fp_usable`,
`fn_usable`, `kept_files_recovered` and `unknown_fp`. The frozen directories
`bench/results/final`, `bench/results/baseline-v0.1` and
`bench/results/cfreds` were not modified. The full-run numbers are validation
evidence only; published figures remain those of 0.1.0.

## 8. What is still not claimed

- D2–D7 datasets remain OPEN; lockbox-v1 uses the D1-style synthetic profile.
- No D1 fragmented-profile recall or `splice_rate` was measured for B-06; the
  S12 fixtures are correctness tests only, and the published figures do not
  include B-06.
- Same-search-space baselines, validator/alignment/FS-aware ablations,
  block-level byte recall/splice rate and McNemar/bootstrap statistics remain
  OPEN.
- `qpdf` and `pdftoppm` were unavailable, so the protocol's `viewable` PDF
  metric was not computed.
- The distributed Windows binary was built and format-checked but not executed
  as `dist/carvex-windows-x86_64.exe`; the successful GitHub
  `test (windows-latest)` job ran the Rust test suite on Windows, not that
  packaged executable.
- GitHub's scheduled fuzz matrix remains conditional and was skipped on push;
  the executed local fuzz evidence is in §4.
- No lockbox result was used for tuning; if a real bug were found, a fresh seed
  set would be required rather than reusing this lockbox.
