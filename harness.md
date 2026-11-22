# CarveX — Remediation Harness (`harness.md`)

> **Unified remediation harness.** Merged from five independent reviews of `carvex` v0.2.0 (`review-v1.md` … `review-v5.md`), cross-checked against each other and against the acceptance baseline `guide.md` (924 lines). It replaces the five reviews as the single instruction set for fixing their findings.
> **Applies to:** commit `706a12a` ("Add opt-in bifragment gap carving for B-06") = v0.2.0 + B-04 evidence containers + B-06 fragment recovery (both under `[Unreleased]` in `CHANGELOG.md`). All five reviews examined this code (v4's zip `sha256:68e075d2…2b62` has the same content).
> **Audience:** an AI coding agent (Claude Code, Codex, Cursor, …) working in the `carvex/` repository (Rust library + CLI, Python bench).
> **Language:** English. Commands, flags, identifiers and JSON fields are verbatim. Decisions already confirmed by the user (2026-11-21) are in **§3.6**.
>
> ### File integrity — do this before anything else
> The previous agent worked from a **truncated** `guide.md` (finding H-01). Do not repeat that.
> 1. `wc -l harness.md` must print **745** and `tail -n 1 harness.md` must print `<!-- HARNESS-EOF -->`. Otherwise you hold a truncated copy: **STOP and tell the user.**
> 2. If `docs/guide.md` exists it must be **924 lines** with `sha256sum` = `22ee8122209e9eda11d6f32a1e650e8f8e48f5610bd808309d94f436e5e78265`. If it is absent, ask the user for the file (task H-01); continue with this harness, which is self-contained for everything it requires.
> 3. Read **§0–§5 completely** before changing any line of code.

---

## 0. How to use this document

1. Work in the **phase order of §7**. Phase 0 (reproduce, instrument, fix the gate) comes first. **No finding is fixed before its reproduction (or static demonstration) is recorded** in `docs/PROGRESS.md`.
2. Every finding has an ID `H-xx` (§6). Status values: `OPEN → REPRODUCED | STATIC-ONLY | NOT-REPRODUCED → IN_PROGRESS → CLOSED`, or `WONTFIX(reason, user-approved)` / `BLOCKED(reason)`. A finding is `CLOSED` only when its **acceptance command(s)** pass and its red-first test is green.
3. Loop per finding (§10): **reproduce → red test → minimal fix → test green → gates → docs/CHANGELOG → one small commit `fix(scope): … [H-xx]` → update `docs/PROGRESS.md`**. One commit = one finding (or one clearly named sub-step). Never mix refactors with behaviour changes.
4. Priority: **CRIT** (false integrity claim, silent loss, process abort/OOM-kill, or whole-run loss on plausible or hostile input), **HIGH**, **MED**, **LOW**. Obligation: **[MUST]** / **[SHOULD]** / **[MAY]**.
5. **Trust the repository over this document.** Line numbers are approximate (reviews quote ±5 lines at `706a12a`): locate code by symbol (`grep -n`). If code, flags or numbers differ from this text, record the difference in `docs/DECISIONS.md`, take the conservative option and continue.
6. Every number in README / reports / BENCHMARK comes from generated results, never typed by hand. A tool or dataset that is unavailable is recorded as `SKIPPED (reason)`; **never invent a number** and never infer the result of a run that did not happen.
7. Reviewers disagree in a few places (§2.5). Those disagreements are settled **by measurement in Phase 0**, not by opinion. Do not "fix" a conflicting claim before its Phase-0 measurement is recorded.
8. Scope discipline: fix only what this harness lists. A new problem you discover goes into `docs/PROGRESS.md → New findings` as `N-xx` with evidence and proposed priority; do not fix it silently.
9. Evidence labels used in this file:

| Label | Meaning |
|---|---|
| **RT×n** | Executed by *n* independent reviewers with consistent results (reviewer list in the register). |
| **SRC×n** | Read in the source by *n* reviewers. |
| **SELF** | Arithmetic or a property re-computed while writing this harness (listed in §2.4). |
| **ONE** | Reported by a single reviewer — treat as a hypothesis until Phase 0 reproduces it. |
| **CONFLICT** | Reviewers disagree; see §2.5 for the resolution rule. |

---

## 1. Mission and scope

**Mission.** Close the gaps that five independent reviews found between CarveX 0.2.0 and the acceptance baseline `guide.md`, in this order of priority: (1) **correct and honest** — never label damaged data as verified, never lose evidence silently; (2) **reliable** — survive hostile or corrupt input with bounded time and memory, never abort or lose a finished run; (3) **usable** — truthful reports, exit codes, interruption; (4) **auditable** — gates, tests and documents that prove the above.

**What the reviews agree is good and must be preserved** (regressions here fail the PR): all-volumes-plus-gaps default (approved, D-23) with `--partition largest --no-gaps` as the compatibility path; PDF end detection (S2, S2c, S3, linearized/ObjStm/encrypted/incremental fixtures byte-exact); nested-PDF suppression before writing; truncated-tail trimming (S6); header-flood speed (S9/S10/S11 pass on source builds); deterministic output across `--threads`; read-only source with before/after identity; atomic, symlink-safe output writes; `#![forbid(unsafe_code)]` and deny-lints on indexing/`unwrap`/`expect` in `fs/` and `signatures/`; 20/20 exact recovery of deleted files on FAT32 fixtures with 0 FP in `unallocated` mode; the repository's candid documentation style.

**In scope:** findings H-01…H-34 (§6). **Backlog, not fixed here** (tracked against `guide.md`): H-35 benchmark science, H-36 scope gaps (B-01, B-03, …). Do not start H-35/H-36 unless the user asks.

**Non-goals** (unless the user asks): GUI; new file systems; decryption; changing any default that changes users' results (§3.5; the two defaults confirmed in §3.6 are the only exceptions).

---

## 2. Verification ledger

### 2.1 What "verified" means in this harness — read this first
The synthesis behind this file **could not clone or fetch the repository** (GitHub refuses automated fetches, the sandbox had no network, web search does not index the repo). Therefore:
- **Nothing here was re-run against the live repository by the author of this harness.** "RT×n" means *n reviewers executed it*, not that the harness author did.
- What *was* done: all five reviews and `guide.md` were read in full; every finding was mapped across reviews (Appendix B); agreements, single-source claims and contradictions were separated (§2.3–§2.5); arithmetic and numeric claims were recomputed (§2.4).
- **Phase 0 is the live verification.** It re-runs every scenario of Appendix A on a fresh source build, records the result next to the reviewers' "before" values, and downgrades any finding that does not reproduce. An agent that skips Phase 0 is violating this harness.

### 2.2 The five reviews
| Review | Code examined | Executed? | Strong at | Limits |
|---|---|---|---|---|
| **v1** | `706a12a`, built (release), 73 tests, fmt/clippy/doc | runtime with `qpdf`, Pillow; flood, determinism, VMDK memory/path | PNG CRC table, PDF `qpdf` oracle (6 variants byte-exact), PDF EOF scaling, VMDK memory + path traversal, README claims, `dist/` staleness | MSRV, deny/audit, Windows, multi-volume (no mkfs) not run; JPEG splice trials from one image pair per cell |
| **v2** | `1ffc0e0` then `706a12a` | runtime **only on `dist/carvex-linux-x86_64`** (0.2.0 built at `cbee72d`, musl, 2 vCPU); **no cargo build** | `--max-memory` totals, PNG inflate sizes, JPEG SOF allocation (N-02), PDF CRLF (N-01), 1,680-mutant run, label-vs-evidence table | B-04/B-06 source-only; timings from the older musl binary |
| **v3** | `main` as served 2026-11-20, commit unpinned | **nothing executed** (no cargo, no DNS) | careful static reading with line links; consolidation of earlier reviews | no runtime evidence at all |
| **v4** | zip of `706a12a` content | built (Rust 1.90), 73 tests, 6 malformed-container inputs, B-06 on `mixed_fat32.img` | B-04/B-06 design review, file-level diff proving `src/signatures` & `src/fs` untouched | F-01…F-13 carried from an earlier round; some positive claims refuted (C3, C4) |
| **v5** | `706a12a`, built (Rust 1.91.1; MSRV 1.85.1), 1 vCPU | by far the broadest runtime: self-built FAT32/NTFS/multi-volume, real `ewfacquire`/`qemu-img` containers, 2,160 mutants, SIGINT, scale | F-13…F-26 (volume downgrade, SIGINT, repair, regions[], E01/VMDK defects, B-06 recall) | PhotoRec/Foremost/Scalpel/cargo-deny/fuzz not run |

**Weighting rule used to resolve disagreements:** runtime on a source build of `706a12a` > runtime on `dist/` > source reading > repository documents. Several reviewers agreeing on source reading do not outweigh one contradicting runtime result.

### 2.3 Consensus summary
Findings reported by **all five** reviews (static and/or runtime): `--max-memory` is post-hoc (H-08); PNG decode inflates unbounded (H-05); PNG bad CRC still `complete`/`decoded` (H-17); PDF `decoded` is lexical (H-19); JPEG strict-scan `None` falls through to `decoded` and docs contradict code (H-18); PDF `%%EOF` handling is O(E×N) (H-07); no per-candidate panic containment (H-10); validity model absent (H-16); gates G7–G9 absent / fuzz not on push-PR (H-30); truncated/unsynchronised guide (H-01); placeholder repository URL (H-04).
Findings reported by **≥3**: stale `dist/` and `tool_info.commit = "unknown"` (H-04); overstated FIXED/DONE labels (H-31); no fuzz targets for new parsers (H-14); missing budgets (H-09); SIGINT/exit 4 (H-28, 2 reviews + docs).
Findings with **runtime in one review only (ONE)** — highest risk of being wrong, reproduce first: JPEG SOF allocation (H-06); E01 partial/damaged chunk (H-22); VMDK unallocated tables (H-23); auto-detect false positives (H-24); unparsable-volume downgrade (H-26); `regions[]` overlap (H-27); README speed claim (H-33).

### 2.4 Numbers re-computed in this synthesis (SELF)
Wilson 95 % bounds (30/30 → 88.65 %; 100/100 → 96.30 %; 500/500 → 99.24 %); PNG score floor `100 − 15·min(bad,3)` = 85/70/55 and `55 > --min-score 50`; test total 34+13+5+8+5+6+2 = **73**; coverage 4,111/6,001 = **68.51 %**; lockbox precision/recall rows of v2 (83.3/82.7/50.0/46.2 % and 100/95.8/78.0 %); VMDK table memory (v1: 256·65,536·4 B = 64 MiB; 1,024·262,144·4 B = 1 GiB; v5: 128·2²⁰·4 B = 512 MiB ≈ 520 MiB observed); E01 tail (1,953,525,168 mod 64 = 48 sectors); `regions[]` over-statement (102,760,448 − 95,367,168 = 7.05 MiB); 2²⁷ sectors × 512 B = 64 GiB (the aborting allocation); zlib on zeros: ratio ≈1,029:1 (256 MiB → ≈261 KB, matching v5's 260,979 B fixture); `guide.md` = 924 lines (v4's "925" is a trailing-newline count). **All consistent with the reviewers' figures.**

### 2.5 Conflict log (resolved here; measured again in Phase 0)
| # | Topic | Statements | Resolution |
|---|---|---|---|
| C1 | JPEG splices labelled `decoded` | **v1:** 15–98 % of 100 random cuts per image pair accepted (noise-baseline 33/100, noise-progressive 20/100, photo-like 15/54 and 44/45), but v1 calls it indicative (one pair per cell, correlated cuts). **v2, v5:** constructed splices rejected (`partial`, "AC coefficient index runs past the block" / "failed to decode huffman code"); Pillow accepts them. | **Not decided by opinion.** H-18 step 0 runs the property test (≥1,000 independent pairs) on unmodified code. The *structural* defects (None→`decoded`, dead `Unverified`, docs contradict) are 5/5 confirmed and fixed regardless. |
| C2 | PDF `%%EOF` cost | v1 18.3–18.6 s, v2 18.05 s (63 MiB, 4,368 markers; control 0.14 s), v5 18.7–19.2 s (15.4 s in a later run) vs **v4** 0.92→1.97 s (8→64 MiB) "harder to trigger". | Fixture-dependent: the slow path needs a revision-like tail after each marker (v5). v4's fixture did not exercise it. Canonical fixture = X04 (v5 recipe + v2 control); v4's fixture kept as a second control. |
| C3 | E01 `chunk_size` bounded? | **v4:** "bound-checked against the file". v1/v2 source: only overflow-checked. **v5 runtime:** `sectors_per_chunk = 2²⁷` → SIGABRT (64 GiB alloc). | v4 mistook overflow checks for a bound. **Refuted** by v5 runtime (H-11). |
| C4 | VMDK table sizing | **v2 (source):** range checks "look correct". **v1, v5 (runtime):** tables multiplied (1,029 MiB from 1 MiB; 520 MiB from 4.49 MiB). | v2 superseded (H-12). |
| C5 | Parallel slower than 1 thread; S9-4M 6–11 s | **v2** on `dist/` (musl, `cbee72d`, 2 vCPU): 6.37 s / 10.87 s. **v1, v4, v5** on source builds: 0.78–1.13 s; v5 A/B: no regression 1ffc0e0→706a12a. | Hypothesis (**unverified**): the static musl `dist/` build's allocator under per-candidate allocation. H-15 is conditional on Phase-0 measurement of both builds. |
| C6 | B-06 `FRAGMENT_*` warnings and exit code | **v4:** exit 0 is deliberate and correct (`Warning::incomplete()` excludes them). **v5:** should be 3. | Fix the cause first (budget consumed on ordinary images — both reviewers saw 5 s used), then exit **3** when a limit actually truncates the stage (guide A-13: budget exceeded ⇒ 3). ADR-H5. |
| C7 | B-06 wall-clock budget | **v4:** "genuine time budget". **v2 N-07:** non-deterministic across machines (guide §3.1). | Work-unit budget is authoritative; wall-clock only as a flagged safety net (ADR-H5). |
| C8 | Severity of PNG bad CRC | High (v1, v3, v4), Medium (v5), Medium–High (v2: zlib/Adler already catches payload damage; only CRC-field damage and chunk-level splices slip through). | Priority HIGH for the label; scope note: the label is wrong, not routinely misleading on corrupted data. |
| C9 | Coverage figure | 63.42 % (v5, pre-B-04), 67.50 % (post-B-04), **68.51 %** (v1, v2, v3 = 4,111/6,001; SELF ✓). | Use 68.51 % as the repository's latest claim; floor 60 %; guide targets 75/80/85 %. |
| C10 | B-04 status | repo MITIGATED; v4 "agree"; v5, v2 PARTIAL. | **PARTIAL** until H-11…H-14 and H-22…H-25 close. |
| C11 | `review_repro.py` on a clean checkout | v1: R7/R8 "SKIP / NOT RUN"; v5: prints FAIL…SKIP and **exits 1**; v3: repo-reported 10/10 PASS (author machine with images). | Compatible: scenarios needing git-ignored images fail the gate on a clean checkout (H-02). |
| C12 | `source` enum | guide: `carved`; repo: `carve`, `bgc` (v1). | Keep repo values (compatibility); document mapping in `docs/report.schema.json` (ADR-H3). |
| C13 | Function length (A-10), README drift | **ONE** each (v2 N-05 heuristic count; v1 README speed claim; v2 flag table). | Verify with tools (`clippy::too_many_lines`, `grep`) in Phase 0 before acting (H-33, H-34). |

### 2.6 Claims deliberately **not** carried into this harness
v1's JPEG percentages as *measured rates* (kept only as hypothesis C1); v2's N-03 as a fact about source builds (C5); v4's "chunk_size is bound-checked" (C3); v2's "VMDK range checks look correct" (C4); v5's 63.42 % coverage as current (C9); every bibliographic statistic about fragmentation rates (unverified in `guide.md` too — do not cite).

### 2.7 Still unverified by every reviewer (Phase-0 / later work)
`cargo deny`/`cargo audit` (G4) and the 18.5 M-execution fuzz claim (repo-reported); GitHub Actions run `44548142275` is on `42376a9` (predates B-04/B-06; **no CI run covers HEAD**); the packaged Windows `.exe`; multi-segment E01 (`.E02`…), EnCase/FTK-made E01, VMware-made VMDK, differencing VHD, encrypted E01; real block devices; CFReDS re-run at 0.2.x; lockbox/full-0.2.0 result files (git-ignored, not in the tree); `ntfs`/`fat32` fallback paths (A-09); B-06 on large volumes.

---

## 3. Guardrails (violating any = PR rejected)

### 3.1 Forensic safety
- **Never write to the source.** Read-only handles only; tests and benchmarks use copies or generated images. Container readers (E01/VHD/VMDK/split/stdin) are read-only; a descriptor must never make the tool read outside the evidence directory unless the user opts in (H-13).
- **Deterministic output:** same image + same options + same version ⇒ same file list, order, names and bytes, **including across `--threads` values and machines**. Budgets that can change results are expressed in *work units* (candidates, bytes examined), never wall-clock. A wall-clock stop is allowed only as a safety net that sets `nondeterministic_stop: true` in the report and emits a coded warning.
- **`exact` means exact:** an output file is the bytes read from the image, except under `--repair`/`--salvage`, where the report must keep `sha256_original` (bytes read) **and** `sha256_output` whenever the two differ (H-21). Bytes that were zero-filled because the source was unreadable are **not evidence**: any output intersecting an unreadable region can never be `verified` (H-22).
- **Never skip data silently:** any unscanned region, skipped candidate, unreadable range, unparsable volume or truncated stage ⇒ a coded warning in `warnings[]` **and** exit code 3.

### 3.2 Anti-gaming
- **Forbidden after seeing results:** editing `bench/corpus/**`, `bench/images/*.json` (ground truth), any `SHA256SUMS`, frozen results (`bench/results/baseline-v0.1`, `final`, `lockbox-*`), thresholds, metric definitions or baseline-tool configurations. Protocol changes go in their own commit with the reason, before re-running.
- **Forbidden:** tuning anything on the lockbox (single-use; consuming it requires the user); special-casing the corpus (names, seeds, hashes, offsets, sizes) anywhere in `src/`.
- **Tests:** never delete, `#[ignore]`, loosen or rewrite an existing test to make it pass. The only allowed `#[ignore]` is `#[ignore = "H-xx xfail"]` on a red-first test you added in Phase 0, listed in `docs/PROGRESS.md` and removed in the commit that fixes it. **Exception E-1 (needed by H-17/H-18):** an existing test that *asserts a defective label* (for example `reports_bad_crc_but_keeps_file` asserts the file is kept and scored 85 with no integrity flag) may be **extended** in the same commit as the fix. If an old assertion must change, quote it in a comment `// H-xx: previously asserted …` and log it in `docs/DECISIONS.md`. Never weaken a test.
- **Performance thresholds** (G8) are committed *before* the gate is enabled and are never raised to make a run pass; a change needs a `DECISIONS.md` entry with measurements.
- Do not write "100 %", "stable", "fair", "N× faster", "verified" without number + CI + scope. Never invent a result, a citation or a dataset.
- `#[allow(...)]` / `# noqa` only with a one-line justification and the finding ID.

### 3.3 Code quality for hostile input
- `#![forbid(unsafe_code)]` stays. The deny-lints `unwrap_used`, `expect_used`, `indexing_slicing` apply to `src/fs/`, `src/signatures/` **and `src/image/`** (extend them in H-11; fix what they reveal).
- **Every allocation whose size derives from image or container fields:** (1) compare against a named constant or option cap *before* allocating; (2) allocate with `Vec::try_reserve_exact` (or equivalent) and turn failure into a typed error — in Rust an allocation failure **aborts the process**, so a cap alone is not enough for large legal values; (3) never `vec![0; n]` with an untrusted `n`.
- Offset/size arithmetic: `u64` with `checked_*`/`saturating_*` chosen deliberately; no silent narrowing `as`.
- Decoders (zlib, `jpeg-decoder`) are never driven by `read_to_end` on untrusted input; use `Read::take(limit)`, fixed buffers, or the decoder's own limit API (verify the pinned crate version supports it).
- New dependencies: only when needed; record licence (MIT/Apache-2.0/BSD), maintainer, transitive-crate count and MSRV (≤ 1.85) in `docs/DEPENDENCIES.md`.
- Concurrency: results must not depend on scheduling; budget acquisition must not deadlock.

### 3.4 Compatibility
- Keep every existing subcommand, flag, output name (`NNNNN_<type>_<offset16hex>.<ext>`) and JSON/CSV field **with its type and meaning**; only add. Legacy `status` keeps meaning "footer found"; legacy `validation` keeps its four values (`structure|decoded|partial|unverified`); `source` keeps `carve`/`bgc`; `--validate` keeps `structure|decode` (the guide's `deep` = the repo's `decode`).
- Any observable behaviour change needs a `CHANGELOG.md` entry under **Changed**, a flag to restore the old behaviour where practical, and (if it changes users' results) user confirmation (§3.5). §8 lists the behaviour changes this harness makes and which of them need confirmation.

### 3.5 STOP-AND-ASK (stop only for these; otherwise decide conservatively and log in `docs/DECISIONS.md`)
1. Changing a **default** that changes users' results: `--validate`, `--align`, `--min-score`, `--max-size`, `--partition`/gaps (the `--on-read-error` default is already confirmed — §3.6).
2. Using `unsafe`; adding a dependency with a doubtful licence/maintainer or > 50 transitive crates; swapping the global allocator (H-15).
3. Deleting committed files (the `dist/` binaries are to be **replaced** at the 0.3.0 release as approved in §3.6 — never deleted), changing the licence, downloading external data with unclear licence/size/content.
4. Changing a benchmark threshold, ground truth or metric after seeing results; consuming a lockbox.
5. Wording in the academic report that changes a **conclusion**.
6. Two items contradict, or a gate cannot be made green within scope.
7. A fix would change forensic semantics (e.g., normalising output bytes).
8. `docs/guide.md` is missing and the user has not supplied it (H-01).

A finding that does **not** reproduce is **not** a stop condition: record `NOT-REPRODUCED` with the exact command and output, keep any static evidence, mention it in the next phase report (CRIT items: say so explicitly), continue.

### 3.6 Decisions already confirmed by the user (2026-11-21) — do not ask again
1. **`--on-read-error` default is `zero-fill`** (values `abort`, `zero-fill`, `skip`; `abort` restores the pre-0.3.0 behaviour). Implemented in H-22; `CHANGELOG` → *Changed*. The guardrail that comes with it: zero-filled bytes are never evidence (§3.1) — every region is listed in `unreadable_regions[]`, the run exits 3, and no output intersecting a zero-filled region may be `verified`.
2. **Release 0.3.0 and rebuild `dist/`.** HEAD is versioned `0.3.0-dev` from P0.9 so it can never again be mistaken for the released 0.2.0 (v1:R-21); the release step at the end of Phase 4 (§7) sets `0.3.0`, rebuilds the `dist/` binaries from the tagged commit (**replace, never delete**), executes them, and regenerates `dist/SHA256SUMS`. The tag is created locally and **not pushed**. Deleting `dist/` or any other committed file still needs the user (§3.5-3).

---

## 4. Environment, gates, progress

### 4.1 Start-up (record results in `docs/PROGRESS.md → Environment`)
```bash
git log -1 --format='%H %ad %s' && git status --short   # expect 706a12a, clean tree; else: git diff --stat 706a12a..HEAD and record
cargo --version && rustc --version                        # rust-version = "1.85" (v5 verified: cargo +1.85 check --locked OK)
cargo build --release --locked && cargo test --all-features --locked --release   # reviewers saw 73 passed, 0 failed
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
for t in mkfs.vfat mkntfs mcopy mdel ntfs-3g ewfacquire ewfverify qemu-img qpdf pdftoppm foremost scalpel photorec \
         cargo-fuzz cargo-deny cargo-audit cargo-llvm-cov; do command -v "$t" >/dev/null && echo "OK   $t" || echo "MISS $t"; done
python3 -c "import PIL, pypdf, numpy" 2>&1 | tail -1
```
Never install tools without network; a missing tool ⇒ `SKIPPED (reason)` naming the affected scenario. **Never trust `dist/`**: it is stale (H-04). Always test `target/release/carvex` built from source; when a scenario is timing- or allocator-sensitive, also test the musl build (H-15).

### 4.2 Gates (a phase is done only when its gates are green; `SKIPPED` is allowed if logged)
| Gate | Condition | State at `706a12a` (from the reviews) |
|---|---|---|
| G0 Format | `cargo fmt --all -- --check` | exists (gate.sh, CI) |
| G1 Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exists |
| G2 Test | all green; every fixed finding has a red-before-fix **Rust** regression test | tests green (73); regression tests mostly Python-only (H-03) |
| G3 MSRV | build with the declared `rust-version` and the committed lockfile | CI job exists; no `scripts/check-msrv.sh`, no `rust-toolchain.toml` (H-02) |
| G4 Supply chain | `cargo deny check`, `cargo audit` | configured; unverified by reviewers (H-30) |
| G5 Fuzz | each parser has a target; 60 s smoke on every push/PR; seeds committed | 7 targets; CI runs fuzz **only** on `schedule`/`workflow_dispatch`; none for `src/image/*` or the fragment joiner (H-14, H-30) |
| G6 Cross-platform | build + test on Linux and Windows (+ macOS), and run the **packaged** binary | matrix exists, last green run predates B-04/B-06; `.exe` never executed (H-30, H-32) |
| G7 Golden | the set of output SHA-256 per D0 scenario equals the ground-truth set: `unallocated` = 100/100 deleted files exact, 0 FP; `raw` = 100 TP + 20 kept | **missing** (H-30). Comparing *output hashes* (not image bytes) makes G7 independent of `mkfs` serials, so fixtures can be built without root (H-02) |
| G8 Performance | guide thresholds (Appendix A, X04/X05/X23) and no regression > 10 % vs `bench/perf/baseline-0.2.0.json`; memory caps respected | **missing** (H-30). Shared CI runners are noisy: hard thresholds block, the 10 % relative check reports unless `--strict` |
| G9 Docs | README flag table = `carvex --help`; numbers in docs = generated results; `cargo doc -D warnings` | only `cargo doc` exists (H-30, H-33) |

`scripts/gate.sh` is the single entry point and is run before every commit. After H-02 it must pass on a **clean checkout without root**, printing `SKIPPED (reason)` for unavailable prerequisites instead of failing. It must call: fmt, clippy, test, doc, deny/audit if installed, MSRV check, release build, `scripts/review_repro.py --assert --xfail scripts/repro_xfail.txt`, `scripts/golden.py` (G7), `scripts/perf_gate.py` (G8), docs check (G9), fuzz smoke if `cargo-fuzz` exists.

### 4.3 Red-first workflow (xfail)
In Phase 0 the new repro scenarios are **red** (that is the evidence). `scripts/repro_xfail.txt` lists the currently failing scenario IDs (`X01`, …) one per line. `--assert --xfail` rules: a scenario **not** listed that fails ⇒ gate fails; a listed scenario that **passes** (XPASS) ⇒ gate fails and forces removal of the line. The commit that fixes a finding removes its scenarios. Rust equivalents carry `#[ignore = "H-xx xfail"]`. **At the end of Phase 1 and Phase 2 the file contains only scenarios of later phases.**

### 4.4 Progress file (`docs/PROGRESS.md`; update after every task)
```markdown
## Environment            <date, commit, rustc, tools present/missing>
## Findings (H-xx)
| ID | Pri | Repro (date, build) | Status | Commit | Test(s) | Before → After (± CI) / evidence |
## Open xfail tests / scenarios
## Blockers / open questions
## New findings outside this harness (N-xx + evidence + proposed priority)
## Decisions → docs/DECISIONS.md      ·     Conflict measurements (C1…C13, §2.5)
```
Per-task report (paste into PROGRESS):
```
Task: H-xx — <title>        Reviews: <v1:R-.., v5:F-..>
Repro: REPRODUCED|STATIC-ONLY|NOT-REPRODUCED  <command, observed value vs reviewers' "before">
Change: <files, 1–3 lines>
New tests: <names; red before / green after: yes|no>
Gate: green | green (SKIPPED: …)
Measurement: before <x> → after <y> on <fixture>
Residual risk / next: <…>
```
`docs/REVIEW_RESPONSE.md` gets one row per `H-xx` (status ⇒ commit ⇒ test ⇒ measurement). **A status may be `FIXED`/`DONE` only if every linked H-finding is `CLOSED`** (H-31 adds a script that enforces this).

---

## 5. Repository map (as reported at `706a12a`; locate by symbol)

```text
src/lib.rs, src/main.rs        library + thin binary; `panic` left at default (unwind); overflow-checks = true in release
src/cli.rs                     clap; options: --max-size --max-truncated-size --max-memory --min-score --validate {structure,decode} --align
                               --partition --no-gaps --input-format --fragment-recovery --max-gap --max-frag-candidates --frag-time-limit
                               NO: --max-candidates --time-budget --max-work-per-candidate --max-output --on-read-error --sector-size --salvage --resume
src/scan.rs                    run_scan, carve_candidates (batches of 64, par_iter, results kept in memory), carve_one,
                               postfilter, memory check ≈L555 (after postfilter, before write), recover_fragments / try_fragment_join ≈L990–L1420,
                               Warning::incomplete() ≈L216–231 (excludes FRAGMENT_TIME_LIMIT / FRAGMENT_CANDIDATE_LIMIT), stop: AtomicBool (not wired to a signal)
src/report.rs, docs/report.schema.json   schema v2 (+ conformance test); tool_info.commit = option_env!("CARVEX_GIT_COMMIT") → "unknown" (no build.rs)
src/io.rs, src/image/{mod,e01,split,vhd,vmdk}.rs   ImageSource + B-04 readers (≈1,289 lines; e01 chunk_size ≈L296–308, load_chunk ≈L199; vmdk grain tables ≈L304–376, resolve ≈L268–274)
src/fs/{mod,fat32,ntfs}.rs     detect/detect_all, VolumeSet (errors collected, never reported; regions() ≈ whole-volume free+allocated); catch_unwind ONLY at mod.rs ≈L373, L443
src/signatures/mod.rs          Validation enum (structure|decoded|partial|unverified — `Unverified` never constructed), validate_jpeg ≈L194–228,
                               validate_png ≈L235–333 (+ expected size / png_rows ≈L363–410), validate_pdf ≈L335–361
src/signatures/jpeg.rs         strict_baseline_scan ≈L337–517 (returns None for progressive/arithmetic/multi-scan)
src/signatures/png.rs          bad_crc ≈L140–248 (−15 per bad CRC, cap −45, still complete: true); test reports_bad_crc_but_keeps_file ≈L345
src/signatures/pdf.rs          find_eofs ≈L203–233 (≤ 4,096 candidates; count_objects(&data[..end]) + "/Root" search per candidate), count_objects ≈L324
tests/                         cli_contract 13 · evidence_formats 5 · fragment_recovery 2 · fs_detect 8 · properties 5 (random-bytes "never panic", extents) · scan_integration 6 · + 34 unit  = 73
fuzz/fuzz_targets              jpeg png pdf fat32_boot ntfs_boot ntfs_mft partition_table   (nothing for image/*)
scripts/                       gate.sh ("guide-v2" header), review_repro.py (R1–R8 = 10 scenarios; R7/R8 need git-ignored images), repro_xfail.txt (empty), coverage.sh
.github/workflows/ci.yml       3-OS test, MSRV, deny/audit, coverage floor 60 %, actionlint; fuzz only on schedule/dispatch
bench/                         corpus (30 files), images/*.json (ground truth; images git-ignored), results/{baseline-v0.1,cfreds,final}; lockbox + full-0.2.0 + fuzz + coverage results are git-ignored
dist/                          Linux musl static + Windows GNU binaries + SHA256SUMS — built at cbee72d, predate B-04/B-06, still report "0.2.0"
docs/                          README, CHANGELOG, PROGRESS, DECISIONS (D-01…D-30), REVIEW_RESPONSE, VALIDATION, BENCHMARK_PROTOCOL, REPORT, ARCHITECTURE
```
**Baselines to preserve (G7):** D0 sanity — `unallocated` 100/100 exact, 0 FP; `raw` 100 TP + 20 FP (the 20 kept files). D0 is **30 distinct files in 4 contexts**, never n = 100. Repository-reported lockbox-v1 (500 deleted + 100 kept, 20 images): CarveX `unallocated` 500/0/0, `raw` 500/100/0 — *not verifiable from the tree* (H-32). Perf baselines reported at `706a12a` (1 vCPU, source build): S9-256K 0.06–0.07 s; S9-1M 0.25 s; S9-4M 0.97–1.13 s; S10 0.50–0.57 s; S11 0.04–0.06 s; random 512 MiB 0.20 s (≈2.5 GiB/s).

---

## 6. Findings register

IDs follow the phase order of §7. "Reviews" gives the original IDs (`v1:R-xx` = `review-v1.md`, …); the full cross-reference is Appendix B. Evidence labels are defined in §0.9; reviewers who *executed* the scenario are listed in the detail block.

| ID | Pri | Area | Finding | Evidence | Ph |
|---|---|---|---|---|---|
| H-01 | HIGH | PROCESS | Guide truncated / not in repo; history not per-item | SRC×5 | 0 |
| H-02 | HIGH | PROCESS | Gate cannot pass on a clean checkout; no no-root fixtures; toolchain not pinned | RT×1 + SRC×4 | 0 |
| H-03 | HIGH | PROCESS | Red-first Rust regression / adversarial / property tests missing | SRC×3 | 0 |
| H-04 | MED | PROCESS | Release identity: stale `dist/`, `commit = "unknown"`, placeholder URL, leaked paths | RT×2 + SRC×5 | 0 |
| H-05 | CRIT | SAFETY | PNG `--validate decode` inflates without bound | RT×4 + SRC×5 | 1 |
| H-06 | CRIT | SAFETY | JPEG SOF dimensions drive an unbounded allocation (decode) | RT×1 (ONE) | 1 |
| H-07 | HIGH | SAFETY | PDF `%%EOF` evaluation is O(E×N) (≈19 s at 4,096 markers) | RT×3 + SRC×5, CONFLICT C2 | 1 |
| H-08 | CRIT | SAFETY | `--max-memory` is post-hoc; whole run lost, no report | RT×3 + SRC×5 | 1 |
| H-09 | HIGH | SAFETY | Required work budgets absent | RT×3 + SRC×2 | 1 |
| H-10 | HIGH | SAFETY | No per-candidate / container panic containment | SRC×5 (no panic ever triggered) | 1 |
| H-11 | CRIT | SAFETY | E01 allocates from untrusted header (SIGABRT); stdin spool unbounded | RT×1 + SRC×3, CONFLICT C3 | 1 |
| H-12 | CRIT | SAFETY | VMDK grain tables multiplied in memory; ZERO extent size | RT×2, CONFLICT C4 | 1 |
| H-13 | HIGH | SAFETY | VMDK descriptor can read arbitrary / wrong files | RT×2 | 1 |
| H-14 | HIGH | SAFETY | No fuzz targets for image parsers and fragment joiner | SRC×4 | 1 |
| H-15 | MED | PERF | Parallel carve slower than 1 thread (conditional on Phase 0) | RT×1 (ONE), CONFLICT C5 | 1 |
| H-16 | CRIT | VALIDITY | Validity model absent; default output reads as "complete / 100" | RT×3 + SRC×5 | 2 |
| H-17 | HIGH | VALIDITY | PNG bad CRC still `complete`/`decoded` | RT×3 + SRC×5 | 2 |
| H-18 | HIGH | VALIDITY | JPEG strict scan `None` → `decoded`; dead `Unverified`; splice rate disputed | RT×2 + SRC×5, CONFLICT C1 | 2 |
| H-19 | HIGH | VALIDITY | PDF `decoded` is a lexical check | RT×2 + SRC×5 | 2 |
| H-20 | MED | VALIDITY | PDF ending `%%EOF\r\n` / `\r` demoted to `partial` | RT×2 | 2 |
| H-21 | HIGH | VALIDITY | `--repair` labels unreadable PDFs `repaired`; no `sha256_original` | RT×1 + SRC×3 | 2 |
| H-22 | CRIT | EVIDENCE | E01 reader aborts the whole scan on partial final / damaged chunk | RT×1 (ONE, real libewf) | 3 |
| H-23 | MED | EVIDENCE | Unallocated VMDK grain tables read as errors, not zeros | RT×1 (ONE) | 3 |
| H-24 | MED | EVIDENCE | Auto-detect reinterprets ordinary raw images as VHD/VMDK | RT×1 (ONE) | 3 |
| H-25 | LOW | EVIDENCE | Container reports mix logical/physical size; stdin identity | RT×1 (ONE) | 3 |
| H-26 | MED | EVIDENCE | Unparsable volume in a multi-volume disk downgraded silently | RT×1 (ONE) | 3 |
| H-27 | MED | EVIDENCE | `regions[]` overlaps; not a faithful coverage map | RT×1 + SRC (ONE) | 3 |
| H-28 | MED | OPS | SIGINT kills the process; exit 4 documented but not implemented | RT×1 + SRC×1 + docs | 3 |
| H-29 | HIGH | EVIDENCE | B-06: recall 0/3 on realistic layouts, budget burned on ordinary images, wall-clock non-determinism, weak acceptance | RT×2 + SRC×2, CONFLICT C6/C7 | 3 |
| H-30 | HIGH | PROCESS | Gates G7–G9 absent; fuzz/repro/packaged-binary not in CI; coverage floor 60 % | SRC×5 | 4 |
| H-31 | MED | PROCESS | Status labels overstated / inconsistent (A-03, A-04, A-06, A-10, A-13, A-14, C-09, D-01, D-03) | SRC/RT×3 | 4 |
| H-32 | MED | PROCESS | Evidence cited in docs not in the tree; no CI run covers HEAD | SRC×4 | 4 |
| H-33 | MED | DOCS | Documentation drift (README speed claim, "318/318 verified", flag table, limitation 4) | ONE each | 4 |
| H-34 | LOW | QUALITY | A-10 limits: functions > 120 lines, `missing_docs`, pedantic | ONE (heuristic) | 4 |
| H-35 | — | SCIENCE | Backlog: benchmark science gaps | SRC×4 | 5 |
| H-36 | — | SCOPE | Backlog: open guide items (B-01, B-03, B-09…B-11, EBR, block devices, …) | SRC×5 | 5 |

### 6.1 Phase 0 — baseline, instrumentation, process

#### H-01 — Guide truncated / not in the repository `HIGH` `MUST` `PROCESS`
- **Reviews:** v1:R-12, v2:R-12, v3:R-13, v4:F-10, v5:F-18 (all SRC). `docs/DECISIONS.md` D-01: the agent's `guide.md` stopped at §3.5, so `guide-v1/v2/v3` were used; no guide is committed; `scripts/gate.sh` header still says "guide-v2"; history is 6 commits, none per item. (v2 refinement: `PROGRESS.md`/`REVIEW_RESPONSE.md` already use the unified item IDs — what is stale is the *acceptance text*: S9/S10 thresholds, A-04 semantics, G7–G9 tooling.)
- **Fix:** place `harness.md` at the root; add the user's 924-line guide verbatim as `docs/guide.md` (verify with the sha256 in the header; if missing → §3.5 item 8); add a `DECISIONS.md` entry that supersedes D-01 (guide now complete; `guide-v1/2/3` are history); fix the `gate.sh` header; from now on every commit is `type(scope): summary [H-xx]`, one finding per commit; add `scripts/check_commit_ids.sh` (CI, warning-only for pre-existing history).
- **Accept:** `wc -l docs/guide.md` = 924 and sha256 matches; `git log --oneline <baseline>..HEAD` shows one `[H-xx]` per fix commit.

#### H-02 — The gate cannot pass on a clean checkout `HIGH` `MUST` `PROCESS`
- **Reviews:** v5:F-11 (RT: `review_repro.py --assert` prints `FAIL … SKIP` for R7/R8 and **exits 1** when the git-ignored images are absent), v1:R-09 (RT: R7/R8 NOT RUN; no `scripts/check-msrv.sh`, no `rust-toolchain.toml`; `review_repro.py` not run in CI), v2/v3/v4 (SRC).
- **Fix:** (a) `scripts/make_fixtures.sh` (or Python) builds the D0-equivalent images **without root or loop mounts** — v5 showed this works: `mkfs.vfat -F 32 -S 512 -s 1` + `MTOOLS_SKIP_CHECK=1 mcopy/mdel`, `mkntfs -F -Q -s 512 -c 4096`, MBR multi-volume disk written by Python — into a git-ignored `target/fixtures/`; (b) `review_repro.py`: a missing prerequisite prints `SKIP (reason)` and is **not** a failure unless `--require-all`; (c) `rust-toolchain.toml` pinning one concrete stable version (≥ 1.85, recorded in DECISIONS) plus `scripts/check-msrv.sh` that reads `rust-version` from `Cargo.toml`; (d) `scripts/gate.sh` per §4.2.
- **Accept:** fresh clone, no root: `scripts/gate.sh` exits 0, `SKIPPED` lines only for genuinely missing external tools.

#### H-03 — Red-first Rust regression, adversarial and property tests `HIGH` `MUST` `PROCESS`
- **Reviews:** v1:R-16, v5:F-10 (SRC), v2 (G2 note). `tests/` has no `regress_*.rs`; the 5 proptests are extent merging/complement and three "never panic on random ≤ 4 KiB bytes" checks (random bytes rarely form valid structure); there is no splice/cut property, no `%%EOF`-flood or PNG-bomb test; `review_repro.py` lacks S2c, S3, S8b, S10, S12, S13; the 5 `evidence_formats` tests round-trip self-generated dense images and did not catch H-11/H-12/H-22/H-23.
- **Fix:** (a) `tests/support/` builders: PNG/JPEG/PDF mutators, hostile E01/VMDK/VHD writers, FAT32/NTFS via `mkfs` when present, otherwise skip; (b) one `tests/regress_<area>.rs` per finding using the Appendix-A recipes, header comment `// H-xx`, created red with `#[ignore = "H-xx xfail"]`; (c) property tests: *cutting a valid file anywhere never yields `verified` and the output is ≤ cut + 1 cluster* (guide A-05), *splice property* (H-18), *structured-mutation never-panic* (mutate valid files, don't use random bytes); (d) Python `X01…X24` in `scripts/review_repro.py` plus the missing S-scenarios; (e) red baseline saved to `docs/baseline/harness_repro_v0.2.0.txt`.
- **Accept:** every Appendix-A scenario exists in the Python harness and (where feasible) as a Rust test; the gate is green through `scripts/repro_xfail.txt`.

#### H-04 — Release identity and package hygiene `MED` `MUST` `PROCESS`
- **Reviews:** v1:R-13/R-21 (RT: `dist/carvex-linux-x86_64`, checksums verify, reports `carvex 0.2.0` but lacks `--input-format`/`--fragment-recovery`; HEAD builds also report `0.2.0`), v2:R-13/N-09/N-10, v3:R-14, v4:F-11, v5:F-19.
- **Fix:** (a) `Cargo.toml` `repository = "https://github.com/jamesinlogos/carvex"`; (b) `build.rs` (or `vergen`) sets the commit (`git rev-parse HEAD`, `-dirty` suffix, fallback env `CARVEX_GIT_COMMIT`, then `unknown`); `carvex --version` and `tool_info.commit` show it; (c) **version and `dist/` (user-approved, §3.6):** set `version = "0.3.0-dev"` in `Cargo.toml` and commit the `Cargo.lock` change (any test that hard-codes `0.2.0` is derived from `env!("CARGO_PKG_VERSION")` in the same commit) so HEAD builds are distinguishable from the released 0.2.0; until the release step add `dist/README.md` stating the binaries were built at `cbee72d`, predate B-04/B-06 and must not be used for evidence; at the release step (§7) rebuild both binaries from the tagged commit, **replace** the old files (never delete the directory), regenerate `dist/SHA256SUMS` and rewrite `dist/README.md` (commit, rustc, target, build command, date); a release check compares `dist/*` `--version` commit with the tag; (d) `/home/kali/...` in `bench/images/*.json` and `bench/results/final/**/*.log`: **do not edit** frozen data (§3.2) — record the caveat in `DECISIONS.md` and fix the generator for future runs; (e) the Windows `.exe` is addressed in H-30/H-32.
- **Accept:** `cargo package --list` succeeds; a source build from git reports `0.3.0-dev` and a real commit in `--version` and in `tool_info.commit`.

### 6.2 Phase 1 — hostile-input safety

#### H-05 — PNG `--validate decode` inflates without bound `CRIT` `MUST` `SAFETY`
- **Reviews:** v1:R-02, v2:R-02, v3:R-03, v4:F-02, v5:F-02. **RT×4:** v1 1,065 KB 32000×32000 grey → **1,063 MiB** RSS (`structure` 20 MiB); v2 510 KiB → 547 MiB, 1,019 KiB → 1,061 MiB, 2,038 KiB → 2,088 MiB (linear); v4 995 KB → 1.03 GiB; v5 261 KB (16×16, 256 MiB inflated) → **325 MiB**. Default `structure` is unaffected; B-06 stitch verification inherits the exposure (v1:R-20, v2:N-07).
- **Where:** `validate_png` (concatenates IDAT, `read_to_end`, compares to the expected size afterwards); `png_rows` builds a row-offset `Vec`.
- **Fix:** (1) compute the IHDR-derived raw size with checked arithmetic (incl. Adam7) **before inflating**; if it exceeds `--max-validator-memory` return `validation=unverified` / `validity=structural` with a note naming the limit, allocating nothing; (2) inflate with `Read::take(expected + 1)` into a fixed/streaming buffer, check filter bytes row by row, stop at the first mismatch; (3) do not copy all IDAT payloads into a second `Vec` (chain readers); (4) compute row offsets arithmetically for large dimensions; (5) new option `--max-validator-memory SIZE` (default 256 MiB) shared with H-06/H-29, documented in README and `CHANGELOG` (Added).
- **Accept:** X02a ≤ 32 MiB RSS and < 100 ms with a not-verified label; X02b rejected by budget before inflation; control: valid 800×600 PNG unchanged and still `decoded`.

#### H-06 — JPEG SOF dimensions drive an unbounded allocation `CRIT` `MUST` `SAFETY` **ONE**
- **Reviews:** v2:N-02 (RT): baseline 640×480 corpus JPEG with SOF0 patched to 20,000×20,000 → `decode` 7.70 s, **1,719 MiB**; 65,535×65,535 → **killed (rc −9) at 5,948 MiB, no report** (`structure`: 0.02 s). Mechanism: the decoder allocates the frame from the header.
- **Fix:** before invoking the decoder, `width × height × components` (checked) must be ≤ `--max-validator-memory`; use the decoder's own buffer-limit API if the pinned `jpeg-decoder` version has one (verify), else the pre-check above; result: `validation=unverified`, `validity=structural`, note "claimed frame size exceeds validator budget". **[SHOULD]** plausibility vs available entropy bytes: a Huffman baseline block needs ≥ 2 bits (DC symbol + EOB) ⇒ at most ≈ 256 samples per entropy byte (derived by the harness author — re-derive and record in `DECISIONS.md`); reject claims above ≥ 2× that margin; **do not** apply to arithmetic-coded (SOF9+) files.
- **Accept:** X03 completes with ≤ 64 MiB RSS and the not-verified label; the same two files in `structure` mode unchanged.

#### H-07 — PDF `%%EOF` evaluation is O(E×N) `HIGH` `MUST` `SAFETY`
- **Reviews:** v1:R-06, v2:R-06, v3:R-06, v4:F-06, v5:F-03. **RT×3:** v1 4,096 markers → 18.31 s (32 MiB) / 18.63 s (64 MiB), 1,024 → 2.39 s; v2 63 MiB / 4,368 markers → **18.05 s** (control `%%EOX` 0.14 s; doubling size and markers ×3.7); v5 32 MiB: 1 → 0.12 s, 256 → 1.36 s, 1,024 → 4.6–5.5 s, **4,096 → 18.7–19.2 s**, RSS 72 MiB (pure CPU, default options). **CONFLICT C2:** v4's fixture stayed ≈ 2 s — the slow path needs a revision-like tail after each marker.
- **Where:** `find_eofs` calls `count_objects(&data[..end])` and a `/Root` search on the whole prefix for every `%%EOF` (≤ 4,096 candidates).
- **Fix:** one pass over the window: record the offsets of `N G obj` occurrences once (sorted) and the first `/Root` offset once; per `%%EOF` derive `obj_count = partition_point(offsets, end)` and `root_present = first_root < end` (O(N + E log N)); bound `looks_like_more_revision` to a ≤ 64-byte window; keep the re-parse after `Need::More` geometric. **Outputs must not change** (A-01 scenarios S2, S2c, S3 and the six `qpdf` variants stay byte-identical; G7 unchanged).
- **Accept:** X04: 4,096 markers at 64 MiB ≤ 1 s (guide A-06 item 8); `time(E=4096) / time(E=1) ≤ 10` (relative test, CI-stable); v2's `%%EOX` control and v4's fixture stay fast.

#### H-08 — `--max-memory` is not a memory budget `CRIT` `MUST` `SAFETY`
- **Reviews:** v1:R-01, v2:R-01, v3:R-01, v4:F-01, v5:F-01. **RT×3:** v1 150 PNGs (200 MiB payload), `--max-memory 16M`, 2 threads → **209 MiB RSS, exit 1, 0 files**; v2 1.05 GiB of JPEG copies at the default 1 GiB cap → **exit 1, RSS 1,087 MiB, 0 files, no report** (a 1.00 GiB payload passed: RSS ≈ total output, so the cap saves nothing); v5 100 PNGs, cap 16M → exit 1, 107 MiB (6.7× cap), 0 files, **no report**. v4 confirmed B-06 output is included in the (still post-hoc) sum.
- **Cause:** `carve_candidates` keeps every carved `Vec<u8>` until `postfilter` and the sum-versus-cap check; on excess the run exits 1 and discards everything. A host with less RAM than the output is OOM-killed before the check runs.
- **Fix (design constraints — implementation is yours):**
  1. `Outcome` stores **ranges, not bytes**: fragments (B-06), length, trim/append transformation (repair/salvage), `sha256`, analysis, validity. Workers read into **reusable per-thread buffers**, analyse, hash, drop the bytes.
  2. The write stage **re-reads** from the read-only `ImageSource`, streams through hasher + temp file + atomic rename, and verifies the digest equals the analysis digest (mismatch ⇒ warning `SOURCE_CHANGED`, exit 3).
  3. `--max-memory` bounds **concurrent working memory** (worker buffers + validator scratch) via a budget with blocking acquisition; a worker that holds no permits may always proceed (no deadlock). **Results must not depend on scheduling or on `--threads`.** A candidate whose own requirement exceeds `--max-memory` is skipped with `CANDIDATE_SKIPPED_BUDGET` (offset, type, needed) — a static per-candidate rule, hence deterministic. Update the help text (today: "Hard cap on the total bytes held for carved output before writing").
  4. Any budget/limit event: write the report, emit the coded warning, exit **3** — never exit 1 with nothing written.
  5. Measure the double-read cost under G8; a hybrid (retain while under budget) only with an ADR, because two code paths multiply bugs.
- **Accept:** X01 — all 100 files written, report present, exit 0 (3 only if something was genuinely skipped), peak RSS ≤ 64 MiB at `--max-memory 16M`; a 1 GiB-class input at `--max-memory 64M` completes with RSS ≤ 256 MiB; output SHA-256 set identical to before for D0 (G7). `CHANGELOG` → Changed (semantics of `--max-memory`).

#### H-09 — Required work budgets are absent `HIGH` `MUST` `SAFETY`
- **Reviews:** v5:F-04 (RT: `scan --help`), v2 (RT), v1:R-01, v3:R-01 (SRC). Present today: `--max-size`, `--max-truncated-size`, `--max-memory`. Missing: `--max-candidates`, `--time-budget`, `--max-work-per-candidate` (guide A-06) and `--max-output` (A-15). Header floods are fast now (S9/S10), but H-05/H-07 show single candidates can still burn CPU or memory.
- **Fix:** add the four flags. `--max-candidates` and `--max-work-per-candidate` (bytes examined by analyzers + validators) are **deterministic work-unit** limits; `--time-budget SECS` is wall-clock ⇒ sets `nondeterministic_stop: true`; `--max-output SIZE` bounds bytes written. Defaults: chosen by measurement so they **never trigger** on D0, CFReDS RM#2, S9/S10 (S9-4M has 262,144 candidates; a 64 MiB flood16 has ≈ 4.2 M — v4) — record in `DECISIONS.md`. On excess: coded warning (`CANDIDATE_LIMIT`, `WORK_LIMIT`, `TIME_LIMIT`, `OUTPUT_LIMIT`), partial report, exit 3.
- **Accept:** X05 — each flag trips on its fixture with warning + report + exit 3 and never hangs; README flag table updated (G9).

#### H-10 — No per-candidate or container panic containment `HIGH` `MUST` `SAFETY`
- **Reviews:** v1:R-07, v2:R-07, v3:R-07, v4:F-07, v5:F-05 (SRC×5). `catch_unwind` exists only at `src/fs/mod.rs` ≈L373, L443. `carve_one` runs inside rayon `par_iter`; `try_fragment_join` and `src/image/*` have none. **No panic was ever triggered**: v2 1,680 mutants, v5 2,160 mutants (exit codes only 0/3) — a latent defence-in-depth gap.
- **Fix:** wrap analyse + validate of **each candidate** and **each fragment-join attempt** in `catch_unwind(AssertUnwindSafe(…))`; convert a panic to an `internal_error` outcome (offset, type, message), count it in stats, warning `CANDIDATE_PANIC`, exit 3, continue; wrap `open_image` so a container-parser panic becomes a typed error. Keep `panic = "unwind"` in every profile (add a test that reads `Cargo.toml`).
- **Accept:** X06 — a test-only carver/validator that panics for one candidate yields a report with `internal_error` and all other candidates carved (inject through the format registry; add a `#[doc(hidden)]` seam only if the registry cannot).

#### H-11 — E01 allocates from untrusted header fields; stdin spool unbounded `CRIT` `MUST` `SAFETY`
- **Reviews:** v5:F-21 (RT), v1:R-19, v2:N-06 (SRC). **CONFLICT C3** (v4 claimed a bound; refuted). Crafted E01 with `sectors_per_chunk = 2²⁷` (64 GiB chunk) and one tiny compressed chunk → **SIGABRT** "memory allocation of 68719476736 bytes failed"; the consistency check against the table does not help because the attacker also sets the media size. `load_chunk` does `vec![0u8; chunk_size]` per compressed chunk. `open_stdin` spools to a `NamedTempFile` with no limit.
- **Fix:** `MAX_EWF_CHUNK` constant (≤ 64 MiB hard cap — legitimate chunks are 32 KiB; record in `DECISIONS.md`); validate `sectors_per_chunk`, `bytes_per_sector` (plausible power of two), `sector_count × bytes_per_sector` (checked), chunk count vs table entries vs file length; allocate with `try_reserve_exact` sized `min(chunk_size, remaining media bytes)` and **reuse** a per-thread buffer; typed errors. Stdin: `--spool-dir`, map `io::ErrorKind::StorageFull` (stable since Rust 1.83) to a clean error with temp-file removal on every path, optional `--max-stdin-bytes`. Extend the deny-lints to `src/image/` (§3.3).
- **Accept:** X07 — `info` and `scan` exit 1 with a typed error in < 1 s and < 64 MiB RSS (no abort).

#### H-12 — VMDK grain tables multiplied in memory; ZERO extent size `CRIT` `MUST` `SAFETY`
- **Reviews:** v1:R-17 (RT), v5:F-21 (RT); **CONFLICT C4** (v2 "range checks look correct" superseded). v1: a 1.00 MiB file declaring 128 GiB → **1,029 MiB** RSS (0.25 MiB / 8 GiB → 68 MiB); v5: a 4.49 MiB file with 128 grain-directory entries all pointing at one 4 MiB table → `info` **520 MiB** (≈ 116×). `num_gte` only has to be non-zero, `grain_size` a power of two; every directory entry copies a full `num_gte × 4`-byte table. **ONE (v5):** flat extent + `RW 1099511627776 ZERO` (512 TiB) → `scan` killed by SIGKILL after 3.5 s (OOM suspected, unconfirmed).
- **Fix:** cap `num_gte`, `gd_count` and total table bytes against the file length (and an absolute ceiling ≤ 256 MiB, recorded); load tables **lazily** through an LRU cache keyed by table offset so identical offsets share one table; represent `ZERO`/logical size as ranges and generate scan chunk tasks lazily — never precompute a per-chunk task vector from a declared size; typed errors.
- **Accept:** X08 — (a) `info` on the crafted sparse file: ≤ 64 MiB RSS, < 1 s; (b) flat + 512 TiB ZERO: error or clean completion, never a kill.

#### H-13 — VMDK descriptors can read arbitrary or wrong files `HIGH` `MUST` `SAFETY`
- **Reviews:** v1:R-18 (RT: absolute path and `../outside/secret.img` made `scan` carve a PNG whose SHA-256 equals the outside file), v5:F-23 (RT: `"/bin/ls"` accepted as the image; a monolithicSparse VMDK copied to another name fails, and if a file with the original name sits next to it **that other file is read silently**). Source-change detection already tracks referenced files (v1 positive).
- **Fix:** for a monolithic sparse file ignore the descriptor's file name and use the opened file; other extents resolve relative to the descriptor's directory, are canonicalised and must stay inside it — reject absolute paths, `..` escapes and symlink escapes unless `--allow-external-extents`; record every resolved file in `input.files[]`.
- **Accept:** X09 — absolute/`..`/symlink escapes rejected; renamed monolithicSparse copy scans its own data; sibling-swap case no longer reads the sibling.

#### H-14 — No fuzz targets for the new parsers `HIGH` `MUST` `SAFETY`
- **Reviews:** v1:R-19, v2:N-06, v4:F-14, v5:F-10 (SRC×4): `fuzz/fuzz_targets` still holds the original 7; none for E01, VHD, VMDK, split-raw, `open_image` dispatch, or `try_fragment_join`, although they parse attacker-controlled headers (≈ 1,289 new lines).
- **Fix:** `fuzz_e01`, `fuzz_vhd`, `fuzz_vmdk`, `fuzz_split_names`, `fuzz_open_image`, `fuzz_fragment_join`; seeds from `tests/evidence_formats` fixtures, real-tool containers if available (`ewfacquire`, `qemu-img`) and the hostile X07–X09 samples; every crash → minimised fixture in `tests/fixtures/crashes/` + regression test; structured-mutation property tests for `src/image/`.
- **Accept:** each new target runs 60 s clean (`SKIPPED` with reason if `cargo-fuzz` is missing); targets listed in the CI matrix (H-30).

#### H-15 — Parallel carve slower than one thread `MED` `SHOULD` `PERF` **conditional**
- **Reviews:** v2:N-03 (ONE): `dist/` musl binary, 2 vCPU — S9-4M 6.37 s (`--threads 1`) vs 10.87 s (default), S10 3.36 s vs 5.23 s, 8/8 flood rows ≈ 1.6× slower in parallel. **CONFLICT C5:** v1/v4/v5 on source builds: 0.78–1.13 s (S9-4M), 0.50–0.57 s (S10).
- **Procedure:** X23 on **both** a glibc source build and the musl build, `--threads 1` vs default. (a) Only musl slow ⇒ release-build issue: record; remedies are buffer reuse (guide A-06 item 4, also done by H-08) and, only after STOP-AND-ASK, a different allocator. (b) Both slow ⇒ profile; expect per-candidate allocation; add buffer reuse + the cheap pre-filters (guide A-06 item 2). (c) Neither ⇒ close as NOT-REPRODUCED on source builds.
- **Accept:** `--threads N` is never slower than `--threads 1` by > 10 % on X23, or the finding is closed with measurements.

### 6.3 Phase 2 — validity and honest labels

Mapping used by H-16…H-21 (ADR-H1) — **legacy fields keep their meaning; `validity` is new, additive and authoritative:**

| `validity` (new) | Legacy `validation` | Legacy `status` | Meaning |
|---|---|---|---|
| `verified` | `decoded` | `complete` | `--validate decode`; **every mandatory check for the type was implemented and passed** |
| `structural` | `structure` (default mode) **or** `unverified` (decode mode, a mandatory check is unsupported) | `complete` | structure parses; **content not checked** |
| `suspect` | `partial` | `complete` | footer/structure found but ≥ 1 integrity check **failed** (PNG CRC, PDF xref/catalog, JPEG strict scan) — still written, never dropped |
| `partial` | `partial` | `truncated` | truncated, salvaged or prefix-only |
| `repaired` | `partial`/`decoded` | `repaired` | repair applied **and the result re-validates** (H-21) |

Mandatory checks per type: **JPEG** strict baseline scan consistent with the frame (+ decoder); **PNG** every chunk CRC, IDAT inflate length = IHDR formula, Adler-32, filter bytes ≤ 4, chunk order; **PDF** (after H-19 stage 2) xref entries → `N G obj`, `/Root` → `/Type /Catalog`, `/Prev` chain. `score` and emission are **unchanged** (golden stays identical). **Zero-filled bytes are not evidence (H-22):** an output intersecting an unreadable region is at best `suspect` (check `overlaps_unreadable_region`).

#### H-16 — Validity model absent; default output reads "complete / 100" `CRIT` `MUST` `VALIDITY`
- **Reviews:** v1:R-08, v2:R-08, v3:R-08, v4:F-08, v5:F-08 + F-17 (RT×3: legacy `status=complete` stays on every `partial` row; spliced JPEG in default mode is `complete / structure / 100`, 394,746 B; the console line `[+] JPEG offset=… score=100 complete NAME` and `SHA256SUMS` show no validation level). D-22 (keep the field name `validation`) is accepted; what is missing are the **semantics**.
- **Fix:** add per-file `validity` and `checks[]` (`{name, ok: true|false|null, note}`) per the table above; fix `validated_bytes` to mean bytes verified contiguously from the start; summary counts, console lines, manifest and CSV show `validity`; **only `verified` is ever labelled complete/verified in a summary, count or benchmark; `structural` is written "content not checked"**; `--complete-only` keeps its legacy meaning but its help says "footer found, not verified". Update `docs/report.schema.json` (+ conformance test), `docs/REPORT.md`, and correct `REVIEW_RESPONSE.md` A-04. Do **not** suppress files or change scores. Keep `source` values `carve`/`bgc` (C12).
- **Accept:** schema validates; X10–X14 labels as specified; default-mode spliced JPEG is `structural` and the console says "content not checked"; G7 unchanged.

#### H-17 — PNG bad CRC still `complete` / `decoded` `HIGH` `MUST` `VALIDITY`
- **Reviews:** v1:R-03, v2:R-03, v3:R-02, v4:F-03, v5:F-06. **RT×3:** `png_02` (22 IDAT): 1 bad CRC → `complete/structure/85` and `decode` → `decoded/85`; 3 bad → 55/55; **all 22 bad → `decoded/55`** (v2); IHDR CRC wrong → `decoded/85`; one payload byte flipped → `partial/65` (zlib/Adler catches payload damage — C8); v5: 128×128 grey, IDAT CRC ^ `0xdeadbeef` → `decoded`, `validated_bytes` = whole file. Score floor 55 > `--min-score 50` ⇒ written (correct for evidence); Pillow also loads the 3-bad-CRC file. The unit test `reports_bad_crc_but_keeps_file` asserts the current behaviour.
- **Fix:** any chunk-CRC mismatch ⇒ `validity=suspect` (legacy `validation=partial`, `status` unchanged) with `checks[]` naming the chunk type/offset; recompute **all** CRCs in decode mode; `validated_bytes` stops at the last chunk whose CRC is good; keep the file and score. Apply exception E-1 to the existing test (extend, don't weaken). Correct README limitation 4.
- **Accept:** X10 — every CRC-damage row is `suspect` and still emitted; payload-flip row stays `partial`/`suspect`; controls unchanged.

#### H-18 — JPEG strict-scan `None` falls through to `decoded`; dead `Unverified`; splice rate disputed `HIGH` `MUST` `VALIDITY`
- **Reviews:** v1:R-05, v2:R-05, v3:R-05, v4:F-05, v5:F-08 (SRC×5; clean progressive JPEG measured `decoded/100` by v2, v5; `Validation::Unverified` never constructed in `src/`; `REVIEW_RESPONSE.md` says "progressive → `unverified`"). **CONFLICT C1** on the splice rate.
- **Steps:** (0) **measure first** on unmodified code: property test with ≥ 1,000 independent pairs (≥ 50 distinct images per content type — noise, gradient+noise, photo-like from the corpus; ≥ 640×480; baseline and progressive; cuts aligned to 512 and 4,096 B inside [10 %, 90 %] of the shorter file; payload `B[:cut] + A[cut:]`; count outputs that would be labelled `decoded`/`verified` with SHA-256 ∉ {A, B}; report rate with Wilson CI) → `docs/baseline/jpeg_splice_v0.2.0.txt`; (1) three-way match on the strict scan: pass / fail / **unsupported** — unsupported ⇒ `validation=unverified`, `validity=structural`, `checks += {strict_scan: null, "unsupported variant"}`; docs and code now agree; (2) if step 0 shows an accepted rate > 0.1 % for the top label, add RSTn sequence checks (when DRI present), MCU count against the SOF geometry and "decoder consumes exactly to EOI" and re-measure; (3) progressive/multi-scan strict support **[MAY]**.
- **Accept:** X12 (a) rate ≤ 0.1 % for `verified` (guide A-04; recorded with CI); (b) clean progressive JPEG under `decode` ⇒ `unverified`/`structural`; (c) the two progressive splices stay not-verified. `CHANGELOG` → Changed (decode mode only).

#### H-19 — PDF `decoded` is a lexical check `HIGH` `MUST` `VALIDITY`
- **Reviews:** v1:R-04, v2:R-04, v3:R-04, v4:F-04, v5:F-07. **RT×2 with the `qpdf --check` oracle:** one xref entry offset +7 → qpdf exit 3, CarveX `decoded/100`; `/Root` → non-existent object → qpdf exit 2, CarveX `decoded/100` (v1); 3-object PDF with all xref offsets at 9 and `/Root` → a `/Type /Page` → qpdf rc 3, CarveX `decoded/100` (v2). `validate_pdf` checks only a trailing `%%EOF`, the substring `/Root`, and the startxref target. Both reviewers note the files are repairable — an overstated label, not a destroyed file.
- **Fix, stage 1 (immediate, label honesty):** PDFs never receive `validity=verified`; under `decode` they get `validation=unverified`, `validity=structural`; `checks[]` lists exactly what ran (`startxref_target`). **Stage 2:** classic xref (each in-use entry points at the right `N G obj`, entry count bounded by the budget); xref streams (`/Type /XRef`, `/W`, `/Index`, inflate bounded with `take`); `/Root` resolves to `/Type /Catalog`; `/Prev` (depth ≤ 8, cycle-safe) and `/XRefStm`; `/Size`; direct `/Length` vs `endstream`; `/Encrypt` flagged (structure verified, content note). Any mismatch ⇒ `suspect` (written, not dropped). Real-world PDFs with small offset errors become `suspect` **by design** — record the rate on the corpus.
- **Accept:** X11 — (a)(b)(c) not verified; six controls (plain Pillow, `qpdf --linearize`, `--object-streams=generate`, both, AES-256 `--encrypt "" owner 256 --`, hand-built incremental) stay **byte-identical** to the input and become `verified` only when `qpdf --check` exits 0; no valid corpus PDF becomes `suspect`.

#### H-20 — PDF ending `%%EOF\r\n` / `\r` demoted to `partial` `MED` `MUST` `VALIDITY`
- **Reviews:** v2:N-01 (RT), v5:F-09 (RT) — identical numbers: ending `\n` / none → `decoded/100`; `\r\n` and `\r` → **`partial/80`** "selected PDF revision does not end with %%EOF". `find_eofs` already includes CR/LF in the analysis end; `validate_pdf` accepts only `%%EOF` or `%%EOF\n`. Conservative error (under-claims) but depresses the rate on Windows-generated PDFs.
- **Fix:** trim trailing CR/LF/whitespace/NUL before the `ends_with` test; unit tests for LF/CRLF/CR/none.
- **Accept:** X13 — the four variants give the same validity; output bytes unchanged.

#### H-21 — `--repair` labels unreadable PDFs `repaired`; no hash of the original `HIGH` `MUST` `VALIDITY`
- **Reviews:** v5:F-15 (RT: `pdf_03_text_2pages.pdf` cut at 70 % → `repaired / partial / 60`, 4,354 B, "repaired by appending the signature footer", pypdf: "startxref not found"; no hash of the unrepaired bytes), guide S15/B-07, v1/v2/v3 (no `--salvage`; `repaired` not re-validated). PNG without IEND *is* salvaged by default (v5 R4b) — keep.
- **Fix:** `repaired` only if the repaired bytes re-validate to at least the level an intact file of that type reaches (PDF: rebuild xref + trailer from a scan of `N G obj` and `/Root`; JPEG: append EOI; PNG: append a valid IEND); otherwise keep `status=truncated`, `validity=partial`, note "repair attempted, not validated". Always emit `sha256_original` when output bytes differ from bytes read; a repaired file never counts as `exact`. `--salvage` **[SHOULD]**: cut at the last valid marker/chunk, labelled `partial` with `validated_bytes`.
- **Accept:** X14 — `repaired` iff pypdf (non-strict) opens it or the validator re-validates; `sha256_original` present in both cases.

### 6.4 Phase 3 — evidence sources, operations, fragment recovery

#### H-22 — E01 reader aborts the whole scan on a partial final or damaged chunk `CRIT` `MUST` `EVIDENCE` **ONE**
- **Reviews:** v5:F-20 (RT with real libewf). A libewf-made, `ewfverify`-clean E01 of an image with 17 sectors beyond a whole number of 64-sector chunks fails in `--mode raw` with "cannot decompress EWF chunk at byte …" (**exit 1, no report**); hand-built partial final chunks (compressed and uncompressed) fail the same way; one flipped byte inside a chunk fails in default **and** raw mode ("checksum mismatch in EWF chunk"). The default mode passed on the fixture only because the volume ended before the tail — but the default scan also scans gaps (A-07 step 2), so trailing unpartitioned space of a whole-disk image reads the tail (1,953,525,168 sectors ÷ 64 leaves 48). `load_chunk` assumes every chunk is exactly `chunk_size` bytes.
- **Fix (default confirmed by the user, §3.6):** (1) last-chunk length = `min(chunk_size, media_bytes − index·chunk_size)` (compressed: `read_exact` of that length; uncompressed: length + 4-byte checksum). (2) Add `--on-read-error {abort|zero-fill|skip}` (guide A-08), **default `zero-fill`**, routing E01 checksum/inflate failures **and** raw read errors through it: `zero-fill` — the unreadable bytes read as `0x00` at the source's granularity (an E01 chunk, or a sector after retrying a failed block read sector by sector to narrow the region); `skip` — any candidate needing bytes from the region is skipped with `CANDIDATE_SKIPPED_UNREADABLE`; `abort` — stop, **still write a partial report**, exit 1 (the pre-0.3.0 behaviour). (3) Every region is recorded in `unreadable_regions[]` (`{offset, len, reason}`) with warning `UNREADABLE_REGION`; the run exits 3. (4) **Zero-filled bytes are not evidence:** an output whose bytes intersect an unreadable region gets `checks += {overlaps_unreadable_region: false}` and `validity=suspect` (never `verified`), and its report entry says so. (5) `CHANGELOG` → Changed (default flipped; restore with `--on-read-error abort`). Rationale: libewf offers a zero-chunk-on-error mode and imaging tools zero-fill unreadable chunks; D-29's "fail instead of returning partial data" is the wrong default for an evidence reader.
- **Accept:** X16 — (a) the libewf fixture scans in raw mode with exit ≤ 3 and tail offsets readable; (b) default (`zero-fill`): a one-chunk-corrupt E01 completes with `UNREADABLE_REGION`, `unreadable_regions[]` equals that chunk, files outside it are recovered exactly, any output overlapping it is not `verified`; (c) `--on-read-error abort`: exit 1 with a partial report; (d) `skip`: overlapping candidates skipped, the rest unchanged; (e) a mock `ImageSource` failing at offset X on a raw file behaves the same (guide A-08); (f) multi-segment (`.E02`) fixture if `ewfacquire -S` is available, else SKIPPED.

#### H-23 — Unallocated VMDK grain tables read as errors, not zeros `MED` `MUST` `EVIDENCE` **ONE**
- **Reviews:** v5:F-22 (RT on a qemu-made image with one directory entry zeroed): `scan` exit 1 at the first read into the range ("VMDK grain index out of range"), no report. Spec: a zero directory entry means the whole table reads as zeros (`qemu-img` does this). Thin-provisioned disks have such entries; the 5 fixture tests use dense images. (v5 corrected itself: qemu-made images have all tables allocated; failure needs a zero entry.)
- **Fix:** a missing table or entry returns zeros. **Accept:** X17 — scan output identical to the raw conversion of the same disk. Keep the explicit refusal of `streamOptimized` extents.

#### H-24 — Auto-detection reinterprets ordinary raw images `MED` `SHOULD` `EVIDENCE` **ONE**
- **Reviews:** v5:F-25 (RT): a raw FAT32 image whose last sector starts with `conectix` → exit 1 "VHD footer checksum mismatch"; with `# Extent description` in its first 64 KiB → exit 1 "VMDK descriptor has no extents"; `--input-format raw` works. Contradicts "ordinary raw images keep the unchanged `RawFile` path". (v4: auto-detection is magic-first — true, but not validated.)
- **Fix:** choose a container only after its header/footer validates (VHD: cookie **and** checksum **and** plausible fields; VMDK: `KDMV` magic or a descriptor with ≥ 1 extent); otherwise fall back to raw with note `INPUT_FORMAT_FALLBACK`; record `input.format` (`raw|split|e01|vhd|vmdk|stdin`) in the report.
- **Accept:** X18 — both fixtures scan as raw.

#### H-25 — Container reports mix logical and physical sizes; stdin identity `LOW` `SHOULD` `EVIDENCE` **ONE**
- **Reviews:** v5:F-26 (RT/SRC): for containers `input.size` is the logical size while `input.size_after` is the physical size (VMDK 67,108,864 vs 14,614,528; compressed E01 67,108,864 vs 14,514,325) yet `unchanged: true`; format, segments and resolved extents are not recorded; for stdin `modified_unix` is null while `unchanged` is `true`.
- **Fix (additive):** `input.logical_size`, `input.physical_size` (+ `_after`), `input.format`, `input.files[]`; legacy fields untouched; stdin: `unchanged: null`. Spool limits are H-11.
- **Accept:** X22 — fields present and consistent for VMDK, compressed E01 and stdin; schema test.

#### H-26 — Unparsable volume in a multi-volume disk is downgraded silently `MED` `MUST` `EVIDENCE` **ONE**
- **Reviews:** v5:F-13 (RT): the S8-corrupted NTFS inside the 98 MiB disk → `scan` **exit 0, `warnings[]` empty**, console silent; `info`/`volumes[]` list one volume; `regions[]` shows the NTFS range as `unsupported, scanned=true` (scanned raw, so bytes are not lost, but allocated and deleted content are mixed without `RAW_SCANS_ALLOCATED`). `VolumeSet` collects parse `errors` that never reach the report. The single-volume S8 case warns, but with a misleading text ("no FAT32/NTFS filesystem found…" although an NTFS boot sector *was* found).
- **Fix:** surface `VolumeSet.errors` as `VOLUME_PARSE_FAILED` (index, offset, underlying reason) in `warnings[]` and on the console, exit 3; mark the fallback region `kind: unsupported`, `may_contain_allocated: true`; make `FS_DETECTION_FAILED` say what happened ("NTFS boot sector at … but `$MFT` unreadable: …"); `info` keeps exit 1 with the specific cause (guide S8b). **Accept:** X19; S8/S8b still do not abort.

#### H-27 — `regions[]` is not a faithful coverage map `MED` `SHOULD` `EVIDENCE` **ONE**
- **Reviews:** v5:F-16 (RT + SRC): per volume one `free` and one `allocated` region **both spanning the whole volume**, distinguished only by `scanned`; on the 98 MiB disk the scanned regions sum to 102,760,448 B vs `bytes_scanned` 95,367,168 B (+7.05 MiB, SELF ✓); `info --json` on a standalone FAT32 shows a 64 MiB `free` region although only 59.4 MiB is free.
- **Fix:** emit the real extents (merged free extents, allocated complement, gaps, unsupported) with a `--max-regions` summarisation cap that preserves the invariants; additive schema note (`extent_id`).
- **Accept:** X20 — regions sorted, non-overlapping, `Σ len(scanned) == stats.bytes_scanned`, covering the image; schema test.

#### H-28 — SIGINT kills the process; exit 4 documented but not implemented `MED` `MUST` `OPS`
- **Reviews:** v5:F-14 (RT: SIGINT 2 s into a 19 s scan → process dies from the signal, rc −2, no outputs, no report), v2:A-13 (SRC: no signal-handler dependency), docs: `ARCHITECTURE.md` "Interrupted (SIGINT) → exit 4". The `stop: AtomicBool` in `scan.rs` is not wired to a signal.
- **Fix:** install a handler (`ctrlc` or `signal-hook`; record in `docs/DEPENDENCIES.md`; no `unsafe` in this crate): first signal sets the stop flag, workers stop taking candidates, completed outputs stay (writes are atomic), a partial report with `interrupted: true` and warning `INTERRUPTED` is written, exit **4**; a second signal exits immediately (130). Works on Windows (Ctrl-C event).
- **Test note (v5):** background jobs started from a non-interactive shell ignore SIGINT — spawn the child directly and signal that PID.
- **Accept:** X21 — exit 4, partial report, no half-written files.

#### H-29 — B-06 fragment recovery: no demonstrated recall, budget burned on ordinary images `HIGH` `SHOULD` `EVIDENCE`
- **Reviews:** v1:R-20 (SRC), v2:N-07 (SRC), v4:F-16 (RT), v5:F-24 (RT). **Depends on H-16–H-19, H-05, H-06, H-10.**
- **Facts:** the acceptance fixtures are tiny (file B = file A with one byte changed). On a realistic layout (64 MiB FAT32, 4 KiB clusters, three real files — 194,042 B JPEG, 1,398,788 B PNG, 623,620 B PDF — each split in half on a cluster boundary with a 3-cluster gap) **0 of 3** were stitched; runtime 5.1 s (the whole budget), `FRAGMENT_TIME_LIMIT`, exit 0 (v5). On the ordinary non-fragmented `mixed_fat32.img` the same 5 s were consumed and the warning appeared, with 20 identical files and no false stitch (v4). A bait (head of A + 2 junk clusters + tail of B) produced no stitch (v5). Mechanism: in default mode the contiguous carve reports the file *with the gap inside* as `complete / structure / 100`, so there is no truncated head to stitch; tails are enumerated from every free cluster. Acceptance of a stitch is `validate_deep == Decoded`, as weak as H-17–H-19. `--frag-time-limit` is wall-clock (non-deterministic, C7). First accepted join per head masks later correct ones; `confidence` is uncalibrated.
- **Fix (in order):** (0) record the baselines above; (1) work-unit budget authoritative; wall-clock only as a flagged safety net (`nondeterministic_stop`); (2) candidate heads = candidates whose **deep validation fails** (so `--fragment-recovery` runs decode-level validation of its candidates regardless of `--validate`; note the cost), tails enumerated by increasing gap with early exit when no plausible footer exists; (3) accept a stitch only if `validity == verified`; (4) every join attempt under the H-10 panic boundary and the H-05/H-06 validator budgets; (5) when a limit really truncates the stage ⇒ coded warning **and exit 3** (ADR-H5 — decided after the spurious hits are gone); (6) record `alternatives` count when several joins validate **[SHOULD]**; (7) measure on a D1 fragmentation dev profile (recall, `splice_rate`, junk FP with Wilson CI) and keep the feature **opt-in** until measured; update README limitation 9 and `DECISIONS.md` D-30.
- **Accept:** X15 — bait: no stitched output; ordinary image: no `FRAGMENT_*` warning, runtime within 10 % of the plain scan, identical output; realistic layout: recall > 0 with the measured value recorded (target set after measurement, never tuned on the lockbox); identical output for `--threads 1` vs default.

### 6.5 Phase 4 — gates, labels, evidence, documentation

#### H-30 — Gates G7–G9 absent; CI does not enforce the guide `HIGH` `MUST` `PROCESS`
- **Reviews:** v1:R-09, v2:R-09, v3:R-09, v4:F-09, v5:F-11 (SRC×5). `scripts/gate.sh` has no G7/G8/G9; fuzz runs only on `schedule`/`workflow_dispatch` (`ci.yml` ≈L110); `review_repro.py` not in CI; coverage floor 60 % vs 68.51 % reported vs targets 75/80/85 %; the packaged `.exe` is never executed; the last green CI run (`44548142275` on `42376a9`) predates B-04/B-06; no SBOM.
- **Fix:** (`scripts/golden.py` and `scripts/perf_gate.py` already exist from Phase 0 — P0.4, P0.7 — so Phase 1 could use them; this item wires them into CI and completes the rest.) `scripts/golden.py` (G7, output-hash sets — §4.2), `scripts/perf_gate.py` + `bench/perf/{scenarios, thresholds.toml, baseline-0.2.0.json}` (G8; thresholds committed before enabling), minimal `bench/render_docs.py --check` (G9: README flag table = `--help`, tables = generated results); CI: fuzz smoke 60 s per target on push/PR (matrix including the new targets), `review_repro.py --assert` job, a Windows job that builds the release binary and **runs it** (`--version`, `info`, a small fixture scan) plus a Linux musl build smoke, coverage floor raised stepwise (68 → 70 → 75, never lowered), SBOM **[SHOULD]**. Run CI on HEAD and record the run ID in `docs/VALIDATION.md`.
- **Gate-efficacy test:** on a scratch branch reintroduce each of H-05, H-07, H-08 (and H-17 once fixed); the gate must go **red**. Record in PROGRESS.
- **Accept:** G0–G9 exist and run from `scripts/gate.sh`; efficacy test recorded; the Windows job's executed binary is the one used by the release step (§7).

#### H-31 — Status labels overstated or inconsistent `MED` `MUST` `PROCESS`
- **Reviews:** v1:R-15 (A-04, A-06, A-14 marked DONE/FIXED while defects reproduce; absent `validity`, `tail_trimmed_bytes`, `aligned`, `nested_in`, `original_name`, `mtime`, `unreadable_regions[]`; `tool_info.commit` always "unknown"), v2:N-04 (+ A-10, D-01, D-03; A-03 `DONE` in `PROGRESS` vs `MITIGATED` in `REVIEW_RESPONSE`; C-09 `PARTIAL` vs `OPEN`), v5:F-12 (A-13: exit 4 documented, not implemented).
- **Fix:** re-label A-03, A-04, A-06, A-13, A-14 **PARTIAL** now; A-10, D-01, D-03 PARTIAL until H-33/H-34/H-04 close; reconcile the two tables; add `scripts/check_status.py` (CI): an item may be `FIXED`/`DONE` only if every linked `H-xx` is `CLOSED` with a commit and an existing test, and `PROGRESS.md` and `REVIEW_RESPONSE.md` agree. Start from Appendix C.
- **Accept:** the script passes and fails on a deliberately wrong label.

#### H-32 — Evidence cited in the documents is not in the tree; no CI run covers HEAD `MED` `MUST` `PROCESS`
- **Reviews:** v1:R-14, v2:N-08 (`.gitignore` excludes `bench/lockbox/`, `bench/results/{fuzz,coverage,full-0.2.0,lockbox-v1}/`, `bench/results/*.log`; `bench/lockbox.log` absent), v3:R-09 and v4:F-15 (CI evidence predates B-04/B-06).
- **Fix:** **do not regenerate or alter lockbox results.** Commit `bench/results/EVIDENCE.md` — a SHA-256 manifest of every artefact `docs/VALIDATION.md` cites (digests are already quoted there); prepare the artefact bundle + checksum for the user to publish (release asset or data repo — the agent cannot publish); add `bench/lockbox.log` reconstructed from `VALIDATION.md` and marked "reconstructed, not independently observed". Anything not backed by an artefact is labelled "author-reported" in the docs.
- **Accept:** manifest in tree; `VALIDATION.md` cites it; no unbacked headline number without the label.

#### H-33 — Documentation drift `MED` `MUST` `DOCS` **ONE each — verify by `grep` first**
- **Reviews:** v1:R-14 — `README.md` ≈L369 says CarveX is "significantly faster in the unallocated configuration" while the repo's own lockbox table shows unallocated **3.524 s vs raw 2.173 s** (dev table 0.275 s vs 0.329 s over 3 repetitions); ≈L413 "318/318 files `complete`, successfully opened and verified with Pillow" (Pillow accepts 3-bad-CRC PNGs and spliced JPEGs; `Image.verify()` does not decode pixels). v2:N-10 — README flag table lacks `--max-memory` and `--max-truncated-size`; limitation 4 says PNG is "checked for chunk CRCs" although a mismatch only lowers the score; README tree omits `tests/fragment_recovery.rs` and `fuzz/`; `/home/kali` paths remain in committed ground truth/logs. v5:F-17 — console/manifest omit the validation level (fixed by H-16).
- **Fix:** replace the speed sentence with a generated table (G9) or delete it; reword the Pillow claim to "structurally parsed; not verified (see `validity`)"; generate the flag table from `--help`; correct limitation 4 after H-17; `DECISIONS.md` caveat for `/home/kali` (do not edit frozen results). Numbers only from generated results (§0.6).
- **Accept:** `bench/render_docs.py --check` passes; each claim above either corrected or recorded as `NOT-REPRODUCED` with the grep output.

#### H-34 — A-10 limits: long functions, `missing_docs`, pedantic `LOW` `SHOULD` `QUALITY` **ONE (heuristic)**
- **Reviews:** v2:N-05 (brace-matching count at `706a12a`; ±few lines): `cli::scan` ≈270, `png::analyze` ≈246, `jpeg::analyze` ≈239, `scan::run_scan` ≈221, `ntfs::parse` ≈198, `fat32::parse` ≈179, `pdf::analyze` ≈134, `scan::postfilter` ≈124 (`carve_one` 116, `recover_fragments` 113 close); no `#![warn(missing_docs)]`; pedantic not applied (the repo says so). v1/v3/v5 call A-10 "mostly/confirmed" — **verify with a tool before acting**: set `too-many-lines-threshold = 120` in `clippy.toml` and run `cargo clippy -- -W clippy::too_many_lines`.
- **Fix (after Phase 2, to keep diffs readable):** pure moves proven by golden/G7; `#![warn(missing_docs)]` on the public API; pedantic in staged groups with `#[allow]` only where a comment proves the cast safe.
- **Accept:** no non-test function > 120 lines, or a `DECISIONS.md` entry per exception.

### 6.6 Phase 5 — backlog (not fixed by this harness)

#### H-35 — Benchmark science gaps (guide C-01…C-09)
Reviews: v1:R-10, v2:R-10, v3:R-10, v4:F-12. Lockbox-v1 is a real improvement (600 unique files, 20 images, frozen protocol, tag before lockbox) but: competitors scan the whole image while `unallocated` scans free space (CarveX raw's 100 FP are exactly the 100 kept files — a search-space effect); no ablation ladder; no block-level `byte_recall`/`splice_rate`/FP taxonomy; no McNemar/bootstrap, CIs per table; D2–D7 realistic/fragmented/overwritten data absent; CFReDS RM#2 not re-run at 0.2.x; performance is small warm-cache only; the repo's own validators are **not** independent ground truth (H-17–H-19 prove it) — use `qpdf`, pixel/SSIM comparison; artefacts not in tree (H-32). Claim only "perfect on a synthetic benchmark of intact deleted files", never "superior". Start when the user asks.

#### H-36 — Open scope (guide A/B items)
Reviews: v1:R-11, v2:R-11, v3:R-11/R-12, v4:F-13, v5 matrix. B-01 metadata/hybrid, B-03 exFAT + FAT12/16, B-09 format variants, B-10 custom signatures/ZIP, B-11 I/O reuse; B-02 EBR chain / 4Kn / `--sector-size`; A-08 `BlockDevice` (a block device reports size 0 via `metadata().len()`), output-on-source-device guard; A-09 NTFS `$ATTRIBUTE_LIST`, multi-extent `$MFT`, `$MFTMirr`, FAT #2 / backup BPB; A-13 typed errors, JSON events, carve-phase ETA; A-15 `--overwrite`, free-space check; B-05 numeric `--align <N>`, `aligned` field (default is `none`; flipping it is STOP-AND-ASK); B-07 `--salvage` (partly H-21); B-08 resume/dry-run/verify/check/extract/completions. Tracked in `docs/guide.md`.

---

## 7. Phased plan

Order: **Phase 0 baseline + instrumentation → 1 hostile-input safety → 2 honest labels → 3 evidence sources, operations, B-06 → 4 gates, labels, evidence, docs → 5 backlog.** Safety precedes labels because a crash or OOM destroys a run no matter how it is labelled; labels precede B-06 because stitching inherits validator strength. Commits: Conventional Commits with the finding ID (`fix(png): bound decode inflation [H-05]`); one finding per commit with its tests and PROGRESS update.

### Phase 0 — Baseline and safety net (no intended behaviour change)
| Task | Content | Done when |
|---|---|---|
| P0.1 | Confirm snapshot (`706a12a`, clean tree); build, test, fmt, clippy, doc, MSRV (§4.1); record numbers (reviewers saw 73 tests) | numbers in PROGRESS |
| P0.2 | H-01: `harness.md` at root, `docs/guide.md` (sha check), DECISIONS entry, `gate.sh` header, commit convention | integrity checks pass |
| P0.3 | H-02: no-root fixtures, SKIP semantics, toolchain pin, `check-msrv.sh` | gate passes on a clean checkout |
| P0.4 | **G7 now:** `scripts/golden.py` (output-hash sets, §4.2) | D0: `unallocated` 100/100, 0 FP; `raw` 100 TP + 20 kept |
| P0.5 | H-03: builders, Rust regression stubs (`#[ignore = "H-xx xfail"]`), Python `X01…X24` + missing S-scenarios | all scenarios present |
| P0.6 | **Reproduce every finding** on the source build (and `dist/` where a scenario is allocator- or timing-sensitive); fill the `Repro` column; save `docs/baseline/harness_repro_v0.2.0.txt`; `scripts/repro_xfail.txt` = currently failing scenarios | every H-xx is `REPRODUCED`, `STATIC-ONLY` or `NOT-REPRODUCED` |
| P0.7 | **G8 baseline:** `scripts/perf_gate.py`, `bench/perf/baseline-0.2.0.json` (S9-256K/1M/4M, S10, S11, X04, random 64 MiB; median of ≥ 3; `--threads 1` and default; glibc **and** musl builds — settles C5), `thresholds.toml` committed before enabling | file committed |
| P0.8 | Settle conflicts: C1 (JPEG splice property test, H-18 step 0), C2 (X04 + both controls), C5 (X23), C13 (grep README/clippy for H-33/H-34) | measurements in PROGRESS → Conflict measurements |
| P0.9 | H-04 quick wins: repository URL, `build.rs` commit stamp, version `0.3.0-dev` (+ `Cargo.lock`), `dist/README.md` stale notice | `--version` shows `0.3.0-dev` + commit |

**Mismatch rule.** A [RT] scenario that does not reproduce: record `NOT-REPRODUCED` with command and output, keep static evidence, continue (§3.5). If **all** RT×3+ findings fail to reproduce, the checkout is probably not `706a12a`: re-check §4.1 and stop to ask.
**Gate:** `scripts/gate.sh` green through xfail; G7 defined; perf baseline committed.

### Phase 1 — Hostile-input safety
Order: **H-05 → H-06** (local; introduces `--max-validator-memory`) **→ H-07 → H-08 → H-09 → H-10 → H-11 → H-12 → H-13 → H-14 → H-15 (only if P0.7/P0.8 left it open).** Each step keeps G7 identical.
**Gate at the end:** G0–G7 green; G8 thresholds met (S9-256K < 1 s, S9-4M ≤ 3 s, S10 ≤ 3 s and ≤ 256 MiB, S11 ≤ 1 s, X04 ≤ 1 s, random 64 MiB not > 10 % slower); X01–X09 and X23 green and removed from the xfail file; 60 s fuzz smoke clean on every new target; CHANGELOG *Fixed/Changed/Added* complete; README flag table lists the new options.

### Phase 2 — Honest labels
Order: **H-16 → H-17 → H-18 → H-19 (stage 1, then stage 2) → H-20 → H-21.** (The model comes first because every later item emits its states.)
**Gate at the end:** G0–G7 green (outputs and scores unchanged); X10–X14 and X24 green; schema + conformance test updated; JPEG splice property recorded with CI; `docs/REPORT.md` has the mapping table; `REVIEW_RESPONSE.md` A-04 corrected; `scripts/repro_xfail.txt` lists only Phase 3+ scenarios.

### Phase 3 — Evidence sources, operations, fragment recovery
Order: **H-22 (step 1) → H-23 → H-24 → H-25 → H-26 → H-27 → H-28 → H-29.** H-22 ships with the user-approved default `zero-fill` (§3.6); report the measured X16 behaviour in the end-of-phase report.
**Gate at the end:** G0–G8 green; X15–X22 green; B-06 measurements in PROGRESS with CI; B-04 limitations in README match behaviour.

### Phase 4 — Gates, labels, evidence, documentation
Order: **H-30 → H-31 → H-32 → H-33 → H-34.** H-30 wires G7/G8 (created in Phase 0) into CI and adds G9, fuzz smoke, the packaged-binary Windows job, coverage ratchet and the gate-efficacy test.
**Gate before the release step:** G0–G9 green on CI at HEAD (run ID recorded); `scripts/check_status.py` passes; `docs/REVIEW_RESPONSE.md` has one row per `H-xx`.

**Release 0.3.0 (user-approved, §3.6) — last task of Phase 4:**
1. Set `version = "0.3.0"` in `Cargo.toml` and `Cargo.lock`.
2. `CHANGELOG.md`: move `[Unreleased]` to `[0.3.0] — <date>`; *Changed* must list the `--max-memory` semantics (ADR-H2), the `--on-read-error` default `zero-fill` (ADR-H6), the decode-mode label changes and the new `validity`/`checks` fields (ADR-H1); plus *Fixed* and *Added*.
3. Commit `chore(release): 0.3.0`; create the annotated tag `v0.3.0` **locally — do not push**.
4. Build the Linux musl static and the Windows GNU binaries from that tag (CI or a pinned container) and **execute each**: `--version` must show `0.3.0` and the tag's commit; run `info` and a small fixture scan. The Windows run happens on a Windows runner (H-30).
5. Replace `dist/carvex-linux-x86_64` and `dist/carvex-windows-x86_64.exe`, regenerate `dist/SHA256SUMS`, rewrite `dist/README.md` (H-04 c).
6. Re-run X23 on the musl build (H-15) and the G8 gate on the release binaries.
7. Report to the user: sha256 of both binaries, build environment, whether the Linux binary reproduces bit-for-bit (v5 could not reproduce it on their toolchain — record the result, do not claim it).
8. If step 4 cannot be executed in your environment (e.g. no Windows runner): leave the old binary of that platform in place with the stale notice and report `SKIPPED (reason)` — **never commit a binary you did not execute.**

### Phase 5 — Backlog (only on the user's request)
H-35, H-36 (and the remaining guide items). Never tune on a lockbox; a lockbox run needs the user (§3.5-4).

### Definition of Done for this harness
- [ ] Every `H-01…H-34` is `CLOSED`, or `NOT-REPRODUCED` with recorded evidence, or `WONTFIX` / `BLOCKED` with a user-approved reason.
- [ ] `scripts/repro_xfail.txt` is empty; X01–X24 pass from a clean checkout without root.
- [ ] No image/container-driven allocation without a cap + `try_reserve_exact`; no abort and no OOM-kill on the X03/X07/X08 inputs; `--max-memory` bounds working memory and never discards a finished run.
- [ ] Only `verified` is ever presented as complete/verified; default-mode output says "content not checked".
- [ ] G0–G9 enforced in CI on HEAD; packaged Windows binary executed; fuzz targets cover every parser.
- [ ] README, CHANGELOG, `docs/REPORT.md`, schema and `docs/REVIEW_RESPONSE.md` consistent with behaviour; numbers generated; no `/home/kali` in new artefacts.
- [ ] `0.3.0` released as in §7 (tag local and not pushed; binaries rebuilt, executed and checksummed — or `SKIPPED (reason)`); `--on-read-error` default is `zero-fill` with the overlap guard and `unreadable_regions[]`.

---

## 8. Decisions made by this harness (seed for `docs/DECISIONS.md`)

| ADR | Decision | Needs the user? |
|---|---|---|
| ADR-H1 | `validity` (+ `checks[]`) is new, additive and authoritative; legacy `status`/`validation`/`source` keep their meaning, types and values; mapping in §6.3. Label changes occur in `--validate decode` only; default-mode console/manifest gain the validity text | No (additive; non-default mode) |
| ADR-H2 | `--max-memory` bounds **concurrent working memory**; outcomes hold ranges; bytes re-read at write; per-candidate skip is a static rule; limit events ⇒ report + exit 3 | No (bug fix; removes whole-run loss). Mention in CHANGELOG *Changed* |
| ADR-H3 | Compatibility: only add; `--validate` stays `structure` / `decode` (= guide's `deep`); `carve`/`bgc` stay | No |
| ADR-H4 | Determinism: budgets that can change results are work units; wall-clock stops are flagged `nondeterministic_stop` | No (guide §3.1) |
| ADR-H5 | B-06: work-unit budget; wall-clock only a flagged safety net; limit that really truncates the stage ⇒ warning + exit 3; stays opt-in until measured | No (feature is `[Unreleased]`) |
| ADR-H6 | `--on-read-error` has values `abort`, `zero-fill`, `skip`; default **`zero-fill`** (`abort` restores the old behaviour); every region in `unreadable_regions[]`, exit 3; outputs intersecting a zero-filled region are never `verified` | **Confirmed by the user, 2026-11-21** |
| ADR-H7 | Defaults of new limits (`--max-candidates`, `--max-work-per-candidate`, `--max-validator-memory`, EWF/VMDK caps) are chosen by measurement and must not trigger on D0, CFReDS RM#2, S9/S10 | No; record numbers |
| ADR-H8 | PDF validity in two stages; real-world PDFs with offset errors become `suspect` by design (written, not dropped) | No |
| ADR-H9 | HEAD is `0.3.0-dev` from P0.9; at the end of Phase 4 the version becomes `0.3.0`, `dist/` binaries are **replaced** (never deleted) by binaries rebuilt from the local tag and executed before commit; the tag is not pushed | **Confirmed by the user, 2026-11-21** |
| ADR-H10 | Fixtures are built without root; G7 compares **output-hash sets** with ground truth, not image bytes | No |

---

## 9. Test strategy

| Layer | Tooling | Content added by this harness |
|---|---|---|
| Unit | `cargo test` | PNG CRC/expected-size/take-limit; PDF single-pass counters; EOL variants; EWF last-chunk length; descriptor path resolver |
| Regression | `tests/regress_*.rs` + `scripts/review_repro.py` (X01…X24, S1…S15) | one red→green test per finding, `// H-xx` header; recipes in Appendix A |
| Property | `proptest` | cut-anywhere never `verified` and output ≤ cut + 1 cluster; JPEG splice rate (≥ 1,000 independent pairs); structured-mutation never-panic for parsers **and** `src/image/`; regions invariants |
| Adversarial | fixtures in `tests/support/` | PNG bomb, SOF bomb, `%%EOF` flood, header flood, hostile E01/VMDK/VHD, descriptor escapes |
| Integration | `assert_cmd` | exit codes 0/1/2/3/4; warnings codes; SIGINT; report schema |
| Golden (G7) | `scripts/golden.py` | output-hash sets on no-root D0 fixtures |
| Performance (G8) | `scripts/perf_gate.py` | S9/S10/S11/X04/random, `--threads 1` vs default, glibc + musl |
| Fuzz (G5) | `cargo-fuzz` | 7 existing + `fuzz_e01`, `fuzz_vhd`, `fuzz_vmdk`, `fuzz_split_names`, `fuzz_open_image`, `fuzz_fragment_join` |
| Oracles | `qpdf --check`, `pypdf`, Pillow `load()` **and** pixel comparison, `ewfverify`, `qemu-img` | never CarveX's own validators; a missing tool ⇒ `SKIPPED` naming the affected scenario |

Timing tests use **ratios** (relative to a same-run baseline) or generous multiples; absolute thresholds live only in G8 and are measured on the CI runner class.

---

## 10. Agent workflow

### 10.1 Loop per finding
```text
1. Read the finding; run its Appendix-A scenario; record REPRODUCED / STATIC-ONLY / NOT-REPRODUCED in PROGRESS (command + observed value vs the reviewers' "before")
2. Write the failing test (correct behaviour) — red, `#[ignore = "H-xx xfail"]` + xfail entry
3. Minimal fix; no refactoring outside the finding
4. Remove the ignore/xfail entry; run scripts/gate.sh (fmt, clippy, test, doc, repro, golden, perf, docs-check)
5. Update docs + CHANGELOG (Fixed / Changed / Added), README flag table, schema if fields changed
6. Commit: `fix(scope): … [H-xx]`
7. Update docs/PROGRESS.md and docs/REVIEW_RESPONSE.md (per-task report, §4.4)
```

### 10.2 When to stop and ask
Only the cases in §3.5. Otherwise decide conservatively, write the decision to `docs/DECISIONS.md`, continue.

### 10.3 End-of-phase report (to the user)
1. Findings closed (IDs + commits); 2. gate table G0–G9 (green / red / SKIPPED + log); 3. before → after numbers **generated by scripts** with CIs where applicable; 4. conflict measurements settled (C1, C2, C5…); 5. new findings `N-xx` and residual risks; 6. decisions awaiting the user; 7. proposal for the next phase.

---

## Appendix A — Reproduction scenarios (X01–X24) and the regression set

**Conventions.** Raw payload image = 4,096 zero bytes + payload padded to a 4 KiB boundary + 8,192 zero bytes. Run: `carvex scan IMG -o OUT --mode raw --no-fs --quiet --json-report R.json [--validate structure|decode]`; disk fixtures: `carvex scan IMG -o OUT`. Peak RSS = `ru_maxrss` from `os.wait4` (or `VmHWM`). "Before" = values the reviewers observed at/near `706a12a` (reviewer in brackets); "After" = acceptance. Fixtures needing an absent tool are `SKIPPED`. Corpus files: `bench/corpus/`.

| ID | Finding | Build | Before (reviewer) | After |
|---|---|---|---|---|
| **X01** | H-08 | 100 distinct 1024×1024 8-bit grey PNGs (random pixels, zlib level 1, ≈ 1 MiB each) at 4 KiB boundaries (≈ 100 MiB). Variants: 150 × `png_02` (v1); ≈ 1.05 GiB of copies of one 593,920 B JPEG (v2). Run with default, `--max-memory 16M`, `--max-memory 64M`, `--threads 1` | cap 16M: exit **1**, 0 files, **no report**, RSS 107 MiB [v5]; 209 MiB for the 200 MiB payload [v1]; 1.05 GiB at default 1 GiB: exit 1, RSS 1,087 MiB, 0 files, no report [v2] | all files written, report present, exit 0/3, RSS ≤ 64 MiB at 16M; 1 GiB-class at 64M ≤ 256 MiB; D0 output hashes unchanged |
| **X02** | H-05 | (a) 16×16 8-bit grey PNG, IDAT = `zlib.compress(zeros(256 MiB), 9)` (≈ 261 KB), valid CRCs, IEND; (b) 32000×32000 grey, zeros IDAT (≈ 1.06 MB). `--validate decode` and `structure` | (a) decode RSS **325 MiB** [v5]; (b) **1,063 MiB** [v1]; 0.5/1/2 GiB inflations → 547/1,061/2,088 MiB [v2]; structure ≈ 11–20 MiB | (a) ≤ 32 MiB, < 100 ms, not-verified; (b) rejected by budget before inflating; valid 800×600 PNG still `decoded` |
| **X03** | H-06 | `jpeg_01_baseline_640x480.jpg` with SOF0 height/width patched to 20000×20000 and 65535×65535; `decode` and `structure` | 7.70 s, **1,719 MiB**; **killed rc −9 at 5,948 MiB, no report** [v2]; structure 0.02 s | ≤ 64 MiB RSS, completes, `unverified`/`structural` with note |
| **X04** | H-07 | `%PDF-1.4` + one catalog object, then E segments of `A` filler each ending `\nstartxref\n0\n%%EOF\n9 0 obj\n`, 32 MiB and 64 MiB, E ∈ {1, 256, 1024, 4096}. Control 1 (v2): one PDF-headed candidate, `\n%%EOF\n` every ≈ 15 KiB to 63 MiB vs `%%EOX`. Control 2 (v4): keyword-bearing filler. `--threads 1`, default options | 0.12 / 1.36 / 4.6–5.5 / **18.7–19.2 s** [v5]; 18.05 s vs 0.14 s [v2]; 18.31–18.63 s [v1]; v4 control ≈ 2 s | 4,096 @ 64 MiB ≤ 1 s; `t(4096)/t(1) ≤ 10`; controls stay fast; A-01 outputs byte-identical |
| **X05** | H-09 | S10 flood512 (64 MiB, `FF D8 FF E0` + filler every 512 B) with `--max-candidates 1000`; X04 (E=4096) with `--max-work-per-candidate`; `--time-budget 1`; `--max-output 1M` on 10 PNGs | flags absent from `--help` [v5, v2] | each trips: coded warning + report + exit 3, no hang; `nondeterministic_stop` set for the time budget |
| **X06** | H-10 | test-only carver/validator that panics for one candidate; 5 other valid files | no containment (SRC×5); 3,840 mutants found no panic [v2, v5] | report lists `internal_error`; the 5 files carved; exit 3 |
| **X07** | H-11 | Python E01 writer: `EVF\x09\x0d\x0a\xff\x00` header, `volume` (`sectors_per_chunk = 2²⁷`, `bytes_per_sector = 512`, `sector_count = 2²⁷`), `sectors` (one tiny zlib chunk), `table` (one entry, compressed flag), `done`, valid Adler-32. `info` and `scan --input-format e01` | **SIGABRT** "memory allocation of 68719476736 bytes failed" [v5] | exit 1, typed error, < 1 s, < 64 MiB |
| **X08** | H-12 | (a) sparse VMDK 4.49 MiB, `numGTEsPerGT = 2²⁰`, capacity = 128·2²⁰·128 sectors, 128 grain-directory entries → one 4 MiB table at sector 1000 (v1 variant: `grain_size = 1`, `num_gte` = 262,144, `gd_count` = 1,024, 1 MiB file). (b) `RW 131072 FLAT "base.img"` + `RW 1099511627776 ZERO` | (a) `info` **520 MiB** [v5]; v1 variant **1,029 MiB**; (b) `scan` SIGKILL after 3.5 s [v5, OOM unconfirmed] | (a) ≤ 64 MiB, < 1 s; (b) error or completion, no kill |
| **X09** | H-13 | evidence dir with `disk.vmdk` descriptor `RW <n> FLAT "<path>" 0`, `<path>` = absolute secret, `../outside/secret.img`, `/bin/ls`, symlink escape; qemu-made monolithicSparse renamed; modified copy beside the original | outside file carved [v1]; `/bin/ls` accepted; renamed copy fails; sibling original read silently [v5] | rejected unless `--allow-external-extents`; renamed copy scans own data; sibling not read |
| **X10** | H-17 | `png_02_rgb_800x600.png` (24 chunks, 22 IDAT): XOR stored CRC of 0 / 1 / 3 / all IDAT; IHDR CRC; control: flip one IDAT payload byte; v5 fixture 128×128 grey noise, CRC ^ `0xdeadbeef`. Both validate modes | 1 bad: `complete/structure/85`, decode `decoded/85`; 3 bad: 55/55; all 22: `decoded/55`; IHDR: `decoded/85`; payload flip: `partial/65`; `validated_bytes` = whole file [v1, v2, v5] | every CRC-damage row `validity=suspect`, still emitted, `validated_bytes` < size; payload flip not verified; 0-bad control `verified` in decode |
| **X11** | H-19 | (a) Pillow PDF, one xref entry offset +7; (b) `/Root` object number replaced by `9…9` (same length); (c) 3-object PDF, all xref offsets 9, `/Root` → `/Type /Page`. Controls: unmodified; `qpdf --linearize`; `--object-streams=generate --compress-streams=n`; both; `--encrypt "" owner 256 --`; hand-built incremental (≈ 2.69 MiB each). Oracle `qpdf --check` | (a) qpdf 3 / (b) qpdf 2 / (c) qpdf 3 — CarveX `decoded/100` for all three [v1, v2]; six controls byte-identical [v1] | (a)(b)(c) not verified; controls byte-identical, `verified` iff `qpdf` exits 0 (stage 2) |
| **X12** | H-18 | (a) property test: ≥ 1,000 independent pairs, baseline + progressive, noise/gradient/photo-like, cuts at 512 and 4,096 B; (b) clean progressive 1200×800 (`jpeg_10_progressive_1200x800.jpg`); (c) progressive 640×480 spliced mid-stream and at a scan boundary; Pillow comparison | (a) **CONFLICT C1**: v1 15–98 % vs v2/v5 rejected; (b) `decoded/100` [v2, v5]; (c) `partial` | (a) top-label rate ≤ 0.1 % with Wilson CI recorded; (b) `unverified`/`structural`; (c) not verified |
| **X13** | H-20 | `pdf_03_text_2pages.pdf` with final EOL ∈ {LF, CRLF, CR, none}; `--min-size 1 --validate decode` | LF/none `decoded/100`; CRLF/CR **`partial/80`** [v2, v5] | identical validity for all four |
| **X14** | H-21 | `pdf_03` cut at 70 % + zeros; `--repair --validate decode --min-size 1 --no-fs --mode raw`; open with pypdf | `repaired / partial / 60`, 4,354 B, pypdf "startxref not found", no original hash [v5] | `repaired` iff pypdf opens / validator re-validates; `sha256_original` present |
| **X15** | H-29 | 64 MiB FAT32 (`mkfs.vfat -F 32 -S 512 -s 8`), 3 deleted files (`jpeg_01` 194,042 B, `png_02` 1,398,788 B, `pdf_05_large_scan` 623,620 B) each split at half length on a 4 KiB boundary with a 3-cluster gap (zeros; second set patterned), from cluster 200, 60 empty clusters between; bait: head of A + 2 junk clusters + tail of B; ordinary `mixed_fat32`-style image. `--fragment-recovery` | **0/3** stitched, 5.1 s, `FRAGMENT_TIME_LIMIT`, exit 0 [v5]; ordinary image: 20 identical files, 5 s consumed, warning [v4]; bait: no stitch | bait none; ordinary: no warning, ≤ +10 % time, identical output; realistic: recall > 0 (value recorded); `--threads 1` = default output |
| **X16** | H-22 | 64 MiB FAT32 + 17 × 512 B appended (131,089 sectors) → `ewfacquire -u -t e01a -c none -f encase5 -S 0 -b 64 -d sha256` and `-c fast -f encase6`; `scan --mode raw`; (b) flip one byte inside a chunk; (c) hand-built partial last chunk (compressed/uncompressed) | exit **1** "cannot decompress EWF chunk at byte 14496418", no report; (b) "checksum mismatch in EWF chunk", exit 1 [v5] | (a) exit ≤ 3, tail readable; (b) default `zero-fill`: `UNREADABLE_REGION`, files outside recovered, overlapping outputs not `verified`; `--on-read-error abort`: partial report, exit 1 |
| **X17** | H-23 | qemu-made monolithicSparse VMDK; zero the last grain-directory entry | `scan` exit 1 "VMDK grain index out of range", no report [v5] | output identical to the raw conversion |
| **X18** | H-24 | raw FAT32 with `conectix` at the start of the last sector; another with `# Extent description` at byte 20,000 | exit 1 "VHD footer checksum mismatch" / "VMDK descriptor has no extents" [v5] | scans as raw, note `INPUT_FORMAT_FALLBACK`, `input.format` recorded |
| **X19** | H-26 | 98 MiB MBR disk: 1 MiB gap with a PNG, FAT32 (20 deleted + 10 kept), 1 MiB gap with a JPEG, NTFS (`mkntfs -F -Q -s 512 -c 4096`, 32 MiB) with MFT record 0 corrupted per S8 | exit **0**, no warning, 1 volume listed, NTFS range `unsupported, scanned=true` [v5] | `VOLUME_PARSE_FAILED` with reason, exit 3, region `may_contain_allocated` |
| **X20** | H-27 | the X19 disk (intact NTFS) and a standalone FAT32 | Σ scanned regions 102,760,448 B vs `bytes_scanned` 95,367,168 B; `free` 64 MiB on a volume with 59.4 MiB free [v5] | invariants: sorted, non-overlapping, Σ scanned = `bytes_scanned`, real free bytes |
| **X21** | H-28 | X04 (E=4096, ≈ 19 s); spawn directly, SIGINT after 2 s | killed by the signal, rc −2, nothing written [v5] | exit 4, partial report `interrupted: true`, no half-written files |
| **X22** | H-25 | reports for monolithicSparse VMDK, compressed E01, stdin | `input.size` 67,108,864 vs `size_after` 14,614,528 / 14,514,325, `unchanged: true`; stdin `unchanged: true` with null mtime [v5] | `logical_size`/`physical_size`, `format`, `files[]`; stdin `unchanged: null` |
| **X23** | H-15 | S9-4M and S10, `--threads 1` vs default, on a glibc source build **and** the musl build | `dist/` musl 2 vCPU: 6.37 s vs 10.87 s; 3.36 s vs 5.23 s [v2]; source builds 0.78–1.13 s and 0.50–0.57 s [v1, v4, v5] | `--threads N` not slower than 1 thread by > 10 %, or closed NOT-REPRODUCED |
| **X24** | H-16 | interleaved fragments `[A1][B1][A2][B2]` split at half length on 4 KiB boundaries: `jpeg_01`/`jpeg_02`, `png_02`/`png_03`, `pdf_03`/`pdf_06`; default and `decode` | JPEG 394,746 B `complete/structure/100`; PDF 23,300 B `complete/structure/75`; PNG `truncated/partial/60` [v5] | default: `structural` and the console says "content not checked"; `decode`: `suspect`/`partial`; no output with a verified label whose SHA ∉ {A, B} |

**Regression set — must keep passing** (observed by v5 at `706a12a` unless noted): S1 analogue (default scans all volumes + gaps; `--partition largest --no-gaps` ⇒ exit 3 with `GAP_NOT_SCANNED`/`VOLUME_NOT_SCANNED`); S2, S2c (first `%%EOF` past 1 MiB stays whole), S3, S4 (`R3`); S5 in `decode`; S6 (531,976 B from 50.5 MB on v0.1.0); S7 partial (`R4b`: PNG without IEND salvaged); S8 (no abort; raw fallback `FS_DETECTION_FAILED`, exit 3); S8b (completes via the same fallback); S9 (4 MiB flood 0.97–1.13 s, 262,144 candidates, 78 MiB RSS); S10 (0.50–0.53 s, 41 MiB RSS); S11 (≤ 1 s); D0 golden; determinism at 1/4/8 threads; source immutability; report hashes equal the files on disk; output-safety behaviours (non-empty dir refused, symlink at the output name not followed, `-o` on a file refused); 20/20 recovery from real-tool containers (split raw, E01 plain/compressed, VMDK sparse/flat, VHD fixed/dynamic, stdin) with `--hash-image` equal to the raw image; `streamOptimized` VMDK refused explicitly.

---

## Appendix B — Cross-reference of the original IDs

| H | v1 | v2 | v3 | v4 | v5 | `guide.md` item |
|---|---|---|---|---|---|---|
| H-01 | R-12 | R-12 | R-13 | F-10 | F-18 | §0, D-03 |
| H-02 | R-09 (part) | R-09 (part) | R-09 (part) | F-09 (part) | F-11 (SKIP-as-FAIL, no-root) | A-11, A-12, §10.8 |
| H-03 | R-16 | G2 note | — | — | F-10 | A-11, §9 |
| H-04 | R-13, R-21 | R-13, N-09, N-10 | R-14 | F-11 | F-19 | A-12, A-14, D-03 |
| H-05 | R-02 | R-02 | R-03 | F-02 | F-02 | A-04, A-06 |
| H-06 | — | N-02 | — | — | — | A-04, A-06 |
| H-07 | R-06 | R-06 | R-06 | F-06 | F-03 | A-06 (8) |
| H-08 | R-01 | R-01 | R-01 | F-01 | F-01 | A-06 (6), A-15 |
| H-09 | R-01 (part) | A-06 row | R-01 (part) | — | F-04 | A-06, A-15 |
| H-10 | R-07 | R-07 | R-07 | F-07 | F-05 | A-03 |
| H-11 | R-19 | N-06 | — | (C3: refuted) | F-21 | A-03, A-06, §3.3 |
| H-12 | R-17 | (C4: superseded) | — | — | F-21 | §3.3, A-06 |
| H-13 | R-18 | — | — | — | F-23 | B-04, A-15 |
| H-14 | R-19 | N-06 | — | F-14 | F-10 | A-03 (4), G5 |
| H-15 | — | N-03 | — | — | (A/B timing) | A-06 |
| H-16 | R-08 | R-08 | R-08 | F-08 | F-08, F-17 | A-04, A-14, §8.3 |
| H-17 | R-03 | R-03 | R-02 | F-03 | F-06 | A-04 |
| H-18 | R-05 | R-05 | R-05 | F-05 | F-08 | A-04, B-09 |
| H-19 | R-04 | R-04 | R-04 | F-04 | F-07 | A-04, B-09 |
| H-20 | — | N-01 | — | — | F-09 | A-04 |
| H-21 | R-11 (B-07) | B-07 row | R-11 (B-07) | — | F-15 | B-07, S15 |
| H-22 | R-11 (A-08) | — | R-12 | — | F-20 | A-08, B-04 |
| H-23 | — | — | — | — | F-22 | B-04 |
| H-24 | — | — | — | — | F-25 | B-04 |
| H-25 | R-15 (commit part → H-04) | — | — | — | F-26 | A-14, A-15 |
| H-26 | — | — | — | — | F-13 | A-03, A-07 |
| H-27 | — | — | — | — | F-16 | A-14 |
| H-28 | — | A-13 row | R-12 (part) | — | F-14 | A-13 |
| H-29 | R-20 | N-07 | — | F-16 | F-24 | B-06 |
| H-30 | R-09 | R-09 | R-09 | F-09 | F-11 | A-11, A-12, G5–G9 |
| H-31 | R-15 | N-04 | — | — | F-12 | §4.3, §12.3 |
| H-32 | R-14 | N-08 | R-09 (CI part) | F-15 | — | C-01, C-07, A-12 |
| H-33 | R-14 | N-10 | — | — | F-17 (part) | D-01 |
| H-34 | — | N-05 | — | — | — | A-10 |
| H-35 | R-10 | R-10 | R-10 | F-12 | — | C-01…C-09 |
| H-36 | R-11 | R-11 | R-11, R-12 | F-13 | matrix | B-01…B-11, A-08, A-09, A-13, A-15 |

---

## Appendix C — Consolidated compliance matrix (38 guide items)

Consensus of the five reviews against `guide.md` (repository labels are *not* used). **MET** = acceptance satisfied in tested scope · **PARTIAL** · **NOT MET** · **OPEN** = not started.

| Item | Consensus | Harness | Dissent / note |
|---|---|---|---|
| A-01 PDF end detection | **MET** | H-07, H-20 | RT×3 (6 `qpdf` variants, 16 scans, S2/S2c/S3); v3 "mostly" (fixture gap) |
| A-02 Embedded PDF | **MET** (scoped) | — | S4 variant not run (v5) |
| A-03 No panics / fuzz | PARTIAL | H-10, H-11–H-14, H-26 | repo `DONE` vs `MITIGATED` (v2) |
| A-04 Validity / deep validation | **NOT MET** | H-16–H-21 | unanimous |
| A-05 Truncated files | **MET** (scoped) | H-03 (property test) | padding variants / property test not run (v1, v2) |
| A-06 Resource safety | PARTIAL | H-05–H-09, H-15 | flood speed fixed; v2 N-03 conditional |
| A-07 No silent volume/gap skipping | **MET** (default approved, D-23) | H-26, H-27 | v5 found two silent paths |
| A-08 Source access | PARTIAL | H-22, H-36 | no `BlockDevice`, `--sector-size`, `--on-read-error` |
| A-09 FS robustness | PARTIAL | H-36 | not re-verified by any reviewer |
| A-10 Architecture | PARTIAL | H-34 | v1/v3/v5 "mostly/confirmed"; v2 heuristic count — verify |
| A-11 Test pyramid | PARTIAL | H-03 | 73 tests; coverage 68.51 % vs 75/80/85 |
| A-12 CI / MSRV / supply chain | PARTIAL | H-02, H-04, H-30 | MSRV verified once (v5) |
| A-13 Errors / logging / progress | PARTIAL | H-28 | exit 4 not implemented; no typed errors/ETA |
| A-14 Report schema v2 | PARTIAL | H-16, H-25, H-27, H-04 | `regions[]` inaccurate; commit "unknown" |
| A-15 Output handling | PARTIAL | H-08, H-09 | no `--max-output`/`--overwrite`; write safety verified (v5) |
| B-01 Metadata recovery | OPEN | H-36 | |
| B-02 Partition tables | PARTIAL | H-36 | MBR/GPT incl. backup; no EBR chain / 4Kn |
| B-03 exFAT, FAT12/16 | OPEN | H-36 | |
| B-04 Evidence formats | **PARTIAL** | H-11–H-14, H-22–H-25 | repo MITIGATED; v4 "agree" — C10 |
| B-05 Alignment | PARTIAL | H-36 | flag present, default `none`; v4 "implemented" |
| B-06 Fragment handling | PARTIAL | H-29 | recall unproven |
| B-07 Salvage / repair | PARTIAL | H-21 | no `--salvage`; `repaired` not re-validated |
| B-08 Operations / UX | PARTIAL | H-28, H-36 | no resume/dry-run/verify/check/extract |
| B-09 Format variants | OPEN | H-36 | |
| B-10 Custom signatures / ZIP | OPEN | H-36 | |
| B-11 I/O performance | OPEN | H-36 | |
| C-01 Protocol / lockbox | PARTIAL (D1 scope) | H-32, H-35 | artefacts not in tree |
| C-02 Datasets | OPEN | H-35 | synthetic D1 only |
| C-03 Independent ground truth | PARTIAL | H-35 | repo validators not independent |
| C-04 Metrics | PARTIAL | H-35 | no block-level / FP taxonomy |
| C-05 Statistics | PARTIAL | H-35 | Wilson only; no paired tests |
| C-06 Baselines / ablation | PARTIAL | H-35 | search-space confound |
| C-07 Runner / reproducibility | PARTIAL | H-04, H-33 | `/home/kali` paths; no `render_docs.py` |
| C-08 Real data | PARTIAL | H-35 | CFReDS RM#2 from 0.1.0 |
| C-09 Performance method | PARTIAL | H-30, H-35 | repo docs say OPEN vs PARTIAL (v2) |
| D-01 Factual hygiene | PARTIAL | H-33, H-04 | v3/v5 "mostly" |
| D-02 Academic content | MET† | — | citations not re-verified by any reviewer |
| D-03 Release / process docs | PARTIAL | H-01, H-04, H-31 | placeholder URL, guide sync |

---

## Appendix D — PR checklist

- [ ] Finding ID in the commit message and in the test header (`// H-xx`); regression test red before the fix; no new `#[ignore]` except the allowed xfail form; no test loosened (exception E-1 documented).
- [ ] Every image/container-driven allocation capped **and** `try_reserve_exact`; no new `unwrap`/`expect`/unchecked index on image data (now also in `src/image/`); checked arithmetic.
- [ ] Source never opened for writing; output deterministic (run twice and with `--threads 1` vs default, compare hashes).
- [ ] Only `verified` presented as complete/verified; `structural` says "content not checked"; scores and emission unchanged unless the finding says otherwise.
- [ ] Limit events produce a coded warning + report + exit 3 (never exit 1 with nothing written); exit codes match §A-13 (0/1/2/3/4).
- [ ] Zero-filled / unreadable bytes never yield a `verified` output; `unreadable_regions[]` populated; exit 3.
- [ ] Docs, CHANGELOG, schema, README flag table updated; numbers generated.
- [ ] G0–G9 green or SKIPPED with reason; no performance regression > 10 % and G8 thresholds met.
- [ ] No change to ground truth, thresholds, frozen results or corpus; nothing tuned on a lockbox.
- [ ] Anything unverified is labelled; nothing invented.

<!-- HARNESS-EOF -->
