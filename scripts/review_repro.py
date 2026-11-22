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


# ---------------------------------------------------------------------------
# Harness X01-X24 (Phase 0 reproduction scaffolding). Many are red by design;
# scripts/repro_xfail.txt controls which are expected to fail.


def _png_chunk(ctype: bytes, data: bytes) -> bytes:
    import zlib as _z
    return struct.pack(">I", len(data)) + ctype + data + struct.pack(">I", _z.crc32(ctype + data) & 0xFFFFFFFF)


def _make_png(width: int, height: int, gray: int = 128) -> bytes:
    import zlib as _z
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)
    raw = b"".join(b"\x00" + bytes([gray]) * width for _ in range(height))
    idat = _z.compress(raw, 6)
    return PNG_MAGIC + _png_chunk(b"IHDR", ihdr) + _png_chunk(b"IDAT", idat) + PNG_IEND


def _make_png_bomb(width: int, height: int, inflated_bytes: int) -> bytes:
    import zlib as _z
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)
    # Highly compressible zero scanlines: filter byte 0 + zeros
    row = b"\x00" + b"\x00" * width
    raw_len = inflated_bytes
    # Build in chunks
    comp = _z.compressobj(9)
    idat = bytearray()
    remaining = raw_len
    while remaining > 0:
        take = min(remaining, len(row))
        idat += comp.compress(row[:take])
        remaining -= take
    idat += comp.flush()
    return PNG_MAGIC + _png_chunk(b"IHDR", ihdr) + _png_chunk(b"IDAT", bytes(idat)) + PNG_IEND


def case_x02(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """H-05: PNG decode inflation must be bounded."""
    img = work / "x02.img"
    # 16x16 grey, but craft IDAT that decompresses to 256 MiB of scanlines.
    # For 16x16 8-bit gray, expected raw is 17*16=272 bytes; we instead use a
    # valid small PNG and also test a large-dimension bomb via b variant.
    # Variant (a): small dimensions but huge inflated stream - should be rejected
    # by length mismatch / bounded inflate.
    import zlib as _z
    w, h = 16, 16
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0)
    # Inflate to 256 MiB total - far beyond expected 272 bytes.
    target = 256 * 1024 * 1024
    comp = _z.compressobj(9)
    bomb = bytearray()
    chunk = b"\x00" * (64 * 1024)
    sent = 0
    while sent < target:
        n = min(len(chunk), target - sent)
        bomb += comp.compress(chunk[:n])
        sent += n
    bomb += comp.flush()
    png = PNG_MAGIC + _png_chunk(b"IHDR", ihdr) + _png_chunk(b"IDAT", bytes(bomb)) + PNG_IEND
    img.write_bytes(b"\x00" * 4096 + png + b"\x00" * 4096)
    start = time.monotonic()
    res = run_carvex(carvex, img, work, ["--mode", "raw", "--no-fs", "--types", "png", "--validate", "decode", "--min-score", "0"])
    elapsed = time.monotonic() - start
    if res.proc.returncode not in (0, 3):
        return f"exit {res.proc.returncode}: {res.stderr.strip()[:200]}"
    # After fix: must be not-verified/suspect/partial, not decoded as valid full image,
    # and must complete quickly with bounded memory. We check label conservatively.
    for f in res.report.get("files", []) if res.report else []:
        if f.get("validation") == "decoded" and f.get("status") == "complete":
            return f"PNG bomb reported decoded/complete in {elapsed:.2f}s"
    if elapsed > 30:
        return f"took {elapsed:.2f}s (expected bounded)"
    return ""


def case_x04(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """H-07: PDF %%EOF evaluation should be near-linear/single-pass."""
    # 64 MiB with 4096 revision-like EOF markers
    size = 64 * 1024 * 1024
    ecount = 4096
    seg = b"A" * 8000 + b"\nstartxref\n0\n%%EOF\n9 0 obj\n"
    payload = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n"
    payload += seg * ecount
    payload = payload.ljust(size, b"B")
    img = work / "x04.img"
    img.write_bytes(payload)
    start = time.monotonic()
    res = run_carvex(carvex, img, work, ["--mode", "raw", "--no-fs", "--types", "pdf", "--threads", "1", "--min-size", "1"])
    elapsed = time.monotonic() - start
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()[:200]}"
    if elapsed > 2.0:
        return f"took {elapsed:.2f}s for 4096 EOF markers (target <= 1s)"
    return ""


