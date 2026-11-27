#!/usr/bin/env python3
"""H-30 production acceptance: PNG CRC-guided joins through --fragment-recovery.

usage: h30_png_crc.py CARVEX CORPUS WORKDIR [N=100]

Builds synthetic FAT32 fixtures from the frozen PNG corpus: the deleted file is
split around a random gap or the interior of another PNG.  Exact SHA-256 and
Wilson 95% intervals are reported for N trials per gap kind.  Controls cover an
intact PNG, a missing second fragment, a gap containing a valid-looking chunk
header, and a live allocated island in unallocated vs raw mode.  No result is
counted as verified unless the report says validity='verified'.
"""
import glob
import hashlib
import json
import math
import os
import random
import shutil
import struct
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fat32_img import build_fat32

CARVEX, CORPUS, W = sys.argv[1], sys.argv[2], sys.argv[3]
N = int(sys.argv[4]) if len(sys.argv) > 4 else 100
CL = 4096
TOTAL_SECTORS = 8192
START_CLUSTER = 100
SEED = 302026
os.makedirs(W, exist_ok=True)
sha = lambda data: hashlib.sha256(data).hexdigest()


def wilson(k, n, z=1.96):
    p = k / n
    c = (p + z * z / (2 * n)) / (1 + z * z / n)
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return round(max(0.0, c - h), 3), round(min(1.0, c + h), 3)


pngs = []
for path in sorted(glob.glob(os.path.join(CORPUS, 'png', '*.png'))):
    data = open(path, 'rb').read()
    if len(data) >= 12 * CL:
        pngs.append((os.path.basename(path), data))
if len(pngs) < 3:
    raise SystemExit('need at least three PNGs of at least 12 clusters')


