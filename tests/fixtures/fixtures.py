#!/usr/bin/env python3
"""Fixture builders for the CarveX harness (Appendix C).  Pure standard library.
Usage: python3 fixtures.py CORPUS_DIR OUT_DIR      (CORPUS_DIR = <repo>/bench/corpus)
Builds the fixtures of Appendix A that need hand-made containers or file systems.
Every builder is deterministic; assert the sizes printed at the end before using them."""
import os, random, struct, sys, zlib


# ---- generic helpers -------------------------------------------------------
def rd(p):
    with open(p, "rb") as f:
        return f.read()


def wr(p, b):
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "wb") as f:
        f.write(b)


def pad4k(b):
    return b + b"\0" * (-len(b) % 4096)


def make_image(payloads, lead=4096, tail=65536):
    out = bytearray(b"\0" * lead)
    for p in payloads:
        out += pad4k(p)
    out += b"\0" * tail
    return bytes(out)

# ---- minimal FAT32 volume (all clusters free; root directory = cluster 2) ----
def fat32(total_sectors, spc):
    fat_sectors = (total_sectors * 4 + 1024 + 511) // 512
    boot = bytearray(512)
    boot[0:3] = bytes([0xEB, 0x3C, 0x90])
    boot[3:11] = b"MSWIN4.1"
    struct.pack_into("<H", boot, 0x0B, 512)
    boot[0x0D] = spc
    struct.pack_into("<H", boot, 0x0E, 1)
    boot[0x10] = 1
    boot[0x15] = 0xF8
    struct.pack_into("<I", boot, 0x20, total_sectors)
    struct.pack_into("<I", boot, 0x24, fat_sectors)
    struct.pack_into("<I", boot, 0x2C, 2)
    boot[0x47:0x52] = b"CARVEX     "
    boot[0x52:0x57] = b"FAT32"
    boot[510:512] = b"\x55\xaa"
    img = bytearray(512 * total_sectors)
    img[0:512] = boot
    fat = 512
    struct.pack_into("<I", img, fat, 0x0FFFFFF8)
    struct.pack_into("<I", img, fat + 4, 0x0FFFFFFF)
    struct.pack_into("<I", img, fat + 8, 0x0FFFFFFF)
    data_start = (1 + fat_sectors) * 512
    return img, data_start, spc * 512


# ---- E01 (EWF v1) writer: volume / sectors / table / done sections ----
def adler(b):
    return zlib.adler32(b) & 0xFFFFFFFF


def ewf_desc(kind, nxt, size):
    d = kind.encode().ljust(16, b"\0") + struct.pack("<QQ", nxt, size) + b"\0" * 40
    return d + struct.pack("<I", adler(d))


def build_e01(path, raw, spc=64, bps=512, compress=None, corrupt=None, media_sectors=None, chunk_bytes=None,
              declared_spc=None, declared_bps=None):
    """raw: bytes of the guest disk.  compress(i)->bool.  corrupt: set of chunk idx with a bad adler.
    The last chunk is stored SHORT (only the remaining bytes) like libewf does."""
    cs = chunk_bytes or spc * bps
    media = len(raw)
    sectors = media_sectors if media_sectors is not None else media // bps
    chunks = [raw[i:i + cs] for i in range(0, len(raw), cs)]
    blobs = []
    for i, c in enumerate(chunks):
        if compress and compress(i):
            blobs.append((True, zlib.compress(c)))
        else:
            a = adler(c)
            if corrupt and i in corrupt:
                a ^= 0xFFFF
            blobs.append((False, c + struct.pack("<I", a)))
    vol = struct.pack("<IIII", 1, len(chunks), declared_spc or spc, declared_bps or bps) + struct.pack("<Q", sectors)
    vol = vol.ljust(94, b"\0")
    pos = 13
    out = bytearray(b"EVF\x09\x0d\x0a\xff\x00" + b"\x01" + struct.pack("<H", 1) + b"\0\0")
    # volume
    vol_pos = pos
    nxt = vol_pos + 76 + len(vol)
    out += ewf_desc("volume", nxt, 76 + len(vol)) + vol
    # sectors
    sec_pos = len(out)
    total_blob = sum(len(b) for _, b in blobs)
    nxt = sec_pos + 76 + total_blob
    out += ewf_desc("sectors", nxt, 76 + total_blob)
    entries = []
    for comp, b in blobs:
        entries.append((len(out)) | (0x80000000 if comp else 0))
        out += b
    # table
    tab_pos = len(out)
    tab = struct.pack("<IIQII", len(entries), 0, 0, 0, 0) + b"".join(struct.pack("<I", e) for e in entries)
    nxt = tab_pos + 76 + len(tab)
    out += ewf_desc("table", nxt, 76 + len(tab)) + tab
    done_pos = len(out)
    out += ewf_desc("done", done_pos, 76)
    wr(path, bytes(out))



