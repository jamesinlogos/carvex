#!/usr/bin/env python3
"""H-32 experiment: JPEG entropy-cluster prefilter, break estimate, seam ranking.

usage: h32_jpeg_join.py CARVEX CORPUS WORKDIR [N=100]

No production source is changed.  The script reproduces the X1 cluster rule,
builds two-fragment baseline-JPEG layouts from the frozen corpus, filters tail
clusters with X1, ranks candidate (k, g) joins with a seam-continuity score,
accepts only a best candidate with a margin plus an independent Pillow decode,
and reports exact recall with Wilson 95% intervals and the false-accept rate.
"""
import glob
import hashlib
import io
import json
import math
import os
import random
import statistics
import struct
import subprocess
import sys
import time

from PIL import Image

CARVEX, CORPUS, W = sys.argv[1], sys.argv[2], sys.argv[3]
N = int(sys.argv[4]) if len(sys.argv) > 4 else 100
CL = 4096
MAX_GAP = 8
SEED = 32012026
MARGIN = 0.90
os.makedirs(W, exist_ok=True)
sha = lambda data: hashlib.sha256(data).hexdigest()


def wilson(k, n, z=1.96):
    p = k / n
    c = (p + z * z / (2 * n)) / (1 + z * z / n)
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return round(max(0.0, c - h), 3), round(min(1.0, c + h), 3)


def stuffing_ok(buf, i):
    seg = buf[i:i + CL + 1]
    j = seg.find(b'\xff')
    while j != -1 and j < CL:
        if j + 1 < len(seg) and seg[j + 1] not in (0, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7):
            return False
        j = seg.find(b'\xff', j + 1)
    return True


def entropy(data):
    counts = {}
    for byte in data:
        counts[byte] = counts.get(byte, 0) + 1
    n = len(data)
    return -sum((v / n) * math.log2(v / n) for v in counts.values()) if n else 0.0


def x1(data, offset):
    return stuffing_ok(data, offset) and entropy(data[offset:offset + CL]) >= 7.0


def scan_start(data):
    pos = 2
    while pos + 4 <= len(data):
        marker = data[pos + 1]
        if marker == 0xDA:
            length = struct.unpack('>H', data[pos + 2:pos + 4])[0]
            return pos + 2 + length
        if marker == 0xD9:
            return None
        if marker == 0x01 or 0xD0 <= marker <= 0xD7:
            pos += 2
            continue
        if pos + 2 > len(data):
            return None
        length = struct.unpack('>H', data[pos + 2:pos + 4])[0]
        pos += 2 + length
    return None


def restart_interval(data):
    pos = 2
    while pos + 4 <= len(data):
        marker = data[pos + 1]
        if marker == 0xDA or marker == 0xD9:
            return 0
        if marker == 0xDD:
            return struct.unpack('>H', data[pos + 4:pos + 6])[0]
        length = struct.unpack('>H', data[pos + 2:pos + 4])[0]
        pos += 2 + length
    return 0


def rst_sequence(data, start):
    out = []
    pos = start
    while pos + 1 < len(data):
        if data[pos] == 0xFF and 0xD0 <= data[pos + 1] <= 0xD7:
            out.append(data[pos + 1] & 7)
            pos += 2
        elif data[pos] == 0xFF and data[pos + 1] == 0x00:
            pos += 2
        elif data[pos] == 0xFF and data[pos + 1] not in (0xD8, 0xD9):
            break
        else:
            pos += 1
    return out


def seam_score(data, dims):
    """Mean absolute row difference; lower means a smoother seam."""
    try:
        with Image.open(io.BytesIO(data)) as image:
            image.load()
            if image.size != dims:
                return None
            rgb = image.convert('RGB')
            pixels = list(rgb.getdata())
    except Exception:
        return None
    width, height = dims
    if width < 8 or height < 8:
        return None
    diffs = []
    for y in range(8, height, 8):
        row = y * width
        prev = (y - 1) * width
        total = 0
        for x in range(width):
            a = pixels[row + x]
            b = pixels[prev + x]
            total += abs(a[0] - b[0]) + abs(a[1] - b[1]) + abs(a[2] - b[2])
        diffs.append(total / (width * 3))
    return statistics.mean(diffs) if diffs else None


