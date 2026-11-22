#!/usr/bin/env python3
"""G7 Golden gate: compare carved output SHA-256 sets against ground truth.

Compares output-hash sets (not image bytes), so fixtures built without root are
acceptable as long as the ground-truth SHA-256s match. By default uses the
benchmark D0 images under bench/images with --partition largest --no-gaps to
preserve the historical search space.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

JPEG_MAGIC = b"\xff\xd8\xff"
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"
PDF_MAGIC = b"%PDF-"
EXT_TO_TYPE = {".jpg": "jpeg", ".jpeg": "jpeg", ".png": "png", ".pdf": "pdf"}


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def detect_type(data: bytes, suffix: str) -> tuple[str | None, bool]:
    if data.startswith(JPEG_MAGIC):
        return "jpeg", True
    if data.startswith(PNG_MAGIC):
        return "png", True
    if data.startswith(PDF_MAGIC):
        return "pdf", True
    ext_type = EXT_TO_TYPE.get(suffix.lower())
    return (ext_type, False) if ext_type else (None, False)


def collect_outputs(out_dir: Path) -> list[dict]:
    outputs = []
    for p in sorted(out_dir.rglob("*")):
        if not p.is_file():
            continue
        if p.name in {"report.json", "report.csv", "SHA256SUMS"}:
            continue
        data = p.read_bytes()
        ftype, magic_ok = detect_type(data, p.suffix)
        outputs.append(
            {
                "path": str(p),
                "type": ftype,
                "magic_ok": magic_ok,
                "sha256": hashlib.sha256(data).hexdigest(),
            }
        )
    return outputs


def load_cases(images_dir: Path) -> list[dict]:
    cases = []
    for meta_path in sorted(images_dir.glob("*.json")):
        meta = json.loads(meta_path.read_text())
        if "deleted" not in meta or "image" not in meta:
            continue
        image = images_dir / meta["image"]
        if not image.exists():
            image = Path(meta.get("image_path", image))
        if not image.exists():
            continue
        cases.append({"meta_path": meta_path, "meta": meta, "image": image})
    return cases


def run_case(carvex: Path, case: dict, mode: str, workdir: Path) -> tuple[bool, str]:
    meta = case["meta"]
    image = case["image"]
    out = workdir / f"{meta_path_stem(case)}-{mode}"
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    cmd = [str(carvex), "scan", str(image), "-o", str(out), "--quiet", "--json-report", str(out / "report.json")]
    if mode == "unallocated":
        cmd += ["--mode", "unallocated", "--partition", "largest", "--no-gaps"]
    elif mode == "raw":
        cmd += ["--mode", "raw", "--partition", "largest", "--no-gaps"]
    else:
        raise ValueError(mode)

    proc = subprocess.run(cmd, capture_output=True, text=True)
    # A-07 restore path may exit 3 with GAP_NOT_SCANNED; accept 0/3 as completed.
    if proc.returncode not in (0, 3):
        return False, f"exit {proc.returncode}: {proc.stderr.strip()[:200]}"

    outputs = [o for o in collect_outputs(out) if o["magic_ok"] and o["type"] in {"jpeg", "png", "pdf"}]
    out_hashes = {o["sha256"] for o in outputs}
    deleted_hashes = {e["sha256"] for e in meta.get("deleted", []) if e.get("sha256")}
    kept_hashes = {e["sha256"] for e in meta.get("kept", []) if e.get("sha256")}

    if mode == "unallocated":
        tp = len(out_hashes & deleted_hashes)
        fp = len(out_hashes - deleted_hashes)
        fn = len(deleted_hashes - out_hashes)
        ok = tp == len(deleted_hashes) and fp == 0 and fn == 0
        return ok, f"TP={tp}/{len(deleted_hashes)} FP={fp} FN={fn}"
    if mode == "raw":
        tp = len(out_hashes & deleted_hashes)
        # Raw scans the whole image; kept files are expected FPs in D0 accounting.
        kept_hits = len(out_hashes & kept_hashes)
        unknown_fp = len(out_hashes - deleted_hashes - kept_hashes)
        fn = len(deleted_hashes - out_hashes)
        ok = tp == len(deleted_hashes) and fn == 0 and unknown_fp == 0 and kept_hits == len(kept_hashes)
        return ok, f"TP={tp}/{len(deleted_hashes)} kept={kept_hits}/{len(kept_hashes)} unknown_fp={unknown_fp} FN={fn}"
    raise ValueError(mode)


def meta_path_stem(case: dict) -> str:
    return case["meta_path"].stem


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--carvex", type=Path, default=Path("target/release/carvex"))
    ap.add_argument("--images-dir", type=Path, default=Path("bench/images"))
    ap.add_argument("--workdir", type=Path, default=None)
    ap.add_argument("--modes", default="unallocated,raw")
    args = ap.parse_args()

    carvex = args.carvex.resolve()
    if not carvex.is_file():
        print(f"SKIPPED golden (carvex binary missing: {carvex})")
        return 0
    images_dir = args.images_dir.resolve()
    cases = load_cases(images_dir)
    if not cases:
        print(f"SKIPPED golden (no image manifests in {images_dir})")
        return 0

    modes = [m.strip() for m in args.modes.split(",") if m.strip()]
    temp = None
    if args.workdir:
        workdir = args.workdir.resolve()
        workdir.mkdir(parents=True, exist_ok=True)
    else:
        temp = tempfile.TemporaryDirectory(prefix="carvex-golden-")
        workdir = Path(temp.name)

    failures = 0
    for mode in modes:
        for case in cases:
            ok, msg = run_case(carvex, case, mode, workdir)
            name = meta_path_stem(case)
            status = "PASS" if ok else "FAIL"
            print(f"{status} golden {mode:<11} {name:<24} {msg}")
            if not ok:
                failures += 1

    if temp is not None:
        temp.cleanup()

    if failures:
        print(f"GOLDEN FAIL ({failures} failure(s))")
        return 1
    print("GOLDEN GREEN")
    return 0


if __name__ == "__main__":
    sys.exit(main())
