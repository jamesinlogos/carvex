# TECHNICAL REPORT

## CarveX — A FAT32/NTFS-Aware JPEG/PNG/PDF File Recovery Tool

| | |
|---|---|
| **Tool name** | CarveX |
| **Version** | 0.2.0 (benchmark figures taken from the 0.1.0 run — carving behavior unchanged, see `docs/DECISIONS.md` D-16) |
| **Language** | Rust 2021 |
| **Platforms** | Linux x86_64 (static musl), Windows x86_64 (GNU) |
| **Source** | `$CARVEX` (repository checkout, e.g. `~/carvex`) |
| **Completion date** | 19/11/2026 |

---

## Abstract

CarveX is a command-line tool for recovering deleted files from FAT32 and NTFS disk images. Rather than merely searching for headers and footers, CarveX validates the internal structure of each JPEG, PNG, and PDF candidate, scores its structural quality, and writes only files that pass the threshold. Its core differentiator is the ability to read filesystem allocation metadata — the FAT32 table or NTFS's `$Bitmap` — to restrict scanning to unallocated clusters. The published benchmark uses a single raw image. An additive read-only source layer can also open split raw, E01, VHD, VMDK and stdin inputs without changing that raw path, the benchmark, or its figures.

On a benchmark suite of 4 64 MiB images (FAT32/NTFS, an all-files-deleted scenario and a mixed deleted/kept-files scenario), CarveX in `unallocated` mode correctly recovers **100/100** deleted files with **0 false positives**, achieving precision, recall, and F1 of **100%** under both the exact and usable measures. This is **100 data points on 30 unique files** (30 files × 4 contexts), not n = 100 independent samples: by the 95% Wilson confidence interval, 100/100 gives [96.3%; 100%] and 30/30 gives [88.6%; 100%] — "100%" holds only under the controlled conditions (contiguous files, no fragmentation, no overwriting, clean free space, 512 B FAT32 clusters, 64 MiB images), not as a guarantee for real-world images (see §6.4). In whole-image `raw` scan mode, the tool still achieves 100% recall but produces 20 false positives — exactly equal to the 20 files still kept in the two mixed images — demonstrating the quantitative value of filesystem awareness. The comparison tools Foremost 1.5.7, Scalpel 1.60, and PhotoRec 7.2 achieve usable F1 of 73.44%, 69.17%, and 87.85%, respectively; CarveX achieves 100% under the same conditions.

This report presents the tool's theoretical background, design, experimental methodology, detailed results, evaluation, and limitations.

---

## 1. Introduction

### 1.1. Background

Data recovery is an important problem in digital investigation and data salvage. When a file is deleted on FAT32 or NTFS, its content usually remains in unallocated clusters until it is overwritten. Traditional carving tools search for a file format's signature (magic bytes) and cut data from header to footer. This approach is simple but has three drawbacks:

1. **False positives:** a byte sequence that coincidentally resembles a header may be treated as a file.
2. **False positives from allocated space:** scanning the whole image also recovers files that were never deleted.
3. **Inaccurate file boundaries:** a false footer in embedded data (e.g. an EXIF thumbnail, `%%EOF` inside a PDF stream) can cut the file at the wrong place.

CarveX was built to address these three problems for three common formats — JPEG, PNG, and PDF — on two filesystems, FAT32 and NTFS.

### 1.2. Objectives

- Provide a stable Rust CLI that only reads the source image and supports JPEG/PNG/PDF.
- Detect FAT32/NTFS and accurately extract unallocated regions.
- Validate file structure rather than only matching header/footer.
- Parallel scanning, with JSON/CSV reports and statistics.
- Provide Linux/Windows binaries and a reproducible benchmark suite against Foremost, Scalpel, and PhotoRec.
- Provide documentation and instructions for testing on CFReDS data.

### 1.3. Scope

**In scope:**

- Raw/DD disk images, partition images, superfloppies.
- MBR and GPT.
- FAT32, NTFS.
- JPEG (baseline, progressive, EXIF), PNG, PDF.
- Recovery based on remaining content; original names/paths are not restored.

**Out of scope:**

- EWF2/Ex01, encrypted E01, L01/Lx01, AFF, differencing VHD, ESX/COWD and
  compressed streamOptimized VMDK (classic E01, split raw, fixed/dynamic VHD and
  flat/sparse VMDK are supported read-only; see §6.4 item 10).
- FAT12/FAT16, exFAT, ext4, APFS, HFS+.
- Reassembling fragmented files based on the deleted file's runlist. The
  opt-in BGC stage in §3.6 joins only two cluster-aligned fragments and does not
  use metadata.
- Decoding encrypted JPEG/PDF content.
- Graphical user interface.

### 1.4. Related Work and CarveX's Position

Carving methods are classified according to [13] and [16] into the following main branches:

1. **Pure header/footer:** cut from the first signature to the last signature, without inspecting the content. This is the principle behind Foremost [8] and Scalpel [12].
2. **Header + size limit (header/max-size):** add size and candidate-count thresholds to block "coincidentally matching" byte sequences.
3. **Structure/validation-based:** a second step checks the candidate's internal structure. Garfinkel [11] demonstrated that header/footer should be used only to *reject* candidates, that every candidate passing this gate must be re-confirmed by a slower validator, and described Bifragment Gap Carving for two-fragment files; Cohen [15] systematized advanced carving techniques (fragmentation, missing fragments).
4. **Content-based:** use statistical properties of encoded data to identify fragments — for example, matching bit sequences against the Huffman table to recover JPEG images with missing fragments [14].
5. **Filesystem-aware / metadata-assisted:** read the FAT table or `$Bitmap` to scan only unallocated regions, and use deleted-file metadata (MFT, directory entries) when available. This is the direction of metadata-based recovery tools (TSK `fls`/`icat`, `ntfsundelete`) and is where CarveX focuses.

**Feature comparison with reference tools:**

**Table 1.** Feature comparison with reference tools

| Feature | Foremost 1.5.7 | Scalpel 1.60 | PhotoRec 7.2 | TSK / ntfsundelete | CarveX |
|---|---|---|---|---|---|
| Branch (1) header/footer | yes | yes | yes | — | yes |
| Branch (2) max-size/threshold | yes | yes | yes | — | yes |
| Branch (3) structure checking | no (built-in) | no (built-in) | yes (dedicated validator) | metadata-based | yes (JPEG/PNG/PDF) |
| FAT32/NTFS awareness, scan only free regions | no | no | freespace/wholespace (by region) | yes (metadata) | yes (FAT/$Bitmap) |
| Suppress images embedded in PDF files (nested suppression) | no | no | no | no | yes (default) |
| Fully configurable JSON/CSV reports | audit txt | — | text report | raw CLI | schema v2 + JSON/CSV |

Categories (3)–(5) explain the results in §5: CarveX sits in branch 3+5 (structure checking *and* filesystem awareness), while Foremost/Scalpel sit in branch 1 and PhotoRec in branch 3 but scans the entire space.

---

## 2. Theoretical Background

### 2.1. File carving

File carving is the process of reconstructing files from raw data without relying on filesystem metadata. Two main families of methods:

