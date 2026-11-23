#!/usr/bin/env python3
"""X08b: 512 TiB declared ZERO extent must not size the scan plan.

The child is started under RLIMIT_AS and its peak RSS is read from wait4.
The scenario passes only when the process never dies from a signal, stays
within the RSS cap, and (with a time budget) leaves a report on disk.
"""
from __future__ import annotations

import argparse
import json
import os
import resource
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SECTOR_BYTES = 512
FLAT_SECTORS = 131_072  # 64 MiB sparse backing file
ZERO_SECTORS = 1 << 40  # 512 TiB logical ZERO extent


def build_descriptor(work: Path, sectors: int) -> Path:
    base = work / "base.img"
    with base.open("wb") as handle:
        handle.truncate(FLAT_SECTORS * SECTOR_BYTES)
    desc = work / "x08b.vmdk"
    desc.write_text(
        "# Disk DescriptorFile\nversion=1\nCID=fffffffe\nparentCID=ffffffff\n"
        'createType="monolithicFlat"\n\n# Extent description\n'
        f'RW {FLAT_SECTORS} FLAT "base.img" 0\nRW {sectors} ZERO\n\n'
        "# The Disk Data Base\n#DDB\n"
    )
    return desc


def run(carvex: Path, desc: Path, out: Path, report: Path, args: argparse.Namespace) -> tuple[int, float, float, int]:
    log_path = out.parent / "child.log"
    log_fd = os.open(log_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
    cmd = [
        str(carvex), "scan", str(desc), "-o", str(out),
        "--mode", "raw", "--no-fs", "--quiet",
        "--json-report", str(report),
        "--max-candidates", "1000",
        "--time-budget", str(args.time_budget),
    ]
    started = time.monotonic()

    def limit_child() -> None:
        limit = args.rlimit_mib * 1024 * 1024
        resource.setrlimit(resource.RLIMIT_AS, (limit, limit))
        os.dup2(log_fd, 1)
        os.dup2(log_fd, 2)

    pid = os.fork()
    if pid == 0:
        try:
            limit_child()
            os.execv(str(carvex), cmd)
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
    if code not in (0, 1, 3):
        sys.stderr.write(log_path.read_text(errors="replace")[-800:])
    return code, elapsed, usage.ru_maxrss / 1024.0, int(log_path.stat().st_size)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--carvex", type=Path, required=True)
    parser.add_argument("--sectors", type=int, default=ZERO_SECTORS)
    parser.add_argument("--time-budget", type=int, default=5)
    parser.add_argument("--rlimit-mib", type=int, default=1024)
    parser.add_argument("--max-rss-mib", type=int, default=128)
    parser.add_argument("--max-seconds", type=float, default=7.0)
    args = parser.parse_args()
    carvex = args.carvex.resolve()
    if not carvex.is_file():
        parser.error(f"binary not found: {carvex}")

    with tempfile.TemporaryDirectory(prefix="carvex-x08b-") as td:
        work = Path(td)
        desc = build_descriptor(work, args.sectors)
        out = work / "out"
        report = work / "report.json"
        code, elapsed, rss_mib, log_bytes = run(carvex, desc, out, report, args)
        valid_report = report.is_file()
        if valid_report:
            try:
                json.loads(report.read_text())
            except (OSError, ValueError):
                valid_report = False
        ok = (
            code in (0, 1, 3)
            and rss_mib <= args.max_rss_mib
            and elapsed <= args.max_seconds
            and valid_report
        )
        print(
            f"X08B {'PASS' if ok else 'FAIL'} exit={code} elapsed={elapsed:.2f}s "
            f"peak_rss_mib={rss_mib:.1f} report={valid_report} "
            f"sectors={args.sectors} rlimit_mib={args.rlimit_mib} child_log_bytes={log_bytes}"
        )
        return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
