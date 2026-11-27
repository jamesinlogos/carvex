#!/usr/bin/env python3
"""H-27 acceptance measurement for the production --free-stream flag.

usage: h27_free_stream.py CARVEX CORPUS WORKDIR [N=40]

For each JPEG/PNG family and each of N synthetic FAT32 trials, one deleted file
is split around a live allocated island.  The normal scan and the opt-in
free-stream scan are compared against the original SHA-256.  A second N-trial
series puts the interior of another deleted file in the gap; no wrong file may
be marked verified.  Output includes Wilson 95% intervals and the measured
extra cost of the second pass.
"""
import glob
import hashlib
import json
import math
import os
import random
import shutil
import statistics
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fat32_img import build_fat32

CARVEX, CORPUS, W = sys.argv[1], sys.argv[2], sys.argv[3]
N = int(sys.argv[4]) if len(sys.argv) > 4 else 40
CL = 4096
TOTAL_SECTORS = 8192
START_CLUSTER = 100
os.makedirs(W, exist_ok=True)
sha = lambda data: hashlib.sha256(data).hexdigest()


def wilson(k, n, z=1.96):
    p = k / n
    c = (p + z * z / (2 * n)) / (1 + z * z / n)
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return max(0.0, c - h), min(1.0, c + h)


def load_family(folder, ext):
    out = []
    for path in sorted(glob.glob(os.path.join(CORPUS, folder, '*' + ext))):
        name = os.path.basename(path)
        if any(word in name for word in ('interlaced', 'progressive', 'cmyk')):
            continue
        data = open(path, 'rb').read()
        if len(data) >= 12 * CL:
            out.append((name, data))
    return out


