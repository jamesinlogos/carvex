# Testing CarveX on the NIST CFReDS dataset

This document guides downloading, verifying, and trial-scanning the
**NIST CFReDS Data Leakage Case** with CarveX. This is an additional validation
step on public data, separate from the synthetic benchmark suite in `bench/`.

> **Warning:** The RM#2 image is a 4 GB DD image after extraction. At least about
> 5 GB of free space is required. Always keep the original unchanged; scan only a
> copy or the extracted image, and do not write to the image.

## 1. Dataset

### 1.1. Data Leakage Case

Home page:

<https://cfreds-archive.nist.gov/data_leakage_case/data-leakage-case.html>

The scenario describes an employee ("Iaman Informant") copying confidential data
to an external device. The dataset includes:

| Device | Description | Format |
|---|---|---|
| PC | Windows 7, 20 GB, NTFS | Split DD 5.05 GB; E01 7.28 GB |
| RM#1 | USB 4 GB, exFAT | E01 74.5 MB |
| RM#2 | USB 4 GB, FAT32 | **DD 4 GB** in a 7z file of 219 MB; E01 243 MB |
| RM#3 | CD-R 700 MB, UDF | RAW/CUE, DD, E01 |

> **Observation from the DD image:** the RM#2 image is 3.73 GiB long but contains
> only one 1 GiB FAT32 partition; about 2.7 GiB remains outside the partition.
> `raw` mode scans the whole image and confirms that there are no JPEG/PNG/PDF
> signatures outside the partition. Details in §5.1.

CarveX is best suited to **RM#2 — DD image, FAT32**, because:

- The image is small and easy to download and extract;
- It matches the target filesystem (FAT32);
- The DD image hash can be checked against NIST documentation.

However, read the NIST answer document carefully before interpreting results. The
confidential files on RM#2 were **renamed and given misleading extensions** to
deceive; their actual content is PPT/PPTX, XLS/XLSX, DOC/DOCX (e.g.,
`landscape.png` is actually DOCX, `new_years_day.jpg` is actually XLSX). CarveX
searches only for **real JPEG/PNG/PDF content** by signature, not by extension, so
RM#2 is not a fair recall measurement for these three formats. The main value of
RM#2 is checking:

- whether CarveX correctly parses FAT32 and the unallocated space of a real USB
  image;
- whether the tool runs stably on a 4 GB image and can extract the JPEG/PNG/PDF
  files actually present in the image;
- behavior on a quick-formatted image, where FAT metadata is erased but content
  remains.

The NTFS PC image can also be scanned but is much larger. This trial used a
raw/DD image. Classic E01 segments are now supported with `--input-format e01`;
EWF2/Ex01, encrypted E01 and logical evidence files are not supported and still
require conversion to raw/DD with another forensic tool.

### 1.2. Files to download

```text
https://cfreds-archive.nist.gov/data_leakage_case/images/rm%232/cfreds_2015_data_leakage_rm%232.7z
```

Size: 229,899,285 bytes (≈ 219 MiB).

Hashes published by NIST (page `hash_values.html`):

| Object | Algorithm | Value |
|---|---|---|
| 7z archive | SHA-1 | `DDFE97AA3D8D0B33CC6092123090A8154945F38E` |
| Extracted DD image | SHA-1 | `048961A85CA3ECED8CC73F1517442D31D4DCA0A3` |
| Extracted DD image | MD5 | `B4644902ACAB4583A1D0F9F1A08FAA77` |

NIST publishes only SHA-1/MD5 for this dataset. These hashes are sufficient to
detect download/extraction errors, but should not be used as anti-tampering
evidence in a real forensic setting. After downloading, compute SHA-256 yourself
and record it.

## 2. Download and verification

### 2.1. Using the script

```bash
cd $CARVEX   # repository checkout, e.g. ~/carvex
./scripts/fetch_cfreds.sh ~/cfreds-data
```

The script will:

1. Download the archive to `~/cfreds-data/cfreds_2015_data_leakage_rm2.7z`;
2. Check the archive's SHA-1;
3. Extract with `7z`;
4. Find the `.dd` image and check the image's SHA-1;
5. Print sizes and suggested CarveX commands.

