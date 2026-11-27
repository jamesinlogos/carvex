#!/usr/bin/env python3
"""H-28/H-29 production acceptance: opt-in deleted-metadata recovery.

usage: h28_h29_metadata.py CARVEX CORPUS WORKDIR [N=100]

Builds synthetic FAT32 fixtures with deleted JPEG/PNG/PDF files and NTFS
fixtures with a deleted JPEG, runs `scan --metadata`, and reports exact SHA-256,
exact recovered size, n and Wilson 95% intervals.  The NTFS fixtures are made
with mkntfs/ntfs-3g when available and are otherwise reported as SKIP (never a
silent pass).  No default run is used to claim recovery.
"""
import glob
import hashlib
import json
import math
import os
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fat32_img import build_fat32

CARVEX, CORPUS, W = sys.argv[1], sys.argv[2], sys.argv[3]
N = int(sys.argv[4]) if len(sys.argv) > 4 else 100
NTFS_N = max(10, min(20, N // 10))
START_CLUSTER = 100
TOTAL_SECTORS = 8192
SEED = 28292026
os.makedirs(W, exist_ok=True)
sha = lambda data: hashlib.sha256(data).hexdigest()


def wilson(k, n, z=1.96):
    p = k / n
    c = (p + z * z / (2 * n)) / (1 + z * z / n)
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return round(max(0.0, c - h), 3), round(min(1.0, c + h), 3)


def run(cmd, check=True):
    result = subprocess.run(cmd, text=True, capture_output=True)
    if check and result.returncode != 0:
        raise RuntimeError('{} failed: {}'.format(' '.join(cmd), result.stderr.strip()))
    return result


def corpus_files():
    out = []
    for kind, pattern in (('jpeg', 'jpeg/*.jpg'), ('png', 'png/*.png'), ('pdf', 'pdf/*.pdf')):
        for path in sorted(glob.glob(os.path.join(CORPUS, pattern))):
            data = open(path, 'rb').read()
            if 4096 <= len(data) <= 128 * 4096:
                out.append((kind, os.path.basename(path), data))
    if len(out) < 3:
        raise SystemExit('need at least three corpus files of 4 KiB..512 KiB')
    return out


def scan(tag, image, types, mode='unallocated'):
    image_path = os.path.join(W, tag + '.img')
    open(image_path, 'wb').write(image)
    out = os.path.join(W, 'out-' + tag)
    report = os.path.join(W, 'report-' + tag + '.json')
    shutil.rmtree(out, ignore_errors=True)
    if os.path.exists(report):
        os.remove(report)
    started = time.monotonic()
    result = subprocess.run([
        CARVEX, 'scan', image_path, '-o', out, '--quiet', '--json-report', report,
        '--mode', mode, '--types', types, '--validate', 'decode', '--metadata',
    ], capture_output=True, timeout=300)
    if not os.path.exists(report):
        raise RuntimeError('{}: no report: {}'.format(tag, result.stderr.decode(errors='replace')))
    return json.load(open(report)).get('files', []), time.monotonic() - started


def accepted(files, data):
    want = sha(data)
    return any(
        row.get('sha256') == want
        and row.get('size') == len(data)
        and row.get('carved_size') == len(data)
        and row.get('contiguous_assumed') is True
        for row in files
    )


files = corpus_files()
commit = run(['git', '-C', os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
              'rev-parse', 'HEAD']).stdout.strip()
manifest = os.path.join(CORPUS, 'manifest.json')
print('H-28/H-29 production --metadata acceptance')
print('commit: ' + commit)
print('binary: {} ({})'.format(CARVEX, sha(open(CARVEX, 'rb').read())))
print('corpus: {} ({})'.format(CORPUS, sha(open(manifest, 'rb').read())))
print('seed: {}, cluster: {}, N={} FAT trials, N={} NTFS trials'.format(SEED, 4096, N, NTFS_N))
print('corpus files used ({}): {}'.format(len(files), ', '.join(name for _, name, _ in files)))

import random
rnd = random.Random(SEED)
fat_exact = 0
per_kind = {kind: [0, 0] for kind in ('jpeg', 'png', 'pdf')}
started = time.monotonic()
for i in range(N):
    kind, name, data = rnd.choice(files)
    per_kind[kind][1] += 1
    clusters = -(-len(data) // 4096)
    runs = list(range(START_CLUSTER, START_CLUSTER + clusters))
    image, _ = build_fat32(total_sectors=TOTAL_SECTORS,
                           files=[('F.' + kind[:3].upper(), data, runs)],
                           deleted=['F.' + kind[:3].upper()])
    rows, _ = scan('fat-{}'.format(i), image, 'jpeg,png,pdf')
    if accepted(rows, data):
        fat_exact += 1
        per_kind[kind][0] += 1
    shutil.rmtree(os.path.join(W, 'out-fat-{}'.format(i)), ignore_errors=True)
    for suffix in ('.img',):
        path = os.path.join(W, 'fat-{}{}'.format(i, suffix))
        if os.path.exists(path):
            os.remove(path)
low, high = wilson(fat_exact, N)
print('\n== H-28 FAT32 deleted files: exact {}/{} = {:.1f}%, Wilson95 ({}, {})'.format(
    fat_exact, N, 100 * fat_exact / N, low, high))
print('   by family: ' + ', '.join('{} {}/{}'.format(kind, v[0], v[1])
      for kind, v in per_kind.items()))
print('   {:.1f}s total'.format(time.monotonic() - started))

if shutil.which('mkntfs') is None or shutil.which('ntfs-3g') is None or os.geteuid() != 0:
    print('\n== H-29 NTFS deleted file: SKIP (mkntfs/ntfs-3g/root unavailable)')
    ntfs_exact = ntfs_low = ntfs_high = None
else:
    ntfs_exact = 0
    started = time.monotonic()
    for i in range(NTFS_N):
        kind, name, data = rnd.choice([f for f in files if f[0] == 'jpeg'])
        img = os.path.join(W, 'ntfs-{}.img'.format(i))
        with open(img, 'wb') as handle:
            handle.truncate(64 << 20)
        run(['mkntfs', '-F', '-f', '-q', img, '131072'])
        mount = os.path.join(W, 'ntfs-{}-mnt'.format(i))
        os.makedirs(mount, exist_ok=True)
        try:
            run(['mount', '-t', 'ntfs-3g', '-o', 'loop,noatime', img, mount])
            target = os.path.join(mount, 'TARGET.JPG')
            open(target, 'wb').write(data)
            blockers = []
            for j in range(6):
                block = os.path.join(mount, 'BLOCK{}.BIN'.format(j))
                open(block, 'wb').write(os.urandom(256 * 1024))
                blockers.append(block)
            run(['sync'])
            for block in blockers[::2]:
                os.remove(block)
            run(['sync'])
            os.remove(target)
            run(['sync'])
            run(['umount', '-l', mount])
        except Exception as exc:
            run(['umount', '-l', mount], check=False)
            print('\n== H-29 NTFS deleted file: SKIP fixture setup: {}'.format(exc))
            ntfs_exact = None
            break
        image = open(img, 'rb').read()
        rows, _ = scan('ntfs-{}'.format(i), image, 'jpeg')
        ntfs_exact += int(any(
            row.get('sha256') == sha(data) and row.get('size') == len(data)
            for row in rows
        ))
        shutil.rmtree(os.path.join(W, 'out-ntfs-{}'.format(i)), ignore_errors=True)
        shutil.rmtree(mount, ignore_errors=True)
        os.remove(img)
    if ntfs_exact is not None:
        ntfs_low, ntfs_high = wilson(ntfs_exact, NTFS_N)
        print('\n== H-29 NTFS deleted files: exact {}/{} = {:.1f}%, Wilson95 ({}, {})'.format(
            ntfs_exact, NTFS_N, 100 * ntfs_exact / NTFS_N, ntfs_low, ntfs_high))
        print('   {:.1f}s total'.format(time.monotonic() - started))

print('\nNo default run is counted; metadata fields are additive and omitted when --metadata is absent.')
print('ACCEPTANCE={}'.format('PASS' if fat_exact >= 0.95 * N and low >= 0.85
      and ntfs_exact is not None and ntfs_exact == NTFS_N else 'SEE-EVIDENCE'))
