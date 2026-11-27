# CarveX — Remediation Harness, round 2 — revision for `f5bda4a` (`harness.md`)

> **Purpose.** Instructions for an AI coding agent (Claude Code, Codex, Cursor, …) that must fix the **verified** findings of five independent reviews of CarveX (`review-v1` … `review-v5`) and, in Part T (§7.4), may add the opt-in recovery upgrades requested on 2026-11-26.
> **Applies to:** `logosnexus/carvex` `main` at commit **`f5bda4a1b27169915fcd4dd67243c833213b0580`** (crate version `0.3.1`; 17 commits after `ab725a6`, the previous revision of this file; fetched from `logosnexus/carvex`, built from source and re-measured on 2026-11-26). The previous revision covered `ab725a6674b6322b304557df8ade52acf309bb54` (tag `v0.3.1`, `df48ab9b-carvex-main.zip`). The *original* review snapshot was `a7a5c08f36d3e5cf8546163427f6b7b23a0e1ca5` (`carvex-main.zip`, version `0.2.0`); Evidence/Where text in §7 and the "observed" column of Appendix A still describe that snapshot, and **§1.6, the per-item status lines headed `f5bda4a` and Appendix A.3 describe `f5bda4a`** (§1.5 and Appendix A.2 describe the previous revision, `ab725a6`).
> **Relation to `guide.md`:** `guide.md` (the unified guide) stays the authority for *what good looks like* (item IDs `A-xx/B-xx/C-xx/D-xx`, gates G0–G9, validity model, report schema, exit codes). This file is the authority for *what is still wrong in this snapshot and how to fix it*. Where this file and `guide.md` disagree, `guide.md` wins unless this file says so explicitly (the few deliberate deviations are listed in §3.3). Where either disagrees with the repository, **trust the repository**, log the difference in `docs/DECISIONS.md`, take the conservative option and continue.
> **Language:** English. Commands, flags, identifiers and paths are verbatim.
> **Read §0–§5 completely before changing a line of code.** Then work in the phase order of §5.

---

## 0. How to use this document

1. **Work items** are `H-01 … H-26` (§7.1–§7.3, defects) and `H-27 … H-33` (§7.4, Part T, new capability). Each carries a **Status** line (FIXED / PARTLY FIXED / OPEN / NEW): the line headed **Status on `f5bda4a`** is current; a line headed `ab725a6` is history and applies only where no newer line exists (summary in §1.5, §1.6 and §6) and then has: evidence, where, fix, red tests, acceptance (checkable by command), pitfalls. An item is done only when its acceptance passes **and** the gates of §4 are green.
2. **Loop per item** (identical to `guide.md` §12.1): *reproduce → write the failing test (red) → minimal fix → test green → `scripts/gate.sh` → docs/CHANGELOG → one small commit `fix(scope): … [H-xx]` → update `docs/PROGRESS.md`*.
3. **Priority tags:** **P0** = false labels, abort/OOM on hostile or ordinary evidence, or a new feature that fails on common real input; **P1** = robustness/operational gaps; **P2** = hygiene. **[MUST]** = required before the next release tag; **[SHOULD]** = strongly recommended.
4. **Evidence labels** (same meaning as `guide.md` §0, plus two):

| Label | Meaning in this document |
|---|---|
| **[REPRODUCED]** | The verification run executed it on the snapshot binary built from source (`cargo build --release --locked`, 80 tests green on `a7a5c08`, 135 on `ab725a6`, 148 on `f5bda4a`) on Linux x86-64, 2 cores, 8 GiB RAM, `qpdf 11.9.0`, `pypdf 6.17.0`, Pillow 12.3.0, rustc 1.97.0. Scenario specs and observed values are in Appendix A. |
| **[CONFIRMED-STATIC]** | Verified by reading the snapshot source/config. File:line references are for this snapshot and **will drift** — search by symbol if a line no longer matches. |
| **[REPORTED]** | Stated by a review, **not** re-run by the verification. Treat as a hypothesis; re-measure in Phase H0 before relying on it. |
| **[NOT RE-RUN]** | Out of reach of the verification (benchmark runs, real hardware, CI, Windows/macOS). |
| **[NOT PROTOTYPED]** | A Part T design with no experiment behind it (H-28, H-29, H-31, H-33 and everything in H-32 except the cluster pre-filter). Evidence for the other Part T items is a Python prototype on a synthetic corpus (Appendix C.4), marked "as prototype". |

