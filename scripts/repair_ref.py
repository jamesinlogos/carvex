#!/usr/bin/env python3
"""Reference algorithm for H-04 (validated PNG/JPEG repair): "crop to the recovered rows".
Python, standard library + Pillow (oracle only).  The Rust port must produce the same rows.

png_repair(cut)  -> (bytes|None, info)   non-interlaced PNG: inflate the readable IDAT prefix, keep the
                                         complete scanlines, rewrite IHDR height, re-deflate, add IEND.
jpeg_repair(cut) -> (bytes|None, info)   baseline sequential JPEG without restart intervals: decode
                                         the entropy data MCU by MCU, keep complete MCU rows, rewrite
                                         the SOF height, pad the last byte with 1-bits, append EOI.
Every pixel of the output is a pixel of the original; nothing is invented.  Everything else
(progressive JPEG, restart intervals, interlaced PNG; 16-bit samples are fine) returns None -> keep `partial`.
INPUT CONTRACT: the exact carved bytes up to the true end of the data (after the tail trim of H-25). Padding
left after the cut is decoded as if it were data and invents rows (4 KiB of zeros: PNG 361 instead of 359 rows,
JPEG 19 instead of 17 MCU rows) -- the oracle comparison with the original catches it.

Usage: repair_ref.py CORPUS_DIR          # runs 30/60/90 % cuts of png_02 and jpeg_01, checks with Pillow
Exit code 0 when every cut loads with Pillow verify()+load() and its top rows equal the original's."""
import io, struct, sys, zlib


def _chunk(t, d):
    return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xFFFFFFFF)


