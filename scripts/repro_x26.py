#!/usr/bin/env python3
"""X26: declared-size containers must not drive proportional task planning.

Three hostile declarations are exercised: a dynamic VHD with a 2^50-byte
maximum size and a one-entry BAT, an E01 with a 2^50-sector media declaration
and one chunk table entry, and a 100,000-segment split set. Each child runs
under RLIMIT_AS and its peak RSS comes from wait4.
"""
from __future__ import annotations

import argparse
import os
import resource
import struct
import sys
import tempfile
import time
import zlib
from pathlib import Path


def ones_complement(data: bytes) -> int:
    return (~sum(data)) & 0xFFFFFFFF


def section(kind: str, body: bytes) -> bytes:
    header = bytearray(76)
    raw = kind.encode()
    header[: len(raw)] = raw
    struct.pack_into("<Q", header, 24, 76 + len(body))
    return bytes(header) + body


def patch_next(buf: bytearray, pos: int, next_offset: int) -> None:
    struct.pack_into("<Q", buf, pos + 16, next_offset)


def build_vhd(work: Path) -> Path:
    path = work / "huge.vhd"
    footer = bytearray(512)
    footer[0:8] = b"conectix"
    struct.pack_into(">Q", footer, 16, 512)  # dynamic header offset
    struct.pack_into(">Q", footer, 48, 1 << 50)  # declared guest size
    struct.pack_into(">I", footer, 60, 3)  # dynamic disk
    struct.pack_into(">I", footer, 64, ones_complement(footer))
    header = bytearray(1024)
    header[0:8] = b"cxsparse"
    struct.pack_into(">Q", header, 16, 1536)  # BAT offset
    struct.pack_into(">I", header, 28, 1)  # one BAT entry
    struct.pack_into(">I", header, 32, 2 * 1024 * 1024)  # block size
    struct.pack_into(">I", header, 36, ones_complement(header))
    data = bytearray(512) + header + struct.pack(">I", 0xFFFFFFFF) + footer
    path.write_bytes(data)
    return path


def build_e01(work: Path) -> Path:
    path = work / "huge.E01"
    chunk_size = 32 * 1024
    chunk = bytes(chunk_size)
    out = bytearray(b"EVF\x09\x0d\x0a\xff\x00")
    out.extend(b"\x01" + (1).to_bytes(2, "little") + (0).to_bytes(2, "little"))

    volume_desc = len(out)
    volume = bytearray(1052)
    struct.pack_into("<I", volume, 8, chunk_size // 512)
    struct.pack_into("<I", volume, 12, 512)
    struct.pack_into("<Q", volume, 16, 1 << 50)  # declared sector count
    out.extend(section("volume", bytes(volume)))
    patch_next(out, volume_desc, len(out))

    sectors_desc = len(out)
    chunk_offset = len(out)
    out.extend(chunk)
    out.extend(zlib.adler32(chunk).to_bytes(4, "little"))
    patch_next(out, sectors_desc, len(out))

    table_desc = len(out)
    table = bytearray(24)
    struct.pack_into("<I", table, 0, 1)
    table.extend(struct.pack("<I", chunk_offset))
    out.extend(section("table", bytes(table)))
    patch_next(out, table_desc, len(out))

    done_desc = len(out)
    out.extend(section("done", b""))
    patch_next(out, done_desc, done_desc)
    path.write_bytes(out)
    return path


def build_split(work: Path, count: int) -> Path:
    # X26 specifies empty segments. The typed "empty image" rejection must be
    # returned without opening/holding all 100,000 descriptors.
    for i in range(count):
        (work / f"disk.{i:06d}").touch()
    return work / "disk.000000"


def run_case(args: argparse.Namespace, image: Path, image_format: str) -> tuple[int, float, float, Path]:
    out = image.parent / f"out-{image.stem}"
    report = image.parent / f"report-{image.stem}.json"
    log = image.parent / f"log-{image.stem}.txt"
    log_fd = os.open(log, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
    cmd = [
        str(args.carvex.resolve()), "scan", str(image), "-o", str(out),
        "--mode", "raw", "--no-fs", "--quiet", "--input-format", image_format,
        "--json-report", str(report), "--max-candidates", "1000",
        "--time-budget", str(args.time_budget),
    ]
    started = time.monotonic()

    def child_setup() -> None:
        limit = args.rlimit_mib * 1024 * 1024
        resource.setrlimit(resource.RLIMIT_AS, (limit, limit))
        os.dup2(log_fd, 1)
        os.dup2(log_fd, 2)

    pid = os.fork()
    if pid == 0:
        try:
            child_setup()
            os.execv(cmd[0], cmd)
        except BaseException:
            os._exit(127)
    _, status, usage = os.wait4(pid, 0)
    os.close(log_fd)
    elapsed = time.monotonic() - started
    if os.WIFSIGNALED(status):
        code = -os.WTERMSIG(status)
    elif os.WIFEXITED(status):
        code = os.WEXITSTATUS(status)
    else:
        code = 999
    return code, elapsed, usage.ru_maxrss / 1024.0, log


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--carvex", type=Path, required=True)
    parser.add_argument("--time-budget", type=int, default=5)
    parser.add_argument("--rlimit-mib", type=int, default=1024)
    parser.add_argument("--max-rss-mib", type=int, default=128)
    parser.add_argument("--max-seconds", type=float, default=7.0)
    parser.add_argument("--split-count", type=int, default=100_000)
    args = parser.parse_args()
    if not args.carvex.is_file():
        parser.error(f"binary not found: {args.carvex}")

    ok = True
    with tempfile.TemporaryDirectory(prefix="carvex-x26-") as td:
        work = Path(td)
        fixtures = [
            ("vhd", build_vhd(work), "vhd"),
            ("e01", build_e01(work), "e01"),
            ("split", build_split(work, args.split_count), "split"),
        ]
        for name, image, image_format in fixtures:
            code, elapsed, rss_mib, log = run_case(args, image, image_format)
            report = image.parent / f"report-{image.stem}.json"
            passed = (
                code in (0, 1, 3)
                and rss_mib <= args.max_rss_mib
                and elapsed <= args.max_seconds
                and (report.exists() or code == 1)
            )
            ok &= passed
            detail = " ".join(log.read_text(errors="replace").split())[:160]
            print(
                f"X26 {name.upper()} {'PASS' if passed else 'FAIL'} exit={code} "
                f"elapsed={elapsed:.2f}s peak_rss_mib={rss_mib:.1f} "
                f"report={report.exists()} detail={detail!r}"
            )
            if not passed:
                sys.stderr.write(log.read_text(errors="replace")[-800:])
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
