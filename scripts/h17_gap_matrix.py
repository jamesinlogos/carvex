#!/usr/bin/env python3
"""H-17 residual: measure S-B06 recall and splice rate by gap class.

The fixture follows the supplied harness Appendix A.2 S-B06 row: a 64 MiB
FAT32 volume, three real corpus files split at cluster-aligned midpoints, and a
three-cluster gap filled with zeros, seeded random bytes or another file's
bytes. Only the opt-in B-06 stage is enabled; all limits are left at defaults.

Usage:
    python3 scripts/h17_gap_matrix.py \
        --carvex target/release/carvex \
        --corpus bench/corpus \
        --work target/h17-gap-matrix \
        --out bench/results/h17_gap_matrix.json
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import time
from pathlib import Path

from review_repro import (
    _fat32_cluster_offset,
    _fat32_free_volume,
    _plant,
    run_carvex,
)

from fragment_profile import wilson

CLUSTER = 4096
PAYLOADS = [
    "jpeg/jpeg_01_baseline_640x480.jpg",
    "png/png_02_rgb_800x600.png",
    "pdf/pdf_05_large_scan.pdf",
]
OTHER_FILE = "jpeg/jpeg_04_grayscale_512x512.jpg"
GAP_KINDS = ("zeros", "random", "otherfile")
BGC_ARGS = ["--min-size", "1", "--fragment-recovery", "--validate", "structure"]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def gap_bytes(kind: str, length: int, other: bytes) -> bytes:
    if kind == "zeros":
        return bytes(length)
    if kind == "random":
        return random.Random(0xC0FFEE).randbytes(length)
    return (other[:CLUSTER] * ((length // CLUSTER) + 1))[:length]


def build_image(corpus: Path, kind: str, other: bytes) -> tuple[bytearray, dict[str, str]]:
    image, meta = _fat32_free_volume(total_sectors=131072, spc=8)
    cluster = 200
    expected: dict[str, str] = {}
    for rel in PAYLOADS:
        payload = (corpus / rel).read_bytes()
        cut = (len(payload) // 2 // CLUSTER) * CLUSTER
        head_clusters = (cut + CLUSTER - 1) // CLUSTER
        tail_clusters = (len(payload) - cut + CLUSTER - 1) // CLUSTER
        _plant(image, meta, cluster, payload[:cut])
        _plant(image, meta, cluster + head_clusters, gap_bytes(kind, 3 * CLUSTER, other))
        _plant(image, meta, cluster + head_clusters + 3, payload[cut:])
        expected[rel] = sha256(payload)
        cluster += head_clusters + 3 + tail_clusters + 60
    return image, expected


def scan(carvex: Path, work: Path, kind: str, image: bytearray) -> dict:
    case_dir = work / kind
    case_dir.mkdir(parents=True, exist_ok=True)
    image_path = case_dir / f"s-b06-{kind}.img"
    image_path.write_bytes(image)
    started = time.perf_counter()
    result = run_carvex(carvex, image_path, case_dir, BGC_ARGS)
    wall = time.perf_counter() - started
    report = result.report or {}
    return {
        "exit": result.proc.returncode,
        "wall_s": round(wall, 4),
        "warning_codes": sorted({w.get("code", "") for w in report.get("warnings", [])}),
        "files": [
            {
                "size": f.get("size"),
                "sha256": f.get("sha256"),
                "source": f.get("source"),
                "status": f.get("status"),
                "validity": f.get("validity"),
            }
            for f in report.get("files", [])
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--carvex", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, default=Path("bench/corpus"))
    parser.add_argument("--work", type=Path, default=Path("target/h17-gap-matrix"))
    parser.add_argument("--out", type=Path, default=Path("bench/results/h17_gap_matrix.json"))
    args = parser.parse_args()
    carvex = args.carvex.resolve()
    corpus = args.corpus.resolve()
    work = args.work.resolve()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)

    other = (corpus / OTHER_FILE).read_bytes()
    cases = []
    for kind in GAP_KINDS:
        image, expected = build_image(corpus, kind, other)
        result = scan(carvex, work, kind, image)
        hashes = {f["sha256"] for f in result["files"]}
        stitches = [f for f in result["files"] if f["source"] == "bgc"]
        exact = sum(1 for digest in expected.values() if digest in hashes)
        splices = [f for f in stitches if f["sha256"] not in expected.values()]
        cases.append(
            {
                "gap_kind": kind,
                "exact_recall": exact,
                "trials": len(PAYLOADS),
                "exact_recall_wilson95": wilson(exact, len(PAYLOADS)),
                "spliced_stitches": splices,
                "expected_sha256": expected,
                **result,
            }
        )

    payload = {
        "fixture": "harness-ab725a6 Appendix A.2 S-B06 (64 MiB FAT32, 4 KiB clusters)",
        "binary_sha256": sha256(carvex.read_bytes()),
        "command_options": BGC_ARGS,
        "gap_classes": {
            "zeros": "all-zero filler",
            "random": "seeded random bytes",
            "otherfile": "repeated first cluster of jpeg_04_grayscale_512x512.jpg",
        },
        "cases": cases,
    }
    args.out.write_text(json.dumps(payload, indent=2) + "\n")
    print(json.dumps({
        c["gap_kind"]: {
            "exact_recall": c["exact_recall"],
            "trials": c["trials"],
            "spliced_stitches": len(c["spliced_stitches"]),
            "wall_s": c["wall_s"],
        }
        for c in cases
    }, indent=2))
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