- **Header/footer carving:** find the opening signature, then the closing signature. Fast but prone to boundary errors.
- **Structure-based carving:** parse the format to determine the endpoint and validity precisely.

CarveX uses a hybrid approach: locate headers with high-speed byte searching, then run a structural parser on each candidate. The parser both determines the real footer and assigns a confidence score.

### 2.2. FAT32

FAT32 consists of the following main regions:

**Table 2.** Main regions of FAT32

| Region | Contents |
|---|---|
| Reserved | Boot sector (BPB), typically 32 sectors |
| FAT #1, FAT #2 | Cluster linkage table, 32 bits per entry |
| Data | Clusters containing data and directories |

CarveX reads the BPB fields at offsets `0x0B`–`0x2F`: bytes/sector, sectors/cluster, reserved sectors, number of FAT tables, total sectors, FAT size, root cluster. Then:

1. Compute `cluster_size = bytes_per_sector × sectors_per_cluster`.
2. Determine that the data region begins after `reserved + num_fats × fat_size`.
3. Read the first FAT table with `(total_clusters + 2) × 4` bytes.
4. Iterate over clusters starting from 2; entry `0` denotes a free cluster.
5. Merge consecutive free clusters into **physical extents**.

CarveX does not read directory entries, does not restore filenames, and does not rely on the deleted file's FAT chain (which is usually erased when the file is deleted).

### 2.3. NTFS

NTFS stores metadata in MFT (Master File Table) records. CarveX:

1. Reads the boot sector, obtaining bytes/sector, sectors/cluster, total sectors, `$MFT` LCN, clusters per MFT record.
2. Reads MFT record 0, applying the **update sequence array fixup** to restore the last sector.
3. Parses the unnamed `$DATA` attribute of `$MFT` to obtain its runlist.
4. Reads MFT record 6 (`$Bitmap`) via the runlist, whether resident or non-resident.
5. Each bit in `$Bitmap` corresponds to a cluster: `1` = allocated, `0` = free.
6. Merges consecutive free clusters into extents.

`$Bitmap` is NTFS's authoritative source of allocation information, more suitable than inferring from file sizes or directory entries.

### 2.4. JPEG

A JPEG is a sequence of markers:

```text
FF D8 (SOI) → segments → FF DA (SOS) → entropy-coded data → FF D9 (EOI)
```

Points to handle:

- **Marker stuffing:** in entropy data, `FF 00` represents the byte `FF`, not a marker.
- **Restart markers:** `FF D0`–`FF D7` appear in entropy data.
- **Progressive JPEG:** has multiple DHT/DQT/SOS pairs; must not stop at the first SOS.
- **EXIF thumbnails:** the embedded image has its own SOI/EOI; looking only for the first `FF D9` truncates the main image.
- **SOF:** gives the image dimensions; must appear before EOI.

CarveX walks the markers sequentially, skips entropy data correctly, and confirms SOF/SOS before accepting EOI.

### 2.5. PNG

PNG starts with a fixed 8-byte signature:

```text
89 50 4E 47 0D 0A 1A 0A
```

Each chunk:

```text
[4 byte length][4 byte type][length byte data][4 byte CRC-32]
```

Required chunks:

- `IHDR`: the first chunk, containing dimensions, bit depth, color type, compression, filter, interlace.
- `IDAT`: compressed image data.
- `IEND`: the final chunk, length = 0.

CarveX checks chunk ordering, IHDR validity, the presence of PLTE for indexed images, and each chunk's CRC-32. A bad CRC does not immediately reject the file but incurs a score penalty, since partial overwriting may damage only one chunk.

### 2.6. PDF

A PDF consists of a `%PDF-1.x` header, objects, an xref table or xref stream, a trailer, and a `startxref` pointing to the xref. A file may have multiple **incremental updates**, each ending with a `%%EOF`.

Challenges:

- `%%EOF` may appear inside a stream or comment.
- Multiple `%%EOF` markers in a multi-revision file; the last revision must be chosen.
- `startxref` must point to a valid `xref` or object stream.
- A `%PDF-` header may appear inside other file content.

CarveX counts objects, checks `/Root`, finds `startxref` near `%%EOF`, and validates the target. `smart` mode selects the last `%%EOF` with a valid `startxref`; if there is none, it selects the last `%%EOF`. Decode mode additionally runs the bounded structural validator described in §3.4.1: it follows classic xref tables and xref streams, verifies object headers, the catalog reference, `/Size`, `/Prev` and `/Encrypt`, and checks direct stream lengths against `endstream`. It does not render page content.

---

## 3. System Design

### 3.1. Overall architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                        carvex CLI                           │
│  info            scan             signatures                │
└───────────────┬──────────────────────────┬──────────────────┘
                │                          │
                ▼                          ▼
┌──────────────────────────┐   ┌──────────────────────────────┐
│  fs::detect              │   │  scan::run_scan              │
│  - superfloppy           │   │  - select extents            │
│  - MBR / GPT             │   │  - split tasks, parallel scan│
│  - FAT32 / NTFS probe    │   │  - carve + validate          │
│  - free extents          │   │  - filter, write files, stats│
└──────────────┬───────────┘   └──────────────┬───────────────┘
               │                              │
               ▼                              ▼