def png_repair(cut):
    if cut[:8] != b'\x89PNG\r\n\x1a\n':
        return None, 'not a PNG'
    pos, pre, idat, ihdr = 8, [], b'', None
    while pos + 8 <= len(cut):
        ln, = struct.unpack('>I', cut[pos:pos + 4])
        typ, body = cut[pos + 4:pos + 8], cut[pos + 8:pos + 8 + ln]      # body is short for the cut chunk
        if typ == b'IHDR':
            ihdr = body
        elif typ == b'IDAT':
            idat += body
        elif typ == b'IEND':
            break
        elif not idat:
            pre.append(cut[pos:pos + 12 + ln])                         # PLTE, tRNS, gAMA ... before IDAT
        pos += 12 + ln                                                  # a cut chunk ends the loop here
    if ihdr is None or len(ihdr) < 13:
        return None, 'no IHDR'
    w, h, bd, ct, cm, fm, il = struct.unpack('>IIBBBBB', ihdr[:13])
    if il != 0:
        return None, 'interlaced: not handled'
    rowbytes = 1 + (w * {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ct] * bd + 7) // 8
    try:
        raw = zlib.decompressobj().decompress(idat)                    # partial output is kept on a cut stream
    except zlib.error:
        raw = b''
    n = min(len(raw) // rowbytes, h)
    if n == 0:
        return None, 'no complete row'
    out = (b'\x89PNG\r\n\x1a\n' + _chunk(b'IHDR', struct.pack('>IIBBBBB', w, n, bd, ct, cm, fm, il))
           + b''.join(pre) + _chunk(b'IDAT', zlib.compress(raw[:n * rowbytes], 6)) + _chunk(b'IEND', b''))
    return out, {'rows': n, 'of': h}


def jpeg_repair(cut):
    pos, dht, sof, sos = 2, {}, None, None
    while pos + 4 <= len(cut) and cut[pos] == 0xFF:
        m, ln = cut[pos + 1], struct.unpack('>H', cut[pos + 2:pos + 4])[0]
        body = cut[pos + 4:pos + 2 + ln]
        if m == 0xC0:
            sof = (pos, body)
        elif m in (0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF):
            return None, 'not baseline sequential: not handled'
        elif m == 0xDD:
            return None, 'restart intervals: not handled'
        elif m == 0xC4:
            p = 0
            while p < len(body):
                tc_th, cnt = body[p], list(body[p + 1:p + 17])
                syms, code, k, tab = list(body[p + 17:p + 17 + sum(cnt)]), 0, 0, {}
                for length in range(1, 17):
                    for _ in range(cnt[length - 1]):
                        tab[(length, code)] = syms[k]; code += 1; k += 1
                    code <<= 1
                dht[tc_th] = tab
                p += 17 + sum(cnt)
        elif m == 0xDA:
            sos = body; pos += 2 + ln
            break
        pos += 2 + ln
    if sof is None or sos is None:
        return None, 'no SOF/SOS in the readable prefix'
    _, Y, X, nc = struct.unpack('>BHHB', sof[1][:6])
    comps = {sof[1][6 + 3 * i]: (sof[1][7 + 3 * i] >> 4, sof[1][7 + 3 * i] & 15) for i in range(nc)}
    scan = [(sos[1 + 2 * i], sos[2 + 2 * i] >> 4, sos[2 + 2 * i] & 15) for i in range(sos[0])]
    if len(scan) != nc:
        return None, 'non-interleaved scan: not handled'
    vmax, hmax = max(v for _, v in comps.values()), max(h for h, _ in comps.values())
    mcux, mcuy = -(-X // (8 * hmax)), -(-Y // (8 * vmax))
    data = cut[pos:]
    bits, owner, raw_end, i = [], [], [], 0                             # un-stuffed bits, byte owning each bit
    while i < len(data):
        b = data[i]
        if b == 0xFF:
            if i + 1 < len(data) and data[i + 1] == 0:
                nxt = i + 2
            else:
                break                                                   # a marker: the entropy data ends here
        else:
            nxt = i + 1
        for k in range(7, -1, -1):
            bits.append((b >> k) & 1); owner.append(len(raw_end))
        raw_end.append(nxt); i = nxt
    bp = 0

    def huff(tab):
        nonlocal bp
        code = 0
        for length in range(1, 17):
            if bp >= len(bits):
                raise EOFError
            code = (code << 1) | bits[bp]; bp += 1
            if (length, code) in tab:
                return tab[(length, code)]
        raise ValueError

    def recv(n):
        nonlocal bp
        if bp + n > len(bits):
            raise EOFError
        bp += n

    rows, endbit = 0, 0
    try:
        for _ in range(mcuy):
            for _ in range(mcux):
                for cid, td, ta in scan:
                    h, v = comps[cid]
                    for _ in range(h * v):
                        s = huff(dht[td])                               # DC table (class 0)
                        if s:
                            recv(s)
                        k = 1
                        while k < 64:
                            rs = huff(dht[16 + ta])                     # AC table (class 1)
                            if rs & 15 == 0:
                                if rs >> 4 == 15:
                                    k += 16; continue
                                break
                            recv(rs & 15); k += (rs >> 4) + 1
            rows += 1; endbit = bp
    except (EOFError, ValueError, KeyError):
        pass
    if rows == 0:
        return None, 'no complete MCU row'
    last = owner[endbit - 1]                                            # index of the last byte that holds data bits
    used = endbit - owner.index(last)                                   # 1..8 bits of that byte are data
    ent = bytearray(data[:raw_end[last]])
    if used < 8 and not ent.endswith(b'\xff\x00'):
        ent[-1] |= (1 << (8 - used)) - 1                                # pad with 1-bits (T.81 F.1.2.3)
        if ent[-1] == 0xFF:
            ent.append(0)
    hdr = bytearray(cut[:pos])
    height = min(rows * 8 * vmax, Y)
    hdr[sof[0] + 5:sof[0] + 7] = struct.pack('>H', height)
    return bytes(hdr) + bytes(ent) + b'\xff\xd9', {'mcu_rows': rows, 'of': mcuy, 'height': height, 'of_height': Y}


def check(out, orig, slack):
    from PIL import Image
    try:
        Image.open(io.BytesIO(out)).verify()
        im = Image.open(io.BytesIO(out)); im.load()
    except Exception as e:                                              # noqa: BLE001
        return False, 'FAIL ' + repr(e)[:80]
    a, o = im.convert('RGB'), Image.open(io.BytesIO(orig)).convert('RGB')
    k = a.size[1] - slack                                               # JPEG: last rows may differ (chroma edge)
    same = a.crop((0, 0, a.size[0], k)).tobytes() == o.crop((0, 0, o.size[0], k)).tobytes()
    return same, 'OK %dx%d, top %d rows identical to the original: %s' % (a.size[0], a.size[1], k, same)


def main():
    corpus, bad = sys.argv[1], 0
    png = open(corpus + '/png/png_02_rgb_800x600.png', 'rb').read()
    jpg = open(corpus + '/jpeg/jpeg_01_baseline_640x480.jpg', 'rb').read()
    for name, fn, src, slack in (('PNG', png_repair, png, 0), ('JPEG', jpeg_repair, jpg, 2)):
        for pct in (30, 60, 90):
            out, info = fn(src[:len(src) * pct // 100])
            ok, msg = check(out, src, slack) if out else (False, 'no repair: %s' % info)
            bad += not ok
            print('%-4s %2d%%  %s  %s' % (name, pct, info, msg))
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
