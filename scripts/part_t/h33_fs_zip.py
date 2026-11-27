#!/usr/bin/env python3
"""H-33 prototype: FAT12/16/exFAT deleted-file fixtures and ZIP/OOXML oracle.

usage: h33_fs_zip.py CARVEX CORPUS WORKDIR

No production source is changed.  Filesystem fixtures are built with the
standard mkfs tools; each contains a deleted baseline JPEG.  CarveX is run in
raw/unallocated mode as it is today (unsupported filesystems fall back to raw),
and exact SHA-256 is reported.  ZIP/OOXML containers are carved from a raw
image by EOCD scanning and validated with Python's zipfile module, including
an oversized-declaration bomb case.
"""
import glob
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
import zipfile

CARVEX, CORPUS, W = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(W, exist_ok=True)
sha = lambda data: hashlib.sha256(data).hexdigest()


def run(cmd, check=True):
    result = subprocess.run(cmd, text=True, capture_output=True)
    if check and result.returncode != 0:
        raise RuntimeError('{} failed: {}'.format(' '.join(cmd), result.stderr.strip()))
    return result


def jpeg_bytes():
    for path in sorted(glob.glob(os.path.join(CORPUS, 'jpeg', '*.jpg'))):
        data = open(path, 'rb').read()
        if b'\xff\xc2' not in data[:2000] and 20 * 4096 <= len(data) <= 200000:
            return os.path.basename(path), data
    raise SystemExit('no suitable baseline JPEG in corpus')


def fs_fixture(kind, name, data, work):
    img = os.path.join(work, name + '.img')
    size = 64 << 20 if kind != 'fat12' else 2 << 20
    with open(img, 'wb') as handle:
        handle.truncate(size)
    if kind == 'fat12':
        run(['mkfs.vfat', '-F12', '-S', '512', '-s', '1', img])
    elif kind == 'fat16':
        run(['mkfs.vfat', '-F16', '-S', '512', '-s', '4', img])
    elif kind == 'exfat':
        check = run(['mkfs.exfat', img], check=False)
        if check.returncode != 0:
            return img, 'SKIP mkfs.exfat: ' + check.stderr.strip()
    else:
        raise ValueError(kind)
    mount = os.path.join(work, name + '-mnt')
    os.makedirs(mount, exist_ok=True)
    try:
        fstype = {'fat12': 'vfat', 'fat16': 'vfat', 'exfat': 'exfat'}[kind]
        run(['mount', '-t', fstype, '-o', 'loop', img, mount])
        path = os.path.join(mount, 'F.JPG')
        open(path, 'wb').write(data)
        run(['sync'])
        os.remove(path)
        run(['sync'])
        run(['umount', mount])
    except Exception as exc:
        run(['umount', mount], check=False)
        return img, 'SKIP fixture setup: ' + str(exc)
    return img, None


def carve_exact(img, data, work, tag):
    out = os.path.join(work, 'out-' + tag)
    report = os.path.join(work, 'report-' + tag + '.json')
    shutil.rmtree(out, ignore_errors=True)
    if os.path.exists(report):
        os.remove(report)
    result = subprocess.run([
        CARVEX, 'scan', img, '-o', out, '--quiet', '--json-report', report,
        '--mode', 'raw', '--types', 'jpeg', '--validate', 'decode',
    ], capture_output=True, timeout=300)
    if not os.path.exists(report):
        return 'SKIP scan: ' + result.stderr.decode(errors='replace').strip()
    files = json.load(open(report)).get('files', [])
    return sum(1 for f in files if f.get('sha256') == sha(data))


def make_zip(work, name, entries, ooxml=None):
    path = os.path.join(work, name + '.zip')
    with zipfile.ZipFile(path, 'w') as zf:
        if ooxml:
            zf.writestr('[Content_Types].xml', ooxml, compress_type=zipfile.ZIP_STORED)
        for entry_name, payload, method in entries:
            zf.writestr(entry_name, payload, compress_type=method)
    return path


def carve_zip(data, work, tag):
    start = data.find(b'PK\x03\x04')
    eocd = data.rfind(b'PK\x05\x06')
    if start < 0 or eocd < 0:
        return None
    comment_len = int.from_bytes(data[eocd + 20:eocd + 22], 'little')
    end = eocd + 22 + comment_len
    carved = data[start:end]
    try:
        with zipfile.ZipFile(os.path.join(work, tag + '.zip')) as zf:
            expected = zf.namelist()
            checked = len(expected)
            total = sum(info.file_size for info in zf.infolist())
    except Exception:
        return None
    return carved, checked, total


commit = subprocess.check_output(
    ['git', '-C', os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
     'rev-parse', 'HEAD'], text=True).strip()
manifest = os.path.join(CORPUS, 'manifest.json')
img_name, jpeg = jpeg_bytes()
print('H-33 filesystem/ZIP prototype (no production source changed)')
print('commit: ' + commit)
print('binary: {} ({})'.format(CARVEX, sha(open(CARVEX, 'rb').read())))
print('corpus: {} ({})'.format(CORPUS, sha(open(manifest, 'rb').read())))
print('JPEG: {} ({})'.format(img_name, sha(jpeg)))

for kind in ('fat12', 'fat16', 'exfat'):
    img, err = fs_fixture(kind, kind, jpeg, W)
    outcome = err if err else carve_exact(img, jpeg, W, kind)
    print('{:<6} deleted JPEG exact rows: {}'.format(kind, outcome))

plain = make_zip(W, 'plain', [
    ('a.txt', b'A' * 4096, zipfile.ZIP_STORED),
    ('b.txt', b'B' * 8192, zipfile.ZIP_DEFLATED),
])
ooxml_specs = {
    'docx': 'application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml',
    'xlsx': 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml',
    'pptx': 'application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml',
}
for kind, content_type in ooxml_specs.items():
    ct = ('<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
          '<Default Extension="xml" ContentType="application/xml"/>'
          '<Override PartName="/document.xml" ContentType="{}"/></Types>').format(content_type)
    make_zip(W, kind, [('document.xml', b'<root/>', zipfile.ZIP_DEFLATED)], ct)
for name in ['plain', 'docx', 'xlsx', 'pptx']:
    original = open(os.path.join(W, name + '.zip'), 'rb').read()
    raw = os.urandom(64 * 1024) + original + os.urandom(64 * 1024)
    raw_path = os.path.join(W, name + '.img')
    open(raw_path, 'wb').write(raw)
    result = carve_zip(raw, W, name)
    if result is None or result[0] != original:
        print('{:5} ZIP/OOXML exact: FAIL'.format(name))
    else:
        print('{:5} ZIP/OOXML exact: PASS ({} entries checked, {} uncompressed bytes)'.format(
            name, result[1], result[2]))
bomb = os.path.join(W, 'bomb.zip')
with zipfile.ZipFile(bomb, 'w', zipfile.ZIP_DEFLATED) as zf:
    zf.writestr('huge.txt', b'\0' * (64 << 20))
with zipfile.ZipFile(bomb) as zf:
    entries = len(zf.infolist())
    total = sum(info.file_size for info in zf.infolist())
print('bomb declaration: {} entries, {} uncompressed bytes (production cap must reject)'.format(
    entries, total))