def build_trial(family, kind, files_by_family):
    name, data = random.choice(files_by_family[family])
    clusters = -(-len(data) // CL)
    break_at = random.randint(2, clusters - 2)
    gap = random.randint(1, 8)
    if kind == 'live':
        island = random.randbytes(gap * CL)
    else:
        other_name, other = random.choice(
            [entry for entry in files_by_family[family] if entry[0] != name]
        )
        start = random.randint(2, max(2, -(-len(other) // CL) - gap - 2))
        island = other[start * CL:(start + gap) * CL]
        island += random.randbytes(gap * CL - len(island))
    runs = (
        list(range(START_CLUSTER, START_CLUSTER + break_at))
        + list(range(START_CLUSTER + break_at + gap, START_CLUSTER + break_at + gap + clusters - break_at))
    )
    files = [('F.BIN', data, runs)]
    deleted = ['F.BIN']
    if kind == 'live':
        files.append(('LIVE.DAT', island, list(range(START_CLUSTER + break_at, START_CLUSTER + break_at + gap))))
    image, info = build_fat32(total_sectors=TOTAL_SECTORS, files=files, deleted=deleted)
    if kind == 'dead':
        mutable = bytearray(image)
        for index in range(gap):
            offset = info['coff'](START_CLUSTER + break_at + index)
            mutable[offset:offset + CL] = island[index * CL:(index + 1) * CL]
        image = bytes(mutable)
    return name, data, image


def scan(image_path, tag, family, free_stream):
    out = os.path.join(W, 'out_' + tag)
    report_path = os.path.join(W, 'report_' + tag + '.json')
    shutil.rmtree(out, ignore_errors=True)
    if os.path.exists(report_path):
        os.remove(report_path)
    command = [
        CARVEX, 'scan', image_path, '-o', out, '--quiet',
        '--json-report', report_path, '--mode', 'unallocated',
        '--types', family, '--validate', 'decode', '--min-size', '1',
    ]
    if free_stream:
        command.append('--free-stream')
    started = time.monotonic()
    result = subprocess.run(command, capture_output=True, timeout=300)
    elapsed = time.monotonic() - started
    if not os.path.exists(report_path):
        raise RuntimeError('no report; exit={} stderr={}'.format(result.returncode, result.stderr.decode(errors='replace')))
    report = json.load(open(report_path))
    return report.get('files', []), elapsed


files_by_family = {
    'jpeg': load_family('jpeg', '.jpg'),
    'png': load_family('png', '.png'),
}
for family, entries in files_by_family.items():
    if len(entries) < 2:
        raise SystemExit('need at least two corpus files for ' + family)

random.seed(2026)
commit = subprocess.check_output(
    ['git', '-C', os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), 'rev-parse', 'HEAD'],
    text=True,
).strip()
manifest = os.path.join(CORPUS, 'manifest.json')

print('H-27 production --free-stream acceptance')
print('commit: ' + commit)
print('binary: {} ({})'.format(CARVEX, sha(open(CARVEX, 'rb').read())))
print('corpus: {} ({})'.format(CORPUS, sha(open(manifest, 'rb').read())))
print('workdir: {}'.format(W))
print('synthetic FAT32 images: {} sectors, {} clusters, seed 2026, gap 1..8 clusters'.format(
    TOTAL_SECTORS, (TOTAL_SECTORS - 292) // 8))
for family in ('jpeg', 'png'):
    print('{} corpus: {}'.format(family, ', '.join(name for name, _ in files_by_family[family])))

overall_exact = overall_live = 0
overall_false = overall_dead = 0
for family in ('jpeg', 'png'):
    exact = 0
    false_verified = 0
    default_times = []
    free_times = []
    for i in range(N):
        name, original, image = build_trial(family, 'live', files_by_family)
        path = os.path.join(W, '{}_live_{}.img'.format(family, i))
        open(path, 'wb').write(image)
        _, normal_elapsed = scan(path, '{}_live_{}_default'.format(family, i), family, False)
        live_files, free_elapsed = scan(path, '{}_live_{}_free'.format(family, i), family, True)
        default_times.append(normal_elapsed)
        free_times.append(free_elapsed)
        if any(
            entry.get('source') == 'free-stream'
            and entry.get('validity') == 'verified'
            and entry.get('sha256') == sha(original)
            for entry in live_files
        ):
            exact += 1
        name, original, image = build_trial(family, 'dead', files_by_family)
        path = os.path.join(W, '{}_dead_{}.img'.format(family, i))
        open(path, 'wb').write(image)
        dead_files, _ = scan(path, '{}_dead_{}_free'.format(family, i), family, True)
        false_verified += sum(
            1 for entry in dead_files
            if entry.get('validity') == 'verified' and entry.get('sha256') != sha(original)
        )
    low, high = wilson(exact, N)
    print('\n== {} live-island exactness: N={}'.format(family, N))
    print('   exact {} / {} = {:.1f}%  Wilson95 ({:.3f}, {:.3f})'.format(exact, N, 100 * exact / N, low, high))
    print('== {} dead-island false verified: {} / {} trials affected'.format(family, false_verified, N))
    print('== {} cost: default mean {:.3f}s, free-stream mean {:.3f}s, extra {:.3f}s ({:.1f}%)'.format(
        family,
        statistics.mean(default_times),
        statistics.mean(free_times),
        statistics.mean(free_times) - statistics.mean(default_times),
        100 * (statistics.mean(free_times) / max(statistics.mean(default_times), 1e-9) - 1),
    ))
    overall_exact += exact
    overall_live += N
    overall_false += false_verified
    overall_dead += N

low, high = wilson(overall_exact, overall_live)
print('\n== TOTAL live-island exactness: {} / {} = {:.1f}%  Wilson95 ({:.3f}, {:.3f})'.format(
    overall_exact, overall_live, 100 * overall_exact / overall_live, low, high))
print('== TOTAL dead-island false verified: {} / {} trials affected'.format(overall_false, overall_dead))
print('ACCEPTANCE={}'.format(
    'PASS' if overall_exact / overall_live >= 0.95 and low >= 0.85 and overall_false == 0 else 'FAIL'))
