#!/usr/bin/env python3
"""Reproduce the independent review findings R1-R8 against CarveX.

The scenarios are self-contained: every image is generated in a temporary
directory from deterministic byte patterns (or, for R7, a copy of the
repository NTFS benchmark image) and CarveX is invoked through its public CLI.
Nothing is written into the source image.

Usage:
    python3 scripts/review_repro.py --carvex target/release/carvex --assert
    python3 scripts/review_repro.py --ntfs-image bench/images/all-deleted_ntfs.img \
        --xfail scripts/repro_xfail.txt --assert
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

PDF_EOF = b"%%EOF\n"
JPEG_SOI = b"\xff\xd8"
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"
PNG_IEND = b"\x00\x00\x00\x00IEND\xaeB`\x82"

# Set by main() from --fat32-image; used by the S1 scenario (R8).
FAT32_IMAGE: Path | None = None


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


# ---------------------------------------------------------------------------
# Fixture builders


def base_pdf() -> bytes:
    out = bytearray(b"%PDF-1.4\n")
    o1 = len(out)
    out += b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n"
    o2 = len(out)
    out += b"2 0 obj\n<< /Type /Pages /Kids [1 0 R] /Count 0 >>\nendobj\n"
    xref = len(out)
    out += (
        b"xref\n0 3\n0000000000 65535 f \n"
        + f"{o1:010d} 00000 n \n{o2:010d} 00000 n \n".encode()
        + b"trailer\n<< /Size 3 /Root 1 0 R >>\n"
        + f"startxref\n{xref}\n".encode()
        + PDF_EOF
    )
    return bytes(out)


def last_startxref(data: bytes) -> int:
    matches = re.findall(rb"startxref\s+(\d+)", data)
    if not matches:
        raise RuntimeError("fixture has no startxref")
    return int(matches[-1])


def incremental(base: bytes) -> bytes:
    out = bytearray(base)
    o3 = len(out)
    out += b"3 0 obj\n<< /Type /Metadata /Subtype /XML >>\nendobj\n"
    xref = len(out)
    prev = last_startxref(base)
    out += (
        b"xref\n0 1\n0000000000 65535 f \n3 1\n"
        + f"{o3:010d} 00000 n \n".encode()
        + f"trailer\n<< /Size 4 /Root 1 0 R /Prev {prev} >>\n".encode()
        + f"startxref\n{xref}\n".encode()
        + PDF_EOF
    )
    return bytes(out)


def pdf_with_embedded_pdf() -> bytes:
    inner = base_pdf()
    out = bytearray(b"%PDF-1.5\n")
    out += f"1 0 obj\n<< /Length {len(inner)} >>\nstream\n".encode()
    out += inner
    out += b"\nendstream\nendobj\n"
    xref = len(out)
    out += (
        b"xref\n0 2\n0000000000 65535 f \n0000000009 00000 n \n"
        + b"trailer\n<< /Size 2 /Root 1 0 R >>\n"
        + f"startxref\n{xref}\n".encode()
        + PDF_EOF
    )
    return bytes(out)


def jpeg_segment(marker: int, payload: bytes) -> bytes:
    return bytes([0xFF, marker]) + struct.pack(">H", len(payload) + 2) + payload


def truncated_jpeg_header() -> bytes:
    """A structurally valid JPEG start with no EOI (used by R6)."""
    out = bytearray(JPEG_SOI)
    out += jpeg_segment(0xE0, b"JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00")
    # SOF0: 8-bit, height/width, one component.
    out += jpeg_segment(0xC0, b"\x08\x01\x00\x01\x00\x01\x01\x11\x00")
    # SOS: one component, Ss=0, Se=63, AhAl=0.
    out += jpeg_segment(0xDA, b"\x01\x01\x00\x00\x3f\x00")
    return bytes(out)


# ---------------------------------------------------------------------------
# CarveX invocation


class ScanResult:
    def __init__(self, proc: subprocess.CompletedProcess[str], report: dict | None, out_dir: Path):
        self.proc = proc
        self.report = report
        self.out_dir = out_dir
        self.files = []
        if out_dir.is_dir():
            self.files = sorted(p for p in out_dir.iterdir() if p.is_file())

    @property
    def stdout(self) -> str:
        return self.proc.stdout or ""

    @property
    def stderr(self) -> str:
        return self.proc.stderr or ""


def run_carvex(
    carvex: Path,
    image: Path,
    work: Path,
    extra: list[str] | None = None,
    timeout: float = 300.0,
) -> ScanResult:
    out = work / "out"
    report_path = work / "report.json"
    shutil.rmtree(out, ignore_errors=True)
    report_path.unlink(missing_ok=True)
    cmd = [
        str(carvex),
        "scan",
        str(image),
        "-o",
        str(out),
        "--quiet",
        "--json-report",
        str(report_path),
    ] + (extra or [])
    proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    report = None
    if report_path.is_file():
        try:
            report = json.loads(report_path.read_text())
        except json.JSONDecodeError:
            report = None
    return ScanResult(proc, report, out)


# ---------------------------------------------------------------------------
# Scenarios


def case_r1(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """F-01: a PDF revision after an early %%EOF must not be lost."""
    filler = b" " * (2 * 1024 * 1024 - len(base_pdf()) - 512)
    doc = incremental(base_pdf() + filler)
    image = work / "r1.img"
    image.write_bytes(doc + b"\x00" * 4096)
    res = run_carvex(carvex, image, work, ["--mode", "raw", "--no-fs", "--types", "pdf"])
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()}"
    if not res.files:
        return "no PDF carved"
    got = [p for p in res.files if p.suffix == ".pdf"]
    if len(got) != 1:
        return f"expected 1 PDF output, got {len(got)}"
    if got[0].stat().st_size != len(doc):
        return f"carved {got[0].stat().st_size} of {len(doc)} bytes (early EOF)"
    if sha256(got[0].read_bytes()) != sha256(doc):
        return "carved bytes differ from the fixture"
    return ""


def case_r2(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """F-01: incremental update crossing the initial 1 MiB read window."""
    base = base_pdf()
    target = 1024 * 1024 - 64 - len(base)
    base = base[: -len(PDF_EOF)] + b" " * target + PDF_EOF
    doc = incremental(base)
    image = work / "r2.img"
    image.write_bytes(doc + b"\x00" * 4096)
    res = run_carvex(carvex, image, work, ["--mode", "raw", "--no-fs", "--types", "pdf"])
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()}"
    got = [p for p in res.files if p.suffix == ".pdf"]
    if len(got) != 1:
        return f"expected 1 PDF output, got {len(got)}"
    if sha256(got[0].read_bytes()) != sha256(doc):
        return f"carved {got[0].stat().st_size} of {len(doc)} bytes"
    return ""


def case_r3(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """F-02: an embedded PDF must not cut the containing PDF short."""
    outer = pdf_with_embedded_pdf()
    image = work / "r3.img"
    image.write_bytes(outer + b"\x00" * 4096)
    res = run_carvex(carvex, image, work, ["--mode", "raw", "--no-fs", "--types", "pdf"])
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()}"
    got = [p for p in res.files if p.suffix == ".pdf"]
    if len(got) != 1:
        return f"expected only the outer PDF, got {len(got)} PDF(s)"
    if sha256(got[0].read_bytes()) != sha256(outer):
        return "outer PDF was truncated at the embedded header"
    return ""


def _corpus_jpegs(corpus: Path) -> list[Path]:
    return sorted(
        p
        for p in (corpus / "jpeg").iterdir()
        if p.suffix.lower() in (".jpg", ".jpeg") and p.stat().st_size > 10_000
    )


def case_r4a(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """F-05: a truncated JPEG head should still be salvaged."""
    source = _corpus_jpegs(corpus)[0].read_bytes()
    partial = source[: len(source) - len(source) // 3] + b"\x00" * 8192
    image = work / "r4a.img"
    image.write_bytes(b"\x00" * 65536 + partial + b"\x00" * 65536)
    res = run_carvex(carvex, image, work, ["--mode", "raw", "--no-fs", "--types", "jpeg"])
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()}"
    got = [p for p in res.files if p.suffix in (".jpg", ".jpeg")]
    if not got:
        return "no JPEG salvaged from a truncated head"
    data = got[0].read_bytes()
    if not data.startswith(JPEG_SOI):
        return "salvaged file does not start with SOI"
    return ""


def case_r4b(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """F-05: a PNG whose IEND is missing should keep its valid prefix."""
    pngs = sorted(p for p in (corpus / "png").glob("*.png") if p.stat().st_size > 4000)
    source = pngs[0].read_bytes()
    iend = source.rfind(PNG_IEND)
    if iend < 0:
        return "SKIP: corpus PNG has no IEND"
    partial = source[:iend] + b"\x00" * 8192
    image = work / "r4b.img"
    image.write_bytes(b"\x00" * 65536 + partial + b"\x00" * 65536)
    res = run_carvex(carvex, image, work, ["--mode", "raw", "--no-fs", "--types", "png"])
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()}"
    got = [p for p in res.files if p.suffix == ".png"]
    if not got:
        return "no PNG salvaged from a missing IEND"
    data = got[0].read_bytes()
    if not data.startswith(PNG_MAGIC):
        return "salvaged file does not start with the PNG signature"
    return ""


def case_r4c(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """F-04: a spliced JPEG is structurally complete but must not decode."""
    jpegs = _corpus_jpegs(corpus)
    a = jpegs[0].read_bytes()
    b = jpegs[1].read_bytes()
    sos_a = a.find(b"\xff\xda")
    sos_b = b.find(b"\xff\xda")
    if sos_a < 0 or sos_b < 0:
        return "SKIP: corpus JPEG has no SOS"
    cut = sos_a + (len(a) - sos_a) * 2 // 3
    tail = sos_b + (len(b) - sos_b) // 3
    spliced = a[:cut] + b[tail:]
    image = work / "r4c.img"
    image.write_bytes(b"\x00" * 65536 + spliced + b"\x00" * 65536)
    res = run_carvex(
        carvex,
        image,
        work,
        ["--mode", "raw", "--no-fs", "--types", "jpeg", "--validate", "decode", "--min-score", "0"],
    )
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()}"
    got = [p for p in res.files if p.suffix in (".jpg", ".jpeg")]
    if not got:
        return "no JPEG output (cannot evaluate decode validation)"
    if res.report is None:
        return "missing JSON report"
    entries = [f for f in res.report.get("files", []) if f.get("type") == "jpeg"]
    if not entries:
        return "report has no JPEG entry"
    if any(e.get("validation") == "decoded" for e in entries):
        return "spliced JPEG was reported as decoded"
    return ""


def case_r5(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """F-06: 25,811 fake JPEG headers in 64 MiB must not take seconds."""
    size = 64 * 1024 * 1024
    data = bytearray(size)
    header = b"\xff\xd8\xff\xe0\x00\x00"
    for off in range(0, size - len(header), 4096):
        data[off : off + len(header)] = header
    image = work / "r5.img"
    image.write_bytes(data)
    start = time.monotonic()
    res = run_carvex(
        carvex, image, work, ["--mode", "raw", "--no-fs", "--types", "jpeg", "--min-size", "1"]
    )
    elapsed = time.monotonic() - start
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()}"
    if res.files:
        return f"{len(res.files)} false-positive file(s) written"
    if elapsed > 1.0:
        return f"took {elapsed:.2f}s (target <= 1.0s)"
    return ""


def case_r6(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """F-07: a header followed by a zero region must not produce 64 MiB."""
    data = truncated_jpeg_header() + b"\x00" * (64 * 1024 * 1024)
    image = work / "r6.img"
    image.write_bytes(data)
    res = run_carvex(carvex, image, work, ["--mode", "raw", "--no-fs", "--types", "jpeg"])
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()}"
    huge = [p for p in res.files if p.stat().st_size > 1024 * 1024]
    if huge:
        return f"{len(huge)} output(s) larger than 1 MiB (max {max(p.stat().st_size for p in huge)})"
    return ""


def _ntfs_offsets(image: Path) -> tuple[int, int, int]:
    data = image.read_bytes()
    if data[510:512] != b"\x55\xaa":
        raise RuntimeError("R7 fixture is not an MBR image")
    part = struct.unpack_from("<I", data, 446 + 8)[0] * 512
    boot = data[part : part + 512]
    if boot[3:7] != b"NTFS":
        raise RuntimeError("R7 partition is not NTFS")
    bps = struct.unpack_from("<H", boot, 0x0B)[0]
    spc = boot[0x0D]
    mft_lcn = struct.unpack_from("<Q", boot, 0x30)[0]
    return part, bps, mft_lcn * spc * bps


def case_r7(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """F-03: corrupt NTFS metadata must fall back, not crash."""
    if ntfs is None or not ntfs.is_file():
        return "SKIP: no NTFS image (pass --ntfs-image)"
    image = work / "r7.img"
    shutil.copyfile(ntfs, image)
    part, _, mft_byte = _ntfs_offsets(image)
    with image.open("r+b") as f:
        f.seek(part + mft_byte)
        f.write(b"BAAD")
    res = run_carvex(carvex, image, work, ["--mode", "auto", "--types", "jpeg"])
    if res.proc.returncode != 3:
        return f"expected exit 3 (warnings), got {res.proc.returncode}"
    if "raw scan" not in res.stderr:
        return "no raw-scan fallback warning on stderr"
    if "panicked" in res.stderr.lower():
        return "parser panicked instead of returning an error"
    if res.report is not None and res.report.get("mode") != "raw":
        return f"report mode is {res.report.get('mode')!r}, expected 'raw'"
    return ""


def _volume_bytes(path: Path) -> bytes:
    """Partition payload of a benchmark MBR image (skip its own MBR + gap)."""
    data = path.read_bytes()
    if data[510:512] != b"\x55\xaa":
        raise RuntimeError(f"{path.name} is not an MBR image")
    for i in range(4):
        e = 446 + i * 16
        if data[e + 4] == 0:
            continue
        start = struct.unpack_from("<I", data, e + 8)[0] * 512
        sectors = struct.unpack_from("<I", data, e + 12)[0]
        if start + sectors * 512 <= len(data) and sectors > 0:
            return data[start : start + sectors * 512]
    raise RuntimeError(f"no partition found in {path.name}")


def case_r8(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """A-07 / S1: never skip a volume or a gap silently.

    MBR disk with a 1 MiB gap, a FAT32 partition and an NTFS partition, both
    built from the repository benchmark images (60 recoverable files in
    total):

      * default                       -> all volumes + gap, exit 0;
      * --partition largest --no-gaps -> historical 30 files, warnings, exit 3;
      * --partition all --no-gaps     -> both volumes, gap still skipped;
      * --partition all --include-gaps -> nothing unscanned, exit 0.
    """
    fat32 = FAT32_IMAGE
    if fat32 is None or not fat32.is_file():
        return "SKIP: no FAT32 image (pass --fat32-image)"
    if ntfs is None or not ntfs.is_file():
        return "SKIP: no NTFS image (pass --ntfs-image)"

    fat = _volume_bytes(fat32)
    nts = _volume_bytes(ntfs)
    gap = 1024 * 1024
    img = bytearray(gap + len(fat) + len(nts))
    img[510:512] = b"\x55\xaa"
    for i, (start, type_id, size) in enumerate(
        [(gap, 0x0C, len(fat)), (gap + len(fat), 0x07, len(nts))]
    ):
        e = 446 + i * 16
        img[e] = 0x00
        img[e + 4] = type_id
        struct.pack_into("<I", img, e + 8, start // 512)
        struct.pack_into("<I", img, e + 12, size // 512)
    img[gap : gap + len(fat)] = fat
    img[gap + len(fat) :] = nts
    image = work / "s1.img"
    image.write_bytes(bytes(img))

    def produced(res: ScanResult) -> list[Path]:
        return sorted(p for p in res.out_dir.rglob("*") if p.is_file())

    def scanned_flags(res: ScanResult) -> list[bool | None]:
        return [v.get("scanned") for v in (res.report or {}).get("volumes") or []]

    # 1. Default (A-07 step 2): every volume and every gap is read.
    res = run_carvex(carvex, image, work)
    if res.proc.returncode != 0:
        return f"default run: expected exit 0, got {res.proc.returncode}: {res.stderr.strip()}"
    codes = set(re.findall(r"warning\[([A-Z_]+)\]", res.stderr))
    if {"VOLUME_NOT_SCANNED", "GAP_NOT_SCANNED"} & codes:
        return f"default run: unexpected skip warning(s) {sorted(codes)}"
    if res.report is None:
        return "default run: no JSON report"
    if scanned_flags(res) != [True, True]:
        return f"default run: wrong scanned flags {scanned_flags(res)}"
    options = res.report.get("options") or {}
    if options.get("partition") != "all" or options.get("include_gaps") is not True:
        return f"default run: wrong options {options}"
    regions = res.report.get("regions") or []
    if not any(r.get("kind") == "gap" and r.get("scanned") is True for r in regions):
        return "default run: regions[] does not mark the gap as scanned"
    if len(produced(res)) != 60:
        return f"default run: expected 60 files, got {len(produced(res))}"
    for sub in ("part1", "part2"):
        if not (res.out_dir / sub).is_dir():
            return f"default run: missing {sub}/ sub-directory"

    # 2. Historical behaviour is still available explicitly.
    res2 = run_carvex(carvex, image, work, ["--partition", "largest", "--no-gaps"])
    if res2.proc.returncode != 3:
        return f"--partition largest --no-gaps: expected exit 3, got {res2.proc.returncode}"
    codes2 = set(re.findall(r"warning\[([A-Z_]+)\]", res2.stderr))
    missing = {"VOLUME_NOT_SCANNED", "GAP_NOT_SCANNED"} - codes2
    if missing:
        return f"legacy run: missing warning code(s) {sorted(missing)} (got {sorted(codes2)})"
    if scanned_flags(res2) != [True, False]:
        return f"legacy run: wrong scanned flags {scanned_flags(res2)}"
    regions2 = (res2.report or {}).get("regions") or []
    if not any(r.get("kind") == "gap" and r.get("scanned") is False for r in regions2):
        return "legacy run: regions[] does not list the unscanned gap"
    if len(produced(res2)) != 30:
        return f"legacy run: expected 30 files, got {len(produced(res2))}"

    # 3. `--partition all --no-gaps` leaves only the gap unread.
    res3 = run_carvex(carvex, image, work, ["--partition", "all", "--no-gaps"])
    if res3.proc.returncode != 3:
        return f"--partition all --no-gaps: expected exit 3 (gap), got {res3.proc.returncode}"
    if "VOLUME_NOT_SCANNED" in res3.stderr:
        return "--partition all --no-gaps: still warns about an unscanned volume"
    if "GAP_NOT_SCANNED" not in res3.stderr:
        return "--partition all --no-gaps: missing GAP_NOT_SCANNED"
    if scanned_flags(res3) != [True, True]:
        return f"--partition all --no-gaps: wrong scanned flags {scanned_flags(res3)}"
    if len(produced(res3)) != 60:
        return f"--partition all --no-gaps: expected 60 files, got {len(produced(res3))}"

    # 4. The old `--include-gaps` flag is still accepted.
    res4 = run_carvex(carvex, image, work, ["--partition", "all", "--include-gaps"])
    if res4.proc.returncode != 0:
        return f"--include-gaps: expected exit 0, got {res4.proc.returncode}: {res4.stderr.strip()}"
    if "GAP_NOT_SCANNED" in res4.stderr:
        return "--include-gaps: gap warning persists"
    regions4 = (res4.report or {}).get("regions") or []
    skipped = [
        r
        for r in regions4
        if r.get("kind") in ("gap", "unsupported") and r.get("scanned") is not True
    ]
    if skipped:
        return f"--include-gaps: regions[] still lists unscanned regions {skipped}"
    if len(produced(res4)) != 60:
        return f"--include-gaps: expected 60 files, got {len(produced(res4))}"
    return ""


CASES = [
    ("R1", "PDF revision after an early %%EOF", case_r1),
    ("R2", "incremental update across the 1 MiB window", case_r2),
    ("R3", "embedded PDF is not a boundary", case_r3),
    ("R4a", "truncated JPEG head is salvaged", case_r4a),
    ("R4b", "PNG without IEND is salvaged", case_r4b),
    ("R4c", "spliced JPEG is not decoded", case_r4c),
    ("R5", "fake-header flood performance", case_r5),
    ("R6", "zero-tail truncation", case_r6),
    ("R7", "corrupt NTFS falls back to raw", case_r7),
    ("R8", "two volumes and a gap are never skipped silently (S1)", case_r8),
]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--carvex", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, default=Path("bench/corpus"))
    parser.add_argument("--ntfs-image", type=Path, default=None)
    parser.add_argument("--fat32-image", type=Path, default=None)
    parser.add_argument("--workdir", type=Path, default=None)
    parser.add_argument("--assert", dest="assert_mode", action="store_true")
    parser.add_argument("--xfail", type=Path, default=None)
    args = parser.parse_args()

    global FAT32_IMAGE
    FAT32_IMAGE = args.fat32_image.resolve() if args.fat32_image else None

    carvex = args.carvex.resolve()
    if not carvex.is_file():
        raise SystemExit(f"carvex binary not found: {carvex}")
    corpus = args.corpus.resolve()
    if not corpus.is_dir():
        raise SystemExit(f"corpus not found: {corpus}")
    xfail: set[str] = set()
    if args.xfail and args.xfail.is_file():
        xfail = {
            line.split()[0]
            for line in args.xfail.read_text().splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        }

    temp = None
    if args.workdir:
        root = args.workdir.resolve()
        root.mkdir(parents=True, exist_ok=True)
    else:
        temp = tempfile.TemporaryDirectory(prefix="carvex-repro-")
        root = Path(temp.name)

    failures = 0
    xpasses = 0
    for ident, title, func in CASES:
        work = root / ident
        shutil.rmtree(work, ignore_errors=True)
        work.mkdir(parents=True)
        try:
            error = func(carvex, work, corpus, args.ntfs_image)
        except subprocess.TimeoutExpired:
            error = "timeout"
        except Exception as exc:  # fixture/run error is a failing case
            error = f"error: {exc}"
        expected_fail = ident in xfail
        if error:
            if expected_fail:
                print(f"XFAIL {ident:<4} {title}: {error}")
            else:
                failures += 1
                print(f"FAIL  {ident:<4} {title}: {error}")
        else:
            if expected_fail:
                xpasses += 1
                print(f"XPASS {ident:<4} {title}: remove it from the xfail list")
            else:
                print(f"PASS  {ident:<4} {title}")

    if temp is not None:
        temp.cleanup()
    print()
    print(f"{len(CASES)} scenario(s), {failures} failure(s), {xpasses} XPASS")
    if args.assert_mode and (failures or xpasses):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
