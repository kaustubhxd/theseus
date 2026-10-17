#!/usr/bin/env python3
"""Add the terminal records SoundFont 2 requires (a terminal pgen/igen
record after the last bag's generators, terminal pmod/imod records) and
rewrite chunk sizes. Used after sbk2sf2."""
import struct, sys

def parse(d, off, end):
    out = []
    while off + 8 <= end:
        cid = d[off:off+4]; size = struct.unpack('<I', d[off+4:off+8])[0]
        if cid in (b'RIFF', b'LIST'):
            out.append([cid, d[off+8:off+12], parse(d, off+12, off+8+size)])
        else:
            out.append([cid, None, d[off+8:off+8+size]])
        off += 8 + size + (size & 1)
    return out

def build(items):
    b = b''
    for cid, typ, body in items:
        data = typ + build(body) if typ is not None else body
        b += cid + struct.pack('<I', len(data)) + data + (b'\0' if len(data) & 1 else b'')
    return b

def fix(path):
    d = open(path, 'rb').read()
    tree = parse(d, 0, len(d))
    pdta = next(x for x in tree[0][2] if x[1] == b'pdta')[2]
    leaf = {c[0]: c for c in pdta}
    for bag, gen, mod in ((b'pbag', b'pgen', b'pmod'), (b'ibag', b'igen', b'imod')):
        last_gen_ndx = struct.unpack('<H', leaf[bag][2][-4:-2])[0]
        n_gens = len(leaf[gen][2]) // 4
        if n_gens == last_gen_ndx:
            leaf[gen][2] += b'\0' * 4
        last_mod_ndx = struct.unpack('<H', leaf[bag][2][-2:])[0]
        if len(leaf[mod][2]) // 10 == last_mod_ndx:
            leaf[mod][2] += b'\0' * 10
    open(path, 'wb').write(build(tree))

for p in sys.argv[1:]:
    fix(p); print('fixed', p)
