#!/usr/bin/env python3
"""Build MBR-partitioned FAT32/NTFS images containing the benchmark corpus,
copy the files, delete the selected subset, and write a ground-truth manifest.

Must run as root (loop mount). Only the generated image is modified.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone

ROOT = os.path.dirname(os.path.abspath(__file__))
DEFAULT_CORPUS = os.path.join(ROOT, "corpus")
DEFAULT_IMAGES = os.path.join(ROOT, "images")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def run(cmd, **kw):
    print("+", " ".join(str(c) for c in cmd))
    kw.setdefault("check", True)
    return subprocess.run(cmd, **kw)


def check_tools():
    needed = ["parted", "losetup", "mkfs.vfat", "mkntfs", "mount", "umount", "sync"]
    missing = [t for t in needed if not shutil.which(t)]
    if missing:
        sys.exit(f"missing tools: {', '.join(missing)}")
    if os.geteuid() != 0:
        sys.exit("this script needs root for loop mounts; run with sudo")


def make_image(path, size_mb, fs):
    if os.path.exists(path):
        os.remove(path)
    with open(path, "wb") as f:
        f.truncate(size_mb * 1024 * 1024)
    end_mb = size_mb - 1
    run(["parted", "-s", path, "mklabel", "msdos"])
    fstype = "fat32" if fs == "fat32" else "ntfs"
    run(["parted", "-s", path, "mkpart", "primary", fstype, "1MiB", f"{end_mb}MiB"])
    loop = run(["losetup", "-Pf", "--show", path], capture_output=True, text=True, check=True)
    loop_dev = loop.stdout.strip()
    if not loop_dev:
        sys.exit("losetup did not return a device")
    part = loop_dev + "p1"
    try:
        if fs == "fat32":
            run(["mkfs.vfat", "-F32", "-n", "CARVEXFAT", part], stdout=subprocess.DEVNULL)
        else:
            run(["mkntfs", "-F", "-q", "-L", "CARVEXNTFS", part],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return loop_dev, part
    except Exception:
        subprocess.run(["losetup", "-d", loop_dev], check=False)
        raise


def destination(corpus_dir, rel):
    # Keep the on-disk layout realistic and deterministic.
    if rel.startswith("pdf/"):
        return os.path.join("docs", os.path.basename(rel))
    if rel.startswith("jpeg/"):
        return os.path.join("photos", os.path.basename(rel))
    return os.path.join("photos", os.path.basename(rel))


def build_one(scenario, fs, corpus_dir, manifest, images_dir, size_mb, carvex, force):
    stem = f"{scenario}_{fs}"
    image = os.path.join(images_dir, stem + ".img")
    meta_path = os.path.join(images_dir, stem + ".json")
    if os.path.exists(meta_path) and not force:
        print(f"skip existing {meta_path} (use --force to rebuild)")
        return meta_path
    if os.path.exists(image):
        os.remove(image)
    print(f"\n=== {stem} ===")
    loop_dev = None
    mount_dir = tempfile.mkdtemp(prefix="carvex_mnt_", dir=images_dir)
    try:
        loop_dev, part = make_image(image, size_mb, fs)
        if fs == "fat32":
            run(["mount", part, mount_dir])
        else:
            run(["mount", "-t", "ntfs-3g", part, mount_dir])

        files = manifest["files"]
        deleted = []
        kept = []
        for i, entry in enumerate(files):
            rel = entry["path"]
            is_deleted = scenario == "all-deleted" or (scenario == "mixed" and i % 3 != 0)
            (deleted if is_deleted else kept).append(entry)
            src = os.path.join(corpus_dir, rel)
            dst_rel = destination(corpus_dir, rel)
            dst = os.path.join(mount_dir, dst_rel)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.copy2(src, dst)
        run(["sync"])

        for entry in deleted:
            dst = os.path.join(mount_dir, destination(corpus_dir, entry["path"]))
            os.remove(dst)
        run(["sync"])

        # Show the resulting directory tree for the log.
        for root, _dirs, names in os.walk(mount_dir):
            for n in names:
                p = os.path.join(root, n)
                print("  on-image:", os.path.relpath(p, mount_dir))
        run(["umount", mount_dir])
        subprocess.run(["losetup", "-d", loop_dev], check=True)
        loop_dev = None

        meta = {
            "image": os.path.basename(image),
            "image_path": os.path.abspath(image),
            "image_size": os.path.getsize(image),
            "image_sha256": sha256(image),
            "scenario": scenario,
            "filesystem": fs,
            "partition_offset": 1024 * 1024,
            "deleted": deleted,
            "kept": kept,
            "deleted_count": len(deleted),
            "kept_count": len(kept),
            "deleted_bytes": sum(f["size"] for f in deleted),
            "kept_bytes": sum(f["size"] for f in kept),
            "corpus_manifest": os.path.abspath(os.path.join(corpus_dir, "manifest.json")),
            "created_at": datetime.now(timezone.utc).isoformat(),
        }
        if carvex and os.path.exists(carvex):
            try:
                info = subprocess.run(
                    [carvex, "info", image, "--json"],
                    capture_output=True, text=True, check=True,
                )
                meta["fs_info"] = json.loads(info.stdout)["filesystem"]
            except Exception as exc:
                print(f"warning: carvex info failed: {exc}")
        with open(meta_path, "w") as f:
            json.dump(meta, f, indent=2)
        print(f"wrote {meta_path}: deleted={len(deleted)} kept={len(kept)}")
        return meta_path
    finally:
        # Best-effort cleanup, including on failure.
        subprocess.run(["umount", mount_dir], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if loop_dev:
            subprocess.run(["losetup", "-d", loop_dev], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        shutil.rmtree(mount_dir, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", default=DEFAULT_CORPUS)
    ap.add_argument("--out", default=DEFAULT_IMAGES)
    ap.add_argument("--size-mb", type=int, default=64)
    ap.add_argument("--scenario", action="append", choices=["all-deleted", "mixed"])
    ap.add_argument("--fs", action="append", choices=["fat32", "ntfs"])
    ap.add_argument("--carvex", default="")
    ap.add_argument("--force", action="store_true")
    args = ap.parse_args()
    check_tools()
    corpus_dir = os.path.abspath(args.corpus)
    images_dir = os.path.abspath(args.out)
    os.makedirs(images_dir, exist_ok=True)
    manifest_path = os.path.join(corpus_dir, "manifest.json")
    if not os.path.exists(manifest_path):
        sys.exit(f"missing {manifest_path}; run make_corpus.py first")
    with open(manifest_path) as f:
        manifest = json.load(f)
    scenarios = args.scenario or ["all-deleted", "mixed"]
    filesystems = args.fs or ["fat32", "ntfs"]
    carvex = args.carvex
    if not carvex:
        for cand in ("target/release/carvex", "target/debug/carvex"):
            p = os.path.join(os.path.dirname(ROOT), cand)
            if os.path.exists(p):
                carvex = p
                break
    for scenario in scenarios:
        for fs in filesystems:
            build_one(scenario, fs, corpus_dir, manifest, images_dir,
                      args.size_mb, carvex, args.force)
    print("\nimages:")
    for name in sorted(os.listdir(images_dir)):
        if name.endswith(".img"):
            p = os.path.join(images_dir, name)
            print(f"  {name:38} {os.path.getsize(p):>12} bytes")


if __name__ == "__main__":
    main()
