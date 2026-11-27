#!/usr/bin/env python3
"""X2 — oracle-guided bifragment gap carving for PNG with the chunk CRC-32 as the oracle (harness item H-30).
usage: x2_png_crc.py CORPUS [CARVEX WORKDIR]
The solver walks the chunks of a PNG laid out on a 'disk'; at the first chunk whose CRC fails it tries every cluster-aligned
break k inside that chunk and every gap g in 1..16 clusters and keeps the stitches whose CRC-32 matches.
Two gap kinds, N=200 each: seeded random bytes, and the interior of another PNG (compressed data).
With CARVEX the same layouts (N=30) go through `scan --fragment-recovery` as a deleted FAT32 file, for comparison."""
import glob, hashlib, math, os, random, struct, sys, time, zlib
C, CL = sys.argv[1], 4096
CARVEX = sys.argv[2] if len(sys.argv) > 2 else None
W = sys.argv[3] if len(sys.argv) > 3 else '/tmp/x2'
pngs = [(os.path.basename(p), open(p, 'rb').read()) for p in sorted(glob.glob(C + '/png/*.png'))]
pngs = [(n, d) for n, d in pngs if len(d) >= 12 * CL]


def crc_ok(buf, pos, L):
    return zlib.crc32(buf[pos + 4:pos + 8 + L]) & 0xFFFFFFFF == struct.unpack('>I', buf[pos + 8 + L:pos + 12 + L])[0]


def solve(disk, maxgap=16):
    """Returns the list of (k, g) stitches whose chunk CRC matches ([] = nothing found / file intact)."""
    pos = 8
    while pos + 12 <= len(disk):
        L, = struct.unpack('>I', disk[pos:pos + 4])
        end = pos + 12 + L
        if end > len(disk):
            return []
        if crc_ok(disk, pos, L):
            if disk[pos + 4:pos + 8] == b'IEND':
                return []
            pos = end
            continue
        hits = []
        for k in range(-(-(pos + 1) // CL), (end - 1) // CL + 1):          # cluster-aligned breaks inside the chunk
            head = disk[pos:k * CL]
            for g in range(1, maxgap + 1):
                t0, need = (k + g) * CL, 12 + L - len(head)
                if t0 + need <= len(disk) and crc_ok(head + disk[t0:t0 + need], 0, L):
                    hits.append((k, g))
        return hits
    return []


def wilson(k, n, z=1.96):
    p = k / n
    c, h = (p + z * z / 2 / n) / (1 + z * z / n), z * math.sqrt(p * (1 - p) / n + z * z / 4 / n / n) / (1 + z * z / n)
    return round(max(0, c - h), 3), round(min(1, c + h), 3)


def layout(kind, rnd):
    name, d = rnd.choice(pngs)
    n = -(-len(d) // CL)
    s, g = rnd.randint(1, n - 2), rnd.randint(1, 16)
    if kind == 'random':
        isl = rnd.randbytes(g * CL)
    else:
        _, od = rnd.choice([x for x in pngs if x[0] != name])
        m = -(-len(od) // CL)
        st = rnd.randint(1, max(1, m - g - 1))
        isl = od[st * CL:(st + g) * CL]
        isl += rnd.randbytes(g * CL - len(isl))
    return d, n, s, g, isl


rnd = random.Random(11)
for kind in ('random', 'other PNG interior'):
    N, ok, t0 = 200, 0, time.time()
    for _ in range(N):
        d, n, s, g, isl = layout(kind, rnd)
        ok += solve(d[:s * CL] + isl + d[s * CL:]) == [(s, g)]
    print(f'CRC oracle, gap = {kind:18s}: unique+correct {ok}/{N}  Wilson95 {wilson(ok, N)}  ({time.time() - t0:.1f}s)')
if CARVEX:
    import json, shutil, subprocess
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from fat32_img import build_fat32
    os.makedirs(W, exist_ok=True)
    rnd = random.Random(11)
    for kind in ('random', 'other PNG interior'):
        N, ok = 30, 0
        for i in range(N):
            d, n, s, g, isl = layout(kind, rnd)
            cl = list(range(100, 100 + s)) + list(range(100 + s + g, 100 + s + g + n - s))
            img, info = build_fat32(total_sectors=131072, files=[('F.PNG', d, cl)], deleted=['F.PNG'])
            b = bytearray(img)
            for j in range(g):
                o = info['coff'](100 + s + j)
                b[o:o + CL] = isl[j * CL:(j + 1) * CL]
            open(f'{W}/x2.img', 'wb').write(bytes(b))
            shutil.rmtree(f'{W}/out', ignore_errors=True)
            subprocess.run([CARVEX, 'scan', f'{W}/x2.img', '-o', f'{W}/out', '--quiet', '--json-report', f'{W}/x2.json',
                            '--validate', 'decode', '--fragment-recovery'], capture_output=True, timeout=300)
            ok += any(f['sha256'] == hashlib.sha256(d).hexdigest() for f in json.load(open(f'{W}/x2.json'))['files'])
        print(f'CarveX --fragment-recovery, gap = {kind:18s}: exact {ok}/{N}  Wilson95 {wilson(ok, N)}')
