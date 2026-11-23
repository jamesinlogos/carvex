# CarveX — Agent Harness (`guide.md`)

> **Unified guide**, merged and verified from `guide-v1.md`, `guide-v2.md` and `guide-v3.md`. It supersedes all three.
> Applies to: `carvex` **v0.1.0** (repository snapshot `carvex.zip`, which already contains the CFReDS RM#2 results of 2026-11-17).
> Audience: an AI coding agent (Claude Code, Codex, Cursor, …) working in the `carvex/` repository (Rust, ~3,600 lines, plus Python bench).
> Language: English. Commands, flags and identifiers are verbatim.
> **Place this file at the repository root and read §0–§5 completely before changing any line of code.**

---

## 0. How to use this document

1. Work in the **phase order of §7**. Do not start a phase until the gates of the previous phase are green (§4.2). Reason: later phases rely on the safety net and the measurements of earlier phases.
2. Every work item has an ID (`A-xx` reliability/correctness, `B-xx` coverage/usability, `C-xx` benchmark science, `D-xx` documentation) defined in §6. Every item ends with **acceptance criteria that can be checked by a command**. An item that has not passed its acceptance is not done.
3. Loop per item (§12.1): **reproduce → write a failing test → minimal fix → test green → gates → docs/CHANGELOG → small commit → update `docs/PROGRESS.md`.**
4. Priority tags: **[MUST]** required for the 0.2.0 release; **[SHOULD]** strongly recommended; **[MAY]** if time permits.
5. When this document and the repository disagree, **trust the repository**, record the difference in `docs/DECISIONS.md`, take the conservative option and continue. Stop and ask the user only in the cases listed in §3.5.
6. Every number that appears in README / report / BENCHMARK must be **generated from benchmark results** (§10.9), never typed by hand.
7. Evidence labels used throughout this document:

| Label | Meaning |
|---|---|
| **[REPRODUCED]** | Re-run during the merge on the shipped binary `dist/carvex-linux-x86_64` (SHA-256 matches `dist/SHA256SUMS`) or on the shipped data. Scenario specs and observed values are in Appendix A. |
| **[CONFIRMED-STATIC]** | Verified by reading the source / lockfile / crate manifests / result files in the repository snapshot. |
| **[REPORTED]** | Stated by one of the three original reviews; not independently re-run during the merge. Treat as a hypothesis until Phase 0 re-measures it. |
| **[UNMEASURED]** | Inferred from the code only; measure before fixing. |

---

## 1. Mission, context, goals

**Thesis topic:** *Nghiên cứu kỹ thuật File Carving dựa trên Header/Footer và xây dựng công cụ phục hồi tệp JPEG, PNG, PDF bị xóa trên hệ thống tệp FAT32 và NTFS.*

**Current state (v0.1.0):** Rust CLI (`info`, `scan`, `signatures`); parallel header search with `memchr` per chunk; structural parsers for JPEG/PNG/PDF; `unallocated` mode that scans only free clusters derived from the FAT / `$Bitmap`; JSON/CSV reports; `--repair`; benchmark of 4 images × 5 configurations × 3 repetitions; CFReDS RM#2 run (no per-file ground truth).

**Verdict of the three reviews (confirmed by the merge):** well above the usual student-project level, but **not yet enterprise-grade** (silent data loss in default mode, false "complete" labels, a parser panic that kills the process, unbounded cost on adversarial input, no CI/fuzz/lib split), and the **benchmark is not yet scientifically defensible** (n=30 unique files, dev=test, ideal conditions, non-equivalent baselines).

**Goals, in priority order:**
1. **Correct and honest.** Never label corrupt data as complete; never lose evidence silently.
2. **Reliable.** Survive hostile/corrupt input with bounded time and memory; a parser bug must not destroy a long run.
3. **Cover real use cases.** Multiple partitions, fragmentation, metadata-assisted recovery, more file systems and evidence formats.
4. **Usable.** Progress, resume, reports, forensic integrity (hashes, chain-of-custody fields), clear errors and exit codes.
5. **Scientific.** Benchmark that survives a thesis defence: ablation, confidence intervals, fair baselines, real data.

**Non-goals** (unless the user asks): GUI; APFS/HFS+/ext4; decryption of encrypted volumes; video recovery; content decoding beyond what validation needs.

---

## 2. Evidence base and verification ledger

### 2.1. What the merge verified, and how

| Claim | Result | Evidence |
|---|---|---|
| Repository geometry: 3,580 lines of Rust; `scan.rs` 689 lines; no `lib.rs`, no `tests/`, no CI; 18 unit tests (util 2, jpeg 5, png 5, pdf 6); `panic = "abort"`; `rust-version = "1.74"`; binaries committed in `dist/` | **CONFIRMED-STATIC** | `wc -l`, `grep -c "#\[test\]"`, `Cargo.toml` |
| `dist/` binaries match `dist/SHA256SUMS`; Linux binary is consistent with the source (panic message reports `src/fs/ntfs.rs:298:51`, the exact location in source) | **CONFIRMED** | `sha256sum -c`; scenario S8 |
| Line references used by v2/v3 (`scan.rs` L451/L364/L93; `util.rs` L128; `fs/mod.rs` L71/L205/L143; `ntfs.rs` L260/L298/L407/L467/L494; `pdf.rs` L15/L32/L87/L148; `jpeg.rs` L330–331; `png.rs` L278) | **CONFIRMED-STATIC** | all match the snapshot |
| **Declared MSRV is false.** Lockfile pins `clap 4.6.8`, `clap_builder 4.6.7`, `clap_lex 1.1.2` (all `edition = "2024"`, `rust-version = "1.85"`) and `rayon 1.12.0` / `rayon-core 1.13.0` (`rust-version = "1.80"`). Effective MSRV of the lockfile is **1.85**, not 1.74 | **CONFIRMED-STATIC** | crate manifests downloaded from `static.crates.io` |
| Multi-volume disk: default `auto` carves 30 of 60 files, scans 63,990,784 of 131,072,000 bytes, exits 0, and only announces the one chosen FAT32 volume (no warning about the skipped NTFS volume) | **REPRODUCED** (S1) | `fs/mod.rs:143` picks the largest volume only |
| PDF with an early `%%EOF` and a later large revision is cut at the first revision and labelled `complete` / score 100; linearized-style PDF is cut at 312 B, score 75 | **REPRODUCED** (S2, S3) | `pdf.rs` returns `Analyzed` as soon as any `%%EOF` is in the 1 MiB window |
| Embedded uncompressed PDF: outer PDF is cut at the inner `%PDF-` (`truncated`, score 60) and the inner file is emitted as a separate file | **REPRODUCED** (S4) | `pdf.rs:32` |
| JPEG made of the head of file B and the tail of file A is emitted as `complete`, score 100, hash ≠ A and ≠ B; **Pillow `load()` decodes it without error** (mean absolute pixel difference vs A = 60.3) | **REPRODUCED** (S5) | no entropy-data validation in `jpeg.rs` |
| Truncated JPEG followed by zeros swallows the rest of the extent (50.5 MB output from a 212 KB cut); score 60 > default `--min-score 50`, so it is emitted by default | **REPRODUCED** (S6) | `jpeg.rs`/`pdf.rs` set `end = data.len()` when truncated |
| Truncated PNG is rejected entirely (no salvage) | **REPRODUCED** (S7) | `png.rs` |
| Malformed NTFS MFT record 0 (attribute header at the record tail) → `index out of bounds` at `ntfs.rs:298:51` → SIGABRT in both `info` and `scan --mode auto`; no fallback to raw. (A milder corruption, runlist offset ≥ attribute length, is handled gracefully.) | **REPRODUCED** (S8) | `panic = "abort"`, `Cargo.toml` |
| Header flood: 1 MiB → 9.9 s, 4 MiB → 74.8 s on the 1-vCPU merge sandbox (65,536 / 262,144 candidates); 64,492 of 65,536 candidates are rejected with `expected JPEG marker at offset 16709` | **REPRODUCED** (S9) | see root-cause analysis in A-06 |
| `scan.rs`: carve loop is sequential; every candidate allocates and reads `min(hard_limit, 1 MiB)`; nested suppression is O(n²) and happens *after* the files were written (`fs::write` L278, `remove_file` L399, `files.remove(i)` L400); file numbers are assigned before suppression (gaps); `Ok(None)` is never produced; `repaired` is a constant `false` inside `carve_one` | **CONFIRMED-STATIC** | `scan.rs` |
| Progress bar **exists** for the header-scan phase (stderr TTY only, not for the carve phase, no ETA) | **CONFIRMED-STATIC** | `scan.rs:564–586` |
| Exit codes today: `0` OK, `1` runtime error, `2` clap usage error | **REPRODUCED** | `main.rs`, clap |
| Foremost/Scalpel false positives: Foremost 44 "unknown" FPs of which **40 are byte-identical to JPEG streams embedded in the corpus PDFs**; Scalpel 54 "unknown", 40 identical; PhotoRec 0 (pooled over 4 images, rep 1). `BAO_CAO.md §6.2` does not state this cause | **CONFIRMED-STATIC** | `bench/results/final/results.json` × `bench/corpus/pdf/*.pdf` |
| Benchmark conditions: FAT32 cluster = 512 B, NTFS cluster = 4 KiB; corpus = 13.6 MiB = 21 % of a 64 MiB image; images are random-noise-plus-gradient; images are not in git (`.gitignore`); `make_images.py` needs root | **CONFIRMED-STATIC** | geometry read from the images, `make_corpus.py`, `.gitignore`, README |
| Baseline configs: Foremost uses built-ins (`foremost.conf` has no entries); Scalpel `pdf y 20000000 %PDF- %%EOF` (no `REVERSE`/`NEXT`) and 20 MB max; PhotoRec runs as `photorec /log /d OUT /cmd IMAGE …` with no free-space restriction (README lists it as `raw`); CarveX uses all CPUs (README: 4-CPU host) | **CONFIRMED-STATIC** | `bench/*.conf`, `run_benchmark.py`, README |
| `results.json` already records tool *version strings* (`Foremost 1.5.7`, `Scalpel 1.60`, `PhotoRec 7.2, February 2024`) but not binary hashes; it embeds absolute `/home/kali/...` paths | **CONFIRMED-STATIC** | `results.json` |
| `/home/kali/...` paths and the "Pillow/ReportLab" statement are still in `BAO_CAO.md` (L11, L341, L633) and `CFREDS.md` | **CONFIRMED-STATIC** | grep |
| Binary-hash-matching the Windows `.exe` against source; Windows runtime behaviour | **NOT VERIFIED** | no Windows host |

### 2.2. What the merge could NOT verify (treat as [REPORTED])

- `cargo build`, `cargo test`, `cargo clippy`, `cargo fuzz` were **not run** (no Rust toolchain in the merge sandbox). v3's statements "18 tests pass", "default clippy shows 6 cosmetic warnings", "Rust 1.90 builds cleanly" are therefore unverified. **Phase 0 must measure them (A-12).**
- Foremost, Scalpel, PhotoRec, The Sleuth Kit were not available. v3's observation that PhotoRec 7.1 (reviewer) differs from 7.2 (report) — 7.1 splitting an EXIF thumbnail into a separate output (`t0006283.jpg`), giving TP 100/FP 24 instead of TP 94/FP 20 — is unverified. It is kept as a **risk** (C-07), not as a fact.
- v1's performance figures for a 64 MiB image (random 0.05 s; JFIF-like header every 4,096 B 5.57 s; every 512 B 44.08 s) and v2's "25,811 fake headers in 7.9 s" were not re-run. They are consistent with the cost model in A-06 (≈150–300 µs per candidate on one vCPU; merge measurements: 151 µs and 285 µs per candidate).
- v1's fragmented-FAT32 results (`t_frag`: Frankenstein JPEG 116,218 B, PDF score 75, PNG rejected by CRC) were not rebuilt; S5 reproduces the JPEG mechanism with a controlled splice.
- Garfinkel's fragmentation statistics quoted in v1 (≈6 %, JPEG ≈16 %, …) were not verified against the paper. **Do not quote them until confirmed from the source.**
- The original review scripts (`evidence/*.py`, `carvex_review_repro.py`) are **not in the repository**. Phase 0 recreates them (A-11, Appendix A).

### 2.3. Corrections to the original guides (conflict log)

| # | Topic | Original statements | Resolution |
|---|---|---|---|
| 1 | Root cause of the header-flood slowdown | v3: repeated `Need::More` buffer doubling re-reads each candidate up to `--max-size`; v1/v2: unconditional ~1 MiB alloc+read per candidate, sequential carve | **v1/v2 correct.** The flood input is *rejected after one APP0 skip* (S9), so no doubling happens. Cost ≈ `Σ min(1 MiB, remaining extent)` over candidates: super-linear below ~1 MiB of input, then linear in candidates with a ~65,536× I/O amplification at 16 B spacing. v3's measurements and remedies (budgets, quick reject, buffer reuse) remain valid. |
| 2 | CFReDS never run | v1 B-08 (P0), v2 F-21/C-08, v3 P1-2 | **Stale.** RM#2 was run on 2026-11-17 (`bench/results/cfreds/`, `docs/CFREDS.md §5`, README). What remains: ground-truth evaluation, RM#1 (exFAT, E01), DFRWS challenges, same-image baselines (C-08). |
| 3 | "No progress bar" | v3 P1-5.3 | **Partly wrong.** Header-scan progress exists (TTY only). Missing: progress and ETA for the carve phase (where the time goes under A-06), machine-readable events. |
| 4 | Exit codes | v1 Q-04: "only 0/1" | `2` already occurs for clap usage errors. Document and extend (§8.7). |
| 5 | MSRV | v2 F-15 "unverified", v3 P0-2 verified | **Verified:** effective MSRV 1.85. |
| 6 | PDF early-EOF condition | v1 R-04 / v2 F-01 | Refined: the bug triggers when a `%%EOF` lies **inside the initial 1 MiB read window** and a later revision extends **beyond** it. A 2-revision PDF whose *first* `%%EOF` is already past 1 MiB is carved whole today (control case S2c, must keep passing). |
| 7 | Decode-based ground truth | v2 C-03: Pillow `verify()` + `load()` | **Insufficient** (S5: a spliced JPEG passes `load()`). Ground truth additionally needs pixel/SSIM comparison against the original for images and text/page comparison for PDFs. Note also that Pillow's `Image.verify()` does not decode pixel data (per Pillow docs); the CFReDS "318/318 `verify()` OK" is structural evidence only. |
| 8 | Report compatibility | v1 §7.4 turns `tool` and `image` into objects; v2 says "only add fields" | Never change the type of an existing field. Keep `tool` (string) and `image` (string); add `tool_info`, `input`. |
| 9 | Hybrid recovery CLI | v1: new subcommand `recover`; v2: `--mode hybrid` | Decision: implement `scan --mode {metadata,hybrid}`; `recover` is an optional thin alias [MAY] (ADR-001). |
| 10 | Validation default | v1: deep validation ON; v2: `structure` default | Decision rule in A-04: ship `deep` as default only if dev-set recall is unchanged and cost ≤ 2×; otherwise default `structure` with honest labels. Either way `status=complete` is never presented as success. |
| 11 | Multi-volume default | v1: `auto` scans everything; v2: keep old default, ask the user | Two-step: (a) **immediately** add WARNINGs + `regions[]` (no confirmation needed); (b) flipping the default to "all volumes + gaps" is a STOP-AND-ASK item (§3.5). |
| 12 | Phase order | v1: benchmark v2 last; v2: benchmark infrastructure before features | v2's order adopted: measure with the same ruler before adding features. |
| 13 | Held-out data | v1: 60/40 dev/test per phase; v2: dev seeds vs single-use lockbox | v2's lockbox protocol adopted (stricter): C-01. |
| 14 | `/home/kali` leak | v2 C-07 | Also present in `results.json` (`command`, `cwd`, `output_dirs`, `log`), not just docs. |

---

## 3. Guardrails (violating any = PR rejected)

### 3.1. Forensic safety
- **Never write to the source image.** Open it with a read-only handle only; no `write` code path. Tests/benchmarks work on copies or generated images.
- Refuse (or warn loudly and require `--overwrite-device-output`) when the output directory lies on the same device/volume as a block-device source (A-08).
- Output is **deterministic**: same image + same options + same version ⇒ same file list, order, names, bytes — including multi-threaded runs.
- `exact` keeps its meaning: an output file is **exactly the bytes read from the image**, except under `--repair`/`--salvage`, where it must be labelled and the report must keep both `sha256_original` (bytes read) and `sha256_output`.

### 3.2. Anti-gaming (no "make the numbers pretty")
- **Forbidden:** editing `bench/corpus/**`, `bench/images/*.json` (ground truth), any `SHA256SUMS`, benchmark thresholds, metric definitions or baseline configurations **after seeing results**. Any protocol change goes into its own commit with the reason, **before** re-running.
- **Forbidden:** tuning parameters/heuristics on anything but the `dev` split (§10.2). The lockbox is single-use.
- **Forbidden:** deleting, `#[ignore]`-ing, loosening or rewriting an existing test to make it pass. The only allowed `#[ignore]` is a red-first regression test introduced in Phase 0 as `#[ignore = "<ID> xfail"]`, listed in `docs/PROGRESS.md`, and removed in the commit that fixes it. Python repro scenarios use `scripts/repro_xfail.txt` (§4.2).
- **Forbidden:** special-casing the corpus (file names, seeds, hashes, offsets, sizes) anywhere in `src/`.
- **Forbidden:** typing numbers into `README.md`, `docs/BAO_CAO.md`, `BENCHMARK.md` by hand; they come from `results.json` through the render script (C-07).
- `#[allow(...)]` / `# noqa` only with a one-line justification and the item ID.
- Do not write "100 %", "stable", "fair", "N× faster" without number + confidence interval + scope (§10.10).
- If a tool, dataset or command is **unavailable** (no network, missing package): record `SKIPPED (reason)` in the result/PROGRESS. **Never invent numbers** or infer the result of a run that did not happen.
- **Never invent citations** (§11, Appendix F).

### 3.3. Code quality
- `#![forbid(unsafe_code)]` at the crate root. If `unsafe` is truly needed (e.g., Windows device `ioctl`), isolate it in one module with `// SAFETY:` comments and its own tests (STOP-AND-ASK).
- No `unwrap()/expect()/panic!/unchecked indexing` on data originating from the image, in `src/fs/` and `src/signatures/` (enforced by `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]` there; tests may `#[allow]`). Use `get()`, `checked_*`, `try_into`, or a bounds-checked cursor.
- Offset/size arithmetic uses `u64` with `checked_*`/`saturating_*` chosen deliberately; no silent narrowing `as` casts.
- Every allocation driven by image data has a **configurable hard cap** (the 512 MiB FAT/`$Bitmap` caps are the model; apply uniformly).
- New dependencies only when needed; prefer small crates with MIT/Apache-2.0/BSD licences; record reason, licence, maintainer, transitive-dep count and MSRV in `docs/DEPENDENCIES.md` / `docs/DECISIONS.md`. For the core parsers (JPEG/PNG/PDF/FS) prefer in-tree code to keep bounds under control.

### 3.4. Compatibility
- Keep every existing subcommand, flag, output name pattern (`NNNNN_<ext>_<offset16hex>.<ext>`) and every JSON/CSV field **with its existing type and meaning**. Only **add**.
- A change of default behaviour needs: an entry under **Changed** in `CHANGELOG.md`, a flag to restore the old behaviour, a minor-version bump, and (if it changes users' results) user confirmation (§3.5).
- Report schema v2 carries `schema_version`; `--report-schema 1` emits the legacy shape until v1.0.

### 3.5. STOP-AND-ASK (stop only for these; otherwise decide conservatively and log in `docs/DECISIONS.md`)
- Changing a **default** in a way that changes users' results (e.g., `--mode auto` scanning every volume and gap, `--validate` default, truncated-file defaults).
- Using `unsafe`, adding a dependency with a doubtful licence/maintainer or > 50 transitive crates.
- Downloading external data (D2/D3) with unclear licence, large size or possibly sensitive content.
- Deleting committed files (e.g., `dist/`) or changing the licence.
- Changing a benchmark threshold, ground truth or metric definition **after** seeing test/lockbox results.
- Wording in the academic report that changes a **conclusion** (not just a factual correction).
- Two items contradict each other, or a gate cannot be made green within scope.
- Doubt about forensic semantics (e.g., whether output data may be normalised).
- The binary in `dist/` no longer matches the source, or a [REPRODUCED] scenario no longer reproduces at Phase 0 (it may mean the source changed).

---

## 4. Environment, gates, progress tracking

### 4.1. Start-up (record the result in `docs/PROGRESS.md` → *Environment*)

```bash
cargo --version && rustc --version      # lockfile needs >= 1.85 until A-12 is resolved (§2.1)
cargo build --release --locked          # offline: add --offline
python3 --version
# External tools: DO NOT install without network; record SKIPPED when missing
for t in foremost scalpel photorec mkfs.vfat mkfs.exfat mkntfs mcopy ntfs-3g blkls fls istat icat tsk_recover \
         ntfsundelete qpdf pdftoppm ewfacquire ewfexport 7z; do
  command -v "$t" >/dev/null && echo "OK   $t" || echo "MISS $t"
done
python3 -c "import PIL, pypdf, numpy" 2>&1 | tail -1     # bench/test dependencies
```

- The binary in `dist/` is **never** trusted to equal the current source. Always test `target/release/carvex` built from source.
- First task of Phase 0 (A-11/A-12): build from source, run `cargo test`, run every [REPRODUCED] scenario of Appendix A against the fresh binary and compare with the "v0.1.0 observed" column. A mismatch ⇒ STOP-AND-ASK (§3.5).

### 4.2. Gates (a phase is "done" only when all are green; `SKIPPED` is allowed but must be logged)

| Gate | Condition | Tool |
|---|---|---|
| G0 Format | no diff | `cargo fmt --all -- --check` |
| G1 Lint | 0 warnings; pedantic staged in A-10 | `cargo clippy --all-targets --all-features --locked -- -D warnings` |
| G2 Test | all green; every fixed finding has ≥ 1 regression test (red before fix) | `cargo test --all-features --locked` |
| G3 MSRV | builds with the **declared** `rust-version`, with the committed lockfile | `scripts/check-msrv.sh` / `cargo +<msrv> check --locked` |
| G4 Supply chain | no unhandled advisory; licences allowed | `cargo deny check`, `cargo audit` |
| G5 Fuzz | each parser/FS driver has a target; smoke 60 s clean; seed corpus committed | `cargo fuzz` |
| G6 Cross-platform | build + test on Linux and **Windows** (macOS optional) | CI matrix |
| G7 Golden | D0 outputs stay **byte-identical** (compare SHA-256 of every output file, not just counts): CarveX `unallocated` = 100/100 exact, 0 FP; `raw` = 100 TP + 20 kept. Differences must be intended, listed in `CHANGELOG`, and explained | `scripts/golden.py` + `bench/` |
| G8 Performance | no regression > 10 % on the perf scenarios; memory caps respected | `bench/perf/*` (C-09) |
| G9 Docs | README/BAO_CAO numbers equal `results.json`; flag table equals `carvex --help`; `RUSTDOCFLAGS="-D warnings" cargo doc` | `bench/render_docs.py --check` |

`scripts/gate.sh` (created in Phase 0) is the single entry point and is run before every commit:

```bash
#!/usr/bin/env bash
set -euo pipefail
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
if command -v cargo-deny  >/dev/null; then cargo deny check;  else echo "SKIPPED cargo-deny (not installed)"; fi
if command -v cargo-audit >/dev/null; then cargo audit;       else echo "SKIPPED cargo-audit (not installed)"; fi
MSRV=$(grep -m1 '^rust-version' Cargo.toml | cut -d'"' -f2)
if rustup toolchain list 2>/dev/null | grep -q "^${MSRV}"; then cargo +"${MSRV}" check --locked; else echo "SKIPPED MSRV ${MSRV} (toolchain missing)"; fi
cargo build --release --locked
python3 scripts/review_repro.py --carvex target/release/carvex --assert --xfail scripts/repro_xfail.txt \
        --workdir "$(mktemp -d)"
python3 scripts/golden.py --carvex target/release/carvex          # G7, needs bench/images/*.img (else SKIPPED)
python3 bench/run_benchmark.py --smoke                            # created in C-07: small dev-set run, < 60 s
```

**Red-first workflow (xfail).** In Phase 0 the repro scenarios are *red* (that is the evidence). To keep the gate green while unfixed, `scripts/repro_xfail.txt` lists the currently failing scenario IDs, one per line. `--assert --xfail` rules: a scenario **not** listed that fails ⇒ gate fails; a scenario **listed** that passes ("XPASS") ⇒ gate fails and forces removal of the line. The commit that fixes an item removes its scenarios from the file. At the end of Phase 1 the file must be empty. For Rust tests the equivalent is `#[ignore = "<ID> xfail"]` (§3.2).

### 4.3. Progress file (`docs/PROGRESS.md`; updated after **every** task)

```markdown
## Environment
<date, rustc, tools present/missing>

## Tasks
| ID | Status | Commit | Gate | Notes / evidence |
|----|--------|--------|------|------------------|
| A-11 | DONE | abc1234 | green | baseline: 18 tests pass, clippy N warnings |
| A-01 | IN_PROGRESS | - | - | red tests S2,S3 added; fixing pdf.rs |

## Open xfail tests / scenarios
## Blockers / open questions
## New findings outside this guide (temp ID N-01 + evidence + proposed severity)
## Decisions → docs/DECISIONS.md   ·   Lockbox consumption → bench/lockbox.log
```

Per-task report (paste into PROGRESS):

```
Task: <ID> — <title>          Findings: <old IDs if relevant>
Change: <main files, 1–3 lines>
New tests: <names; red before / green after: yes|no>
Gate: green | green (SKIPPED: ...)
Measurement (if any): before <x ± CI> → after <y ± CI> on <dataset/seed>
Residual risk / next: <...>
```

---

## 5. Repository map (verified) and baselines to preserve

```text
src/main.rs            entry; panic="abort" in Cargo.toml (A-03); NO lib.rs  → no integration tests/benches possible
src/cli.rs   (264)     clap: info | scan | signatures
src/scan.rs  (689)     run_scan (~300 lines, god function); carve_one L451; nested suppression L364; ScanReport L93;
                       progress thread L564 (header phase only, TTY only)
src/report.rs (76)     JSON/CSV/summary
src/util.rs  (238)     open_image L128 (metadata().len() ⇒ block device = 0); read_at (unix pread / windows seek_read)
src/fs/mod.rs (379)    detect L71 (largest volume only L143), read_partition_table L205 (MBR primary + GPT primary only),
                       byte_offset = lba*512 (L64–65)
src/fs/fat32.rs (172)  BPB + FAT#1 → free extents (FAT read fully into RAM, 512 MiB cap)
src/fs/ntfs.rs (536)   boot → $MFT rec 0 → $Bitmap (rec 6); parse_attrs L260 (OOB panic L298); 3× allow(too_many_arguments) L407/467/494
src/signatures/        mod.rs (FileType enum, Status{Complete,Truncated,Repaired}, Analysis.complete: bool, print_signatures hard-coded)
                       jpeg.rs (allow L330), png.rs (allow L278), pdf.rs (analyze L15, next_header L32, chosen_idx L87, truncated L148)
bench/                 make_corpus.py, make_images.py (root + loop mount), run_benchmark.py, foremost.conf, scalpel.conf,
                       corpus/ (30 files, 13.6 MiB), images/ (4 × 64 MiB, git-ignored), results/final/, results/cfreds/
dist/                  committed Linux (musl static) + Windows (GNU) binaries + SHA256SUMS
docs/                  BAO_CAO.md (VN report), CFREDS.md (RM#2 results now in §5)
scripts/               fetch_cfreds.sh only
```

**Missing entirely:** `lib.rs`, `tests/`, CI (`.github/`), `rustfmt.toml`, `clippy.toml`, `deny.toml`, `rust-toolchain.toml`, fuzz targets, `CHANGELOG`, `SECURITY`, `CONTRIBUTING`, repro scripts. **Tests today:** 18 unit tests (`signatures/*`, `util.rs`); none for `fs/*`, `scan.rs`, `cli.rs`, `report.rs`.

### Baselines to preserve (G7) and to freeze (C-01)
- **D0 sanity** (4 images × 30 files; README "Kết quả pooled"): CarveX `unallocated` 100 exact/0 FP; `raw` 100 TP + 20 FP (the 20 kept files); Foremost 94/62/6 and Scalpel 92/74/8 (usable); PhotoRec 94/20/6. Note: **30 distinct files**, each present in 4 contexts — never count 100 as independent samples (§10.7).
- **CFReDS RM#2** (`bench/results/cfreds/`): 318 outputs (113 JPEG, 205 PNG, 0 PDF), 73 rejected, 38 nested suppressed, `unallocated` (1,020 MiB, 1.6 s) and `raw` (3.73 GiB, 3.4 s) give identical output; no per-file ground truth. After A-04 the *labels* may change (that is the point); the *bytes* of outputs must not change unless a validator legitimately rejects a file — list differences in `CHANGELOG`.
- Copy `bench/results/final/` → `bench/results/baseline-v0.1/` (read-only, with `SHA256SUMS`) in Phase 0. All before/after comparisons use that copy.

---

## 6. Findings register and work items

Priority: **P0** = silent data loss/wrong data or a false conclusion; **P1** = serious reliability/coverage/science gap; **P2** = quality, maintainability, UX. "Old IDs" are traceable in Appendix D (`v1:` = guide-v1, `v2:` = guide-v2, `v3:` = guide-v3).

### 6.0. Register

| ID | Pri | Item | Evidence | Old IDs |
|---|---|---|---|---|
| **A-01** | P0 | PDF end detection: early `%%EOF`, incremental, linearized | REPRODUCED | v1:R-04 · v2:F-01 |
| **A-02** | P1 | Embedded PDF cuts the outer PDF | REPRODUCED | v1:R-05 · v2:F-02 |
| **A-03** | P1 | Parser panics + `panic=abort`; no fuzz | REPRODUCED | v1:R-10 · v2:F-03 |
| **A-04** | P0 | "complete" = structure only; validity model + deep validators | REPRODUCED | v1:R-01 · v2:F-04 |
| **A-05** | P0 | Truncated files swallow the rest of the extent | REPRODUCED | v1:R-03 · v2:F-07 |
| **A-06** | P1 | Unbounded cost on dense/adversarial candidates; O(n²) parts | REPRODUCED / UNMEASURED | v1:R-09,R-11,R-12 · v2:F-06 · v3:P0-1 |
| **A-07** | P0 | Silent skipping of volumes and gaps | REPRODUCED | v1:R-02,U-01 · v2:F-08 |
| **A-08** | P1 | Source access: block devices, read errors, sector size | CONFIRMED-STATIC | v1:R-08 · v2:F-09 |
| **A-09** | P2 | FS parser robustness (NTFS attribute list, mirrors, FAT backups) | CONFIRMED-STATIC | v1:R-14 |
| **A-10** | P2 | Architecture, maintainability, clippy | CONFIRMED-STATIC | v1:Q-01,Q-05,Q-06 · v2:F-14 · v3:P2-1 |
| **A-11** | P1 | Test pyramid and repro harness | CONFIRMED-STATIC | v1:Q-02 · v2:F-15 |
| **A-12** | P1 | CI, MSRV truth, supply chain, release hygiene, Windows | CONFIRMED-STATIC | v1:Q-03,Q-09 · v2:F-15 · v3:P0-2 |
| **A-13** | P2 | Typed errors, logging, exit codes, progress events | CONFIRMED-STATIC | v1:Q-04 |
| **A-14** | P2 | Report schema v2 and forensic integrity | CONFIRMED-STATIC | v1:Q-07 · v2:F-13 |
| **A-15** | P2 | Output-handling hardening | CONFIRMED-STATIC | v1:Q-08 |
| **B-01** | P1 | Metadata-assisted recovery (FAT dir entries, NTFS MFT) | design gap | v1:U-02,R-13 · v2:F-10 · v3:P1-5.4 |
| **B-02** | P1 | Partition-table completeness (EBR, GPT backup/CRC, 4Kn) | CONFIRMED-STATIC | v1:R-14 · v2:B-02 |
| **B-03** | P1 | exFAT, FAT12/16 | design gap | v1:U-05 · v2:B-03 · v3:P1-1 |
| **B-04** | P1 | Evidence formats: E01, split raw, VHD/VMDK, stdin | design gap | v1:U-03 · v2:B-04 · v3:P1-5.1 |
| **B-05** | P2 | Cluster/sector alignment of header search | CONFIRMED-STATIC | v1:R-15 · v2:F-11 |
| **B-06** | P1 | Fragment handling (bifragment gap carving) | REPORTED + A-04 | v1:R-13 · v2:F-12 |
| **B-07** | P1 | Salvage and honest `--repair` | REPRODUCED (S7) | v1:R-06,R-07 · v2:F-05 |
| **B-08** | P1 | Operations/UX: carve-phase progress, resume, dry-run, verify, hashes | CONFIRMED-STATIC | v1:U-04 · v2:B-10 · v3:P1-5.2,5.3 |
| **B-09** | P2 | Format-variant coverage | UNMEASURED | v1:R-16 |
| **B-10** | P2 | Custom signatures and new formats (ZIP/OOXML first) | design gap | v1:U-06 · v2:B-09 · v3:P1-5.5 |
| **B-11** | P2 | I/O performance | UNMEASURED | v2:B-11 |
| **C-01** | P0 | Written protocol, freeze, single-use lockbox | process gap | v1:B-02 · v2:C-01 |
| **C-02** | P0 | Datasets: unique-file n, fragmentation, overwrite, realism | CONFIRMED-STATIC | v1:B-01,B-03,B-07 · v2:F-18 · v3:P1-4 |
| **C-03** | P1 | Independent ground truth and checkers | design gap | v2:C-03 |
| **C-04** | P1 | Metrics: false-complete, splice, bloat, partial, FP taxonomy | CONFIRMED-STATIC | v1:B-05 · v2:F-17,F-20 |
| **C-05** | P1 | Statistics: CI, paired tests | design gap | v1:B-01 · v2:C-05 |
| **C-06** | P1 | Fair baselines and ablation | CONFIRMED-STATIC | v1:B-04,B-09 · v2:F-16,F-19 |
| **C-07** | P1 | Reproducible runner, version+hash pinning, no hard-coded paths | CONFIRMED-STATIC | v1:B-07 · v2:C-07 · v3:P1-3 |
| **C-08** | P0 | Real data: extend the CFReDS RM#2 run; RM#1; DFRWS | PARTLY DONE | v1:B-08 · v2:F-21,C-08 · v3:P1-2 |
| **C-09** | P1 | Performance measurement method and scaling | CONFIRMED-STATIC | v1:B-06 · v3:P1-4 |
| **D-01** | P1 | Factual hygiene of README/BAO_CAO/CFREDS | CONFIRMED-STATIC | v1:§10.7 · v2:F-21 |
| **D-02** | P1 | Academic content: related work, RQs, taxonomy, threats | design gap | v1:B-10 · v2:C-09 |
| **D-03** | P2 | Release docs and review response | — | v1,v2 |

### 6.1. Workstream A — reliability, correctness, maintainability

#### A-01 PDF end detection [P0, MUST]
- **Problem.** `pdf::analyze` returns `Analyzed(complete)` as soon as any `%%EOF` lies in the buffer (`pdf.rs` L65–L146) — it never asks for more data. With the initial 1 MiB read window, a PDF whose first `%%EOF` is inside the window but whose later revision extends beyond it is cut at the first revision and labelled `complete`. Affects incremental updates and linearized PDFs (first-page `%%EOF`, `startxref 0`). `--pdf-eof smart` does not help (it picks among `%%EOF`s *in the window*). Observed: S2 (329 B of 1,500,481 B, score 100), S3 (312 B, score 75).
- **Fix.** Implement the continuation rule of **Appendix B**: after the selected `%%EOF`, skip EOL/whitespace; if the next token is `N G obj`, `xref` or `trailer` the file continues (incremental update or linearized body) → extend to the next `%%EOF`; request more data (`Need::More`) when fewer than 32 bytes remain and the limit is not reached; conclude only when what follows is not PDF continuation (zeros, other signature, garbage) or the limit is reached (then `partial`/`suspect`, with a note, never `verified`). A `%PDF-` is a boundary **only** if it follows a `%%EOF` (modulo whitespace/NUL); a header inside a stream or object is not a boundary. Optionally confirm by walking the `/Prev` chain (bounded depth); do the same for xref streams (`/Type /XRef`). Score `Complete` high only when `%%EOF` **and** consistent structure are present; a linearized first-page `%%EOF` must never score 100.
- **Tests (red → green).** (1) incremental update crossing 1, 2, 4 MiB (± 1 byte around the boundary); (2) linearized 1/2/3 MiB (a real `qpdf --linearize` fixture when `qpdf` exists, plus the synthetic S3); (3) `%%EOF` split across two reads; (4) two PDFs back to back with no gap → two files; (5) truncated PDF → `partial`, no swallowing of trailing garbage; (6) xref-stream/`/ObjStm` PDF (qpdf `--object-streams=generate` when available); (7) **control S2c**: two-revision PDF whose first `%%EOF` is already past 1 MiB must stay whole.
- **Acceptance.** S2, S2c, S3 green; all old PDF unit tests green; G7 unchanged; no TP lost on dev set.

#### A-02 Embedded PDF [P1, MUST]
- **Problem.** `next_header` (`pdf.rs` L32) treats any later `%PDF-` as a boundary, even inside a valid stream. Observed S4: outer 17,828 B → 11,414 B `truncated`; inner 6,211 B emitted separately; when the part before the inner header is < `--min-size` the outer file vanishes altogether (6,633 B outer case: 240 B before the inner header, counted in `below_min_size`; with `--min-size 1` it appears as a 240 B `truncated` file).
- **Fix.** Boundary rule of A-01. A child fully inside a parent whose `validity ∈ {verified, structural}` gets `nested_in: <seq>` and is dropped by default (`--keep-nested` keeps it). Nested suppression must be decided **before** files are written (A-06).
- **Acceptance.** S4 ⇒ exactly one output equal to the outer file; child suppressed/marked.

#### A-03 No panics, no abort, fuzzing [P1, MUST]
- **Problem.** `ntfs.rs:298` indexes `rec[pos + 0x20]` for a non-resident `$DATA` header located at the record tail → `index out of bounds` → with `panic = "abort"` the process dies (SIGABRT) in `info` **and** `scan --mode auto`; there is no fallback to raw (S8). Other unchecked indexing sits in the same function (L273–L323) and in `fs/mod.rs`.
- **Fix.**
  1. Replace indexing/`unwrap` in `src/fs/*` and `src/signatures/*` with a bounds-checked cursor (`struct Cur<'a>{buf:&'a [u8]}` with `u16/u32/u64(off) -> Result<_>`); `checked_add/mul` for every offset computation (LCN × cluster, volume offset + …); `unsigned_abs()` instead of negation.
  2. `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]` for `fs` and `signatures`.
  3. `panic = "unwind"` in the release profile; wrap `fs::detect` in `catch_unwind` → convert to `Err` → **auto mode falls back to raw with a WARNING and exit code 3**; wrap each candidate analysis in `catch_unwind` → record `internal_error` in the report and continue. `overflow-checks = true` for release (measure the cost; justify in `DECISIONS.md` if disabled).
  4. Fuzz targets: `fuzz_jpeg`, `fuzz_png`, `fuzz_pdf`, `fuzz_fat32_boot`, `fuzz_ntfs_boot`, `fuzz_ntfs_mft_record`, `fuzz_partition_table` (MBR+GPT), later `fuzz_exfat_boot`. Seeds from `bench/corpus` and `bench/images`. Every crash found becomes a fixture in `tests/fixtures/crashes/` plus a regression test.
  5. Durations: CI smoke 60 s/target; nightly 10 min/target; release 30 min/target; v1.0 requires ≥ 24 h cumulative per target without crash.
- **Acceptance.** S8 ⇒ no abort; `info` prints a diagnostic; `scan` auto falls back to raw + WARNING + exit 3; **control S8b** (runlist offset ≥ attribute length) keeps behaving gracefully; a deliberately panicking fake validator still yields a report with `internal_error`; fuzz gates met.

#### A-04 Validity model and deep validation [P0, MUST]
- **Problem.** `Analysis.complete` means "a footer was found", and `Status::Complete` is read as "valid". JPEG is never checked past the marker structure (no entropy data, no RST sequence); PNG deducts only 15 points per bad CRC and still reports `complete`; PDF does not check xref offsets. Observed S5: spliced JPEG ⇒ `complete`, score 100, and it even decodes in Pillow. (v1 additionally reports PDF `complete` score 75 for a Frankenstein PDF — [REPORTED].) Academic basis: Garfinkel (DFRWS 2007) — header/footer may only *exclude*; content must be *validated*.
- **Fix (two steps).**
  1. **State model (§8.3):** add `validity ∈ {verified, structural, suspect, partial, repaired}`, `source ∈ {carved, metadata, hybrid}`, `fragments[]`, `validated_bytes`. Keep the legacy `status` field for compatibility but document it as "footer found"; **only `verified` may be counted as "complete" in any report or benchmark**; `structural` is explicitly "content not checked".
  2. **Deep validators** (`--validate {none,structure,deep}`):
     - **JPEG:** RSTn sequence (mod 8) when DRI is present (cheap, catches splices well); DQT/DHT present before SOS; SOF component count = SOS component count; Huffman-decode every scan, count MCUs against the SOF geometry and require the decoder to consume exactly to EOI (baseline first, progressive later).
     - **PNG:** chunk order (IHDR first, PLTE before IDAT, IDAT contiguous, IEND last); concatenate IDAT and inflate in streaming mode (`miniz_oxide`/`flate2` after evaluation); check Adler-32; decompressed length = formula from IHDR (non-interlaced and Adam7); filter bytes ≤ 4; **bad CRC ⇒ `suspect`**.
     - **PDF:** xref table **and** xref stream, follow `/Prev` and `/XRefStm`; each in-use entry points at the right `N G obj`; `/Size`; `/Root` resolves to a `/Type /Catalog`; direct `/Length` matches `endstream`; flag `/Encrypt`.
     - `score` must be calibrated and documented (B-05/C-04); no PDF "score 75" can carry a success label.
  3. **Independence.** In-tree validators may be self-written or a vetted crate, but the **benchmark's ground-truth checkers are never CarveX's own validators** (C-03).
  4. **Default decision rule.** Make `deep` the default only if (a) no true file of the dev set is rejected and (b) throughput cost ≤ 2×. Otherwise default `structure`; labels stay honest either way.
- **Acceptance.** S5 ⇒ output is not `verified`; no valid corpus file (D0 + D2 dev) is rejected by `deep`; property test: splice the tail of file X onto the head of file Y at a cluster boundary ⇒ `verified` rate ≤ 0.1 % over ≥ 1,000 pairs (proposed threshold; record any change in `DECISIONS.md`); cost of `deep` measured and written down; `false_complete_rate` (C-04) reported on the fragmented benchmark.

#### A-05 Truncated files [P0, MUST]
- **Problem.** For a truncated file `jpeg::truncated`/`pdf::truncated` set `end = data.len()` (buffer length), not the last verified byte. Observed S6: a 212,468-byte JPEG cut followed by 48 MiB of zeros produced one 50,544,116-byte output (`truncated`, score 60). Score 60 exceeds the default `--min-score 50`, so such outputs are emitted by default (same for PDF 50+10 and PNG 60). With the default `--max-size 64M`, every dangling header can write up to 64 MiB of junk.
- **Fix.** `Analysis.end` for a truncated file = **last verified byte**: JPEG — end of the last valid marker/RST or the last non-zero entropy byte before a zero run ≥ 1 cluster; PNG — end of the last valid chunk; PDF — after the last `endobj`/`endstream`. Report `tail_trimmed_bytes`. Add `--max-truncated-size` (conservative default, e.g. 16 MiB), a global `--max-output` guard (A-15), and make partial outputs carry `validity=partial`; their default emission threshold is a measured dev-set decision (proposed: require `validated_bytes` ≥ a documented minimum and SOS/IDAT present).
- **Acceptance.** S6 ⇒ output size ≤ cut + 1 cluster. Property test: cut a valid file at a random position and pad with zeros / random / 0xFF ⇒ output ≤ cut + 1 cluster.

#### A-06 Resource safety and performance [P1, MUST]
- **Root cause (verified).** `scan.rs`: the carve loop is sequential (L244); for **every** candidate `carve_one` allocates `vec![0; min(hard_limit, 1 MiB)]` and `pread`s it (L463–L477) even when the parser rejects after a few bytes; `Need::More` doubles the buffer and re-parses from scratch (up to 64 iterations); each parse restarts at byte 0. Cost ≈ `Σ min(1 MiB, remaining extent)` over candidates → ~150–300 µs per candidate on one vCPU. Under the flood of S9 the validators reject after one APP0 skip; the time is the per-candidate allocation + 1 MiB read (merge measurements: 1 MiB → 9.9 s, 4 MiB → 74.8 s; ratio 7.6 vs 7.0 predicted by the model). This is not only an attack: thumbnail caches, DCIM burst storage or heavily overwritten free space produce dense candidates. [v1: 131,074 candidates over 64 MiB took 44 s — consistent.]
- **Fix (all [MUST] unless noted).**
  1. **Incremental reads:** start at 4–64 KiB (measure, then choose), grow geometrically, keep one `Vec` and read only the delta; no re-parse from byte 0 (streaming `feed(&[u8])` parsers, §8.2).
  2. **Cheap pre-filters** before the full parser (≤ 64 B): JPEG — the marker after SOI is valid and the segment length plausible against the remaining extent; PNG — IHDR at offset 8 with length 13; PDF — `%PDF-d.d` and an `obj` within the first 1 KiB.
  3. **Non-convergence rule:** repeated `Need::More` with no new structural signal (SOF/SOS/IHDR/IDAT/xref/obj) between calls ⇒ reject early; distinguish "buffer ended" from "length/offset points implausibly far".
  4. **Parallel carve phase** (rayon) with deterministic output (sort by offset, then number); per-thread reusable buffers; reuse the buffer already read for neighbouring candidates.
  5. **Single multi-pattern header scan** (`aho-corasick`, after dependency evaluation) instead of three `memmem` passes.
  6. **Budgets:** `--max-candidates`, `--time-budget`, `--max-work-per-candidate`, `--max-memory`; on excess: WARNING + partial report + exit code 3 (never hang).
  7. **Pipeline:** *analyze → filter (nested suppression by sweep-line over sorted ranges, O(n log n)) → assign `seq` → write*; write through temp file + atomic `rename`; compute SHA-256 while writing; no "write then delete" (`scan.rs` L270–L279 vs L396–L401); no gaps in numbering.
  8. **PDF O(E×N):** `count_objects(&data[..end])` and `find(&data[..end], "/Root")` rescan from the start for each `%%EOF` (≤ 4,096 of them) [UNMEASURED] → single pass with prefix counters.
  9. [SHOULD] I/O reuse of the header-phase chunk during carving (B-11).
- **Acceptance (thresholds proposed; measure after the fix and record in `DECISIONS.md`; never loosen without a decision entry).** On one vCPU: S9-256K (16,384 candidates) < 1 s; S9-4M ≤ 3 s (hard fail > 10 s); S10 flood512 (64 MiB, 131,074 candidates) ≤ 3 s and ≤ 256 MiB RSS; S11 (25,811 fake headers) ≤ 1 s; random 64 MiB not > 10 % slower than baseline; criterion micro-benchmark "4,096 `%%EOF` in 64 MiB" ≤ 1 s; 200,000 synthetic candidates ⇒ filter phase < 1 s; exceeding `--max-candidates` ⇒ `warnings[]` + exit 3; D0 golden unchanged; README/BAO_CAO limitations updated with the trade-off (a budget can drop a legitimate file adjacent to a noisy region — measure its recall impact on dev).

#### A-07 Never skip data silently [P0, MUST]
- **Problem.** `fs::detect` sorts candidate volumes by length and takes the first (`fs/mod.rs` L143–L144); `scan.rs` accepts one `FsInfo`. Other volumes, partitions that cannot be parsed, free space outside partitions and deleted-partition areas are never scanned in `unallocated` mode — and nothing says so. S1: 30 of 60 files, 63,990,784 of 131,072,000 bytes, exit 0, one `Filesystem:` line for the chosen volume only. Real-data echo: the CFReDS RM#2 image has a 1 GiB FAT32 partition and 2.7 GiB outside it; the default run scans only the partition and does not mention the rest (here `raw` shows nothing is lost, but the tool did not know).
- **Fix, step 1 (no confirmation needed):**
  - `detect_all()` returns **every** supported volume plus the list of regions that belong to no supported volume (gaps, unsupported/unparseable partitions, reserved areas).
  - `info` lists all volumes and regions. `scan` always emits a WARNING (stderr + `warnings[]`, with a code such as `VOLUME_NOT_SCANNED`, `GAP_NOT_SCANNED`, `VOLUME_UNSUPPORTED` and a reason) for every volume/region that is not scanned, and adds `regions[]` to the report (`kind: free|allocated|gap|unsupported`, `offset`, `len`, `scanned`). Exit code `3` when evidence was skipped.
  - New options: `--partition all|N|largest` (default `largest` = today's behaviour until step 2) and `--include-gaps` (scan gaps and unsupported partitions raw). With `--partition all` each volume gets its own sub-directory (`part1/`, `part2/`) and a report entry.
- **Fix, step 2 (STOP-AND-ASK):** make `auto` scan the free space of **all** supported volumes **plus** all gaps raw; keep `--partition largest` as the old behaviour; CHANGELOG *Changed*; minor-version bump.
- **Acceptance.** S1: `--partition all` ⇒ 60/60 and `regions[]` lists two volumes; default ⇒ 30 + WARNING + exit 3 (until step 2 is approved); an image with an unsupported partition ⇒ WARNING, and `--include-gaps` scans it raw; CFReDS RM#2 default run reports the 2.7 GiB region as `gap, scanned=false`.

#### A-08 Source access [P1, MUST]
- **Problem.** `open_image` sizes the source with `metadata().len()` (`util.rs` L128–L133): a block device reports 0. 4Kn sectors, read errors and output-on-source-device are unhandled; `byte_offset` assumes 512-byte sectors. The `util.rs` comment says positional reads do not change the cursor, but Windows `seek_read` does change it [UNVERIFIED for thread safety].
- **Fix.** `trait ImageSource { fn len(&self)->u64; fn read_at(&self, off:u64, buf:&mut [u8])->io::Result<usize>; }` with `RawFile`, `BlockDevice` (size via `seek(End)`/`BLKGETSIZE64`/`IOCTL_DISK_GET_LENGTH_INFO` behind `cfg`; any `unsafe` isolated and approved), later `SplitRaw`/`Ewf` (B-04). Detect logical/physical sector size (512/4096) and add `--sector-size`; sector-aligned buffered reads for raw devices (Windows requires alignment). `--on-read-error {abort|zero-fill|skip}` with `unreadable_regions[]` in the report. Warn (or refuse) when the output directory is on the source device. Windows: verify multi-thread safety on the CI runner or give each thread its own handle; fix the comment either way.
- **Acceptance.** A mock `ImageSource` failing at offset X ⇒ `unreadable_regions` exact; `carvex info /dev/loopN` works (or `SKIPPED` with reason); `metadata().len()` is no longer the only size source.

#### A-09 FS parser robustness [P2, SHOULD]
- NTFS: read `$ATTRIBUTE_LIST` and merge runs from extension records; support a multi-extent `$MFT` (`pick_data` takes only the first `$DATA`; `BAO_CAO §6.4` admits it); use `$MFTMirr` when record 0 is damaged (`mftmirr_lcn` is parsed but only stored in `details`); fall back to the backup boot sector (last sector of the volume); separate the `0x60` volume-label attribute from `DataAttr` (type confusion in `parse_attrs`).
- FAT32: use FAT #2 when #1 is damaged, backup BPB (sector 6), cross-check FSInfo; honour the mirroring flags.
- Every FS-parse failure is reported in `warnings[]` with the reason.
- **Acceptance.** Fixtures: NTFS with a fragmented `$MFT` (ntfs-3g in a container) ⇒ correct `info`; wiped boot sector ⇒ recovered via backup; builders in `tests/support/`.

#### A-10 Architecture and maintainability [P2, MUST for the lib split, SHOULD for the rest]
- Split `src/lib.rs` + thin `src/main.rs`. **First as a pure move (Phase 0) proven by byte-identical golden output**; deeper refactors after the P0/P1 fixes so diffs stay readable.
- Public API: `Image`/`ImageSource`, `Volume`, `Carver` registry, `Engine`, `Report`; `#![warn(missing_docs)]`; `cargo doc` clean.
- Break up `run_scan` into pure stages `plan_regions → find_candidates → analyze → assemble → filter → write → report` (each ≲ 80–120 lines).
- Replace the five `#[allow(clippy::too_many_arguments)]` (`ntfs.rs` ×3, `png.rs` L278, `jpeg.rs` L330) with parameter structs (`NtfsCtx`, `ReadCtx`, a `TruncCtx` for the parsers); merge the duplicated extent-building code of `fat32.rs`/`ntfs.rs` into an `ExtentBuilder`.
- Remove hacks: `let _ = sparse;` (`ntfs.rs` L375), `let _ = chosen.marker_pos;` (`pdf.rs` L136), the never-taken `Ok(None)` branch and the constant `repaired = false` in `carve_one`; move `#[cfg(test)] mod tests` to the end of each file; replace magic numbers (16, 1 MiB, 64, 4,096, 2,000,000) with documented constants.
- Format registry (§8.2): `signatures`, `--types` and dispatch are generated from it (today adding a format touches ≥ 5 places and `print_signatures` is hard-coded).
- Clippy: default level must be warning-free (v3 reports 6 cosmetic warnings — measure); then clear `clippy::pedantic` in groups (mostly numeric casts such as `u32 as f64`, `usize as u8`), using `#[allow]` only with a comment proving the cast is safe (e.g., image offsets < 2^53).
- **Acceptance.** All tests green; golden byte-identical; no `allow(too_many_arguments)`; no function > 120 lines (tests excepted); `cargo doc -D warnings` clean.

#### A-11 Test pyramid and repro harness [P1, MUST]
- Create `scripts/review_repro.py` (Python; Pillow, pypdf, numpy) implementing Appendix A with `--assert` and `--xfail FILE` (semantics §4.2), and port each scenario to Rust integration tests (`tests/regress_*.rs`) that build their images in memory/tempfile (no Python dependency). Save the *red* baseline output in `docs/baseline/repro_v0.1.0.txt`.
- Layers and targets in §9. Coverage (`cargo llvm-cov`): ≥ 85 % lines for `fs/` and `signatures/`, ≥ 80 % for the engine, ≥ 75 % for the crate (proposed; report in PROGRESS; no empty tests to inflate it).
- Every Critical/High finding has at least one red-before/green-after test with the item ID in a comment (`// A-01`).

#### A-12 CI, MSRV, supply chain, release hygiene [P1, MUST]
- **MSRV truth (verified).** The committed lockfile needs Rust **1.85**. Resolve with one of: **(a) set `rust-version = "1.85"` and fix README ("Rust 1.74 or later") — recommended, simplest**; **(b)** keep 1.74 and re-pin `clap`/`clap_builder`/`clap_lex`/`rayon`/`rayon-core` to versions whose manifests allow it (`cargo update -p <crate> --precise <ver>`, re-verify the whole tree) — only if the user insists. Add `scripts/check-msrv.sh` and a CI job that builds with exactly the declared `rust-version`.
- `.github/workflows/ci.yml`: matrix `ubuntu`, `windows`, (`macos`); jobs: fmt, clippy `-D warnings`, test, doc, MSRV, `cargo deny`/`audit`, fuzz smoke, coverage; nightly fuzz; `bench/run_benchmark.py --smoke`. Add `rustfmt.toml`, `clippy.toml`, `deny.toml`, `rust-toolchain.toml`.
- Release job: Linux musl static + Windows build, `SHA256SUMS`, SBOM (`cargo cyclonedx` or `cargo auditable`), signature (`cosign`/`minisign`) [SHOULD]. **Remove the committed binaries from `dist/` and publish them as GitHub Release assets (STOP-AND-ASK).**
- Run the test suite on a real Windows runner (README and `dist/README.md` admit the `.exe` was never executed).
- **Acceptance.** CI green; MSRV job builds with the declared version and the committed lockfile; `cargo build --release` succeeds with the toolchain named in the README.

#### A-13 Errors, logging, exit codes, progress [P2, SHOULD]
- `thiserror` types in the library (`ImageError`, `FsError`, `ParseError`, `ScanError`); `anyhow` only in `main`. No `println!` in the library; logging with levels `error/warn/info/debug` (`--verbose` = debug, `--log-format json`). Progress through a `ProgressSink` trait/event channel so the UI is separate from the engine.
- **Exit codes (documented, tested):** `0` OK · `1` fatal runtime error · `2` usage error (clap) · `3` finished but **incomplete** (unscanned region, unparseable volume, budget exceeded, read errors, internal errors) · `4` interrupted (Ctrl-C) with a partial report written.
- User-facing messages: short English, actionable ("use --force …").

#### A-14 Report schema v2 and forensic integrity [P2, MUST]
- Add (never retype existing fields): `schema_version: 2`, `argv`, `options` (all effective parameters), `tool_info {version, commit (via build.rs), rustc}`, `input {size, sector_size, sha256_before, sha256_after, mtime/size before and after}` (`--hash-image`, streaming; optional `--hash sha256,md5,sha1`), ISO-8601 UTC `started_at`, `--case-id`, `--examiner`, `volumes[]`, `regions[]`, `warnings[]`, `unreadable_regions[]`.
- Per file: `validity`, `source`, `validated_bytes`, `tail_trimmed_bytes`, `fragments[]`, `lba`, `partition_index`, `cluster`/`lcn`, `extent_id`, `allocated`, `aligned`, `nested_in`, `original_name`/`mtime` (B-01), `sha256_original`/`sha256_output` when repaired.
- **Document, don't rename:** `size` = bytes written (including a repair suffix); `carved_size` = bytes of the file proper (`analysis.end`).
- Ship `report.schema.json` and a validation test; CSV through the `csv` crate; atomic report writes (temp + rename); `--manifest` writes `SHA256SUMS` for the output directory; `--report-schema 1` emits the legacy shape.
- **Acceptance.** Sample reports validate against the schema; two runs on the same input give identical reports except timestamps/duration; old fields intact.

#### A-15 Output hardening [P2, SHOULD]
Allocation caps everywhere; refuse an output directory that equals/is inside the source or on the source device; never follow symlinks when writing; `--force` allows a non-empty directory but does **not** overwrite existing files unless `--overwrite`; free-space check and `--max-output` before writing; atomic file writes.

### 6.2. Workstream B — coverage and usability

For **every** B item: (1) run the dev set *before*; (2) implement behind a flag when risky; (3) run *after*; (4) write the before/after table (with CI) into `docs/PROGRESS.md`. A feature that raises the main FP rate beyond the confidence interval must be fixed or ship **off by default**.

#### B-01 Metadata-assisted recovery [P1, MUST]
- **Why.** Today the tool ignores directory entries and MFT records (README limitation #2; a deliberate choice). Result: no names/dates/exact sizes, files in the Recycle Bin are missed, NTFS runlists (exact fragments) are unused.
- **FAT:** walk all directories (root, allocated, readable deleted ones); entries whose first byte is `0xE5`; rebuild long names (LFN attr `0x0F`, 8.3 checksum); `start_cluster = (hi<<16)|lo`, size, timestamps; first character overwritten by `0xE5` is inferred from the LFN or guessed. Assume contiguous allocation of `ceil(size/cluster)` clusters; if some are re-allocated mark `overwritten`/fragmented, try filling with following free clusters, **always confirm with the deep validator** (`source = hybrid`).
- **NTFS:** walk MFT records without `IN_USE`; read `$STANDARD_INFORMATION` (times), `$FILE_NAME` (name + `parent` → path), `$DATA` (resident ⇒ extract directly; non-resident ⇒ runlist ⇒ **exact `fragments[]` even for fragmented files**); recover only clusters that `$Bitmap` shows free, partially re-allocated ⇒ `partial`; compare sequence numbers; basic `$ATTRIBUTE_LIST`, `$MFTMirr` fallback (A-09); optionally Recycle Bin `$I`/`$R` pairs.
- **Constraint:** metadata is a **hint**, not truth — always validate content and record the source.
- **CLI:** `scan --mode metadata` and `--mode hybrid` (metadata first, then carve the free space **excluding** the recovered ranges; de-duplicate by hash and overlapping fragments); `--name-from-metadata`; report fields `source`, `original_name`, `original_path`, `mtime/ctime/atime`. (ADR-001: no separate `recover` verb; an alias is optional.) v3's lighter `--try-filenames` is covered by this item (notes only, "not guaranteed accurate").
- **Acceptance.** On D1 fragmentation/overwrite profiles `hybrid` recall ≥ carve-only and recovers fragmented files exactly via runlists; names/times correct against the truth; junk-FP does not rise beyond CI; comparison against `tsk_recover` when available.

#### B-02 Partition-table completeness [P1, MUST]
MBR extended/logical (EBR chain with loop protection); GPT: validate header CRC, use the backup header when the primary is damaged; honour sector size 512/4096 (A-08); `info` lists **all** partitions (type, LBA, size, detected FS). **Acceptance:** 2–3-partition images (FAT32+NTFS, extended) give the union of per-partition results; unit tests for corrupt tables and EBR cycles.

#### B-03 exFAT, FAT12/16 [P1, MUST for exFAT free-space, SHOULD for FAT12/16]
- **Why exFAT.** Default FS of large USB/SDXC media; CFReDS **RM#1 is a 4 GB exFAT USB** (E01, 74.5 MB) per `docs/CFREDS.md`.
- **Minimum:** exFAT boot sector (volume length, FAT offset/length, cluster-heap offset), root directory → Allocation Bitmap entry `0x81` → free extents (honour NoFatChain); then metadata (deleted directory entries `0x05/0x85`-family) as part of B-01. New `src/fs/exfat.rs` following the `fat32.rs`/`ntfs.rs` pattern and the `FsDriver` trait; `FsKind::ExFat`. FAT12/16 share the FAT reader with different entry widths.
- **Acceptance.** `carvex info` identifies exFAT on RM#1 or an `mkfs.exfat` image; `scan --mode unallocated` recovers deleted files; new benchmark scenarios `all-deleted_exfat`, `mixed_exfat`; fuzz target for the exFAT boot sector; README feature table updated. Missing tools ⇒ `SKIPPED`, never invented numbers.

#### B-04 Evidence formats [P1: E01 and split-raw SHOULD; VHD/VMDK, stdin MAY]
Through `ImageSource` (A-08): split raw (`.001/.002`, `.aa/.ab`), **E01/EWF** (a vetted crate or an optional `libewf` binding behind a feature; otherwise document `ewfmount`/`xmount` and mark SKIPPED), raw from stdin. Minimum for E01 even without a reader: `carvex info/scan` detect the EWF magic (`EVF\x09\x0d\x0a\xff\x00`) and print a clear instruction instead of "no FAT32/NTFS filesystem found". **Acceptance:** an E01 made from a benchmark image gives byte-identical carve output and a matching image hash.

#### B-05 Alignment [P2, MUST]
Files on FAT/NTFS start on cluster boundaries; scanning every byte offset generates FPs in compressed data (EXIF thumbnails, images inside PDFs) and wastes time (v2:F-17 evidence: 40 of Foremost's unknown FPs are JPEGs embedded in PDFs). Add `--align auto|cluster|sector|none|<N>`: `auto` = cluster when FS-aware (offset relative to the volume's cluster heap origin: FAT data-region start, NTFS volume start), sector (512 or detected size) in raw mode; `none` = today's behaviour (needed for embedded/shifted files and as an ablation arm). Report `aligned: bool`. NTFS resident data does not start on a cluster boundary — covered by B-01. **Acceptance:** D0 golden TP unchanged; fewer candidates/nested objects; no recall loss on D1; `--align none` is an arm in the ablation table.

#### B-06 Fragment handling [P1, SHOULD]
Staged strategy: (a) **detect** (A-04) → (b) **bifragment gap carving** → (c) metadata/runlist (B-01). BGC (§8.6) is bounded: `--max-gap`, `--max-frag-candidates`, `--frag-time-limit`, ≤ 4 fragments, cluster boundaries only. Confidence recorded; **never** `verified` without passing the deep validator. **Acceptance (D1 fragmentation profile, dev):** overall recall rises versus no-stitching with separated CIs; `splice_rate` and junk-FP do not rise beyond CI; success rate reported per fragment count.

#### B-07 Salvage and honest repair [P1, SHOULD]
- `--salvage`: JPEG — cut at the last valid marker/RST (or last decodable MCU row in `deep`); PNG — keep up to the last CRC-valid chunk, optionally re-chunk the partial IDAT (recompute length/CRC) and append IEND to build a partial image (`partial`, estimated `recovered_rows`); PDF — only if all earlier objects parse. All labelled `partial` with `validated_bytes`.
- `--repair` keeps its CLI meaning but **`repaired` is awarded only if the validator re-opens the repaired file** (PDF: rebuild xref+trailer from a scan of `N G obj` and `/Root`; if that fails keep `partial`). "Footer appended" ≠ "repaired". Report keeps `sha256_original` and `sha256_output`; a repaired file never counts as an `exact` result.
- **Why:** S7 — a PNG cut at 60 % is rejected entirely; v1 reports a "repaired" PDF (just `\n%%EOF\n` appended) that pypdf cannot open [REPORTED].
- **Acceptance.** S7 with `--salvage` ⇒ a decodable partial PNG; `repaired` PDFs open with pypdf (non-strict) or are not labelled `repaired`; byte-recall rises and junk-FP does not beyond CI on the overwrite profile.

#### B-08 Operations and UX [P1, MUST for progress/exit codes/hashes, SHOULD for the rest]
- Progress **for the carve phase** and an ETA (only the header phase has progress today, TTY only); `--json-lines` machine-readable events; `NO_COLOR`.
- Ctrl-C: clean stop, partial report, exit 4. `--resume state.json`: checkpoint per finished extent plus a hash of the effective options (refuse resume if options changed). `--dry-run`/`--list`: list candidates and expected sizes, write nothing.
- `carvex verify REPORT OUTDIR [--image IMG]`: re-hash outputs (and optionally the image) against the report; `carvex check FILE…` (run validators on arbitrary files) and `carvex extract IMAGE --offset N --type T` [SHOULD]; `report render --html` (sortable table with thumbnails) [MAY].
- Per-type output sub-directories and `--name-template` (default naming unchanged); `--hash sha256,md5,sha1`; `carvex completions <shell>` (`clap_complete`) and man page (`clap_mangen`); disk-space guard; every user error carries a remediation hint.
- CI test: README flag table equals `carvex --help`.
- **Acceptance.** Kill mid-run, then `--resume` ⇒ results identical (SHA-256) to an uninterrupted run; `verify` detects a modified output file.

#### B-09 Format-variant coverage [P2, SHOULD]
Each variant = one fixture + test; if unsupported, write an explicit `notes[]` entry instead of staying silent. JPEG: data after EOI (MPF/MPO, Motion Photo, vendor trailers), DNL, lossless/arithmetic, complex multi-scan. PNG: APNG (`acTL/fcTL/fdAT`), 16-bit, `eXIf`, `iCCP`, chunk ordering. PDF: xref streams + `/ObjStm` (currently only recognised), `/Encrypt`, header offset ≤ 1,024 B, hybrid-reference files. Publish a support table in the docs.

#### B-10 Custom signatures and new formats [P2: ZIP/OOXML SHOULD, rest MAY]
`--signatures sig.toml` (header, optional footer, min/max size, `reverse_footer`, `generic` kind without validator) on top of the registry (A-10) — also the vehicle for the Foremost-like ablation arm. New-format checklist: signature → bounds-checked parser → tests + fuzz target → D2 corpus entry → benchmark row → docs. Priority: **ZIP** (`PK\x03\x04` + central-directory check; DOCX/XLSX/PPTX are ZIPs; CFReDS RM#2 is Office-heavy), then GIF/BMP/WebP/TIFF/MP4/HEIC.

#### B-11 I/O performance [P2, MAY]
Reuse the header-phase chunk during carving (no double read); `posix_fadvise(SEQUENTIAL)` on Linux; optional `--io-threads`; `mmap` only if measured to win (record in `DECISIONS.md`). **Acceptance:** the time-versus-image-size curve (C-09) improves or does not worsen; peak RSS respects `--max-memory`.

### 6.3. Workstream C — benchmark science

#### C-01 Protocol, freeze, lockbox [P0, MUST]
Write `docs/BENCHMARK_PROTOCOL.md` **before** any new benchmark run: research questions (§10.1), hypotheses, metrics, datasets, per-tool configuration, statistics, threats to validity. Split every generated dataset into `dev` (tunable) and `lockbox` (single-use) by seed **and** by content hash (no file shared between them; verify by hash). Commit protocol + metric code + tool configs under tag **`protocol-v1`** before the first lockbox run. A lockbox is consumed once (log in `bench/lockbox.log`); if it was used to adjust anything it is burned — generate a new one with new seeds and say so in the report. Phases may consume a lockbox only for their end-of-phase report; the final release uses a never-consumed lockbox. **Acceptance:** the document exists and is tagged; every number in the docs traces back to the protocol.

#### C-02 Datasets [P0, MUST]
See §10.2. Builders take `--seed` and `--profile`, need no root where possible, and produce ground truth **independently of CarveX**. Builders self-check: actual fragmentation level, fragment counts and overwrite percentage must match the design (fail otherwise). `bench/make_dataset.py --profile dev --seed N` is reproducible (same seed ⇒ same manifest SHA-256). Where a tool is not deterministic (`mkfs` serials/UUIDs), **publish the images and their hashes** (release asset/Zenodo) instead of promising regeneration. Print corpus statistics (unique files, size and feature distributions) into the report.

#### C-03 Independent ground truth [P1, MUST]
Classify "valid file" with tools that are **not CarveX**: JPEG/PNG — Pillow `verify()` **and** `load()`, plus pixel/SSIM comparison against the original (S5 shows `load()` alone accepts a spliced JPEG); PDF — `qpdf --check` or `pypdf` + `pdftoppm`, plus text/page comparison. Fragmented-file truth comes from the **file-system chain/runlist before deletion**, read by an independent parser (`istat`/`fls` or a dedicated Python parser written for the bench), never from CarveX. Missing tool ⇒ `SKIPPED`, naming the affected metric.

#### C-04 Metrics [P1, MUST]
Definitions in Appendix C. Minimum: file-level `exact`, `usable` (with its definition: bytes after the true footer are removed before hashing), `decodes`, `visually_equal`, `viewable`; block-level `byte_recall`, `byte_precision`, `splice_rate`; **`false_complete_rate`**, **`bloat`**; FP taxonomy `junk | spliced | duplicate | kept_allocated | embedded` reported under two views (*deleted-only* and *any-real-file*) so that the FP definition does not favour FS-aware tools. Performance: wall, user+sys CPU, **peak RSS**, bytes read, ≥ 10 repetitions, cold and warm cache, 1/2/4/8 threads, scaling curve 64 MiB → 8 GiB; record CPU, RAM, kernel, I/O scheduler, governor. **Acceptance:** `bench/metrics.py` has unit tests (e.g., output = correct file + 1 cluster of another file ⇒ `splice_rate > 0`; output = correct embedded JPEG ⇒ `embedded`, not `junk`).

#### C-05 Statistics [P1, MUST]
Unit of analysis = **unique source file** (never the same file replicated across images): report `n_files_unique` and `n_images`. Wilson 95 % interval for every proportion; paired comparison between tools with exact McNemar and/or paired bootstrap (≥ 10,000 resamples over files); Holm correction; effect sizes. Timing: median, IQR, min/max, repetitions; no "faster" claim when intervals overlap. **Acceptance:** `bench/stats.py` with tests on synthetic data with known answers.

#### C-06 Baselines and ablation [P1, MUST]
See §10.3–§10.4. Each tool gets `bench/tools/<tool>.md` (best-effort configuration, rationale, version). Two search spaces for **every** tool: whole image, and unallocated-only (extract with `blkls`, run the tool on the blob, or the tool's own free-space option). Configurations are fixed in C-01 and **not tuned after results**. **Acceptance:** one command runs the (tool × search space × dataset) matrix; cells without the tool say `SKIPPED (reason)`.

#### C-07 Runner, reproducibility, versions [P1, MUST]
Remove hard-coded paths (`/home/kali/...` in docs **and** in `results.json`); configuration through `bench/config.toml`. Every run records git commit, SHA-256 of the CarveX binary, **version string and SHA-256 of every external binary** (today only version strings are recorded), host, full argv, seeds → immutable `results/<date>-<commit>/` with `SHA256SUMS`. Provide a pinned Dockerfile (Foremost/Scalpel/TestDisk versions) and `make bench-small` (< 5 min, CI) / `make bench-full`; `--smoke` (dev set, < 60 s) and `--full`. Risk reminder (reported, unverified): PhotoRec output changed between 7.1 and 7.2 (thumbnail splitting) — a version string is not enough for reproducibility. `bench/render_docs.py` renders tables/figures (PR curve, time vs size; SVG) into `<!-- BENCH:<id> -->` markers and CI fails if the docs diverge from `results.json`. **Acceptance:** re-running `--full` on the same commit yields identical file-level results (performance may vary within its CI).

#### C-08 Real data [P0, MUST — partly done]
- **Done:** CFReDS RM#2 (FAT32, 4 GB DD): hashes match NIST; `unallocated` and `raw` runs; 318 files, Pillow `verify()` on all, 276 unique SHA-256, ≥ 178 are media embedded in DOCX/PPTX; no PDFs; no ground truth ⇒ structural validation only.
- **To do:** (1) re-run with the fixed tool and `deep` validation; apply `load()` + pixel checks (not just `verify()`); explain the 42 duplicate pairs; (2) obtain per-file ground truth where the case documentation allows it, otherwise keep the manual protocol (`docs/CFREDS.md §6.3`) and state the limits; (3) **RM#1 (exFAT, E01)** after B-03/B-04; (4) DFRWS 2006/2007 carving challenges (verify that the source still exists; without network ⇒ `NOT RUN: <reason>`, keep the script, **no numbers**); (5) run the baseline tools on the same images; (6) a small self-acquired USB/SD image (consent, no sensitive data). Never fill `CFREDS.md` with numbers that were not produced.

#### C-09 Performance measurement and scale [P1, SHOULD]
Add at least one 1–4 GiB image (synthetic is fine) and a sparse 8–16 GiB image for scaling; ≥ 10 repetitions with cold cache (`posix_fadvise(DONTNEED)` or drop caches where permitted) **and** warm; report median + IQR for throughput (the runner already warns about variance — print it); include A-06 stress inputs (flood, noisy overwritten free space) so regressions surface before users hit them. **Acceptance:** `BENCHMARK.md` has a table per scenario; the report cites it rather than listing limitations qualitatively.

### 6.4. Workstream D — documentation and academic content

#### D-01 Factual hygiene [P1, MUST]
Remove `/home/kali/...` (BAO_CAO L11, L633; CFREDS L63, L98, L123, L136, L224–L231); fix "Pillow/ReportLab" (BAO_CAO L341: ReportLab is not used); state that the Windows `.exe` is untested until A-12 runs it; make versions/dates consistent; number tables/figures; list tool **versions and hashes**; update the Limitations section from measured results (do not keep a sentence that was fixed; do not delete one that was not); fix README "Rust 1.74"; no "100 %" without type, n, CI and conditions (§10.10). Replace the BAO_CAO §6.2 explanation by the evidenced one: most Foremost/Scalpel "unknown" FPs are JPEG streams embedded in the corpus PDFs (40 of 44 / 40 of 54), a boundary/embedded-object effect rather than only trailing padding.

#### D-02 Academic content [P1, MUST]
See §11.

#### D-03 Release and process docs [P2, MUST for 0.2.0]
`CHANGELOG.md` (Keep a Changelog, with *Changed*/*Behavior changes*), `SECURITY.md`, `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md` (ADRs), `docs/DEPENDENCIES.md`, `docs/REVIEW_RESPONSE.md` (table of every ID ⇒ commit / test / measurement / status `FIXED|MITIGATED|WONTFIX(reason)|OPEN` — open items are listed, not hidden).

---

## 7. Phased plan

Order: **P0 baseline → P1 correctness → P2 engine + benchmark infrastructure → P3 coverage/usability (measured on dev) → P4 evaluation and documentation → P5 release.** Benchmark infrastructure precedes feature work so every improvement is measured with the same ruler and nothing is tuned on held-out data. Commits: Conventional Commits in English with the item ID, e.g. `fix(pdf): follow incremental updates past the read window [A-01]`; one commit = one logical change + its tests + PROGRESS update; never mix refactors with behaviour changes.

### Phase 0 — Baseline and safety net (no intended behaviour change)
| Task | Content | Done when |
|---|---|---|
| P0.1 | Build from source, `cargo test`, `clippy`, `fmt` (A-11/A-12); create `scripts/gate.sh`; record the baseline numbers (v3's "6 clippy warnings / 18 tests" are claims to check) | numbers in PROGRESS; gate script runs |
| P0.2 | **MSRV truth** (A-12): set `rust-version = "1.85"` (or re-pin), fix README, add `scripts/check-msrv.sh` | G3 green |
| P0.3 | Repro harness `scripts/review_repro.py` + Rust regression tests for **every Appendix A scenario**; run against the fresh binary, compare with the "observed" column (mismatch ⇒ STOP-AND-ASK); save red output to `docs/baseline/repro_v0.1.0.txt`; create `scripts/repro_xfail.txt` | gate green via xfail |
| P0.4 | Freeze `bench/results/final/` → `bench/results/baseline-v0.1/` (read-only + `SHA256SUMS`); write `scripts/golden.py` (byte-level D0 comparison) | G7 defined |
| P0.5 | `lib.rs`/`main.rs` split as a **pure move** | G7 byte-identical; `cargo doc` clean |
| P0.6 | CI skeleton (matrix, fmt/clippy/test/doc/MSRV/deny/audit); `rustfmt.toml`, `clippy.toml`, `deny.toml`, `rust-toolchain.toml` | G0–G4, G6 green |
| P0.7 | Fuzz skeleton for the 7 targets; `panic = "unwind"` + `catch_unwind` plumbing (the OOB fix itself is A-03 in Phase 1) | G5 smoke green |
| P0.8 | Behaviour-neutral clean-ups of A-10 (hacks, test placement, constants) | G1, G7 green |

### Phase 1 — Correct, honest, safe (P0/P1 findings)
Order: **A-07 step 1 → A-05 → A-01 → A-02 → A-03 → A-06 → A-04 → A-08.** (Honesty/data-loss first; panic and cost next; `deep` validation after the incremental/streaming reader exists so it does not amplify A-06.)
**Gate at the end:** G0–G9 green; `scripts/repro_xfail.txt` is **empty**; G7 unchanged; README/BAO_CAO limitation sections match behaviour (D-01).

### Phase 2 — Engine, hygiene, benchmark infrastructure (two tracks)
- **Track E (engine):** A-10 (pipeline, registry, parameter structs, clippy pedantic), A-13, A-14, A-15, A-09, coverage targets of A-11.
- **Track C (benchmark):** C-01 → C-02 → C-03 → C-04 → C-05 → C-06 → C-07 → C-09; D-01 factual fixes.
- **Rule:** no Phase 3 feature is merged before C-01 is tagged and the dev runner works.
**Gate:** protocol tagged `protocol-v1`; `--smoke` runs in < 60 s in CI; schema v2 + JSON Schema test; coverage targets met or explained; ablation harness runs (flags `--align`, `--validate`, strategy toggles).

### Phase 3 — Coverage and usability (each item measured before/after on dev)
Suggested order (value/cost): **B-02 → B-01 (FAT, then NTFS) → B-05 → B-03 (exFAT) → B-07 → B-06 → B-08 → B-04 → B-09 → B-10 → B-11.** Then A-07 step 2 once the user decides (§3.5).
**Gate:** every item has its before/after table (with CI) in PROGRESS; no unexplained rise of the main FP rate; support table in the docs; B-items not done are marked `OPEN` with reason.

### Phase 4 — Evaluation and documentation
C-08 (real data), **one** lockbox run (C-01), D-02, finalise D-01; all numbers rendered from `results.json` (G9). Never change code based on lockbox results; if a real bug is found, fix it, generate a **new** lockbox and say so.

### Phase 5 — Release
Full gates; fuzz 30 min/target; `bench --full`; `CHANGELOG`; bump to `0.2.0`; tag; CI builds artifacts + `SHA256SUMS`; `docs/REVIEW_RESPONSE.md` (D-03).
**Definition of Done for 0.2.0:**
- [ ] A-01…A-07 `FIXED` with green regression tests; `review_repro.py --assert` green with an empty xfail list.
- [ ] No unchecked indexing/`unwrap` on image data in `fs/` and `signatures/`; fuzz clean (≥ 30 min/target).
- [ ] lib + bin, `ImageSource`, registry; no `too_many_arguments`.
- [ ] Report schema v2 with hashes, options, commit, JSON Schema + test; legacy fields intact.
- [ ] CI green (Linux/Windows), MSRV truthful, deny/audit clean.
- [ ] Multi-volume default decided; partitions/gaps never silent; exFAT/FAT16, block devices (or `OPEN` with reason), metadata recovery, alignment delivered or `OPEN`.
- [ ] Protocol tagged before the lockbox; lockbox result immutable; fair baselines + ablation; CI on every proportion; no hand-typed numbers.
- [ ] `CFREDS.md` and report contain only produced results or "NOT RUN: <reason>".
- [ ] Report has verified related work, RQs, threats to validity; no factual errors left.
- [ ] `REVIEW_RESPONSE.md` maps every ID.

**v1.0 criteria (not required for 0.2.0):** 0 open P0/P1; fuzz ≥ 24 h cumulative per target without crash; Windows tests green; benchmark v2 published with data; complete user documentation.

---

## 8. Target design (guidance; the agent may refine it — record each deviation as an ADR)

### 8.1. Module layout
```text
src/lib.rs
src/io/        ImageSource trait: RawFile, BlockDevice, SplitRaw, Ewf (feature); sector size; read-error policy
src/volume/    mbr.rs gpt.rs (EBR chain, CRC, backup) · VolumeSet { volumes, gaps, warnings }
src/fs/        FsDriver trait: probe(), free_extents(), deleted_entries() -> Vec<DeletedEntry>
               fat32.rs fat16.rs ntfs.rs exfat.rs
src/format/    Carver trait + Registry · jpeg.rs png.rs pdf.rs (+ zip.rs, gif.rs later) · userdef.rs
src/engine/    pipeline.rs · candidates.rs (multi-pattern search, alignment) · assemble.rs (contiguous | BGC | metadata)
               filter.rs · budget.rs · progress.rs
src/report/    schema.rs (v2) · json.rs · csv.rs · html.rs
src/cli/       info.rs scan.rs verify.rs check.rs extract.rs report.rs signatures.rs completions.rs
```

### 8.2. Carver (streaming) — sketch
```rust
pub trait Carver: Send + Sync {
    fn id(&self) -> FormatId;                          // "jpeg"
    fn extensions(&self) -> &'static [&'static str];
    fn header_patterns(&self) -> &'static [Pattern];   // bytes + offset + alignment hint
    fn quick_reject(&self, head: &[u8]) -> bool;       // <= 64 bytes, cheap
    fn begin(&self, ctx: &AnalyzeCtx) -> Box<dyn Analyzer>;
}
pub trait Analyzer {
    fn feed(&mut self, data: &[u8], at_limit: bool) -> Step;   // NeedMore | Done(Analysis) | Reject(reason)
}
pub struct Analysis {
    pub end: u64,                 // last verified byte
    pub validity: Validity,       // Verified | Structural | Suspect | Partial | Repaired
    pub checks: Vec<CheckResult>, // explainable pass/fail per check
    pub dims: Option<(u32, u32)>, pub notes: Vec<Note>, pub tail_trimmed: u64,
}
```
`AnalyzeCtx` carries `cluster_size` (fragmentation breakpoints are cluster-aligned), resource limits and the `deep` flag. `Analyzer` never re-parses from byte 0 when more data arrives. File systems implement `FsDriver`; sources implement `ImageSource` (A-08).

### 8.3. State model
```text
validity : verified | structural | suspect | partial | repaired
source   : carved | metadata | hybrid          (hybrid = metadata locates, validator confirms content)
fragments: [{offset, len}]                      (>= 1; > 1 means stitched)
legacy `status` (complete|truncated|repaired) stays in reports as "footer found" for compatibility
```
**Hard rule:** only `verified` may appear in a "complete" column of any report or benchmark; `structural` must be written as "content not checked".

### 8.4. Report schema v2 (additions only; existing field types never change)
```json
{
  "schema_version": 2,
  "tool": "CarveX 0.2.0",
  "tool_info": {"name": "carvex", "version": "…", "commit": "…", "rustc": "…"},
  "argv": ["carvex", "scan", "…"], "options": {"…": "…"},
  "started_at_unix": 0, "started_at": "2026-11-18T00:00:00Z", "duration_ms": 0,
  "image": "…", "image_size": 0,
  "input": {"sector_size": 512, "sha256_before": "…", "sha256_after": "…"},
  "case_id": null, "examiner": null,
  "filesystem": {"…": "legacy FsInfo, unchanged"},
  "volumes": [{"index": 1, "fs": "fat32", "offset": 0, "length": 0, "cluster_size": 0, "free_bytes": 0}],
  "regions": [{"kind": "free|allocated|gap|unsupported", "offset": 0, "len": 0, "scanned": true}],
  "warnings": [{"code": "VOLUME_NOT_SCANNED", "message": "…"}],
  "unreadable_regions": [],
  "stats": {"…": "legacy counters + new ones"},
  "files": [{
    "seq": 1, "type": "jpeg", "offset": 0, "size": 0, "carved_size": 0, "sha256": "…",
    "status": "complete", "score": 100, "width": 0, "height": 0, "repaired": false, "path": "jpg/00001_….jpg", "notes": [],
    "validity": "verified", "source": "carved", "validated_bytes": 0, "tail_trimmed_bytes": 0,
    "fragments": [{"offset": 0, "len": 0}], "partition_index": 1, "lba": 0, "cluster": 0, "extent_id": 0,
    "allocated": false, "aligned": true, "checks": [{"name": "rst_sequence", "ok": true}],
    "original_name": null, "mtime": null, "nested_in": null
  }]
}
```
(`path` keeps today's flat naming unless `--name-template`/per-type directories are selected.)

### 8.5. Metadata recovery
Specified in B-01. Hybrid pipeline: metadata pass first → carve the free space excluding recovered ranges → de-duplicate by `fragments` overlap and hash → every result validated.

### 8.6. Bifragment gap carving (B-06)
Find head fragments (valid header, validator fails at cluster `k`) and tail fragments (valid footer); try `head[0..k] + gap(g clusters) + tail` with `g ≤ --max-gap`, scored incrementally by the deep validator, ordered by increasing distance, stopping at the first `verified` stitch. Only at volume cluster boundaries; bounded by `--max-frag-candidates`/`--frag-time-limit` to prevent combinatorial blow-up. Report `fragments`, `checks`, `confidence`. References (verify before citing): Garfinkel 2007 (BGC); Pal & Memon 2009; Pal–Sencar–Memon 2008 (sequential hypothesis testing); Sencar & Memon 2009 (JPEG with missing fragments); SmartCarving as an optional extension.

### 8.7. CLI surface and exit codes
- **Existing (unchanged):** `info`, `scan`, `signatures` and all current flags (README table).
- **New `scan` flags:** `--partition all|N|largest`, `--include-gaps`, `--mode metadata|hybrid`, `--align`, `--validate none|structure|deep`, `--salvage`, `--max-candidates`, `--time-budget`, `--max-work-per-candidate`, `--max-memory`, `--max-truncated-size`, `--max-output`, `--on-read-error`, `--sector-size`, `--hash-image`, `--hash`, `--case-id`, `--examiner`, `--dry-run`/`--list`, `--resume`, `--json-lines`, `--html-report`, `--name-template`, `--name-from-metadata`, `--signatures`, `--overwrite`, `--report-schema`.
- **New commands:** `verify REPORT OUTDIR [--image IMG]`, `check FILE…`, `extract IMAGE --offset N --type T`, `report render`, `completions <shell>`.
- **Exit codes:** `0` OK · `1` fatal · `2` usage · `3` finished but incomplete · `4` interrupted with partial report.

---

## 9. Test strategy

| Layer | Tooling | Content |
|---|---|---|
| Unit | `cargo test` | every parser function; boundary tables (0/1/max/overflow) |
| Property | `proptest` | `merge/complement` extents (union + complement = whole, disjoint); NTFS runlist round-trip; parsers never panic on arbitrary bytes; cutting a valid file anywhere never yields `verified`; splice property of A-04 |
| Integration | `tests/*.rs`, `assert_cmd` | FAT32/NTFS images built **in Rust** in `tests/support/` (FAT32 writer: BPB, 2 FATs, arbitrary allocation, directories + LFN, deletion marking; cluster 512 B … 32 KiB); NTFS: a minimal writer or `mkntfs`/`ntfs-3g` generated compressed fixtures committed with hash and the generating command in `tests/fixtures/README.md`; CLI exit codes and messages |
| Regression | Appendix A | every finding has ≥ 1 red→green test, `// <ID>` in the file header |
| Golden | `scripts/golden.py` | byte-exact D0 outputs (G7) |
| Fuzz | `cargo-fuzz` | targets of A-03, seeds from `bench/` |
| Snapshot | `insta` or golden files | report JSON/CSV |
| Coverage | `cargo llvm-cov` | targets in A-11 |

**Scenario matrix (mandatory fixtures):** contiguous / 2-fragment / n-fragment · cluster re-allocated in the middle · tail zero / random / 0xFF · multiple volumes · unsupported volume · damaged boot sector · PDF multi-revision > 1 MiB / linearized / embedded PDF / encrypted · JPEG with EXIF thumbnail / MPO / progressive / CMYK / DRI · PNG APNG / interlaced / 16-bit / bad CRC · header flood · 0-byte / 1-byte image / image cut inside the BPB.

**Per finding:** (1) a regression test for the *correct* behaviour, red before the fix; (2) a new fuzz seed when a parser is involved; (3) a micro-benchmark or budget test when performance is involved; (4) docs + `CHANGELOG`.

---

## 10. Benchmark v2 — scientific design

### 10.1. Research questions (go into the protocol and the report)
- **RQ1** Do structural and deep validation reduce false positives and *false-complete* outputs relative to plain header/footer carving, at what cost, and without losing recall?
- **RQ2** How much does FS-awareness (unallocated-only scanning + cluster alignment) contribute, *separated* from carving-algorithm quality, with and without fragmentation — and does it lose recall (e.g., deleted-to-Recycle-Bin files that are still allocated)?
- **RQ3** What do alignment, bifragment stitching and metadata recovery add on fragmented or partly overwritten files?
- **RQ4** How does performance scale with image size and thread count, and how does it behave on adversarial/noisy input?
- **RQ5** What does metadata recovery add over carving, and **when is carving unnecessary** (contiguous, not overwritten files, where TSK-style tools may reach very high recall — report this honestly)?

### 10.2. Data
| Set | Description | Role |
|---|---|---|
| **D0 sanity** | the current 4 images (30 distinct files × 4 contexts) | regression only; **not** evidence of quality |
| **D1 controlled synthetic** | deterministic, no root where possible. Axes: FS ∈ {FAT32 (cluster 4 KiB and 32 KiB; 512 B only for legacy parity), NTFS 4 KiB, exFAT} × fragmentation ∈ {0 %, 10 %, 30 % two-fragment files, 5 % ≥ 3 fragments} × overwrite ∈ {0, 10, 30, 60 % of freed clusters re-allocated} × free-space fill ∈ {zero, random, remnants of old files} × fill level ∈ {20, 50, 80 %} × wipe ∈ {delete, quick-format, partition delete} × size ∈ {64 MiB, 1 GiB, 8–16 GiB sparse for performance}. Fragmentation comes from **file-system ageing** (repeated create/delete/append with a size distribution) or from the filler/hole method of C-02, not only from manual cuts | RQ1–RQ3 |
| **D2 realistic corpus** | ≥ 300 files per type: real photos (EXIF, ICC, 100 KB–20 MB), public PNGs, PDFs from varied producers (LaTeX, Word, Skia/Chrome, scans; linearized; incremental; encrypted; ≥ 10 MiB). Public, clearly-licensed sources only (Digital Corpora/govdocs1, Wikimedia Commons PD). **The agent must verify each URL, licence and hash before use — never invent a source.** Manifest feature tags: JPEG (baseline/progressive/DRI/CMYK/EXIF-thumb/MPF), PDF (version, xref table/stream, linearized, encrypted, embedded images/PDFs), PNG (colour type, depth 1–16, interlaced, APNG) | RQ1–RQ3 |
| **D3 public forensic images** | DFRWS 2006/2007 carving challenges; NIST CFReDS (RM#2 done; RM#1 exFAT pending); a small self-acquired USB/SD image (consent, no sensitive data) | external validity |
| **D4 adversarial / negative** | header flood, `%%EOF` flood, zero-fill with stale headers, PNG decompression bomb, deeply nested PDFs, images with many header-lookalikes, ZIP/Office containing JPEGs, MPF/Ultra HDR, thumbnail caches | RQ4, FP rate |

**Splitting.** Every generated D1/D2/D4 set has a `dev` part (tunable; e.g., seeds 1–20) and a `lockbox` part (e.g., seeds 1001–1020) with **disjoint content hashes**. Manifests record: per-file SHA-256, feature tags, the **fragment list `(LBA, length)`** at creation, and the allocation state at scoring time. Current corpus facts to remember: synthetic noise-plus-gradient images; PDFs ≤ 650 KB, mostly xref-table 1.4, no linearized/encrypted; JPEG without DRI; images 21 % full; FAT32 cluster 512 B.

### 10.3. Baselines (equivalent configuration, exact command published)
- **CarveX** ablation arms (§10.4).
- **PhotoRec:** run **both** whole-space and free-space (`/cmd … freespace`/`wholespace`; verify what the installed version supports; otherwise use the unallocated blob). Today's run is whole-space only, so it is not comparable to CarveX `unallocated`. Add the `paranoid`/`keep_corrupted_file` variant.
- **Foremost:** configuration file only (no `-t`, to avoid duplicated signatures), generic JPEG header `\xff\xd8\xff`, `REVERSE` for PDF, plus `-q` variant. **Scalpel:** `REVERSE`/`NEXT` for PDF and per-type max size (today `pdf y 20000000 %PDF- %%EOF`, 20 MB). Default *and* tuned configuration for both.
- **The Sleuth Kit** (`fls -rd` + `icat`, `tsk_recover`, `blkls | carver`) and **`ntfsundelete`:** metadata-based reference, required to answer "is carving even needed here?".
- Every tool runs with `--threads 1` **and** with maximum threads (CarveX defaults to all CPUs while Foremost/Scalpel are single-threaded); two search spaces: whole image and unallocated-only. Configurations are frozen in C-01.

### 10.4. Ablation (the scientific core of "Header/Footer")
Ladder: `A0 header+footer only` → `A1 header+max-size` → `A2 +structure parser` → `A3 +deep validation` → `A4 +cluster alignment` → `A5 +FS-aware (free space)` → `A6 +BGC` → `A7 +metadata`. Also leave-one-out arms (no validator, no nested suppression, `--align none`) and a **precision–recall curve over `--min-score`**. Report the marginal contribution of each step by (FS, fragmentation, overwrite).

### 10.5. Metrics
Appendix C. Both views of FP (*deleted-only* and *any-real-file*). Decoding success alone is not a quality criterion (S5).

### 10.6. Performance
Wall, CPU, peak RSS, bytes read; cold and warm cache; ≥ 10 repetitions (`hyperfine` or equivalent); scale 1/4/16/64 GiB where feasible and 64 MiB → 8 GiB at minimum; thread scaling; adversarial inputs (A-06).

### 10.7. Statistics
Analysis unit = unique source file. **30 distinct files in four contexts are not n = 100.** Wilson 95 %: 30/30 ⇒ [88.6 %, 100 %]; even 100/100 independent ⇒ lower bound 96.3 %, so "100 %" is not proof of an absolute level. Pairs of tools: exact McNemar or paired bootstrap resampling **files** (≥ 10,000), Holm correction, effect size. Report negative results and where CarveX loses.

### 10.8. Reproducibility
Pinned Dockerfile; `make bench-small` (CI, < 5 min) and `make bench-full`; no root (FAT through userspace writer/`mtools`; NTFS through `mkntfs` + `ntfs-3g`/FUSE in a container); publish image + hash when generation is not deterministic; provenance: commit, tool versions **and hashes**, CPU/RAM/disk, kernel, seed, timestamp.

### 10.9. Auto-generated documentation
`bench/render_docs.py` reads `results.json` and writes Markdown/LaTeX/CSV tables plus SVG figures into `<!-- BENCH:begin name -->…<!-- BENCH:end -->` markers in README/BENCHMARK/BAO_CAO. CI fails when documentation numbers differ from the latest `results.json` (G9).

### 10.10. Interpretation rules for the documents
- Call D0 the *sanity benchmark*; quote "100 %" only with type of data, n (unique files), CI and conditions (contiguous, not overwritten, noise-free free space, FAT32 512 B clusters, 64 MiB images).
- Separate *measured* from *expected*. Never write "superior" when baselines are not configured equivalently.
- Every limitation (§6, README list) has a matching entry under *Threats to validity*.

---

## 11. Upgrading the academic report (`docs/BAO_CAO.md`, Vietnamese)

Add without deleting correct content:
1. **Related work** with a technique taxonomy: header/footer; header/max-size; structure/validation-based; content-based; fragment-aware (BGC, SHT); metadata-assisted — and CarveX's place in it. Compare CarveX with Foremost/Scalpel/PhotoRec/TSK feature-by-feature.
2. **Problem statement and hypotheses** (RQ1–RQ5) with formal definitions: candidate, extent, validator, fragment, error types (FP, FN, false-complete).
3. **Complexity and resources:** scan O(N); analysis O(Σ check lengths); worst case and budgets (A-06); memory.
4. **Method** per the protocol (C-01); **Results** rendered from `results.json`; **ablation** and qualitative failure analysis (a catalogue of failure cases with examples such as S1–S9); **Threats to validity** (internal, external, construct, conclusion) and **Limitations**; a reproducibility appendix (commands, seeds, commit, checksums).
5. **Ethics/legal:** chain of custody, image hashing, read-only access, test-data provenance.
6. **Hygiene:** D-01 list; numbered tables/figures; references with DOI/URL that were actually opened.
7. **Citations: the agent must verify each one (authors, year, venue, DOI) on the publisher/DOI page before adding it; never cite from memory; when unverifiable write "to be added".** Candidate list in Appendix F.

---

## 12. Agent workflow

### 12.1. Loop per item
```text
1. Read the item; run its Appendix A scenario → confirm it reproduces (record in PROGRESS)
2. Write the failing test (correct behaviour) — red
3. Minimal fix; no refactoring outside the item's scope
4. Run scripts/gate.sh (fmt/clippy/test/doc/repro/golden/smoke)
5. Update docs + CHANGELOG; remove the xfail entry/#[ignore = "<ID> xfail"]
6. Commit: `fix(scope): … [ID]`
7. Update docs/PROGRESS.md with the per-task report (§4.3)
```

### 12.2. When to stop and ask
Only the cases in §3.5. Otherwise decide conservatively, write the decision to `docs/DECISIONS.md`, continue.

### 12.3. End-of-phase report (submit to the user)
1. Closed items + commits. 2. Gate table G0–G9 (green/red + log). 3. Before/after numbers (benchmark/perf), **auto-generated**. 4. New findings and residual risks. 5. Proposal for the next phase.

---

## Appendix A — Repro scenarios (build specs and observed values)

The reviewers' scripts are **not in the repository**; implement these in `scripts/review_repro.py` and as Rust tests (A-11). "Observed" = v0.1.0 `dist/` binary on a 1-vCPU sandbox. Image = 4,096 zero bytes + payload + 64 KiB zeros unless stated; run `scan IMG -o OUT --mode raw --no-fs --quiet --json-report R.json` for payloads, `scan IMG -o OUT` for disks.

| ID | Build | v0.1.0 observed | Expected after fix | Item | Evidence |
|---|---|---|---|---|---|
| **S1** multipart | MBR disk, 1 MiB gap, partition 1 FAT32 (type 0x0C, 126,976 sectors) = bytes of `all-deleted_fat32.img` from 1 MiB, partition 2 NTFS (0x07, 126,976 sectors) from `all-deleted_ntfs.img`; 131,072,000 B total | auto: **30/60** files, `bytes_scanned` 63,990,784, exit 0, only the FAT32 volume announced; `--mode raw --no-fs`: 60 | `--partition all`: 60/60 + `regions[]` with 2 volumes; default: 30 + WARNING + exit 3 (until step 2) | A-07 | REPRODUCED |
| **S2** incremental PDF | rev1: 3 objects (329 B, `%%EOF` at 329) + update adding a 1,500,000-byte stream object, xref (`/Prev`), trailer, `startxref`, `%%EOF`; total 1,500,481 B | 1 output, **329 B**, `complete`, score 100 | 1 output = full 1,500,481 B | A-01 | REPRODUCED |
| **S2c** control | same shape but rev1 stream 1.2 MB / 2.0 MB so the first `%%EOF` is already past 1 MiB; and a 0.4 MB two-revision PDF | carved whole (420,000 / 1,258,865 / 2,097,726 B) | must stay whole | A-01 | REPRODUCED |
| **S3** linearized-style | `%PDF-1.5`, linearization dict, first-page xref + trailer with `startxref 0` + `%%EOF` at ~300 B, 1,500,000-byte stream, main xref/trailer/startxref/`%%EOF`; 1,500,583 B. Add a real `qpdf --linearize` fixture > 1 MiB when `qpdf` exists | 1 output, **312 B**, `complete`, score 75 | full file | A-01 | REPRODUCED |
| **S4** nested PDF | outer PDF with a 300-line content stream + an uncompressed EmbeddedFile stream holding `bench/corpus/pdf/pdf_03_text_2pages.pdf` (6,211 B); outer 17,828 B. Variant: outer 6,633 B with 240 B before the inner header | 2 outputs: **11,414 B `truncated`** score 60 + 6,211 B `complete`. Variant: only the inner file (outer dropped by the default `--min-size 256`, `below_min_size = 1`) | 1 output = 17,828 B; inner `nested_in` | A-02 | REPRODUCED |
| **S5** spliced JPEG | two 640×480 baseline JPEGs from noise (Pillow q=90, seeds 1 and 2; 275,885 B / 276,179 B); payload = first 16,384 B of the second + bytes ≥ 16,384 of the first (275,885 B) | `complete`, score 100, SHA ≠ both; Pillow `load()` OK, mean abs diff vs original 60.3 | `validity ≠ verified` (suspect/partial) | A-04 | REPRODUCED |
| **S6** truncated JPEG | `jpeg_10_progressive_1200x800.jpg` cut at 60 % (212,468 B) + 48 MiB zeros | 1 output **50,544,116 B**, `truncated`, score 60 | size ≤ 212,468 + 1 cluster; `tail_trimmed_bytes` reported | A-05 | REPRODUCED |
| **S7** truncated PNG | `png_02_rgb_800x600.png` (1,398,788 B) cut at 60 % (839,272 B) + 1 MiB zeros | 0 outputs, `rejected = 1` | default: documented; `--salvage`: decodable `partial` PNG | B-07 | REPRODUCED |
| **S8** NTFS record OOB | copy of `all-deleted_ntfs.img`; in MFT record 0 (1,024 B) set the first-attribute offset (`+0x14`) to 1,000 and write at 1,000: type `0x80`, length `0x18`, non-resident = 1, name_len = 0 | `info` and `scan`: **abort**, `panicked at src/fs/ntfs.rs:298:51: index out of bounds: the len is 1024 but the index is 1032` (SIGABRT) | no abort; `info` diagnostic; `scan` auto → raw + WARNING + exit 3 | A-03 | REPRODUCED |
| **S8b** control | same image, `$DATA` run offset (`attr+0x20`) set to `0xFFFF` | `info` exits 1 (`runlist offset outside attribute`); `scan` completes without crashing | unchanged | A-03 | REPRODUCED |
| **S9** flood16 | repeat the 16-byte unit `FF D8 FF E0` + 12 × `A` to 256 KiB / 1 MiB / 4 MiB (`python3 -c "open('f.img','wb').write((b'\xff\xd8\xff\xe0'+b'A'*12)*(N//16))"`) | 1 MiB (65,536 candidates): **9.9 s**; 4 MiB (262,144): **74.8 s**; 64,492/65,536 rejected with `expected JPEG marker at offset 16709`; 0 files. (v3 on its machine: 1.77 s / 6.19 s / 16.34 s for 1/2/4 MiB — same shape) | 256 KiB < 1 s; 4 MiB ≤ 3 s; 0 files | A-06 | REPRODUCED |
| **S10** flood512 | 64 MiB with a JFIF-like header every 512 B (131,074 candidates); also every 4,096 B (16,390); random data | 44.08 s / 5.57 s / 0.05 s | flood512 ≤ 3 s, ≤ 256 MiB RSS; random not > 10 % slower | A-06 | REPORTED (v1) |
| **S11** fake headers | 25,811 fake headers in 64 MiB | 7.9 s (~300 µs/candidate) | ≤ 1 s | A-06 | REPORTED (v2) |
| **S12** interleaved fragments | FAT32 ~280 MiB, deleted files `[A1][B1][A2][B2]` for JPEG, PDF, PNG | JPEG: 1 output 116,218 B `complete` score 100 (head of B + tail of A); PDF: `truncated` 77,824 B + `complete` score 75 (451,919 B), neither is A or B; PNG: 0 files (CRC catches it) | no `verified` output whose SHA ∉ {A,B}; with B-06: A and B recovered exactly | A-04, B-06 | REPORTED (v1) |
| **S13** big PDF | two-revision PDFs of 0.4 / 1.9 / 2.5 MB | 0.4 MB right; 1.9 MB → 717,222 B; 2.5 MB → 2,048,424 B | all whole | A-01 | REPORTED (v1); consistent with S2 |
| **S14** device size | `util.rs:128–133` uses `metadata().len()`; v1 used `/dev/zero` as a proxy (`image … is empty`) | block device ⇒ size 0 | mock source with `len()` ≠ `metadata()` works | A-08 | CONFIRMED-STATIC |
| **S15** fake repair | v1: PDF cut then `--repair` appends `\n%%EOF\n` | labelled `repaired`, pypdf cannot open | `repaired` only if re-validated | B-07 | REPORTED (v1) |

---

## Appendix B — PDF end-detection algorithm (A-01)

Input: `data` (buffer), `at_limit` (reached `min(max_size, end of extent)`), EOF mode.

```text
eofs  = all positions of "%%EOF" in data
bound = first "%PDF-" at offset > 0 whose preceding bytes, ignoring whitespace/NUL, END with "%%EOF"
        (a header inside a stream or object is NOT a boundary)
eofs  = eofs ∩ [0, bound)
if eofs is empty:
    return at_limit ? Partial(end = last verified byte) : Need::More

e = EOF chosen by the current rule (last EOF with a valid startxref; otherwise the last EOF)
loop (bounded iterations and bytes):
    after = skip EOL ("\r\n"|"\n"|"\r") then whitespace after data[e+5 ..]
    if len(after) < 32 and !at_limit:        return Need::More          # cannot decide yet
    if after is empty:                       return Complete(end = e+5+EOL)
    if after matches /^\d+\s+\d+\s+obj/ | "xref" | "trailer":
        # more revisions follow (incremental update, or linearized: %%EOF of the first-page section)
        e' = next EOF in data (< bound)
        if e' exists: e = e'; continue
        if !at_limit:                        return Need::More
        return Partial(...)                  # chain continues beyond the limit: never `verified`
    else:
        return Complete(end = e+5+EOL)       # what follows is not PDF continuation (zeros, other signature, junk)
```
Notes: bound the iterations/bytes (avoid O(n·k)); optionally follow `/Prev` (classic xref, bounded depth) and the xref-stream `/Prev` to confirm consistency; `Complete` earns a high score only with a `%%EOF` **and** consistent structure — a linearized first-page `%%EOF` never scores 100.

---

## Appendix C — Metric and statistics definitions

Ground-truth file `T` is split into blocks of size `B` (the volume cluster size); `H(T,i)` = SHA-256 of block `i` (the last short block compared separately).
- `exact(T,O)` = SHA-256(O) = SHA-256(T). `usable`: remove bytes after the true footer (last JPEG EOI, PNG IEND, last PDF `%%EOF`) then compare — fair to tools that append NULs/newlines.
- Output `O` is split into blocks from its offset 0; block `j` is **correct** if `H(O,j) = H(T,j)` for the assigned `T`, **cross** if equal to a block of another file, **junk** otherwise. **Assignment** `O→T`: the `T` with most correct blocks (ties: closest size); unassigned if < 1 block matches.
- `byte_recall(T) = correct bytes / |T|`; `byte_precision(O) = correct bytes / |O|`; `splice_rate(O) = cross bytes / |O|`.
- `viewable(O)`: full decode by an external tool (C-03) **and** `visually_equal` (images: pixel/SSIM against the original decode; PDFs: text/page comparison).
- `false_complete_rate = #(outputs labelled verified/complete but not exact) / #(outputs labelled verified/complete)`.
- `bloat = total output bytes / total ground-truth bytes recovered`.
- **FP taxonomy:** `kept_allocated` (exact/viewable match with a still-allocated file), `embedded` (its SHA-256 is inside the bytes of another recovered file), `duplicate` (SHA-256 already seen), `spliced` (`splice_rate > 0`), `junk` (rest). **Main FP = junk + spliced + duplicate.** Report *deleted-only* and *any-real-file* views.
- **Wilson 95 %** for `k/n`, `z = 1.96`: `centre = (p + z²/2n)/(1 + z²/n)`, `half = z·sqrt(p(1−p)/n + z²/4n²)/(1 + z²/n)`. Checks: 30/30 → lower 88.6 %; 100/100 → 96.3 %.
- **Exact McNemar** (same files, tools A and B): `b` = A right/B wrong, `c` = A wrong/B right; `p = min(1, 2·P(Bin(b+c, 0.5) ≤ min(b,c)))`.
- **Paired bootstrap:** resample **files** with replacement ≥ 10,000 times, compute the F1 (or recall) difference, report the 2.5–97.5 % percentile interval; always report `n_files_unique`, `n_images`, seed, dataset/profile.

---

## Appendix D — Cross-reference of the original IDs

| Old | New | Old | New | Old | New |
|---|---|---|---|---|---|
| v1:R-01 | A-04 | v1:Q-01 | A-10 | v1:B-01 | C-02, C-05 |
| v1:R-02 | A-07 | v1:Q-02 | A-11 | v1:B-02 | C-01, C-02 |
| v1:R-03 | A-05 | v1:Q-03 | A-12 | v1:B-03 | C-02 |
| v1:R-04 | A-01 | v1:Q-04 | A-13 | v1:B-04 | C-06 |
| v1:R-05 | A-02 | v1:Q-05 | A-10 | v1:B-05 | C-04 |
| v1:R-06, R-07 | B-07 | v1:Q-06 | A-10, B-10 | v1:B-06 | C-09 |
| v1:R-08 | A-08 | v1:Q-07 | A-14 | v1:B-07 | C-02, C-07 |
| v1:R-09 | A-06 | v1:Q-08 | A-15 | v1:B-08 | C-08 |
| v1:R-10 | A-03 | v1:Q-09 | A-08, A-12 | v1:B-09 | C-06 |
| v1:R-11 | A-06 | v1:U-01 | A-07 | v1:B-10 | D-02 |
| v1:R-12 | A-06, A-10 | v1:U-02 | B-01 | v1:U-03 | B-04 |
| v1:R-13 | B-06, B-01 | v1:U-04 | B-08 | v1:U-05 | B-03 |
| v1:R-14 | A-09, B-02 | v1:U-06 | B-10 | v1:R-15 | B-05 |
| v1:R-16 | B-09 | | | | |
| v2:F-01 | A-01 | v2:F-12 | B-06 | v2:A-01…A-08 | A-01, A-03, A-06, A-04, A-10, A-11, A-14, A-12 |
| v2:F-02 | A-02 | v2:F-13 | A-14 | v2:P0-1…P0-4 | P0.1, P0.3, P0.6, P0.4 |
| v2:F-03 | A-03 | v2:F-14 | A-10 | v2:B-01 | A-08 |
| v2:F-04 | A-04 | v2:F-15 | A-11, A-12 | v2:B-02, B-03, B-04 | B-02, B-03, B-04 |
| v2:F-05 | B-07 | v2:F-16 | C-06 | v2:B-05 | B-01 |
| v2:F-06 | A-06 | v2:F-17 | C-04, D-01 | v2:B-06, B-07, B-08 | B-05, B-06, B-07 |
| v2:F-07 | A-05 | v2:F-18 | C-02 | v2:B-09, B-10, B-11 | B-10, B-08, B-11 |
| v2:F-08 | A-07, B-02 | v2:F-19 | C-06 | v2:C-01…C-07 | C-01…C-07 (same meaning) |
| v2:F-09 | A-08, B-03, B-04 | v2:F-20 | C-04, C-09 | v2:C-08 | C-08 |
| v2:F-10 | B-01 | v2:F-21 | C-08, D-01, D-02 | v2:C-09 · v2:C-10 | D-02 · C-01 / Phase 4 |
| v2:F-11 | B-05 | | | | |
| v3:P0-1 | A-06 | v3:P1-2 | C-08 | v3:P1-5.1 | B-04 |
| v3:P0-2 | A-12 | v3:P1-3 | C-07 | v3:P1-5.2, 5.3 | B-08 |
| v3:P1-1 | B-03 | v3:P1-4 | C-02, C-09 | v3:P1-5.4 | B-01 |
| v3:P2-1 | A-10 | v3:P1-5.5 | B-10 | | |

---

## Appendix E — Decisions made by this merge (seed for `docs/DECISIONS.md`)

| ADR | Decision | Rationale |
|---|---|---|
| ADR-001 | Metadata/hybrid recovery is `scan --mode {metadata,hybrid}`; no separate `recover` verb (optional alias) | one CLI surface; compatible with existing flags |
| ADR-002 | Reports gain fields; existing field types never change; `tool_info`/`input` are new objects | compatibility (v2) over v1's schema rewrite |
| ADR-003 | `status` stays as "footer found"; `validity` is authoritative; only `verified` is "complete" in benchmarks | honesty without breaking consumers |
| ADR-004 | Multi-volume: warnings + `regions[]` now; default flip only after user approval | removes silent loss immediately, keeps user control |
| ADR-005 | MSRV: raise `rust-version` to 1.85 (re-pin only if the user insists) | lockfile already requires it |
| ADR-006 | Phase order: benchmark infrastructure before features; single-use lockbox | prevents tuning on held-out data |
| ADR-007 | Benchmark ground truth never uses CarveX validators; image ground truth includes pixel comparison | independence; S5 shows decode-only is insufficient |
| ADR-008 | Fuzz durations: smoke 60 s, nightly 10 min, release 30 min, v1.0 ≥ 24 h cumulative | merges the three guides' differing figures |

---

## Appendix F — Citations to verify before use (do not quote from memory)

Open the DOI/publisher page for each; fix author order, venue and pages from the source; add only what you verified, otherwise write "to be added".
- S. L. Garfinkel, *Carving contiguous and fragmented files with fast object validation*, DFRWS 2007 (Digital Investigation 4S) — BGC, object validation; **fragmentation statistics quoted in v1 (≈6 % overall; JPEG ≈16 %, Word ≈17 %, AVI ≈22 %, PST ≈58 %) are unverified.**
- G. G. Richard III, V. Roussev, *Scalpel: a frugal, high performance file carver*, DFRWS 2005.
- A. Pal, N. Memon, *The evolution of file carving*, IEEE Signal Processing Magazine, 2009 (v1 lists the authors in the opposite order — check).
- A. Pal, H. T. Sencar, N. Memon, *Detecting file fragmentation point using sequential hypothesis testing*, DFRWS 2008.
- H. T. Sencar, N. Memon, *Identification and recovery of JPEG files with missing fragments*, DFRWS 2009.
- M. I. Cohen, *Advanced carving techniques*, Digital Investigation, 2007.
- R. Poisel, S. Tjoa, *A comprehensive literature review of file carving*, ARES 2013.
- S. Garfinkel et al., *Bringing science to digital forensics with standardized forensic corpora* (Govdocs1), DFRWS 2009.
- B. Carrier, *File System Forensic Analysis*, Addison-Wesley, 2005.
- Specifications: Microsoft FAT32, exFAT; Linux-NTFS docs; ITU-T T.81; ISO/IEC 15948; ISO 32000-1/-2; PhotoRec "scripted run" documentation (`freespace`/`wholespace`).
- NIST CFReDS; DFRWS carving challenges; Digital Corpora (govdocs1) — recheck URL, licence and hash at download time.

---

## Appendix G — PR review checklist

- [ ] Regression test (red → green) citing the item ID; no new `#[ignore]` except the allowed xfail form.
- [ ] No new `unwrap/expect/panic/unchecked index` on image data; offset arithmetic checked.
- [ ] Source never opened for writing; output deterministic (run twice, compare hashes).
- [ ] `verified` only after deep validation.
- [ ] Every image-driven allocation capped.
- [ ] User errors carry a message and a hint; exit code matches §8.7.
- [ ] Docs + CHANGELOG + schema updated; numbers generated.
- [ ] G0–G9 green; no loss of coverage; no performance regression > 10 %.
- [ ] No change to ground truth/thresholds; no tuning on the lockbox.

## Appendix H — Environment notes

- External tools expected (record `SKIPPED` when absent, never install without network): `foremost scalpel photorec mkfs.vfat mkfs.exfat mkntfs mcopy ntfs-3g blkls fls istat icat tsk_recover ntfsundelete qpdf pdftoppm ewfacquire ewfexport 7z`.
- `bench/make_images.py` needs root (loop mount) today; C-02 replaces it with a no-root builder (userspace FAT writer/`mtools`; NTFS via `mkntfs` + `ntfs-3g` FUSE in a container).
- **Unverified sandbox tips from v3** (kept as hints, not facts): without `rustup`, a Rust toolchain can be assembled from conda-forge (`rust` in subdir `linux-64`, `rust-std-x86_64-unknown-linux-gnu` in subdir `noarch`); FAT images without the `vfat` kernel module through `mtools` plus `losetup -o <offset> --sizelimit <len>` and `parted` for the MBR; NTFS via `mkntfs` + `mount -t ntfs-3g` (FUSE). Distribution `apt` toolchains older than 1.85 cannot build the committed lockfile (§2.1).