5. **Numbers in this file are observations on one machine**, not thresholds. Acceptance thresholds are marked *(proposed)*; measure, then record the final value in `docs/DECISIONS.md`. Never loosen a threshold to make a test pass.
6. **Review IDs differ between reviews** (v1 `R-01…R-21`, v2 `R-01…R-13` + `N-01…N-10`, v3 `R-01…R-14`, v4 `F-01…F-16`, v5 `F-01…F-26`; the repo's own `REVIEW_RESPONSE.md` also uses `R-01/R-02`). Always use the `H-xx` IDs of this file in commits and PROGRESS; Appendix B maps every review ID to `H-xx`.
7. **Scope by status (revision for `f5bda4a`).** *FIXED* = do **not** re-implement: run the item's acceptance / A.2 / A.3 scenario once in Phase H0 to confirm, keep it as a regression guard (§8), move on. *PARTLY FIXED* = only the item's **Remaining work** list is in scope. *OPEN* = the whole item. *NEW* (Part T) = capability work, done only in Phase H5, after H1–H4 are green. The *Evidence*, *Where* and *Fix* text of §7 is the `a7a5c08` "before" picture; file:line references have drifted further (`src/scan.rs` and `src/signatures/mod.rs` were refactored).
8. **Two ID spaces.** The repository's own `harness.md` (repo root), `docs/PROGRESS.md`, `docs/DECISIONS.md` and `docs/REVIEW_RESPONSE.md` use a different numbering (`H-01 … H-34`, `R2-01 … R2-11`, `X01 … X24`). In this file `H-xx` is always **this** file's item; write the repository's as `repo H-xx`. Appendix D maps both ways. Commit messages and PROGRESS rows for work done from this file use this file's IDs plus the repo ID in brackets, e.g. `fix(report): … [H-01 / repo H-16]`.

---

## 1. Context, scope and how the findings were verified

**Project.** CarveX 0.2.0: Rust 2021 CLI + library that carves JPEG/PNG/PDF with FAT32/NTFS awareness; `ImageSource` trait with raw, split-raw, E01, VHD, VMDK and stdin back-ends (B-04); opt-in bifragment gap carving `--fragment-recovery` (B-06); validation levels `--validate structure|decode`. The repository's own `docs/REVIEW_RESPONSE.md` labels many guide items FIXED/MITIGATED.

**What was reviewed, and what changed since.** Reviews v1, v2 and v5 name commit `706a12a` ("Add opt-in bifragment gap carving for B-06"); v3 and v4 name no hash (v4 covers the B-04/B-06 round). The supplied snapshot is **`a7a5c08`, a later commit**: it already contains a `MemoryBudget` (`src/scan.rs:66-182`) and a PNG decode budget (`ValidationLimits`). Therefore every claim of every review was **re-verified against `a7a5c08`**; nothing was copied from the reviews.

**Method.** Build from source; run the existing tests (80 on `a7a5c08`, 135 on `ab725a6`, 148 on `f5bda4a`); then execute purpose-built experiments (fixtures generated by script, outputs checked against independent oracles — `qpdf --check`, `pypdf`, Pillow, SHA-256 of known source files — never against CarveX's own validators). Peak memory was measured from `/proc/<pid>/status` `VmHWM` (not `ru_maxrss`, which is contaminated by the parent). Static claims were verified by reading the code.

**Headline verdict.** Most reviewed claims reproduce unchanged; one was fixed by the snapshot (PNG decompression bomb), one is partly outdated (`--max-memory`), one was refuted (v4's remark that the E01 chunk size is bounded) and one did not reproduce (v2 N-03). In addition the verification found **new defects in the evidence-container readers** (several independently also found by review v5) and **a root-cause explanation for why B-06 recovers nothing on realistic layouts**. The strongest remaining problems are: (i) labels that say "complete/decoded/repaired" when content is damaged or unverified (H-01…H-04, H-09, H-10); (ii) abort/out-of-memory on small crafted inputs (H-05, H-06); (iii) the new E01/VMDK readers failing on ordinary real-world images (H-07, H-08); (iv) budget excess being fatal instead of "warning + partial report + exit 3" (H-12). Two further problems were found while cross-checking this file with other harness drafts and re-running the evidence: (v) truncated files are cut correctly only when the padding after them is zero bytes (H-25), and (vi) the local gate exits 1 on any machine without root-built disk images (H-26).

**Claim-level verdicts (re-verified on `a7a5c08`; the current status of each item is in §1.5)**

| Review claim | Verdict on `a7a5c08` |
|---|---|
| `--max-memory` is only a post-hoc check (v1 R-01, v2 R-01, v3 R-01, v4 F-01, v5 F-01) | **Partly outdated.** Enforcement is now in-flight (`MemoryBudget`, reserve before buffer growth and before retaining a file). **Still defective:** excess is fatal (exit 1, no report), the decode path under-reserves, and a redundant post-hoc bail remains → **H-12** |
| PNG decode validation is a decompression bomb (v1 R-02, v2 R-02, v3 R-03, v4 F-02, v5 F-02) | **FIXED on the snapshot.** [REPRODUCED] 256 MiB-of-zeros and 1 GB-declared PNGs now end as `partial` with a budget note, peak RSS 4–6 MiB. Keep as a regression guard (§8) |
| Bad PNG CRC still `complete` (v1 R-03, v2 R-03, v3 R-02, v4 F-03, v5 F-06) | **CONFIRMED** → **H-02** |
| PDF `decoded` is lexical (v1 R-04, v2 R-04, v3 R-04, v4 F-04, v5 F-07) | **CONFIRMED** → **H-03** |
| JPEG non-baseline falls through to `decoded` (v1 R-05, v2 R-05 "partly", v3 R-05, v4 F-05) | **CONFIRMED** (static) and quantified (5–15 % of splices pass decode mode) → **H-09** |
| PDF `%%EOF` handling is O(E×N) (v1 R-06, v2 R-06, v3 R-06, v4 F-06, v5 F-03) | **CONFIRMED**: 4,096 `%%EOF` in 32 MiB = 9.4 s → **H-11** |
| No per-candidate panic containment (v1 R-07, v2 R-07, v3 R-07, v4 F-07, v5 F-05) | **CONFIRMED** (static; latent) → **H-13** |
| Validity model absent / labels overstate (v1 R-08/R-15, v2 R-08/N-04, v3 R-08, v4 F-08, v5 F-08/F-12/F-17) | **CONFIRMED** → **H-01** |
| Gates G7–G9 missing, fuzz not on PR, coverage floor 60 % (v1 R-09, v2 R-09, v3 R-09, v4 F-09, v5 F-11) | **CONFIRMED** (static) → **H-19** |
| A-05 truncated files are handled (v1 "Implemented", v2 "MET (scoped)", v5 "Confirmed [M]"; v3, v4 "reported") | **PARTLY — zero padding only.** All reviews tested zero padding (v1: "random/0xFF padding not re-run"; v2: property test absent). **[REPRODUCED]** random or `0xFF` bytes after the cut are swallowed (PDF +1 MiB, PNG +12,885 B, JPEG +4 MiB of `0xFF`) or the file is lost (random continuation after a JPEG → `rejected`; 48 MiB of `0xFF` → 0 outputs, only `stats.too_large_truncated`); a truncated JPEG directly followed by a complete one is rejected → **H-25** |
| `scripts/review_repro.py --assert` is usable as a gate (v5 F-11 notes that the gate needs root) | **CONFIRMED, worse than reported — [REPRODUCED]:** without the root-built FAT32/NTFS images R7 and R8 return `SKIP` strings that the runner counts as failures: "10 scenario(s), 2 failure(s)", **exit 1**; the gate cannot go green on an unprivileged machine → **H-26** |
| Guide not in repo; build used a truncated guide (v1 R-12, v2 R-12, v3 R-13, v4 F-10, v5 F-18) | **CONFIRMED** (`docs/DECISIONS.md` D-01; `scripts/gate.sh:2` cites "guide-v2") → **H-23** |
| `Cargo.toml` repository is a placeholder (all five) | **CONFIRMED** (`Cargo.toml:10`) → **H-21** |
| v1 R-17 VMDK grain tables multiplied in memory; v5 F-21 | **CONFIRMED** (0.27 MiB file → 1,028 MiB RSS) → **H-05** |
| v1 R-18 / v5 F-23 VMDK descriptor reads arbitrary files | **CONFIRMED** (absolute and `..` paths read) → **H-15** |
| v1 R-19 / v5 F-21 E01 allocation uncapped; **v4 says chunk size is "bound-checked"** | **v1/v5 CONFIRMED, v4 REFUTED**: the field is only overflow-checked (`e01.rs:300-304`); a 1 TiB chunk size aborts the process → **H-05** |
| v1 R-20 / v2 N-07 / v4 F-16 / v5 F-24 B-06 weak | **CONFIRMED** (0/3 exact at defaults) and the **root cause was found** → **H-17** |
| v2 N-01 CRLF after `%%EOF` demoted; v5 F-09 | **CONFIRMED** → **H-10** |
| v2 N-02 JPEG SOF dimensions drive allocation | **CONFIRMED** (20000² → 1.7 GiB; 65535² → alloc abort) → **H-06** |
| v2 N-03 parallel carving slower than `--threads 1` | **NOT REPRODUCED** on 2 cores (default threads 1.9–2.0× *faster*; see §8). Re-check on a 1-vCPU runner only |
| v2 N-05 A-10 limits unmet | **CONFIRMED** (12 functions > 120 lines by brace-matching count — v2 said 11, the difference is the counting method; 2 `allow(too_many_arguments)`) → **H-22** |
| v2 N-09 / v1 R-21 stale shipped binary, unknown commit | **CONFIRMED** → **H-21** |
| v4 F-14 no fuzz targets for new parsers | **CONFIRMED** → **H-19** |
| v4 F-15 no CI run after the B-04/B-06 change | **[NOT RE-RUN]** (CI logs unavailable offline); `REVIEW_RESPONSE.md` itself says recorded runs predate B-04, B-06 and the R-01/R-02 fix |
| v5 F-13…F-16, F-20…F-26 | **CONFIRMED** except F-13's multi-volume NTFS variant (and the "silent sibling-file read" half of F-23), which are **CONFIRMED-STATIC / [REPORTED]** only → see Appendix B for the `H-xx` of each |
| Benchmark evidence is narrow (v1 R-10, v2 R-10, v3 R-10, v4 F-12) | **[NOT RE-RUN]**; handled only as wording constraints → **H-24** |

**Not verified at all (treat as open):** benchmark numbers and `bench/results`; any Windows/macOS behaviour; block devices; real libewf/VMware/Hyper-V produced containers (v5 used a libewf-produced E01; this verification used hand-built, spec-conforming fixtures); CFReDS images; fuzzing (no fuzzing was run); B-01 (metadata recovery) and B-03 (exFAT) remain unimplemented by the repository's own admission.

### 1.5 Update to `ab725a6` (v0.3.1), 2026-11-25 — read this first

**What changed.** The project moved from `a7a5c08` (0.2.0) to `ab725a6` (0.3.1, 105 commits in the clone, tag `v0.3.1`). It adopted a harness of the same family (its own `harness.md` at the repo root, IDs `repo H-01 … H-34`) and a second series (`R2-01 … R2-11`), added `docs/guide.md` verbatim, `scripts/gate.sh` (G0–G9), `scripts/check_dist.sh`, `scripts/check_status.py`, `scripts/golden.py`, `scripts/perf_gate.py` + `bench/perf/`, `fuzz/` (13 targets), the dependency `ctrlc`, `rust-toolchain.toml` (pins 1.100.0; MSRV 1.85) and new flags (`--allow-external-extents --max-output --max-regions --max-stdin-bytes --max-validator-memory --max-work-per-candidate --on-read-error --spool-dir`; `--max-decode-bytes` survives only as an alias). The last commits (`7d6bd02 … ab725a6`) touch `src/signatures/mod.rs` (a `too_many_lines` refactor), CI, the rebuilt `dist/` binaries and documentation only.

**Method (same oracles as §1).** Built `ab725a6` from source (`cargo build --release --locked` with `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu`, rustc 1.97.0, because the pinned 1.100.0 cannot be installed offline), `cargo test` → **135 passed, 0 failed** (README and PROGRESS say 134 — `regress_task_plan` is missing from the README breakdown, H-23). The Appendix-A scenarios listed in A.2 were then re-run against **both** binaries (`a7a5c08` and `ab725a6`) on the same fixtures (Appendix C) and judged against the independent oracles (`qpdf --check`, `pypdf`, SHA-256 of the source files). Results: **Appendix A.2**. `cargo clippy --lib -- -W clippy::too_many_lines` was run as well (0 findings).

**Not re-run (treat as unverified):** fuzzing (needs nightly `cargo-fuzz`; the toolchain download is blocked), coverage ratchet, `scripts/golden.py` / `scripts/perf_gate.py` (G7/G8), CI runs and the archived CI evidence, Windows `dist/` binary (`check_dist.sh`: "SKIP … wine missing"), benchmarks / `bench/results`, real libewf/VMware/Hyper-V images, the user-confirmed decisions quoted in the repo's own harness (not visible to this file).

**Result.** 26 items: **15 FIXED**, **2 FIXED (static)** — H-13 and H-19 are confirmed by reading code, CI configuration and the green test suite, not by a runtime scenario —, **7 PARTLY FIXED** (H-01, H-02, H-04, H-09, H-17, H-22, H-23), **1 OPEN** (H-25: byte-for-byte the same observations as on `a7a5c08`; the repository has no item for it), **1 NOT RE-RUN** (H-24).

| Status | Items |
|---|---|
| FIXED | H-03, H-05, H-06, H-07, H-08, H-10, H-11, H-12, H-14, H-15, H-16, H-18, H-20, H-21, H-26 |
| FIXED (static) | H-13, H-19 |
| PARTLY FIXED | H-01, H-02, H-04, H-09, H-17, H-22, H-23 |
| OPEN | H-25 |
| NOT RE-RUN | H-24 |

**New observations that are not in the `a7a5c08` register** (each is folded into the item named, no new item number):
1. **H-02** — `--validate decode` with ≥ 3 corrupted PNG IDAT CRCs gives score 35 < default `--min-score 50`; the file is **not written and no warning is emitted** (only `stats.below_min_score = 1`). On `a7a5c08` the same file was written as `complete` (the false label), in structure mode it is written as `suspect` today. Evidence is silently lost.
2. **H-04** — after the honesty fix the PDF repair is real, but the image repairs are not what they look like. **PNG:** `--repair` produces no repaired file in either validation mode (`truncated/partial`, `repaired=false`, "repair attempted, not validated"; the cut chunk's declared length swallows the appended IEND). **JPEG:** rejected in `--validate decode` (honest) but **accepted in the default structure mode** as `status=repaired`/`validity=repaired` on a structural proof only — on `jpeg_01` cut at 60 % the output is 640×480 and loads in Pillow, yet rows ≥ 271 are one flat grey (libjpeg's fill for the missing MCUs): ~43 % of the canvas is invented and the report does not say so. (An earlier revision of this file said "no repaired PNG/JPEG (0/2)"; that was wrong for JPEG in structure mode.) **Decision by the user (2026-11-25, §9 Q11): `--repair` must repair PNG and JPEG too, *with verification*** — now the scope of H-04.
3. **H-17** — the `--fragment-recovery` rewrite is *conservative*: zero-filled gaps are recovered exactly (3/3, 1.25 s, `verified`), random-filled and other-file-filled gaps are not (0/3), and the old "raised limits" mode that recovered JPEG+PNG across a random gap (2/3 on `a7a5c08`, with a false PDF stitch) no longer does. Precision was bought with recall.
4. **H-22** — the 12 long functions became 4 by raw-line count (0 by clippy's metric), but `allow(clippy::too_many_arguments)` grew from 2 to **13** (`scan.rs` 11, `fs/mod.rs` 1, `cli.rs` 1): stages were extracted by passing many parameters instead of parameter structs.
5. **H-01** — `--verbose` still prints every `[+]` line twice (and also under `--quiet`).
6. **H-05** — a 512 TiB `ZERO` VMDK extent no longer aborts and stays at 20 MiB RSS, but without `--time-budget` the scan simply runs for as long as the declared size takes (> 120 s measured); with `--time-budget 5` it ends in 5.01 s with exit 3, `TIME_LIMIT` and a report.

**Decisions the repository already took on the harness's STOP-AND-ASK list** (CONFIRMED-STATIC / `--help`; treat as answered unless the user objects — this file cannot see whether the user confirmed them): Q1 default `--validate` stays `structure`; Q2 `--on-read-error` default is `zero-fill`; Q3 CRC-bad PNGs are emitted as `suspect` (but see observation 1); Q4 `FRAGMENT_CANDIDATE_LIMIT`/`FRAGMENT_TIME_LIMIT` are in the exit-3 warning set (`src/scan.rs` ≈ 860-875); Q6 `dist/` is kept, rebuilt for 0.3.1 and checked by `scripts/check_dist.sh`; Q7 `ctrlc` was added as a dependency.

### 1.6 Update to `f5bda4a` (v0.3.1), 2026-11-26 — read this first (it supersedes the statuses of §1.5)

**What changed.** 17 commits after `ab725a6`, all dated 2026-11-25, crate version unchanged. The repository worked through the remaining items of the previous revision of this file: H-01 (`a3a5b9e`), H-02 (`af5663d`, D-69), H-04 (`3b14cc5`, D-68), H-25 (`5a8eaf5`, `b4c08a6`, `794512d`, D-70), H-17 / H-23 / H-22 (`0f3522a`, D-71 and the H-22 closure), the external H-09 reopen (`43dcf59`: progressive JPEG → `unverified`, 4,000-pair splice evaluation), then a `dist/` rebuild and CI / G9 fixes (`6f0d4ac`, `c68c012`, `d18f02e`, `9a54293`, `eeb4243`) and a CI record (`f5bda4a`).

**Method (same oracles as §1).** Built `f5bda4a` from source (`cargo build --release --locked`, `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu`, rustc 1.97.0). Re-run on that binary: `cargo test --all-features --locked` → **148 passed, 0 failed** (README says 148: it is generated now); `cargo clippy --all-targets --all-features --locked -- -D warnings` clean; `cargo clippy --lib -- -W clippy::too_many_lines` 0 findings; `scripts/review_repro.py --assert --xfail scripts/repro_xfail.txt` → 34 scenarios, 0 failures, 6 SKIP (no `mkfs.vfat`/`mkntfs`/`qemu-img`/FAT32 fixture on this machine), 0 XPASS; `scripts/check_status.py` green; the Appendix-A scenario scripts (A.3); `scripts/h17_gap_matrix.py` and `scripts/jpeg_splice_property.py --pairs 4000 --assert-max-rate 0.001` (both reproduced the repository's numbers); and the recovery experiments of Appendix C.4 (new, Part T).

**Not re-run:** fuzzing, coverage ratchet, `scripts/golden.py` / `scripts/perf_gate.py` (the repository reports them green — [REPORTED]), CI (run `45749230527` recorded green by the repository — [REPORTED]), G9 locally (`bench/render_docs.py --check` needs the repository's own `target/release`), benchmarks / `bench/results`, Windows, real libewf/VMware/Hyper-V containers.

**Result.** 26 items: **21 FIXED**, **2 FIXED (static)** (H-13, H-19), **2 PARTLY FIXED** (H-04, H-09), **1 NOT RE-RUN** (H-24).

| Status | Items |
|---|---|
| FIXED | H-01, H-02, H-03, H-05, H-06, H-07, H-08, H-10, H-11, H-12, H-14, H-15, H-16, H-17, H-18, H-20, H-21, H-22, H-23, H-25, H-26 |
| FIXED (static) | H-13, H-19 |
| PARTLY FIXED | H-04, H-09 |
| NOT RE-RUN | H-24 |

**Observations that are new on `f5bda4a`** (each is folded into the item named):
1. **H-04** — the repository answered §9 Q11 as "document PDF/PNG/JPEG *footer* repair; a cut through the payload stays `partial`" (D-68, test `mid_stream_png_and_jpeg_repair_stays_partial`). That is honest for PNG and for JPEG in `--validate decode`, but it **leaves the over-claim in place**: JPEG in the default `--validate structure` mode is still `repaired` on a structural proof only [REPRODUCED]: `jpeg_01` cut at 60 % → 116,427 B, Pillow OK 640×480, first differing row 271 of 480, rows 400–479 one colour (128,128,128) — ~43 % of the canvas is invented and the report says nothing. The new test scans with `--validate decode` and cannot see it. The repository's reading of Q11 is also narrower than the user's words ("repair PNG and JPEG too, with verification"): a truncated PNG/JPEG — the common case — still yields no viewable file → new §9 Q12.
2. **H-25** — closed, with measured residues: random/ASCII padding after the cut still adds 337–424 B to the carved file (one cluster is 4,096 B) because the end is a *structural* end, not a content hash (D-70, consistent with Q10). Side effect worth knowing: a truncated file directly followed by another file is now **emitted** as `truncated`/`partial` instead of `rejected` (S-TAIL-NEIGH, S-JPEGSPLICE: `carved` +1, `truncated` +1).
3. **H-17** — closed **by decision** (D-71 "precision over recall"). The documented supported gap classes are filler and an embedded header at the gap start. Measured recall depends on that wording: gap = repeated first cluster of another JPEG (begins with `FF D8`) → 2/3; gap = other bytes without a header → 0/3. Realistic layouts (a fragment interleaved with another file's *interior* clusters) are therefore not recovered: 3/40 in Appendix C.4 *E1-dead*, 0/30 for PNG in *X2*. That gap is the motivation of Part T (H-27 … H-33).
4. **H-02** — the dropped file is reported by `BELOW_MIN_SCORE` (count and first offsets) but the run still exits **0** (D-69 deliberately keeps it out of the exit-3 set).
5. **H-01** — `--verbose` prints each `[+]` line once; `--verbose --quiet` is now a CLI conflict (exit 2); `--verified-only` exists; `scan` has 50 flags (`--help` count, 49 on `ab725a6`).

**Decisions.** §9 Q11 was answered by the user; the repository implemented a narrower reading (above) → **Q12**. Q10 is answered by D-70 (conservative structural end). Everything else in §1.5 stands.

**New work — Part T (§7.4), H-27 … H-33.** Recovery-capability upgrades (metadata, free-space stream, oracle-guided fragment joins, coverage), requested on 2026-11-26. They are **not defects**: opt-in first, default behaviour and the frozen benchmark untouched (§9 Q5), evidence from Appendix C.4.

---

## 2. Verified strengths — do not regress them

These are the things reviewers agreed are good and the verification re-confirmed. Every item has a guard in §8.

- `#![forbid(unsafe_code)]`; `deny(unwrap_used, expect_used, indexing_slicing)` in `src/fs/` and `src/signatures/`; checked arithmetic on container header fields.
- `ImageSource` seam works: six sources (raw, split-raw, E01, VHD, VMDK, stdin) behind one trait.
- Deterministic output, offset-ordered, with an in-flight `MemoryBudget`.
- JPEG header-flood complexity (guide S9) is fixed: 4 MiB flood 2.85 s (default threads), 64 MiB `flood512` 1.49 s, random 64 MiB 0.03 s (`a7a5c08`; on `ab725a6`: 1.04 s / 0.57 s / 0.02 s).
- PNG decode is budgeted (`--max-validator-memory`, 256 MiB; `--max-decode-bytes` is kept as an alias).
- Multi-volume `auto` mode scans all volumes and gaps (user-approved default change, A-07 step 2).
- Truncated files whose padding is **zero bytes** are cut at the true end (`trimmed N trailing zero byte(s)`; guide S6: 48 MiB of zeros give exactly the cut size) and `--max-truncated-size 16M` exists. **Non-zero padding is not handled — H-25** (keep the zero-padding result byte-identical, §8).
- B-06 is opt-in and bounded by design, and its synthetic S12 fixtures recover exactly (`tests/fragment_recovery.rs`).
- **Since `f5bda4a` (guarded in §8):** `--verbose` prints each carved file once, `--verified-only` and the additive report fields (H-01); `BELOW_MIN_SCORE` for a dropped candidate (H-02); truncated ends bounded at the last verified byte, with `tail_trimmed_bytes` and `TRUNCATED_TOO_LARGE` (H-25); progressive JPEG `unverified` in decode mode and the measured splice rate 3/4,000 (H-09); the documented B-06 gap classes with limits proven on a hostile layout (H-17); `allow(too_many_arguments)` ×0 (H-22); a generated README test count (H-23).
- **Since `ab725a6` (guarded in §8):** cooperative SIGINT (exit 4 + report, H-14), `internal_errors[]` panic containment (H-13), `unreadable_regions[]` and `--on-read-error` (H-07), lazy scan planning (H-05/H-12), typed VMDK path rules (H-15), plan-backed `regions[]` (H-18), `scripts/gate.sh` G0–G9 with a `Skip`-aware `review_repro.py` (H-19, H-26), `scripts/check_dist.sh` (H-21).

---

## 3. Guardrails (violating any = PR rejected)

### 3.1. Inherited from `guide.md` §3 (apply verbatim)
Never write to the source image; deterministic output; `exact` keeps its meaning; **no anti-gaming** (no editing `bench/corpus/**`, `bench/images/*.json`, any `SHA256SUMS`, thresholds, metric definitions or baselines after seeing results; no tuning on anything but `dev`; no corpus special-casing in `src/`; no hand-typed numbers in README/report/benchmark docs); no `unwrap/expect/panic!/unchecked indexing` on image-derived data; checked `u64` arithmetic; **every allocation driven by image data has a configurable hard cap**; additive-only report/CSV changes; `#[allow]` only with a one-line justification and the item ID; never invent numbers, citations or results of runs that did not happen — record `SKIPPED (reason)`.

### 3.2. Additional rules for this round
1. **Red first.** Every item starts with a failing test that reproduces the defect on the snapshot. Keep it `#[ignore = "H-xx xfail"]` only in the Phase H0 commit that introduces it and remove the attribute in the commit that fixes it. Python repro scenarios use `scripts/repro_xfail.txt` (the file is currently empty by design; XPASS fails the gate).
2. **One allowed test rewrite.** `src/signatures/png.rs` unit test `reports_bad_crc_but_keeps_file` (≈ line 345) **asserts the defect of H-02**. It may be rewritten **only inside the H-02 commit**, must keep its "file is still emitted" assertion, must add the new validity assertion, and the commit message must say so. No other existing test may be deleted, loosened or rewritten.
3. **Independent oracles.** Tests and repro scripts judge outputs with `qpdf --check`, `pypdf`, Pillow (`verify()` + `load()`), and SHA-256 of known inputs — **never** with CarveX's own validators. When a tool is missing, record `SKIPPED` and keep a Rust-only assertion on bytes/hashes.
4. **Hostile fixtures are generated, not committed.** Builders live in `tests/common/` (Rust) and `scripts/` (Python); do not commit crafted binaries > 64 KiB. Crash-reproducers found by fuzzing go to `tests/fixtures/crashes/` (small).
5. **Allocation failure aborts the process in Rust** (`handle_alloc_error`), it is **not** a panic and `catch_unwind` cannot help. Every image-driven size must therefore be bounded **before** `Vec::with_capacity`/`vec![0; n]` (compare with a named constant or option cap first, then allocate with `Vec::try_reserve_exact` and turn failure into a typed error — `try_reserve*` returns `Err` instead of aborting).
6. **Do not change a default silently.** The items that would change a default are listed in §9 (STOP-AND-ASK). Implement the *mechanism* (flag, field) first; flip the default only after the user's answer.
7. **No refactor outside the item.** The only pure refactor is H-22 and it goes last, proven by byte-identical golden output.
8. **Status honesty.** `docs/REVIEW_RESPONSE.md`, README and PROGRESS may only say FIXED when the acceptance command of the item passes (H-23).
9. **Do not undo fixed behaviour.** Every scenario of Appendix A.2 / A.3 marked FIXED must keep its `ab725a6` value (or the newer `f5bda4a` value where A.3 has the row) after every commit, even when all unit tests stay green (§8). Changing a default that a FIXED item introduced is a §9 stop.

### 3.3. Deliberate deviations from `guide.md`
| Topic | `guide.md` | This file | Why |
|---|---|---|---|
| Rewriting an existing test (§3.2 of the guide forbids it) | forbidden | one exception, H-02 (rule 3.2.2) | the test encodes the defect |
| B-06 limit warnings and exit code | A-06/A-13: budget exceeded ⇒ exit 3 | **superseded on `ab725a6`:** the repository puts `FRAGMENT_CANDIDATE_LIMIT`/`FRAGMENT_TIME_LIMIT` in the exit-3 set (Q4 answered "yes"); original wording: exit 3 **only after** H-17 removes the budget waste on ordinary images; until then keep exit 0 and add `stats.fragment_truncated` | v4 (exit 0 is right) and v5 (exit 3) conflict; the 5 s default is consumed even on non-fragmented images (v4 F-16, reproduced) |
| `--on-read-error` default | A-08 lists the option | default `zero-fill` + `UNREADABLE_REGION` + exit 3 — **adopted on `ab725a6`** (it was gated by §9 Q2) | changes behaviour on read errors |

---

## 4. Environment, gates, progress

**Start-up (Phase H0, record in `docs/PROGRESS.md` → Environment):**
```bash
cargo --version && rustc --version           # lockfile needs >= 1.85
cargo build --release --locked
cargo test --all-features --locked           # expect 148 tests green on f5bda4a (135 on ab725a6, 80 on a7a5c08)
# rust-toolchain.toml pins 1.100.0; if it cannot be installed: RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo ...  (MSRV 1.85)
for t in qpdf pdftoppm ewfacquire ewfexport ewfverify qemu-img mkfs.vfat mkfs.exfat mkntfs mcopy ntfs-3g 7z; do
  command -v "$t" >/dev/null && echo "OK   $t" || echo "MISS $t"; done
python3 -c "import PIL, pypdf; print('PIL', PIL.__version__, 'pypdf', pypdf.__version__)"
```
- Never trust `dist/`; always test `target/release/carvex` built from source. On `f5bda4a` `dist/` was rebuilt again (commit `43dcf59`, `carvex 0.3.1`, 50 `scan` flags) and `src/`, `Cargo.toml` and `Cargo.lock` are identical between `43dcf59` and `f5bda4a` (`git diff --stat` empty); `scripts/check_dist.sh` verifies checksums, version and commit ancestry [NOT RE-RUN on `f5bda4a`]. The repository keeps its own release-identity item (repo H-04) PARTIAL because the Windows executable was only run under Wine, not on a Windows runner; H-21 here stays FIXED for the Linux binary. On `a7a5c08` the shipped binary was older than the source.
- The golden (`scripts/golden.py`) and performance (`scripts/perf_gate.py`, `bench/perf/`) gates exist and the repository reports them green on `f5bda4a` ([REPORTED]: D0 hashes unchanged; `rand64m` 1.051×): **verify** them in H0 instead of freezing new baselines; re-freeze only for an item that is allowed to change output bytes or timing (H-04 item 0 may change the bytes of a JPEG that is currently labelled `repaired`; Part T items are opt-in and must not) and record why in `docs/DECISIONS.md`.
- **Freeze the golden baseline before changing anything (G7):** with the *unmodified* snapshot binary, compute SHA-256 of every output file for the D0 images (`unallocated` and `raw`) and store them in `bench/golden/D0.sha256` in its own commit (`chore(golden): freeze D0 outputs of a7a5c08`). Later commits must keep these hashes unless the change is intended, listed in CHANGELOG and explained.
- **Record performance baselines** (G8) from the snapshot on the same machine: S9-256K/1M/4M, S10-flood512, random 64 MiB, PDF-EOF flood (E=4096/32 MiB), E01/VMDK scans of the Appendix-C fixtures, `--fragment-recovery` on the Appendix-A S-B06 fixture. Store as `bench/perf/baseline.json` (values + machine description). No regression > 10 % afterwards.

**Gates** (definitions as in `guide.md` §4.2; `SKIPPED` allowed but logged):

| Gate | Condition | Status on `f5bda4a` |
|---|---|---|
| G0 fmt, G1 clippy `-D warnings`, G2 tests, G3 MSRV, G4 deny/audit | as in guide | present in `scripts/gate.sh`; G2 = **148** tests green [REPRODUCED] (135 on `ab725a6`); G1 `cargo clippy --all-targets --all-features --locked -- -D warnings` clean and `clippy --lib -W clippy::too_many_lines` → 0 findings [REPRODUCED]; `review_repro.py` has `Skip` support and `--require-all`, so the gate needs no root (H-26 fixed) |
| G5 fuzz | each parser/FS/container driver has a target; 60 s smoke clean | 13 targets on `ab725a6` incl. `fuzz_e01`, `fuzz_vhd`, `fuzz_vmdk`, `fuzz_split_names`, `fuzz_open_image`, `fuzz_fragment_join`; CI job on push/PR + nightly [CONFIRMED-STATIC; **NOT RE-RUN** on `f5bda4a`: needs nightly `cargo-fuzz`]; every Part T parser adds a target (§7.4 rule 5) |
| G6 cross-platform | Linux + Windows CI | CI run `45749230527` recorded green by the repository (`bench/results/ci_run_45749230527_jobs.json`) [REPORTED, NOT RE-RUN]; record `PENDING-CI` for new work |
| G7 golden | D0 outputs byte-identical | `scripts/golden.py` + committed hashes; the repository reports it green on `f5bda4a` [REPORTED]; NOT RE-RUN here |
| G8 performance | no regression > 10 %; memory caps respected | `scripts/perf_gate.py` + `bench/perf/` baselines; the repository reports it green (`rand64m` 1.051×) [REPORTED]; NOT RE-RUN here; the S-FLOOD medians of A.3 were measured |
| G9 docs | README / `docs/REPORT.md` numbers equal `results.json`; flag table equals `--help`; `RUSTDOCFLAGS="-D warnings" cargo doc` | `scripts/check_status.py` → "STATUS GREEN" (38 guide IDs, 34 harness IDs) [REPRODUCED]; README test count is generated and equals `cargo test` (148 = 148, H-23); `bench/render_docs.py --check` [NOT RE-RUN locally: needs the repository's `target/release`] |

Per-task report template (paste into `docs/PROGRESS.md`):
```
Task: H-xx — <title>        Reviews: <v1 R-.., v5 F-.., …>
Change: <main files, 1–3 lines>
New tests: <names; red before / green after: yes|no>
Gate: green | green (SKIPPED: …) | PENDING-CI
Measurement: before <x> → after <y> on <fixture, machine>
Residual risk / next: <…>
```

---

## 5. Phased plan (do not start a phase before the gates of the previous one are green)

| Phase | Items | Goal |
|---|---|---|
| **H0 Baseline and safety net** | **H-26** (scripts only; no product behaviour change) | Build, test, reproduce every Appendix-A scenario against the fresh binary and compare with Appendix A.3 (the `f5bda4a` values; A.2 for rows that A.3 does not repeat; the "observed" column of Appendix A is the `a7a5c08` baseline) (**a mismatch ⇒ STOP-AND-ASK**, §9 Q9). On `f5bda4a` verify — do not re-freeze — the golden and perf baselines, and note that H-26 is already FIXED (H0 is now pure verification). `docs/guide.md` already exists; add this file as `docs/harness-f5bda4a.md` and do not overwrite the repository's root `harness.md`. Add all red tests as `xfail` (Python `scripts/repro_xfail.txt`, Rust `#[ignore = "H-xx xfail"]`) and list them in PROGRESS. Implement the fixture builders of Appendix C in `tests/common/`, commit `scripts/fn_len.py` (Appendix C.2) and keep `scripts/repair_ref.py` (Appendix C.3, the model for H-04). |
| **H1 Honest labels, hostile-input safety (P0)** | H-01 → H-02, H-03, H-04 → H-05, H-06 → H-07 → H-08 | After H1 no `verified` claim without proof, no abort from a crafted small file, E01/VMDK work on ordinary images. `scripts/repro_xfail.txt` contains no P0 scenario. |
| **H2 Engine robustness (P1)** | H-09, H-10, H-11, H-12, H-13, then H-25 | Deep validators complete, bounded work, budgets non-fatal, panic boundary. |
| **H3 Operations and B-06 (P1)** | H-14, H-15, H-17, H-18, H-19 | SIGINT/exit 4, container path rules, B-06 effective, honest coverage map, gates G5/G7/G8/G9. |
| **H4 Hygiene (P2)** | H-16, H-20, H-21, H-23, H-24, then **H-22 last** | Container report integrity, release identity, docs honesty; pure refactor last. |
| **H5 Recovery upgrades (Part T, new capability)** | H-27 → H-28 → H-29, H-30 → H-31, H-32, H-33 | Opt-in recovery gains with measured evidence (Appendix C.4); starts only after H1–H4 are green and H-04 / H-09 are closed. Default output, golden D0 and frozen benchmark artefacts stay byte-identical. |

Dependency notes: **H-26 first in H0** (without it the local gate is red on a machine without root, so no later "gate green" is attainable); **H-25 after H-01 and H-12** (it uses the `validity` vocabulary, the `tail_trimmed_bytes` field of H-01 and the warning/exit-3 mechanism of H-12). **H-01 first** (introduces `validity`; H-02/H-03/H-04/H-09/H-10 write into it). **H-13 before H-17** (B-06 attempts need the panic boundary). Build the Appendix-C container fixtures once in H0 and reuse them for H-05/H-07/H-08/H-15/H-16/H-20. H-19's golden/perf baselines are created in H0 but the *gate wiring* can land in H3.

**Revision note (`ab725a6`, history).** The phases keep their meaning but only items that are not FIXED are executed. Remaining order: **H0** re-verify Appendix A.2 (a mismatch ⇒ §9 Q9) → **H1** H-01 residual, H-02 residual (silent drop), H-04 validated PNG/JPEG repair (Q11 answered: repair both, with verification) → **H2** H-25 (needs the `tail_trimmed_bytes` field of H-01) → **H3** H-17 residual (decision; the exact recall target is a §9 question, Q4 is already answered) → **H4** H-23 residual, then H-22 residual last. Everything else: regression guards only.


**Revision note (`f5bda4a`).** Executed items are closed; what remains of H1–H4 is **H-04** (first the P0 over-claim in JPEG structure mode, then validated recovery of mid-stream cuts — §9 Q12) and the **H-09** residual (documented, accepted by the repository for 0.3.1). Then **H5** (Part T). H0 is still the first step: re-verify Appendix A.3 on the agent's build (a mismatch ⇒ §9 Q9).

---

## 6. Unified register (26 defect items + 7 recovery-upgrade items)

Verdict: **R** = reproduced on the snapshot, **S** = confirmed by static reading, **R+S** both. Reviews column lists every review ID that raised the same finding (full map in Appendix B). Guide column = item of `guide.md`.

| ID | Pri | Title | Verdict | Reviews | Guide | Status on `f5bda4a` · repo ID |
|---|---|---|---|---|---|---|
| **H-01** | P0 | Validity model; honest default labels and console/CSV vocabulary; missing A-14 fields | R+S | v1 R-08,R-15 · v2 R-08 · v3 R-08 · v4 F-08 · v5 F-08,F-17 | A-04, A-14, §8.3 | FIXED · repo H-16 |
| **H-02** | P0 | PNG CRC errors must not yield `complete`/`decoded`/`verified` | R+S | v1 R-03 · v2 R-03 · v3 R-02 · v4 F-03 · v5 F-06 | A-04 | FIXED · repo H-17 |
| **H-03** | P0 | PDF "decoded" is lexical — implement xref/object-graph verification | R+S | v1 R-04 · v2 R-04 · v3 R-04 · v4 F-04 · v5 F-07 | A-04 | FIXED · repo H-19 |
| **H-04** | P0 | `--repair` labels unreadable files `repaired` | R+S | v1 R-15 (part) · v5 F-15 | B-07, S15 | PARTLY · repo H-21 (D-68) |
| **H-05** | P0 | Container/logical-size allocations driven by header fields (E01 chunk size, VMDK alias tables, huge logical size) | R+S | v1 R-17,R-19 · v2 N-06 · v5 F-21 | A-03, §3.3 | FIXED · repo H-11, H-12 |
| **H-06** | P0 | JPEG SOF dimensions drive allocation in decode mode (OOM/abort) | R+S | v2 N-02 | A-03, A-06 | FIXED · repo H-06 |
| **H-07** | P0 | Read-error policy; E01 short final chunk and damaged chunk abort the whole scan | R+S | v5 F-20 · v3 R-12 (part) | A-08 | FIXED · repo H-22 |
| **H-08** | P0 | VMDK unallocated grain-directory entries read as errors, not zeros | R+S | v5 F-22 | B-04 | FIXED · repo H-23 |
| **H-09** | P1 | JPEG deep validation: non-baseline falls through to `decoded`; splices accepted | R+S | v1 R-05 · v2 R-05 · v3 R-05 · v4 F-05 · v5 F-17 | A-04 | PARTLY · repo H-18 (D-67) |
| **H-10** | P1 | PDF `%%EOF` followed by CRLF/CR demoted to `partial` | R+S | v2 N-01 · v5 F-09 | A-01 | FIXED · repo H-20 |
| **H-11** | P1 | PDF EOF candidate processing is O(E×N) | R+S | v1 R-06 · v2 R-06 · v3 R-06 · v4 F-06 · v5 F-03 | A-06 #8 | FIXED · repo H-07 |
| **H-12** | P1 | Budgets: excess is fatal; no work budget; time budget not enforced inside a batch | R+S | v1 R-01 · v2 R-01,N-07 · v3 R-01 · v4 F-01 · v5 F-01,F-04 | A-06 #6 | FIXED · repo H-08, H-09 |
| **H-13** | P1 | No panic boundary around candidate analysis, B-06 attempts and container readers | S | v1 R-07 · v2 R-07 · v3 R-07 · v4 F-07 · v5 F-05 | A-03 | FIXED (static) · repo H-10 |
| **H-14** | P1 | No SIGINT handling; exit code 4 and partial report on interrupt | R+S | v5 F-14 · v3 R-12 (part) | A-13 | FIXED · repo H-28 |
| **H-15** | P1 | VMDK descriptor paths: absolute/`..` accepted; renamed sparse extent fails | R+S | v1 R-18 · v5 F-23 | B-04, A-15 | FIXED · repo H-13 |
| **H-17** | P1 | B-06 recovers nothing on realistic layouts; root cause; PDF false stitch | R+S | v1 R-20 · v2 N-07 · v4 F-16 · v5 F-24 | B-06 | FIXED (by decision D-71) · repo H-29 |
| **H-18** | P1 | `regions[]` is not a coverage map; unparsable volume silently downgraded | R+S | v5 F-13,F-16 | A-07, A-14 | FIXED · repo H-26, H-27 |
| **H-19** | P1 | Gates G5/G7/G8/G9 missing; no fuzz for containers/B-06; fuzz not on PR; coverage floor | S | v1 R-09,R-16,R-19 · v2 R-09,N-06,N-08 · v3 R-09 · v4 F-09,F-14,F-15 · v5 F-10,F-11 | A-11, A-12 | FIXED (static) · repo H-14, H-30 |
| **H-25** | P1 | Truncated-file end: only zero padding is trimmed; random/`0xFF` bytes after the cut are swallowed or lose the file | R+S | — (verifier finding; all five reviews rated A-05 met on zero padding) | A-05 | FIXED · repo D-70 (no repo item) |
| **H-26** | P1 | Local gate cannot pass without root: `review_repro.py` counts `SKIP` as failure; no no-root fixtures | R+S | v5 F-11 (part) | A-12, §10.8 | FIXED · repo H-02 |
| **H-16** | P2 | Auto-detect reinterprets ordinary raw images as VMDK/VHD | R | v5 F-25 | B-04 | FIXED · repo H-24 |
| **H-20** | P2 | Container/stdin report integrity and spool limits | R+S | v5 F-26 · v2 N-06 (part) · v3 R-12 (part) | A-14, A-15 | FIXED · repo H-25 |
| **H-21** | P2 | Release identity: placeholder URL, `commit: "unknown"`, stale `dist/`, author paths | R+S | v1 R-13,R-21 · v2 R-13,N-09,N-10 · v3 R-14 · v4 F-11 · v5 F-19 | A-12, D-03 | FIXED · repo H-04 |
| **H-22** | P2 | A-10 limits: 12 functions > 120 lines, 2 `allow(too_many_arguments)` | S | v2 N-05 | A-10 | FIXED · repo H-34 |
| **H-23** | P2 | Docs honesty: guide not in repo, over-stated statuses, README claims | R+S | v1 R-12,R-14,R-15 · v2 R-12,N-04,N-08,N-10 · v3 R-13 · v4 F-10 · v5 F-12,F-18 | D-01…D-03 | FIXED · repo H-01, H-31, H-32, H-33 |
| **H-24** | P2 | Benchmark/scope claims (wording constraints only) | [NOT RE-RUN] | v1 R-10,R-11 · v2 R-10,R-11 · v3 R-10,R-11 · v4 F-12,F-13 | C-xx, B-01, B-03 | NOT RE-RUN · repo H-33 |

| **H-27** | P2 | Free-space stream: join free clusters across allocated islands (FAT32, NTFS) | S + experiment | — (user request 2026-11-26) | B-01 (extent logic), A-07 | NEW · — |
| **H-28** | P2 | FAT deleted-directory-entry recovery (name, start cluster, exact size, times) | S | — | B-01 | NEW · — |
| **H-29** | P2 | NTFS deleted MFT record recovery (runlist = exact fragments, names, times) | S | — | B-01 | NEW · — |
| **H-30** | P2 | Oracle-guided bifragment join (PNG chunk CRC-32) beyond filler/header gaps | experiment | — | B-06 | NEW · — |
| **H-31** | P3 | PDF xref-guided fragment reassembly | design only | — | B-06 | NEW · — |
| **H-32** | P3 | JPEG fragment join: cluster pre-filter, break point from the decoder, seam continuity, RST order | experiment (X1 only) | — | B-06 | NEW · — |
| **H-33** | P3 | Coverage: exFAT / FAT12/16 and ZIP/OOXML (B-03, B-10) | S | — | B-03, B-10 | NEW · — |

(The table lists H-16 after H-19 because phase order differs from numeric order; numbering is stable. The Verdict column is the `a7a5c08` verdict; the last column is the status on `f5bda4a` and the repository's own ID for the same work, Appendix D. H-27 … H-33 are Part T, §7.4; their Verdict column says how the need was established.)

---
## 7. Work items

Every item: **Evidence → Where → Fix → Red tests → Acceptance → Pitfalls**. `file:line` references are for commit `a7a5c08`.

### 7.1. Priority P0 — Phase H1

#### H-01 Validity model, honest labels, missing report fields [P0, MUST]
- **Status on `f5bda4a`: FIXED** (repo H-16, commit `a3a5b9e`; A.3 *S-CONTROL*, [REPRODUCED]). All four points of the previous remaining work are closed: (1) `--verbose` prints each `[+]` line once (3 carved files → 3 lines) and `--verbose --quiet` is a clap conflict (exit 2); (2) file objects carry `aligned`, `nested_in`, `sha256_output`, `tail_trimmed_bytes`, `validity_reason`, and `tool_info` is `{commit:"f5bda4a1b271", name, rustc:"rustc 1.97.0 (2d8144b78 2026-07-07)", version:"0.3.1"}`; (3) `--verified-only` exists (a default run writes 0 files and counts `stats.skipped_verified_only=3`; with `--validate decode` it writes the 3 `verified` files); `--complete-only` is unchanged; (4) the benchmark scripts' `verified_rate` column was [NOT RE-RUN]. `scan` now has 50 flags (49 on `ab725a6`). *Do not re-implement; the guard is in §8.*
- Previous status on `ab725a6` (history, superseded by the line above): **PARTLY FIXED** (repo H-16; A.2 *S-CONTROL*, [REPRODUCED]). Done: `validity ∈ {verified, structural, suspect, partial, repaired}` in JSON and CSV (CSV `validity` is followed by `sha256_original`, `alternatives`); default rows read "structural (content not checked)"; console table `VERIFIED STRUCTURAL SUSPECT PARTIAL REPAIRED BYTES` plus a legend; per-file `checks[]` of `{name, ok, note}` (it **replaces** the integer `checks` of `a7a5c08`, so the `validation_checks[]` name proposed below is obsolete); top-level `unreadable_regions`, `internal_errors`, `interrupted`, `nondeterministic_stop`; `sha256_original` on repaired files; `tool_info.commit` is the real commit.
- Remaining work as of `ab725a6` (history — closed on `f5bda4a`; do not redo):
  1. `--verbose` still prints every `[+]` line **twice**, and also under `--quiet` [REPRODUCED]: 3 carved files → 6 lines. Red test: each carved file exactly once, nothing under `--quiet`.
  2. File-object fields still absent: `validity_reason`, `tail_trimmed_bytes` (**H-25 needs it**), `aligned`, `nested_in`, `sha256_output`; top-level `tool_info.rustc`. Additive only; update `docs/report.schema.json` and the schema test.
  3. `--verified-only` does not exist (`--complete-only` is unchanged and must stay documented as "footer found").
  4. [NOT RE-RUN] the benchmark scripts' `verified_rate` column (never redefine existing metrics — §9 Q5).
- *The Evidence/Where/Fix text below is the `a7a5c08` picture; nothing remains to implement; keep the acceptance command as a regression guard (§8).*
- **Evidence.**
  - [REPRODUCED] The per-file report object has **no `validity`**. Keys observed: `allocated, carved_size, cluster, extent_id, fragments, height, lba, notes, offset, partition_index, path, repaired, score, seq, sha256, size, source, status, type, validated_bytes, validation, width`. Missing relative to `guide.md` A-14/§8.4: `validity`, `tail_trimmed_bytes` (today only a note: "trimmed 69381 trailing zero byte(s)"), `aligned`, `nested_in`, the per-check array (`checks[]` in the guide; **in this repo `checks` is already an integer** — the number of deep-validation checks of a B-06 stitch, `docs/report.schema.json:265` — so the array must get another name), `original_name`/`mtime` (B-01 not implemented; null is fine), `sha256_original`/`sha256_output` for repaired files. Top level lacks `unreadable_regions`; `tool_info` is `{commit:"unknown", name, version}` (no `rustc`).
  - [REPRODUCED] Interleaved-fragment JPEG (S12-style `[A1][B1][A2][B2]`, one 394,746-byte output made of the head of one file and the tail of another): default mode → `status=complete validation=structure score=100`; `--validate decode` → `validation=partial score=80` ("baseline scan does not match the frame"). The console summary table has a `COMPLETE` column counting it, with no validation level anywhere on the console.
  - [REPRODUCED] `--verbose` prints the same `[+] JPEG offset=… ` line **twice** for one carved file.
  - [CONFIRMED-STATIC] `Validation::Unverified` (`src/signatures/mod.rs:172`) is never constructed. PDF `decoded` is lexical (H-03) and JPEG `decoded` can mean "the strict scanner did not look" (H-09), so **today `decoded` does not mean verified**.
- **Where.** `src/signatures/mod.rs` (`Validation`, `validate_*`), `src/scan.rs` (report assembly ≈ 775-840, file rows, summary), `src/cli.rs` (summary table, verbose line), `docs/report.schema.json`, CSV writer.
- **Fix.**
  1. Add `enum Validity { Verified, Structural, Suspect, Partial, Repaired }` (guide §8.3) and derive it in **one** pure function `derive_validity(&FileFacts) -> Validity`, table-tested. Validators must report **how** they validated (e.g. `Proof::{None, Lexical, Structural, Decoded, Full}`); only `Full` may yield `Verified`.
  2. Mapping (legacy `status` is kept and documented as "footer found"):

     | legacy `status` | validator outcome | validity |
     |---|---|---|
     | complete | `Full` proof, no integrity flag | `verified` |
     | complete | `Decoded` proof that is not `Full` (today: every PDF, JPEG when the strict scanner returned `None`) | `structural` |
     | complete | `structure` only (default `--validate`) | `structural` — written as **"content not checked"** |
     | complete | validator failed, CRC error, dimension/frame mismatch, splice signal | `suspect` |
     | truncated | any | `partial` |
     | repaired | only if the repaired bytes re-validate (H-04) | `repaired`, else `partial` |

     Until H-03 and H-09 land, PDF `decoded` and JPEG-`None` outcomes **must** map to `structural` so that no file is `verified` on a lexical check.
  3. Report: add `validity` (and `validity_reason` short code), `tail_trimmed_bytes`, `aligned`, `nested_in` (seq or null), `validation_checks[]` (`{name, ok}`; **do not change the type of the existing integer `checks`**, record the naming deviation from the guide in `DECISIONS.md`), `sha256_original`/`sha256_output` (repaired only); top-level `unreadable_regions: []`, `tool_info.rustc`. **Additive only**; update `docs/report.schema.json` and the schema-validation test.
  4. CSV: append a `validity` column **at the end** (never reorder).
  5. Console: summary table columns `VERIFIED STRUCTURAL SUSPECT PARTIAL REPAIRED BYTES` plus a one-line legend; the word "complete" must not appear as a quality claim. `--verbose` lines print `validity`. Remove the duplicate `[+]` line: with `--verbose` it is printed once by the pre-write loop at `src/scan.rs:728-731` (even under `--quiet`) and again by the record loop at `src/scan.rs:1889-1891` — keep one, honour `--quiet`, add a test.
  6. New flag `--verified-only` (additive). Leave `--complete-only` unchanged and document it as "footer found".
  7. Benchmark scripts: add a `verified_rate` column; **do not redefine** existing metrics after seeing results (anti-gaming) — a metric-definition change is a §9 Q5 stop.
- **Red tests.** (a) table test `validity_mapping` covering every row above; (b) interleaved-JPEG fixture: default ⇒ `validity=structural`, never `verified`; decode ⇒ `suspect`; (c) schema test: every file has `validity`, CSV last column is `validity`; (d) console snapshot test: no "COMPLETE" column, legend present; (e) verbose output contains each `[+]` line exactly once.
- **Acceptance.** `jq -e '.files|all(has("validity"))'` on D0 `raw` and `unallocated` reports; no code path yields `verified` unless the validator returned `Full`; D0 golden **output files** byte-identical (only report/console changed); `report.schema.json` validates sample reports.
- **Pitfalls.** Do not rename or drop `status`; do not compute validity in the renderer; do not touch output file naming.

#### H-02 PNG CRC errors must not be `complete`/`decoded`/`verified` [P0, MUST]
- **Status on `f5bda4a`: FIXED** (repo H-17, commit `af5663d`, D-69; A.3 *S-PNGCRC3*, [REPRODUCED]). Three corrupted IDAT CRCs in `--validate decode` (fixture `pngcrc3.img`): the file is still not written (score 35 < `--min-score 50`, unchanged by design) but the run now says so: `warnings=['BELOW_MIN_SCORE']` (count and first offsets), `stats.below_min_score=1`, exit **0** — D-69 deliberately keeps this code out of the exit-3 set. Structure mode writes the same bytes as `suspect`. *Do not re-implement; the guard is in §8.*
- Previous status on `ab725a6` (history, superseded by the line above): **PARTLY FIXED** (repo H-17; A.2 *S-PNGCRC*, [REPRODUCED]). Done: a bad chunk CRC gives `validation=partial`, `validity=suspect` (scores 85 / 55 in structure mode, 65 in decode mode) with a `chunk_crc` check; the clean control is `verified` in decode mode. On `a7a5c08` every variant was `complete` with no validity.
- Remaining work as of `ab725a6` (history — closed on `f5bda4a`; do not redo):
  1. **Silent loss.** `--validate decode` with ≥ 3 corrupted IDAT CRCs scores 35 < the default `--min-score 50`: the file is **not written and no warning is emitted** — the only trace is `stats.below_min_score = 1` (structure mode writes the same bytes as `suspect`). Red test `png_three_bad_crc_decode_is_not_silent` (fixture `pngcrc3.img`, Appendix C): the candidate must either be written as `suspect` or be reported by a warning code (e.g. `BELOW_MIN_SCORE` with count and first offsets). Implement the warning mechanism now; whether `suspect` files are exempt from the default `--min-score` is §9 Q3.
  2. Keep the one allowed test rewrite (§3.2.2) closed: the unit test that encoded the defect must already be rewritten; do not touch other tests.
- *The Evidence/Where/Fix text below is the `a7a5c08` picture; nothing remains to implement; keep the acceptance command as a regression guard (§8).*
- **Evidence.** [REPRODUCED] `png_02_rgb_800x600.png` (22 IDAT chunks, 1,398,788 B). One flipped CRC in IHDR, in one IDAT, or in IEND ⇒ `status=complete`, `validation=structure score=85` (default) and `validation=decoded score=85 validated_bytes=1398788` (decode mode). Three bad CRCs ⇒ score 55, still `complete`/`decoded`. All 22 IDAT CRCs bad ⇒ `complete/decoded` score 55. [CONFIRMED-STATIC] `src/signatures/png.rs:24,150` count `bad_crc`; `:234` `score = 100 − min(bad_crc,3)×15`; `:248` `complete: true`; `validate_png` (`src/signatures/mod.rs`, ≈ 296) ignores the count; the unit test `reports_bad_crc_but_keeps_file` (≈ 345) asserts this behaviour.
- **Fix.** Carry `crc_errors` (critical vs ancillary) from `png::analyze` into `Analysis`. **Critical chunk** (IHDR, PLTE, IDAT, IEND) CRC error ⇒ validity `suspect`; `validate_png` must return `Partial` with a note even if the zlib stream inflates; `validated_bytes` counts only bytes of CRC-valid chunks. **Ancillary** chunk CRC error ⇒ note only (libpng treats it as a warning); log the choice in `docs/DECISIONS.md`. **Keep emitting the file** (evidence is not discarded; §9 Q3). Keep the legacy score formula unchanged (compatibility); `validity` is authoritative.
- **Red tests.** Parametrised by corrupted chunk (IHDR, PLTE, first/middle/last IDAT, IEND, tEXt) × `--validate {structure, decode}`: critical ⇒ `validity=suspect`, file present; ancillary ⇒ not suspect. Property: a single-byte flip anywhere inside any IDAT payload never yields `verified`. Rewrite `reports_bad_crc_but_keeps_file` per §3.2 rule 2 (keep "file kept", add `crc_errors`/validity assertion).
- **Acceptance.** All of the above; clean corpus PNGs (D0 + dev) stay `verified` (no TP lost; G7 unchanged); B-06 never accepts a stitched PNG with a bad critical CRC.
- **Pitfalls.** Do not drop PNGs with bad CRC from the output by default (that changes users' results); do not touch `min-score` semantics.

#### H-03 PDF validation is lexical — implement real xref/object-graph verification [P0, MUST]
- **Status on `ab725a6`: FIXED** (repo H-19; [REPRODUCED], Appendix A.2). S-PDFLEX: `qpdf --check` exit 0 / 3 / 2 / 3 ↔ CarveX `verified` / `suspect` / `suspect` / `suspect` (each damaged variant names its failed checks: `xref_entries`, `xref_size`, `root_catalog`); on `a7a5c08` all four were `decoded/100`. Caveat for docs (H-23): PDF `verified` means "PDF structure checks passed; page content was not rendered" (the `content_check` note). *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.** [REPRODUCED] On `pdf_03_text_2pages.pdf` (6,211 B) three corruptions that `qpdf --check` rejects — xref offsets shifted by 7 bytes (`qpdf` rc 3), `/Root` object missing (rc 2), all xref offsets wrong (rc 3) — are all reported by CarveX as `status=complete validation=decoded score=100` with the notes "startxref=5987 (target validated)" and "xref/startxref target validated". [CONFIRMED-STATIC] `validate_pdf` (`src/signatures/mod.rs:658-…`) checks only: `ends_with("%%EOF")`/`"%%EOF\n"` (:660), the substring `/Root` anywhere (:667), and that the `startxref` target begins with `xref` or looks like `N G obj`.
- **Fix (guide A-04 PDF).** New bounded, in-tree `pdf_verify(data, limits) -> PdfCheck` (no `unwrap`/indexing; checked arithmetic): parse the classic xref table **and** xref streams; follow `/Prev` and `/XRefStm` (depth ≤ 32, cycle detection); for each in-use entry confirm `N G obj` at the offset (all entries up to a documented cap, deterministic sampling beyond); `/Size` ≥ highest object + 1; `/Root` resolves to an object containing `/Type /Catalog`; direct `/Length` matches `endstream`; flag `/Encrypt`. Outcome mapping: all checks pass ⇒ `Full` proof (`verified`); some entries bad ⇒ `Partial` ⇒ `suspect` with counts in `validation_checks[]`; xref unreadable ⇒ `structural` with note. **Do not reuse** the string "target validated" for anything weaker. Cost bounded by file size and by `ValidationLimits`. Optional: compare with the objects found by a linear scan (`N G obj` pattern) to catch shifted xref.
- **Red tests.** The three corruptions above ⇒ `validity != verified` (oracle: `qpdf --check` rc ≠ 0; `SKIPPED` if `qpdf` is missing, keep the byte-level assertion). Controls that must stay `verified`: all corpus PDFs, incremental update (S2), linearized (S3), xref-stream/`/ObjStm` PDF (`qpdf --object-streams=generate` when available).
- **Acceptance.** No valid corpus PDF loses `verified` (G7); throughput cost of the verifier measured and recorded (G8); the verifier is covered by a fuzz target (H-19).
- **Pitfalls.** Benchmark ground-truth checkers must stay independent (C-03): never call `pdf_verify` from the benchmark's truth path. A PDF with millions of objects must stay within caps.

#### H-04 `--repair` labels unreadable files `repaired` [P0, MUST]
- **Status on `f5bda4a`: PARTLY FIXED** (repo H-21, commit `3b14cc5`, D-68; A.3 *S-REPAIR*, [REPRODUCED]). The repository read §9 Q11 as **footer repair only**: `--repair` rebuilds a missing PDF xref/trailer and appends a missing JPEG EOI / PNG IEND, and "mid-stream cuts stay partial" (`--help`, README, test `mid_stream_png_and_jpeg_repair_stays_partial`). Measured on the new build with the same files and cuts as A.2:
  - **PNG** `png_02` cut at 30/60/90 %, both `--validate` modes: `truncated`/`partial`, `repaired=false`, 419,636 / 839,272 / 1,258,909 B, Pillow "Truncated File Read". Honest; no recovery.
  - **JPEG, `--validate decode`:** `truncated`/`partial` at 30/60/90 % (58,212 / 116,425 / 174,637 B), reason "revalidated as Partial". Honest; no recovery.
  - **JPEG, default `--validate structure`: the over-claim is still there.** `status=repaired`, `validity=repaired`, `repaired=true` (58,214 / 116,427 / 174,639 B), note "repair appended EOI and revalidated"; Pillow `verify()`+`load()` OK at 640×480 — yet at 60 % the first differing row is 271 of 480 and rows 400–479 are one colour (128,128,128), i.e. ~43 % of the canvas is invented and the report does not say so. The repository's new test scans with `--repair --validate decode`, so it cannot see this; [CONFIRMED-STATIC] `src/signatures/repair.rs` is unchanged (decode mode accepts `Decoded | Unverified`; structure mode accepts any JPEG/PNG result).
  - **PDF** cut at 70 %: `repaired`/`decoded`, `sha256_original` and `sha256_output` both present — done.
  - **Net:** PDF repair is real; footer-only PNG/JPEG repair is documented; recovery of a *truncated* PNG/JPEG — the common case — is **not implemented**, and a viewable-but-invented JPEG is still labelled `repaired` in the default mode. Hence "partly", and the new §9 Q12.
- Previous status on `ab725a6` (history, superseded by the line above): **PARTLY FIXED** (repo H-21; A.2 *S-REPAIR*, [REPRODUCED]). Done: a repaired **PDF** re-validates (`status=repaired`, `validity=repaired`, `sha256_original` present; `qpdf --check` exit 3 = recoverable, `pypdf` reads 2 pages — on `a7a5c08`: qpdf exit 2, pypdf "startxref not found").
  - **PNG** cut at 30/60/90 % (`png_02`): no repair in either `--validate` mode (0/3): `status=truncated`, `validity=partial`, `repaired=false`, notes "repair attempted, not validated" and "repair reason: repaired file parsed only 786609/839284 byte(s)" (60 %). Root cause: the cut IDAT chunk still declares its original length, so the appended `IEND` is read as IDAT payload — the result is never a complete file. The honest outcome, but no recovery.
  - **JPEG** cut at 30/60/90 % (`jpeg_01`): in `--validate decode` rejected (`partial`, "repair reason: repaired file revalidated as Partial", 0/3). In the **default structure mode** it is accepted: `status=repaired`, `validity=repaired`, `repaired=true`, Pillow `verify()`+`load()` OK at 640×480 — on a **structural** proof only. [REPRODUCED] at 60 %: first differing row 271, and the bottom 80 rows are a single colour (128,128,128): libjpeg fills the missing MCUs with grey, so ~43 % of the image is invented and nothing in the report says how much was recovered. Pillow alone cannot catch this; the original image's rows can.
  - [CONFIRMED-STATIC] `src/signatures/repair.rs:74-84`: in decode mode a result of `Validation::Unverified` is also accepted as a successful repair (meant for intact progressive files) — for a *repaired* truncated file this is not a proof either.
- **Decision (user, 2026-11-25, §9 Q11): repair PNG and JPEG too, with verification.** The scope below replaces the earlier option "PDF only". **The repository's reading is narrower** (footer repair, D-68); the difference is the new question **§9 Q12**. Until Q12 is answered: do item 0 (needed under either reading) and keep items 1–7 as the target design.
- **Remaining work (this is the scope of the item now):**
  0. **P0, independent of Q12 — remove the structure-mode over-claim.** The repair step must run the in-tool **decode** validator on the repaired bytes in **both** `--validate` modes (repair touches only truncated candidates, so the cost is bounded) and must not accept `Unverified`: a JPEG cut mid-scan then ends `truncated`/`partial`, `repaired=false`, bytes unchanged, notes "repair attempted, not validated" + reason. Red test `jpeg_mid_stream_repair_structure_mode_stays_partial` in `tests/repair_validation.rs`: default `--validate structure`, `jpeg_01` cut at 30/60/90 %, oracle = Pillow `verify()`+`load()` **and** a first-differing-row comparison with the original, so that a grey-filled result can never pass; extend the existing mid-stream test to run in both modes. Controls that must stay `repaired`: a JPEG whose scan is complete but whose EOI is missing, a PNG whose only missing piece is IEND, the PDF rows.
  1. **Definition — a verified repair.** `validity = repaired` only if **all** hold: (a) the in-tool parser says the output is complete (`analysis.complete && end == len`); (b) the in-tool **decode** validator returns `Decoded` on the output — in **both** `--validate` modes (repair only runs on truncated candidates, so the cost is bounded) and `Unverified` is **not** accepted; (c) in tests the independent oracle passes (Pillow `verify()`+`load()`; `qpdf --check` for PDF) **and** the recovered rows equal the original's; (d) the report says what was recovered (fields below). Otherwise `partial`, `repaired=false`, bytes unchanged, notes "repair attempted, not validated" + reason.
  2. **Design: crop to the recovered rows** (default; no pixel is invented; record the choice in `docs/DECISIONS.md`). Reference implementation, tested on the corpus: **Appendix C.3 `scripts/repair_ref.py`** — port its algorithm, do not copy its structure.
     - *PNG, non-interlaced:* inflate the readable IDAT prefix (partial output is kept), keep the `n` complete scanlines (filter bytes included, each row depends only on the row above), write IHDR with height `n` (CRC recomputed), keep the ancillary chunks that precede IDAT (PLTE, tRNS, gAMA …), one new IDAT (re-deflate), IEND. All colour types and bit depths work because the unit is the scanline. Adam7-interlaced → not repairable → `partial`.
     - *JPEG, baseline sequential, interleaved single scan, no restart interval:* Huffman-decode MCU by MCU, stop at the first MCU that cannot be completed, keep the `r` complete MCU rows, patch the SOF height to `min(r·8·Vmax, Y)`, pad the last data byte with 1-bits (T.81 F.1.2.3; stuff `00` after an `FF`), append EOI. Progressive, restart intervals, non-interleaved scans → `partial` (say so in `--help`).
     - *PDF:* unchanged (xref/trailer rebuild, done).
     - *Rejected alternative:* keep the full canvas and let the decoder fill the rest (this is what the structure-mode JPEG does today: grey). If maintainers want full-size output it must be a separate explicit flag (e.g. `--repair-fill`), still labelled with `recovered_fraction < 1`, never the default and never counted as a verified repair.
  3. **Report fields** (per repaired file): `repair_validated: bool`, `recovered_rows`, `total_rows` (the header's height), `recovered_fraction`, `sha256_original` (bytes read from the image), `sha256_output`; note "repair cropped to N of M rows". The console `REPAIRED` column counts `validity=repaired` only; a repaired file never counts as `exact`/`verified`.
  4. **Tests** (`tests/regress_repair_images.rs` + the Python regress suite; builders in `tests/common/`): for ≥ 3 corpus files per format — and at least one that cannot be repaired (interlaced PNG, progressive JPEG) — cut at 30/60/90 %, plus *PNG cut inside a chunk header* (1–7 bytes after an IDAT boundary; reference: 27 rows from the first IDAT, identical to the original) and *cut followed by padding* (run through the whole scan so that the carver's tail trim applies — see the input contract in the Pitfalls). Assert: every `validity=repaired` output passes Pillow `verify()`+`load()` **and** its top `recovered_rows` rows equal the original's (JPEG: ignore the last 2 rows, chroma edge); `repaired=true ⇒ repair_validated=true`, both hashes present; non-repairable cases stay `truncated/partial`, `repaired=false`, bytes identical to the carved prefix; `--validate structure` and `decode` give the same `validity` for the same file; intact files are untouched (G7 golden byte-identical). **The reference script repairs 9/10 corpus PNGs and 8/10 corpus JPEGs at each of the three cuts** (the unrepairable ones are the interlaced PNG and the two progressive JPEGs, which fall back to `partial`); a Rust port that repairs fewer needs an explanation in `DECISIONS.md`.
  5. **Docs:** README and `--help` list exactly what `--repair` supports (PDF: xref/trailer rebuild; PNG/JPEG: crop to the recovered rows; progressive JPEG and interlaced PNG are carved as `partial`).
  6. **Acceptance.** Every file labelled `repaired` passes the independent oracle in the tests; the S-REPAIR PNG/JPEG rows become `repaired` at 30/60/90 % with Pillow OK and identical top rows (A.2); no output contains a pixel that is not in the original; G7 unchanged, G8 within 10 % (repair only runs on truncated candidates).
  7. **Pitfalls.** Do not trust Pillow alone (it accepts a grey-filled JPEG); do not accept `Unverified`; keep the repair code panic-free (no `unwrap`/indexing, checked arithmetic) and bounded — partial inflate must respect the decompression-bomb cap of H-05, JPEG MCU count and PNG `height × rowbytes` are taken from the header and must be capped before any loop; reuse the existing decode validator for the final proof if it can report the decoded rows, otherwise the Huffman walk of `repair_ref.py` is the model; a repair is a *new* file — never overwrite the carved original, keep both hashes. **Input contract:** the repair must run on the carved bytes up to the true end of the data (H-25 tail trim), never on padding that follows: a decoder reads padding as data and invents rows — [REPRODUCED] with the reference on 4 KiB of zeros left after the cut: PNG 361 instead of 359 rows, JPEG 19 instead of 17 MCU rows, and the oracle comparison with the original fails. Tests with non-zero padding depend on H-25 and stay `xfail` until it lands.
- *The Evidence/Where/Fix text below is the `a7a5c08` picture; implement only the remaining work above.*
- **Evidence.** [REPRODUCED] `--repair` on a PDF cut at 70 % (+ zeros): output 4,354 B, `status=repaired validation=partial score=60 validated_bytes=0`; `pypdf` fails ("startxref not found"), `qpdf --check` rc 2. Same pattern for a PNG cut at 60 % (839,284 B, "repair appends IEND", one CRC note) and a JPEG cut at 60 % (116,427 B, "repair appends EOI"): all labelled `repaired` with `validated_bytes=0`. [CONFIRMED-STATIC] `src/scan.rs:1130-1184` appends `analysis.repair_suffix` and sets `repaired = true` without re-validating.
- **Fix (guide B-07).** After appending the suffix, run the format validator on the **repaired bytes**. `validity = repaired` only if it passes (JPEG: decodes; PNG: IEND present, IDAT inflates completely; PDF: `pdf_verify` of H-03 passes, which needs a rebuilt xref/trailer — implement `rebuild_xref` from a linear scan of `N G obj` and `/Root`, or do not claim). Otherwise `validity = partial` with the note "repair attempted, not validated". Keep legacy `repaired` (bytes modified) and legacy `status` unchanged; add `repair_validated: bool`, `sha256_original` (bytes read from the image) and `sha256_output`. The console `REPAIRED` column counts `validity=repaired` only. A repaired file never counts as an `exact` result.
- **Red tests.** The three cut files with `--repair`: PDF ⇒ `validity != repaired` unless `qpdf --check` passes; PNG/JPEG cut mid-stream ⇒ `partial` unless Pillow `verify()`+`load()` succeeds. Both hashes present for every repaired file.
- **Acceptance.** A file labelled `repaired` always passes the independent oracle in the tests.
- **Pitfalls.** *(`a7a5c08` text.)* "Do not change what `--repair` writes unless the validator-guided rebuild is implemented; this item is about labels first." Superseded: the labels are fixed and the rebuild is now in scope (Remaining work above).

#### H-05 Allocations driven by container header fields and logical size [P0, MUST]
- **Status on `ab725a6`: FIXED** (repo H-11 and H-12 / R2-01; [REPRODUCED], Appendix A.2). S-E01-HUGE: no abort — `implausible EWF bytes per sector 1048576` (exit 1; `a7a5c08`: SIGABRT) and `EWF chunk size 4294967296 exceeds the 67108864-byte limit` (exit 1). S-VMDK-ALIAS (`info`): 0.01 s / 0 MiB for both tables (`a7a5c08`: 0.27 s / 260 MiB and 1.09 s / 1,028 MiB). S-VMDK-ZERO (512 TiB `ZERO` extent, `ulimit -v 4000000`): `a7a5c08` SIGABRT after 2.2 s (2.6 GB RSS) → no abort, 20 MiB RSS; with `--time-budget 5` exit 3 in 5.01 s with `TIME_LIMIT` and a report. The acceptance line "exit 1 in < 1 s" is therefore superseded for this one row: without `--time-budget` the scan simply runs as long as the declared size takes (> 120 s measured, bounded memory) — a design choice of the repository, not a defect of this item. *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence (three reproduced vectors).**
  1. **E01 chunk size.** `chunk_size = sectors_per_chunk × bytes_per_sector` (two untrusted `u32`) is only overflow-checked (`src/image/e01.rs:300-304`) and then used in `vec![0u8; self.chunk_size]` (`:199`); the cache keeps `CACHE_CHUNKS = 8` chunks (`:20`). [REPRODUCED] A crafted E01 declaring 2²⁰ × 2²⁰ bytes (1 TiB) with one tiny compressed chunk: `memory allocation of 1099511627776 bytes failed` → SIGABRT (exit 134). A 4 GiB declaration allocates lazily and fails with an ordinary error; a genuinely compressible 4 GiB chunk × 8 cache slots would need 32 GiB. *(v4's statement that the field is "bound-checked" is only true for overflow.)*
  2. **VMDK alias bomb.** Each non-zero grain-directory entry loads its own `num_gte × 4`-byte table (`src/image/vmdk.rs:353-380`), validated only against the file length. [REPRODUCED] A 0.27 MiB file (4,096 grain-directory entries all pointing at one 256 KiB table) ⇒ `info` peak RSS **1,028 MiB** (≈ ×3,800); memory grows linearly with the entry count (a 4 MiB file with 1 M entries would need ≈ 256 GiB).
  3. **Huge logical size.** [REPRODUCED] A VMDK with a flat extent plus a `ZERO` extent of 2⁴⁰ sectors (512 TiB): `scan --mode raw` aborts in `scan::build_tasks` — `memory allocation of 5368709120 bytes failed` (run with `ulimit -v 4000000`; without a limit it is an OOM-kill, as v5 observed). The task list is materialised eagerly for any source size.
- **Fix.**
  - `MAX_EWF_CHUNK_BYTES` (proposed 16 MiB; EWF default is 32 KiB) — reject larger values with an error; derive the cache size from the chunk size so `CACHE_CHUNKS × chunk_size ≤ 128 MiB` *(proposed)*.
  - VMDK: load grain tables **lazily** with a small LRU (≤ 64 MiB total, proposed) or de-duplicate tables by sector offset; refuse directories whose declared entries × table size exceed a documented multiple of the file length.
  - `build_tasks`: generate tasks lazily per extent/chunk range (iterator), or reject sources whose planning memory would exceed a budget with a clear error (exit 1) **before** allocating. Planning memory must be O(extents), not O(size).
  - Funnel every image-driven allocation through one helper (`alloc_limit(bytes, what) -> Result<…>`) consulting a parser cap or the `MemoryBudget`. Never rely on catching allocation failure.
- **Red tests.** The three fixtures (Appendix C). The test spawns the binary and asserts `status.code() == Some(1)` (a SIGABRT yields `None`), wall < 1 s, and peak VmHWM < 64 MiB (Python repro measures VmHWM; Rust test asserts exit status and wall time). Control: a normal 64-sector-chunk E01 and a normal sparse VMDK still scan.
- **Acceptance.** Each crafted input ends with an error (exit 1), not an abort, in < 1 s and < 64 MiB *(proposed)*; fuzz targets `fuzz_e01`, `fuzz_vmdk`, `fuzz_vhd` exist (H-19).
- **Pitfalls.** No `unsafe`; no `catch_unwind` as a substitute (allocation failure is not a panic).

#### H-06 JPEG SOF dimensions drive allocation in decode mode [P0, MUST]
- **Status on `ab725a6`: FIXED** (repo H-06; [REPRODUCED], Appendix A.2). S-JPEGSOF: 20,000² `a7a5c08` 4.25 s / 1,723 MiB, 65,535² SIGABRT → 0.02 s / 0 MiB for both, `validation=unverified`, `validity=structural` (decode skipped by the frame budget). *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.** [REPRODUCED] `--validate decode` on a structurally valid baseline JPEG whose SOF is edited to 20000×20000: 3.2 s wall, **1.7 GiB peak RSS**, result `partial`; edited to 65535×65535: allocation failure ⇒ SIGABRT, no report (measured under `RLIMIT_AS` 3.5 GiB; on a larger host the request is multi-GiB). Default `--validate structure` is unaffected. [CONFIRMED-STATIC] `validate_jpeg` (`src/signatures/mod.rs:254`) calls `jpeg_decoder::Decoder::decode()` with no dimension check; `carve_one` reserves only `bytes.len()` in the budget guard (`src/scan.rs:1144-1148`). The PNG path already has the right pattern (`ValidationLimits`, budget note → `partial`).
- **Fix.** Before decoding, compute `needed = width × height × components × bytes_per_sample` from the SOF (`analysis.width/height` + component count; checked arithmetic). If `needed > limits.max_decoded_bytes` (reuse `--max-decode-bytes`, default 256 MiB) ⇒ skip decode, return `Partial` with a note "JPEG decoded size N exceeds the decode-validation budget", validity `structural`/`suspect` per H-01. Otherwise **reserve `needed` from the `MemoryBudget`** for the duration of the decode so parallel decodes cannot exceed `--max-memory` (this also closes the under-reservation noted in H-12).
- **Red tests.** SOF edits 20000² and 65535² ⇒ report present, rc 0, peak RSS < 300 MiB, note present. Control: a valid 8000×6000 JPEG (≈ 144 MB decoded) still decodes under the default budget.
- **Acceptance.** As above, plus a memory-accounting test with 8 parallel large decodes under a small `--max-memory` (no abort; warning per H-12).

#### H-07 Read-error policy; E01 short final chunk and damaged chunk abort the whole scan [P0, MUST]
- **Status on `ab725a6`: FIXED** (repo H-22; [REPRODUCED], Appendix A.2). S-E01-SHORT (compressed and raw): exit 1, no report → exit 0 with the JPEG carved. S-E01-ADLER: chunk 1 → exit 3, `UNREADABLE_REGION`, the JPEG lying in the damaged chunk is skipped (0 files); chunk 7 → exit 3, `UNREADABLE_REGION`, JPEG carved (`a7a5c08`: exit 1, no report). `--on-read-error` default is `zero-fill` (§9 Q2). *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.** [REPRODUCED on spec-conforming hand-built fixtures; v5 additionally reproduced with a libewf-produced, `ewfverify`-clean E01] Media of 7.5 chunks (short final chunk, compressed **or** uncompressed): `scan --mode raw` exits 1 — `cannot read scan chunk at offset 0` / `cannot decompress EWF chunk` — with **no report**. A single chunk with a wrong Adler-32 (chunk 1, or chunk 7 *after* the JPEG that lies in chunks 1–6) aborts the same way; the intact JPEG is not recovered. [CONFIRMED-STATIC] `load_chunk` (`src/image/e01.rs:185-…`) inflates with `read_exact` into a `chunk_size` buffer (`:199-205`) and treats an uncompressed chunk as `chunk_size` bytes plus a 4-byte Adler-32 (`:216`); the scan makes **any** read error fatal (`src/scan.rs:979-980`, `read_exact_at(...).with_context("cannot read scan chunk …")?`), for every source including raw files (EIO). Real disks are rarely a multiple of the chunk size (a 1,953,525,168-sector drive leaves 48 sectors), so whole-disk E01 images fail whenever a scan touches the tail.
- **Fix.**
  1. E01: last chunk length = `min(chunk_size, media_len − index × chunk_size)`; for compressed chunks read at most `expected_len + 1` bytes (`Read::take`) and compare; for uncompressed expect `expected_len + 4`.
  2. Implement guide A-08: `--on-read-error {abort|zero-fill|skip}`. An unreadable range (inflate failure, checksum mismatch, `io::Error`) is **coalesced into `unreadable_regions[{offset,len,reason}]`**, produces a coded `UNREADABLE_REGION` warning (cap the number of individual warnings; add a summary) and **exit 3**. Any carved file overlapping an unreadable region carries a `validation_checks` entry and validity ≤ `partial`.
  3. Even with `abort`, write a **partial report** (with the error) before exiting 1.
  4. Default value: recommended `zero-fill` (the forensic norm: `ewfexport`, `dd conv=noerror,sync`) — **gated by §9 Q2**. Until answered, keep `abort` as default.
- **Red tests.** Appendix-C E01 builder: good (control), short last chunk compressed, short last chunk uncompressed, bad Adler in chunk 1, bad Adler in chunk 7. Mock `ImageSource` failing at offset X ⇒ `unreadable_regions` exact. If `ewfacquire` exists, also build an E01 of an image with 17 sectors beyond a whole number of chunks and compare carve output to the raw image (SKIPPED otherwise).
- **Acceptance.** Short-last-chunk E01 scans in raw mode with exit ≤ 3 and the tail offsets readable (byte-identical carve output vs the raw image); the corrupt-chunk E01 completes with `UNREADABLE_REGION`, exit 3, and recovers the files outside the chunk.
- **Pitfalls.** `D-29` in `docs/DECISIONS.md` ("fail explicitly instead of returning partial data") is the wrong policy for an evidence reader — update it with the new decision.

#### H-08 VMDK unallocated grain-directory entries must read as zeros [P0, MUST]
- **Status on `ab725a6`: FIXED** (repo H-23; [REPRODUCED], Appendix A.2). S-VMDK-THIN: exit 1 "grain index out of range" → exit 0, JPEG carved; the full-grain control is unchanged. *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.** [REPRODUCED] Constructed monolithicSparse VMDK (2 MiB, 64 KiB grains, 4 GTEs per table ⇒ 8 grain-directory entries; entry 0 allocated and holds a JPEG; entries 1–7 = 0): `scan` exits 1 — **"VMDK grain index out of range"**. The same image with entries 1–7 pointing at all-zero tables scans correctly (JPEG recovered, exit 0). [CONFIRMED-STATIC] `src/image/vmdk.rs:352-354` stores `Vec::new()` for a zero directory entry; `:403-415` turns the missing entry into an error. Thin-provisioned disks have such entries for every unwritten region, and the existing fixtures are dense.
- **Fix.** Directory entry 0 or grain-table entry 0 ⇒ zeros (the whole table range). Keep error behaviour for entries pointing beyond the file end. `streamOptimized`/compressed grains ⇒ explicit "unsupported" error (verify the current message).
- **Red tests.** Appendix-C sparse builder: thin image (above), GTE = 0 inside an allocated table, `ZEROED_GRAIN` flag (`vmdk.rs:15,348`), table beyond EOF (must error). Compare against the known raw bytes of the fixture.
- **Acceptance.** The thin image scans with byte-identical carve output to its raw equivalent (`qemu-img convert -O raw` when available, else the fixture's own raw bytes).

### 7.2. Priority P1 — Phases H2 and H3

#### H-09 JPEG deep validation: non-baseline falls through to `decoded`; splices accepted [P1, MUST]
- **Status on `f5bda4a`: PARTLY FIXED** (repo H-18 = PARTIAL, commit `43dcf59`, D-67; A.3 *S-H09-PROG*, *S-H09-EVAL*, *S-JPEGSPLICE*, [REPRODUCED]). Both items of the previous remaining work are closed: (1) **the fall-through is fixed** — a progressive JPEG in `--validate decode` is `unverified`/`structural` with the note "JPEG strict scan unsupported for this variant; content not checked" (it was `decoded` on `a7a5c08`); (2) **the splice rate was re-measured** with the repository's evaluator `scripts/jpeg_splice_property.py --pairs 4000 --assert-max-rate 0.001`: **3 accepted of 4,000** (0.0008; Wilson 95 % [0.0003, 0.0022]; gradient 3/1,323, noise 0/1,336, photo 0/1,341), exit 0 — against 5–15 % on `a7a5c08`. The S-JPEGSPLICE pair is still caught in decode mode (`partial`/`suspect 80`), and since H-25 its truncated head is also emitted as a separate `truncated`/`partial` file.
- **Residual (this is what is left):** the point estimate meets the repository's 0.1 % target, the **Wilson upper bound (0.22 %) does not**; the repository's own stricter criterion (upper bound ≤ 0.1 %) stays PARTIAL and was accepted for 0.3.1. All 3 accepted splices are gradient images (flat content gives the seam test nothing to discontinue). Wording constraint (H-23): say "3 of 4,000 synthetic pairs accepted in decode mode (95 % upper bound 0.22 %)", not "splices are detected". Closing the residual needs a larger evaluation or a stronger seam model (H-32 uses the same continuity score for ranking). The default depth stays `structure` (§9 Q1).
- Previous status on `ab725a6` (history, superseded by the line above): **PARTLY FIXED** (repo H-18 = "PARTIAL" in the repository's own PROGRESS; R2-03 / D-67; A.2 *S-JPEGSPLICE*, [REPRODUCED] for the cases below). Done: a conservative baseline `content_continuity` check ("no extreme seam discontinuity detected") appears in `checks[]`; default depth stays `structure` and is labelled `structural` (H-01); in decode mode the S-JPEGSPLICE pair and four extra splice pairs I built (progressive+progressive, grayscale+baseline, CMYK+baseline, progressive+baseline) are all `partial/suspect 80` — identical to `a7a5c08`, because those cases were already caught. The repository itself records the residual: its evaluation upper bound is above the 0.1 % target (accepted for 0.3.1).
- Remaining work as of `ab725a6` (history — closed on `f5bda4a`; do not redo):
  1. Re-measure the share of splices that pass decode mode on the evaluation corpus (`a7a5c08`: 5–15 % [REPORTED/measured]); the corpus is not in this verification — [NOT RE-RUN]. Until then README/REPORT must not claim splices are detected beyond the measured rate (H-23).
  2. The non-baseline `None → decoded` fall-through of `src/signatures/jpeg.rs` was not re-read on `ab725a6` — confirm by reading before closing the item.
- *The Evidence/Where/Fix text below is the `a7a5c08` picture; nothing remains except the residual above.*
- **Evidence.** [CONFIRMED-STATIC] `validate_jpeg` (`src/signatures/mod.rs:254-290`) downgrades only on `Some(Err(..))` from `jpeg::strict_baseline_scan` (`src/signatures/jpeg.rs:358`, returns `Option<Result<(), String>>`); `None` ("not checked": progressive, arithmetic, 12-bit, …) falls through to `Validation::Decoded`. [REPRODUCED] Splice sweep `B[:cut] + A[cut:]`, cuts on 512-byte boundaries, two 640×480 Pillow-generated JPEGs per class, `--validate decode`: accepted as `decoded` — noise baseline **8/91 (8.8 %)**, noise progressive **4/90 (4.4 %)**, photo-like baseline **7/79 (8.9 %)**, photo-like progressive **11/76 (14.5 %)**; of those, visibly different from A (mean abs. difference > 8): 8, 4, 1, 11. In the default mode (`structure`) every splice is `complete/structure/100` (H-01). *(v2 called v1's claim "partly confirmed"; the merged measurement is the one above.)*
- **Where.** `src/signatures/mod.rs:254-290`, `src/signatures/jpeg.rs` (`strict_baseline_scan`, `analyze`).
- **Fix (guide A-04 JPEG).**
  1. Replace `Option<Result>` by an explicit `ScanCheck::{Checked(Result<..>), Unsupported(reason)}`; `Unsupported` ⇒ `structural` with the note "deep check not available for <mode>" — **never `Full`**.
  2. Deep baseline checks: RSTn sequence (mod 8) when DRI is present; DQT/DHT present before SOS; SOF component count = SOS component count; Huffman-decode every scan, count MCUs against the SOF geometry, require the decoder to consume exactly to EOI.
  3. Progressive: stage it — verify scan headers (Ss/Se/Ah/Al progression legal, every coefficient band covered, component sets consistent) plus decoder success plus EOI consumption; claim `Full` only for what is actually checked.
- **Red tests.** Property test over **≥ 1,000** deterministic splice pairs from the corpus (cluster-aligned and 512-aligned cuts, baseline + progressive): `verified` fraction ≤ 0.1 % *(proposed, from guide A-04)*; progressive files without a complete check are `structural` by construction. Controls: every valid corpus JPEG (D0 + dev) baseline stays `verified`; no valid file is rejected by the deep check.
- **Acceptance.** Property test green; cost of `--validate decode`/deep measured vs structure (guide rule: make deep the default only if no true file is rejected and cost ≤ 2× — that is a §9 Q1 question, not a decision for the agent).
- **Pitfalls.** Re-labelling progressive files `structural` changes the `verified` count of the corpus — report it, do not hide it; do not weaken the oracle by reusing the same decoder crate as the *only* check.

#### H-10 PDF `%%EOF` followed by CRLF/CR is demoted to `partial` [P1, MUST]
- **Status on `ab725a6`: FIXED** (repo H-20; [REPRODUCED], Appendix A.2). S-PDFEOL: LF 6,211 B, CRLF 6,212 B, CR 6,211 B, none 6,210 B are all `decoded`/`verified`/100 (`a7a5c08`: CRLF and CR `partial/80`). *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.** [REPRODUCED] On `pdf_03_text_2pages.pdf` (6,211 B): `%%EOF` + LF ⇒ `complete/decoded/100` (6,211 B); `%%EOF` + CRLF ⇒ 6,212 B `complete / partial / 80`, note "selected PDF revision does not end with %%EOF"; `%%EOF` + CR ⇒ 6,211 B `partial/80`; no EOL ⇒ `decoded/100`. [CONFIRMED-STATIC] `validate_pdf` (`src/signatures/mod.rs:660`): `selected.ends_with(b"%%EOF") || selected.ends_with(b"%%EOF\n")`. Windows-produced PDFs commonly end with CRLF. (`find_footers` in B-06 already treats CR/LF/CRLF after `%%EOF` as part of the file, `src/scan.rs:1295-1305` — be consistent.)
- **Fix.** One shared helper `pdf::eof_tail_ok(selected)` implementing the tolerant tail of `guide.md` Appendix B (`%%EOF` followed by ≤ N whitespace/EOL bytes, including `\r\n` and `\r`), used by `validate_pdf`, `pdf.rs` end detection and B-06.
- **Red tests.** LF / CRLF / CR / none / trailing space / `%%EOF` + junk (⇒ `partial`). Sizes unchanged semantics (the EOL bytes belong to the file).
- **Acceptance.** The four first variants reach the same validity as LF; the junk variant does not.

#### H-11 PDF EOF candidate processing is O(E×N) [P1, MUST]
- **Status on `ab725a6`: FIXED** (repo H-07; [REPRODUCED], Appendix A.2). S-PDFEOF (32 MiB, E `%%EOF` markers): E=256 1.54 → 0.18 s, E=1024 4.30 → 0.18 s, E=4096 **18.85 → 0.27 s**; RSS 76 → 44 MiB. Watch item: the single-marker control (E=1) went 0.11 → 0.68 s (absolute cost small; keep it under the G8 gate). *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.** [REPRODUCED] 32 MiB raw image, one `%PDF-` header followed by E `%%EOF` markers, default options: E=1 ⇒ 0.16 s; 256 ⇒ 0.77 s; 1,024 ⇒ 2.42 s; **4,096 ⇒ 9.39 s** (≈ 2.3 ms per `%%EOF`; `guide.md` A-06 asks ≤ 1 s for 4,096 `%%EOF` in 64 MiB). [CONFIRMED-STATIC] `src/signatures/pdf.rs:215-246` `find_eofs`: for every `%%EOF` it calls `count_objects(&data[..end])` (:231) and `memmem::find(&data[..end], b"/Root")` (:232), cap 4,096 (:241); `:252` counts objects again; each EOF also runs `find_startxref` over a 4 KiB window. Cost grows with prefix length × number of EOFs; several headers multiply it.
- **Fix.** Single pass: maintain a running object count and the first `/Root` position while scanning once (`memchr`), then answer every EOF in O(1); run the expensive structural check (H-03) only on the selected EOF(s). Stop enumerating EOFs once `max_size` is exceeded.
- **Red tests.** Python perf scenario S-PDFEOF (Appendix A) with a generous threshold, **plus** a deterministic unit test proving the scan primitive is called O(1) times per `find_eofs` call (inject a counter/closure; no timing in unit tests). No `#[ignore]` tricks.
- **Acceptance.** 4,096 `%%EOF`/32 MiB ≤ 1 s on the reference machine *(proposed)*; output identical (G7); no regression of other floods (§8).

#### H-12 Budgets: excess is fatal; no work budget; time budget not enforced inside a batch [P1, MUST]
- **Status on `ab725a6`: FIXED** (repo H-08, H-09 and H-12b / R2-01; [REPRODUCED], Appendix A.2). S-MAXMEM (100 × ≈ 1 MiB PNG): `--max-memory 16M` and `64M` → exit 0, 100/100 files, RSS 12–16 MiB, no warning (`a7a5c08`: exit 1, "retaining a carved file would exceed --max-memory", no report). S-MAXCAND (`--max-candidates 5`): exit 3, `CANDIDATE_LIMIT`, partial output + report (`a7a5c08`: exit 1, no report). The in-batch time budget cannot be exercised by the old S-TIMEBUD fixture any more (it now finishes in 0.16 s); use the 512 TiB `ZERO` extent of H-05 (`--time-budget 5` → exit 3, `TIME_LIMIT`, 5.01 s) or `tests/work_budgets.rs`. *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence (snapshot behaviour).**
  - `--max-memory` is **in-flight** (superseding the post-hoc check described by all five reviews): 100 distinct ≈ 1 MiB PNGs in a ≈ 100 MiB raw image, default ⇒ 100 files; `--max-memory 64M` ⇒ **exit 1**, "retaining a carved file would exceed --max-memory 64.0 MiB (currently using 63.1 MiB)", **no report, 0 files**; `16M` likewise. Cause [CONFIRMED-STATIC]: `carve_batch` (`src/scan.rs:1055`) collects `Result<Vec<_>>` from `par_iter`, so one budget `Err` aborts everything. A redundant post-hoc bail `total_carved > max_memory` remains at `:745-753`.
  - `--max-candidates 5` on a small image ⇒ exit 1, "found more than --max-candidates 5 header candidates", no report (`:670-677`).
  - `--time-budget` is checked only **between** batches (`:701`) and then `bail!("scan exceeded --time-budget")` (fatal). [REPRODUCED] `--time-budget 1` on a scan whose single batch took 1.24 s ⇒ exit 0 (not enforced inside the batch).
  - `--max-work-per-candidate` (guide A-06 #6) does not exist (no such symbol).
  - The decode path reserves only the compressed `bytes.len()` (`:1144-1148`); decoder working memory is not accounted (H-06).
- **Fix (guide A-06 #6).** On **any** budget excess: coded warning (`MEMORY_BUDGET_EXCEEDED`, `CANDIDATE_LIMIT`, `TIME_BUDGET_EXCEEDED`, `WORK_BUDGET_EXCEEDED`), **partial report** containing everything found so far, `stats.truncated_by`, **exit 3**. A per-candidate memory excess skips that candidate (record it with reason) instead of aborting the scan. Commit results in offset order so the output is deterministic across `--threads`; mark the report `deterministic: false` only when the wall-clock budget fired. Add `--max-work-per-candidate N` (bytes parsed/validated per candidate; default proposed 64 MiB). Check a cooperative cancel token **inside** `carve_one`, the PDF loops and B-06. Replace the redundant post-hoc bail by a warning.
- **Red tests.** The three snapshot scenarios ⇒ exit 3 + report + files found before the limit; `--threads 1/2/8` produce identical output under a candidate/memory limit; a work-budget fixture; decode-mode accounting (with H-06).
- **Acceptance.** `guide.md` A-06 acceptance; S9/S10 timings no worse than the §8 baselines; README/limitations updated with the trade-off ("a budget can drop a legitimate file adjacent to a noisy region — measure its recall impact on dev").

#### H-13 No panic boundary around candidate analysis, B-06 attempts and container readers [P1, MUST]
- **Status on `ab725a6`: FIXED (static)** (repo H-10; [CONFIRMED-STATIC]). `catch_unwind` boundaries at 9 sites in `src/image/mod.rs`, `src/fs/mod.rs`, `src/scan.rs`; `tests/panic_containment.rs` (3 tests, green); report fields `internal_errors[]`, `stats.internal_errors`; warning `CANDIDATE_PANIC` is in the exit-3 set. The repository's own `scripts/review_repro.py` scenario X06 ("per-candidate panic containment") passes on `ab725a6` [REPRODUCED]; no other panic was injected here. *Do not re-implement; keep as a regression guard.*
- **Evidence.** [CONFIRMED-STATIC] `catch_unwind` exists only in `src/fs/mod.rs:373,443` (FS detection). `carve_one`, the validators, `try_fragment_join` and the `ImageSource` back-ends are unguarded; `Cargo.toml` has no `panic = "abort"` (`[profile.release]` at `:38`, `overflow-checks = true` at `:42`), so unwinding works and a panic in a rayon worker propagates and kills the scan with no report. The `deny(unwrap/expect/indexing)` lints cover `src/fs/` and `src/signatures/` but **not** `src/scan.rs` or `src/image/`. The guide's acceptance test ("a deliberately panicking fake validator still yields a report with `internal_error`") does not exist.
- **Fix.** Wrap analysis + validation of each candidate, each B-06 attempt and container `read_at` in `catch_unwind(AssertUnwindSafe(..))`; on panic record `INTERNAL_ERROR` (offset, type, truncated message) in `warnings[]`, skip the candidate, continue, **exit 3**. Add a Cargo feature `fault-injection` (enabled only by tests via `required-features`) that triggers a panic at a chosen offset. Extend `#![deny(clippy::indexing_slicing, unwrap_used, expect_used)]` to `src/image/` (measure the violation count; fix or `#[allow]` with justification and `H-13`).
- **Red tests.** Injected panic at candidate k ⇒ report contains `INTERNAL_ERROR` for k, every other candidate's output byte-identical to a clean run, exit 3; same for a B-06 attempt and for a container read.
- **Acceptance.** As above; fuzz smoke finds no panic escaping to the process level.
- **Pitfalls.** Panics are not allocation failures (H-05/H-06 must still bound sizes). Make sure the panic message does not flood stderr in release (`--quiet`).

#### H-14 No SIGINT handling; exit code 4 and partial report [P1, MUST]
- **Status on `ab725a6`: FIXED** (repo H-28; [REPRODUCED], Appendix A.2). S-SIGINT: a 512 MiB random raw scan interrupted after 0.15 s → exit **4**, report written, `interrupted=true`; `a7a5c08`: killed by the signal (exit −2), no report. The original S-PDFEOF-flood variant now ends in 0.27 s, so use a longer scan to exercise it. *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.** [REPRODUCED] SIGINT sent after 2 s to a slow scan (PDF `%%EOF` flood, 32 MiB): the process dies from the signal (exit −2), **no report**. [CONFIRMED-STATIC] no signal handling anywhere in `src/` or `Cargo.toml`; `src/main.rs` exits only with the code returned by `cli::run` (0/1/3). `guide.md` A-13: `4` = interrupted with a partial report.
- **Fix.** Install a handler (the `ctrlc` crate is the obvious candidate — `forbid(unsafe_code)` stays; record licence, transitive-crate count and MSRV in `docs/DEPENDENCIES.md`; STOP-AND-ASK only if the guide's dependency thresholds are exceeded) that sets an `AtomicBool` cancel token checked in the batch loop, `carve_one`, B-06, PDF loops and container reads. On cancel: finish the current candidates, write the partial report atomically (temp + rename) with warning `INTERRUPTED`, **exit 4**. A second SIGINT may exit immediately.
- **Red tests.** Spawn the binary on a flood fixture, send SIGINT (unix: `kill -INT`), assert exit 4, a schema-valid report, `INTERRUPTED` warning, and that files written before the signal are listed and intact. Windows: `SKIPPED` unless the CI runner can do it.
- **Acceptance.** As above; also verify no half-written output file remains.

#### H-15 VMDK descriptor paths: absolute and `..` accepted; renamed sparse extent fails [P1, MUST]
- **Status on `ab725a6`: FIXED** (repo H-13; [REPRODUCED], Appendix A.2). S-VMDK-PATH: absolute path and `../..` path → exit 1 with a typed error (`VMDK absolute extent path … is not allowed`, `extent … escapes the descriptor directory`); `--allow-external-extents` opts back in; `a7a5c08`: both were accepted silently (exit 0, no flag to control it). S-VMDK-RENAMED: scan exit 0, `input.format=vmdk`, 1 JPEG (`a7a5c08`: exit 1, "cannot open …/original_name.vmdk"). *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.** [REPRODUCED] A descriptor with `RW 1 FLAT "/abs/path/secret.bin" 0` **and** one with `"../../secret.bin"`: both opened; a 512-byte file outside the evidence directory became the scanned image (`input.size = 512`). [CONFIRMED-STATIC] `resolve()` (`src/image/vmdk.rs:268-274`) returns absolute paths verbatim and joins relative paths without checking `..`. [REPRODUCED] A self-contained monolithicSparse VMDK (embedded descriptor naming `original_name.vmdk`) copied to `renamed.vmdk`: `info` fails "cannot open …/original_name.vmdk". [REPORTED, v5] if a *different* file with the original name sits next to the renamed copy, that file is read silently.
- **Fix.** (a) When the opened file itself has the `KDMV` magic, the extent **is the opened file**; ignore the descriptor's self-reference. (b) Descriptor-named extents must resolve **inside the descriptor's directory subtree**: reject absolute paths, drive letters, `..` components and symlinks that escape (canonicalise and prefix-check); add `--allow-external-extents` for the rare legitimate layout, with the error text naming the flag. (c) Record every resolved file (path, size) in the report (`input.files[]`, H-20).
- **Red tests.** Absolute path, `..`, symlink escape (unix), renamed sparse (must now work), sibling-name collision (must **not** read the sibling).
- **Acceptance.** As above; document the new default in CHANGELOG (**Changed**).

#### H-17 B-06 recovers nothing on realistic layouts; root cause; PDF false stitch [P1, SHOULD]
- **Status on `f5bda4a`: FIXED — by decision** (repo H-29, commit `0f3522a`, D-71 "precision over recall"; A.3 *S-B06*, [REPRODUCED]). Remaining-work item 1 is answered: precision over recall is the intended design and the **supported gap classes are documented** in `scan --help` and README — a gap of filler bytes, or a gap whose first cluster carries an embedded header. Item 2 is closed by `tests/fragment_recovery.rs::h17_fragment_limits_bind_on_a_hostile_layout` (64 distinct truncated heads and valid footers: `--max-frag-candidates 1` → `FRAGMENT_CANDIDATE_LIMIT`; a 1 ns `--frag-time-limit` → `FRAGMENT_TIME_LIMIT` with `nondeterministic_stop=true`; raised limits → neither), green among the 148. Measured: zero-filled gap **3/3** exact (SHA-256 equal, `validity=verified`, `source=bgc`, 1.27 s, no warning); random-filled gap **0/3**; gap made of another file's *interior* clusters (my fixture) **0/3**; gap = the repeated first cluster of `jpeg_04` (begins with `FF D8`; the repository's `scripts/h17_gap_matrix.py`) **2/3** (JPEG, PNG), 0 spliced stitches. The difference between "0/3" and "2/3" for "other-file" gaps is the header at the gap start, not the file. With limits raised, a random gap gives JPEG 94,260 B `partial`, PNG 698,593 B `partial`, PDF 635,908 B `suspect`; an other-file gap gives JPEG 206,330 B `suspect`, PNG 705,505 B `partial`, PDF 635,908 B `suspect`; none is `verified`.
- **What this leaves (the motivation of Part T, not a defect):** a fragment interleaved with another file's interior clusters — the common layout on a used volume — is not recovered by B-06 (Appendix C.4: *E1-dead* 3/40 = 7.5 %, *X2* 0/30). H-27, H-30 and H-32 address it. *Do not re-implement; the guard is in §8.*
- Previous status on `ab725a6` (history, superseded by the line above): **PARTLY FIXED** (repo H-29 / D-53; A.2 *S-B06*, [REPRODUCED]). Done: with `--fragment-recovery` the **zero-filled gap** is recovered exactly for JPEG, PNG and PDF (3/3 SHA-256 equal) in 1.25 s at default limits, `validity=verified`, `source=bgc`, no warnings (`a7a5c08`: 0/3, 5.1 s, `FRAGMENT_TIME_LIMIT`). The false PDF stitch is gone: on random and other-file gaps the PDF is `complete`/`suspect` (it contains the gap) or `partial`, never `verified`. `FRAGMENT_*` limit warnings are in the exit-3 set (Q4).
- Remaining work as of `ab725a6` (history — closed on `f5bda4a`; do not redo):
  1. **Random-filled and other-file-filled gaps recover nothing** (0/3; the JPEG is not even emitted for the random gap). `a7a5c08` with raised limits recovered 2/3 on these gaps (JPEG and PNG exact) and made a false PDF stitch; that recall is gone. Decide and record in `docs/DECISIONS.md` whether precision-over-recall is the intended final design (repo D-53 "measured, bounded"): if yes, document the supported gap classes in README/`--help` and in the A.2 row; if no, extend the join (ranked candidates + decode proof) until the random/other-file rows recover. Never emit a stitch as `verified` without a decode proof.
  2. `--max-frag-candidates` and `--frag-time-limit` are accepted but have no effect on the S-B06 fixtures (raising them to 100,000 / 60,000 ms changes nothing: 0.3 s). Confirm on a hostile fixture that they still bind.
- *The Evidence/Where/Fix text below is the `a7a5c08` picture; nothing remains to implement; keep the acceptance command as a regression guard (§8).*
- **Evidence.** Fixture S-B06 (Appendix A): 64 MiB FAT32, 4 KiB clusters, three corpus files (`jpeg_01_baseline_640x480.jpg` 194,042 B, `png_02_rgb_800x600.png` 1,398,788 B, `pdf_05_large_scan.pdf`) each split at a cluster-aligned midpoint with a **3-cluster gap**, all unallocated.
  - [REPRODUCED] At default limits (`--max-frag-candidates 1024`, `--frag-time-limit 5000`, `--max-gap 16`): **0 of 3 exact** in every configuration run (12: 3 gap contents {zeros, random bytes, another file's bytes} × `--validate {structure,decode}` × fragment recovery {off,on}); with recovery on the run takes ≈ 5.1 s and ends with `FRAGMENT_TIME_LIMIT` (or `FRAGMENT_CANDIDATE_LIMIT`), **exit 0**.
  - [REPRODUCED] With limits raised (`--max-frag-candidates 100000 --frag-time-limit 60000`, decode): random gap ⇒ JPEG and PNG **exact** in 3.0 s, but the **third file (623,620 B) is stitched from wrong pieces and still labelled `complete/decoded`, `source=bgc`**; gap filled with another file's bytes ⇒ same 2/3 in 12.1 s; **zero-filled gap ⇒ 0/3 after 60 s** (`FRAGMENT_TIME_LIMIT`). With the default candidate cap but a zero gap the stage costs ≈ 10.9 s for 1,024 attempts (≈ 10 ms per attempt = a full decode each).
  - [CONFIRMED-STATIC] Mechanism, `src/scan.rs:1276-1421`: **(1)** `find_footers` collects footers over the whole image and truncates *by image offset* to `min(4 × max_frag_candidates, 8192)` (`:1314-1318`), independent of where the head is. **(2)** `candidate_joins` enumerates **cut-major → gap → every footer** and executes `break 'search` as soon as `joins.len() >= max_frag_candidates` (`:1373-1410`) — **before** the "increasing-distance" sort at `:1418`. With 20 footers in the volume the cap is hit after ≈ 3 cuts, so the true cut (≈ cluster 24 for a 97 KiB half) is never enumerated; the guide's ordering (§8.6) is not honoured whenever the cap binds. **(3)** Every footer after `tail_start` is tried, not the nearest few. **(4)** Every attempt does a full read plus `validate_deep` (decode) with no cheap pre-filter. **(5)** A stitched PDF is accepted by the lexical PDF validator (H-03).
- **Fix.**
  1. **Estimate the cut instead of enumerating it.** Per format, locate the first bad cluster of the contiguous carve: PNG — first chunk whose CRC fails; JPEG — first RST-sequence break / strict-scan failure offset (extend `strict_baseline_scan` to return the failure offset); PDF — first object/xref inconsistency. Try cuts at that cluster ± 2 first; fall back to bounded enumeration only afterwards.
  2. **Best-first enumeration with a fair budget**: order by `(gap, |cut − estimate|, tail distance)` *before* applying any cap; apply the cap/time limit **per head** and split the global time budget across heads so one noisy head cannot starve the rest.
  3. **Nearest footers only**: for each `(cut, gap)` try the first K footers after `tail_start` (K small and documented; larger for PDF, which has several `%%EOF`s); never rank footers by global image order.
  4. **Cheap pre-filter** before `validate_deep`: PNG — CRC of every chunk across the join; JPEG — marker sync + RST sequence across the join; PDF — object/xref consistency (H-03). Full decode only for survivors.
  5. **Never `verified` without the deep validator**; a stitch that passes only a lexical check must be `structural` (H-01/H-03).
  6. Report `stats.fragment_attempts`, `fragment_heads_tried/exhausted`, `fragment_truncated`; keep exit 0 until §9 Q4 is answered (§3.3).
- **Red tests.** Fixture S-B06 for all gap contents (zeros, random, another file's bytes) × JPEG/PNG/PDF: with default limits, every file is recovered **byte-exact** *(target; if not reachable, document the measured recall in `DECISIONS.md` rather than loosening the fixture)*; the PDF stitch is never labelled `verified` unless `qpdf --check` passes; `splice_rate` (outputs with `source=bgc` whose SHA-256 is not an original) = 0 on the fixture set; a layout with > 8,192 footer needles before the target (footer truncation case); wall ≤ 5 s at defaults. The existing `tests/fragment_recovery.rs` S12 tests must keep passing.
- **Acceptance.** Recall and splice rate are measured on the D1 fragmentation profile **dev** split (never the lockbox) before/after with confidence intervals (guide B-06); success rate reported per fragment count; B-06 stays **opt-in**.
- **Pitfalls.** Do not tune any heuristic on the lockbox or on the corpus files by name/offset; do not flip `--fragment-recovery` to default.

#### H-18 `regions[]` is not a coverage map; unparsable volume silently downgraded [P1, MUST]
- **Status on `ab725a6`: FIXED** (repo H-26 and H-27; [REPRODUCED], Appendix A.2). S-REGIONS (MBR: 1 MiB gap | 32 MiB FAT32 | 2 MiB gap | 16 MiB FAT32 = 53,477,376 B): `regions[]` is now a disjoint, ordered coverage map whose scanned rows sum to `bytes_scanned` (53,067,776); on `a7a5c08` they summed to 53,477,376 (overlapping, 409,600 B off). A second volume with an NTFS signature and garbage boot sector → exit 3, warning `VOLUME_PARSE_FAILED` (`a7a5c08`: exit 0, no warning, silently downgraded). *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.**
  - [REPRODUCED] Two-FAT32-volume MBR disk (32 MiB + 16 MiB, 1 MiB and 2 MiB gaps, 51 MiB total): for each volume `regions[]` contains **both** a `free` region (`len` = the **whole volume length**, `scanned=true`) and an `allocated` region (same offset, same length, `scanned=false`); gaps are `scanned=true`. Sum of `scanned` regions = 53,477,376 (= image size) but `bytes_scanned` = 53,067,776, so the map neither reconciles with the byte counter nor describes free vs allocated space.
  - [CONFIRMED-STATIC; v5 reproduced with a corrupted-NTFS volume] `src/cli.rs` (≈ 480-520): the `VOLUME_UNSUPPORTED` warning that carries the parse reason is emitted only `if !include_gaps`; with the default (all gaps scanned) a partition whose boot sector is recognised but whose MFT fails to parse is scanned raw with **exit 0, empty `warnings[]`** and without `RAW_SCANS_ALLOCATED`; `VolumeSet.errors` (`src/fs/mod.rs:336-357,413-435`) never reach the report. The single-volume fallback message "no FAT32/NTFS filesystem found … no usable MBR/GPT" is misleading when an NTFS boot sector *was* found.
- **Fix.** Build `regions[]` from the real extents: coalesced `free` runs and `allocated` runs with their true byte lengths (or one aggregate per kind with a `count`), `gap`, `unsupported`; **invariant** `Σ len(scanned=true) == bytes_scanned` (document any alignment slack) asserted in a test. Add a coded `VOLUME_PARSE_FAILED` warning (reason from `VolumeSet.errors`) whenever a partition with a recognisable boot signature fails to parse — regardless of `--include-gaps` — with **exit 3**; mark the fallback region `kind: unsupported, scanned: true, contains_allocated: true` and emit `RAW_SCANS_ALLOCATED`. Fix the misleading single-volume message to quote the real cause.
- **Red tests.** Multi-volume invariant test (the reproduced layout); corrupted-NTFS-in-multi-volume (the S8 corruption of `guide.md` Appendix A placed inside a disk) ⇒ warning + exit 3 + region flag.
- **Acceptance.** Invariant holds on D0 multi-partition images; S1/S8 scenarios of the guide remain green.

#### H-19 Gates and fuzzing: G5/G7/G8/G9, container and B-06 fuzz targets, CI [P1, MUST]
- **Status on `ab725a6`: FIXED (static)** (repo H-14 and H-30; [CONFIRMED-STATIC]). `fuzz/` has 13 targets including `fuzz_e01`, `fuzz_vhd`, `fuzz_vmdk`, `fuzz_split_names`, `fuzz_open_image` and `fuzz_fragment_join` plus committed seeds; `.github/workflows/ci.yml` runs the gates (fuzz smoke on push/PR and nightly, coverage ratchet, golden, perf, docs); `scripts/gate.sh` implements G0–G9. **NOT RE-RUN here:** fuzzing, coverage, golden, perf gate, CI (offline; no nightly toolchain). Re-verify G5/G7/G8 on the agent's machine in Phase H0 before relying on them. *Do not re-implement; keep as a regression guard.*
- **Evidence (all [CONFIRMED-STATIC]).** `scripts/gate.sh` runs fmt, clippy, tests, doc, actionlint, deny, audit, MSRV, release build, `review_repro.py`, benchmark smoke — **no G7 (golden), G8 (perf) or G9 (docs/numbers)**; its header cites "guide-v2 §3.2" (`:2`). `fuzz/fuzz_targets/` has `jpeg, png, pdf, fat32_boot, ntfs_boot, ntfs_mft, partition_table` (+`common.rs`) — **none** for `src/image/*` (E01/VMDK/VHD/split discovery), B-06 join code or the PDF verifier. `.github/workflows/ci.yml`: the `fuzz` job runs only on `schedule`/`workflow_dispatch`; coverage floor `--fail-under-lines 60` (guide: 75–85 %). CI is configured for Linux/Windows/macOS but no run after B-04/B-06/R-01/R-02 can be verified offline (v4 F-15) — record `PENDING-CI`.
- **Fix.**
  - **G7:** `scripts/golden.py` comparing SHA-256 of every output file of the D0 `unallocated` and `raw` runs with `bench/golden/D0.sha256` (frozen in Phase H0).
  - **G8:** `scripts/perf.sh` running the Appendix-A performance scenarios against `bench/perf/baseline.json` (recorded in H0); fail on > 10 % regression or when a documented memory cap is exceeded.
  - **G9:** `bench/render_docs.py --check` (create it if missing): README / `docs/REPORT.md` (the guide's `BAO_CAO`) numbers equal `results.json`; the README flag table equals `carvex --help`.
  - Add all three to `scripts/gate.sh` and CI; fix the `guide-v2` citation.
  - **Fuzz targets:** `fuzz_e01`, `fuzz_vmdk` (descriptor + sparse), `fuzz_vhd`, `fuzz_split_discover`, `fuzz_fragment_joins` (synthetic `FsInfo`/extents/footers into `candidate_joins` + `try_fragment_join`), `fuzz_pdf_verify` (H-03); seeds from the Appendix-C builders; every crash becomes a small fixture in `tests/fixtures/crashes/` plus a regression test. CI: **60 s smoke per target on pull requests**, nightly 10 min/target, release 30 min/target (guide A-03 durations).
  - `review_repro.py` must not turn a missing image into a failure: that is **H-26** (Phase H0); here only wire the repro step into CI with `CARVEX_REQUIRE_IMAGES=1`.
  - Raise the coverage floor to the measured value minus 2 points now, and toward 75 % as items land; no empty tests to inflate it.
- **Red tests.** Prove each new gate can fail: on a scratch branch change one golden hash, one baseline time (×0.5) and one README number — G7, G8 and G9 must each turn red (do not commit these mutations); a deliberately crashing input added to a fuzz target's corpus must fail the smoke run.
- **Acceptance.** `scripts/gate.sh` runs G0–G9 (or logs `SKIPPED` with reason); a PR touching `src/image/` triggers the fuzz smoke; golden and perf baselines exist in the repo; a green 3-OS CI run after the last H1–H3 commit is attached to the end-of-phase report (or `PENDING-CI`).


#### H-25 Truncated-file end: only zero padding is trimmed; random or `0xFF` bytes after the cut are swallowed or lose the file [P1, MUST]
- **Status on `f5bda4a`: FIXED** (no repository item; commits `5a8eaf5`, `b4c08a6`, `794512d`; D-70, which also answers §9 Q10; A.3 *S-TAIL*, *S-TAIL-NEIGH*, [REPRODUCED]). The carved end of a truncated JPEG/PNG/PDF is the last verified byte, not the end of the padding. Sizes (cut → zeros / `0xFF` / seeded random): JPEG 212,468 → 212,468 / 212,468 / 212,892; PNG 839,272 → 839,272 / 839,272 / 839,609; PDF 374,172 → 374,172 / 374,172 / 374,596. 48 MiB JPEG: zeros 212,468, random 212,892 (`rejected=2`, file emitted), `0xFF` 4 MiB and 48 MiB 212,468; `too_large_truncated` 0 — no file is lost any more. `tail_trimmed_bytes` exists (H-01); if a bound still binds the run reports `TRUNCATED_TOO_LARGE` (structured, exit 3). **Residue, by decision (D-70):** random/ASCII padding after the cut still adds 337–424 B (less than one 4 KiB cluster), because the end is a *structural* end, not a content hash. **Side effect:** a truncated file followed by another file is now emitted — S-TAIL-NEIGH (gap 4,096 B): truncated JPEG 212,468 B (`truncated`/`partial`) + complete 234,049 B (`structural`), `carved=2 rejected=0` (was: the truncated one `rejected`). *Do not re-implement; the guard is in §8.*
- Previous status on `ab725a6` (history, superseded by the line above): **OPEN — unchanged** (the repository has no item for it; [REPRODUCED] Appendix A.2 *S-TAIL* and *S-TAIL-NEIGH* give the same sizes, notes and losses as on `a7a5c08`: zero padding exact, `0xFF`/random padding swallowed up to 1 MiB, random-padded JPEG lost (`rejected`), 48 MiB `0xFF` padding → `too_large_truncated`, file lost). The only change is the new `validity=partial` label on the truncated rows. (History: on `ab725a6` the whole item was to be implemented; it is now done.)
- **Evidence.** **[REPRODUCED]** (Appendix A S-TAIL; builder `tail_fixtures`, Appendix C). Corpus files cut at 60 % and followed by padding, scanned alone (`--mode raw --no-fs --types <t>`, default options). Sizes are the carved file sizes:

  | Padding after the cut | JPEG `jpeg_10_progressive_1200x800.jpg` (cut = 212,468 B) | PNG `png_02_rgb_800x600.png` (cut = 839,272 B) | PDF `pdf_05_large_scan.pdf` (cut = 374,172 B) |
  |---|---|---|---|
  | zeros (JPEG 48 MiB; PNG, PDF 1 MiB) | 212,468 B ✔ | 839,272 B ✔ | 374,172 B ✔ |
  | random, seeded (same lengths) | **0 outputs** (`stats.rejected ≥ 1`) | 852,157 B (**+12,885 B**) | 1,422,748 B (**+1,048,576 B**, all of the padding) |
  | `0xFF` × 1 MiB | **1,261,044 B** (all of the padding) | 852,157 B (+12,885 B) | 1,422,748 B (all of the padding) |
  | `0xFF` × 4 MiB | 4,406,772 B (all of the padding) | – | – |
  | `0xFF` × 48 MiB | **0 outputs**; only `stats.too_large_truncated = 1`; `warnings[]` empty; exit 0 | – | – |

  Realistic case (`tail_jpeg_neigh_0.img`, `tail_jpeg_neigh_4096.img`): the same truncated JPEG directly followed — with 0 or 4,096 zero bytes between — by the complete `jpeg_03_progressive_800x600.jpg` (234,049 B). Only the complete file is carved (`carved = 1`, `rejected = 1`); the truncated one is lost without any warning. On a disk the bytes after a truncated file are other data, so this is the common case, not the zero-padded one.
- **Causes.** **[CONFIRMED-STATIC]**
  - The only tail trimming is `trim_zero_tail` (`src/scan.rs:1231`, applied at `:1110`): zero bytes only. Nothing else bounds an incomplete candidate.
  - JPEG (`src/signatures/jpeg.rs`): in entropy data an `FF xx` whose `xx` is not `00`, `01`, `D0`–`D7` or `D9` is parsed as the next *segment* (`:230`). Random bytes then give a random segment length and the next position is not `FF`, so the candidate ends in a `Reject` of the segment parser — typically `expected JPEG marker …` (`:39-41`) or `invalid JPEG segment length` (`:120-122`) — **although SOF and SOS were already seen** — a true positive is thrown away. A run of `FF` is skipped by the loop at `:185` and ends at the buffer end (`truncated(data.len(), …)`), so all of it becomes file content; beyond `--max-truncated-size` the candidate is dropped (`src/scan.rs:1120`) and only the counter `stats.too_large_truncated` records it (`:1708`; shown on the console "Filtered" line, never in `warnings[]`).
  - PNG (`src/signatures/png.rs`): a chunk whose declared length runs past the verified data is accepted up to its declared end (bad CRC only counted, `bad_crc`); `end = last_good` = the end of that chunk. Here the IDAT chunk containing the cut ends at 852,157, i.e. exactly 12,885 B of padding counted as image data.
  - PDF (`src/signatures/pdf.rs:62-74`): without `%%EOF` the end is `search_end`, the buffer end ("no %%EOF marker within the scan limit").
  - `tail_trimmed_bytes` (guide A-05) exists nowhere (`grep -r tail_trimmed src docs` is empty) and `tests/properties.rs` has no truncation property.
  - The only truncation scenario in the repro suite is R6 "zero-tail truncation" (`scripts/review_repro.py`).
  - All five reviews accepted A-05 as implemented/met; each tested zero padding only (v1: "random/0xFF padding not re-run"; v2: "property test not in `tests/properties.rs`").
- **Fix.** The end of an incomplete candidate is the **last verified byte**, never the buffer end; the field `tail_trimmed_bytes` (introduced by H-01) carries the number of bytes cut off.
  1. **JPEG:** after SOS has been seen, a continuation that is not valid (an `FF xx` that is not a legal entropy continuation, or a segment that fails to parse: `expected JPEG marker`, `invalid JPEG segment length`) ends the candidate as **`truncated` at the offset of that `FF`** instead of `Reject` (SOF + SOS prove it is a JPEG; everything before that offset was consistent). A run of `FF` at the end of the candidate is never entropy data (data `FF` is always stuffed as `FF 00`): trim it like zeros.
  2. **PNG:** end of an incomplete candidate = the last byte up to which the zlib stream of the IDAT payload still inflates (stop at the first inflate error), else the end of the last chunk whose CRC verified. Reuse the existing decode budget (`--max-decode-bytes`, H-12) — never inflate unbounded.
  3. **PDF:** end = after the last complete top-level object (`endobj`), or, for a `FlateDecode` stream that is cut, the inflatable prefix of that stream as for PNG; if the cut stream cannot be proven, take the conservative end and note it (Q10).
  4. **Generic net** in `carve_one` (all incomplete candidates only): also trim a trailing run of one repeated byte (`0x00`; `0xFF` ≥ 512 B) — zero behaviour stays as today.
  5. A truncated candidate dropped by `--max-truncated-size` produces the warning `TRUNCATED_TOO_LARGE` (`type`, `offset`, `size`) in `warnings[]` and counts toward exit 3 like the other limit events (H-12); keep the stat.
  6. Leave complete files, zero-padded results (§8) and scores/labels unchanged (`truncated`/`partial`, score 60, unless H-01 renames the vocabulary).
- **Red tests.**
  - `tests/regress_tail_padding.rs` (builders in `tests/common/`, Appendix C `tail_image`): for ≥ 3 corpus files per format (include the progressive JPEG and the large-scan PDF), cut at ≥ 10 fractions in [0.15, 0.95], pad with {zeros, `0xFF`, seeded random, ASCII text} × {4 KiB, 1 MiB}, plus 48 MiB for JPEG. Assert: at most one output per cut file; if there is an output then `size ≤ cut + 4,096` (guide A-05) and `output[..size − 4,096] == original[..size − 4,096]`; for JPEG and for Flate-compressed PNG/PDF streams also `size ≥ cut − 4,096`; the label stays `truncated`/`partial`, never `complete`/`verified`; `tail_trimmed_bytes` + kept bytes = bytes before the next cut-off boundary.
  - Neighbour test: `tail_jpeg_neigh_0/4096` ⇒ **both** files recovered (truncated one `partial` within the bounds above, complete one `verified`/`structural` with its exact original SHA-256); no nested suppression of the complete file.
  - `--max-truncated-size 64K` on a 200 KiB truncated file ⇒ warning `TRUNCATED_TOO_LARGE`, exit 3, schema-valid report.
  - Property test in `tests/properties.rs` (proptest, ≥ 256 cases): minimal valid JPEG/PNG/PDF built in the test, cut at a random position, random/`0xFF`/zero padding of random length ⇒ the bounds above. This is the property test guide A-05 asks for.
  - Controls: a complete JPEG whose entropy data contains `FF 00` just before EOI, and a complete PNG/PDF followed by padding, are unchanged (same size, SHA-256).
- **Acceptance.** Every cell of the S-TAIL table satisfies the bounds; the zero rows are byte-identical to the snapshot (§8); `S-TAIL-NEIGH` recovers both files; golden D0 unchanged or every change listed in CHANGELOG; header-flood timings of §8 not slower by more than 10 %.
- **Pitfalls.** Do not `Reject` a candidate that already has SOF and SOS; trim `FF` only at the **end of the candidate**, never inside entropy data or before a marker (the specification allows `FF` fill bytes there); a data stream may legitimately end in `FF 00` (do not trim stuffed bytes); do not trust that padding "looks invalid" — test the all-`0xFF`, all-`0x00` and random cases; `--repair` appends its suffix at the new end, so re-run the repair tests of H-04; the fix must be O(tail), not a rescan per byte.

#### H-26 The local gate cannot pass without root: `review_repro.py` counts `SKIP` as failure [P1, MUST — first item of Phase H0]
- **Status on `ab725a6`: FIXED** (repo H-02; [REPRODUCED], Appendix A.2). `scripts/review_repro.py --assert --xfail scripts/repro_xfail.txt` → "34 scenario(s), 0 failure(s), 6 SKIP(s), 0 XPASS", exit 0 (SKIPs: R7, R8, X08, X17, X19, X20 — missing `mkfs.vfat`/`mkntfs`/`qemu-img`/images); `--require-all` turns SKIPs into failures. The repository's X01–X24 scenarios are mapped in Appendix D. *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.** **[REPRODUCED]** `python3 scripts/review_repro.py --carvex target/release/carvex --workdir W --assert --xfail scripts/repro_xfail.txt` on a checkout without the FAT32/NTFS image fixtures: R1–R6 `PASS`, R7 and R8 print `FAIL … SKIP: no NTFS image (pass --ntfs-image)` / `no FAT32 image …`, summary "10 scenario(s), 2 failure(s), 0 XPASS", **exit 1**.
- **Causes.** **[CONFIRMED-STATIC]** `case_r7` and `case_r8` return a `"SKIP: …"` *string* (`scripts/review_repro.py:367,416,418`; also `:262`, `:286`) and the runner treats every non-empty return value as an error (`failures += 1`, `:582`; `:595` returns 1 under `--assert`). The images are git-ignored and `bench/make_images.py` needs root (`losetup`, `mount`, `mkfs.vfat`, `mkntfs`). `scripts/gate.sh:55-78` (`set -euo pipefail`) runs `make_images.py` with errors ignored, prints `SKIPPED R7 …`/`SKIPPED R8 …` and then runs `review_repro.py --assert`, which exits 1: the gate **cannot go green on any unprivileged machine or container without loop devices**, although the script itself says the scenarios were skipped. An agent following this file would either never reach "gate green" or learn to ignore the gate. CI does not run `review_repro.py` at all (H-19).
- **Fix.**
  1. `scripts/review_repro.py`: a skipped scenario becomes a first-class outcome. Cases return a small `Skip(reason)` value (not a string); the runner prints `SKIP R7 <title>: <reason>`, counts `skipped`, and the summary reads "N scenario(s), F failure(s), X XPASS, S skipped". `--assert` exits 1 only if `F or X`. A new flag `--require-all` additionally exits 1 when `S > 0`. The PASS/FAIL logic of every scenario is unchanged — this is a runner fix, not a loosened assertion; say so in the commit message.
  2. The skips at `:262` and `:286` ("corpus PNG has no IEND", "corpus JPEG has no SOS") mean the corpus is not what the scenario expects: they must stay **failures** (return an error string), not become skips.
  3. `scripts/gate.sh`: pass `--require-all` when `CARVEX_REQUIRE_IMAGES=1` (set it in CI once H-19 adds the repro step and builds the images with `sudo`); otherwise keep the `SKIPPED …` lines and continue. The gate's final summary lists every skipped scenario (§10, item 2).
  4. **[SHOULD]** A no-root path for R8: build the multi-volume FAT32 image with the Appendix-C `fat32`/`make_image` builders (or `mkfs.vfat` / `mcopy` on a regular file, which need no root) so R8 runs everywhere. R7 (NTFS fallback) keeps needing an image: `mkntfs -F` on a regular file where available, else `SKIP`. Do not touch `bench/images/*.json` or other benchmark inputs (anti-gaming).
  5. Record in `docs/PROGRESS.md` → Environment which scenarios were skipped on the agent's machine and why.
- **Red tests.** New `scripts/test_review_repro.py` (plain `unittest`, run from `gate.sh`): with a stub `--carvex` and no image arguments `main()` returns 0 and the output contains `2 skipped`; with `--require-all` it returns 1; with a stub that makes R1 fail it returns 1 (failures are still detected); an XPASS still returns 1.
- **Acceptance.** On a clean unprivileged checkout without `bench/images/*.img`: `python3 scripts/review_repro.py … --assert` exits 0 and prints `2 skipped` (before: exit 1, `2 failure(s)`); with `--require-all` it exits 1; where the images exist the PASS/FAIL lines are unchanged; `scripts/gate.sh` reaches its end on a machine without root.
- **Pitfalls.** A scenario that *can* run must run — only a missing external fixture may skip; do not widen `repro_xfail.txt` to hide R7/R8; do not edit the assertions of any scenario.

### 7.3. Priority P2 — Phase H4

#### H-16 Auto-detect reinterprets ordinary raw images as VMDK/VHD [P2, SHOULD]
- **Status on `ab725a6`: FIXED** (repo H-24; [REPRODUCED], Appendix A.2). S-DETECT: a raw file containing `# Extent description`, and one with a bad-checksum `conectix` footer, are scanned as `input.format=raw` (exit 0, JPEG at 65,536 carved) instead of exit 1; a bare `KDMV` magic at offset 0 is still rejected ("VMDK sparse extent has no embedded descriptor", exit 1) — a deliberate magic match; `--input-format raw` carves it. *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.**
  - [CONFIRMED-STATIC] `detect()` (`src/image/mod.rs:211-253`) decides the container type from weak signals only: the first 4 bytes `KDMV`; the first 8 bytes of the **last 512 bytes** equal to `conectix`; or the 20-byte string `# Extent description` **anywhere in the first 64 KiB**. No structural check precedes the decision, and a later parse failure is returned as an error.
  - [REPRODUCED] Three plain raw images (≈ 1 MiB each; Appendix A S-DETECT): (a) text `# Extent description` inside the first 64 KiB, (b) `conectix` at the start of the last 512 bytes, (c) first four bytes `KDMV`. `carvex scan <img>` (auto) → **exit 1** with "VMDK descriptor has no extents" / "VHD footer checksum mismatch" / "VMDK sparse extent has no embedded descriptor"; the same images with `--input-format raw` → exit 0. A disk that merely *contains* a VMDK descriptor file near its start (for example a data partition holding a VM folder) cannot be scanned in the default mode.
- **Fix.**
  1. **Strict probes.** VMDK descriptor: after an optional BOM/whitespace the text must start with `# Disk DescriptorFile`, be ≤ 1 MiB and be valid UTF-8/ASCII, and contain at least one extent line accepted by the extent grammar. VHD: cookie `conectix` **and** valid footer checksum, `features` reserved bit set, sane `disk_type` ∈ {2, 3, 4} and `current_size > 0`. VMDK sparse: `KDMV`, `version ∈ {1,2,3}`, `capacity > 0`, `grain_size` a power of two ≥ 8 sectors. E01 keeps its 8-byte magic.
  2. **Fallback only for weak probes.** In `--input-format auto`, if a *weak* probe (descriptor text, VHD footer, `KDMV`) fails its strict checks, open the file as raw and add the informational warning `CONTAINER_PROBE_FAILED` (reason included; exit stays 0, like `RAW_SCANS_ALLOCATED`). If the strict checks **pass** and the later parse fails, that is an error (no silent reinterpretation of a genuine container). An explicit `--input-format vmdk|vhd|e01` never falls back.
  3. Record the chosen format in the report (`input.format`, H-20).
- **Red tests.** The three S-DETECT images ⇒ auto scan exits 0 and carves the JPEG planted at a known offset; explicit `--input-format vmdk` on image (a) still fails with the parse reason; the genuine fixtures of Appendix C (E01, sparse VMDK, descriptor+flat VMDK, VHD) are still auto-detected; existing `tests/evidence_formats.rs` unchanged and green.
- **Acceptance.** As above; add the warning code to `docs/report.schema.json` and to the README warning table.
- **Pitfalls.** Do not make the probes *looser* to be safe; do not read more than 64 KiB for detection; do not follow paths found in a descriptor during detection (H-15).

#### H-20 Container and stdin report integrity; spool limits [P2, SHOULD]
- **Status on `ab725a6`: FIXED** (repo H-25; [REPRODUCED], Appendix A.2). S-STDIN: `input.format="stdin"`, `input.unchanged` is `null` (not the false `true` of `a7a5c08`); `--max-stdin-bytes 100000` → exit 1 "stdin image exceeds --max-stdin-bytes" (the flag did not exist); `--spool-dir` exists. Container reports carry `input.format` (e.g. `vmdk`). *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence.**
  - [REPRODUCED] Mixed compressed/uncompressed E01 fixture (8 × 32 KiB chunks, Appendix C `build_e01`): `input.size = 262,144` (logical) but `input.size_after = 227,301` (physical size of the `.E01` file) with `input.unchanged = true`. The two fields use different units, so they cannot support the integrity claim they exist for (v5 F-26 measured the same on VMDK 67,108,864 vs 14,614,528).
  - [REPRODUCED] stdin (`scan - < disk.img`): `input.path = "-"`, `modified_unix = null`, `modified_unix_after = null`, **`unchanged = true`** — no identity check was possible; the claim is vacuous.
  - [CONFIRMED-STATIC] `open_stdin()` (`src/image/mod.rs:188-209`) copies the whole stream with `io::copy` into `NamedTempFile::new()` in the default temp directory (often RAM-backed) with **no size limit and no free-space check**.
  - The container format, segment list and resolved extent files are not recorded anywhere in the report.
- **Fix** (additive only).
  1. `input.logical_size`, `input.physical_size`, `input.physical_size_after` (sum over all files of the container); keep the existing `size`, `size_after`, `unchanged` but define them in `docs/report.schema.json` as "physical, comparable with each other" and make `unchanged` compare the **physical** before/after values and mtimes of **every** file of the container.
  2. `input.format` (`raw|split|e01|vhd|vmdk|stdin`) and `input.files[]` = `{path, size, modified_unix}` for every file actually opened (split segments, VMDK extents; relates to H-15).
  3. stdin: `unchanged: null` and `input.identity: "spooled-copy"`; the report must say that the scanned bytes are a copy.
  4. `--spool-dir <DIR>` and `--max-spool <SIZE>` (default proposed 64 GiB — record the final value in `DECISIONS.md`); before spooling, check free space when the OS reports it; on overflow stop with an error that names `--max-spool`, remove the partial spool and exit 1. `--hash-image` on stdin hashes the spool as it is written.
- **Red tests.** E01 and VMDK fixtures: `logical_size` equals the guest size, `physical_size == physical_size_after == Σ files[].size`, `unchanged == true`; stdin: `unchanged == null`; a stream longer than `--max-spool` aborts cleanly (no partial file left in the spool dir); schema validation test updated.
- **Acceptance.** `jq` assertions above on the Appendix-C fixtures; golden outputs unchanged (only the `input` block grows).

#### H-21 Release identity: placeholder URL, `commit: "unknown"`, stale `dist/`, author paths [P2, MUST for a release tag]
- **Status on `ab725a6`: FIXED** (repo H-04 and R2-04, R2-10; [REPRODUCED], Appendix A.2). `Cargo.toml` `repository = "https://github.com/logosnexus/carvex"`; `tool_info.commit` = `ab725a6674b6` in a source build; `dist/carvex-linux-x86_64` is `carvex 0.3.1`, `commit=be4217f7e2b4` (no `src/` change since), exposes the same 49 `scan` flags as the source build, and `scripts/check_dist.sh` → "DIST OK" (Windows binary skipped: wine missing). The README still keeps `/home/kali` paths in committed ground truth and says so (accepted by the repository). *Do not re-implement; the Evidence/Fix text below is the `a7a5c08` picture. Keep the acceptance command as a regression guard (§8).*
- **Evidence (all [CONFIRMED-STATIC] unless stated).**
  - `Cargo.toml:10` `repository = "https://example.invalid/carvex"`.
  - `tool_info.commit` comes from `option_env!("CARVEX_GIT_COMMIT")` (`src/scan.rs:809`); nothing in the repository (no `build.rs`, no CI step, no gate) ever sets it ⇒ every build reports `"unknown"` [REPRODUCED]; `tool_info` has no compiler version.
  - `dist/` holds a prebuilt Linux and a Windows binary plus `SHA256SUMS`. [REPRODUCED] `dist/carvex-linux-x86_64 --version` prints `carvex 0.2.0` but its `scan --help` lacks eight flags that the source has (`--fragment-recovery --frag-time-limit --input-format --max-candidates --max-decode-bytes --max-frag-candidates --max-gap --time-budget`). Same version string, different behaviour.
  - `grep -rl "/home/kali" bench docs README.md CHANGELOG.md` lists **179 files** (e.g. `bench/images/*.json`, `bench/results/final/**`): author-machine absolute paths in committed data.
  - `scripts/gate.sh:2` cites "guide-v2 §3.2" (the guide of record is `guide.md`).
- **Fix.**
  1. `Cargo.toml` `repository` and `homepage` → `https://github.com/jamesinlogos/carvex` (the repository named in the task).
  2. Add `build.rs` that sets `CARVEX_GIT_COMMIT` (from `git rev-parse --short=12 HEAD`, plus `-dirty` when the tree is modified; fall back to `unknown` only when git is unavailable) and `CARVEX_RUSTC` (from `RUSTC --version`), with `cargo:rerun-if-changed=.git/HEAD`. Report `tool_info.commit` and `tool_info.rustc`. No new dependency.
  3. `dist/`: **STOP-AND-ASK Q6** (rebuild and publish with matching `SHA256SUMS`, or remove `dist/` from the tree and publish binaries as release assets only). Until the answer arrives, add a loud line to `dist/README.md`: "built from an older commit; do not use for evaluation".
  4. Do **not** edit `bench/images/*.json` or any committed result (anti-gaming, §3.1). Document the author-path leakage in `docs/VALIDATION.md` and make the runner **stop writing** absolute paths for future runs (store paths relative to the repo root).
  5. Fix the `guide-v2` citation in `scripts/gate.sh`; bump the version per `guide.md` §3.4 when behaviour changes (this round changes defaults only after §9 answers, so likely `0.3.0`; record in CHANGELOG).
- **Red tests.** Integration test: `carvex --version --json`-style output or the report's `tool_info.commit` is not `unknown` when built in a git checkout, and `tool_info.rustc` is non-empty; `cargo metadata` check that `repository` is not `example.invalid`.
- **Acceptance.** Gate has a check `! grep -q "example.invalid" Cargo.toml`; `dist/` decision recorded.

#### H-23 Documentation honesty [P2, MUST for a release tag]
- **Status on `f5bda4a`: FIXED** (repo H-01, H-31, H-32, H-33; [REPRODUCED] `scripts/check_status.py` and test count). The README test count is generated and equals `cargo test` (148); `docs/VALIDATION.md` no longer cites `/tmp/opencode` (only historical lines of PROGRESS/DECISIONS do); `scripts/check_status.py` → STATUS GREEN (38 guide IDs and 34 harness IDs agree). G9 locally [NOT RE-RUN] (`bench/render_docs.py --check` needs the repository's `target/release`); CI run `45749230527` green [REPORTED]. The wording constraints of the Fix below stay binding for every new README sentence — including any claim about Part T.
- Previous status on `ab725a6` (history, superseded by the line above): **PARTLY FIXED** (repo H-01, H-31, H-32, H-33; [REPRODUCED] `scripts/check_status.py`, `bench/render_docs.py --check`). Done: `docs/guide.md` is in the repository (committed verbatim; the repo records its sha256); open work is labelled `PARTIAL` in PROGRESS (e.g. repo H-04, H-18) instead of FIXED; `scripts/check_status.py` → "STATUS GREEN: 38 guide IDs and 34 harness IDs agree"; `bench/render_docs.py --check` → "G9 GREEN: 48 scan options match README"; README speed/Pillow claims were rewritten (repo H-33).
- Remaining work as of `ab725a6` (history — closed on `f5bda4a`; do not redo):
  1. README ("The current suite has 134 tests") and PROGRESS say 134 tests; `cargo test` runs **135** — the README breakdown omits `regress_task_plan` (1 test). Generate the number (and the breakdown) from `cargo test` output in the render script so it cannot drift.
  2. `docs/VALIDATION.md` cites evidence logs under `/tmp/opencode/…` (lines ≈ 398, 578, 608) that are not in the repository: archive them under `docs/evidence/` with SHA-256, or mark them "not archived".
  3. Re-run `scripts/check_status.py` and the G9 check after every status edit.
- *The Evidence/Where/Fix text below is the `a7a5c08` picture; nothing remains to implement; keep the acceptance command as a regression guard (§8).*
- **Evidence ([CONFIRMED-STATIC] unless stated).**
  - `guide.md` is **not in the repository**; `docs/DECISIONS.md` D-01 states the guide was truncated at §3.5 and that `guide-v1/v2/v3` were used instead.
  - `docs/REVIEW_RESPONSE.md` marks **FIXED** items that this verification could not confirm: A-04 (labels, H-01/H-02/H-03/H-09), A-06 (budget excess fatal, H-12), A-10 (limits unmet, H-22), A-14 (fields missing, H-01), D-03 (release hygiene, H-21), and "MITIGATED" B-06 (recall 0/3 at defaults, H-17).
  - README (≈ line 374): "CarveX is significantly faster in the unallocated configuration because it reads only about 61–62 MiB instead of 64 MiB per image, while also using parallel scanning" — a speed claim with no stated comparison conditions here **[NOT RE-RUN]**. README (≈ lines 416-419): "318/318 files `complete`, successfully opened and verified with Pillow" — `complete` is a footer-found status, not validity (H-01); the README itself says there is no ground truth for that image.
  - Duplicate verbose line (H-01) is a user-visible defect to be listed in CHANGELOG.
- **Fix.**
  1. Add `guide.md` and this `harness.md` under `docs/` (Phase H0, single commit). Replace D-01's "truncated" wording with the actual state: *the guide was truncated when the first build ran; the complete guide is now in `docs/guide.md`*; keep the original text as history.
  2. **Status honesty rule (§3.2.8):** after each item lands, update `REVIEW_RESPONSE.md` for the guide item it touches, quoting the acceptance command that passed. Items that were over-stated are changed to **PARTIAL** *now* (Phase H0) with a pointer to the open `H-xx`, and back to FIXED only when the item's acceptance passes.
  3. README: delete the speed sentence, or regenerate it through `bench/render_docs.py` from a result file that states machine, image set and thread count (numbers are never typed by hand — `guide.md` §3.2); replace "complete … verified with Pillow" with the validity vocabulary (after H-01: *N files structural, M verified by Pillow `load()`*), and say plainly that the 3.73 GiB image has no ground truth.
  4. Keep README flag table = `carvex --help` (enforced by G9, H-19).
- **Red tests / Acceptance.** G9 (`bench/render_docs.py --check`) green; a script greps README/REPORT for the banned phrases `significantly faster` (without a table reference) and `verified` next to `complete` (allow-list file for legitimate uses); `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` green.

#### H-24 Benchmark and scope wording [P2, SHOULD]
- **Status on `ab725a6`: NOT RE-RUN** (repo H-33 rewrote the README speed/Pillow sentences; benchmarks and `bench/results` were not re-run here). Treat the wording constraints below as still binding.
- **Evidence.** [NOT RE-RUN] Reviews v1–v4 note that the benchmark is synthetic and narrow (JPEG/PNG/PDF, FAT32/NTFS D0/D1-style profiles, generated corpus), compares against Foremost/Scalpel with a different search space, has no lockbox repeat for 0.2.0, and that B-01 (metadata recovery) and B-03 (exFAT) are unimplemented. The repository already states most of this in `docs/REPORT.md §6.4` and `docs/REVIEW_RESPONSE.md`.
- **Fix (wording and metadata only — no benchmark is re-run, no baseline or threshold changes).**
  1. Every headline number in README/REPORT carries n, corpus, profile, confidence interval and `version/commit` of the tool that produced it; numbers produced by 0.1.0 (CFReDS RM#2) are labelled as such.
  2. "exact" is used only in the sense of byte-identical SHA-256 to a ground-truth file; "usable" and "viewable" are defined next to their first use (guide C-04).
  3. Numbers in README/REPORT come from the render script (`guide.md` §3.2, C-07), never typed by hand. A "Scope and limits" box lists: formats (JPEG/PNG/PDF), filesystems (FAT32/NTFS; exFAT/ext4/APFS not supported), containers (E01 v1, VMDK, VHD, split raw; not Ex01/AFF4), B-01/B-03 not implemented, B-06 two-fragment only and opt-in.
  4. Do not claim "forensically validated" or "court-ready"; the tool has no independent certification.
- **Acceptance.** G9 green; reviewer checklist in the end-of-phase report: each headline number has its conditions on the same line.
- **Pitfall.** Any *new* benchmark run is a separate task under `guide.md` Part C and the anti-gaming rules; it is **not** part of this item (§9 Q5 if metric definitions must change).

#### H-22 A-10 limits: oversized functions and `allow(too_many_arguments)` [P2, SHOULD — **last item of the round**]
- **Status on `f5bda4a`: FIXED** (repo H-34, commit `0f3522a`; [REPRODUCED] clippy and `scripts/fn_len.py`). `#[allow(clippy::too_many_arguments)]` count **0** (13 on `ab725a6`); `cargo clippy --lib -- -W clippy::too_many_lines` 0 findings; `cargo clippy --all-targets --all-features --locked -- -D warnings` clean; `scripts/fn_len.py` (raw lines, informational, stricter) 3 findings: `src/signatures/jpeg.rs:703` `content_continuity_check` 129, `src/signatures/pdf_validate.rs:138` `validate` 125, `src/signatures/jpeg.rs:264` `analyze` 124. Golden and performance after the refactor: [REPORTED] green by the repository, not re-run here. *Do not re-implement.*
- Previous status on `ab725a6` (history, superseded by the line above): **PARTLY FIXED** (repo H-34 / D-58 / R2-07; [REPRODUCED] clippy and `scripts/fn_len.py`). Done: by clippy's metric (`clippy.toml`: `too-many-lines-threshold = 120`; counts code lines, ignores comments/blank lines/signature) **0 functions** exceed 120 (`cargo clippy --lib -- -W clippy::too_many_lines` → 0 findings). `scripts/fn_len.py` (raw lines, stricter) went from 12 findings to 4: `src/signatures/pdf.rs:23` 136 lines, `src/scan.rs:2514` 134, `src/signatures/jpeg.rs:543` 129, `src/signatures/pdf_validate.rs:138` 125 — informational.
- Remaining work as of `ab725a6` (history — closed on `f5bda4a`; do not redo):
  1. `#[allow(clippy::too_many_arguments)]` count rose from 2 to **13** (`src/scan.rs` 11, `src/fs/mod.rs` 1, `src/cli.rs` 1). The A-10 acceptance says none. Replace them with parameter structs (stage contexts), as a pure refactor proven by byte-identical golden output, still **last** in the round.
  2. Keep clippy as the metric of record (already in `clippy.toml` and CI); keep `fn_len.py` as an informational gate with a note in `docs/DECISIONS.md` that the two counts differ.
- *The Evidence/Where/Fix text below is the `a7a5c08` picture; nothing remains to implement; keep the acceptance command as a regression guard (§8).*
- **Evidence [CONFIRMED-STATIC].** Brace-matching count over `src/**/*.rs` (signature to closing brace) finds **12** functions above 120 lines (v2 N-05 reported 11; the difference is the counting method; `scripts/fn_len.py` of Appendix C.2 is the method of record and reproduces exactly this list): `cli.rs::scan` 273 · `signatures/png.rs::analyze` 246 · `scan.rs::run_scan` 244 · `signatures/jpeg.rs::analyze` 239 · `fs/ntfs.rs::parse` 198 · `fs/fat32.rs::parse` 179 · `signatures/jpeg.rs::strict_baseline_scan` 176 · `scan.rs::carve_one` 155 · `signatures/pdf.rs::analyze` 134 · `scan.rs::postfilter` 131 · `signatures/mod.rs::validate_png` 126 · `scan.rs::recover_fragments` 125. Two `#[allow(clippy::too_many_arguments)]` at `src/scan.rs:932` (`find_candidates`) and `:1542` (`recover_fragments`); neither carries the one-line justification with an item ID that `guide.md` §3.2 requires. `guide.md` A-10 acceptance: no function > 120 lines (tests excepted) and no `allow(too_many_arguments)`.
- **Why last.** Every other item edits these functions (H-01…H-13, H-17 touch `carve_one`, `run_scan`, the three `analyze`s, `validate_*`, `recover_fragments`). Refactoring first would create conflicts and hide behaviour changes.
- **Fix.** Commit `scripts/fn_len.py` (Appendix C.2) in Phase H0 and run it from `scripts/gate.sh` — informational until this item is done, fatal afterwards. Pure behaviour-preserving extraction, one function per commit: stage structs for `run_scan` (`plan_regions → find_candidates → analyze → assemble → filter → write → report`), parameter structs for the two `allow`s, per-format helper modules for the three `analyze`s. No change to public CLI, report or output bytes.
- **Acceptance.** `scripts/fn_len.py` reports 0 functions > 120 lines in `src/`; no `allow(too_many_arguments)`; **G7 golden byte-identical, G8 within 10 %**; all tests green with **no** test edited; `cargo doc -D warnings` clean.
- **Pitfall.** If one commit changes any golden hash or any measured time by > 10 %, revert it — it is not a pure refactor.

### 7.4. Part T — recovery upgrades (new capability, Phase H5)

*Requested by the user on 2026-11-26: which techniques (metadata, fragment, …) can raise CarveX's recovery? These items are **not defects** — the repository's decisions (D-71 "precision over recall", D-68 footer-only repair) stand. They add opt-in capability, in this order: **H-27 → H-28 → H-29, H-30 → H-31, H-32 → H-33**. Start only after H1–H4 are green and H-04 / H-09 are closed (Phase H5, §5).*

**Evidence behind Part T** (Appendix C.4, run on the `f5bda4a` binary; prototypes in Python, **synthetic corpus of 14 JPEG/PNG files** generated by the repository's own corpus script, 4 KiB clusters, seed 2026; every "exact" means SHA-256 equal to the original):

| Prototype | Question | Result (n, Wilson 95 %) |
|---|---|---|
| **E1-live** | deleted file split around a **live** (allocated) island of 1–8 clusters | default 0/40; `--fragment-recovery` 0/40 (upper 0.088); **free-space stream 40/40** (0.912–1) |
| **E1-dead** | same, island = interior of **another deleted** file (free) | default 0/40; `--fragment-recovery` **3/40 = 7.5 %** (0.026–0.199); free-space stream 0/40 |
| **X1** | can one 4 KiB cluster be recognised as JPEG entropy data (every `FF` followed by `00` or `RSTn`, and order-0 entropy ≥ 7.0 bit/byte)? | baseline JPEG clusters 572/572 accepted; progressive 123/141 (the rest hold real markers); PNG 0/2,293; random 0/2,047; zeros 0/255; text PDFs 0; PDFs with `/DCTDecode` 37/38, 87/90, 151/152, 111/115, 42/43 |
| **X2** | PNG chunk CRC-32 as oracle for one gap (break inside a chunk, gap 1…16 clusters) | solver unique **and** correct 200/200 for a random gap and 200/200 for an other-PNG-interior gap (0.981–1; 0.3 s each); CarveX `--fragment-recovery` on the same layouts 0/30 and 0/30 (upper 0.114) |
| single case | `jpeg_01` (194,042 B, 48 clusters) split 20 + 28 around a 4-cluster live file | default and `--fragment-recovery`: 81,920 B `truncated`/`partial`; free-space stream: 194,042 B `complete`/`verified`, SHA equal |

**Reading of the evidence.** B-06 (D-71) recovers a gap of filler or a gap that starts with a header; it does **not** recover the common layouts "a live file sits between the fragments" (E1-live) and "another deleted file's data sits between them" (E1-dead, X2). A live island is solved by *not reading it* (H-27, no oracle needed); a dead island needs a *content oracle* (H-30 for PNG: strong; H-31 PDF: strong, design only; H-32 JPEG: weak, ranking only). Metadata (H-28, H-29) is the other route: it names the exact clusters and size when the directory entry or MFT record survives. Limits of this evidence: Python prototypes, synthetic images, 14 source files, one fragmentation shape (2 fragments, forward order, cluster-aligned break); no real-world volume was used; the numbers justify *trying* the items, not a promise of field recall.

**Rules for every Part T item** (violating any = PR rejected):
1. **Opt-in first.** A new flag (or mode) selects the technique; the default run, golden D0 and the frozen benchmark artefacts stay **byte-identical** (§9 Q5, G7). Whether a technique becomes a default is a separate decision recorded in `DECISIONS.md` after the numbers below exist.
2. **`verified` only with a deep validator** (H-01 model). Recovery by an oracle that can be ambiguous never labels `verified` on a non-unique solution; ambiguous results go to `alternatives[]` with `validity=suspect`.
3. **Independent ground truth.** Acceptance numbers compare SHA-256 against the known source file, or an independent parser (Pillow, `qpdf`, `zipfile`) — never the tool's own validator (ADR-007, guide C-03).
4. **Report with n, Wilson 95 % interval, commit and corpus**; synthetic-corpus results are labelled as such (H-23 wording rule).
5. **Code rules:** panic-free on image data (`deny(unwrap_used, expect_used, indexing_slicing)`), checked arithmetic, bounded by `--max-memory`, `--max-candidates`, `--time-budget`; the image stays read-only; new report fields are **additive** (update `docs/report.schema.json` and the schema test); each new parser gets a fuzz target (H-19).
6. **Provenance.** A recovered file says how it was found: `source` ∈ {existing values, `free-stream`, `metadata`, `bgc` (existing)} and `fragments[]` lists the **physical** runs (offset, length) in image coordinates.

#### H-27 Free-space stream: join the free clusters across allocated islands [P2, COULD — new capability]
- **Status on `f5bda4a`: NEW** (not in the repository; prototype E1 / single case, [REPRODUCED] as prototype, Appendix C.4).
- **Evidence.** E1-live 0/40 (default and `--fragment-recovery`) → 40/40 with the stream; E1-dead 0/40 with the stream (another deleted file's data is in the stream) — so the stream is a *complement* to H-30/H-32, not a substitute. The repository already computes `free_extents` per volume (`FsInfo`, `regions[]` tiles the image), so no new filesystem parsing is needed.
- **Design.** A read-only `ImageSource` adapter in `src/image/` (`FreeStreamSource`): logical stream = the free-cluster extents of **one volume** concatenated in ascending LBA order; the unchanged scan pipeline runs on it; each result's `fragments[]` is translated back to physical runs; `source=free-stream`; one stream per volume (never across volumes or partitions); results identical (same SHA-256) to ones the plain scan already produced are deduplicated. Flag: `--free-stream` (opt-in). FAT32: free = FAT entry 0; NTFS: free = `$Bitmap` clear bits (both already parsed). `regions[]` records the stream as scanned; the 16-byte boundary overlap of the chunker applies to the logical stream.
- **Red tests.** Fixture builder `build_fat32(...)` of Appendix C.4: a deleted JPEG and a deleted PNG, each split around a live file, expect `exact`; **property:** every run in every `fragments[]` lies inside the volume's free extents (allocated bytes are never read into a result); two deleted files adjacent in the free stream never produce a `verified` wrong file (the validator end decides); default scan byte-identical with the flag absent.
- **Acceptance.** On ≥ 40 trials per format family (JPEG, PNG) with live islands: exact ≥ 95 % **and** Wilson lower bound ≥ 0.85; dead-island trials: **0 false `verified`**; golden D0 unchanged; the extra cost (one more pass over free space) measured and recorded (G8, opt-in only).
- **Pitfalls.** Files fragmented backwards (second fragment at a lower LBA) are not recovered by an ascending stream — say so in `--help`; do not claim recovery when a dead island is present; clusters marked bad / reserved are not free; keep `--mode` semantics (`unallocated` must not read allocated clusters — invariant 2 of `ARCHITECTURE.md`).

#### H-28 FAT deleted-directory-entry recovery (B-01, part 1) [P2, COULD — new capability]
- **Status on `f5bda4a`: NEW** (`guide.md` B-01 unimplemented; **[NOT PROTOTYPED]** — the fixture builder already supports deleted entries).
- **Design.** In each FAT directory (root and subdirectories reachable from live entries; deleted directories only to a bounded depth) read 32-byte entries whose first byte is `0xE5` and whose attribute is not LFN/volume label: short name, start cluster (high 16 bits at offset 20 + low 16 bits at offset 26), size (offset 28), times. Reassemble the long name from the preceding LFN entries (attribute `0x0F`, sequence numbers) and **bind it by the 8.3 checksum** stored in the LFN entries; discard a mismatch. The FAT chain of a deleted file is cleared, so assume **contiguous** clusters from the start cluster for `ceil(size / cluster)` clusters and mark `contiguous_assumed=true`. Carve exactly `size` bytes; run the format validator; mark clusters that are now allocated or whose content does not match as overwritten → `partial` with `overwritten_clusters`. Flag `--metadata` (opt-in); `source=metadata`; fields `original_name`, `mtime` (the guide's B-01 fields; null elsewhere). A metadata row whose offset and length equal a carved row **merges** with it (name attached, one file, counted once).
- **Caveats to test, not to assume.** Some implementations clear or reuse the high word of the start cluster on delete: check the signature at the computed cluster and fall back to the plain carve result rather than emitting a guess. Names go into report fields only — never into output file names (naming stays `NNNNN_<ext>_<offset16hex>.<ext>`); sanitise on output (control characters, path separators, `..`, length cap).
- **Red tests.** Fixture: deleted JPEG/PNG/PDF with long names (including a non-ASCII name and a name whose LFN checksum is wrong) → exact SHA, exact size (no over-read), name equal / name dropped on checksum mismatch; a deleted file whose first cluster was reallocated → `partial` + `overwritten_clusters`; a corrupted directory (random bytes) → no panic, bounded work; carved-only run unchanged (golden).
- **Acceptance.** ≥ 95 % exact on contiguous deleted files of the fixture corpus (report n and Wilson); names equal; golden D0 unchanged; a fuzz target for the directory parser.
- **Pitfalls.** A deleted entry proves a *name and an extent*, not that the data is intact — the validator, not the entry, sets `validity`. Do not treat every `0xE5` slot as a file: require a plausible start cluster (inside the data area), a size ≤ volume, and a signature match before emitting.

#### H-29 NTFS deleted-MFT-record recovery (B-01, part 2) [P2, COULD — new capability]
- **Status on `f5bda4a`: NEW** (**[NOT PROTOTYPED]**; `src/fs/ntfs.rs` already reads `$MFT` for `$Bitmap`).
- **Design.** Enumerate `$MFT` records with `FILE` signature and the in-use flag clear; apply the update-sequence fixups (every sector end must equal the update-sequence number, else skip); read `$STANDARD_INFORMATION` / `$FILE_NAME` (name, parent, times) and the unnamed `$DATA`: **resident** → the bytes are inside the record; **non-resident** → decode the runlist (variable-length fields, **signed** relative offsets, sparse runs with offset length 0), `real_size` and `initialized_size`. The runlist is the exact list of fragments → `fragments[]` physical runs; this recovers fragmented deleted files without any oracle when the record survives. Mark runs whose clusters are now set in `$Bitmap` as overwritten → `partial`. Not supported in v1 and reported as such (`unsupported` note): `$ATTRIBUTE_LIST` / extension records, compressed or encrypted `$DATA`. Flag `--metadata` (shared with H-28); `source=metadata`.
- **Bounds.** Record size from the boot sector (1 KiB typical, 4 KiB possible); every attribute length checked against the record; runlist ≤ a documented run count; total clusters ≤ the volume and ≥ `real_size`; panic-free and bounded by `--max-memory`.
- **Red tests.** `mkntfs` is **not available** on the verification machine (6 SKIPs in the repository's repro script): unit tests use a **synthetic MFT-record builder** (round-trip property test for runlist encode/decode including negative offsets and sparse runs; fixup verification; truncated and hostile records) and CLI tests `SKIP` with a clear message when `mkntfs` is missing — never a silent pass. With `mkntfs`: a deleted fragmented JPEG recovered with exact SHA-256.
- **Acceptance.** Runlist round-trip property holds on ≥ 10,000 random runlists; hostile-record fuzz target; on an NTFS fixture a fragmented deleted file is exact; golden D0 unchanged.
- **Pitfalls.** A reused MFT record belongs to another file — check the in-use flag, the sequence number and `$FILE_NAME` parent consistency before trusting it; do not follow runs outside the volume; resident data larger than the record is corruption.

#### H-30 Oracle-guided bifragment join beyond filler and header gaps (PNG chunk CRC-32) [P2, COULD — new capability]
- **Status on `f5bda4a`: NEW** (prototype X2 and E1-dead, [REPRODUCED] as prototype, Appendix C.4).
- **Evidence.** X2: the solver finds the unique correct stitch 200/200 (random gap) and 200/200 (other-PNG-interior gap), Wilson 0.981–1, 0.3 s per case in Python; CarveX `--fragment-recovery` on the same layouts 0/30 and 0/30. **Assumptions of the prototype:** two fragments in forward order; the first fragment's end is cluster-aligned and falls **inside one chunk**; gap 1…16 clusters; 4 KiB clusters; PNG ≥ 12 clusters. Anything outside that is untested.
- **Mechanism.** Every PNG chunk carries a CRC-32 over type + data. Walk the chunks of the head; at the first chunk whose CRC fails (offset *p*, length *L*), for every cluster-aligned break *k* inside it and every gap *g* ≤ `--max-gap`, test the CRC of `head[p:k·C] ‖ disk[(k+g)·C : …]`. A 32-bit check against ≈ 16 × (clusters in the chunk) candidates makes an accidental match negligible. Plain recomputation costs 0.3 s per case; `crc32_combine` (prefix CRCs at cluster boundaries) is an optimisation, not required. Continue with the next failing chunk for more than one gap (bounded count). The stitched file must then pass the **decode** validator (all chunk CRCs, IDAT inflates, IEND) before it is labelled.
- **Design.** A new gap class in the B-06 candidate generator (`--fragment-recovery`, existing flag, existing limits and `FRAGMENT_*` warnings; D-71's supported-class wording in `--help`/README is extended, not removed). **Uniqueness rule:** exactly one (k, g) ⇒ `verified`, `source=bgc`; more than one ⇒ all in `alternatives[]`, `validity=suspect`; none ⇒ unchanged behaviour.
- **Red tests.** The X2 layouts as FAT32 fixtures (random gap, other-file-interior gap, live island) for ≥ 3 PNGs of different colour types; controls: intact PNG (no stitch emitted, output byte-identical), PNG whose second fragment is missing (no stitch, `partial`), a gap that happens to contain a valid-looking chunk header (no wrong `verified`).
- **Acceptance.** ≥ 95 % exact on N ≥ 100 trials **per gap kind** with Wilson lower bound ≥ 0.9; **false `verified` = 0** across all trials including controls; time within `--frag-time-limit`; golden D0 and the S-B06 rows of §8 unchanged (zero gap 3/3, random gap without header 0/3 for JPEG and PDF — PNG is the one format that changes, and the A.3 row is updated in the same PR).
- **Pitfalls.** The oracle is per chunk: a PNG whose break falls exactly on a chunk boundary leaves no failing chunk to key on — fall back to the chunk-header walk (next chunk header must parse, length plausible). Do not let the solver read allocated clusters unless the user chose `--mode raw`.

#### H-31 PDF xref-guided fragment reassembly [P3, COULD — design only]
- **Status on `f5bda4a`: NEW — design only, [NOT PROTOTYPED].**
- **Idea.** A classic xref table lists the byte offset of every object in the *original* file. Under a correct stitch every listed offset after the break holds `N G obj`; under a wrong one it does not. That is many independent checks (far stronger than a hash of one chunk) and the existing `pdf_verify` (H-03) already performs them on a candidate. What is missing is **candidate generation for non-filler gaps**: take the first xref offset whose `obj` header is not found at the expected place in `head ‖ tail(g)` as the break estimate, then search *k*, *g* near it.
- **Limits.** The xref table must be in the surviving tail fragment (typical: it is at the end); xref *streams* are Flate-compressed (decode first); linearized and incrementally updated files have several tables. Deliver as a prototype with ≥ 100 trials per gap kind **before** any production code; acceptance as H-30.

#### H-32 JPEG fragment join: cluster pre-filter, break from the decoder, seam continuity, RST order [P3, COULD — experiment]
- **Status on `f5bda4a`: NEW** (only the pre-filter X1 is reproduced; everything else **[NOT PROTOTYPED]**).
- **Evidence (X1).** The rule "every `FF` is followed by `00` or `D0…D7`, and order-0 entropy ≥ 7.0 bit/byte" accepts 572/572 baseline-JPEG entropy clusters and rejects 2,293/2,293 PNG clusters, 2,047/2,047 random clusters and 255/255 zero clusters; the 18 rejected progressive clusters (of 141) hold real markers (correct rejection). It is a **cheap candidate filter** (cuts the gap search space), not a proof.
- **Design.** (1) filter clusters with X1; (2) take the break point from the strict baseline scanner (the first MCU that cannot be completed, ± one cluster); (3) with a restart interval, `RSTn` numbering must continue across the seam (mod 8); (4) score each candidate (k, g) with the seam-continuity measure the repository already has (`content_continuity_check`, H-09) and rank; (5) accept only if the best candidate beats the second by a margin **and** the stitched file passes the decode validator — otherwise `alternatives[]` (capped) and `suspect`.
- **Honesty constraint.** JPEG has no per-chunk checksum, so there is no oracle like H-30's. H-09's measurement (3 of 4,000 splices accepted, all on gradients) is the warning: continuity is weak on flat content. Report recall **with its confidence interval** and the false-accept rate; **no fixed recall target**; `verified` only when the margin rule holds, else `suspect`.

#### H-33 Coverage: exFAT / FAT12/16 and ZIP/OOXML [P3, COULD — new capability]
- **Status on `f5bda4a`: NEW** (`guide.md` B-03 and B-10 unimplemented; **[NOT PROTOTYPED]**).
- **exFAT (B-03).** Parse the boot region, allocation bitmap (free extents, so H-27 works unchanged) and directory entry sets (`0x85` file, `0xC0` stream extension with start cluster / size / **NoFatChain** flag, `0xC1` name); deleted sets have the in-use bit clear (H-28 logic applies). **FAT12/16:** fixed root directory, 12-bit FAT entries; same carving, `free_extents` from FAT zeros.
- **ZIP / OOXML (B-10).** Local-file-header signature `PK\x03\x04`; end by the End-of-Central-Directory record; validate with each entry's CRC-32 and an independent parser (`zipfile`); OOXML = a ZIP containing `[Content_Types].xml` (classify `docx`/`xlsx`/`pptx` by the content-type parts). A ZIP bomb cap (total uncompressed size, entry count) is mandatory.
- **Acceptance.** Per filesystem a fixture image and a red test that a deleted JPEG is carved exactly; `regions[]` tiles the image; no regression on FAT32/NTFS rows (golden D0).

**Considered and not scheduled.** (a) *Orphan header-less JPEG fragments* — no header means no start offset; only sector-level classification could find them, with no oracle to prove a result. (b) *Machine-learning fragment classifiers* — acceptable only as a ranking feature after an oracle has failed, never as a proof, and never in the default path (determinism, invariant 5 of `ARCHITECTURE.md`).

**Sources for the named techniques** (links from a web search on 2026-11-27; the pages were **not** re-read in full here and none of the measurements above comes from them): [Garfinkel, *Carving contiguous and fragmented files with fast object validation*, DFRWS 2007](https://dfrws.org/presentation/carving-contiguous-and-fragmented-files-with-object-validation/) (bifragment gap carving, object validation); [Pal and Memon, *The evolution of file carving*, IEEE Signal Processing Magazine 26(2), 2009](https://researchr.org/publication/PalM09-1/bibtex) (survey); [Pal, Sencar and Memon, *Detecting file fragmentation point using sequential hypothesis testing*, DFRWS 2008](https://dfrws.org/sites/default/files/session-files/2008_USA_paper-detecting_file_fragmentation_point_using_sequential_hypothesis_testing.pdf) (break-point detection); [W3C, *Portable Network Graphics (PNG) Specification, Third Edition*](https://www.w3.org/TR/png-3/) (chunk layout and CRC-32); [Microsoft FAT32 File System Specification 1.03](https://course.khoury.northeastern.edu/csg256/handouts/fat32_spec_1.03.pdf) (mirror of the specification: directory entries, `0xE5`, long names).

---

## 8. Regression guards (must stay green after every phase)

These measurements were taken on `a7a5c08` (Linux x86-64, 2 cores; observations, not thresholds); the `ab725a6` values are given after an arrow or in the extra rows below and in Appendix A.2; the `f5bda4a` values are in the rows marked so and in Appendix A.3. Re-measure in H0 on the agent's machine and store them in `bench/perf/baseline.json`.

| Guard | Fixture / command | Snapshot observation | Rule |
|---|---|---|---|
| PNG decode bomb stays fixed | 16×16 PNG with 256 MiB of zeros; 100×100 with 512 MiB; declared 32000×32000 (1 GB); 8192×8192 valid (67 MB) — `--validate decode` | all end `partial` with a budget/rows note except the valid 8192² (`decoded`); RSS 4–6 MiB in the dedicated runs; wall ≤ 0.03 s | never inflate beyond `--max-decode-bytes` (256 MiB); `tests/png_decode_budget.rs` stays |
| Header-flood speed (guide S9/S10) | S9-256K / 1M / 4M, S10-flood512-64M, random-64M (default threads) | 0.18 s / 0.72 s / 2.85 s / 1.49 s / 0.03 s; with `--threads 1` 0.39 / 1.49 / 5.44 / 2.68 / 0.04 s; RSS ≤ ≈ 90 MiB | no regression > 10 %; default threads must not be slower than `--threads 1` on ≥ 2 cores (v2 N-03 was **not reproduced**) |
| Header-flood speed on `ab725a6` | same fixtures (Appendix C `flood_fixtures`), median of 3, default threads | S9-256K / 1M / 4M, S10-flood512-64M, random-64M: **0.07 / 0.27 / 1.04 / 0.57 / 0.02 s**, RSS ≤ 44 MiB; `--threads 1`: 0.14 / 0.51 / 2.08 / 1.14 / 0.03 s (`a7a5c08` the same day: 0.18 / 0.61 / 2.32 / 1.23 / 0.02 s) | no regression > 10 % against these numbers |
| Header-flood speed on `f5bda4a` | same fixtures, median of 3, default threads | S9-256K / 1M / 4M, S10-flood512-64M, random-64M: **0.07 / 0.25 / 1.07 / 0.60 / 0.02 s**, RSS 7 / 14 / 44 / 25 / 12 MiB; `--threads 1`: 0.12 / 0.51 / 1.96 / 1.02 / 0.03 s | no regression > 10 % against these numbers |
| Many valid files | 100 distinct ≈ 1 MiB PNGs in a ≈ 100 MiB raw image, default options | 100 files, 0.16 s, exit 0 | default must keep carving all; `--max-memory` behaviour changes only as H-12 specifies |
| Memory budget tests | `tests/memory_budget.rs` (3 on `a7a5c08`, 4 on `ab725a6` and `f5bda4a`) | green | keep; H-12 extends, never loosens |
| A-07 multi-volume default | MBR/GPT with two volumes + gaps, `--mode auto` | scans all volumes and gaps | keep (user-approved default) |
| B-06 synthetic exactness | `tests/fragment_recovery.rs` (S12; 2 tests on `a7a5c08`, 5 on `ab725a6`, 6 on `f5bda4a` incl. `h17_fragment_limits_bind_on_a_hostile_layout`) | green | keep; H-17 adds, never removes |
| Zero-padded truncated files (guide S6) | Appendix A S-TAIL, zero rows: `jpeg_10` cut at 60 % + 48 MiB zeros; `png_02` and `pdf_05` cut at 60 % + 1 MiB zeros | 212,468 / 839,272 / 374,172 B, `truncated`/`partial`, score 60 | identical sizes and SHA-256 after H-25 |
| Test count | `cargo test --all-features --locked` | **80** tests on `a7a5c08`, **135** on `ab725a6`, **148** on `f5bda4a` | only increases (one rewritten test allowed by §3.2.2) |
| Golden D0 | `bench/golden/D0.sha256` (frozen in H0) | n/a until frozen | byte-identical unless the change is listed in CHANGELOG |
| `unsafe` / lints | `#![forbid(unsafe_code)]`; `deny(unwrap_used, expect_used, indexing_slicing)` in `src/fs/`, `src/signatures/` | present | extend to `src/image/` when touched (done on `ab725a6` by repo R2-11 / D-65; new code must obey the same lints) |
| FIXED scenarios of A.2 | every A.2 / A.3 row marked FIXED (S-PDFLEX, S-PDFEOL, S-E01-*, S-VMDK-*, S-DETECT, S-REGIONS, S-STDIN, S-SIGINT, S-JPEGSOF, S-MAXMEM, S-MAXCAND, S-PDFEOF, S-B06 zero-gap row, S-REPAIR PDF row) | the `ab725a6` values of A.2, or the newer `f5bda4a` values of A.3 where a row exists there | re-run in every end-of-phase report (§10); any reverted row is a regression even if unit tests are green |
| Repository repro script | `python3 scripts/review_repro.py --carvex target/release/carvex --assert --xfail scripts/repro_xfail.txt` | 34 scenarios, 0 failures, 6 SKIP, 0 XPASS, exit 0 (same on `f5bda4a`) | stays 0 failures; SKIPs only for missing tools |
| Lint metric | `cargo clippy --lib -- -W clippy::too_many_lines` (`clippy.toml` threshold 120) | 0 findings; `#[allow(clippy::too_many_arguments)]` ×13 on `ab725a6`, **×0 on `f5bda4a`**; `fn_len.py` 3 raw-line findings (informational) | findings 0; allow count stays 0 |
| H-25 truncated ends (`f5bda4a`) | A.3 *S-TAIL*: `jpeg_10`, `png_02`, `pdf_05` cut at 60 % + padding (zeros / `0xFF` / seeded random) | JPEG 212,468 / 212,468 / 212,892 B; PNG 839,272 / 839,272 / 839,609 B; PDF 374,172 / 374,172 / 374,596 B; 48 MiB `0xFF` → 212,468 B; `too_large_truncated` 0 | zero and `0xFF` rows exact, random within +424 B; any tail-trim change re-runs this row |
| Report fields and filters (H-01) | A.3 *S-CONTROL*: 3 carved files, `--verbose`; `--verbose --quiet`; `--verified-only` | 3 `[+]` lines; clap conflict exit 2; default `--verified-only` run writes 0 files with `stats.skipped_verified_only=3`, with `--validate decode` writes 3 `verified`; file keys include `aligned, nested_in, sha256_output, tail_trimmed_bytes, validity_reason`; `tool_info.rustc` present | keep; additive changes only |
| Low-score PNG drop (H-02) | `pngcrc3.img` (3 bad IDAT CRCs), `--validate decode` | file not written, `warnings=['BELOW_MIN_SCORE']`, `stats.below_min_score=1`, exit 0 | a dropped candidate stays reported |
| B-06 gap matrix (H-17) | `scripts/h17_gap_matrix.py` and A.3 *S-B06* | zero gap 3/3 exact, random gap 0/3, gap starting with an embedded header 2/3, 0 spliced stitches | supported gap classes only; a stitch is never `verified` without a decode proof |
| JPEG splice rate (H-09) | `scripts/jpeg_splice_property.py --pairs 4000 --assert-max-rate 0.001` | 3 of 4,000 accepted (Wilson 95 % [0.0003, 0.0022]), exit 0; progressive JPEG in decode mode → `unverified`/`structural` | the rate must not rise |
| Repair labels (H-04) | A.3 *S-REPAIR*: `png_02`, `jpeg_01` cut at 30/60/90 %, `pdf` cut at 70 % | PNG `partial` ×3; JPEG decode `partial` ×3; PDF `repaired/decoded` with both hashes; **JPEG structure `repaired` with grey fill = known defect** | after H-04 item 0: no `repaired` output with invented rows in either mode |

---

## 9. STOP-AND-ASK (do not decide alone; implement the mechanism, keep the old default, ask)

When one of these questions is reached: finish the mechanism, keep the current behaviour as default, write the question and your recommendation into `docs/PROGRESS.md` under "Open questions", continue with the next item, and surface the list in the end-of-phase report.

| # | Question | Blocks | Recommendation |
|---|---|---|---|
| Q1 | Default depth of `--validate` (`structure` today). Should the default become `decode`? | H-01, H-09 | Keep `structure`, but label it "content not checked" (H-01). Measured cost (repo): structure 0.11–0.20 s vs decode 0.49–1.10 s on the benchmark images. |
| Q2 | Default of `--on-read-error` (`abort` today for every read error). | H-07 | `zero-fill` + `UNREADABLE_REGION` warning + exit 3 for evidence images; keep `abort` available. |
| Q3 | Output policy for PNG files with CRC errors: keep emitting with `validity=suspect`, or withhold unless `--keep-suspect`; behaviour of `--complete-only`. | H-02 | Keep emitting (forensic value) as `suspect`; `--complete-only` keeps only `verified`/`structural`. |
| Q4 | B-06 limit warnings (`FRAGMENT_TIME_LIMIT`, `FRAGMENT_CANDIDATE_LIMIT`) → exit 3? | H-17, H-12 | Yes, after H-17 stops wasting the budget on non-fragmented images (§3.3). |
| Q5 | Any change to benchmark metric definitions or thresholds. | H-24, C-xx | Do not change; open a separate task. |
| Q6 | `dist/`: rebuild with matching checksums, or remove from the tree. | H-21 | Remove from the tree; publish release assets with checksums and commit id. |
| Q7 | A new dependency (e.g. `ctrlc`/`signal-hook`) if the std-only SIGINT approach cannot meet H-14. | H-14 | Try a flag set from a small `libc`-free approach first; only then ask. |
| Q8 | Default for VMDK external extents (`--allow-external-extents` off by default). | H-15 | Off (secure default), clear error text naming the flag. |
| Q9 | Phase H0: any Appendix-A scenario whose observed value does not match this document on the agent's build. | whole phase | Stop, record the mismatch, re-derive the affected item from the repository; do not force the document's number. |
| Q10 | H-25: when the true end of a truncated PNG/PDF cannot be located within one 4 KiB cluster (e.g. a PDF stream that is not `FlateDecode`), accept the conservative end (last verified structure boundary) and lose the unverifiable tail? | H-25 | Yes for those cases only; JPEG and Flate-compressed PNG/PDF streams must keep the full prefix. Record the measured numbers in `DECISIONS.md`. |
| Q11 | H-04: scope of `--repair` after the honesty fix — PDF only, or also a validated PNG/JPEG repair. | H-04 | **ANSWERED by the user (2026-11-25): "repair PNG and JPEG too, with verification."** Do not ask again; implement H-04 Remaining work (crop to the recovered rows, `repaired` only with a decode proof). Crop vs. fill is a default recorded in `DECISIONS.md`, not a question. **The repository implemented a narrower reading — see Q12.** |
| Q12 | H-04: the repository reads the answer to Q11 as *footer repair only* (D-68: a cut through the payload stays `partial`); this file reads it as *validated recovery of mid-stream cuts* (crop to the recovered rows). Which is intended? | H-04 items 1–7 | Keep the crop design as the target — a truncated PNG/JPEG is the common case, and footer-only repair covers a rare one. Do **item 0 now** (needed under either reading), implement the crop behind the existing `--repair`, record the decision in `DECISIONS.md`. If the user confirms footer-only: close H-04 after item 0 and document the limit in README/`--help`. |

**Answered on `ab725a6` by the repository** (§1.5): Q1, Q2, Q3 (the silent-drop caveat of H-02 was closed on `f5bda4a` by `BELOW_MIN_SCORE`), Q4, Q6, Q7. **Answered on `f5bda4a` by the repository:** Q10 (D-70: conservative structural end; residue ≤ 424 B measured). **Answered by the user (2026-11-25): Q11** — `--repair` must also repair PNG and JPEG, with verification (H-04); the repository implemented a narrower reading → **Q12 (new, open)**. Q5, Q8, Q9 are still open for the agent to raise when reached.

---

## 10. End-of-phase report and definition of done

**End of each phase** — append to `docs/PROGRESS.md` and show to the user:
1. Table of items done in the phase: `H-xx | commit | red test names | gates | measurement before → after`.
2. Output of `scripts/gate.sh` (G0–G9, `SKIPPED` lines included) and `PENDING-CI` entries.
3. `scripts/repro_xfail.txt` contents (must contain no scenario of a finished phase).
4. Open questions from §9 and deviations logged in `docs/DECISIONS.md`.
5. A sentence per **residual risk** (what was *not* measured).

**Definition of done for the round**
- Every register item H-01…H-26 (and H-27…H-33 once started) is either **done** (acceptance command passes, gates green), **deferred with a recorded reason**, or **blocked on a §9 question**. On `f5bda4a` the 21 FIXED and 2 FIXED (static) items count as done only while their A.2 / A.3 scenario still holds; H-04 is done when its *Remaining work* is (item 0 first; items 1–7 depend on Q12); H-09 is done as documented (the residual is accepted by the repository for 0.3.1 unless the user asks for the Wilson bound); H-24 is NOT RE-RUN; H-27 … H-33 (Part T, Phase H5) are optional capability work and count as done only when their own acceptance passes.
- No scenario of Appendix A (except those marked *by design*) reproduces its "observed" defect.
- No output is labelled `verified` without a deep validator that proved it; the console never uses "complete" as a quality claim; no crafted input below 64 KiB makes the process abort or allocate more than the configured caps; budget excess yields a partial report and exit 3, not exit 1 without a report.
- `docs/REVIEW_RESPONSE.md`, README and PROGRESS contain only claims backed by a command in this file.
- Golden D0 hashes unchanged (or each change explained in CHANGELOG); no performance regression > 10 %; test count ≥ 148 + new tests (80 on `a7a5c08`, 135 on `ab725a6`); the only rewritten existing test is the one allowed by §3.2.2.

---

## Appendix A — Scenario table (fixtures, commands, observed values; "observed" = `a7a5c08`; A.2 for `ab725a6`, A.3 for `f5bda4a`)

**Reference invocation** (raw fixtures): `carvex scan IMG -o OUT --quiet --json-report OUT/r.json --mode raw --no-fs [extra]`; file-system fixtures use the default `--mode auto`. `make_image(payloads)` = 4,096 zero bytes + each payload zero-padded to a multiple of 4,096 + 65,536 zero bytes (Appendix C). Corpus files are under `bench/corpus/{jpeg,png,pdf}/`. "Observed" = snapshot `a7a5c08`, Linux x86-64, 2 cores, release build; **[M]** = measured by the verifier, **[R]** = reported by a review and not re-run. Wall times and RSS are machine-dependent: use them as orders of magnitude and re-measure in Phase H0 (§9 Q9 if a *qualitative* observation differs).

| ID | Fixture | Extra args | Observed on the snapshot | Required after the fix | Item |
|---|---|---|---|---|---|
| **S-PNGCRC** | `png_02_rgb_800x600.png` (24 chunks, 22 IDAT), `make_image`; flip one bit of the CRC field of: first IDAT / first 3 IDAT / all 22 IDAT / IHDR / IEND | `--types png`, `--validate structure` and `decode` | [M] Control: `complete / decoded / 100`. Every corrupted variant: exit 0, `status=complete`; `validation=structure`/`decoded`, **score 85** (one bad CRC) or **55** (3 and 22), note "N chunk CRC error(s)" | `validity=suspect` for every CRC failure; never `verified`/100; file still emitted (§9 Q3) | H-02 |
| **S-PDFLEX** | `pdf_03_text_2pages.pdf` (PDF 1.4, 7 objects) and three variants: first xref entry offset +7; `/Root N` → `99…9` (same length); every xref entry → offset 9. Oracle: `qpdf --check` | `--validate decode --types pdf --min-size 1` | [M] qpdf exit 0 / 3 / 2 / 3; CarveX labels **all four `complete / decoded / 100`** ("xref/startxref target validated") | clean ⇒ `verified` only after the object-graph check passes; the three damaged ⇒ `suspect` (never `verified`) | H-03 |
| **S-PDFEOL** | the same PDF with the final `\n` replaced by `\r\n`, `\r`, nothing, or kept | `--validate decode` | [M] LF and none: `decoded / 100`; **CRLF and CR: `partial / 80`** "selected PDF revision does not end with %%EOF" | all four have the same validity | H-10 |
| **S-PDFEOF** | 32 MiB raw image: one `%PDF-` header followed by E ∈ {1, 256, 1024, 4096} `%%EOF` markers (an object + filler between them; v1 R-06 construction) | `--types pdf --max-size 64M` | [M] 0.16 s / 0.77 s / 2.42 s / **9.39 s**; v5 [R] 0.32 / 4.16 / 15.4 s for 1 / 1,024 / 4,096 markers (`guide.md` A-06 asks ≤ 1 s for 4,096 in 64 MiB) | linear-ish: E=4096 ≤ 2 s *(proposed; record measured value)* and no super-linear growth between E=1024 and 4096 | H-11 |
| **S-REPAIR** | `pdf_03` cut at 70 %, `png_02` at 60 %, `jpeg_01` at 60 %, `make_image` | `--repair --validate decode --min-size 1` | [M] all three: `status=repaired`, `validation=partial`, score 60; truncated-PDF output fails `pypdf` ("startxref not found") and `qpdf --check` (exit 2); report has no hash of the original | an unreadable PDF is **not** `repaired` (stays `partial` with note); `sha256_original` and `sha256_output` recorded for every repaired file | H-04 |
| **S-TAIL** | `tail_{jpeg,png,pdf}_{zeros,ff,random}.img` (`tail_fixtures`; 1 MiB padding; `--big` adds the 4 MiB / 48 MiB JPEG rows): `jpeg_10_progressive_1200x800.jpg`, `png_02_rgb_800x600.png`, `pdf_05_large_scan.pdf` cut at 60 % + padding | `scan IMG -o OUT --quiet --json-report R --mode raw --no-fs --types <t>` | [M] zeros: 212,468 / 839,272 / 374,172 B (the cut sizes). random: JPEG **0 files, `rejected ≥ 1`**; PNG 852,157 B; PDF 1,422,748 B. `0xFF`: JPEG 1,261,044 B; PNG 852,157 B; PDF 1,422,748 B. large JPEG rows: zeros 48 MiB 212,468 B; random 48 MiB 0 files; `0xFF` 4 MiB 4,406,772 B; `0xFF` 48 MiB 0 files, `too_large_truncated = 1`, `warnings = []`, exit 0 | every output: `size ≤ cut + 4,096`, prefix-identical outside the last 4,096 B; JPEG and Flate PNG/PDF also `size ≥ cut − 4,096`; zero rows unchanged | H-25 |
| **S-TAIL-NEIGH** | `tail_jpeg_neigh_0.img`, `tail_jpeg_neigh_4096.img`: truncated `jpeg_10` (60 %) + 0 / 4,096 zero bytes + complete `jpeg_03_progressive_800x600.jpg` (234,049 B) | `--mode raw --no-fs --types jpeg` | [M] `carved = 1` (the complete file at offset 216,564 / 220,660), `rejected = 1`: the truncated JPEG is lost | both files carved; truncated one `partial` within the S-TAIL bounds | H-25 |
| **S-GATE** | clean checkout without `bench/images/*.img`, unprivileged | `python3 scripts/review_repro.py --carvex target/release/carvex --workdir W --assert --xfail scripts/repro_xfail.txt` | [M] R7, R8 `FAIL … SKIP`, "10 scenario(s), 2 failure(s)", **exit 1** | exit 0, `2 skipped`; `--require-all` ⇒ exit 1 | H-26 |
| **S-JPEGSPLICE** | A = `jpeg_01` (640×480), B = `jpeg_02_large_1280x960`; image `[A1][B1][A2][B2]` with halves cut at 4 KiB-aligned midpoints, each padded to 4 KiB; first output at offset 98,304. **Sweep (v1 R-05 construction):** two Pillow-generated 640×480 JPEGs per class (noise / photo-like × baseline / progressive); `B[:cut] + A[cut:]` for 512-byte-aligned cuts | `--types jpeg`, default and `--validate decode`; sweep with `decode` | [M] default: one 394,746-byte output, **`complete / structure / 100`**; decode: `partial / 80`; console shows a COMPLETE column and no validation level; `--verbose` prints each `[+]` line twice. Sweep, outputs accepted as `decoded`: noise baseline 8/91, noise progressive 4/90, photo-like baseline 7/79, photo-like progressive 11/76 | default ⇒ `structural` with "content not checked" (Q1) and never counted as a quality "complete"; a splice that a full decode or Pillow `load()` rejects ⇒ `suspect`; no `verified` without the deep validator | H-01, H-09 |
| **S-JPEGSOF** | baseline `jpeg_01` with SOF0 height/width patched to 20000×20000 and to 65535×65535 | `--validate decode`; run under `ulimit -v` ≈ 3.5 GiB | [M] 20000²: 3.2 s wall, **1.7 GiB peak RSS**, `partial`; 65535²: allocation failure ⇒ **SIGABRT, no report**; `structure` mode unaffected (0.02 s) | both: report present, exit 0 or 3, peak RSS < 300 MiB, note "exceeds the decode-validation budget"; valid 8000×6000 JPEG still decodes | H-06 |
| **S-MAXMEM** | 100 distinct ≈ 1 MiB PNGs in a ≈ 100 MiB raw image | default; `--max-memory 64M`; `16M`; `200M` | [M] default 100 files, 0.16 s; `64M` and `16M`: **exit 1, no report, 0 files** ("retaining a carved file would exceed --max-memory"); `200M`: 100 files | `64M`/`16M`: exit 3 + report + `MEMORY_BUDGET_EXCEEDED`; identical output for `--threads 1/2/8` | H-12 |
| **S-MAXCAND** | tiny image with a few headers | `--max-candidates 5` | [M] exit 1, "found more than --max-candidates 5", no report | exit 3 + partial report + `CANDIDATE_LIMIT` | H-12 |
| **S-TIMEBUD** | PDF `%%EOF` flood (1,024 markers in 16 MiB), single batch | `--time-budget 1` | [M] batch took 1.24 s, **exit 0** (budget checked only between batches) | exit 3 + report + `TIME_BUDGET_EXCEEDED` within ≈ budget + one candidate | H-12 |
| **S-PNGBOMB** | 16×16 PNG with 256 MiB of zeros (valid CRCs); 100×100 with 512 MiB; declared 32000×32000 (1 GB); valid 8192×8192 | `--validate decode` | [M] **fixed on the snapshot**: budget guard ⇒ `partial` + note ("exceeds the decode-validation budget of 268435456 bytes" or "more rows than IHDR declares"); wall ≤ 0.02 s, peak RSS 4–6 MiB (VmHWM); valid 8192² ⇒ `decoded`. (v5 [R] measured 324 MiB on an earlier build) | unchanged (guard, §8) | guard |
| **S-B06** | 64 MiB FAT32 (`fat32(131072, 8)`: 4 KiB clusters), nothing allocated; for each of `jpeg_01` (194,042 B), `png_02` (1,398,788 B), `pdf_05_large_scan` write the first half (cluster-aligned midpoint) at cluster `cl`, then a **3-cluster gap** (zeros / random / 12 KiB of `jpeg_04`), then the second half; `cl` starts at 200 and advances by (span in clusters + 60) | `--validate structure` and `decode`, each with and without `--fragment-recovery`, default mode | [M] defaults: **0/3 exact in all 12 runs**; recovery on ⇒ ≈ 5.1 s, `FRAGMENT_TIME_LIMIT`/`FRAGMENT_CANDIDATE_LIMIT`, exit 0. Raised limits (`--max-frag-candidates 100000 --frag-time-limit 60000`, decode): random gap ⇒ JPEG+PNG exact in 3.0 s, **PDF (623,620 B) mis-stitched yet `complete/decoded`**; other-file gap ⇒ 2/3 in 12.1 s; zero gap ⇒ 0/3 after 60 s | see H-17 red tests: byte-exact at defaults *(target)*, no false `verified`, wall ≤ 5 s | H-17 |
| **S-REGIONS** | MBR disk: FAT32 volumes of 32 MiB and 16 MiB with 1 MiB and 2 MiB gaps (53,477,376 B) | default | [M] each volume appears twice in `regions[]` (`free` scanned=true, `allocated` scanned=false, same offset/length); Σ scanned = 53,477,376 vs `bytes_scanned` = 53,067,776. Corrupted-NTFS volume in a multi-volume disk: exit 0, `warnings[]` empty ([R] v5; code path confirmed statically) | invariant Σ scanned = `bytes_scanned`; `VOLUME_PARSE_FAILED` + exit 3 | H-18 |
| **S-E01-GOOD** (control) | `build_e01`: 8 chunks × 32 KiB, alternating compressed/uncompressed, `jpeg_01` at offset 32,768 | `--mode raw --no-fs` | [M] exit 0, 1 JPEG `complete/decoded` | unchanged | H-07 |
| **S-E01-SHORT** | media = 7.5 chunks (last chunk stored short), compressed **and** uncompressed variants | same | [M] **exit 1, no report**: "cannot decompress EWF chunk at byte 194114" / "EWF uncompressed chunk at byte 229663 is truncated" | exit ≤ 3, JPEG recovered, tail readable | H-07 |
| **S-E01-ADLER** | uncompressed chunk 1, or chunk 7 (after the JPEG), with a wrong Adler-32 | same | [M] **exit 1, no report** ("checksum mismatch in EWF chunk at byte 33031 / 229663") | `UNREADABLE_REGION` + exit 3; JPEG recovered | H-07 |
| **S-E01-HUGE** | E01 declaring sectors-per-chunk × bytes-per-sector = 2²⁰ × 2²⁰ (1 TiB) with one tiny compressed chunk (`build_e01(..., declared_spc, declared_bps)`); also 2¹⁶ × 2¹⁶ (4 GiB) and 2¹² × 2¹² | same | [M] 1 TiB: `memory allocation of 1099511627776 bytes failed`, **SIGABRT (exit 134)**; 4 GiB and 16 MiB: ordinary error exit 1 | all: ordinary error exit 1 naming the limit; no abort | H-05 |
| **S-VMDK-ALIAS** | monolithicSparse with `num_gte` = 65,536 and a grain directory of 1,024 / 4,096 entries **all pointing at the same table** (0.26 / 0.27 MiB file) | `info` | [M] peak RSS 260 MiB (×1,008) / **1,028 MiB (×3,814)** | refuse or de-duplicate; RSS bounded (≤ 64 MiB table cache) | H-05 |
| **S-VMDK-ZERO** | descriptor `RW 1099511627776 ZERO` + `RW 8 FLAT "flat.bin" 0` (512 TiB logical) | `--mode raw --no-fs`, `ulimit -v 4000000` | [M] `memory allocation of 5368709120 bytes failed` in `build_tasks`, **SIGABRT (exit 134)** | planning is O(extents): scan starts (or fails with an ordinary error before allocating) | H-05 |
| **S-VMDK-THIN** | monolithicSparse 2 MiB, 64 KiB grains, 4 GTEs per table ⇒ 8 grain-directory entries; entry 0 allocated (holds `jpeg_01`), entries 1–7 = 0 | `--mode raw --no-fs` | [M] **exit 1** "VMDK grain index out of range"; same image with entries 1–7 pointing at zero tables: exit 0, JPEG recovered (control `vm_full`) | thin image scans, output byte-identical to the control | H-08 |
| **S-VMDK-PATH** | descriptor with `RW 1 FLAT "/abs/…/secret.bin" 0`; another with `"../../secret.bin"`; 512-byte secret outside the evidence directory | `scan d.vmdk --mode raw --no-fs` | [M] both exit 0 with `input.size = 512` (the outside file was read) | error naming `--allow-external-extents`; no read outside the subtree | H-15 |
| **S-VMDK-RENAMED** | self-contained monolithicSparse (embedded descriptor naming `original_name.vmdk`) copied to `renamed.vmdk` | `info` | [M] fails: "cannot open …/original_name.vmdk" | opens `renamed.vmdk` itself | H-15 |
| **S-DETECT** | three 1 MiB raw images: (a) `# Extent description` at offset 4,096; (b) `conectix` at the start of the last 512 bytes; (c) first 4 bytes `KDMV`; each with a JPEG planted at a known offset for the fixed behaviour | `scan IMG` (auto) vs `--input-format raw` | [M] auto: **exit 1** ("VMDK descriptor has no extents" / "VHD footer checksum mismatch" / "VMDK sparse extent has no embedded descriptor"); raw: exit 0 | auto: raw fallback + `CONTAINER_PROBE_FAILED`, exit 0 | H-16 |
| **S-STDIN** | `make_image([jpeg_01])` (266,240 B) piped as `scan -` | `--mode raw --no-fs` | [M] exit 0, 1 file; `input.modified_unix = null`, **`unchanged = true`**, spool uncapped | `unchanged = null`; `--max-spool` honoured | H-20 |
| **S-SIGINT** | S-PDFEOF flood (32 MiB) | SIGINT after 2 s | [M] process killed by the signal (exit −2), no report | exit 4, schema-valid partial report, `INTERRUPTED` | H-14 |
| **S-FLOOD** | guide S9/S10: JFIF-like header + 'A' repeated to 256 KiB / 1 MiB / 4 MiB; 64 MiB with a header every 512 B; 64 MiB random | default and `--threads 1` | [M] see §8 (0.18 / 0.72 / 2.85 / 1.49 / 0.03 s default) | no regression > 10 % | guard |
| **S-DIST** | `dist/carvex-linux-x86_64` vs `target/release/carvex` | `--version`, `scan --help` | [M] both `carvex 0.2.0`; `dist` lacks 8 flags (`--fragment-recovery --frag-time-limit --input-format --max-candidates --max-decode-bytes --max-frag-candidates --max-gap --time-budget`); `tool_info.commit = "unknown"` | Q6; commit id non-empty | H-21 |

### A.2 Re-measurement on `ab725a6` (2026-11-25)

Both binaries were run on the same fixtures (Appendix C builders; Appendix A commands). `new` = `ab725a6` release build, `old` = `a7a5c08` release build. All rows [REPRODUCED] unless stated. Oracles: `qpdf --check`, `pypdf`, SHA-256 of the corpus files. Times are wall clock on the 2-core verification machine.

| Scenario | Observed on `ab725a6` (`a7a5c08` in parentheses) | Status | Item |
|---|---|---|---|
| S-CONTROL (jpeg_01 + png_02 + pdf_03, raw) | 3 files, all `structural` in the default run; file keys now include `checks[]`, `validity`; top level adds `internal_errors`, `interrupted`, `nondeterministic_stop`, `unreadable_regions`; `tool_info.commit = ab725a6674b6`; console table with `VERIFIED/STRUCTURAL/SUSPECT/PARTIAL/REPAIRED` + legend; **`--verbose` prints each `[+]` line twice** (6 lines for 3 files), also with `--quiet`; no `--verified-only`; absent: `validity_reason`, `tail_trimmed_bytes`, `aligned`, `nested_in`, `sha256_output`, `tool_info.rustc` | PARTLY | H-01 |
| S-PNGCRC | clean: structure `structural/100`, decode `verified/100`. 1 bad IDAT CRC: structure `suspect/85`, decode `suspect/65`. 3 bad: structure `suspect/55`, decode **file not written, no warning** (`stats.below_min_score=1`, score 35 < 50). 22 bad: same as 3. IHDR/IEND CRC: `suspect/85` / `65` (all rows `complete` with score 100, 85 or 55, no validity) | PARTLY | H-02 |
| S-PDFLEX | `qpdf` exit 0 / 3 / 2 / 3 ↔ `verified` / `suspect/80` / `suspect/80` / `suspect/80` for clean / first xref entry +7 / `/Root 99…9` / every xref offset = 9 (all four `decoded/100`) | FIXED | H-03 |
| S-PDFEOL | LF 6,211 B, CRLF 6,212 B, CR 6,211 B, none 6,210 B: all `decoded/verified/100` (CRLF, CR: `partial/80`) | FIXED | H-10 |
| S-REPAIR | PDF cut at 70 %: `repaired/decoded`, `validity=repaired`, `sha256_original` present, `qpdf --check` exit 3, pypdf 2 pages (exit 2, "startxref not found", no `sha256_original`). **PNG** cut at 30/60/90 % (both `--validate` modes): `truncated`/`partial`, `repaired=false`, "repair attempted, not validated … parsed only 786609/839284 byte(s)", Pillow "Truncated File Read" (0/3). **JPEG** cut at 30/60/90 %: `--validate decode` ⇒ `truncated`/`partial`, `repaired=false` ("revalidated as Partial"); **default structure mode ⇒ `repaired`/`repaired`, `repaired=true`, Pillow OK 640×480 but at 60 % rows ≥ 271 are flat grey (128,128,128) — ~43 % invented, no recovered-fraction field** (`repaired`, `partial`). Reference algorithm `scripts/repair_ref.py` (C.3): PNG 9/10 and JPEG 8/10 corpus files repaired at 30/60/90 %, Pillow OK, top rows identical to the original | PARTLY (Q11 answered: implement validated PNG/JPEG repair) | H-04 |
| S-JPEGSPLICE | basic pair and four extra pairs (progressive+progressive, gray+baseline, CMYK+baseline, progressive+baseline): structure `structural/100`, decode `partial/suspect 80` — same decisions as `a7a5c08`; escape rate on the evaluation corpus [NOT RE-RUN] | PARTLY | H-09 |
| S-JPEGSOF | 20,000²: 0.02 s / 0 MiB, `unverified`, `structural` (4.25 s / 1,723 MiB); 65,535²: 0.01 s / 0 MiB, same (SIGABRT, exit −6) | FIXED | H-06 |
| S-PDFEOF (32 MiB, E markers) | E=1 0.68 s (0.11); E=256 0.18 s (1.54); E=1024 0.18 s (4.30); E=4096 **0.27 s** (18.85); RSS 44–48 MiB (76) | FIXED | H-11 |
| S-MAXMEM (100 × ≈ 1 MiB PNG) | `--max-memory 16M`/`64M`: exit 0, 100/100 files, RSS 12–16 MiB, no warning (exit 1, "retaining a carved file would exceed --max-memory", no report); default 0.5 s / 16 MiB (0.27 s / 105 MiB) | FIXED | H-12 |
| S-MAXCAND (`--max-candidates 5`) | exit 3, `CANDIDATE_LIMIT`, partial output + report (exit 1, no report) | FIXED | H-12 |
| S-TIMEBUD (PDF flood 1024 in 16 MiB) | 0.16 s (2.67 s) — no longer exercises the budget; see S-VMDK-ZERO | FIXED | H-12 |
| S-PNGBOMB (16×16 with 256 MiB zeros; 100×100 with 512 MiB) | `partial`/`suspect 80`, 0.01 s, ≈ 0 MiB (`partial 80`) — unchanged guard | FIXED | §2, §8 |
| S-E01-GOOD / -SHORT / -ADLER | good: exit 0, 1 JPEG; short chunk (compressed, raw): exit 0, 1 JPEG (exit 1, no report); Adler chunk 1: exit 3, `UNREADABLE_REGION`, 0 files; chunk 7: exit 3, `UNREADABLE_REGION`, JPEG carved (exit 1, no report) | FIXED | H-07 |
| S-E01-HUGE | 1 MiB × 1 MiB header: exit 1 "implausible EWF bytes per sector 1048576" (SIGABRT); 65,536: exit 1 "EWF chunk size 4294967296 exceeds the 67108864-byte limit"; 4,096: exit 0, 0 files | FIXED | H-05 |
| S-VMDK-THIN / -ALIAS | thin: exit 0, JPEG carved (exit 1 "grain index out of range"); alias tables via `info`: 0.01 s / 0 MiB for both (0.27 s / 260 MiB; 1.09 s / 1,028 MiB) | FIXED | H-08, H-05 |
| S-VMDK-ZERO (`z512/d.vmdk`, `ulimit -v 4000000`) | 20 MiB RSS, no abort; default run > 120 s (stopped); `--time-budget 5`: exit 3, 5.01 s, `TIME_LIMIT`, report, `bytes_scanned = 562,949,953,425,408` (SIGABRT after 2.2 s, 2.6 GB RSS) | FIXED | H-05 |
| S-VMDK-PATH / -RENAMED | absolute and `..` extent: exit 1 typed error; `--allow-external-extents`: accepted; renamed sparse extent: scan exit 0, `input.format=vmdk`, 1 JPEG (both accepted silently, exit 0, no flag; renamed: exit 1 "cannot open …/original_name.vmdk") | FIXED | H-15 |
| S-DETECT (JPEG planted at 65,536 in each) | text `# Extent description` and bad-checksum `conectix`: auto → `input.format=raw`, exit 0, JPEG carved (exit 1); bare `KDMV`: exit 1 "VMDK sparse extent has no embedded descriptor" (same) — `--input-format raw` carves it | FIXED | H-16 |
| S-REGIONS | Σ scanned region lengths = `bytes_scanned` = 53,067,776, disjoint and ordered (Σ = 53,477,376 ≠ 53,067,776, overlapping); NTFS-garbage 2nd volume: exit 3, `VOLUME_PARSE_FAILED` (exit 0, no warning) | FIXED | H-18 |
| S-STDIN | `input.format="stdin"`, `unchanged=null` (`true`); `--max-stdin-bytes 100000` → exit 1 "stdin image exceeds --max-stdin-bytes" (flag absent) | FIXED | H-20 |
| S-SIGINT | 512 MiB random raw scan, SIGINT after 0.15 s: exit 4, report written, `interrupted=true` (killed, exit −2, no report) | FIXED | H-14 |
| S-B06 | zeros gap, `--fragment-recovery`: **3/3 exact**, `verified`, `source=bgc`, 1.25 s, no warning (0/3, 5.1 s, `FRAGMENT_TIME_LIMIT`). Random gap: 0/3, 0.26–0.31 s, 2 files (png `partial` 721,061 B; pdf `complete`/`suspect` 635,908 B = file + 12,288 B gap), no warning (0/3; raised limits 2/3 exact + false PDF stitch, 3.3 s). Other-file gap: 0/3, 0.23–0.28 s (raised limits 2/3, 10.5 s). Without `--fragment-recovery` all gaps: 0/3 as before | PARTLY | H-17 |
| S-TAIL | zeros rows exact (212,468 / 839,272 / 374,172 B, `truncated`, now `validity=partial`) — `0xFF` rows JPEG 1,261,044, PNG 852,157, PDF 1,422,748 B (≈ +1 MiB swallowed); random rows PNG 852,157, PDF 1,422,748 B, **JPEG lost** (`rejected=1`); big rows: zeros-48M 212,468; random-48M no file (`rejected=3`); ff-4M 4,406,772; ff-48M no file (`too_large_truncated=1`) — identical to `a7a5c08` | OPEN | H-25 |
| S-TAIL-NEIGH | only the complete JPEG (234,049 B) is carved, `rejected=1`, gap 0 and 4,096 — identical (now `structural`) | OPEN | H-25 |
| S-FLOOD (median of 3, default threads; `--threads 1`) | S9-256K 0.07 s (0.18); S9-1M 0.27 (0.61); S9-4M 1.04 (2.32), RSS 44 MiB (89); S10-flood512-64M 0.57 (1.23); random-64M 0.02 (0.02). `--threads 1`: 0.14 / 0.51 / 2.08 / 1.14 / 0.03 s (0.29 / 1.16 / 4.53 / 2.66 / 0.03) | FIXED (guard) | §8 |
| S-GATE | `scripts/review_repro.py --assert --xfail scripts/repro_xfail.txt`: 34 scenarios, 0 failures, 6 SKIP, 0 XPASS, exit 0 (log: `review_repro_ab725a6.log` of the verification) | FIXED | H-26 |
| S-DIST | `dist/carvex-linux-x86_64` = `carvex 0.3.1`, `commit=be4217f7e2b4`, 49 `scan` flags = the source build; `scripts/check_dist.sh` → DIST OK (Windows binary skipped, wine missing) | FIXED | H-21 |
| S-QUALITY | `cargo test` 135 passed / 0 failed; `cargo clippy --lib -- -W clippy::too_many_lines` 0 findings; `scripts/fn_len.py` 4 functions (136 / 134 / 129 / 125 raw lines: `pdf.rs:23`, `scan.rs:2514`, `jpeg.rs:543`, `pdf_validate.rs:138`) (12); `allow(clippy::too_many_arguments)` 13 (2); `check_status.py` and G9 render check green | PARTLY | H-22, H-23 |

Not re-run in A.2: fuzz targets, coverage ratchet, `scripts/golden.py`, `scripts/perf_gate.py`, CI, Windows, benchmarks, the 5–15 % JPEG-splice escape rate (H-09), real libewf/VMware/Hyper-V images.

### A.3 Re-measurement on `f5bda4a` (2026-11-26)

The `f5bda4a` release build (rustc 1.97.0, `cargo build --release --locked`) was run on the same fixtures as A.2 (Appendix C builders, scenario runner `scen/`), `ONLY_NEW=1`. All rows [REPRODUCED] unless stated; oracles as in A.2 (`qpdf --check`, `pypdf`, Pillow, SHA-256 of the corpus files). Values in parentheses are the `ab725a6` values of A.2. Scenarios that are not listed here were re-run and gave the A.2 values unchanged (S-PDFLEX, S-PDFEOL, S-JPEGSOF, S-PDFEOF, S-MAXMEM, S-MAXCAND, S-PNGBOMB, S-E01-*, S-VMDK-*, S-DETECT, S-REGIONS, S-STDIN: outputs of the two builds compared with `diff`, no difference).

| Scenario | Observed on `f5bda4a` (`ab725a6` in parentheses) | Status | Item |
|---|---|---|---|
| S-CONTROL (jpeg_01 + png_02 + pdf_03, raw) | `--verbose`: 3 `[+]` lines (6); `--verbose --quiet`: clap conflict, exit 2 (accepted); default run with `--verified-only`: 0 files written, `stats.skipped_verified_only=3`; with `--validate decode` and `--verified-only`: 3 `verified` files; file keys now also `aligned`, `nested_in`, `sha256_output`, `tail_trimmed_bytes`, `validity_reason`; `tool_info` = `{commit:"f5bda4a1b271", name, rustc:"rustc 1.97.0 (2d8144b78 2026-07-07)", version:"0.3.1"}`; `scan` has 50 flags (49) | FIXED | H-01 |
| S-PNGCRC3 (`pngcrc3.img`, 3 bad IDAT CRCs, `--validate decode`) | file not written (score 35 < 50) **and reported**: `warnings=['BELOW_MIN_SCORE']`, `stats.below_min_score=1`, exit 0 (no warning) | FIXED | H-02 |
| S-REPAIR | PDF cut at 70 %: `repaired`/`decoded`, `sha256_original` and `sha256_output` present. **PNG** (`png_02`) cut at 30/60/90 %, both modes: `truncated`/`partial`, `repaired=false`, 419,636 / 839,272 / 1,258,909 B, Pillow "Truncated File Read". **JPEG** (`jpeg_01`) cut at 30/60/90 %, `--validate decode`: `truncated`/`partial`, 58,212 / 116,425 / 174,637 B, "revalidated as Partial". **JPEG default `--validate structure`: `repaired`/`repaired`, `repaired=true`**, 58,214 / 116,427 / 174,639 B, "repair appended EOI and revalidated", Pillow OK 640×480, **at 60 % first differing row 271, rows 400–479 one colour (128,128,128)** — unchanged from A.2 | PARTLY | H-04 |
| S-H09-PROG (progressive JPEG, `--validate decode`) | fall-through closed: `unverified`/`structural`, note "JPEG strict scan unsupported for this variant; content not checked" (`decoded` on `a7a5c08`) | PARTLY | H-09 |
| S-H09-EVAL (`scripts/jpeg_splice_property.py --pairs 4000 --assert-max-rate 0.001`) | 3 accepted of 4,000 = 0.0008, Wilson 95 % [0.0003, 0.0022]; gradient 3/1,323, noise 0/1,336, photo 0/1,341; exit 0 (A.2: [NOT RE-RUN]; `a7a5c08`: 5–15 %). The repository's stricter bound (Wilson upper ≤ 0.1 %) is not met → repo H-18 PARTIAL | PARTLY | H-09 |
| S-JPEGSPLICE (basic pair) | decode mode `partial`/`suspect 80`; the cut head is now **also** emitted: 94,208 B `truncated`/`partial` + 394,746 B (`structural` in structure mode, `suspect 80` in decode mode); `carved` 2 (1) | PARTLY | H-09 |
| S-B06 | zeros gap: **3/3 exact**, `verified`, `source=bgc`, 1.27 s, no warning. Random gap: 0/3. Gap of another file's *interior* bytes (my fixture): 0/3. `scripts/h17_gap_matrix.py` (gap = repeated first cluster of `jpeg_04`, starts with `FF D8`): 2/3 (JPEG, PNG), 0 spliced stitches. Raised limits: random → JPEG 94,260 B `partial`, PNG 698,593 B `partial`, PDF 635,908 B `suspect`; other-file → JPEG 206,330 B `suspect`, PNG 705,505 B `partial`, PDF 635,908 B `suspect`; none `verified` | FIXED (by decision D-71) | H-17 |
| S-TAIL | cut → zeros / `0xFF` / random: JPEG 212,468 → 212,468 / 212,468 / 212,892 B; PNG 839,272 → 839,272 / 839,272 / 839,609 B; PDF 374,172 → 374,172 / 374,172 / 374,596 B. 48 MiB JPEG: zeros 212,468; random 212,892 (`rejected=2`, file emitted); `0xFF` 4 MiB and 48 MiB 212,468; `too_large_truncated` 0 (random rows were +12,885 B / +1 MiB / **lost**; `0xFF` rows +1 MiB; 48 MiB `0xFF` lost) | FIXED | H-25 |
| S-TAIL-NEIGH (gap 0 and 4,096 B) | at 4,096 B: truncated JPEG 212,468 B `truncated`/`partial` **plus** complete 234,049 B `structural`; `carved=2 rejected=0` (only the complete JPEG, `rejected=1`) | FIXED | H-25 |
| S-FLOOD (median of 3, default threads; `--threads 1`) | S9-256K 0.07 s / 7 MiB; S9-1M 0.25 s / 14; S9-4M 1.07 s / 44; S10-flood512-64M 0.60 s / 25; random-64M 0.02 s / 12. `--threads 1`: 0.12 / 0.51 / 1.96 / 1.02 / 0.03 s (0.07 / 0.27 / 1.04 / 0.57 / 0.02; 0.14 / 0.51 / 2.08 / 1.14 / 0.03) | FIXED (guard) | §8 |
| S-SIGINT | exit 4, report written, `interrupted=true` (unchanged) | FIXED | H-14 |
| S-GATE | `scripts/review_repro.py --assert --xfail scripts/repro_xfail.txt`: 34 scenarios, 0 failures, 6 SKIP (X17, X19, X20 need `qemu-img` / `mkfs`; the others missing tools), 0 XPASS; X09, X11, X12, X14, X15, X21, X22 PASS | FIXED | H-26 |
| S-QUALITY | `cargo test --all-features --locked` **148 passed / 0 failed** (README 148); `cargo clippy --all-targets --all-features --locked -- -D warnings` clean; `clippy --lib -W clippy::too_many_lines` 0; `allow(clippy::too_many_arguments)` 0 (13); `fn_len.py` 3 findings (129 / 125 / 124 raw lines: `jpeg.rs:703`, `pdf_validate.rs:138`, `jpeg.rs:264`) (4); `check_status.py` STATUS GREEN (38 guide IDs, 34 harness IDs) | FIXED | H-22, H-23 |
| S-DIST | `dist/carvex-linux-x86_64` = `carvex 0.3.1`, commit `43dcf59b…`, 50 `scan` flags; **`src/`, `Cargo.toml` and `Cargo.lock` are identical between `43dcf59` and `f5bda4a`** (`git diff --stat`: empty), so the binary corresponds to the source; `scripts/check_dist.sh` and the Windows binary [NOT RE-RUN] | FIXED (static) | H-21 |

Not re-run in A.3: fuzz targets, coverage ratchet, `scripts/golden.py`, `scripts/perf_gate.py` (the repository reports golden and performance green, `rand64m` 1.051×), CI run `45749230527` (recorded green by the repository), G9 locally, Windows, benchmarks, real libewf/VMware/Hyper-V images; S-REPAIR on additional corpus files beyond `png_02` / `jpeg_01` / one PDF.

---

## Appendix B — Cross-reference: every review ID → harness item

Verdict key: **C** confirmed on the snapshot (REPRODUCED or CONFIRMED-STATIC by the verifier); **F** already fixed on the snapshot (kept as a regression guard); **P** partly confirmed / partly outdated; **N** not reproduced; **U** not re-run (reported only). The review file names are `review-v1…v5` as attached by the user; IDs follow each file's own scheme (v1 and v3 `R-`; v2 `R-`/`N-`; v4 and v5 `F-`).

### review-v1 (commit `706a12a`; R-01…R-21)
| ID | Title (short) | → | Verdict / note |
|---|---|---|---|
| R-01 | `--max-memory` is a post-hoc check | H-12 | **P**: in-flight on the snapshot but an excess is fatal (exit 1, no report) |
| R-02 | PNG decode inflates without bound | guard §8 | **F**: budget guard present (`tests/png_decode_budget.rs`) |
| R-03 | A bad PNG CRC is still `complete` | H-02 | **C** |
| R-04 | PDF `decoded` is a lexical check | H-03 | **C** |
| R-05 | JPEG `decoded` ≠ entropy-data check | H-09 | **C** |
| R-06 | PDF `%%EOF` evaluation is O(E×N) | H-11 | **C** |
| R-07 | No per-candidate panic containment | H-13 | **C** (static; no panic found) |
| R-08 | Validity model absent | H-01 | **C** |
| R-09 | CI/gate do not enforce all guide gates | H-19 | **C** |
| R-10 | Benchmark evidence narrow | H-24 | **U** (wording only) |
| R-11 | Scope still open (B-01, B-03, B-04/B-06) | H-24 | **C** (static) |
| R-12 | Repository guide is not the unified guide | H-23 | **C** |
| R-13 | Placeholder repository URL | H-21 | **C** |
| R-14 | Cited evidence not in repository; README speed claim | H-23 | **C** (claim not re-run) |
| R-15 | Status labels overstate completion | H-01, H-04, H-23 | **C** |
| R-16 | Red-first regression tests mostly Python-only | H-19 | **C** |
| R-17 | VMDK grain tables multiplied in memory | H-05 | **C** (1,028 MiB from 0.27 MiB) |
| R-18 | VMDK descriptors read files outside the evidence dir | H-15 | **C** |
| R-19 | New parsers: no fuzz targets; E01 allocation uncapped | H-05, H-19 | **C** (1 TiB chunk ⇒ SIGABRT) |
| R-20 | B-06 stitching relies on weak validators | H-17 | **C** (mis-stitched PDF labelled `decoded`) |
| R-21 | Release identity ambiguous | H-21 | **C** |

### review-v2 (R-01…R-13, N-01…N-10)
| ID | Title (short) | → | Verdict / note |
|---|---|---|---|
| R-01 | `--max-memory` post-hoc | H-12 | **P** (as v1 R-01) |
| R-02 | PNG decode unbounded | guard §8 | **F** |
| R-03 | PNG CRC failures still "complete" | H-02 | **C** |
| R-04 | PDF `decoded` lexical | H-03 | **C** |
| R-05 | JPEG variants can be `decoded` | H-09 | **C** (partly: depends on kind/cut; see S-JPEGSPLICE) |
| R-06 | PDF EOF O(E×N) | H-11 | **C** |
| R-07 | No per-candidate panic containment | H-13 | **C** (static) |
| R-08 | Validity semantics absent | H-01 | **C** |
| R-09 | Gates incomplete | H-19 | **C** |
| R-10 | Benchmark evidence insufficient | H-24 | **U** |
| R-11 | Capabilities open | H-24 | **C** (static) |
| R-12 | Governing guide not synchronised | H-23 | **C** |
| R-13 | Placeholder repository URL | H-21 | **C** |
| N-01 | PDF `%%EOF` + CRLF/CR demoted to `partial` | H-10 | **C** (S-PDFEOL) |
| N-02 | JPEG SOF dimensions drive unbounded allocation | H-06 | **C** (S-JPEGSOF) |
| N-03 | Parallel carving slower than one thread | — | **N**: default threads 1.9–2× *faster* on 2 cores (§8); keep as guard |
| N-04 | Status labels overstated in REVIEW_RESPONSE/PROGRESS | H-23 | **C** |
| N-05 | A-10 limits unmet | H-22 | **C** (12 by our count) |
| N-06 | Container readers: uncapped E01 chunk, unbounded spool, no fuzz | H-05, H-20, H-19 | **C** |
| N-07 | B-06: wall-clock limit breaks determinism; weak validators | H-17, H-12 | **C** |
| N-08 | Validation evidence gitignored; CI evidence predates B-04/B-06 | H-23, H-19 | **C** (static) |
| N-09 | Stale binary; `commit = "unknown"` | H-21 | **C** |
| N-10 | Documentation drift (flags, absolute paths, tree) | H-23, H-21 | **C** |

### review-v3 (R-01…R-14)
| ID | Title (short) | → | Verdict / note |
|---|---|---|---|
| R-01 | `--max-memory` post-hoc output-size rejection | H-12 | **P** |
| R-02 | PNG CRC errors can be `complete`/`decoded` | H-02 | **C** |
| R-03 | PNG decode unbounded | guard §8 | **F** |
| R-04 | PDF `decoded` lexical | H-03 | **C** |
| R-05 | JPEG paths fall through to `decoded` | H-09 | **C** |
| R-06 | PDF EOF rescans growing prefixes | H-11 | **C** |
| R-07 | Candidate-level panic containment missing | H-13 | **C** (static) |
| R-08 | No authoritative validity semantics | H-01 | **C** |
| R-09 | Gate lacks G7–G9; CI fuzz not on push/PR | H-19 | **C** |
| R-10 | Benchmark narrow | H-24 | **U** |
| R-11 | Recovery/fs/format coverage incomplete | H-24 | **C** (static) |
| R-12 | Forensic source/output safety, operational UX | H-07, H-14, H-20 | **C** (read errors fatal; no SIGINT; stdin) |
| R-13 | Guide not synchronised | H-23 | **C** |
| R-14 | Placeholder repository URL | H-21 | **C** |

### review-v4 (F-01…F-16)
| ID | Title (short) | → | Verdict / note |
|---|---|---|---|
| F-01 | `--max-memory` post-hoc | H-12 | **P** |
| F-02 | PNG decode unbounded | guard §8 | **F** |
| F-03 | PNG CRC corruption yields `complete` | H-02 | **C** (its parser test must be rewritten, §3.3) |
| F-04 | PDF `decoded` lexical | H-03 | **C** |
| F-05 | JPEG variants outside strict checker labelled `decoded` | H-09 | **C** |
| F-06 | PDF EOF O(E×N) | H-11 | **C** |
| F-07 | No per-candidate panic boundary | H-13 | **C** (static) |
| F-08 | `status` ignores `validation` | H-01 | **C** |
| F-09 | G7–G9 missing; fuzz skipped on push/PR; coverage floor 60 % | H-19 | **C** |
| F-10 | Work done against a truncated guide | H-23 | **C** |
| F-11 | Placeholder repository URL | H-21 | **C** |
| F-12 | Benchmark narrow, partly confounded | H-24 | **U** |
| F-13 | B-01, B-03 open | H-24 | **C** (static) |
| F-14 | No fuzz/property tests for new parsers | H-19 | **C** |
| F-15 | No post-change CI execution | H-19 | **U** (`PENDING-CI`; CI cannot be run here) |
| F-16 | B-06 default time budget consumed by ordinary images | H-17 | **C** (≈ 5 s per image at defaults; root cause found) |
| (v4 §"chunk size bound-checked") | E01 chunk size claimed bound-checked | H-05 | **Refuted**: overflow-checked only |

### review-v5 (F-01…F-26)
| ID | Title (short) | → | Verdict / note |
|---|---|---|---|
| F-01 | `--max-memory` post-hoc; whole scan fails | H-12 | **P** (in-flight now; still fatal) |
| F-02 | PNG decode bomb | guard §8 | **F** |
| F-03 | PDF O(E×N) | H-11 | **C** |
| F-04 | Work budgets absent | H-12 | **C** (`--max-candidates`/`--time-budget` now exist; `--max-work-per-candidate` absent; time not enforced in batch) |
| F-05 | No panic containment | H-13 | **C** (static) |
| F-06 | CRC errors reach `decoded`/`complete` | H-02 | **C** |
| F-07 | PDF validation lexical | H-03 | **C** |
| F-08 | Validity vocabulary incomplete; docs contradict code | H-01 | **C** |
| F-09 | `%%EOF\r\n` demoted | H-10 | **C** |
| F-10 | Guide-mandated tests missing | H-19 | **C** |
| F-11 | Gate/CI do not enforce gates; gate needs root | H-19, H-26 | **C**; the SKIP-exit half is reproduced → H-26 |
| F-12 | Overstated statuses | H-23 | **C** |
| F-13 | Unparsable volume downgraded silently | H-18 | **C** (static confirmation) |
| F-14 | SIGINT / exit 4 not implemented | H-14 | **C** |
| F-15 | `--repair` labels unreadable PDFs `repaired` | H-04 | **C** |
| F-16 | `regions[]` not a coverage map | H-18 | **C** |
| F-17 | Default mode shows spliced files as `complete/100`; console omits level | H-01, H-09 | **C** |
| F-18 | Built against a truncated guide | H-23 | **C** |
| F-19 | Hygiene: placeholder URL, leaked paths, unrunnable Windows binary | H-21 | **C** (Windows binary not run) |
| F-20 | E01 aborts on partial final/damaged chunk | H-07 | **C** |
| F-21 | Container parsers allocate from untrusted fields | H-05 | **C** |
| F-22 | Unallocated VMDK grain tables are errors | H-08 | **C** |
| F-23 | VMDK descriptors read wrong/arbitrary file | H-15 | **C** |
| F-24 | B-06: no real-world recall; limit warnings exit 0 | H-17 | **C** |
| F-25 | Auto-detect reinterprets raw images | H-16 | **C** |
| F-26 | Container reports mix logical/physical; stdin identity and spool | H-20 | **C** |

### Verifier notes that are not attributable to one review
- **H-25** (truncated tail with non-zero padding) has no review ID: it was found by re-running A-05 with random and `0xFF` padding after all five reviews had accepted A-05 on zero padding only. **H-26** extends v5 F-11 ("gate needs root") with the reproduced exit-1 behaviour of `review_repro.py`.
- `validation_checks[]` (H-01) is named so because the report already has an integer `checks` field (`docs/report.schema.json:265`) that the guide's `checks[]` array would collide with.
- v4's statement that the E01 chunk size is "bound-checked" is **refuted** (overflow check only, H-05); v2 N-03 is **not reproduced** (§8); the post-hoc `--max-memory` claim shared by all five reviews is **partly outdated** on the snapshot (H-12); v4 F-15 (CI) and every benchmark number are **not re-run** (H-19, H-24).
- Root causes established by the verifier and used by the fixes: the B-06 cut-enumeration order and footer truncation (H-17), the duplicated `[+]` verbose line produced by two print sites (H-01), and the E01 short-final-chunk length rule (H-07).

---

## Appendix C — Fixture builders (tested)

Phase H0 must commit these builders (translate to Rust test helpers or keep the Python script under `tests/fixtures/`, run from the Python regress suite) **before** any fix. The script below is pure standard library, deterministic, and was run during verification: its `e01_good.E01`, `e01_short_comp.E01`, `e01_adler7.E01`, `vm_thin_ext.vmdk`, `vm_full_ext.vmdk`, `vm_renamed.vmdk`, `b06_zeros.img`, `b06_random.img`, `det_text.img` and the `tail_*.img` files are byte-identical to the fixtures behind the observations of Appendix A. Use it as follows:

```
python3 fixtures.py <repo>/bench/corpus <out-dir> [--big]   # writes every fixture below (--big adds the 4 MiB / 48 MiB S-TAIL JPEG rows, ≈ 150 MB, and the 64 MiB S10-flood512 and random images, ≈ 128 MB)
```

| File(s) | Scenario | Expected on the **snapshot** (before the fix) |
|---|---|---|
| `e01_good.E01` | S-E01-GOOD (control) | exit 0, 1 JPEG |
| `e01_short_comp.E01`, `e01_short_raw.E01` | S-E01-SHORT | exit 1, no report |
| `e01_adler1.E01`, `e01_adler7.E01` | S-E01-ADLER | exit 1, no report |
| `e01_huge_1048576_1048576.E01` (+ 2 smaller) | S-E01-HUGE | SIGABRT (exit 134) / ordinary error |
| `vm_thin.vmdk` (+ `vm_thin_ext.vmdk`) / `vm_full.vmdk` | S-VMDK-THIN / control | exit 1 "grain index out of range" / exit 0, 1 JPEG |
| `vm_alias1024.vmdk`, `vm_alias4096.vmdk` | S-VMDK-ALIAS (`carvex info`) | peak RSS ≈ 260 MiB / ≈ 1,028 MiB |
| `tv_abs/sub/d.vmdk`, `tv_dotdot/sub/d.vmdk` | S-VMDK-PATH | exit 0, `input.size = 512` |
| `vm_renamed.vmdk` | S-VMDK-RENAMED (`carvex info`) | fails "cannot open …/original_name.vmdk" |
| `z512/d.vmdk` | S-VMDK-ZERO (`ulimit -v 4000000`) | SIGABRT (exit 134) |
| `det_text.img`, `det_conectix.img`, `det_kdmv.img` | S-DETECT | auto: exit 1; `--input-format raw`: exit 0 |
| `b06_{zeros,random,otherfile}.img` | S-B06 (default mode; oracle = SHA-256 of the three corpus files) | 0/3 exact at default limits |
| `tail_{jpeg,png,pdf}_{zeros,ff,random}.img` (+ `tail_jpeg_{zeros,random,ff}_48M.img`, `tail_jpeg_ff_4M.img` with `--big`) | S-TAIL (cut at 60 % + 1 MiB / 48 MiB padding) | zeros: cut size; random: JPEG 0 files, PNG 852,157 B, PDF 1,422,748 B; `ff`: JPEG 1,261,044 B, PNG 852,157 B, PDF 1,422,748 B |
| `tail_jpeg_neigh_{0,4096}.img` | S-TAIL-NEIGH | only the complete JPEG is carved, `rejected = 1` |
| `regions.img`, `regions_ntfsbad.img` | S-REGIONS (1 MiB gap, 32 MiB FAT32, 2 MiB gap, 16 MiB FAT32; second volume garbage with an NTFS signature) | Σ scanned regions 53,477,376 ≠ `bytes_scanned` 53,067,776; garbage volume: exit 0, no warning |
| `S9-{256K,1M,4M}.img` (+ `S10-flood512-64M.img`, `random-64M.img` with `--big`) | S-FLOOD (guide S9/S10) | 0.18 / 0.61 / 2.32 / 1.23 / 0.02 s (default threads) |
| `pngcrc{1,3}.img` | S-PNGCRC with 1 and 3 corrupted IDAT CRCs | `complete`, no validity |

The "Expected" column is the `a7a5c08` behaviour; the `ab725a6` behaviour of every row is in Appendix A.2. `detect_fixtures` plants one JPEG at byte 65,536 of each `det_*.img`. Fixtures for S-PDFLEX/S-PDFEOL/S-REPAIR/S-JPEGSPLICE/S-JPEGSOF are corpus files corrupted as described in Appendix A (for the PDF xref variants locate the table with `pdf.index(b"\nxref\n") + 1` — `rindex(b"xref\n")` finds `startxref` and the patches become no-ops).

Notes: `build_vmdk_sparse` writes a header-only extent (its `desc` argument is unused; the reader accepts it, as the control `vm_full.vmdk` shows). The E01 writer stores the last chunk **short**, as libewf does. For S-PNGCRC, S-PDFLEX, S-PDFEOL, S-REPAIR, S-JPEGSPLICE and S-JPEGSOF no special container is needed: corrupt a corpus file as described in Appendix A and wrap it with `make_image`.

```python
#!/usr/bin/env python3
"""Fixture builders for the CarveX harness (Appendix C).  Pure standard library.
Usage: python3 fixtures.py CORPUS_DIR OUT_DIR      (CORPUS_DIR = <repo>/bench/corpus)
Builds the fixtures of Appendix A that need hand-made containers or file systems.
Every builder is deterministic; assert the sizes printed at the end before using them."""
import os, random, struct, sys, zlib


# ---- generic helpers -------------------------------------------------------
def rd(p):
    with open(p, "rb") as f:
        return f.read()


def wr(p, b):
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "wb") as f:
        f.write(b)


def pad4k(b):
    return b + b"\0" * (-len(b) % 4096)


def make_image(payloads, lead=4096, tail=65536):
    out = bytearray(b"\0" * lead)
    for p in payloads:
        out += pad4k(p)
    out += b"\0" * tail
    return bytes(out)

# ---- minimal FAT32 volume (all clusters free; root directory = cluster 2) ----
def fat32(total_sectors, spc):
    fat_sectors = (total_sectors * 4 + 1024 + 511) // 512
    boot = bytearray(512)
    boot[0:3] = bytes([0xEB, 0x3C, 0x90])
    boot[3:11] = b"MSWIN4.1"
    struct.pack_into("<H", boot, 0x0B, 512)
    boot[0x0D] = spc
    struct.pack_into("<H", boot, 0x0E, 1)
    boot[0x10] = 1
    boot[0x15] = 0xF8
    struct.pack_into("<I", boot, 0x20, total_sectors)
    struct.pack_into("<I", boot, 0x24, fat_sectors)
    struct.pack_into("<I", boot, 0x2C, 2)
    boot[0x47:0x52] = b"CARVEX     "
    boot[0x52:0x57] = b"FAT32"
    boot[510:512] = b"\x55\xaa"
    img = bytearray(512 * total_sectors)
    img[0:512] = boot
    fat = 512
    struct.pack_into("<I", img, fat, 0x0FFFFFF8)
    struct.pack_into("<I", img, fat + 4, 0x0FFFFFFF)
    struct.pack_into("<I", img, fat + 8, 0x0FFFFFFF)
    data_start = (1 + fat_sectors) * 512
    return img, data_start, spc * 512


# ---- E01 (EWF v1) writer: volume / sectors / table / done sections ----
def adler(b):
    return zlib.adler32(b) & 0xFFFFFFFF


def ewf_desc(kind, nxt, size):
    d = kind.encode().ljust(16, b"\0") + struct.pack("<QQ", nxt, size) + b"\0" * 40
    return d + struct.pack("<I", adler(d))


def build_e01(path, raw, spc=64, bps=512, compress=None, corrupt=None, media_sectors=None, chunk_bytes=None,
              declared_spc=None, declared_bps=None):
    """raw: bytes of the guest disk.  compress(i)->bool.  corrupt: set of chunk idx with a bad adler.
    The last chunk is stored SHORT (only the remaining bytes) like libewf does."""
    cs = chunk_bytes or spc * bps
    media = len(raw)
    sectors = media_sectors if media_sectors is not None else media // bps
    chunks = [raw[i:i + cs] for i in range(0, len(raw), cs)]
    blobs = []
    for i, c in enumerate(chunks):
        if compress and compress(i):
            blobs.append((True, zlib.compress(c)))
        else:
            a = adler(c)
            if corrupt and i in corrupt:
                a ^= 0xFFFF
            blobs.append((False, c + struct.pack("<I", a)))
    vol = struct.pack("<IIII", 1, len(chunks), declared_spc or spc, declared_bps or bps) + struct.pack("<Q", sectors)
    vol = vol.ljust(94, b"\0")
    pos = 13
    out = bytearray(b"EVF\x09\x0d\x0a\xff\x00" + b"\x01" + struct.pack("<H", 1) + b"\0\0")
    # volume
    vol_pos = pos
    nxt = vol_pos + 76 + len(vol)
    out += ewf_desc("volume", nxt, 76 + len(vol)) + vol
    # sectors
    sec_pos = len(out)
    total_blob = sum(len(b) for _, b in blobs)
    nxt = sec_pos + 76 + total_blob
    out += ewf_desc("sectors", nxt, 76 + total_blob)
    entries = []
    for comp, b in blobs:
        entries.append((len(out)) | (0x80000000 if comp else 0))
        out += b
    # table
    tab_pos = len(out)
    tab = struct.pack("<IIQII", len(entries), 0, 0, 0, 0) + b"".join(struct.pack("<I", e) for e in entries)
    nxt = tab_pos + 76 + len(tab)
    out += ewf_desc("table", nxt, 76 + len(tab)) + tab
    done_pos = len(out)
    out += ewf_desc("done", done_pos, 76)
    wr(path, bytes(out))



# ---- VMDK hosted sparse extent (KDMV) writer ----
def build_vmdk_sparse(path, capacity_sectors, grain_sectors, num_gte, gd_entries, tables, grains,
                      desc=None, flags=3):
    """gd_entries: list of table index (None=unallocated). tables: list of list[int] grain sector numbers.
    grains: dict sector->bytes written at that sector. Layout: sector0 header, sector1 desc, sector 8 GD, then tables."""
    hdr = bytearray(512)
    hdr[0:4] = b"KDMV"
    struct.pack_into("<II", hdr, 4, 1, flags)
    struct.pack_into("<QQ", hdr, 12, capacity_sectors, grain_sectors)
    struct.pack_into("<QQ", hdr, 28, 0, 0)
    struct.pack_into("<I", hdr, 44, num_gte)
    gd_sector = 8
    struct.pack_into("<Q", hdr, 56, gd_sector)
    hdr[73:77] = b"\n \r\n"
    img = bytearray(hdr)
    img += b"\0" * (gd_sector * 512 - len(img))
    gd_bytes = len(gd_entries) * 4
    gd_secs = -(-gd_bytes // 512)
    tab_start = gd_sector + gd_secs
    tab_secs = -(-(num_gte * 4) // 512)
    tab_sector = [tab_start + i * tab_secs for i in range(len(tables))]
    gd = b"".join(struct.pack("<I", 0 if t is None else tab_sector[t]) for t in gd_entries)
    img += gd.ljust(gd_secs * 512, b"\0")
    for t in tables:
        tb = b"".join(struct.pack("<I", g) for g in t)
        img += tb.ljust(tab_secs * 512, b"\0")
    for sec, data in sorted(grains.items()):
        need = sec * 512
        if len(img) < need:
            img += b"\0" * (need - len(img))
        img[need:need + len(data)] = data
    wr(path, bytes(img))



# ---- recipes ---------------------------------------------------------------
def e01_fixtures(out, jpg):
    cs = 64 * 512                                   # 32 KiB chunks

    def disk(nchunks, extra=0):
        d = bytearray(b"\0" * (nchunks * cs + extra))
        assert cs + len(jpg) <= len(d)
        d[cs:cs + len(jpg)] = jpg                   # JPEG at offset 32,768
        return bytes(d)

    d8, d75 = disk(8), disk(7, extra=cs // 2)       # 8 chunks / 7.5 chunks (short last chunk)
    build_e01(out + "/e01_good.E01", d8, compress=lambda i: i % 2 == 0)                    # S-E01-GOOD
    build_e01(out + "/e01_short_comp.E01", d75, compress=lambda i: True)                   # S-E01-SHORT
    build_e01(out + "/e01_short_raw.E01", d75, compress=lambda i: False)                   # S-E01-SHORT
    build_e01(out + "/e01_adler1.E01", d8, compress=lambda i: False, corrupt={1})          # S-E01-ADLER
    build_e01(out + "/e01_adler7.E01", d8, compress=lambda i: False, corrupt={7})          # S-E01-ADLER
    for spc, bps in ((1 << 20, 1 << 20), (1 << 16, 1 << 16), (1 << 12, 1 << 12)):         # S-E01-HUGE
        build_e01(out + f"/e01_huge_{spc}_{bps}.E01", b"\0" * 4096, spc=8, bps=512,
                  compress=lambda i: True, declared_spc=spc, declared_bps=bps, media_sectors=1)


def vmdk_fixtures(out, jpg):
    grain = 128                                     # 64 KiB grains
    ds = 100                                        # first data sector
    gdata = {ds + i * grain: jpg.ljust(4 * 65536, b"\0")[i * 65536:(i + 1) * 65536] for i in range(4)}
    tables = [[ds + i * grain for i in range(4)]]
    D = ("# Disk DescriptorFile\nversion=1\nCID=fffffffe\nparentCID=ffffffff\n"
         "createType=\"monolithicSparse\"\n\n# Extent description\nRW 4096 SPARSE \"%s\"\n")
    # S-VMDK-THIN: 8 grain-directory entries x 4 GTEs x 64 KiB = 2 MiB, only entry 0 allocated
    build_vmdk_sparse(out + "/vm_thin_ext.vmdk", 4096, grain, 4, [0] + [None] * 7, tables, gdata)
    wr(out + "/vm_thin.vmdk", (D % "vm_thin_ext.vmdk").encode())
    # control: same, entries 1..7 point at a table of zero grains
    build_vmdk_sparse(out + "/vm_full_ext.vmdk", 4096, grain, 4, [0] + [1] * 7, [tables[0], [0] * 4], gdata)
    wr(out + "/vm_full.vmdk", (D % "vm_full_ext.vmdk").encode())
    # S-VMDK-ALIAS: n directory entries, all pointing at one 256 KiB table (file stays ~0.27 MiB)
    for n in (1024, 4096):
        num_gte = 65536
        build_vmdk_sparse(out + f"/vm_alias{n}_ext.vmdk", n * num_gte, 1, num_gte, [0] * n, [[0] * num_gte], {})
        wr(out + f"/vm_alias{n}.vmdk", ("# Disk DescriptorFile\nversion=1\ncreateType=\"monolithicSparse\"\n\n"
           f"# Extent description\nRW {n * num_gte} SPARSE \"vm_alias{n}_ext.vmdk\"\n").encode())
    # S-VMDK-PATH: absolute path and '..' to a 512-byte secret outside the evidence directory
    wr(out + "/vm_secret.bin", b"SECRET-OUTSIDE-EVIDENCE" + b"\0" * (512 - 23))
    for tag, target in (("abs", os.path.abspath(out + "/vm_secret.bin")), ("dotdot", "../../vm_secret.bin")):
        wr(out + f"/tv_{tag}/sub/d.vmdk", ("# Disk DescriptorFile\nversion=1\ncreateType=\"monolithicFlat\"\n\n"
           f"# Extent description\nRW 1 FLAT \"{target}\" 0\n").encode())
    # S-VMDK-RENAMED: self-contained sparse extent whose embedded descriptor names original_name.vmdk
    emb = bytearray(open(out + "/vm_full_ext.vmdk", "rb").read())
    desc = (D % "original_name.vmdk").replace("CID=fffffffe\nparentCID=ffffffff\n", "").encode().ljust(512, b"\0")
    struct.pack_into("<QQ", emb, 28, 2, 1)          # descriptor offset (sector 2), size (1 sector)
    emb[1024:1536] = desc
    wr(out + "/vm_renamed.vmdk", bytes(emb))
    # S-VMDK-ZERO (small control) and the 512 TiB case
    wr(out + "/vm_zero_flat.bin", b"\x11" * 4096)
    wr(out + "/vm_zero.vmdk", ("# Disk DescriptorFile\nversion=1\ncreateType=\"custom\"\n\n# Extent description\n"
                              "RW 8 ZERO\nRW 8 FLAT \"vm_zero_flat.bin\" 0\n").encode())
    wr(out + "/z512/flat.bin", b"\0" * 4096)
    wr(out + "/z512/d.vmdk", ("# Disk DescriptorFile\nversion=1\nCID=fffffffe\nparentCID=ffffffff\n"
        "createType=\"monolithicFlat\"\n\n# Extent description\nRW 1099511627776 ZERO\nRW 8 FLAT \"flat.bin\" 0\n").encode())


def detect_fixtures(out, jpg):                       # S-DETECT: a JPEG is planted at byte 65,536 of each image
    a = bytearray(1 << 20); a[4096:4096 + 24] = b"# Extent description\n\n\n"
    b = bytearray(1 << 20); b[-512:-504] = b"conectix"
    c = bytearray(1 << 20); c[0:4] = b"KDMV"
    for n, v in (("det_text", a), ("det_conectix", b), ("det_kdmv", c)):
        v[65536:65536 + len(jpg)] = jpg
        wr(out + f"/{n}.img", bytes(v))


def b06_fixture(out, corpus, gapfill):               # S-B06; gapfill in {"zeros", "random", "otherfile"}
    files = {"jpeg": rd(corpus + "/jpeg/jpeg_01_baseline_640x480.jpg"),
             "png": rd(corpus + "/png/png_02_rgb_800x600.png"),
             "pdf": rd(corpus + "/pdf/pdf_05_large_scan.pdf")}
    other = rd(corpus + "/jpeg/jpeg_04_grayscale_512x512.jpg")
    img, ds, cs = fat32(131072, 8)                   # 64 MiB, 4 KiB clusters
    cl, rng = 200, random.Random(5)
    for k, v in files.items():
        half = (len(v) // 2) // 4096 * 4096          # cluster-aligned midpoint
        h1, h2 = v[:half], v[half:]
        gap = (b"\0" * (3 * cs) if gapfill == "zeros" else rng.randbytes(3 * cs) if gapfill == "random"
               else other[20000:20000 + 3 * cs])
        base = ds + (cl - 2) * cs
        img[base:base + len(h1)] = h1
        o2 = base + (-(-len(h1) // cs)) * cs
        img[o2:o2 + len(gap)] = gap
        o3 = o2 + len(gap)
        img[o3:o3 + len(h2)] = h2
        cl += (o3 + len(h2) - base) // cs + 60
    wr(out + f"/b06_{gapfill}.img", bytes(img))      # oracle: SHA-256 of each corpus file


def tail_image(path, src_file, frac, pad, pad_len, seed=7, lead=4096, trail=8192):
    """S-TAIL: `lead` zero bytes + the first `frac` of src_file + padding + `trail` zero bytes.
    pad in {"zeros", "ff", "random"} (random is seeded -> deterministic). Returns the cut length."""
    d = rd(src_file)
    cut = int(len(d) * frac)
    if pad == "zeros":
        padding = b"\0" * pad_len
    elif pad == "ff":
        padding = b"\xff" * pad_len
    else:
        padding = random.Random(seed).randbytes(pad_len)
    wr(path, b"\0" * lead + d[:cut] + padding + b"\0" * trail)
    return cut


def tail_fixtures(out, corpus, big=False):          # S-TAIL, S-TAIL-NEIGH
    src = {"jpeg": corpus + "/jpeg/jpeg_10_progressive_1200x800.jpg",
           "png": corpus + "/png/png_02_rgb_800x600.png",
           "pdf": corpus + "/pdf/pdf_05_large_scan.pdf"}
    cuts = {}
    for fmt, f in src.items():
        for pad in ("zeros", "ff", "random"):
            cuts[f"tail_{fmt}_{pad}"] = tail_image(f"{out}/tail_{fmt}_{pad}.img", f, 0.6, pad, 1 << 20)
    if big:                                          # the large JPEG rows of S-TAIL (about 150 MB on disk)
        for pad, mib in (("zeros", 48), ("random", 48), ("ff", 4), ("ff", 48)):
            cuts[f"tail_jpeg_{pad}_{mib}M"] = tail_image(f"{out}/tail_jpeg_{pad}_{mib}M.img", src["jpeg"], 0.6, pad, mib << 20)
    # S-TAIL-NEIGH: a truncated JPEG directly followed by a complete one (0 or 4,096 zero bytes between)
    d = rd(src["jpeg"])
    cut = int(len(d) * 0.6)
    nxt = rd(corpus + "/jpeg/jpeg_03_progressive_800x600.jpg")
    for gap in (0, 4096):
        wr(f"{out}/tail_jpeg_neigh_{gap}.img", b"\0" * 4096 + d[:cut] + b"\0" * gap + nxt + b"\0" * 8192)
    cuts["neigh_next_size"] = len(nxt)
    return cuts


def mbr(parts):                                      # parts: [(type, start_lba, nsect)]
    m = bytearray(512)
    for i, (t, s, n) in enumerate(parts):
        e = 446 + 16 * i
        m[e + 4] = t
        struct.pack_into("<II", m, e + 8, s, n)
    m[510:512] = b"\x55\xaa"
    return bytes(m)


def regions_fixtures(out, corpus):                   # S-REGIONS (51 MiB = 53,477,376 B)
    MiB = 1 << 20
    j = rd(corpus + "/jpeg/jpeg_01_baseline_640x480.jpg")
    v1 = fat32(32 * MiB // 512, 8)[0]
    v2 = fat32(16 * MiB // 512, 8)[0]
    disk = bytearray(51 * MiB)                       # 1 MiB gap | 32 MiB FAT32 | 2 MiB gap | 16 MiB FAT32
    disk[0:512] = mbr([(0x0C, 2048, 32 * MiB // 512), (0x0C, 35 * MiB // 512, 16 * MiB // 512)])
    disk[MiB:MiB + len(v1)] = v1
    disk[35 * MiB:35 * MiB + len(v2)] = v2
    disk[33 * MiB + 4096:33 * MiB + 4096 + len(j)] = j   # JPEG in the 2 MiB gap
    disk[10 * MiB:10 * MiB + len(j)] = j                 # JPEG in the free area of volume 1
    wr(out + "/regions.img", bytes(disk))
    bad = bytearray(disk)                            # second volume: type 0x07 with an NTFS signature but garbage
    bad[0:512] = mbr([(0x0C, 2048, 32 * MiB // 512), (0x07, 35 * MiB // 512, 16 * MiB // 512)])
    bad[35 * MiB:35 * MiB + 512] = b"\0" * 512
    bad[35 * MiB + 3:35 * MiB + 11] = b"NTFS    "
    bad[35 * MiB + 510:35 * MiB + 512] = b"\x55\xaa"
    wr(out + "/regions_ntfsbad.img", bytes(bad))


def flood_fixtures(out, big=True):                   # S9 / S10 (guide S9-S11); random file is not deterministic
    for name, n in (("S9-256K", 256 << 10), ("S9-1M", 1 << 20), ("S9-4M", 4 << 20)):
        wr(f"{out}/{name}.img", (b"\xff\xd8\xff\xe0" + b"A" * 12) * (n // 16))
    if big:
        hdr = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00"
        d = bytearray(64 << 20)
        for off in range(0, len(d) - len(hdr), 512):
            d[off:off + len(hdr)] = hdr
        wr(out + "/S10-flood512-64M.img", bytes(d))
        wr(out + "/random-64M.img", random.Random(1).randbytes(64 << 20))


def pngcrc_fixture(out, corpus, nbad=3):             # S-PNGCRC (nbad IDAT chunks with a flipped CRC byte)
    png = rd(corpus + "/png/png_02_rgb_800x600.png")
    p, idat = 8, []
    while p < len(png):
        n, = struct.unpack(">I", png[p:p + 4])
        if png[p + 4:p + 8] == b"IDAT":
            idat.append((p, n))
        p += 12 + n
    b = bytearray(png)
    for p, n in idat[:nbad]:
        b[p + 8 + n] ^= 1
    wr(f"{out}/pngcrc{nbad}.img", make_image([bytes(b)]))


if __name__ == "__main__":
    corpus, out = sys.argv[1], sys.argv[2]
    jpg = rd(corpus + "/jpeg/jpeg_01_baseline_640x480.jpg")
    e01_fixtures(out, jpg)
    vmdk_fixtures(out, jpg)
    detect_fixtures(out, jpg)
    for g in ("zeros", "random", "otherfile"):
        b06_fixture(out, corpus, g)
    regions_fixtures(out, corpus)
    flood_fixtures(out, big="--big" in sys.argv)
    for k in (1, 3):
        pngcrc_fixture(out, corpus, k)
    print(tail_fixtures(out, corpus, big="--big" in sys.argv))
    print(sorted(os.listdir(out)))
```

### C.2 `scripts/fn_len.py` (H-22; tested)

*Metric note (`ab725a6`):* the repository's metric of record is clippy's `too_many_lines` (`clippy.toml`, threshold 120, code lines only): 0 findings. This script counts **raw lines** from signature to closing brace and is stricter: 12 findings on `a7a5c08`, 4 on `ab725a6` (`src/signatures/pdf.rs:23` 136, `src/scan.rs:2514` 134, `src/signatures/jpeg.rs:543` 129, `src/signatures/pdf_validate.rs:138` 125). Use it as an informational gate.

Brace-matching counter that skips comments, strings and `#[cfg(test)]` modules. Run on the snapshot it prints exactly the 12 functions listed in H-22 (273, 246, 244, 239, 198, 179, 176, 155, 134, 131, 126, 125 lines) and exits 1.

```python
#!/usr/bin/env python3
"""List Rust functions longer than a limit (guide A-10: 120 lines).
Method: a function starts at a line containing `fn NAME` and ends at the matching closing brace;
its length is (closing line - signature line + 1). Braces inside strings, char literals and comments
are ignored. Test code (`#[cfg(test)]` modules) is skipped. Usage: fn_len.py SRC_DIR [--limit 120]
Exit code 0 when no function exceeds the limit, 1 otherwise."""
import os, re, sys

FN = re.compile(r'\bfn\s+([A-Za-z_][A-Za-z0-9_]*)')


def strip(src):
    """Replace comments, string and char literals by spaces, keeping newlines and length."""
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith('//', i):
            j = src.find('\n', i); j = n if j < 0 else j
            out.append(' ' * (j - i)); i = j
        elif src.startswith('/*', i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith('/*', j): depth += 1; j += 2
                elif src.startswith('*/', j): depth -= 1; j += 2
                else: j += 1
            out.append(re.sub(r'[^\n]', ' ', src[i:j])); i = j
        elif c == 'r' and re.match(r'r#*"', src[i:i + 8]):
            m = re.match(r'r(#*)"', src[i:]); close = '"' + m.group(1)
            j = src.find(close, i + len(m.group(0)))
            j = n if j < 0 else j + len(close)
            out.append(re.sub(r'[^\n]', ' ', src[i:j])); i = j
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == '\\' else 1
            j = min(n, j + 1)
            out.append(re.sub(r'[^\n]', ' ', src[i:j])); i = j
        elif c == "'":
            m = re.match(r"'(\\.[^']*|[^\\'])'", src[i:i + 12])
            if m:
                out.append(' ' * len(m.group(0))); i += len(m.group(0))
            else:
                out.append(c); i += 1                 # a lifetime
        else:
            out.append(c); i += 1
    return ''.join(out)


def functions(path):
    src = strip(open(path, encoding='utf-8', errors='replace').read())
    lines = src.split('\n')
    starts = [0]
    for ln in lines: starts.append(starts[-1] + len(ln) + 1)
    # skip #[cfg(test)] modules: find `mod NAME {` that follows a cfg(test) attribute
    skip = []
    for m in re.finditer(r'#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*mod\s+\w+\s*\{', src):
        depth, j = 1, m.end()
        while j < len(src) and depth:
            depth += (src[j] == '{') - (src[j] == '}'); j += 1
        skip.append((m.start(), j))
    res = []
    for m in FN.finditer(src):
        if any(a <= m.start() < b for a, b in skip): continue
        j = m.end()
        while j < len(src) and src[j] not in '{;': j += 1     # signature ends at `{` (body) or `;` (declaration)
        if j >= len(src) or src[j] == ';': continue
        depth, k = 1, j + 1
        while k < len(src) and depth:
            depth += (src[k] == '{') - (src[k] == '}'); k += 1
        a = src.count('\n', 0, m.start()) + 1
        b = src.count('\n', 0, k - 1) + 1
        res.append((b - a + 1, m.group(1), a))
    return res


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else 'src'
    limit = int(sys.argv[sys.argv.index('--limit') + 1]) if '--limit' in sys.argv else 120
    found = []
    for d, _, fs in os.walk(root):
        for f in fs:
            if f.endswith('.rs'):
                p = os.path.join(d, f)
                found += [(n, os.path.relpath(p, root) + '::' + name, line) for n, name, line in functions(p) if n > limit]
    found.sort(reverse=True)
    for n, name, line in found: print(f'{n:4d}  {name}  (line {line})')
    print(f'{len(found)} function(s) longer than {limit} lines in {root}')
    return 1 if found else 0


if __name__ == '__main__':
    sys.exit(main())
```

### C.3 `scripts/repair_ref.py` (H-04; tested)

Reference algorithm for the validated PNG/JPEG repair of H-04 ("crop to the recovered rows"): Python, standard library plus Pillow as the oracle. It is a model for the Rust port, not a deliverable. Run on the corpus (`python3 -I scripts/repair_ref.py <repo>/bench/corpus`, 2026-11-25) it repairs `png_02` and `jpeg_01` at 30/60/90 % — PNG 180 / 359 / 539 of 600 rows, JPEG 8 / 17 / 27 of 30 MCU rows — and every output passes Pillow `verify()`+`load()` with the top rows identical to the original's (exit 0). Over the whole corpus it repairs 9/10 PNGs (the interlaced one is skipped) and 8/10 JPEGs (the two progressive ones are skipped) at each of the three cuts, with no oracle failure.

```python
#!/usr/bin/env python3
"""Reference algorithm for H-04 (validated PNG/JPEG repair): "crop to the recovered rows".
Python, standard library + Pillow (oracle only).  The Rust port must produce the same rows.

png_repair(cut)  -> (bytes|None, info)   non-interlaced PNG: inflate the readable IDAT prefix, keep the
                                         complete scanlines, rewrite IHDR height, re-deflate, add IEND.
jpeg_repair(cut) -> (bytes|None, info)   baseline sequential JPEG without restart intervals: decode
                                         the entropy data MCU by MCU, keep complete MCU rows, rewrite
                                         the SOF height, pad the last byte with 1-bits, append EOI.
Every pixel of the output is a pixel of the original; nothing is invented.  Everything else
(progressive JPEG, restart intervals, interlaced PNG; 16-bit samples are fine) returns None -> keep `partial`.
INPUT CONTRACT: the exact carved bytes up to the true end of the data (after the tail trim of H-25). Padding
left after the cut is decoded as if it were data and invents rows (4 KiB of zeros: PNG 361 instead of 359 rows,
JPEG 19 instead of 17 MCU rows) -- the oracle comparison with the original catches it.

Usage: repair_ref.py CORPUS_DIR          # runs 30/60/90 % cuts of png_02 and jpeg_01, checks with Pillow
Exit code 0 when every cut loads with Pillow verify()+load() and its top rows equal the original's."""
import io, struct, sys, zlib


def _chunk(t, d):
    return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xFFFFFFFF)


def png_repair(cut):
    if cut[:8] != b'\x89PNG\r\n\x1a\n':
        return None, 'not a PNG'
    pos, pre, idat, ihdr = 8, [], b'', None
    while pos + 8 <= len(cut):
        ln, = struct.unpack('>I', cut[pos:pos + 4])
        typ, body = cut[pos + 4:pos + 8], cut[pos + 8:pos + 8 + ln]      # body is short for the cut chunk
        if typ == b'IHDR':
            ihdr = body
        elif typ == b'IDAT':
            idat += body
        elif typ == b'IEND':
            break
        elif not idat:
            pre.append(cut[pos:pos + 12 + ln])                         # PLTE, tRNS, gAMA ... before IDAT
        pos += 12 + ln                                                  # a cut chunk ends the loop here
    if ihdr is None or len(ihdr) < 13:
        return None, 'no IHDR'
    w, h, bd, ct, cm, fm, il = struct.unpack('>IIBBBBB', ihdr[:13])
    if il != 0:
        return None, 'interlaced: not handled'
    rowbytes = 1 + (w * {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ct] * bd + 7) // 8
    try:
        raw = zlib.decompressobj().decompress(idat)                    # partial output is kept on a cut stream
    except zlib.error:
        raw = b''
    n = min(len(raw) // rowbytes, h)
    if n == 0:
        return None, 'no complete row'
    out = (b'\x89PNG\r\n\x1a\n' + _chunk(b'IHDR', struct.pack('>IIBBBBB', w, n, bd, ct, cm, fm, il))
           + b''.join(pre) + _chunk(b'IDAT', zlib.compress(raw[:n * rowbytes], 6)) + _chunk(b'IEND', b''))
    return out, {'rows': n, 'of': h}


def jpeg_repair(cut):
    pos, dht, sof, sos = 2, {}, None, None
    while pos + 4 <= len(cut) and cut[pos] == 0xFF:
        m, ln = cut[pos + 1], struct.unpack('>H', cut[pos + 2:pos + 4])[0]
        body = cut[pos + 4:pos + 2 + ln]
        if m == 0xC0:
            sof = (pos, body)
        elif m in (0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF):
            return None, 'not baseline sequential: not handled'
        elif m == 0xDD:
            return None, 'restart intervals: not handled'
        elif m == 0xC4:
            p = 0
            while p < len(body):
                tc_th, cnt = body[p], list(body[p + 1:p + 17])
                syms, code, k, tab = list(body[p + 17:p + 17 + sum(cnt)]), 0, 0, {}
                for length in range(1, 17):
                    for _ in range(cnt[length - 1]):
                        tab[(length, code)] = syms[k]; code += 1; k += 1
                    code <<= 1
                dht[tc_th] = tab
                p += 17 + sum(cnt)
        elif m == 0xDA:
            sos = body; pos += 2 + ln
            break
        pos += 2 + ln
    if sof is None or sos is None:
        return None, 'no SOF/SOS in the readable prefix'
    _, Y, X, nc = struct.unpack('>BHHB', sof[1][:6])
    comps = {sof[1][6 + 3 * i]: (sof[1][7 + 3 * i] >> 4, sof[1][7 + 3 * i] & 15) for i in range(nc)}
    scan = [(sos[1 + 2 * i], sos[2 + 2 * i] >> 4, sos[2 + 2 * i] & 15) for i in range(sos[0])]
    if len(scan) != nc:
        return None, 'non-interleaved scan: not handled'
    vmax, hmax = max(v for _, v in comps.values()), max(h for h, _ in comps.values())
    mcux, mcuy = -(-X // (8 * hmax)), -(-Y // (8 * vmax))
    data = cut[pos:]
    bits, owner, raw_end, i = [], [], [], 0                             # un-stuffed bits, byte owning each bit
    while i < len(data):
        b = data[i]
        if b == 0xFF:
            if i + 1 < len(data) and data[i + 1] == 0:
                nxt = i + 2
            else:
                break                                                   # a marker: the entropy data ends here
        else:
            nxt = i + 1
        for k in range(7, -1, -1):
            bits.append((b >> k) & 1); owner.append(len(raw_end))
        raw_end.append(nxt); i = nxt
    bp = 0

    def huff(tab):
        nonlocal bp
        code = 0
        for length in range(1, 17):
            if bp >= len(bits):
                raise EOFError
            code = (code << 1) | bits[bp]; bp += 1
            if (length, code) in tab:
                return tab[(length, code)]
        raise ValueError

    def recv(n):
        nonlocal bp
        if bp + n > len(bits):
            raise EOFError
        bp += n

    rows, endbit = 0, 0
    try:
        for _ in range(mcuy):
            for _ in range(mcux):
                for cid, td, ta in scan:
                    h, v = comps[cid]
                    for _ in range(h * v):
                        s = huff(dht[td])                               # DC table (class 0)
                        if s:
                            recv(s)
                        k = 1
                        while k < 64:
                            rs = huff(dht[16 + ta])                     # AC table (class 1)
                            if rs & 15 == 0:
                                if rs >> 4 == 15:
                                    k += 16; continue
                                break
                            recv(rs & 15); k += (rs >> 4) + 1
            rows += 1; endbit = bp
    except (EOFError, ValueError, KeyError):
        pass
    if rows == 0:
        return None, 'no complete MCU row'
    last = owner[endbit - 1]                                            # index of the last byte that holds data bits
    used = endbit - owner.index(last)                                   # 1..8 bits of that byte are data
    ent = bytearray(data[:raw_end[last]])
    if used < 8 and not ent.endswith(b'\xff\x00'):
        ent[-1] |= (1 << (8 - used)) - 1                                # pad with 1-bits (T.81 F.1.2.3)
        if ent[-1] == 0xFF:
            ent.append(0)
    hdr = bytearray(cut[:pos])
    height = min(rows * 8 * vmax, Y)
    hdr[sof[0] + 5:sof[0] + 7] = struct.pack('>H', height)
    return bytes(hdr) + bytes(ent) + b'\xff\xd9', {'mcu_rows': rows, 'of': mcuy, 'height': height, 'of_height': Y}


def check(out, orig, slack):
    from PIL import Image
    try:
        Image.open(io.BytesIO(out)).verify()
        im = Image.open(io.BytesIO(out)); im.load()
    except Exception as e:                                              # noqa: BLE001
        return False, 'FAIL ' + repr(e)[:80]
    a, o = im.convert('RGB'), Image.open(io.BytesIO(orig)).convert('RGB')
    k = a.size[1] - slack                                               # JPEG: last rows may differ (chroma edge)
    same = a.crop((0, 0, a.size[0], k)).tobytes() == o.crop((0, 0, o.size[0], k)).tobytes()
    return same, 'OK %dx%d, top %d rows identical to the original: %s' % (a.size[0], a.size[1], k, same)


def main():
    corpus, bad = sys.argv[1], 0
    png = open(corpus + '/png/png_02_rgb_800x600.png', 'rb').read()
    jpg = open(corpus + '/jpeg/jpeg_01_baseline_640x480.jpg', 'rb').read()
    for name, fn, src, slack in (('PNG', png_repair, png, 0), ('JPEG', jpeg_repair, jpg, 2)):
        for pct in (30, 60, 90):
            out, info = fn(src[:len(src) * pct // 100])
            ok, msg = check(out, src, slack) if out else (False, 'no repair: %s' % info)
            bad += not ok
            print('%-4s %2d%%  %s  %s' % (name, pct, info, msg))
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
```

### C.4 Part T prototypes (H-27, H-30, H-32; tested on `f5bda4a`)

Four Python files (standard library only; Pillow is not needed) that produced the Part T evidence of §7.4. They are **prototypes and measurement tools**, not deliverables; the Rust implementation must be tested against the same layouts. Corpus: `<repo>/bench/corpus` (the repository's generated JPEG/PNG/PDF files; `e1_islands.py` and `x2_png_crc.py` use the files of at least 12 clusters; 14 JPEG/PNG files qualified for E1). Run 2026-11-26 on the `f5bda4a` release binary (`carvex scan … --validate decode`), seed 2026; each script prints exact recall with Wilson 95 % intervals. `fat32_img.py` is a module: `build_fat32(total_sectors=65536, spc=8, files, deleted)` writes a FAT32 image with live and deleted files at chosen clusters, `free_clusters(info)` and `free_stream(img, info)` compute the free-space stream.

```
python3 -I e1_islands.py   <carvex> <repo>/bench/corpus <workdir> 40   # E1-live and E1-dead  → e1.out
python3 -I x1_cluster_rule.py <repo>/bench/corpus                      # X1                   → x1.out
python3 -I x2_png_crc.py   <repo>/bench/corpus <carvex> <workdir>      # X2                   → x2.out
```

Recorded outputs (the numbers quoted in §7.4):

```text
corpus files used (14): jpeg_01_baseline_640x480.jpg, jpeg_02_large_1280x960.jpg, jpeg_04_grayscale_512x512.jpg, jpeg_05_low_quality_1024x768.jpg, jpeg_06_portrait_480x800.jpg, jpeg_08_exif_thumb_900x700.jpg, png_01_rgba_640x480.png, png_02_rgb_800x600.png, png_03_grayscale_512x512.png, png_04_palette_600x400.png, png_06_large_1200x800.png, png_07_small_200x150.png, png_09_alpha_1024x768.png, png_10_palette_320x240.png
== island = LIVE (allocated) file: N=40
   default            exact  0/40 =   0.0%  Wilson95 (0, 0.088)
   fragment-recovery  exact  0/40 =   0.0%  Wilson95 (0, 0.088)
   free-space stream  exact 40/40 = 100.0%  Wilson95 (0.912, 1)
== island = interior of another DELETED file (free): N=40
   default            exact  0/40 =   0.0%  Wilson95 (0, 0.088)
   fragment-recovery  exact  3/40 =   7.5%  Wilson95 (0.026, 0.199)
   free-space stream  exact  0/40 =   0.0%  Wilson95 (0, 0.088)

JPEG baseline entropy clusters               accepted  572/572  
JPEG progressive entropy clusters            accepted  123/141  
PNG corpus clusters                          accepted    0/2293 
PDF pdf_01_image_1page.pdf         DCT=True  accepted   37/38   
PDF pdf_02_image_3pages.pdf        DCT=True  accepted   87/90   
PDF pdf_03_text_2pages.pdf         DCT=False accepted    0/1    
PDF pdf_04_incremental.pdf         DCT=False accepted    0/1    
PDF pdf_05_large_scan.pdf          DCT=True  accepted  151/152  
PDF pdf_06_text_3pages.pdf         DCT=False accepted    0/3    
PDF pdf_07_mixed_4pages.pdf        DCT=True  accepted  111/115  
PDF pdf_08_small_text.pdf          DCT=False accepted    0/1    
PDF pdf_09_text_long.pdf           DCT=False accepted    0/5    
PDF pdf_10_image_portrait.pdf      DCT=True  accepted   42/43   
random bytes 8 MiB                           accepted    0/2047 
zeros 1 MiB                                  accepted    0/255  

CRC oracle, gap = random            : unique+correct 200/200  Wilson95 (0.981, 1)  (0.3s)
CRC oracle, gap = other PNG interior: unique+correct 200/200  Wilson95 (0.981, 1)  (0.3s)
CarveX --fragment-recovery, gap = random            : exact 0/30  Wilson95 (0, 0.114)
CarveX --fragment-recovery, gap = other PNG interior: exact 0/30  Wilson95 (0, 0.114)
```

**`fat32_img.py`**

```python
"""Minimal FAT32 image builder for recovery experiments (stdlib only).

build_fat32(files, deleted) -> (image_bytes, info)
  files   : [(name_8_3, data, [cluster numbers])]  clusters are placed exactly as given (fragmentation on purpose)
  deleted : names whose directory entry is marked 0xE5 and whose FAT chain is zeroed (data bytes stay)
info: cs (bytes per cluster), clusters, fat (list of FAT entries), coff(n) (byte offset of cluster n)
Layout: 512-byte sectors, 8 sectors per cluster (4 KiB), 32 reserved sectors, 2 FATs, root directory = cluster 2."""
import struct


def build_fat32(total_sectors=65536, spc=8, files=(), deleted=()):
    bps, res, nf = 512, 32, 2
    fat_sectors = (total_sectors * 4 + 1024) // 512 + 1
    data_start = res + nf * fat_sectors
    clusters = (total_sectors - data_start) // spc
    img = bytearray(total_sectors * bps)
    b = bytearray(512)
    b[0:3], b[3:11] = b'\xEB\x3C\x90', b'MSWIN4.1'
    struct.pack_into('<HBHBHHBHHHII', b, 11, bps, spc, res, nf, 0, 0, 0xF8, 0, 0, 0, 0, total_sectors)
    struct.pack_into('<I', b, 0x24, fat_sectors)
    struct.pack_into('<I', b, 0x2C, 2)
    b[0x47:0x52], b[0x52:0x5A], b[510:512] = b'CARVEX     ', b'FAT32   ', b'\x55\xAA'
    img[0:512] = b
    fat = [0] * (clusters + 2)
    fat[0], fat[1], fat[2] = 0x0FFFFFF8, 0x0FFFFFFF, 0x0FFFFFFF
    cs = spc * bps

    def coff(c):
        return data_start * bps + (c - 2) * cs

    root, info = bytearray(cs), {}
    for i, (name, data, cl) in enumerate(files):
        assert (len(cl) - 1) * cs < len(data) <= len(cl) * cs, (name, len(data), len(cl))
        for k, c in enumerate(cl):
            piece = data[k * cs:(k + 1) * cs]
            img[coff(c):coff(c) + len(piece)] = piece
            fat[c] = cl[k + 1] if k + 1 < len(cl) else 0x0FFFFFFF
        e = bytearray(32)
        stem, _, ext = name.partition('.')
        e[0:11] = (stem.upper().ljust(8) + ext.upper().ljust(3)).encode()
        e[11] = 0x20
        struct.pack_into('<H', e, 20, cl[0] >> 16)
        struct.pack_into('<H', e, 26, cl[0] & 0xFFFF)
        struct.pack_into('<I', e, 28, len(data))
        if name in deleted:
            e[0] = 0xE5
            for c in cl:
                fat[c] = 0
        root[i * 32:i * 32 + 32] = e
        info[name] = (cl[0], len(data), len(cl))
    img[coff(2):coff(2) + cs] = root
    for k in range(nf):
        o = res * bps + k * fat_sectors * bps
        for i, v in enumerate(fat):
            struct.pack_into('<I', img, o + 4 * i, v)
    return bytes(img), dict(cs=cs, clusters=clusters, fat=fat, coff=coff, files=info)


def free_clusters(info):
    """Cluster numbers whose FAT entry is 0, ascending (what `unallocated` mode may read)."""
    return [n for n in range(3, info['clusters'] + 2) if info['fat'][n] == 0]


def free_stream(img, info):
    """The free clusters concatenated in ascending order: the 'free-space stream' of harness item H-27."""
    cs = info['cs']
    return b''.join(img[info['coff'](n):info['coff'](n) + cs] for n in free_clusters(info))
```

**`e1_islands.py`**

```python
#!/usr/bin/env python3
"""E1 — a deleted file fragmented around another cluster run ("island"); harness item H-27 / H-30.
usage: e1_islands.py CARVEX CORPUS WORKDIR [N=40]
Per trial: one corpus JPEG/PNG (>= 12 clusters) is deleted from a FAT32 image; its clusters are split at a random
cluster boundary and an island of 1..8 clusters sits in between.
  live : the island is a LIVE file (allocated, random bytes)           -> the free-space stream can skip it
  dead : the island is the interior of ANOTHER DELETED file (free)     -> only a content oracle can reject it
Methods: default scan, `--fragment-recovery`, and the free-space stream (free clusters concatenated, raw scan).
Output: exact recall (SHA-256 equal to the original) with Wilson 95 % intervals."""
import glob, hashlib, json, math, os, random, shutil, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fat32_img import build_fat32, free_stream

CARVEX, CORPUS, W = sys.argv[1], sys.argv[2], sys.argv[3]
N = int(sys.argv[4]) if len(sys.argv) > 4 else 40
CL = 4096
os.makedirs(W, exist_ok=True)
sha = lambda b: hashlib.sha256(b).hexdigest()


def carve(img, tag, extra):
    out, rep = f'{W}/o_{tag}', f'{W}/r_{tag}.json'
    shutil.rmtree(out, ignore_errors=True)
    if os.path.exists(rep):
        os.remove(rep)
    subprocess.run([CARVEX, 'scan', img, '-o', out, '--quiet', '--json-report', rep, '--validate', 'decode'] + extra,
                   capture_output=True, timeout=300)
    return json.load(open(rep))['files'] if os.path.exists(rep) else []


def wilson(k, n, z=1.96):
    p = k / n
    c, h = (p + z * z / 2 / n) / (1 + z * z / n), z * math.sqrt(p * (1 - p) / n + z * z / 4 / n / n) / (1 + z * z / n)
    return round(max(0, c - h), 3), round(min(1, c + h), 3)


corp = []
for p in sorted(glob.glob(CORPUS + '/jpeg/*.jpg') + glob.glob(CORPUS + '/png/*.png')):
    n = os.path.basename(p)
    d = open(p, 'rb').read()
    if len(d) >= 12 * CL and not any(w in n for w in ('interlaced', 'progressive', 'cmyk')):
        corp.append((n, d))
print('corpus files used (%d): %s' % (len(corp), ', '.join(n for n, _ in corp)))
random.seed(2026)


def trial(kind, i):
    name, d = random.choice(corp)
    n = -(-len(d) // CL)
    s, g = random.randint(2, n - 2), random.randint(1, 8)
    if kind == 'live':
        isl = random.randbytes(g * CL)
    else:
        _, od = random.choice([x for x in corp if x[0] != name])
        m = -(-len(od) // CL)
        st = random.randint(2, max(2, m - g - 2))
        isl = od[st * CL:(st + g) * CL]
        isl += random.randbytes(g * CL - len(isl))
    cl = list(range(100, 100 + s)) + list(range(100 + s + g, 100 + s + g + n - s))
    files, deleted = [('F.BIN', d, cl)], ['F.BIN']
    if kind == 'live':
        files.append(('LIVE.DAT', isl, list(range(100 + s, 100 + s + g))))
    img, info = build_fat32(files=files, deleted=deleted)
    if kind == 'dead':                       # island clusters are free: written but never allocated in the FAT
        b = bytearray(img)
        for j in range(g):
            o = info['coff'](100 + s + j)
            b[o:o + CL] = isl[j * CL:(j + 1) * CL]
        img = bytes(b)
    p, sp = f'{W}/{kind}{i}.img', f'{W}/{kind}{i}.stream'
    open(p, 'wb').write(img)
    open(sp, 'wb').write(free_stream(img, info))
    res = {}
    for tag, pp, ex in (('default', p, []), ('fragment-recovery', p, ['--fragment-recovery']),
                        ('free-space stream', sp, ['--mode', 'raw', '--no-fs'])):
        res[tag] = any(f['sha256'] == sha(d) for f in carve(pp, f'{kind}{i}{tag[:4]}', ex))
    return res


for kind, label in (('live', 'island = LIVE (allocated) file'), ('dead', 'island = interior of another DELETED file (free)')):
    tot = {}
    for i in range(N):
        for k, v in trial(kind, i).items():
            tot[k] = tot.get(k, 0) + v
    print(f'== {label}: N={N}')
    for k, v in tot.items():
        print(f'   {k:18s} exact {v:2d}/{N} = {100 * v / N:5.1f}%  Wilson95 {wilson(v, N)}')
```

**`x1_cluster_rule.py`**

```python
#!/usr/bin/env python3
"""X1 — can a 4 KiB cluster be the middle of JPEG entropy data?  (harness item H-32)
Rule: every 0xFF byte must be followed by 0x00 or RSTn (D0..D7) (T.81 B.1.1.5 byte stuffing), AND order-0 entropy >= 7.0 bit/byte.
usage: x1_cluster_rule.py CORPUS"""
import collections, glob, math, os, random, struct, sys
C, CL = sys.argv[1], 4096


def stuffing_ok(buf, i):
    seg = buf[i:i + CL + 1]                       # peek one byte into the next cluster
    j = seg.find(b'\xff')
    while j != -1 and j < CL:
        if j + 1 < len(seg) and seg[j + 1] not in (0, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7):
            return False
        j = seg.find(b'\xff', j + 1)
    return True


def entropy(seg):
    c, n = collections.Counter(seg), len(seg)
    return -sum(v / n * math.log2(v / n) for v in c.values())


accept = lambda b, i: stuffing_ok(b, i) and entropy(b[i:i + CL]) >= 7.0


def scan_start(b):
    pos = 2
    while True:
        m, ln = b[pos + 1], struct.unpack('>H', b[pos + 2:pos + 4])[0]
        pos += 2 + ln
        if m == 0xDA:
            return pos


def rate(name, blobs, start=lambda b: 0, end=lambda b: len(b) - CL - 1):
    n = k = 0
    for b in blobs:
        for i in range(-(-start(b) // CL) * CL, end(b), CL):
            n += 1
            k += accept(b, i)
    print('%-44s accepted %4d/%-5d' % (name, k, n))


rd = lambda pat: [open(p, 'rb').read() for p in sorted(glob.glob(C + pat))]
base = [b for b in rd('/jpeg/*.jpg') if b'\xff\xc2' not in b[:2000]]
prog = [b for b in rd('/jpeg/*.jpg') if b'\xff\xc2' in b[:2000]]
rate('JPEG baseline entropy clusters', base, scan_start, lambda b: b.rindex(b'\xff\xd9') - CL)
rate('JPEG progressive entropy clusters', prog, scan_start, lambda b: b.rindex(b'\xff\xd9') - CL)
rate('PNG corpus clusters', rd('/png/*'))
for p in sorted(glob.glob(C + '/pdf/*')):
    b = open(p, 'rb').read()
    rate('PDF %-30s DCT=%s' % (os.path.basename(p)[:30], b'/DCTDecode' in b), [b])
random.seed(1)
rate('random bytes 8 MiB', [random.randbytes(8 << 20)])
rate('zeros 1 MiB', [bytes(1 << 20)])
```

**`x2_png_crc.py`**

```python
#!/usr/bin/env python3
"""X2 — oracle-guided bifragment gap carving for PNG with the chunk CRC-32 as the oracle (harness item H-30).
usage: x2_png_crc.py CORPUS [CARVEX WORKDIR]
The solver walks the chunks of a PNG laid out on a 'disk'; at the first chunk whose CRC fails it tries every cluster-aligned
break k inside that chunk and every gap g in 1..16 clusters and keeps the stitches whose CRC-32 matches.
Two gap kinds, N=200 each: seeded random bytes, and the interior of another PNG (compressed data).
With CARVEX the same layouts (N=30) go through `scan --fragment-recovery` as a deleted FAT32 file, for comparison."""
import glob, hashlib, math, os, random, struct, sys, time, zlib
C, CL = sys.argv[1], 4096
CARVEX = sys.argv[2] if len(sys.argv) > 2 else None
W = sys.argv[3] if len(sys.argv) > 3 else '/tmp/x2'
pngs = [(os.path.basename(p), open(p, 'rb').read()) for p in sorted(glob.glob(C + '/png/*.png'))]
pngs = [(n, d) for n, d in pngs if len(d) >= 12 * CL]


def crc_ok(buf, pos, L):
    return zlib.crc32(buf[pos + 4:pos + 8 + L]) & 0xFFFFFFFF == struct.unpack('>I', buf[pos + 8 + L:pos + 12 + L])[0]


def solve(disk, maxgap=16):
    """Returns the list of (k, g) stitches whose chunk CRC matches ([] = nothing found / file intact)."""
    pos = 8
    while pos + 12 <= len(disk):
        L, = struct.unpack('>I', disk[pos:pos + 4])
        end = pos + 12 + L
        if end > len(disk):
            return []
        if crc_ok(disk, pos, L):
            if disk[pos + 4:pos + 8] == b'IEND':
                return []
            pos = end
            continue
        hits = []
        for k in range(-(-(pos + 1) // CL), (end - 1) // CL + 1):          # cluster-aligned breaks inside the chunk
            head = disk[pos:k * CL]
            for g in range(1, maxgap + 1):
                t0, need = (k + g) * CL, 12 + L - len(head)
                if t0 + need <= len(disk) and crc_ok(head + disk[t0:t0 + need], 0, L):
                    hits.append((k, g))
        return hits
    return []


def wilson(k, n, z=1.96):
    p = k / n
    c, h = (p + z * z / 2 / n) / (1 + z * z / n), z * math.sqrt(p * (1 - p) / n + z * z / 4 / n / n) / (1 + z * z / n)
    return round(max(0, c - h), 3), round(min(1, c + h), 3)


def layout(kind, rnd):
    name, d = rnd.choice(pngs)
    n = -(-len(d) // CL)
    s, g = rnd.randint(1, n - 2), rnd.randint(1, 16)
    if kind == 'random':
        isl = rnd.randbytes(g * CL)
    else:
        _, od = rnd.choice([x for x in pngs if x[0] != name])
        m = -(-len(od) // CL)
        st = rnd.randint(1, max(1, m - g - 1))
        isl = od[st * CL:(st + g) * CL]
        isl += rnd.randbytes(g * CL - len(isl))
    return d, n, s, g, isl


rnd = random.Random(11)
for kind in ('random', 'other PNG interior'):
    N, ok, t0 = 200, 0, time.time()
    for _ in range(N):
        d, n, s, g, isl = layout(kind, rnd)
        ok += solve(d[:s * CL] + isl + d[s * CL:]) == [(s, g)]
    print(f'CRC oracle, gap = {kind:18s}: unique+correct {ok}/{N}  Wilson95 {wilson(ok, N)}  ({time.time() - t0:.1f}s)')
if CARVEX:
    import json, shutil, subprocess
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from fat32_img import build_fat32
    os.makedirs(W, exist_ok=True)
    rnd = random.Random(11)
    for kind in ('random', 'other PNG interior'):
        N, ok = 30, 0
        for i in range(N):
            d, n, s, g, isl = layout(kind, rnd)
            cl = list(range(100, 100 + s)) + list(range(100 + s + g, 100 + s + g + n - s))
            img, info = build_fat32(total_sectors=131072, files=[('F.PNG', d, cl)], deleted=['F.PNG'])
            b = bytearray(img)
            for j in range(g):
                o = info['coff'](100 + s + j)
                b[o:o + CL] = isl[j * CL:(j + 1) * CL]
            open(f'{W}/x2.img', 'wb').write(bytes(b))
            shutil.rmtree(f'{W}/out', ignore_errors=True)
            subprocess.run([CARVEX, 'scan', f'{W}/x2.img', '-o', f'{W}/out', '--quiet', '--json-report', f'{W}/x2.json',
                            '--validate', 'decode', '--fragment-recovery'], capture_output=True, timeout=300)
            ok += any(f['sha256'] == hashlib.sha256(d).hexdigest() for f in json.load(open(f'{W}/x2.json'))['files'])
        print(f'CarveX --fragment-recovery, gap = {kind:18s}: exact {ok}/{N}  Wilson95 {wilson(ok, N)}')
```

---

*End of harness.*

---

## Appendix D — ID map: this file ↔ the repository's own numbering (`f5bda4a`)

The repository's `docs/PROGRESS.md` / `harness.md` use `repo H-01 … H-34`, `R2-01 … R2-11` and the repro scenarios `X01 … X24` (`scripts/review_repro.py`). "Repo status" is what the repository records; "This file" is the status verified here (§1.6; the `ab725a6` status is §1.5).

| This file | Repo ID(s) | Repro scenario(s) in `scripts/review_repro.py` (result on `f5bda4a`; identical on `ab725a6`) | Repo status | This file |
|---|---|---|---|---|
| H-01 validity model | repo H-16 | X24 PASS | CLOSED | FIXED |
| H-02 PNG CRC | repo H-17 | X10 PASS | CLOSED | FIXED (`BELOW_MIN_SCORE`, exit 0) |
| H-03 PDF verification | repo H-19 | X11 PASS | CLOSED | FIXED |
| H-04 `--repair` | repo H-21 | X14 PASS | CLOSED (D-68: footer repair only) | PARTLY (PDF done; PNG/JPEG mid-stream cuts stay `partial` — by the repository's narrower reading of Q11, §9 Q12; JPEG in `--validate structure` is still `repaired` with grey fill → item 0) |
| H-05 container allocations / logical size | repo H-11, H-12 (R2-01) | X07 PASS, X08 SKIP (needs RSS harness) | CLOSED | FIXED |
| H-06 JPEG SOF | repo H-06 | X03 PASS | CLOSED | FIXED |
| H-07 read errors / E01 | repo H-22 | X16 PASS | CLOSED | FIXED |
| H-08 VMDK grain zeros | repo H-23 | X17 SKIP (no `qemu-img`) | CLOSED | FIXED |
| H-09 JPEG deep validation | repo H-18 (R2-03) | X12 PASS | PARTIAL (accepted for 0.3.1) | PARTLY (fall-through fixed; 3/4,000 splices accepted, Wilson upper 0.22 %) |
| H-10 PDF EOL | repo H-20 | X13 PASS | CLOSED | FIXED |
| H-11 PDF EOF O(E×N) | repo H-07 | X04 PASS | CLOSED | FIXED |
| H-12 budgets | repo H-08, H-09 | X01 PASS, X05 PASS | CLOSED | FIXED |
| H-13 panic boundary | repo H-10 | X06 PASS | CLOSED | FIXED (static) |
| H-14 SIGINT | repo H-28 | X21 PASS | CLOSED | FIXED |
| H-15 VMDK paths | repo H-13 | X09 PASS | CLOSED | FIXED |
| H-16 auto-detect | repo H-24 | X18 PASS | CLOSED | FIXED |
| H-17 B-06 | repo H-29 (D-71) | X15 PASS | CLOSED (precision over recall) | FIXED by decision (zero gap 3/3; random 0/3; header-at-gap-start 2/3) |
| H-18 regions / unparsable volume | repo H-26, H-27 | X19 SKIP, X20 SKIP (no `mkfs`) | CLOSED | FIXED |
| H-19 gates / fuzz | repo H-14, H-30 | — | CLOSED | FIXED (static) |
| H-20 container / stdin report | repo H-25 | X22 PASS | CLOSED | FIXED |
| H-21 release identity | repo H-04 (R2-04, R2-10) | — | PARTIAL | FIXED |
| H-22 A-10 limits | repo H-34 (R2-07) | — | CLOSED | FIXED (`allow` ×0; `fn_len.py` 3 informational) |
| H-23 docs honesty | repo H-01, H-31, H-32, H-33 | — | CLOSED | FIXED (148 = 148; no `/tmp/opencode` evidence) |
| H-24 benchmark wording | repo H-33 | — | CLOSED | NOT RE-RUN |
| H-25 truncated-file end | — (no repo item; D-70) | — | — | FIXED (last-verified-byte end; residue ≤ 424 B) |
| H-26 gate without root | repo H-02 | whole script: 34 scenarios, 0 failures, 6 SKIP | CLOSED | FIXED |
| H-27 free-space stream | — (new, Part T) | — | — | NEW |
| H-28 FAT deleted entries | — (guide B-01; not scheduled in the repository) | — | — | NEW |
| H-29 NTFS deleted MFT records | — (guide B-01) | — | — | NEW |
| H-30 oracle-guided bifragment (PNG CRC) | — (extends repo H-29 / B-06) | — | — | NEW |
| H-31 PDF xref-guided reassembly | — | — | — | NEW (design only) |
| H-32 JPEG join (filter, seam, RST) | — | — | — | NEW (experiment) |
| H-33 exFAT / FAT12/16, ZIP/OOXML | — (guide B-03, B-10) | — | — | NEW |

Repo items without a counterpart here: repo H-03 (the X01–X24 red controls themselves), repo H-05 (shared validator-memory flag `--max-validator-memory`, alias `--max-decode-bytes` → §2), repo H-15 / R2-02 (per-worker scratch reuse; performance only → §8 flood guard, X23 PASS), repo H-30 (gate efficacy → H-19), repo H-31/H-32/H-33 (status labels, evidence manifest, claims → H-23/H-24). Repro scenarios R1–R8 are the original review reproductions (R7, R8 SKIP without images). Part T (H-27 … H-33) has no repository counterpart: B-01, B-03, B-06 and B-10 are `guide.md` roadmap items; the repository decided only B-06's scope (D-71) and has not scheduled the others.
