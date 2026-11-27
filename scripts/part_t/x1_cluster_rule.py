#!/usr/bin/env python3
"""X1 — can a 4 KiB cluster be the middle of JPEG entropy data?  (harness item H-32)
Rule: every 0xFF byte must be followed by 0x00 or RSTn (D0..D7) (T.81 B.1.1.5 byte stuffing), AND order-0 entropy >= 7.0 bit/byte.
usage: x1_cluster_rule.py CORPUS"""
import collections, glob, math, os, random, struct, sys
C, CL = sys.argv[1], 4096


def stuffing_ok(buf, i):
    seg = buf[i:i + CL + 1]                       # peek one byte into the next cluster
    j = seg.find(b'\xff')
    while j != -1 and j < CL:
        if j + 1 < len(seg) and seg[j + 1] not in (0, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7):
            return False
        j = seg.find(b'\xff', j + 1)
    return True


def entropy(seg):
    c, n = collections.Counter(seg), len(seg)
    return -sum(v / n * math.log2(v / n) for v in c.values())


accept = lambda b, i: stuffing_ok(b, i) and entropy(b[i:i + CL]) >= 7.0


def scan_start(b):
    pos = 2
    while True:
        m, ln = b[pos + 1], struct.unpack('>H', b[pos + 2:pos + 4])[0]
        pos += 2 + ln
        if m == 0xDA:
            return pos


def rate(name, blobs, start=lambda b: 0, end=lambda b: len(b) - CL - 1):
    n = k = 0
    for b in blobs:
        for i in range(-(-start(b) // CL) * CL, end(b), CL):
            n += 1
            k += accept(b, i)
    print('%-44s accepted %4d/%-5d' % (name, k, n))


rd = lambda pat: [open(p, 'rb').read() for p in sorted(glob.glob(C + pat))]
base = [b for b in rd('/jpeg/*.jpg') if b'\xff\xc2' not in b[:2000]]
prog = [b for b in rd('/jpeg/*.jpg') if b'\xff\xc2' in b[:2000]]
rate('JPEG baseline entropy clusters', base, scan_start, lambda b: b.rindex(b'\xff\xd9') - CL)
rate('JPEG progressive entropy clusters', prog, scan_start, lambda b: b.rindex(b'\xff\xd9') - CL)
rate('PNG corpus clusters', rd('/png/*'))
for p in sorted(glob.glob(C + '/pdf/*')):
    b = open(p, 'rb').read()
    rate('PDF %-30s DCT=%s' % (os.path.basename(p)[:30], b'/DCTDecode' in b), [b])
random.seed(1)
rate('random bytes 8 MiB', [random.randbytes(8 << 20)])
rate('zeros 1 MiB', [bytes(1 << 20)])