def layout(kind, rnd):
    name, data = rnd.choice(pngs)
    clusters = -(-len(data) // CL)
    split = rnd.randint(1, clusters - 2)
    gap = rnd.randint(1, 16)
    if kind == 'random':
        island = rnd.randbytes(gap * CL)
    else:
        other = rnd.choice([p for p in pngs if p[0] != name])
        start = rnd.randint(1, max(1, -(-len(other[1]) // CL) - gap - 1))
        island = other[1][start * CL:(start + gap) * CL]
        island += rnd.randbytes(gap * CL - len(island))
    return name, data, clusters, split, gap, island


def build_image(data, clusters, split, gap, island, live=False):
    runs = (list(range(START_CLUSTER, START_CLUSTER + split))
            + list(range(START_CLUSTER + split + gap,
                         START_CLUSTER + split + gap + clusters - split)))
    files = [('F.PNG', data, runs)]
    deleted = ['F.PNG']
    if live:
        files.append(('LIVE.DAT', island, list(range(START_CLUSTER + split, START_CLUSTER + split + gap))))
    image, info = build_fat32(total_sectors=TOTAL_SECTORS, files=files, deleted=deleted)
    if not live:
        mutable = bytearray(image)
        for i in range(gap):
            off = info['coff'](START_CLUSTER + split + i)
            mutable[off:off + CL] = island[i * CL:(i + 1) * CL]
        image = bytes(mutable)
    return image


def scan(tag, image, mode='unallocated'):
    path = os.path.join(W, tag + '.img')
    open(path, 'wb').write(image)
    out = os.path.join(W, 'out-' + tag)
    report = os.path.join(W, 'report-' + tag + '.json')
    shutil.rmtree(out, ignore_errors=True)
    if os.path.exists(report):
        os.remove(report)
    started = time.monotonic()
    result = subprocess.run([
        CARVEX, 'scan', path, '-o', out, '--quiet', '--json-report', report,
        '--mode', mode, '--types', 'png', '--validate', 'decode',
        '--fragment-recovery', '--frag-time-limit', '5000',
    ], capture_output=True, timeout=300)
    elapsed = time.monotonic() - started
    if not os.path.exists(report):
        raise RuntimeError('{}: no report: {}'.format(tag, result.stderr.decode(errors='replace')))
    return json.load(open(report)).get('files', []), elapsed, result.returncode


def cleanup(tag):
    """Remove one trial's fixture so 200 trials do not consume gigabytes."""
    for path in (os.path.join(W, tag + '.img'),
                 os.path.join(W, 'report-' + tag + '.json')):
        if os.path.exists(path):
            os.remove(path)
    shutil.rmtree(os.path.join(W, 'out-' + tag), ignore_errors=True)


commit = subprocess.check_output(
    ['git', '-C', os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
     'rev-parse', 'HEAD'], text=True).strip()
manifest = os.path.join(CORPUS, 'manifest.json')
print('H-30 production --fragment-recovery acceptance')
print('commit: ' + commit)
print('binary: {} ({})'.format(CARVEX, sha(open(CARVEX, 'rb').read())))
print('corpus: {} ({})'.format(CORPUS, sha(open(manifest, 'rb').read())))
print('seed: {}, cluster: {}, N={} per gap kind, gap 1..16 clusters'.format(SEED, CL, N))
print('PNGs: ' + ', '.join(name for name, _ in pngs))

rnd = random.Random(SEED)
overall_exact = overall_false = overall_wilson_low = 0
for kind in ('random', 'other PNG interior'):
    exact = false_verified = 0
    elapsed_total = 0.0
    for i in range(N):
        name, data, clusters, split, gap, island = layout(kind, rnd)
        image = build_image(data, clusters, split, gap, island)
        tag = '{}-{}'.format(kind.split()[0], i)
        files, elapsed, _ = scan(tag, image)
        cleanup(tag)
        elapsed_total += elapsed
        exact += any(
            f.get('sha256') == sha(data) and f.get('validity') == 'verified'
            and f.get('source') == 'bgc' for f in files
        )
        false_verified += sum(
            1 for f in files
            if f.get('validity') == 'verified' and f.get('sha256') != sha(data)
            and f.get('source') == 'bgc'
        )
    low, high = wilson(exact, N)
    overall_exact += exact
    overall_false += false_verified
    overall_wilson_low = min(overall_wilson_low or low, low)
    print('\n== gap = {}: exact {} / {} = {:.1f}%, Wilson95 ({}, {})'.format(
        kind, exact, N, 100 * exact / N, low, high))
    print('   false verified {}/{}; mean scan {:.3f}s'.format(false_verified, N, elapsed_total / N))

# Controls. A control failure is any false verified row or any stitch that
# changes an intact file's SHA-256.
control_failures = 0
name, data, clusters, split, gap, island = layout('random', rnd)
intact, _, _ = scan('control-intact', build_fat32(
    total_sectors=TOTAL_SECTORS, files=[('F.PNG', data,
        list(range(START_CLUSTER, START_CLUSTER + clusters)))], deleted=['F.PNG'])[0])
control_failures += sum(1 for f in intact if f.get('source') == 'bgc')
missing_runs = list(range(START_CLUSTER, START_CLUSTER + split))
missing_data = data[:len(missing_runs) * CL]
missing_image, _ = build_fat32(total_sectors=TOTAL_SECTORS, files=[('F.PNG', missing_data, missing_runs)], deleted=['F.PNG'])
missing, _, _ = scan('control-missing-tail', missing_image)
control_failures += sum(1 for f in missing if f.get('source') == 'bgc' and f.get('validity') == 'verified')

# A chunk header embedded in the gap: must not create a wrong verified join.
fake_chunk = struct.pack('>I', 4) + b'tEXt' + b'abcd' + struct.pack('>I', 0x12345678)
island = fake_chunk + rnd.randbytes(max(0, gap * CL - len(fake_chunk)))
header_gap = build_image(data, clusters, split, gap, island)
header_files, _, _ = scan('control-header-gap', header_gap)
control_failures += sum(
    1 for f in header_files
    if f.get('source') == 'bgc' and f.get('validity') == 'verified' and f.get('sha256') != sha(data)
)

# Live allocated island: unallocated mode must not read it; raw mode may.
live = build_image(data, clusters, split, gap, island, live=True)
live_unalloc, _, _ = scan('control-live-unallocated', live, 'unallocated')
control_failures += sum(
    1 for f in live_unalloc
    if f.get('source') == 'bgc' and f.get('validity') == 'verified'
    and f.get('allocated') is True
)
live_raw, _, _ = scan('control-live-raw', live, 'raw')

print('\ncontrols: failures={} (intact/missing/header-gap/live-unallocated)'.format(control_failures))
print('TOTAL exact {} / {} ; false verified {} ; controls {}'.format(
    overall_exact, 2 * N, overall_false, control_failures))
print('ACCEPTANCE={}'.format('PASS' if overall_exact >= 0.95 * 2 * N
      and overall_wilson_low >= 0.9 and overall_false == 0 and control_failures == 0 else 'FAIL'))
