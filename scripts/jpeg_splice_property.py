#!/usr/bin/env python3
"""H-18 step 0: measure JPEG splice acceptance on an unmodified build.

The protocol follows harness.md §6.2 H-18(0): independent image pairs, cuts
aligned to 512 and 4096 bytes inside 10-90% of the shorter file, and a Wilson
95% interval for the share of splices that receive the top decode label.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageEnhance


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def wilson(k: int, n: int, z: float = 1.96) -> tuple[float, float]:
    if n == 0:
        return (0.0, 0.0)
    p = k / n
    den = 1 + z * z / n
    centre = (p + z * z / (2 * n)) / den
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / den
    return (centre - half, centre + half)


def build_pool(work: Path, corpus: Path) -> list[Path]:
    """Create >=50 distinct images for each of three content classes.

    Every image is saved as both baseline and progressive JPEG so both scan
    variants contribute to the measurement.
    """
    rng = random.Random(20261121)
    pool: list[Path] = []
    sources = sorted((corpus / "jpeg").glob("*.jpg"))
    if len(sources) < 2:
        raise SystemExit("corpus JPEGs missing")
    seeds = [Image.open(p).convert("RGB") for p in sources[:8]]
    sizes = [(640, 480), (800, 600), (1024, 768)]
    for kind in ("noise", "gradient_noise", "photo_like"):
        for i in range(50):
            w, h = sizes[i % len(sizes)]
            if kind == "noise":
                base = Image.effect_noise((w, h), 48).convert("RGB")
            elif kind == "gradient_noise":
                gradient = Image.new("RGB", (w, h))
                px = gradient.load()
                for y in range(h):
                    colour = int(255 * y / max(1, h - 1))
                    for x in range(w):
                        px[x, y] = (colour, (x * 255) // max(1, w - 1), 255 - colour)
                noise = Image.effect_noise((w, h), 30).convert("RGB")
                base = Image.blend(gradient, noise, 0.35)
            else:
                src = seeds[i % len(seeds)]
                x0 = rng.randrange(0, max(1, src.width - w + 1))
                y0 = rng.randrange(0, max(1, src.height - h + 1))
                base = src.crop((x0, y0, x0 + w, y0 + h)).resize((w, h))
                base = Image.blend(
                    base, Image.effect_noise((w, h), 18).convert("RGB"), 0.12
                )
                base = ImageEnhance.Contrast(base).enhance(0.75 + rng.random() * 0.5)
            for mode in ("baseline", "progressive"):
                path = work / "pool" / f"{kind}_{i:02d}_{mode}.jpg"
                path.parent.mkdir(parents=True, exist_ok=True)
                base.save(path, "JPEG", quality=88, progressive=(mode == "progressive"))
                pool.append(path)
    return pool


def scan(carvex: Path, work: Path, payload: bytes, index: int) -> list[dict]:
    image = work / f"case_{index}.img"
    image.write_bytes(b"\x00" * 4096 + payload + b"\x00" * 8192)
    out = work / f"out_{index}"
    out.mkdir(exist_ok=True)
    report = work / f"report_{index}.json"
    proc = subprocess.run(
        [
            str(carvex), "scan", str(image), "-o", str(out), "--mode", "raw",
            "--no-fs", "--min-size", "1", "--validate", "decode", "--quiet",
            "--json-report", str(report),
        ],
        capture_output=True,
        text=True,
    )
    if proc.returncode not in (0, 3):
        raise SystemExit(f"scan {index} exited {proc.returncode}: {proc.stderr[:200]}")
    return json.loads(report.read_text()).get("files", [])


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--carvex", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, default=Path("bench/corpus"))
    parser.add_argument("--pairs", type=int, default=1000)
    parser.add_argument("--workdir", type=Path)
    args = parser.parse_args()

    rng = random.Random(70612)  # fixed for reproducibility
    temp = tempfile.TemporaryDirectory(prefix="carvex-splice-") if args.workdir is None else None
    work = Path(temp.name) if temp else args.workdir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    pool = build_pool(work, args.corpus)

    accepted = 0
    by_class: dict[str, list[int]] = {}
    for index in range(args.pairs):
        a_path, b_path = rng.sample(pool, 2)
        a, b = a_path.read_bytes(), b_path.read_bytes()
        originals = {sha256(a), sha256(b)}
        align = 512 if index % 2 == 0 else 4096
        shorter = min(len(a), len(b))
        lo = int(shorter * 0.10 / align) * align
        hi = int(shorter * 0.90 / align) * align
        if hi <= lo:
            continue
        cut = rng.randrange(lo, hi + 1, align)
        files = scan(args.carvex, work, b[:cut] + a[cut:], index)
        top_label = any(
            f.get("validation") == "decoded" and f.get("sha256") not in originals
            for f in files
        )
        if top_label:
            accepted += 1
            (work / f"accepted_{index}.img").write_bytes(b[:cut] + a[cut:])
            print(f"accepted case {index}: {a_path.name} <- {b_path.name}, cut={cut}")
            key = a_path.name.split("_")[0]
            by_class.setdefault(key, [0, 0])
            by_class[key][0] += 1
        key = a_path.name.split("_")[0]
        by_class.setdefault(key, [0, 0])
        by_class[key][1] += 1

    lo, hi = wilson(accepted, args.pairs)
    print(f"pairs={args.pairs} accepted={accepted} rate={accepted / args.pairs:.4f}")
    print(f"wilson95=[{lo:.4f},{hi:.4f}]")
    for key, (k, n) in sorted(by_class.items()):
        clo, chi = wilson(k, n)
        print(f"{key}: {k}/{n} rate={k / n:.4f} wilson95=[{clo:.4f},{chi:.4f}]")
    if temp:
        temp.cleanup()


if __name__ == "__main__":
    main()
