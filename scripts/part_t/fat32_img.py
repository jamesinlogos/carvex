"""Minimal FAT32 image builder for recovery experiments (stdlib only).

build_fat32(files, deleted) -> (image_bytes, info)
  files   : [(name_8_3, data, [cluster numbers])]  clusters are placed exactly as given (fragmentation on purpose)
  deleted : names whose directory entry is marked 0xE5 and whose FAT chain is zeroed (data bytes stay)
info: cs (bytes per cluster), clusters, fat (list of FAT entries), coff(n) (byte offset of cluster n)
Layout: 512-byte sectors, 8 sectors per cluster (4 KiB), 32 reserved sectors, 2 FATs, root directory = cluster 2."""
import struct


def build_fat32(total_sectors=65536, spc=8, files=(), deleted=()):
    bps, res, nf = 512, 32, 2
    fat_sectors = (total_sectors * 4 + 1024) // 512 + 1
    data_start = res + nf * fat_sectors
    clusters = (total_sectors - data_start) // spc
    img = bytearray(total_sectors * bps)
    b = bytearray(512)
    b[0:3], b[3:11] = b'\xEB\x3C\x90', b'MSWIN4.1'
    struct.pack_into('<HBHBHHBHHHII', b, 11, bps, spc, res, nf, 0, 0, 0xF8, 0, 0, 0, 0, total_sectors)
    struct.pack_into('<I', b, 0x24, fat_sectors)
    struct.pack_into('<I', b, 0x2C, 2)
    b[0x47:0x52], b[0x52:0x5A], b[510:512] = b'CARVEX     ', b'FAT32   ', b'\x55\xAA'
    img[0:512] = b
    fat = [0] * (clusters + 2)
    fat[0], fat[1], fat[2] = 0x0FFFFFF8, 0x0FFFFFFF, 0x0FFFFFFF
    cs = spc * bps

    def coff(c):
        return data_start * bps + (c - 2) * cs

    root, info = bytearray(cs), {}
    for i, (name, data, cl) in enumerate(files):
        assert (len(cl) - 1) * cs < len(data) <= len(cl) * cs, (name, len(data), len(cl))
        for k, c in enumerate(cl):
            piece = data[k * cs:(k + 1) * cs]
            img[coff(c):coff(c) + len(piece)] = piece
            fat[c] = cl[k + 1] if k + 1 < len(cl) else 0x0FFFFFFF
        e = bytearray(32)
        stem, _, ext = name.partition('.')
        e[0:11] = (stem.upper().ljust(8) + ext.upper().ljust(3)).encode()
        e[11] = 0x20
        struct.pack_into('<H', e, 20, cl[0] >> 16)
        struct.pack_into('<H', e, 26, cl[0] & 0xFFFF)
        struct.pack_into('<I', e, 28, len(data))
        if name in deleted:
            e[0] = 0xE5
            for c in cl:
                fat[c] = 0
        root[i * 32:i * 32 + 32] = e
        info[name] = (cl[0], len(data), len(cl))
    img[coff(2):coff(2) + cs] = root
    for k in range(nf):
        o = res * bps + k * fat_sectors * bps
        for i, v in enumerate(fat):
            struct.pack_into('<I', img, o + 4 * i, v)
    return bytes(img), dict(cs=cs, clusters=clusters, fat=fat, coff=coff, files=info)


def free_clusters(info):
    """Cluster numbers whose FAT entry is 0, ascending (what `unallocated` mode may read)."""
    return [n for n in range(3, info['clusters'] + 2) if info['fat'][n] == 0]


def free_stream(img, info):
    """The free clusters concatenated in ascending order: the 'free-space stream' of harness item H-27."""
    cs = info['cs']
    return b''.join(img[info['coff'](n):info['coff'](n) + cs] for n in free_clusters(info))
