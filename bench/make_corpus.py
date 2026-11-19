#!/usr/bin/env python3
"""Generate a deterministic JPEG/PNG/PDF corpus for CarveX benchmarks.

Requirements: Python 3, Pillow, numpy. exiftool/ImageMagick are optional and
used for the EXIF-thumbnail and interlaced-PNG edge cases.
"""
import argparse
import hashlib
import json
import os
import random
import shutil
import subprocess
import sys
from datetime import datetime, timezone

try:
    import numpy as np
    from PIL import Image, ImageDraw
except ImportError as exc:
    sys.exit(f"missing dependency: {exc} (pip install pillow numpy)")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def image(w, h, mode="RGB", seed=1):
    rng = np.random.default_rng(seed)
    arr = rng.integers(0, 256, (h, w, 3), dtype=np.uint8)
    # Smooth gradient plus a few shapes makes the image look less like noise
    # while still defeating naive compression assumptions.
    yy, xx = np.mgrid[0:h, 0:w]
    arr[:, :, 0] = (arr[:, :, 0] // 2 + (xx * 255 // max(w - 1, 1)) // 2).astype(np.uint8)
    arr[:, :, 1] = (arr[:, :, 1] // 2 + (yy * 255 // max(h - 1, 1)) // 2).astype(np.uint8)
    im = Image.fromarray(arr, "RGB")
    d = ImageDraw.Draw(im)
    for i in range(6):
        x0 = rng.integers(0, max(w - 10, 1))
        y0 = rng.integers(0, max(h - 10, 1))
        x1 = min(w, int(x0 + rng.integers(20, max(w // 3, 21))))
        y1 = min(h, int(y0 + rng.integers(20, max(h // 3, 21))))
        color = tuple(int(v) for v in rng.integers(0, 256, 3))
        d.rectangle([int(x0), int(y0), int(x1), int(y1)], outline=color, width=3)
    d.text((12, 12), f"CarveX corpus {w}x{h} seed={seed}", fill=(255, 255, 255))
    if mode == "RGBA":
        return im.convert("RGBA")
    if mode == "L":
        return im.convert("L")
    if mode == "P":
        return im.convert("P", palette=Image.Palette.ADAPTIVE, colors=64)
    if mode == "1":
        return im.convert("1")
    return im


def escape_pdf_text(s):
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def text_pdf(page_count=2, seed=1, incremental=False, padding=0):
    """Build a valid multi-page PDF with an xref table (and optional update)."""
    rng = random.Random(seed)
    words = (
        "forensic carving header footer unallocated cluster recovery validation "
        "jpeg png pdf signature structure evidence integrity fragment filesystem "
        "fat32 ntfs bitmap report experiment benchmark deleted file"
    ).split()
    lines = []
    for p in range(page_count):
        lines.append(f"CarveX synthetic document - page {p + 1} of {page_count}")
        for _ in range(28 + padding):
            lines.append(" ".join(rng.choice(words) for _ in range(rng.randint(8, 14))))
        lines.append("")
    # Split lines into pages and build one content stream per page.
    per_page = max(1, len(lines) // page_count)
    pages = [lines[i:i + per_page] for i in range(0, len(lines), per_page)][:page_count]

    objects = {}
    n_pages = len(pages)
    font_id = 3
    first_page = 4
    page_ids = [first_page + i * 2 for i in range(n_pages)]
    content_ids = [first_page + i * 2 + 1 for i in range(n_pages)]
    kids = " ".join(f"{pid} 0 R" for pid in page_ids)
    objects[1] = "<< /Type /Catalog /Pages 2 0 R >>"
    objects[2] = f"<< /Type /Pages /Kids [{kids}] /Count {n_pages} >>"
    objects[font_id] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
    for i, page_lines in enumerate(pages):
        pid = page_ids[i]
        cid = content_ids[i]
        content = "BT\n/F1 11 Tf\n14 TL\n54 748 Td\n"
        for line in page_lines:
            content += f"({escape_pdf_text(line)}) Tj\nT*\n"
        content += "ET\n"
        data = content.encode("latin-1", "replace")
        objects[cid] = (
            f"<< /Length {len(data)} >>\nstream\n" + content + "endstream"
        )
        objects[pid] = (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            f"/Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {cid} 0 R >>"
        )

    out = bytearray(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n")
    offsets = {}
    max_obj = max(objects)
    for num in sorted(objects):
        offsets[num] = len(out)
        out.extend(f"{num} 0 obj\n".encode())
        out.extend(objects[num].encode("latin-1", "replace"))
        out.extend(b"\nendobj\n")

    def append_xref(start_num, count, size, prev=None):
        nonlocal out
        xref_pos = len(out)
        out.extend(f"xref\n{start_num} {count}\n".encode())
        for n in range(start_num, start_num + count):
            if n == 0:
                out.extend(b"0000000000 65535 f \n")
            else:
                out.extend(f"{offsets[n]:010d} 00000 n \n".encode())
        trailer = f"<< /Size {size} /Root 1 0 R"
        if prev is not None:
            trailer += f" /Prev {prev}"
        trailer += " >>\n"
        out.extend(b"trailer\n")
        out.extend(trailer.encode())
        out.extend(f"startxref\n{xref_pos}\n%%EOF\n".encode())
        return xref_pos

    first_xref = append_xref(0, max_obj + 1, max_obj + 1)
    if incremental:
        update_id = max_obj + 1
        offsets[update_id] = len(out)
        out.extend(f"{update_id} 0 obj\n".encode())
        note = "incremental update object " + ("x" * 180)
        out.extend(f"<< /Type /Metadata /Subtype /XML /Note ({note}) >>".encode())
        out.extend(b"\nendobj\n")
        append_xref(update_id, 1, update_id + 1, prev=first_xref)
    return bytes(out)


def make_pdf_with_image(path, pages, seed):
    imgs = [image(w, h, seed=seed + i) for i, (w, h) in enumerate(pages)]
    first, rest = imgs[0], imgs[1:]
    if rest:
        first.save(path, "PDF", resolution=72.0, save_all=True, append_images=rest)
    else:
        first.save(path, "PDF", resolution=72.0)


def run(cmd):
    try:
        subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return True
    except Exception:
        return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=os.path.join(os.path.dirname(__file__), "corpus"))
    ap.add_argument("--seed", type=int, default=20261117)
    args = ap.parse_args()
    out = os.path.abspath(args.out)
    if os.path.isdir(out):
        shutil.rmtree(out)
    os.makedirs(os.path.join(out, "jpeg"), exist_ok=True)
    os.makedirs(os.path.join(out, "png"), exist_ok=True)
    os.makedirs(os.path.join(out, "pdf"), exist_ok=True)
    files = []

    def add(path, kind, desc):
        rel = os.path.relpath(path, out)
        files.append({
            "path": rel,
            "name": os.path.basename(path),
            "type": kind,
            "size": os.path.getsize(path),
            "sha256": sha256(path),
            "description": desc,
        })

    # --- JPEG ------------------------------------------------------------
    specs = [
        ("jpeg_01_baseline_640x480.jpg", (640, 480), "RGB", dict(quality=87), "baseline JFIF"),
        ("jpeg_02_large_1280x960.jpg", (1280, 960), "RGB", dict(quality=78), "large baseline"),
        ("jpeg_03_progressive_800x600.jpg", (800, 600), "RGB", dict(quality=82, progressive=True), "progressive"),
        ("jpeg_04_grayscale_512x512.jpg", (512, 512), "L", dict(quality=88), "grayscale"),
        ("jpeg_05_low_quality_1024x768.jpg", (1024, 768), "RGB", dict(quality=35), "low quality"),
        ("jpeg_06_portrait_480x800.jpg", (480, 800), "RGB", dict(quality=91), "portrait"),
        ("jpeg_07_cmyk_600x600.jpg", (600, 600), "RGB", dict(quality=84), "CMYK JPEG"),
        ("jpeg_08_exif_thumb_900x700.jpg", (900, 700), "RGB", dict(quality=86), "EXIF thumbnail"),
        ("jpeg_09_small_320x240.jpg", (320, 240), "RGB", dict(quality=80), "small baseline"),
        ("jpeg_10_progressive_1200x800.jpg", (1200, 800), "RGB", dict(quality=72, progressive=True), "progressive large"),
    ]
    thumb = image(160, 120, seed=args.seed + 77)
    thumb_path = os.path.join(out, "jpeg", "_thumb.jpg")
    thumb.save(thumb_path, quality=70)
    for i, (name, (w, h), mode, opts, desc) in enumerate(specs):
        path = os.path.join(out, "jpeg", name)
        im = image(w, h, mode, seed=args.seed + i * 13)
        if name.startswith("jpeg_07"):
            im = im.convert("CMYK")
        if name.startswith("jpeg_08"):
            im.save(path, quality=86)
            if shutil.which("exiftool"):
                run(["exiftool", "-q", "-overwrite_original", f"-ThumbnailImage<={thumb_path}", path])
        else:
            im.save(path, **opts)
        add(path, "jpeg", desc)
    os.remove(thumb_path)

    # --- PNG -------------------------------------------------------------
    png_specs = [
        ("png_01_rgba_640x480.png", (640, 480), "RGBA", "RGBA"),
        ("png_02_rgb_800x600.png", (800, 600), "RGB", "RGB"),
        ("png_03_grayscale_512x512.png", (512, 512), "L", "grayscale"),
        ("png_04_palette_600x400.png", (600, 400), "P", "indexed/palette"),
        ("png_05_1bit_400x300.png", (400, 300), "1", "1-bit"),
        ("png_06_large_1200x800.png", (1200, 800), "RGB", "large RGB"),
        ("png_07_small_200x150.png", (200, 150), "RGBA", "small RGBA"),
        ("png_08_interlaced_700x500.png", (700, 500), "RGB", "interlaced/Adam7"),
        ("png_09_alpha_1024x768.png", (1024, 768), "RGBA", "alpha channel"),
        ("png_10_palette_320x240.png", (320, 240), "P", "palette small"),
    ]
    for i, (name, (w, h), mode, desc) in enumerate(png_specs):
        path = os.path.join(out, "png", name)
        im = image(w, h, mode, seed=args.seed + 100 + i * 17)
        if "interlaced" in name and shutil.which("convert"):
            tmp = path + ".tmp.png"
            im.save(tmp, optimize=True)
            if not run(["convert", tmp, "-interlace", "PNG", path]):
                shutil.move(tmp, path)
            if os.path.exists(tmp):
                os.remove(tmp)
        else:
            im.save(path, optimize=True)
        add(path, "png", desc)

    # --- PDF -------------------------------------------------------------
    pdfs = [
        ("pdf_01_image_1page.pdf", lambda p: make_pdf_with_image(p, [(700, 500)], args.seed + 201), "image PDF, 1 page"),
        ("pdf_02_image_3pages.pdf", lambda p: make_pdf_with_image(p, [(600, 400), (500, 500), (700, 460)], args.seed + 202), "image PDF, 3 pages"),
        ("pdf_03_text_2pages.pdf", lambda p: open(p, "wb").write(text_pdf(2, args.seed + 203)), "text PDF, xref table"),
        ("pdf_04_incremental.pdf", lambda p: open(p, "wb").write(text_pdf(2, args.seed + 204, incremental=True)), "incremental update"),
        ("pdf_05_large_scan.pdf", lambda p: make_pdf_with_image(p, [(1400, 1000)], args.seed + 205), "large image PDF"),
        ("pdf_06_text_3pages.pdf", lambda p: open(p, "wb").write(text_pdf(3, args.seed + 206, padding=20)), "text PDF, 3 pages"),
        ("pdf_07_mixed_4pages.pdf", lambda p: make_pdf_with_image(p, [(500, 400), (600, 450), (450, 600), (550, 550)], args.seed + 207), "image PDF, 4 pages"),
        # padding=10 keeps this sample above the 4096-byte NTFS non-residency
        # guard for every lockbox seed (1001-1020); padding=6 failed for 19 of them.
        ("pdf_08_small_text.pdf", lambda p: open(p, "wb").write(text_pdf(1, args.seed + 208, padding=10)), "small text PDF"),
        ("pdf_09_text_long.pdf", lambda p: open(p, "wb").write(text_pdf(4, args.seed + 209, padding=30)), "long text PDF"),
        ("pdf_10_image_portrait.pdf", lambda p: make_pdf_with_image(p, [(520, 760)], args.seed + 210), "portrait image PDF"),
    ]
    for name, builder, desc in pdfs:
        path = os.path.join(out, "pdf", name)
        builder(path)
        if not os.path.exists(path):
            sys.exit(f"failed to create {path}")
        # NTFS stores files under ~700 bytes as resident attributes in $MFT,
        # which are outside unallocated clusters. Keep all samples non-resident.
        if os.path.getsize(path) < 4096:
            sys.exit(
                f"{path} is only {os.path.getsize(path)} bytes; increase text padding "
                "so NTFS does not store it resident in the MFT"
            )
        add(path, "pdf", desc)

    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "seed": args.seed,
        "file_count": len(files),
        "total_bytes": sum(f["size"] for f in files),
        "files": files,
    }
    with open(os.path.join(out, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=2)
    print(f"corpus: {len(files)} files, {manifest['total_bytes']} bytes -> {out}")
    for kind in ("jpeg", "png", "pdf"):
        n = sum(1 for f in files if f["type"] == kind)
        b = sum(f["size"] for f in files if f["type"] == kind)
        print(f"  {kind:5} {n:2} files, {b:8} bytes")


if __name__ == "__main__":
    main()