### 2.2. Manual steps

```bash
mkdir -p ~/cfreds-data
cd ~/cfreds-data

curl -L --fail --retry 3 -o cfreds_rm2.7z \
  'https://cfreds-archive.nist.gov/data_leakage_case/images/rm%232/cfreds_2015_data_leakage_rm%232.7z'

sha1sum cfreds_rm2.7z
# DDFE97AA3D8D0B33CC6092123090A8154945F38E

7z x cfreds_rm2.7z

sha1sum 'cfreds_2015_data_leakage_rm#2.dd"
# 048961A85CA3ECED8CC73F1517442D31D4DCA0A3
```

If the hashes do not match, download again; do not continue the analysis.

## 3. Filesystem check

```bash
$CARVEX/dist/carvex-linux-x86_64 info \
  "$HOME/cfreds-data/cfreds_2015_data_leakage_rm#2.dd"
```

Expected:

- Filesystem: **FAT32**;
- The image may be a superfloppy (filesystem at offset 0) or have an MBR;
- A volume label and a plausible free-cluster count.

If CarveX reports that no filesystem was found:

```bash
# List partitions with another tool
fdisk -l image.dd
# Or force an offset if the boot sector is known
carvex info image.dd --offset 1M
```

## 4. Scanning with CarveX

### 4.1. Unallocated mode

```bash
mkdir -p ~/cfreds-data/recovered
$CARVEX/dist/carvex-linux-x86_64 scan \
  "$HOME/cfreds-data/cfreds_2015_data_leakage_rm#2.dd" \
  -o "$HOME/cfreds-data/recovered" \
  --mode unallocated \
  --types jpeg,png,pdf \
  --json-report "$HOME/cfreds-data/cfreds-report.json" \
  --csv-report "$HOME/cfreds-data/cfreds-report.csv"
```

### 4.2. Raw mode for comparison

```bash
mkdir -p ~/cfreds-data/recovered-raw
$CARVEX/dist/carvex-linux-x86_64 scan \
  "$HOME/cfreds-data/cfreds_2015_data_leakage_rm#2.dd" \
  -o "$HOME/cfreds-data/recovered-raw" \
  --mode raw --no-fs \
  --types jpeg,png,pdf \
  --json-report "$HOME/cfreds-data/cfreds-raw-report.json"
```

Compare file counts and inspect visually. The difference between raw and
unallocated shows how many still-allocated files (not deleted files) raw
recovered.

### 4.3. Checking the results

```bash
# Count by type in the JSON report
python3 - <<'PY'
import json
from collections import Counter
from pathlib import Path

report = json.loads((Path.home() / "cfreds-data" / "cfreds-report.json").read_text())
print(Counter(f["type"] for f in report["files"]))
print("bytes_written:", report["stats"]["bytes_written"])
PY

# Check magic and try opening
file ~/cfreds-data/recovered/*

python3 - <<'PY'
from collections import Counter
from pathlib import Path
from PIL import Image

root = Path.home() / "cfreds-data" / "recovered"
counts = Counter()
for p in sorted(root.iterdir()):
    if p.suffix.lower() in {".jpg", ".jpeg", ".png"}:
        try:
            im = Image.open(p)
            im.verify()
            counts["image-ok"] += 1
            print("OK ", p.name, im.size)
        except Exception as exc:
            counts["image-bad"] += 1
            print("BAD", p.name, exc)
    else:
        counts["pdf-or-other"] += 1
print(counts)
PY
```

`exiftool`, `identify`, `pdfinfo` (if installed) or an image/PDF viewer can be used
for manual checks. For PDFs, `pypdf` can check the page count.

## 5. Trial run results

This section records results from the development environment. The figures are
indicative only because the CFReDS dataset **does not include per-file ground
truth** for CarveX's carving task; precision/recall cannot be computed
automatically as with the synthetic benchmark. **The figures are from CarveX
0.1.0** (the binary was hashed in §5.1); 0.2.0 has not been re-run on CFReDS —
0.2.0 carving behavior has been verified unchanged on benchmark images (100/100
files match SHA-256), but there are no new CFReDS results to publish.

