#!/usr/bin/env python3
"""E1 — a deleted file fragmented around another cluster run ("island"); harness item H-27 / H-30.
usage: e1_islands.py CARVEX CORPUS WORKDIR [N=40]
Per trial: one corpus JPEG/PNG (>= 12 clusters) is deleted from a FAT32 image; its clusters are split at a random
cluster boundary and an island of 1..8 clusters sits in between.
  live : the island is a LIVE file (allocated, random bytes)           -> the free-space stream can skip it
  dead : the island is the interior of ANOTHER DELETED file (free)     -> only a content oracle can reject it
Methods: default scan, `--fragment-recovery`, and the free-space stream (free clusters concatenated, raw scan).
Output: exact recall (SHA-256 equal to the original) with Wilson 95 % intervals."""
import glob, hashlib, json, math, os, random, shutil, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fat32_img import build_fat32, free_stream

CARVEX, CORPUS, W = sys.argv[1], sys.argv[2], sys.argv[3]
N = int(sys.argv[4]) if len(sys.argv) > 4 else 40
CL = 4096
os.makedirs(W, exist_ok=True)
sha = lambda b: hashlib.sha256(b).hexdigest()


def carve(img, tag, extra):
    out, rep = f'{W}/o_{tag}', f'{W}/r_{tag}.json'
    shutil.rmtree(out, ignore_errors=True)
    if os.path.exists(rep):
        os.remove(rep)
    subprocess.run([CARVEX, 'scan', img, '-o', out, '--quiet', '--json-report', rep, '--validate', 'decode'] + extra,
                   capture_output=True, timeout=300)
    return json.load(open(rep))['files'] if os.path.exists(rep) else []


def wilson(k, n, z=1.96):
    p = k / n
    c, h = (p + z * z / 2 / n) / (1 + z * z / n), z * math.sqrt(p * (1 - p) / n + z * z / 4 / n / n) / (1 + z * z / n)
    return round(max(0, c - h), 3), round(min(1, c + h), 3)


corp = []
for p in sorted(glob.glob(CORPUS + '/jpeg/*.jpg') + glob.glob(CORPUS + '/png/*.png')):
    n = os.path.basename(p)
    d = open(p, 'rb').read()
    if len(d) >= 12 * CL and not any(w in n for w in ('interlaced', 'progressive', 'cmyk')):
        corp.append((n, d))
print('corpus files used (%d): %s' % (len(corp), ', '.join(n for n, _ in corp)))
random.seed(2026)


def trial(kind, i):
    name, d = random.choice(corp)
    n = -(-len(d) // CL)
    s, g = random.randint(2, n - 2), random.randint(1, 8)
    if kind == 'live':
        isl = random.randbytes(g * CL)
    else:
        _, od = random.choice([x for x in corp if x[0] != name])
        m = -(-len(od) // CL)
        st = random.randint(2, max(2, m - g - 2))
        isl = od[st * CL:(st + g) * CL]
        isl += random.randbytes(g * CL - len(isl))
    cl = list(range(100, 100 + s)) + list(range(100 + s + g, 100 + s + g + n - s))
    files, deleted = [('F.BIN', d, cl)], ['F.BIN']
    if kind == 'live':
        files.append(('LIVE.DAT', isl, list(range(100 + s, 100 + s + g))))
    img, info = build_fat32(files=files, deleted=deleted)
    if kind == 'dead':                       # island clusters are free: written but never allocated in the FAT
        b = bytearray(img)
        for j in range(g):
            o = info['coff'](100 + s + j)
            b[o:o + CL] = isl[j * CL:(j + 1) * CL]
        img = bytes(b)
    p, sp = f'{W}/{kind}{i}.img', f'{W}/{kind}{i}.stream'
    open(p, 'wb').write(img)
    open(sp, 'wb').write(free_stream(img, info))
    res = {}
    for tag, pp, ex in (('default', p, []), ('fragment-recovery', p, ['--fragment-recovery']),
                        ('free-space stream', sp, ['--mode', 'raw', '--no-fs'])):
        res[tag] = any(f['sha256'] == sha(d) for f in carve(pp, f'{kind}{i}{tag[:4]}', ex))
    return res


for kind, label in (('live', 'island = LIVE (allocated) file'), ('dead', 'island = interior of another DELETED file (free)')):
    tot = {}
    for i in range(N):
        for k, v in trial(kind, i).items():
            tot[k] = tot.get(k, 0) + v
    print(f'== {label}: N={N}')
    for k, v in tot.items():
        print(f'   {k:18s} exact {v:2d}/{N} = {100 * v / N:5.1f}%  Wilson95 {wilson(v, N)}')
