#!/usr/bin/env python3
"""H-29: measure a small D1-style fragmentation development profile.

The profile is deliberately separate from the frozen lockbox. It plants a
handful of real corpus files as two cluster-aligned fragments with a filler
gap, then scans each image with B-06 enabled. It reports exact-recall, the
splice rate of accepted stitches and the false-positive rate on bait layouts
where the head and tail come from different files. Wilson 95% intervals are
computed for all three proportions.

Usage:
    python3 scripts/fragment_profile.py \
        --carvex target/release/carvex \
        --corpus bench/corpus \
        --work /tmp/opencode/fragment-profile \
        --out bench/results/d1_fragment_profile.json
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import tempfile
from pathlib import Path

from review_repro import (
    _fat32_cluster_offset,
    _fat32_free_volume,
    _plant,
    run_carvex,
)

CLUSTER = 4096
BGC_ARGS = [
    "--min-size",
    "1",
    "--fragment-recovery",
    "--max-gap",
    "16",
    "--max-frag-candidates",
    "4096",
    "--frag-time-limit",
    "5000",
]

PAYLOADS = [
    "jpeg/jpeg_01_baseline_640x480.jpg",
    "jpeg/jpeg_02_large_1280x960.jpg",
    "png/png_02_rgb_800x600.png",
    "png/png_06_large_1200x800.png",
    "pdf/pdf_04_incremental.pdf",
    "pdf/pdf_05_large_scan.pdf",
]

BAIT_HEAD = "jpeg/jpeg_01_baseline_640x480.jpg"
BAIT_TAIL = "jpeg/jpeg_02_large_1280x960.jpg"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def wilson(successes: int, trials: int, z: float = 1.959963984540054) -> list[float]:
    """Two-sided Wilson 95% interval for a binomial proportion."""
    if trials == 0:
        return [0.0, 1.0]
    p = successes / trials
    denom = 1.0 + z * z / trials
    centre = (p + z * z / (2 * trials)) / denom
    half = (z / denom) * math.sqrt(
        p * (1 - p) / trials + z * z / (4 * trials * trials)
    )
    lower = max(0.0, centre - half)
    upper = min(1.0, centre + half)
    # Clamp floating-point residue at the binomial boundaries.
    if successes == 0:
        lower = 0.0
    if successes == trials:
        upper = 1.0
    return [lower, upper]


def gap_bytes(kind: str, length: int, seed: int = 0) -> bytes:
    if kind == "zero":
        return bytes(length)
    if kind == "letters":
        return bytes((0x41 + ((i * 37 + seed) % 26)) for i in range(length))
    if kind == "pattern":
        pattern = bytes(((i * 17 + seed) & 0xFF) for i in range(13))
        return (pattern * (length // len(pattern) + 1))[:length]
    if kind == "sawtooth":
        return bytes(((i + seed) & 0xFF) for i in range(length))
    raise ValueError(f"unknown gap pattern: {kind}")


def planted_image(payload: bytes, kind: str, seed: int) -> tuple[bytearray, int, int, int]:
    """Return the image, head offset, tail offset and expected file length."""
    image, meta = _fat32_free_volume(total_sectors=32768, spc=8)
    cut = (len(payload) // 2 // CLUSTER) * CLUSTER
    head_clusters = (cut + CLUSTER - 1) // CLUSTER
    head_offset = _fat32_cluster_offset(meta, 200)
    tail_offset = _fat32_cluster_offset(meta, 200 + head_clusters + 3)
    gap = gap_bytes(kind, 3 * CLUSTER, seed)
    _plant(image, meta, 200, payload[:cut])
    _plant(image, meta, 200 + head_clusters, gap)
    _plant(image, meta, 200 + head_clusters + 3, payload[cut:])
    return image, head_offset, tail_offset, len(payload)


def scan_case(carvex: Path, work: Path, name: str, image: bytearray) -> dict:
    sub = work / name
    sub.mkdir(parents=True, exist_ok=True)
    image_path = sub / f"{name}.img"
    image_path.write_bytes(image)
    result = run_carvex(carvex, image_path, sub, BGC_ARGS)
    image_path.unlink(missing_ok=True)
    report = result.report or {}
    return {
        "exit": result.proc.returncode,
        "warnings": sorted({w.get("code", "") for w in report.get("warnings", [])}),
        "stitches": [
            {
                "sha256": f.get("sha256"),
                "size": f.get("size"),
                "source": f.get("source"),
                "validity": f.get("validity"),
            }
            for f in report.get("files", [])
            if f.get("source") == "bgc"
        ],
        "carves": [
            {
                "sha256": f.get("sha256"),
                "size": f.get("size"),
                "source": f.get("source"),
            }
            for f in report.get("files", [])
            if f.get("source") != "bgc"
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--carvex", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, default=Path("bench/corpus"))
    parser.add_argument("--work", type=Path, default=Path("/tmp/opencode/fragment-profile"))
    parser.add_argument("--out", type=Path, default=Path("bench/results/d1_fragment_profile.json"))
    args = parser.parse_args()
    carvex = args.carvex.resolve()
    corpus = args.corpus.resolve()
    work = args.work.resolve()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)

    patterns = ["zero", "letters", "pattern", "sawtooth"]
    fragmented = []
    for index, rel in enumerate(PAYLOADS):
        payload = (corpus / rel).read_bytes()
        kind = patterns[index % len(patterns)]
        seed = index // len(patterns)
        image, head, tail, length = planted_image(payload, kind, seed)
        case = scan_case(carvex, work, f"fragment_{index}", image)
        expected = sha256(payload)
        recalled = any(s["sha256"] == expected for s in case["stitches"])
        bad = [s for s in case["stitches"] if s["sha256"] != expected]
        fragmented.append(
            {
                "name": rel,
                "gap_kind": kind,
                "seed": seed,
                "source_sha256": expected,
                "file_size": length,
                "head_offset": head,
                "tail_offset": tail,
                "recalled": recalled,
                "unexpected_stitches": bad,
                **case,
            }
        )

    head_payload = (corpus / BAIT_HEAD).read_bytes()
    tail_payload = (corpus / BAIT_TAIL).read_bytes()
    bait = []
    for index in range(24):
        kind = patterns[index % len(patterns)]
        seed = index // len(patterns)
        image, meta = _fat32_free_volume(total_sectors=32768, spc=8)
        cut_head = (len(head_payload) // 2 // CLUSTER) * CLUSTER
        cut_tail = (len(tail_payload) // 2 // CLUSTER) * CLUSTER
        head_clusters = (cut_head + CLUSTER - 1) // CLUSTER
        _plant(image, meta, 200, head_payload[:cut_head])
        _plant(image, meta, 200 + head_clusters, gap_bytes(kind, 2 * CLUSTER, seed))
        _plant(image, meta, 200 + head_clusters + 2, tail_payload[cut_tail:])
        case = scan_case(carvex, work, f"bait_{index}", image)
        bait.append({"gap_kind": kind, "seed": seed, **case})

    recall_hits = sum(case["recalled"] for case in fragmented)
    stitched = sum(len(case["stitches"]) for case in fragmented)
    splices = sum(len(case["unexpected_stitches"]) for case in fragmented)
    junk_hits = sum(1 for case in bait if case["stitches"])

    result = {
        "profile": "D1 synthetic fragmentation development profile (not the frozen lockbox)",
        "binary_sha256": sha256(carvex.read_bytes()),
        "command_options": BGC_ARGS,
        "fragmented_cases": fragmented,
        "bait_cases": bait,
        "summary": {
            "fragmented_trials": len(fragmented),
            "recall": recall_hits / len(fragmented),
            "recall_wilson95": wilson(recall_hits, len(fragmented)),
            "stitched_outputs": stitched,
            "spliced_outputs": splices,
            "splice_rate": splices / stitched if stitched else 0.0,
            "splice_rate_wilson95": wilson(splices, stitched),
            "bait_trials": len(bait),
            "junk_false_positives": junk_hits,
            "junk_fp_rate": junk_hits / len(bait),
            "junk_fp_wilson95": wilson(junk_hits, len(bait)),
        },
    }
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    summary = result["summary"]
    print(
        "D1 profile: recall={recall:.3f} {recall_wilson95} "
        "splice_rate={splice_rate:.3f} {splice_rate_wilson95} "
        "junk_fp={junk_fp_rate:.3f} {junk_fp_wilson95}".format(**summary)
    )


if __name__ == "__main__":
    main()