### 5.1. Environment and data verification

- Run date: 17–18/11/2026.
- Binary: `dist/carvex-linux-x86_64`, CarveX 0.1.0, SHA-256 `75c81ca5613933e42f17c56a37f0bb695027d1283dd028933e2afe6153d5d825` (hash of the binary at the time of the 17/11 run; the file in `dist/` has since been rebuilt as 0.2.0 — see `dist/SHA256SUMS`).
- Archive fully downloaded: **229,899,285 bytes**; SHA-1 matches the NIST value `DDFE97AA3D8D0B33CC6092123090A8154945F38E`.
- Extracted DD image: **4,004,511,744 bytes**; SHA-1 matches `048961A85CA3ECED8CC73F1517442D31D4DCA0A3`, MD5 matches `B4644902ACAB4583A1D0F9F1A08FAA77`.
- Self-computed SHA-256: archive `ade9fb60ba1f700b93c6b8b1f538c72000411e5b30037dc95c300c5a0aeafd65`; DD image `d930afa23f8905e88a28041786edbe3791b5bf41b01468027ee27a19cd59d407`.

Image layout per MBR:

| Property | Value |
|---|---|
| Image size | 4,004,511,744 bytes (3.73 GiB) |
| Partitions | 1 partition, type `0x0b` (W95 FAT32), starting at sector 128 |
| Partition offset | 65,536 bytes |
| Partition size | 2,097,152 sectors = 1.00 GiB |
| Volume label | `NO NAME` |
| Volume serial | `B4D85399` |
| Cluster | 512 B/sector × 8 sectors = 4,096 bytes |
| Total clusters | 261,120 |
| Free clusters | 261,119 (100.0%) |
| Free extent | 1 extent, offset 4,263,936, length 1,069,543,424 bytes |
| Root directory | Empty (first byte is 0) |

The remaining ~2.7 GiB of the image lies outside the single MBR partition; the raw
scan found no JPEG/PNG/PDF signatures there. Inside the partition, the FAT table
marks only cluster 2 (the root directory) as used, and the root directory is empty.
This is typical of a FAT32 partition that has just been **quick-formatted**:
metadata is erased but the old data clusters remain intact. Therefore CarveX treats
the entire 1 GiB partition as unallocated.

### 5.2. Scan results

Commands used:

```bash
./dist/carvex-linux-x86_64 info \
  "$HOME/cfreds-data/cfreds_2015_data_leakage_rm#2.dd"

./dist/carvex-linux-x86_64 scan \
  "$HOME/cfreds-data/cfreds_2015_data_leakage_rm#2.dd" \
  -o $HOME/cfreds-data/recovered --mode unallocated \
  --types jpeg,png,pdf \
  --json-report $HOME/cfreds-data/cfreds-report.json \
  --csv-report $HOME/cfreds-data/cfreds-report.csv

./dist/carvex-linux-x86_64 scan \
  "$HOME/cfreds-data/cfreds_2015_data_leakage_rm#2.dd" \
  -o $HOME/cfreds-data/recovered-raw --mode raw --no-fs \
  --types jpeg,png,pdf \
  --json-report $HOME/cfreds-data/cfreds-raw-report.json
```

| Mode | Scanned area | Time | Throughput | Candidates | Carved | Rejected | Nested blocked | JPEG | PNG | PDF | Bytes written |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `unallocated` | 1,069,543,424 B (1,020 MiB) | 1.609 s | 633.5 MiB/s | 429 | 318 | 73 | 38 | 113 | 205 | 0 | 92,272,113 |
| `raw --no-fs` | 4,004,511,744 B (3.73 GiB) | 3.418 s | 1,117.2 MiB/s | 429 | 318 | 73 | 38 | 113 | 205 | 0 | 92,272,113 |

Candidate balance: `318 + 73 + 38 = 429`. The outputs of the two modes are
completely identical: the same 318 file names, the same content, and the sorted
SHA-256 lists match line by line (`diff` shows no differences). This is consistent
with the partition being quick-formatted and the 2.7 GiB outside the partition
containing no JPEG/PNG/PDF signatures.

