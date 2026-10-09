import struct, sys
CONT = {b'moov', b'trak', b'mdia', b'minf', b'stbl', b'sinf', b'schi', b'moof', b'traf', b'mvex', b'edts', b'dinf', b'udta'}
def walk(d, off, end, depth):
    while off + 8 <= end:
        size, typ = struct.unpack('>I4s', d[off:off+8]); hdr = 8
        if size == 1: size = struct.unpack('>Q', d[off+8:off+16])[0]; hdr = 16
        if size == 0: size = end - off
        extra = ''
        if typ == b'stsd':
            print('  '*depth + f'{typ.decode()} {size}'); walk(d, off+hdr+8, off+size, depth+1); off += size; continue
        if typ in (b'enca', b'mp4a'):
            print('  '*depth + f'{typ.decode()} {size}'); walk(d, off+hdr+28, off+size, depth+1); off += size; continue
        if typ == b'tenc':
            b = d[off+hdr:off+size]; extra = f'ver={b[0]} isProt={b[6]} ivsize={b[7]} kid={b[8:24].hex()}'
        if typ == b'schm':
            extra = d[off+hdr+4:off+hdr+8].decode()
        if typ == b'frma':
            extra = d[off+hdr:off+hdr+4].decode()
        if typ == b'esds':
            extra = d[off+hdr:off+size].hex()
        if typ == b'sidx':
            b = d[off+hdr:off+size]; ver = b[0]
            ts = struct.unpack('>I', b[8:12])[0]
            p = 12 + (8 if ver == 0 else 16)
            p += 2; cnt = struct.unpack('>H', b[p:p+2])[0]; p += 2
            refs = [struct.unpack('>III', b[p+12*i:p+12*i+12]) for i in range(cnt)]
            extra = f'ts={ts} refs={cnt} first={[ (r[0]&0x7fffffff, r[1]) for r in refs[:3]]}'
        print('  '*depth + f'{typ.decode(errors="replace")} {size} {extra}')
        if typ in CONT: walk(d, off+hdr, off+size, depth+1)
        off += size
d = open(sys.argv[1], 'rb').read()
walk(d, 0, len(d), 0)