# ---- VMDK hosted sparse extent (KDMV) writer ----
def build_vmdk_sparse(path, capacity_sectors, grain_sectors, num_gte, gd_entries, tables, grains,
                      desc=None, flags=3):
    """gd_entries: list of table index (None=unallocated). tables: list of list[int] grain sector numbers.
    grains: dict sector->bytes written at that sector. Layout: sector0 header, sector1 desc, sector 8 GD, then tables."""
    hdr = bytearray(512)
    hdr[0:4] = b"KDMV"
    struct.pack_into("<II", hdr, 4, 1, flags)
    struct.pack_into("<QQ", hdr, 12, capacity_sectors, grain_sectors)
    struct.pack_into("<QQ", hdr, 28, 0, 0)
    struct.pack_into("<I", hdr, 44, num_gte)
    gd_sector = 8
    struct.pack_into("<Q", hdr, 56, gd_sector)
    hdr[73:77] = b"\n \r\n"
    img = bytearray(hdr)
    img += b"\0" * (gd_sector * 512 - len(img))
    gd_bytes = len(gd_entries) * 4
    gd_secs = -(-gd_bytes // 512)
    tab_start = gd_sector + gd_secs
    tab_secs = -(-(num_gte * 4) // 512)
    tab_sector = [tab_start + i * tab_secs for i in range(len(tables))]
    gd = b"".join(struct.pack("<I", 0 if t is None else tab_sector[t]) for t in gd_entries)
    img += gd.ljust(gd_secs * 512, b"\0")
    for t in tables:
        tb = b"".join(struct.pack("<I", g) for g in t)
        img += tb.ljust(tab_secs * 512, b"\0")
    for sec, data in sorted(grains.items()):
        need = sec * 512
        if len(img) < need:
            img += b"\0" * (need - len(img))
        img[need:need + len(data)] = data
    wr(path, bytes(img))



# ---- recipes ---------------------------------------------------------------
def e01_fixtures(out, jpg):
    cs = 64 * 512                                   # 32 KiB chunks

    def disk(nchunks, extra=0):
        d = bytearray(b"\0" * (nchunks * cs + extra))
        assert cs + len(jpg) <= len(d)
        d[cs:cs + len(jpg)] = jpg                   # JPEG at offset 32,768
        return bytes(d)

    d8, d75 = disk(8), disk(7, extra=cs // 2)       # 8 chunks / 7.5 chunks (short last chunk)
    build_e01(out + "/e01_good.E01", d8, compress=lambda i: i % 2 == 0)                    # S-E01-GOOD
    build_e01(out + "/e01_short_comp.E01", d75, compress=lambda i: True)                   # S-E01-SHORT
    build_e01(out + "/e01_short_raw.E01", d75, compress=lambda i: False)                   # S-E01-SHORT
    build_e01(out + "/e01_adler1.E01", d8, compress=lambda i: False, corrupt={1})          # S-E01-ADLER
    build_e01(out + "/e01_adler7.E01", d8, compress=lambda i: False, corrupt={7})          # S-E01-ADLER
    for spc, bps in ((1 << 20, 1 << 20), (1 << 16, 1 << 16), (1 << 12, 1 << 12)):         # S-E01-HUGE
        build_e01(out + f"/e01_huge_{spc}_{bps}.E01", b"\0" * 4096, spc=8, bps=512,
                  compress=lambda i: True, declared_spc=spc, declared_bps=bps, media_sectors=1)


def vmdk_fixtures(out, jpg):
    grain = 128                                     # 64 KiB grains
    ds = 100                                        # first data sector
    gdata = {ds + i * grain: jpg.ljust(4 * 65536, b"\0")[i * 65536:(i + 1) * 65536] for i in range(4)}
    tables = [[ds + i * grain for i in range(4)]]
    D = ("# Disk DescriptorFile\nversion=1\nCID=fffffffe\nparentCID=ffffffff\n"
         "createType=\"monolithicSparse\"\n\n# Extent description\nRW 4096 SPARSE \"%s\"\n")
    # S-VMDK-THIN: 8 grain-directory entries x 4 GTEs x 64 KiB = 2 MiB, only entry 0 allocated
    build_vmdk_sparse(out + "/vm_thin_ext.vmdk", 4096, grain, 4, [0] + [None] * 7, tables, gdata)
    wr(out + "/vm_thin.vmdk", (D % "vm_thin_ext.vmdk").encode())
    # control: same, entries 1..7 point at a table of zero grains
    build_vmdk_sparse(out + "/vm_full_ext.vmdk", 4096, grain, 4, [0] + [1] * 7, [tables[0], [0] * 4], gdata)
    wr(out + "/vm_full.vmdk", (D % "vm_full_ext.vmdk").encode())
    # S-VMDK-ALIAS: n directory entries, all pointing at one 256 KiB table (file stays ~0.27 MiB)
    for n in (1024, 4096):
        num_gte = 65536
        build_vmdk_sparse(out + f"/vm_alias{n}_ext.vmdk", n * num_gte, 1, num_gte, [0] * n, [[0] * num_gte], {})
        wr(out + f"/vm_alias{n}.vmdk", ("# Disk DescriptorFile\nversion=1\ncreateType=\"monolithicSparse\"\n\n"
           f"# Extent description\nRW {n * num_gte} SPARSE \"vm_alias{n}_ext.vmdk\"\n").encode())
    # S-VMDK-PATH: absolute path and '..' to a 512-byte secret outside the evidence directory
    wr(out + "/vm_secret.bin", b"SECRET-OUTSIDE-EVIDENCE" + b"\0" * (512 - 23))
    for tag, target in (("abs", os.path.abspath(out + "/vm_secret.bin")), ("dotdot", "../../vm_secret.bin")):
        wr(out + f"/tv_{tag}/sub/d.vmdk", ("# Disk DescriptorFile\nversion=1\ncreateType=\"monolithicFlat\"\n\n"
           f"# Extent description\nRW 1 FLAT \"{target}\" 0\n").encode())
    # S-VMDK-RENAMED: self-contained sparse extent whose embedded descriptor names original_name.vmdk
    emb = bytearray(open(out + "/vm_full_ext.vmdk", "rb").read())
    desc = (D % "original_name.vmdk").replace("CID=fffffffe\nparentCID=ffffffff\n", "").encode().ljust(512, b"\0")
    struct.pack_into("<QQ", emb, 28, 2, 1)          # descriptor offset (sector 2), size (1 sector)
    emb[1024:1536] = desc
    wr(out + "/vm_renamed.vmdk", bytes(emb))
    # S-VMDK-ZERO (small control) and the 512 TiB case
    wr(out + "/vm_zero_flat.bin", b"\x11" * 4096)
    wr(out + "/vm_zero.vmdk", ("# Disk DescriptorFile\nversion=1\ncreateType=\"custom\"\n\n# Extent description\n"
                              "RW 8 ZERO\nRW 8 FLAT \"vm_zero_flat.bin\" 0\n").encode())
    wr(out + "/z512/flat.bin", b"\0" * 4096)
    wr(out + "/z512/d.vmdk", ("# Disk DescriptorFile\nversion=1\nCID=fffffffe\nparentCID=ffffffff\n"
        "createType=\"monolithicFlat\"\n\n# Extent description\nRW 1099511627776 ZERO\nRW 8 FLAT \"flat.bin\" 0\n").encode())


def detect_fixtures(out, jpg):                       # S-DETECT: a JPEG is planted at byte 65,536 of each image
    a = bytearray(1 << 20); a[4096:4096 + 24] = b"# Extent description\n\n\n"
    b = bytearray(1 << 20); b[-512:-504] = b"conectix"
    c = bytearray(1 << 20); c[0:4] = b"KDMV"
    for n, v in (("det_text", a), ("det_conectix", b), ("det_kdmv", c)):
        v[65536:65536 + len(jpg)] = jpg
        wr(out + f"/{n}.img", bytes(v))


def b06_fixture(out, corpus, gapfill):               # S-B06; gapfill in {"zeros", "random", "otherfile"}
    files = {"jpeg": rd(corpus + "/jpeg/jpeg_01_baseline_640x480.jpg"),
             "png": rd(corpus + "/png/png_02_rgb_800x600.png"),
             "pdf": rd(corpus + "/pdf/pdf_05_large_scan.pdf")}
    other = rd(corpus + "/jpeg/jpeg_04_grayscale_512x512.jpg")
    img, ds, cs = fat32(131072, 8)                   # 64 MiB, 4 KiB clusters
    cl, rng = 200, random.Random(5)
    for k, v in files.items():
        half = (len(v) // 2) // 4096 * 4096          # cluster-aligned midpoint
        h1, h2 = v[:half], v[half:]
        gap = (b"\0" * (3 * cs) if gapfill == "zeros" else rng.randbytes(3 * cs) if gapfill == "random"
               else other[20000:20000 + 3 * cs])
        base = ds + (cl - 2) * cs
        img[base:base + len(h1)] = h1
        o2 = base + (-(-len(h1) // cs)) * cs
        img[o2:o2 + len(gap)] = gap
        o3 = o2 + len(gap)
        img[o3:o3 + len(h2)] = h2
        cl += (o3 + len(h2) - base) // cs + 60
    wr(out + f"/b06_{gapfill}.img", bytes(img))      # oracle: SHA-256 of each corpus file


def tail_image(path, src_file, frac, pad, pad_len, seed=7, lead=4096, trail=8192):
    """S-TAIL: `lead` zero bytes + the first `frac` of src_file + padding + `trail` zero bytes.
    pad in {"zeros", "ff", "random"} (random is seeded -> deterministic). Returns the cut length."""
    d = rd(src_file)
    cut = int(len(d) * frac)
    if pad == "zeros":
        padding = b"\0" * pad_len
    elif pad == "ff":
        padding = b"\xff" * pad_len
    else:
        padding = random.Random(seed).randbytes(pad_len)
    wr(path, b"\0" * lead + d[:cut] + padding + b"\0" * trail)
    return cut


def tail_fixtures(out, corpus, big=False):          # S-TAIL, S-TAIL-NEIGH
    src = {"jpeg": corpus + "/jpeg/jpeg_10_progressive_1200x800.jpg",
           "png": corpus + "/png/png_02_rgb_800x600.png",
           "pdf": corpus + "/pdf/pdf_05_large_scan.pdf"}
    cuts = {}
    for fmt, f in src.items():
        for pad in ("zeros", "ff", "random"):
            cuts[f"tail_{fmt}_{pad}"] = tail_image(f"{out}/tail_{fmt}_{pad}.img", f, 0.6, pad, 1 << 20)
    if big:                                          # the large JPEG rows of S-TAIL (about 150 MB on disk)
        for pad, mib in (("zeros", 48), ("random", 48), ("ff", 4), ("ff", 48)):
            cuts[f"tail_jpeg_{pad}_{mib}M"] = tail_image(f"{out}/tail_jpeg_{pad}_{mib}M.img", src["jpeg"], 0.6, pad, mib << 20)
    # S-TAIL-NEIGH: a truncated JPEG directly followed by a complete one (0 or 4,096 zero bytes between)
    d = rd(src["jpeg"])
    cut = int(len(d) * 0.6)
    nxt = rd(corpus + "/jpeg/jpeg_03_progressive_800x600.jpg")
    for gap in (0, 4096):
        wr(f"{out}/tail_jpeg_neigh_{gap}.img", b"\0" * 4096 + d[:cut] + b"\0" * gap + nxt + b"\0" * 8192)
    cuts["neigh_next_size"] = len(nxt)
    return cuts


def mbr(parts):                                      # parts: [(type, start_lba, nsect)]
    m = bytearray(512)
    for i, (t, s, n) in enumerate(parts):
        e = 446 + 16 * i
        m[e + 4] = t
        struct.pack_into("<II", m, e + 8, s, n)
    m[510:512] = b"\x55\xaa"
    return bytes(m)


def regions_fixtures(out, corpus):                   # S-REGIONS (51 MiB = 53,477,376 B)
    MiB = 1 << 20
    j = rd(corpus + "/jpeg/jpeg_01_baseline_640x480.jpg")
    v1 = fat32(32 * MiB // 512, 8)[0]
    v2 = fat32(16 * MiB // 512, 8)[0]
    disk = bytearray(51 * MiB)                       # 1 MiB gap | 32 MiB FAT32 | 2 MiB gap | 16 MiB FAT32
    disk[0:512] = mbr([(0x0C, 2048, 32 * MiB // 512), (0x0C, 35 * MiB // 512, 16 * MiB // 512)])
    disk[MiB:MiB + len(v1)] = v1
    disk[35 * MiB:35 * MiB + len(v2)] = v2
    disk[33 * MiB + 4096:33 * MiB + 4096 + len(j)] = j   # JPEG in the 2 MiB gap
    disk[10 * MiB:10 * MiB + len(j)] = j                 # JPEG in the free area of volume 1
    wr(out + "/regions.img", bytes(disk))
    bad = bytearray(disk)                            # second volume: type 0x07 with an NTFS signature but garbage
    bad[0:512] = mbr([(0x0C, 2048, 32 * MiB // 512), (0x07, 35 * MiB // 512, 16 * MiB // 512)])
    bad[35 * MiB:35 * MiB + 512] = b"\0" * 512
    bad[35 * MiB + 3:35 * MiB + 11] = b"NTFS    "
    bad[35 * MiB + 510:35 * MiB + 512] = b"\x55\xaa"
    wr(out + "/regions_ntfsbad.img", bytes(bad))


def flood_fixtures(out, big=True):                   # S9 / S10 (guide S9-S11); random file is not deterministic
    for name, n in (("S9-256K", 256 << 10), ("S9-1M", 1 << 20), ("S9-4M", 4 << 20)):
        wr(f"{out}/{name}.img", (b"\xff\xd8\xff\xe0" + b"A" * 12) * (n // 16))
    if big:
        hdr = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00"
        d = bytearray(64 << 20)
        for off in range(0, len(d) - len(hdr), 512):
            d[off:off + len(hdr)] = hdr
        wr(out + "/S10-flood512-64M.img", bytes(d))
        wr(out + "/random-64M.img", random.Random(1).randbytes(64 << 20))


def pngcrc_fixture(out, corpus, nbad=3):             # S-PNGCRC (nbad IDAT chunks with a flipped CRC byte)
    png = rd(corpus + "/png/png_02_rgb_800x600.png")
    p, idat = 8, []
    while p < len(png):
        n, = struct.unpack(">I", png[p:p + 4])
        if png[p + 4:p + 8] == b"IDAT":
            idat.append((p, n))
        p += 12 + n
    b = bytearray(png)
    for p, n in idat[:nbad]:
        b[p + 8 + n] ^= 1
    wr(f"{out}/pngcrc{nbad}.img", make_image([bytes(b)]))


if __name__ == "__main__":
    corpus, out = sys.argv[1], sys.argv[2]
    jpg = rd(corpus + "/jpeg/jpeg_01_baseline_640x480.jpg")
    e01_fixtures(out, jpg)
    vmdk_fixtures(out, jpg)
    detect_fixtures(out, jpg)
    for g in ("zeros", "random", "otherfile"):
        b06_fixture(out, corpus, g)
    regions_fixtures(out, corpus)
    flood_fixtures(out, big="--big" in sys.argv)
    for k in (1, 3):
        pngcrc_fixture(out, corpus, k)
    print(tail_fixtures(out, corpus, big="--big" in sys.argv))
    print(sorted(os.listdir(out)))