No PDFs were found. The total carved data is 92,272,113 bytes (about 88 MiB), of
which JPEG is 30.3 MiB and PNG 57.7 MiB.

Copies of the JSON/CSV reports and logs from this run are in
[`bench/results/cfreds/`](../bench/results/cfreds/).

### 5.3. Output structure checks

- **318/318 files** have `status = complete`, `score = 100`; no file is truncated
  and `--repair` was not used.
- `file` correctly identifies **113 JPEG** and **205 PNG**.
- Pillow `Image.open(...).verify()` successfully opened and verified **318/318**
  images (0 errors).
- There are 214 distinct image sizes; the smallest is 564 bytes and the largest
  8,455,527 bytes (about 8.1 MiB).
- File offsets range from 4,272,665 to 260,313,088, i.e., all lie within the FAT32
  partition's free extent.
- The 318 files correspond to only **276 unique SHA-256 values**: there are 42
  duplicate groups, each with exactly 2 copies. This is expected when the same
  embedded image appears in multiple Office documents. With `--dedupe-hashes`
  enabled, the file count would drop to 276.
- **73 candidates rejected**, all fake or invalid JPEGs: 61 "expected JPEG marker"
  errors, 7 "nested JPEG SOI", 3 "JPEG EOI before any SOF", 1 "invalid JPEG segment
  length", 1 "invalid JPEG marker". No PNG candidates were rejected, consistent
  with the PNG parser checking chunk CRCs.
- **38 nested objects blocked**: all are JPEGs located inside metadata segments of
  larger images (18 in APP1/Exif, 20 in APP13/Photoshop), i.e., thumbnails/metadata
  rather than standalone images. Nested suppression works correctly and creates no
  extra junk files.

### 5.4. Important interpretation: most images are media embedded in Office documents

The 318 files must not be interpreted as 318 user-deleted files. Analyzing the ZIP
headers in the image:

- There are **216 visible media entries** in OOXML containers: **161** under
  `word/media/`, **55** under `ppt/media/`.
- By extension: 97 `png`, 81 `jpeg`, 20 `emf`, 16 `wmf`, 1 `gif`, 1 `pcz`.
- **179 entries are stored uncompressed** (ZIP method 0). Of these, **178 entries**
  have data offsets exactly matching the offset of a CarveX-carved file; the
  remaining entry is `word/media/image25.gif` (GIF is not supported).
- The remaining 37 entries are deflate-compressed, so their image signatures do
  not appear directly in free space.

Thus **at least 178/318 files (56%)** are images embedded inside DOCX/PPTX, not
standalone deleted images. The remaining 140 files do not match any visible media
entry; they may be standalone images, images in compressed containers, or media
from documents whose ZIP headers are no longer intact. This matches NIST's
description: the confidential files on RM#2 were renamed/given fake extensions
(`new_years_day.jpg`, `my_smartphone.png`, `landscape.png`, ...) but their real
content is PPT/PPTX, XLS/XLSX, DOC/DOCX. CarveX does not recover Office containers;
it only recovers the real JPEG/PNG images inside those containers.

Therefore, the CFReDS experiment demonstrates:

- CarveX correctly parses quick-formatted FAT32 and the unallocated space of a real
  USB image;
- CarveX runs stably on a 3.73 GiB image and returns structurally valid images;
- CarveX does not mistake fake extensions for real formats, but also cannot claim
  recall for the renamed Office files.

### 5.5. Assessment

**Strengths:**

1. The archive and DD image hashes both match NIST; the test data is reproducible.
2. Low scan times: 1.6 s for 1,020 MiB unallocated and 3.4 s for the whole 3.73 GiB
   image.
3. 318/318 output files opened and verified successfully; no truncated files, no
   repair needed.
4. No fake PNGs slipped through; all 73 wrong JPEG candidates were rejected before
   writing.
5. Nested suppression correctly removed 38 thumbnails, avoiding extra output.

**Limitations of the test:**

1. There is no per-file ground truth, so **precision/recall cannot be computed**
   from CFReDS.