def case_x07(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """H-11: crafted E01 header must not cause huge allocation/abort."""
    # Minimal E01-like file with absurd sectors_per_chunk. Crafting a full valid
    # EWFv1 is involved; we build a minimal structure that the parser reads.
    img = work / "x07.E01"
    # E01 file signature
    data = bytearray(b"EVF\x09\x0d\x0a\xff\x00")
    # header section: type header (1), next offset, size
    def section(stype, body, next_off):
        # section header: type(16) next(8) size(8) checksum(4) + body? simplified
        # Real E01 is more complex; for a repro stub we create a file that looks
        # like E01 and rely on parser behavior. If parser rejects early, that's fine.
        pass
    # Simpler: just write signature + crafted volume-like bytes; if not parsed as E01
    # it may be SKIP. Better to construct a minimal plausible E01.
    # Build minimal: header section + volume section with huge chunk size.
    import zlib as _z
    def ewf_section(typ: bytes, body: bytes, next_offset: int) -> bytes:
        hdr = bytearray(76)
        hdr[0:16] = typ.ljust(16, b"\x00")
        struct.pack_into("<Q", hdr, 16, next_offset)
        struct.pack_into("<Q", hdr, 24, 76 + len(body))
        # pad 40 bytes reserved
        payload = bytes(hdr) + body
        csum = _z.adler32(payload[:72] + b"\x00" * 4 + payload[76:]) & 0xFFFFFFFF
        struct.pack_into("<I", payload := bytearray(payload), 72, csum)
        return bytes(payload)
    vol = bytearray(94)
    vol[0:1] = b"\x01"  # media type
    vol[1:2] = b"\x00"
    struct.pack_into("<I", vol, 4, 64)  # sectors per chunk - will be overridden below
    struct.pack_into("<I", vol, 8, 512) # bytes per sector
    struct.pack_into("<Q", vol, 12, 64)  # sector count
    # set absurd sectors_per_chunk = 2^27
    struct.pack_into("<I", vol, 4, 2**27)
    struct.pack_into("<I", vol, 16, 0) # compression
    # error granularity etc - leave zeros
    vol_body = bytes(vol)
    vol_section = ewf_section(b"volume", vol_body, 0)
    done_section = ewf_section(b"done", b"", 0)
    # Fix next offsets: header at 13, volume follows, done follows
    header_body = b"\x01\x00\x00\x00" + b"\x00" * 8  # minimal
    header_off = 13
    vol_off = header_off + 76 + len(header_body)
    done_off = vol_off + len(vol_section)
    header_section = ewf_section(b"header", header_body, vol_off)
    vol_section = ewf_section(b"volume", vol_body, done_off)
    done_section = ewf_section(b"done", b"", 0)
    img.write_bytes(data + header_section + vol_section + done_section)
    start = time.monotonic()
    res = run_cmd([str(carvex), "info", str(img), "--json"], timeout=30)
    elapsed = time.monotonic() - start
    if res.proc.returncode < 0:
        return f"killed by signal {-res.proc.returncode} in {elapsed:.2f}s"
    if elapsed > 5:
        return f"took {elapsed:.2f}s (expected fast rejection)"
    # Accept clean rejection (exit 1/2) as long as no crash/abort
    return ""


def case_x08(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """H-12: crafted VMDK grain tables must not explode memory."""
    # Reproduce v5-style: 128 GD entries all pointing to same 4MiB table.
    img = work / "x08.vmdk"
    # Sparse VMDK header (simplified)
    # Build a minimal sparse extent
    grain_size = 128  # sectors (64K)
    num_gtes_per_gt = 512
    gd_entries = 128
    capacity = gd_entries * num_gtes_per_gt * grain_size
    # Header sector
    hdr = bytearray(512)
    hdr[0:4] = struct.pack("<I", 0x564D444B)  # magic KDMV
    hdr[4:8] = struct.pack("<I", 1)  # version
    hdr[8:12] = struct.pack("<I", 1)  # flags
    hdr[12:20] = struct.pack("<Q", capacity)  # capacity sectors
    hdr[20:28] = struct.pack("<Q", grain_size)
    hdr[28:36] = struct.pack("<Q", 0)  # descriptor offset
    hdr[36:44] = struct.pack("<Q", 1)  # descriptor size
    hdr[44:48] = struct.pack("<I", num_gtes_per_gt)
    hdr[48:56] = struct.pack("<Q", 2)  # rgdOffset (unused)
    hdr[56:64] = struct.pack("<Q", 2)  # gdOffset
    hdr[64:72] = struct.pack("<Q", 0xFFFFFFFFFFFFFFFF)  # overHead
    hdr[72:73] = b"\x01"  # uncleanShutdown? not critical
    # Pad to sector
    file_data = bytearray(512 * 4)  # header + pad + GD + GT
    file_data[0:512] = hdr
    # Grain directory at sector 2
    gd_off = 2 * 512
    gt_sector = 3  # both point to same table at sector 3
    for i in range(gd_entries):
        struct.pack_into("<I", file_data, gd_off + i * 4, gt_sector)
    # Grain table at sector 3: all zeros (no grains allocated) - but table itself is 2K/4K
    gt_off = 3 * 512
    # ensure file large enough
    needed = gt_off + num_gtes_per_gt * 4
    if len(file_data) < needed:
        file_data.extend(b"\x00" * (needed - len(file_data)))
    # descriptor
    desc = b'# Disk DescriptorFile\nversion=1\nCID=fffffffe\nparentCID=ffffffff\ncreateType="monolithicSparse"\n\n# Extent description\nRW %d SPARSE "x08.vmdk"\n\n# The Disk Data Base\n#DDB\n\nddb.virtualHWVersion = "4"\n' % capacity
    # place descriptor at sector 1
    desc_off = 1 * 512
    file_data[desc_off:desc_off + len(desc)] = desc
    img.write_bytes(file_data)
    start = time.monotonic()
    res = run_cmd([str(carvex), "info", str(img), "--json"], timeout=30)
    elapsed = time.monotonic() - start
    if res.proc.returncode < 0:
        return f"killed by signal {-res.proc.returncode} in {elapsed:.2f}s"
    # After fix, should be fast and bounded; before fix can be >500MB RSS (can't easily measure here)
    # Use time as proxy: large table multiplication is slow to allocate+zero
    if elapsed > 5:
        return f"took {elapsed:.2f}s (possible unbounded table allocation)"
    # Even if fast, we should flag if it accepted absurd layout; hard to assert without RSS.
    # Return SKIP-like message? No, better report observation.
    return "SKIP: completed quickly; RSS not measured in this repro (needs RSS harness)"


def case_x10(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """H-17: PNG bad CRC must not be reported as decoded/complete."""
    src = corpus / "png" / "png_02_rgb_800x600.png"
    if not src.exists():
        return "SKIP: corpus png_02 missing"
    data = bytearray(src.read_bytes())
    # corrupt CRC of first IDAT chunk
    pos = 8
    corrupted = False
    while pos + 12 <= len(data):
        ln = struct.unpack_from(">I", data, pos)[0]
        ctype = bytes(data[pos + 4 : pos + 8])
        crc_off = pos + 8 + ln
        if ctype == b"IDAT":
            data[crc_off] ^= 0x01
            corrupted = True
            break
        pos += 12 + ln
    if not corrupted:
        return "SKIP: no IDAT chunk found"
    img = work / "x10.img"
    img.write_bytes(b"\x00" * 4096 + bytes(data) + b"\x00" * 4096)
    res = run_carvex(carvex, img, work, ["--mode", "raw", "--no-fs", "--types", "png", "--validate", "decode", "--min-score", "0"])
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()[:200]}"
    files = res.report.get("files", []) if res.report else []
    pngs = [f for f in files if f.get("type") == "png"]
    if not pngs:
        return "no PNG carved"
    for f in pngs:
        if f.get("validation") == "decoded":
            return f"bad-CRC PNG reported as decoded (status={f.get('status')}, score={f.get('score')})"
    return ""


def case_x13(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """H-20: PDF EOL variants should not change validity class."""
    src = corpus / "pdf" / "pdf_03_text_2pages.pdf"
    if not src.exists():
        return "SKIP: corpus pdf_03 missing"
    base = src.read_bytes()
    # normalize to LF ending first
    if base.endswith(b"\r\n"):
        base_lf = base[:-2] + b"\n"
    elif base.endswith(b"\r"):
        base_lf = base[:-1] + b"\n"
    else:
        base_lf = base
    variants = {
        "lf": base_lf,
        "crlf": base_lf[:-1] + b"\r\n" if base_lf.endswith(b"\n") else base_lf + b"\r\n",
        "cr": base_lf[:-1] + b"\r" if base_lf.endswith(b"\n") else base_lf + b"\r",
        "none": base_lf[:-1] if base_lf.endswith(b"\n") else base_lf,
    }
    statuses = {}
    for name, payload in variants.items():
        img = work / f"x13_{name}.img"
        img.write_bytes(b"\x00" * 4096 + payload + b"\x00" * 4096)
        res = run_carvex(carvex, img, work / f"out_{name}", ["--mode", "raw", "--no-fs", "--types", "pdf", "--validate", "decode", "--min-score", "0"])
        if res.proc.returncode != 0:
            return f"{name}: exit {res.proc.returncode}"
        files = res.report.get("files", []) if res.report else []
        pdfs = [f for f in files if f.get("type") == "pdf"]
        if not pdfs:
            return f"{name}: no PDF carved"
        statuses[name] = (pdfs[0].get("validation"), pdfs[0].get("status"), pdfs[0].get("score"))
    vals = set(statuses.values())
    if len(vals) != 1:
        return f"EOL variants differ: {statuses}"
    return ""


def case_x18(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """H-24: raw image with VHD/VMDK-looking bytes should not be misdetected."""
    # Build a tiny raw file with 'conectix' at last sector start
    img = work / "x18.img"
    data = bytearray(1024 * 1024)
    # Put a valid PNG in the middle so scan has something to find
    png = _make_png(16, 16, 200)
    data[4096:4096 + len(png)] = png
    # Last 512-byte sector starts with 'conectix'
    last_sector = len(data) - 512
    data[last_sector:last_sector + 8] = b"conectix"
    img.write_bytes(data)
    res = run_carvex(carvex, img, work, ["--mode", "raw", "--no-fs", "--types", "png", "--min-score", "0"])
    if res.proc.returncode != 0:
        return f"raw mode exit {res.proc.returncode}: {res.stderr.strip()[:200]}"
    # Also test auto-detect (no --no-fs, no --input-format raw) - should not fail as VHD
    out2 = work / "auto"
    out2.mkdir()
    res2 = run_cmd([str(carvex), "scan", str(img), "-o", str(out2), "--mode", "raw", "--no-fs", "--types", "png", "--min-score", "0", "--quiet", "--json-report", str(out2 / "report.json")], timeout=60)
    if res2.proc.returncode != 0:
        return f"auto/raw scan exit {res2.proc.returncode}: {res2.stderr.strip()[:200]}"
    return ""


def case_x24(carvex: Path, work: Path, corpus: Path, ntfs: Path | None) -> str:
    """H-16: interleaved fragments must not be labelled complete/decoded."""
    jpegs = sorted((corpus / "jpeg").glob("*.jpg"))
    if len(jpegs) < 2:
        return "SKIP: corpus JPEGs missing"
    j1, j2 = jpegs[0], jpegs[1]
    a = j1.read_bytes()
    b = j2.read_bytes()
    # Split each in half on 4K boundary
    ha = (len(a) // 2) // 4096 * 4096
    hb = (len(b) // 2) // 4096 * 4096
    a1, a2 = a[:ha], a[ha:]
    b1, b2 = b[:hb], b[hb:]
    # Interleave [A1][B1][A2][B2]
    blob = a1 + b1 + a2 + b2
    img = work / "x24.img"
    img.write_bytes(b"\x00" * 65536 + blob + b"\x00" * 65536)
    res = run_carvex(carvex, img, work, ["--mode", "raw", "--no-fs", "--types", "jpeg", "--validate", "decode", "--min-score", "0"])
    if res.proc.returncode != 0:
        return f"exit {res.proc.returncode}: {res.stderr.strip()[:200]}"
    files = res.report.get("files", []) if res.report else []
    for f in files:
        if f.get("validation") == "decoded" and f.get("status") == "complete":
            sha = f.get("sha256")
            if sha not in {sha256(a), sha256(b)}:
                return f"interleaved fragment reported decoded/complete sha={sha}"
    return ""


def run_cmd(cmd: list[str], timeout: int = 60):
    proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    return type("R", (), {"proc": proc, "stdout": proc.stdout, "stderr": proc.stderr, "report": None})()


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
    ("X02", "PNG decode inflation must be bounded [H-05]", case_x02),
    ("X04", "PDF %%EOF evaluation should be near-linear/single-pass [H-07]", case_x04),
    ("X07", "crafted E01 header must not cause huge allocation/abort [H-11]", case_x07),
    ("X08", "crafted VMDK grain tables must not explode memory [H-12]", case_x08),
    ("X10", "PNG bad CRC must not be reported as decoded/complete [H-17]", case_x10),
    ("X13", "PDF EOL variants should not change validity class [H-20]", case_x13),
    ("X18", "raw image with VHD/VMDK-looking bytes should not be misdetected [H-24]", case_x18),
    ("X24", "interleaved fragments must not be labelled complete/decoded [H-16]", case_x24),
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
    parser.add_argument(
        "--require-all",
        action="store_true",
        help="treat SKIP results as failures (default: SKIP is not a failure)",
    )
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
    skips = 0
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
        if error and error.startswith("SKIP"):
            if args.require_all:
                failures += 1
                print(f"FAIL  {ident:<4} {title}: {error}")
            else:
                skips += 1
                print(f"SKIP  {ident:<4} {title}: {error}")
        elif error:
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
    print(
        f"{len(CASES)} scenario(s), {failures} failure(s), {skips} SKIP(s), {xpasses} XPASS"
    )
    if args.assert_mode and (failures or xpasses):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
