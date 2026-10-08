#!/usr/bin/env python3
"""Synthetic Xenos texture chunks + the Python decoder's RGBA output, for the Rust texture parity test.
usage: make_tex_reference.py <kkpc dir> <out dir>"""
import sys, os, json, struct, types
import numpy as np
K = sys.argv[1]; OUT = sys.argv[2]
sys.modules['lzallright'] = types.SimpleNamespace(LZOCompressor=lambda: None)   # only needed for LZO, which these chunks bypass
sys.path.insert(0, K + '/tools')
import tex_decode as T
rng = np.random.default_rng(12345)
D3D = {'DXT1': 0x1a200152, 'DXT3': 0x1a200153, 'DXT5': 0x1a200154, 'DXN': 0x1a200171, 'A8R8G8B8': 0x18280186, 'L8': 0x04900102}
BPB = {'DXT1': 8, 'DXT3': 16, 'DXT5': 16, 'DXN': 16}
cases = [('DXT1', 64, 64), ('DXT1', 200, 100), ('DXT5', 256, 128), ('DXT5', 100, 60), ('DXT3', 64, 32), ('DXN', 128, 128), ('DXN', 64, 40),
         ('A8R8G8B8', 64, 64), ('A8R8G8B8', 48, 40), ('L8', 128, 128), ('L8', 100, 50), ('L8', 32, 32), ('DXT1', 16, 16)]
def chunk(fmt, w, h, key):
    if fmt in BPB:
        bw, bh = (w + 3) // 4, (h + 3) // 4; lb = 3 if fmt == 'DXT1' else 4
        aw, ah = (bw + 31) & ~31, (bh + 31) & ~31; n = aw * ah * BPB[fmt]
    else:
        lb = {'A8R8G8B8': 2, 'L8': 0}[fmt]; aw, ah = (w + 31) & ~31, (h + 31) & ~31; n = aw * ah << lb
    pix = rng.integers(0, 256, n, dtype=np.uint8).tobytes()
    hdr = b'\xff\xff\xff\xff' + struct.pack('<HBBHHII', 0x4000, 11, 0x10, w, h, 0x20, key) + T.MARK
    sub = b'D2KK' + struct.pack('>IIIIIII', w, h, D3D[fmt], 1, 0, 0, 0)
    assert len(hdr) == 32 and len(sub) == 32
    return hdr + sub + pix
bank = bytearray(); meta = []
def add(c): bank.extend(struct.pack('<I', len(c))); bank.extend(c)
add(b'\xff\xff\xff\xff' + b'\0' * 28)            # header-only 32 byte record: not counted
add(b'\1' * 48)                                  # unmarked chunk
ordinal = 0
for i, (fmt, w, h) in enumerate(cases):
    key = 0x6f00298c if i == 4 else 0
    c = chunk(fmt, w, h, key)
    try:
        a, info = T.decode_xe(c); ok = True
        open(f'{OUT}/tex_{ordinal:02d}.rgba', 'wb').write(np.ascontiguousarray(a).tobytes())
        assert a.shape == (h, w, 4) and info['key'] == key
    except Exception as e:
        ok = False; print('python fails', fmt, w, h, e)
    meta.append(dict(ordinal=ordinal, fmt=fmt, w=w, h=h, ok=ok, key=key))
    add(c)
    if i == 5: add(b'\2' * 100)                  # unmarked chunk in the middle
    ordinal += 1
open(f'{OUT}/tex_bank.bin', 'wb').write(bank)
json.dump(meta, open(f'{OUT}/tex_cases.json', 'w'))
print(len(meta), 'cases', sum(m['ok'] for m in meta), 'decoded by python')
