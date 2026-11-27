#!/usr/bin/env python3
"""H-31 prototype: PDF xref-guided bifragment join (design evidence only).

usage: h31_pdf_xref.py CARVEX CORPUS WORKDIR [N=100]

This script does NOT change production source.  It builds synthetic FAT32-like
two-fragment PDF layouts from the frozen corpus, uses surviving classic xref
offsets as a candidate oracle, and verifies candidates with qpdf --check plus
SHA-256 equality.  Two gap kinds are measured: seeded random bytes and the
interior of another corpus PDF.  Results include n, Wilson 95% intervals,
false-accept counts, commit, corpus digest, seed and tool versions.
"""
import glob
import hashlib
import json
import math
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

CARVEX, CORPUS, W = sys.argv[1], sys.argv[2], sys.argv[3]
N = int(sys.argv[4]) if len(sys.argv) > 4 else 100
CL = 4096
MAX_GAP = 8
SEED = 31012026
os.makedirs(W, exist_ok=True)
sha = lambda data: hashlib.sha256(data).hexdigest()


def wilson(k, n, z=1.96):
    p = k / n
    c = (p + z * z / (2 * n)) / (1 + z * z / n)
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return round(max(0.0, c - h), 3), round(min(1.0, c + h), 3)


def xref_entries(data):
    """Return (startxref offset, [(num, gen, byte_offset), ...]) for classic tables."""
    starts = list(re.finditer(rb'startxref\s+(\d+)\s+%%EOF', data))
    if not starts:
        return None, []
    offset = int(starts[-1].group(1))
    if data[offset:offset + 4] != b'xref':
        return None, []
    entries = []
    pos = offset + 4
    while pos < len(data):
        line_end = data.find(b'\n', pos)
        if line_end < 0:
            return None, []
        line = data[pos:line_end].strip()
        pos = line_end + 1
        if not line:
            continue
        if line == b'trailer':
            break
        parts = line.split()
        if len(parts) != 2 or not all(p.isdigit() for p in parts):
            return None, []
        first, count = int(parts[0]), int(parts[1])
        for num in range(first, first + count):
            eol = data.find(b'\n', pos)
            if eol < 0:
                return None, []
            row = data[pos:eol].rstrip(b'\r')
            pos = eol + 1
            if len(row) < 18:
                return None, []
            try:
                obj_off = int(row[:10])
                gen = int(row[11:16])
            except ValueError:
                return None, []
            if row[17:18] == b'n':
                entries.append((num, gen, obj_off))
        if not entries:
            return None, []
    return offset, entries


def header_ok(data, entries):
    for num, gen, off in entries:
        want = '{} {} obj'.format(num, gen).encode()
        if off + len(want) > len(data) or data[off:off + len(want)] != want:
            return False
    return True


def qpdf_ok(path):
    result = subprocess.run(
        ['qpdf', '--check', path], capture_output=True, timeout=30
    )
    return result.returncode == 0 and b'no syntax or stream encoding errors' in (result.stdout + result.stderr).lower()


def solve(disk, entries, work, trial):
    """Return (unique, [(k, g, sha256)], candidate_count)."""
    # The first xref offset whose object header is absent from the disk head
    # bounds the break; enumerate cluster boundaries before it. Header checks
    # are cheap, so only survivors are sent to qpdf.
    first_bad = None
    for num, gen, off in entries:
        want = '{} {} obj'.format(num, gen).encode()
        if off + len(want) > len(disk) or disk[off:off + len(want)] != want:
            first_bad = off
            break
    if first_bad is None:
        return False, [], 0
    accepted = []
    tried = 0
    max_k = min(first_bad // CL, len(disk) // CL - 1)
    for k in range(1, max_k + 1):
        for g in range(1, MAX_GAP + 1):
            tail = (k + g) * CL
            if tail >= len(disk):
                continue
            tried += 1
            stitched = disk[:k * CL] + disk[tail:]
            if not header_ok(stitched, entries):
                continue
            path = os.path.join(work, 'candidate_{}_{}_{}.pdf'.format(trial, k, g))
            open(path, 'wb').write(stitched)
            if qpdf_ok(path):
                accepted.append((k, g, sha(stitched)))
            os.remove(path)
            if len(accepted) > 1:
                return False, accepted, tried
    return len(accepted) == 1, accepted, tried


pdfs = []
for path in sorted(glob.glob(os.path.join(CORPUS, 'pdf', '*.pdf'))):
    data = open(path, 'rb').read()
    start, entries = xref_entries(data)
    if start is not None and len(entries) >= 5 and len(data) >= 20 * CL:
        pdfs.append((os.path.basename(path), data, start, entries))
if len(pdfs) < 2:
    raise SystemExit('need at least two classic-xref PDFs of at least 20 clusters')

commit = subprocess.check_output(
    ['git', '-C', os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
     'rev-parse', 'HEAD'], text=True).strip()
manifest = os.path.join(CORPUS, 'manifest.json')
print('H-31 PDF xref prototype (design evidence; no production source changed)')
print('commit: ' + commit)
print('binary (not used by prototype): {} ({})'.format(CARVEX, sha(open(CARVEX, 'rb').read())))
print('corpus: {} ({})'.format(CORPUS, sha(open(manifest, 'rb').read())))
print('qpdf: ' + subprocess.check_output(['qpdf', '--version'], text=True).splitlines()[0])
print('seed: {}, cluster: {}, max gap: {} clusters, N={} per gap kind'.format(SEED, CL, MAX_GAP, N))
print('PDFs: ' + ', '.join(name for name, _, _, _ in pdfs))

rnd = random.Random(SEED)
for kind in ('random', 'other PDF interior'):
    exact = false_accept = ambiguous = 0
    candidates_total = 0
    started = time.monotonic()
    for i in range(N):
        name, data, start_off, entries = rnd.choice(pdfs)
        clusters = -(-len(data) // CL)
        # Break before the xref table but after the first object.
        max_split = min(start_off // CL - 1, clusters - 3)
        split = rnd.randint(2, max(2, max_split))
        gap = rnd.randint(1, MAX_GAP)
        if kind == 'random':
            island = rnd.randbytes(gap * CL)
        else:
            other = rnd.choice([p for p in pdfs if p[0] != name])
            pos = rnd.randint(1, max(1, -(-len(other[1]) // CL) - gap - 1))
            island = other[1][pos * CL:(pos + gap) * CL]
            island += rnd.randbytes(gap * CL - len(island))
        disk = data[:split * CL] + island + data[split * CL:]
        unique, accepted, tried = solve(disk, entries, W, i)
        candidates_total += tried
        if unique and accepted[0][2] == sha(data):
            exact += 1
        elif accepted:
            false_accept += 1
        elif len(accepted) > 1:
            ambiguous += 1
    low, high = wilson(exact, N)
    print('\n== gap = {}: unique+correct {}/{} = {:.1f}%, Wilson95 ({}, {})'.format(
        kind, exact, N, 100 * exact / N, low, high))
    print('   false accepted {}/{}; ambiguous {}/{}; mean candidates {:.1f}; {:.1f}s total'.format(
        false_accept, N, ambiguous, N, candidates_total / N, time.monotonic() - started))
print('\nACCEPTANCE={}'.format('PASS' if False else 'SEE-DESIGN-REVIEW'))