┌──────────────────────────┐   ┌──────────────────────────────┐
│ fs/fat32.rs  fs/ntfs.rs  │   │ signatures/jpeg|png|pdf.rs   │
│ read FAT / $Bitmap       │   │ parser + score + repair      │
└──────────────────────────┘   └──────────────────────────────┘
```

Modules:

**Table 3.** CarveX modules

| File | Responsibility |
|---|---|
| `src/main.rs` | Entry point, error handling, and exit code |
| `src/cli.rs` | clap definitions, option validation, invokes `info`/`scan` |
| `src/io.rs` | `ImageSource` trait and the default read-only `RawFile` |
| `src/image/*` | Read-only split raw, E01, VHD, VMDK and stdin sources; `open_image()` selection |
| `src/fs/mod.rs` | Filesystem detection, MBR/GPT, `FsInfo`, print information |
| `src/fs/fat32.rs` | Parse BPB, FAT, create free extents |
| `src/fs/ntfs.rs` | Parse boot sector, MFT, runlist, `$Bitmap` |
| `src/scan.rs` | Parallel scan engine, carving, filtering, nested suppression, reporting |
| `src/signatures/*` | Per-format parsers and validators |
| `src/report.rs` | JSON/CSV output |
| `src/util.rs` | File reading, extents, SHA-256, size parsing |

### 3.2. Filesystem detection

Order:

1. If `--offset` is present, probe that offset directly.
2. Probe offset 0 (superfloppy or single-partition image).
3. Read the MBR; if partition type `0xEE` is present, try GPT first.
4. For each partition, probe FAT32/NTFS.
5. If `--partition` is not specified, select **all** valid volumes and also scan
   gaps/unsupported partitions (A-07 step 2); `--partition largest --no-gaps`
   restores the 0.1.0 behavior of selecting the largest volume.
6. If nothing is found and filesystem mode is not required, fall back to a raw scan.

The FAT32 probe relies on:

- Signature `0x55AA` at offset 510;
- String `FAT32` at offset `0x52`, or characteristic BPB fields (`sectors_per_cluster != 0`, `reserved != 0`, `root_entries = 0`, `total16 = 0`, `fat_size32 > 0`).

The NTFS probe relies on the string `NTFS` at offset 3 and the `0x55AA` signature.

### 3.3. Extent-based scanning

For each extent `[start, end)`, the engine splits it into tasks of size `--chunk-size` (default 4 MiB). Each task reads an extra 16 bytes to catch headers straddling a boundary. Tasks run in parallel via Rayon.

Each candidate stores:

```text
offset       : absolute header position
extent_end   : end of the containing extent
kind         : JPEG / PNG / PDF
```

The `extent_end` limit prevents the parser from reading into allocated clusters. This is important: a deleted file in free space may be truncated if the next cluster is allocated; CarveX records a truncated file rather than mixing in another file's data.

### 3.4. Carving and buffer escalation

For each candidate:

1. Read up to `min(extent_end - offset, --max-size)`, starting at 1 MiB.
2. Invoke the parser.
3. If the parser returns `Need::More`, double the buffer (up to 64 times) and retry.
4. If the parser returns `Analyzed`, check the score, size, and `--complete-only`.
5. If the parser returns `Reject`, discard the candidate.

This mechanism avoids reading the full 64 MiB for every small candidate while still handling large files.

Work and retained memory are bounded independently of the image. Before a carve
buffer is allocated or grown, the scanner reserves its bytes from a shared
`--max-memory` budget; carved and stitched results stay charged while retained
and are released when they are filtered out. A shared counter stops candidate collection as soon as
`--max-candidates` (default 1,000,000) is exceeded, and `--time-budget` aborts
after the configured wall-clock budget. These checks can stop a hostile scan, but
`--max-memory` is not a process-RSS cap: it covers carve buffers, retained
outputs and B-06 stitch buffers, while JPEG/PDF decoder internals are not fully
instrumented.

`--validate decode` uses format-specific validators. For PNG, the validator
checks the expected filtered-scanline size derived from IHDR against
`--max-decode-bytes` (default 256 MiB) as soon as IHDR is available. If the
declared image exceeds that bound, the candidate is downgraded to
`validation=partial` before any IDAT data is inflated. Otherwise, IDAT payloads
are inflated incrementally through `flate2::Decompress` into a fixed 64 KiB
buffer; filter bytes are checked as rows arrive, Adam7 passes are tracked
without an offset vector, and the output length must equal the declared size
exactly. The full compressed IDAT payload, decoded image and row-offset vector
are never materialized. JPEG and PDF use their own bounded checks and are not
covered by the PNG-specific inflation bound; neither renders payload content.

#### 3.4.1. Validity labels (ADR-H1)

Every file record carries an authoritative `validity` field and a `checks[]`
array whose entries are `{name, ok, note}`; `ok` is `true`, `false` or `null`
(not performed or unsupported). The legacy `validation`, `status`, `source`
and score fields keep their historical meanings and are not altered by this
model:

| `validity` | Legacy `validation` | Legacy `status` | Meaning |
|---|---|---|---|
| `verified` | `decoded` | `complete` | every mandatory check for the type was implemented and passed |
| `structural` | `structure` or `unverified` | `complete` | structure parses; content not checked |
| `suspect` | `partial` | `complete` | footer/structure found but at least one integrity check failed; the file is still written |
| `partial` | `partial` | `truncated` | truncated, salvaged or prefix-only |
| `repaired` | `partial`/`decoded` | `repaired` | repair applied and the result re-validates |

A `repaired` row is produced only after the reconstructed bytes pass
revalidation (§3.8); an unvalidated suffix is not represented by this label.
The optional `sha256_original` field records the source bytes when repair
changed the output, so the repaired and original hashes remain distinguishable.
Only `verified` may be presented as complete or verified in a summary, count,
console line or benchmark. Default `--validate structure` output is therefore
`structural` and is printed as **content not checked**, even when its legacy
status is `complete` and its score is 100. `validated_bytes` counts only bytes
verified contiguously from offset zero; it is zero when no content check ran.
For PNG, every chunk-CRC mismatch adds a `chunk_crc` check naming the chunk
type and byte offset and sets `validation=partial`/`validity=suspect` without
changing the score or suppressing the file. Decode mode recomputes all CRCs
and reports the contiguous prefix ending at the last good chunk; structure
mode reports zero decoded bytes. For JPEG, unsupported strict-scan variants
are `unverified`/`structural`; a baseline tail swap between files with
identical headers can still be self-consistent and receive `verified`. The
measured residual rate and confidence interval are disclosed in
`docs/baseline/jpeg_splice_v0.2.0.txt` (D-45). An output intersecting an
unreadable source range is at best `suspect` and carries
`overlaps_unreadable_region: false`.

For PDF decode mode, the validator records `startxref_target`, `xref_entries`,
`xref_stream`, `stream_length`, `prev_chain`, `xref_size`, `encrypt` and
`root_catalog` checks. Every implemented check passing gives `verified`; a
mismatch gives `suspect` with the failing check named, and the file is still
written with its historical score. A check that is unsupported or exceeds the
validator budget is recorded with `ok: null` and gives `structural`, not
`suspect`; the compressed catalog path resolves `/ObjStm` entries. Because the
checks visit scattered offsets rather than a contiguous decoded payload,
`validated_bytes` remains zero for PDF even when `validation=decoded`. The
10-file PDF corpus all reached `verified`/100 with no suspect result. External
`qpdf`/`pdftoppm` oracles were unavailable, so that control matrix is SKIPPED;
`pypdf` 6.9.2 opened generated fixtures for H-19 and was used as the X14
non-strict oracle for a real corpus PDF cut at 70%; `qpdf`/`pdftoppm` remain
unavailable.

### 3.5. Scoring

Scores are used to filter junk candidates. The default threshold is 50.

**JPEG:**

**Table 4.** JPEG structure-check scoring scale

| Status | Score |
|---|---:|
| Complete, with SOF + SOS + EOI | 100 |
| Truncated but with SOF + SOS | 60 |
| Truncated, SOF only | 35 |
| Truncated, no SOF yet | 10 |

**PNG:**

**Table 5.** PNG structure-check scoring scale

| Status | Score |
|---|---:|
| Complete | 100 − 15 × number of chunks with bad CRC (max 45) |
| Truncated, with IHDR + IDAT | 60 |
| Truncated, IHDR only | 40 |
| Truncated, no IHDR yet | 10 |

**PDF:**

**Table 6.** PDF structure-check scoring scale

| Component | Score |
|---|---:|
| Base score when objects are present | 60 |
| `startxref` points to valid xref/object | +20 |
| `startxref` present but target invalid | +5 |
| `/Root` present | +10 |
| xref or `/XRef` present | +10 |
| Truncated PDF with objects | 60 (+5 if xref present) |

### 3.6. Bounded bifragment gap carving (B-06)

Bifragment Gap Carving (BGC) is available as an opt-in stage enabled by
`--fragment-recovery`. It is not part of the default scan and was not used for
the published benchmark. The stage works only inside a detected FAT32/NTFS
volume, where cluster boundaries supply the fragment geometry. A header
candidate is treated as the first fragment; a later cluster-aligned offset is
treated as the start of the second fragment, which ends at a format footer.
Candidate joins are limited by `--max-gap` (clusters),
`--max-frag-candidates` (joins evaluated) and `--frag-time-limit`
(milliseconds), and are tried in order of increasing gap. Reads are confined to
scanned extents.

A join is accepted only when the stitched byte sequence passes the same deep
validator used by `--validate decode`. This prevents a structural splice from
being reported as verified without content validation. Join buffers and
accepted stitches are charged to the same `--max-memory` budget, so budget
exhaustion aborts the scan instead of silently continuing. Each accepted record
has `source="bgc"`, exactly two `fragments[]` entries, a `checks` count and a
`confidence` score. These JSON fields are optional and omitted when B-06 is
disabled; the CSV columns are unchanged. Bounded warnings are
`FRAGMENT_RECOVERY_SKIPPED`, `FRAGMENT_FOOTER_SCAN_FAILED`,
`FRAGMENT_CANDIDATE_LIMIT` and `FRAGMENT_TIME_LIMIT`.

The S12 acceptance case is an interleaved `[A1][B1][A2][B2]` FAT32 layout.
Generated fixtures for JPEG, PNG and PDF recover both source files
byte-exactly, including the PDF trailing line ending. This is a controlled
correctness test, not a recall measurement: no fragmented-profile recall,
splice rate or confidence interval has been measured, and no fragment count
above two or metadata/runlist-assisted reassembly is implemented.

### 3.7. Filtering and deduplication

After carving:

- **`--min-size`:** discard files that are too small.
- **`--min-score`:** discard weak structures.
- **`--complete-only`:** discard files with no footer found.
- **`--dedupe-hashes`:** keep the first record for each SHA-256.
- **Nested suppression:** discard candidates entirely contained within a larger complete file. For example, a JPEG image embedded in a PDF, or a PDF embedded in a PDF.
- **A candidate file is written only once** per `(offset, type)` pair.

Nested suppression applies only when the parent file has `status = complete` and a `carved_size` larger than the child. The parent ends at the parsed footer rather than extending to the end of the cluster, so it does not swallow a physically subsequent file.

### 3.8. Repairing truncated files

With `--repair`, CarveX first reconstructs candidate suffix bytes and then
revalidates the complete result:

- Truncated JPEG: append `FF D9`.
- Truncated PNG: append the real length-zero IEND chunk
  (`00 00 00 00 49 45 4E 44 AE 42 60 82`).
- Truncated PDF: scan the surviving prefix for `N G obj` headers and `/Root`
  or `/Type /Catalog`, rebuild a bounded classic xref table and trailer with
  `startxref` pointing at the rebuilt table, then append `%%EOF`.

Only a result that passes revalidation is labelled `repaired`. For PDF this
means the H-19 structural validator passes even in default `structure` mode; a
bare `%%EOF` without a resolvable xref/catalog is not sufficient. Revalidation
does not change the original score or `carved_size`. If it fails, no suffix is
written: the row remains `truncated`/`partial`, its bytes are unchanged, and it
carries the note `repair attempted, not validated` plus a failed `repair` check.
A successful repair emits `sha256_original`, the hash of the bytes read from
the source, alongside the normal `sha256` of the written output. The legacy
validation label becomes `decoded` for a decoded-mode repair; emitted scores,
output names and the benchmark's exact metric are unchanged. Use `--repair`
only when opening the file matters more than byte-exact evidence.

### 3.9. Read-error policy (H-22)

`--on-read-error {abort|zero-fill|skip}` defaults to `zero-fill`, the behavior
confirmed by the user during the review. E01 inflate/checksum failures and raw/split read
errors are converted into a recorded `{offset, len, reason}` range plus an
`UNREADABLE_REGION` warning. E01 failures are recorded at chunk granularity;
raw and split reads are retried one 512-byte sector at a time, so good sectors
in a failed request are retained. Zero-filled bytes are never treated as
evidence: any output intersecting an unreadable range is not `verified` and
carries an `overlaps_unreadable_region: false` check. With `skip`, candidates
needing the unreadable bytes are dropped while unaffected files are unchanged.

With `abort`, the reader records the failed range before returning an error.
The CLI then writes a schema-2 partial report with no carved files, the recorded
`unreadable_regions[]`, `UNREADABLE_REGION` and `READ_ABORTED`, and returns the
original error so the process exits 1. A completed run with unreadable ranges
still exits 3 as an incomplete run. VHD and VMDK keep their own
container-specific error semantics; real libewf and `.E02` controls are
SKIPPED because `ewfacquire` is unavailable.

### 3.10. Container identity in reports (H-25)

Schema 2 keeps the historical `input.size` (logical image length) and
`input.size_after` (aggregate physical bytes of the backing files) unchanged,
and adds an explicit distinction: `logical_size` with `logical_size_after`,
`physical_size` with `physical_size_after`, the resolved `format`, and
`files[]` with each backing file's path, size and mtime. `format` is never
`auto`. The logical capacity of an E01, VHD or VMDK is fixed when its
descriptor is opened; raw and split sources have no separate physical layer, so
their logical size follows the current backing size when a change is observed.

For stdin, logical and physical sizes both describe the temporary spool,
`files[]` is empty, and `unchanged` is `null`: there is no stable backing path
to compare. Disk-backed sources still report a boolean `unchanged`. All new
fields are omitted by `--report-schema 1`, which removes the whole `input`
object. The executed controls use a compressed E01, a monolithicSparse VMDK
and a stdin PNG; real libewf-made multi-segment E01/.E02 containers are
SKIPPED because `ewfacquire` is unavailable.

### 3.11. Unparsable volumes (H-26)

`VolumeSet.errors` now stores a structured `VolumeParseError` for each
partition whose boot sector was recognised but whose filesystem metadata could
not be parsed: the 1-based partition index, the absolute byte offset and the
underlying parser reason. `probe()` adds context such as `NTFS boot sector at
byte 1048576 but $MFT unreadable`, so the report no longer claims that no
filesystem was found when one was recognised and failed later.

`scan` emits one `VOLUME_PARSE_FAILED` warning per parse error in both
`warnings[]` and stderr. The code is incomplete, so a scan that finishes with
such a warning exits 3; it does not abort and other healthy volumes are still
scanned. Each `regions[]` row gains an additive `may_contain_allocated` boolean.
For a fallback `unsupported` region backed by a parse error it is true,
declaring that the raw scan mixes allocated and unallocated bytes. Other region
kinds keep `false`.

With no selectable volume, `info` keeps its documented exit 1 but reports the
specific boot-sector offset and failed metadata instead of the previous
generic "no FAT32/NTFS filesystem found" text. Explicit `--partition` and
`--offset` views retain their existing error paths. No filesystem
reconstruction, backup boot sector, `$MFTMirr` fallback or scoring change was
added.

### 3.12. Coverage regions (H-27)

`regions[]` is now generated from the same `scan::plan_extents` result that
creates scanner work. A supported volume contributes its merged free extents
and the allocated complement inside that volume; gaps and unsupported
partitions contribute their own extents. The rows are sorted, disjoint and
cover the whole logical image. Holes left by malformed partition metadata are
filled with unscanned gap rows. `info --json` uses the same builder, so a
standalone FAT32 volume reports its real free extents rather than the whole
volume.

Each scanned region carries the `extent_id` of the scanner extent that reads
it, matching the `extent_id` used by carved-file records. Unscanned regions
have `extent_id: null`. `--max-regions` (default 4096, 0 = unlimited) merges
adjacent rows with the same `scanned` state and sets `summarized: true` on the
result; a merge across different original kinds uses the additive `mixed`
kind. Coverage and `Σ len(scanned regions) == bytes_scanned` are never
relaxed. When the requested cap is smaller than the number of scanned/unscanned
alternations, the exact map is retained instead of inventing a lossy summary.

Carving itself is untouched: the extents, tasks, scores, digests, emission
rules, output names, CSV and schema 1 remain byte-for-byte compatible. Schema 2
adds `options.max_regions`, the two region fields and the `mixed` kind.

### 3.13. CLI and reporting

- Three commands: `info`, `scan`, `signatures`.
- `--json-report` outputs all metadata and the file list.
- `--csv-report` outputs a flat table.
- `--quiet` and `--verbose` control the log level.
- The default output directory must be empty; `--force` allows overwriting.

---

## 4. Experimental Methodology

### 4.1. Environment

**Table 7.** Experimental environment

| Component | Value |
|---|---|
| Operating system | Linux 6.19.14 (Kali amd64) |
| CPU | 4 logical CPU |
| Python | 3.13.12 |
| Rust | stable, edition 2021 |
| CarveX | 0.1.0, release, LTO thin |
| Foremost | 1.5.7 |
| Scalpel | 1.60 |
| PhotoRec | 7.2 (2/2024) |

SHA-256 of the competing binaries (measured on the experimental machine): Foremost `/usr/bin/foremost` = `7ab32990bd84c7c3a1af94e8ec864f9ffa2d655c748bce0d5f3768d6e3275b30`; Scalpel `/usr/bin/scalpel` = `a655eddd4f837c718ede2c83b61ffad90132293150321385f3a66df74da35c3c`; PhotoRec `/usr/bin/photorec` = `681a31f01e5cc0f1e9d10fa81e989b206173dcaae720da54b7e75567c1ee1549`. The CarveX binary used for the benchmark is `carvex 0.1.0` built from this source (`cargo build --release --locked`); **the binary hash was not recorded at run time** — this is a documented shortcoming rather than a guess.

### 4.2. Corpus

The corpus comprises 30 files, totaling 14,259,990 bytes:

**Table 8.** Corpus composition (30 files)

| Type | Count | Variants |
|---|---:|---|
| JPEG | 10 | baseline, progressive, grayscale, low quality, portrait, CMYK, EXIF thumbnail, small, large |
| PNG | 10 | RGBA, RGB, grayscale, palette, 1-bit, interlaced, alpha, various sizes |
| PDF | 10 | images, multi-page, text, incremental update, scanned, large/small files |

The corpus was generated with Pillow (PDF files were built by an in-house PDF script, without an external PDF library) and saved with a JSON manifest containing SHA-256, size, and description. Manifest hash verification was performed before each benchmark run.

### 4.3. Test images

Each image:

- 64 MiB in size.
- MBR, primary partition starting at 1 MiB.
- FAT32: label `CARVEXFAT`, cluster 512 byte.
- NTFS: label `CARVEXNTFS`, cluster 4096 byte.
- Files were copied into the `photos/` and `docs/` directories, then deleted according to the scenario.

**Table 9.** Four test images

| Image | Filesystem | Scenario | Deleted files | Kept files |
|---|---|---|---:|---:|
| `all-deleted_fat32` | FAT32 | all-deleted | 30 | 0 |
| `all-deleted_ntfs` | NTFS | all-deleted | 30 | 0 |
| `mixed_fat32` | FAT32 | mixed | 20 | 10 |
| `mixed_ntfs` | NTFS | mixed | 20 | 10 |

Mixed rule: in the 30-file list, positions `i % 3 == 0` (zero-based) are kept. The kept files:

```text
jpeg_01, jpeg_04, jpeg_07, jpeg_10
png_03, png_06, png_09
pdf_02, pdf_05, pdf_08
```

Files were deleted with `os.remove` after sync; no overwriting, no secure deletion. Therefore the content remains in free clusters and filesystem metadata accurately reflects the state.

### 4.4. Tools and configuration

**CarveX:**

```bash
carvex scan IMAGE -o OUT --mode unallocated --types jpeg,png,pdf --min-score 50 --quiet
carvex scan IMAGE -o OUT --mode raw --no-fs --types jpeg,png,pdf --min-score 50 --quiet
```

No `--repair`, no `--dedupe-hashes`, default nested suppression retained.

**Foremost:**

```bash
foremost -Q -c bench/foremost.conf -t jpg,png,pdf -i IMAGE -o OUT
```

`bench/foremost.conf` contains only comments so that Foremost uses its built-in signatures. Adding JPEG/PDF entries to the config would register a second signature and create duplicate output.

**Scalpel:**

```bash
scalpel -c bench/scalpel.conf -o OUT IMAGE
```

Minimal config:

```text
jpg y 20000000 \xff\xd8\xff \xff\xd9
png y 20000000 \x89\x50\x4e\x47\x0d\x0a\x1a\x0a \x49\x45\x4e\x44\xae\x42\x60\x82
pdf y 20000000 %PDF- %%EOF
```

**PhotoRec:**

```bash
photorec /log /d OUT /cmd IMAGE partition_i386,1,fileopt,everything,disable,jpg,enable,png,enable,pdf,enable,search
```

PhotoRec requires a separate working directory; the actual results are in `OUT.1`. The image is passed by absolute path.

### 4.5. Metrics

For each run:

- **TP:** an output file matching an unassigned original file.
- **FP:** output matching no original file, or a duplicate.
- **FN:** an original file with no matching output.

Greedy one-to-one assignment in output order; each original file is counted only once. Output without a valid magic is still counted as FP if it has a target extension.

$$P = \frac{TP}{TP + FP}, \quad R = \frac{TP}{TP + FN}, \quad F1 = \frac{2PR}{P + R}$$

Two matching methods:

1. **Exact:** the SHA-256 of the entire output equals the original file's SHA-256.
2. **Usable:** trim the output at the last real footer (JPEG EOI, PNG IEND, PDF `%%EOF`), then compare SHA-256. This ignores the padding/NUL/extra newline bytes that some tools append.

Timing is wall-clock, taking the median of 3 repetitions for each configuration. The 64 MiB images reside in the operating system's cache after the first run, so the figures reflect a warm-cache scenario.

### 4.6. Procedure

1. Create the corpus and verify the manifest.
2. Create 4 images using loop mounts; write a ground-truth manifest containing hashes, sizes, and the deleted/kept lists.
3. Verify image hashes before running.
4. Run 5 configurations × 4 images × 3 repetitions = 60 runs.
5. Collect outputs, classify by magic and extension, compute metrics.
6. Aggregate pooled results by tool and by file type.
7. Save logs, JSON, CSV, Markdown.

---

## 5. Results

### 5.1. Pooled results — usable

**Table 10.** Pooled results across 4 images — usable measure

| Tool | Space | Out | TP | FP | FN | Precision | Recall | F1 |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| CarveX | unallocated | 100 | 100 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| CarveX | raw | 120 | 100 | 20 | 0 | 83.33% | 100.00% | 90.91% |
| Foremost | raw | 156 | 94 | 62 | 6 | 60.26% | 94.00% | 73.44% |
| Scalpel | raw | 166 | 92 | 74 | 8 | 55.42% | 92.00% | 69.17% |
| PhotoRec | raw | 114 | 94 | 20 | 6 | 82.46% | 94.00% | 87.85% |

### 5.2. Pooled results — exact

**Table 11.** Pooled results across 4 images — exact measure

| Tool | Space | TP | FP | FN | Precision | Recall | F1 |
|---|---|---:|---:|---:|---:|---:|---:|
| CarveX | unallocated | 100 | 0 | 0 | 100.00% | 100.00% | 100.00% |
| CarveX | raw | 100 | 20 | 0 | 83.33% | 100.00% | 90.91% |
| Foremost | raw | 78 | 78 | 22 | 50.00% | 78.00% | 60.94% |
| Scalpel | raw | 78 | 88 | 22 | 46.99% | 78.00% | 58.65% |
| PhotoRec | raw | 94 | 20 | 6 | 82.46% | 94.00% | 87.85% |

The difference between exact and usable shows that most Foremost/Scalpel errors on PDF files are **padding bytes after the footer**, not lost content. For Foremost, usable PDF files rise from 14 to 30 TP; for Scalpel, from 16 to 30 TP. PhotoRec is unchanged because its outputs already match exactly.

### 5.3. Per-image results — CarveX

**Table 12.** CarveX results per image

| Image | Mode | Out | TP | FP | FN | P | R | F1 |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| all-deleted_fat32 | unallocated | 30 | 30 | 0 | 0 | 100% | 100% | 100% |
| all-deleted_fat32 | raw | 30 | 30 | 0 | 0 | 100% | 100% | 100% |
| all-deleted_ntfs | unallocated | 30 | 30 | 0 | 0 | 100% | 100% | 100% |
| all-deleted_ntfs | raw | 30 | 30 | 0 | 0 | 100% | 100% | 100% |
| mixed_fat32 | unallocated | 20 | 20 | 0 | 0 | 100% | 100% | 100% |
| mixed_fat32 | raw | 30 | 20 | 10 | 0 | 66.67% | 100% | 80.00% |
| mixed_ntfs | unallocated | 20 | 20 | 0 | 0 | 100% | 100% | 100% |
| mixed_ntfs | raw | 30 | 20 | 10 | 0 | 66.67% | 100% | 80.00% |

On all-deleted images, unallocated space is nearly the entire image, so raw and unallocated give identical results. On mixed images, raw recovers exactly the 10 kept files per image, producing 10 FP. CarveX unallocated does not read allocated clusters, so it avoids this error.

On all-deleted images, the engine saw 41 candidates, wrote 30 files, and discarded 11 nested candidates (images embedded in PDF files). In unallocated mode on mixed images, there were only 27 candidates in free space, 20 files written, and 7 nested candidates discarded. Nested removal did not lose any original files: exact TP remained 30/30 and 20/20.

### 5.4. Results by file type — usable

**Table 13.** Results by file type — usable

| Tool | Type | Deleted | Out | TP | FP | FN |
|---|---|---:|---:|---:|---:|---:|
| CarveX unallocated | JPEG | 32 | 32 | 32 | 0 | 0 |
| CarveX unallocated | PNG | 34 | 34 | 34 | 0 | 0 |
| CarveX unallocated | PDF | 34 | 34 | 34 | 0 | 0 |
| CarveX raw | JPEG | 32 | 40 | 32 | 8 | 0 |
| CarveX raw | PNG | 34 | 40 | 34 | 6 | 0 |
| CarveX raw | PDF | 34 | 40 | 34 | 6 | 0 |
| Foremost | JPEG | 32 | 76 | 30 | 46 | 2 |
| Foremost | PNG | 34 | 40 | 34 | 6 | 0 |
| Foremost | PDF | 34 | 40 | 30 | 10 | 4 |
| Scalpel | JPEG | 32 | 86 | 28 | 58 | 4 |
| Scalpel | PNG | 34 | 40 | 34 | 6 | 0 |
| Scalpel | PDF | 34 | 40 | 30 | 10 | 4 |
| PhotoRec | JPEG | 32 | 36 | 28 | 8 | 4 |
| PhotoRec | PNG | 34 | 38 | 32 | 6 | 2 |
| PhotoRec | PDF | 34 | 40 | 34 | 6 | 0 |

**Exact by type — notable points:**

**Table 14.** Results by file type — exact (TP/FP/FN)

| Tool | JPEG (TP/FP/FN) | PNG (TP/FP/FN) | PDF (TP/FP/FN) |
|---|---:|---:|---:|
| CarveX unallocated | 32/0/0 | 34/0/0 | 34/0/0 |
| CarveX raw | 32/8/0 | 34/6/0 | 34/6/0 |
| Foremost | 30/46/2 | 34/6/0 | 14/26/20 |
| Scalpel | 28/58/4 | 34/6/0 | 16/24/18 |
| PhotoRec | 28/8/4 | 32/6/2 | 34/6/0 |

### 5.5. Timing

The times below are the sums of medians across 4 images (median of 3 repetitions per image), not the time of a single combined run.

**Table 15.** Time and throughput (median across 4 images)

| Tool | Total time (s) | Image throughput (MiB/s) |
|---|---:|---:|
| CarveX unallocated | 0.443 | 577.3 |
| CarveX raw | 0.535 | 478.8 |
| Foremost | 1.500 | 170.7 |
| Scalpel | 1.896 | 135.0 |
| PhotoRec | 1.539 | 166.4 |

CarveX unallocated is fastest because it scans only 61–62 MiB per image instead of 64 MiB, while using 4 threads. The time difference between unallocated and raw is larger on mixed images because unallocated skips the region containing kept files.

### 5.6. Additional validation on NIST CFReDS RM#2

In addition to the synthetic benchmark, CarveX was run on the public DD image of the NIST CFReDS Data Leakage Case RM#2 (FAT32). The archive and the DD image match the SHA-1/MD5 published by NIST; the SHA-256 is recorded in [`CFREDS.md`](CFREDS.md).

**Table 16.** Results on NIST CFReDS RM#2

| Mode | Scanned region | Time | Throughput | JPEG | PNG | PDF | Files |
|---|---:|---:|---:|---:|---:|---:|---:|
| unallocated | 1,020 MiB | 1.609 s | 633.5 MiB/s | 113 | 205 | 0 | 318 |
| raw | 3.73 GiB | 3.418 s | 1,117.2 MiB/s | 113 | 205 | 0 | 318 |

- 318/318 files `complete`, score 100; Pillow successfully opened and verified all 318; 276 unique SHA-256 values.
- 73 JPEG candidates rejected; 38 JPEG images embedded in metadata blocked; no false PNG.
- Raw and unallocated outputs are identical because the 1 GiB partition was quick-formatted (261,119/261,120 free clusters).
- At least 178/318 files are media embedded in DOCX/PPTX (`word/media/`, `ppt/media/`), not standalone images. With no per-file ground truth, precision/recall cannot be computed; for details and limitations see [`CFREDS.md`](CFREDS.md).

---

## 6. Discussion

### 6.1. Why CarveX performs well on this corpus

1. **The structural parser determines file boundaries correctly.** For JPEG, walking the entropy data and skipping thumbnails avoids premature cuts. For PDF, checking `startxref` and selecting the last revision avoids false `%%EOF` markers.
2. **No scanning of allocated space.** On mixed images, this is the decisive factor in eliminating 20 FP.
3. **Extent limits.** Candidates do not read into other clusters, avoiding data mixing and reducing junk output.
4. **Nested suppression.** PDF files containing embedded images do not produce extra false output.
5. **Clean, unfragmented corpus.** Files were written contiguously before deletion, an ideal condition for linear carving.

### 6.2. Why Foremost/Scalpel have many FP

Foremost and Scalpel use built-in signatures without deep structure checking. For PDF files, they often write extra bytes after `%%EOF`, causing exact to fail while usable passes: Foremost achieves only 14/34 exact TP but 30/34 usable TP; Scalpel achieves 16/34 and 30/34. For JPEG, the two tools produce many non-matching outputs (Foremost 76 outputs for 32 original files; Scalpel 86 outputs) even though exact and usable are equal, indicating that the problem lies in boundaries/candidates rather than in trailing padding bytes.

**`unknown_fp` classification (re-verified by running the exact benchmark command on all 4 images; totals match `results.json`: Foremost 44, Scalpel 54):**

- **Foremost — 40/44 `unknown_fp` are JPEG byte ranges located inside the corpus's PDF files.** The image PDF files in the corpus (`pdf_01/02/05/07/10`) embed JPEG images as DCTDecode; Foremost cuts those JPEG ranges out into separate `.jpg` files. The remaining 4 `unknown_fp` are prematurely truncated PDF files (ending only at the `%%EOF` of the first revision in `pdf_04_incremental`), also artifacts of the corpus's PDF files — none of them is a "false footer" outside the data.
- **Scalpel — 40/54 `unknown_fp` are also JPEG images embedded in PDF files** (same mechanism as above); **4/54** are prematurely truncated incremental PDF files like Foremost's; the remaining **10/54** are JPEG sub-spans: 8 spans cut from the EXIF thumbnail of `jpeg_08_exif_thumb_900x700.jpg` (2 spans × 4 images — Scalpel matches the `\xff\xd8\xff`/`\xff\xd9` header/footer inside a larger JPEG file) and 2 empty 4-byte JPEG fragments (`FFD8 FFD9`) from the NTFS image.

That is, **none of Foremost's/Scalpel's FP in this benchmark suite are coincidental byte sequences outside the data**: they are all images embedded in PDF files, prematurely truncated PDF revisions, or sub-spans (thumbnails) — truly "embedded-object / boundary" in nature rather than simple footer errors. CarveX avoids these types through nested suppression (by default) and a structural parser that selects the correct final `%%EOF`. This classification is not yet automated in the benchmark runner (`unknown_fp` is still aggregated), as recorded in `docs/REVIEW_RESPONSE.md` F-17; the 40/44 and 40/54 counts agree with the assessment in the guide D-01.

PhotoRec has its own validator and cleaner output, but it still scans the whole image, so it recovers the 20 kept files in mixed images and misses some JPEG/PNG files due to its internal validation conditions.

### 6.3. Meaning of exact and usable

- **Exact** is appropriate when byte-for-byte intact evidence is required.
- **Usable** is appropriate when the goal is to open the file and have untruncated content.

Reporting both avoids misjudging tools that append padding bytes. However, usable does not replace exact in forensic reporting.

### 6.4. Reliability and Threats to Validity

**Internal validity:**

- 3 repetitions per configuration, median timing.
- Image and corpus hashes verified.
- One-to-one metrics, no inflation of TP by duplicates.
- FP classification into kept/unknown to explain the causes.

**Threats:**

1. **Small, synthetic corpus.** 30 files, 4 64 MiB images are not representative of disk images of hundreds of GB.
2. **No fragmentation.** Files were written contiguously; results will drop on real, fragmented disks.
3. **No overwriting.** The images are freshly created and free space is intact; in practice, partial overwriting may occur.
4. **Warm cache.** Timing figures do not reflect cold I/O.
5. **Competitor configuration.** Foremost/Scalpel used minimal configs; different configurations may change the results.
6. **Tool versions.** Results are tied to Foremost 1.5.7, Scalpel 1.60, PhotoRec 7.2.
7. **Complex fragmented MFT.** The current NTFS parser reads `$MFT` through a single unnamed `$DATA` attribute; images with an attribute list or multiple `$MFT` extents may not be fully supported. Oversized FAT tables (> 512 MiB) are also rejected to avoid unbounded memory allocation.
8. **No real-world Windows testing.**
9. **Search-space confound (F-16).** CarveX `unallocated` scans only free space, while Foremost/Scalpel/PhotoRec scan the whole image; part of the recall/FP difference on mixed images is *definitional* of the mode, not just a matter of carving-algorithm quality. The full pair of modes (baseline on the same unallocated blob, CarveX on the whole image with and without the validator) has not been run to separate these two factors — the figures in §5 must be read under the stated conditions only and must not be generalized.
10. **Evidence-source scope (B-04).** The additional readers were verified with synthetic fixtures, not with acquired case images. They cover concatenated raw segments, classic EWF-E01 with compressed or Adler-32 uncompressed chunks, fixed and dynamic VHD, flat and sparse VMDK extents, and stdin via a temporary spool. Recognized EWF2/Ex01, L01/Lx01, differencing VHD, ESX/COWD and compressed streamOptimized VMDK variants are rejected explicitly by magic, cookie or descriptor inspection. Encrypted E01 is not supported: there is no decryption, and no claim is made that every encrypted variant is detected before parsing. E01 segment discovery handles the usual two-digit base-36 suffixes (up to `.EZZ`, 1295 segments). The dynamic VHD reader consults the block allocation table but does not interpret the per-sector presence bitmap inside an allocated block, so a partially written block may expose stale bytes. No interoperability test with `ewfacquire`/`ewfexport` was possible because those tools were unavailable. These sources are outside the published benchmark; ordinary raw images retain the original `RawFile` behavior.
11. **Resource-budget scope (R-01/R-02).** `--max-memory` bounds carve buffers,
   retained carved outputs and B-06 stitch buffers; it is not a total
   process-RSS allocator cap, and JPEG/PDF decoder internals are not fully
   instrumented. PNG decode validation has an independent expected-size bound,
   `--max-decode-bytes` (256 MiB default), and streams inflation through a fixed
   64 KiB buffer. Images above that bound are reported as `validation=partial`
   without being inflated. The default benchmark path does not use decode
   validation, so the frozen figures are unaffected.
12. **Repair scope (H-21).** PDF reconstruction is limited to classic xref
    tables whose object headers and `/Root` or `/Type /Catalog` survive in the
    salvaged prefix; xref-stream-only or heavily damaged documents may remain
    `truncated` rather than be repaired. JPEG/PNG repair proves only that the
    appended footer restores the same structural/decode result as an intact
    file of that type; it does not prove that missing payload bytes were
    recovered. Repaired outputs are excluded from exactness claims and the
    published benchmark does not enable `--repair`.
13. **Read-error scope (H-22).** The executed evidence uses synthetic E01
    sections and a failing mock `ImageSource`; no libewf-made E01, `.E02`
    multi-segment image, real block device or Windows read failure was
    available. Raw/split sectors are retried at a fixed 512-byte granularity,
    which may be narrower or wider than a device's native failure unit. VHD and
    VMDK errors are governed by their container readers, not by
    `PolicySource`, and an abort partial report contains no carved files
    because the run stopped before a complete candidate set existed.

---

## 7. Conclusion and Future Work

### 7.1. Conclusion

CarveX meets its stated objectives:

- Working Rust CLI supporting JPEG/PNG/PDF.
- Accurate detection and exploitation of the unallocated space of FAT32/NTFS.
- Structural parser reduces false positives and determines the correct footer.
- Reproducible benchmark comparing the same corpus and ground truth against three popular tools (not an absolutely "fair" comparison: see the search-space confound in §6.4 item 9).
- Linux/Windows binaries and documentation.

On the test corpus, CarveX unallocated achieved a perfect 100/100 exact, 0 FP. This is the result of a controlled condition, not a guarantee for every real-world image, but it shows that the filesystem-aware design and the structural parser are the right direction.

### 7.2. Future work

1. **Metadata-assisted fragmented file reassembly:** extend the two-fragment BGC stage by reading the deleted file's runlist from the MFT (NTFS) or directory entries (FAT32) when metadata remains, and measure recall/splice rate on fragmented profiles.
2. **Additional formats:** HEIC, TIFF, DOCX, ZIP, MP4; prioritize formats with a clear structure.
3. **Support exFAT, FAT12/16, ext4.**
4. **Broader container support:** add EWF2/Ex01, encrypted E01, L01,
   differencing VHD, ESX/COWD and compressed streamOptimized VMDK behind the
   same `ImageSource` interface, with real acquisition-tool interoperability
   tests.
5. **Deeper validation:** decode JPEG entropy data to detect errors, fully parse PDF xref streams, and extend PNG validation beyond chunk structure, filter bytes and decoded length.
6. **Speedups:** direct I/O, `io_uring`, or memory-mapping for large images.
7. **Evaluate on CFReDS and real-world images**, adding independent ground truth.
8. **Interface:** a TUI or visual HTML reports.

---

## References

1. NIST, *CFReDS Data Leakage Case*, <https://cfreds-archive.nist.gov/data_leakage_case/data-leakage-case.html>.
2. NIST, *CFReDS Hacking Case*, <https://cfreds.nist.gov/all/NIST/HackingCase>.
3. Microsoft, *FAT32 File System Specification*.
4. Linux-NTFS Project, *NTFS Documentation*.
5. ITU-T Recommendation T.81, *Digital compression and coding of continuous-tone still images*.
6. ISO/IEC 15948:2004, *PNG (Portable Network Graphics) Specification*.
7. ISO 32000-1:2008, *Document management — Portable Document Format*.
8. Foremost, <https://foremost.sourceforge.net/>.
9. Scalpel, <https://github.com/sleuthkit/scalpel>.
10. PhotoRec, <https://www.cgsecurity.org/wiki/PhotoRec>.
11. S. L. Garfinkel, *Carving contiguous and fragmented files with fast object validation*, Digital Investigation, vol. 4, pp. S2–S12, 2007, DOI: [10.1016/j.diin.2007.06.017](https://doi.org/10.1016/j.diin.2007.06.017) (DFRWS 2007; DOI page opened).
12. G. G. Richard III, V. Roussev, *Scalpel: A Frugal, High Performance File Carver*, DFRWS 2005, New Orleans, pp. 71–77, <https://dfrws.org/presentation/scalpel-a-frugal-high-performance-file-carver/> (no DOI; DFRWS page opened).
13. A. Pal, N. D. Memon, *The evolution of file carving*, IEEE Signal Processing Magazine, vol. 26, no. 2, pp. 59–71, 2009, DOI: [10.1109/MSP.2008.931081](https://doi.org/10.1109/MSP.2008.931081) (DOI page opened; Pal–Memon author order matches IEEE Xplore).
14. H. T. Sencar, N. Memon, *Identification and recovery of JPEG files with missing fragments*, Digital Investigation, vol. 6, pp. S88–S98, 2009, DOI: [10.1016/j.diin.2009.06.007](https://doi.org/10.1016/j.diin.2009.06.007) (DFRWS 2009; DOI page opened).
15. M. I. Cohen, *Advanced carving techniques*, Digital Investigation, vol. 4, no. 3–4, pp. 119–128, 2007, DOI: [10.1016/j.diin.2007.10.001](https://doi.org/10.1016/j.diin.2007.10.001) (DOI page opened).
16. R. Poisel, S. Tjoa, *A Comprehensive Literature Review of File Carving*, ARES 2013, pp. 475–484, DOI: [10.1109/ARES.2013.62](https://doi.org/10.1109/ARES.2013.62) (DOI page opened).

All citations in entries 11–16 were opened and cross-checked (authors, year, volume, pages, DOI) on the publisher/DOI pages before being included in the report; nothing was cited from memory (guide §11, Appendix F).

---

## Appendix A — Reproduction Commands

```bash
cd $CARVEX   # repository checkout, e.g. ~/carvex

# Build and test
cargo build --release
cargo test

# Create corpus
python3 bench/make_corpus.py

# Create images (requires root)
sudo python3 bench/make_images.py --carvex target/release/carvex --force

# Run the official benchmark
python3 bench/run_benchmark.py --reps 3 --out bench/results/final

# View results
cat bench/results/final/BENCHMARK.md
```

## Appendix B — Usage Examples

```bash
# Filesystem information
carvex info disk.img

# Recover only free space
carvex scan disk.img -o recovered --mode unallocated

# Scan the whole image, with reports
carvex scan disk.img -o raw-out --mode raw --no-fs \
  --json-report raw-report.json --csv-report raw-report.csv

# Scan partition 2, repair truncated files
carvex scan disk.img -o out --partition 2 --repair

# Keep embedded content
carvex scan disk.img -o out --keep-nested

# Evidence sources (auto-detection normally suffices; explicit formats shown)
carvex scan disk.001 -o split-out --input-format split
carvex scan disk.E01 -o e01-out --input-format e01
carvex scan disk.vhd -o vhd-out --input-format vhd
carvex scan disk.vmdk -o vmdk-out --input-format vmdk
cat disk.img | carvex scan - -o stdin-out --input-format stdin
```