jpeg = []
for path in sorted(glob.glob(os.path.join(CORPUS, 'jpeg', '*.jpg'))):
    data = open(path, 'rb').read()
    if b'\xff\xc2' in data[:2000] or len(data) < 20 * CL:
        continue
    start = scan_start(data)
    if start is None:
        continue
    with Image.open(io.BytesIO(data)) as image:
        dims = image.size
    jpeg.append((os.path.basename(path), data, start, dims, restart_interval(data)))
if len(jpeg) < 2:
    raise SystemExit('need at least two baseline JPEGs of at least 20 clusters')

commit = subprocess.check_output(
    ['git', '-C', os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
     'rev-parse', 'HEAD'], text=True).strip()
manifest = os.path.join(CORPUS, 'manifest.json')
print('H-32 JPEG join experiment (no production source changed)')
print('commit: ' + commit)
print('binary (not used by experiment): {} ({})'.format(CARVEX, sha(open(CARVEX, 'rb').read())))
print('corpus: {} ({})'.format(CORPUS, sha(open(manifest, 'rb').read())))
print('Pillow: ' + Image.__version__)
print('seed: {}, cluster: {}, max gap: {}, margin: {:.2f}, N={} per gap kind'.format(
    SEED, CL, MAX_GAP, MARGIN, N))
print('baseline JPEGs: ' + ', '.join(name for name, _, _, _, _ in jpeg))
print('restart intervals: ' + ', '.join('{}={}'.format(name, ri) for name, _, _, _, ri in jpeg))

rnd = random.Random(SEED)
for kind in ('random', 'other JPEG interior'):
    x1_hits = exact = false_accept = no_margin = no_decode = 0
    candidates_total = 0
    started = time.monotonic()
    for i in range(N):
        name, data, entropy_start, dims, ri = rnd.choice(jpeg)
        clusters = -(-len(data) // CL)
        split = rnd.randint(max(2, entropy_start // CL + 1), clusters - 3)
        gap = rnd.randint(1, MAX_GAP)
        if kind == 'random':
            island = rnd.randbytes(gap * CL)
        else:
            other = rnd.choice([p for p in jpeg if p[0] != name])
            pos = rnd.randint(1, max(1, -(-len(other[1]) // CL) - gap - 1))
            island = other[1][pos * CL:(pos + gap) * CL]
            island += rnd.randbytes(gap * CL - len(island))
        disk = data[:split * CL] + island + data[split * CL:]
        head = disk[:split * CL]
        head_rst = rst_sequence(head, entropy_start)
        ranked = []
        for k in range(max(1, split - 1), split + 2):
            for g in range(1, MAX_GAP + 1):
                tail = (k + g) * CL
                if tail >= len(disk):
                    continue
                if not x1(disk, tail):
                    continue
                x1_hits += 1
                stitched = disk[:k * CL] + disk[tail:]
                if ri:
                    tail_rst = rst_sequence(stitched[tail:], 0)
                    if tail_rst and ((head_rst[-1] + 1) % 8 if head_rst else 0) != tail_rst[0]:
                        continue
                score = seam_score(stitched, dims)
                if score is None:
                    no_decode += 1
                    continue
                candidates_total += 1
                ranked.append((score, k, g, sha(stitched)))
        ranked.sort()
        if not ranked:
            continue
        best = ranked[0]
        second = ranked[1][0] if len(ranked) > 1 else math.inf
        if second < math.inf and best[0] > MARGIN * second:
            no_margin += 1
            continue
        if best[3] == sha(data):
            exact += 1
        else:
            false_accept += 1
    low, high = wilson(exact, N)
    print('\n== gap = {}: exact {}/{} = {:.1f}%, Wilson95 ({}, {})'.format(
        kind, exact, N, 100 * exact / N, low, high))
    print('   false accepted {}/{}, no-margin {}/{}, X1 tail hits {}, decoded candidates {}, {:.1f}s total'.format(
        false_accept, N, no_margin, N, x1_hits, candidates_total, time.monotonic() - started))
print('\nH-09 baseline: 3 of 4,000 synthetic pairs accepted in decode mode (Wilson 95% upper bound 0.22%).')
print('No fixed recall target. Flat/gradient content remains a known weakness; no production claim is made here.')