2. The 318 files are not 318 user files: at least 178 are media embedded in Office;
   42 copies are SHA-256 duplicates.
3. There are no PDFs in the image, so the PDF parser could not be validated on real
   data.
4. Only FAT32; NTFS was not validated.
5. The image was quick-formatted with intact data; no actual overwrite or
   fragmentation was simulated.
6. Assessing "whether the recovered images are actually the case's target files"
   requires manual cross-checking with the NIST answer document and the original
   file list; CarveX cannot read original file names.

**Conclusion:** CFReDS RM#2 is a good test of FAT32 parsing, free-space scanning,
and valid output generation on a real image. It is **not** an accuracy benchmark
for JPEG/PNG/PDF, and the numbers 318/276 should be reported only as output
statistics, not as TP or recall.

## 6. Assessment and limitations

### 6.1. Strengths of the CFReDS test

- Public data, with hashes published by NIST.
- A real FAT32 image, created by FTK Imager with a real usage history, not a
  synthetic image.
- Can be cross-checked with other tools (Foremost, PhotoRec, Autopsy, FTK Imager).
- RM#2 is small enough to run in a reasonable time.

### 6.2. Limitations

1. **No per-file ground truth.** TP/FP/FN cannot be computed exactly. Assessment
   must rely on manual checks or the CFReDS answer document.
2. **No hashes of individual deleted files.** Only the image hash is available.
3. **The image may contain allocated files.** A raw scan will also recover
   non-deleted files; distinguish them with `--mode unallocated`.
4. **The data may contain sensitive content.** Handle only in a controlled
   environment.
5. **FAT32, not representative of NTFS.** For real NTFS testing, use the CFReDS PC
   image (5.05 GB compressed, 20 GB extracted) or another Windows image.
6. **Not a JPEG/PNG/PDF benchmark.** The confidential files on RM#2 have changed
   extensions and contain Office content, not the three formats CarveX supports.
   Do not use the number of carved files on RM#2 to draw conclusions about
   CarveX's accuracy.
7. **Limited container support.** Classic E01 reads were verified with
   generated fixtures, not `ewfacquire`/`ewfexport` output. Convert EWF2/Ex01,
   encrypted E01 and L01 images to raw first; no such conversion tool was
   available in the validation environment.
8. **Most output is embedded media.** At least 178/318 files lie in `word/media/`
   or `ppt/media/` of DOCX/PPTX; 42 files also share SHA-256 values with other
   files. These are real images but not standalone files, so 318 cannot be counted
   as the number of recovered user files.
9. **The partition is smaller than the image.** The RM#2 image is 3.73 GiB long but
   has only one 1 GiB FAT32 partition; about 2.7 GiB lies outside the partition.
   Results may differ if the E01 version or an RM#2 image with a different
   partition layout is analyzed.

### 6.3. Manual assessment criteria

When there is no ground truth, record at minimum:

- The number of carved files by type;
- The number of files opened/verified successfully;
- The number of truncated or structurally invalid files;
- The number of files clearly still allocated (if raw scanning);
- Scan time and throughput;
- Parser rejection cases (use `--verbose`).

## 7. Command summary

```bash
# Download + verify + extract
./scripts/fetch_cfreds.sh ~/cfreds-data

# Information
./dist/carvex-linux-x86_64 info ~/cfreds-data/'cfreds_2015_data_leakage_rm#2.dd"

# Unallocated scan
./dist/carvex-linux-x86_64 scan \
  ~/cfreds-data/'cfreds_2015_data_leakage_rm#2.dd" \
  -o ~/cfreds-data/recovered \
  --mode unallocated --json-report ~/cfreds-data/report.json

# Quick output check
file ~/cfreds-data/recovered/*
```

## 8. References

- CFReDS Data Leakage Case: <https://cfreds-archive.nist.gov/data_leakage_case/data-leakage-case.html>
- Hash values: <https://cfreds-archive.nist.gov/data_leakage_case/hash_values.html>
- NIST answer document: <https://cfreds-archive.nist.gov/data_leakage_case/leakage-answers.pdf>
- CFReDS Hacking Case: <https://cfreds.nist.gov/all/NIST/HackingCase>
