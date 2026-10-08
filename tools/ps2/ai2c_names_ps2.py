#!/usr/bin/env python3 -I
"""Recover the AI2C name table from the PS2 retail ELF (SLUS_213.11) and map it to the PC names.

Table layouts found (empirically; all little endian, vaddr == file offset + 0xFFF00 for the single PT_LOAD):
  function table : rows of 12 bytes  {u32 fn, u32 name_ptr, u32 key}      PS2: 0x62db84..0x632774, 1621 rows
                   (PC: {u32 key, u32 fn, u32 name_ptr}; field order differs, and the keys are NOT equal
                   between the builds - 0 of 1621 match - so match by NAME, never by key)
  trigger table  : rows of 16 bytes {u32 file, u32 index, u32 fn, u32 name_ptr}  PS2: 0x635400..0x635df0, 160 rows
                   (same layout as PC; 'file' keys are equal across builds, e.g. 0x3d00cede = TrigCINE group)
  Seven trigger functions (Trig_CINE, TrigExec_Simple, TrigTest_Simple, TrigExec_Spawner, TrigExec_FX,
  TrigExec_InteractiveObject, TrigSound) sit in the 12-byte function table on PS2 (PC: trigger table).

usage: python3 -I ai2c_names_ps2.py [elf] [outdir] [pc_functions.json] [pc_triggers.json] [map.tsv]
"""
import struct, json, sys, os

ELF = sys.argv[1] if len(sys.argv) > 1 else '/home/claude/kkps2/SLUS_213.11'
OUT = sys.argv[2] if len(sys.argv) > 2 else '/home/claude/kkps2'
PCF = sys.argv[3] if len(sys.argv) > 3 else '/home/claude/kkpc/code/ai2c_functions.json'
PCT = sys.argv[4] if len(sys.argv) > 4 else '/home/claude/kkpc/code/ai2c_triggers.json'
MAP = sys.argv[5] if len(sys.argv) > 5 else '/home/claude/kkframe/tools/ps2/pc_ps2_map.tsv'
ANCHORS = [b'H_exec_read_joy', b'PNJ_Raptor_exec_select_action', b'Trig_CINE']

d = open(ELF, 'rb').read()
assert d[:4] == b'\x7fELF' and d[4] == 1 and d[5] == 1, 'expected ELF32 LE'
phoff, = struct.unpack_from('<I', d, 0x1c)
phnum, = struct.unpack_from('<H', d, 0x2c)
segs = []  # (vaddr, offset, filesz)
for i in range(phnum):
    t, o, v, p, fs, ms, fl, al = struct.unpack_from('<8I', d, phoff + 32 * i)
    if t == 1 and fs:
        segs.append((v, o, fs))

def v2o(v):
    for va, o, fs in segs:
        if va <= v < va + fs:
            return o + v - va
    return None

def o2v(o):
    for va, so, fs in segs:
        if so <= o < so + fs:
            return va + o - so
    return None

def cstr(v):
    o = v2o(v)
    if o is None:
        return None
    e = d.find(b'\0', o, o + 100)
    if e < 0:
        return None
    x = d[o:e]
    if 2 <= len(x) < 80 and all(32 <= c < 127 for c in x):
        return x.decode()
    return None

# code lies below the string pool; use the lowest anchor string as upper bound for fn pointers
anch = []
for a in ANCHORS:
    o = d.find(a + b'\0')
    assert o >= 0, a
    anch.append(o2v(o))
CODE_HI = min(anch)
CODE_LO = segs[0][0]
print('segments', [(hex(a), hex(c)) for a, _, c in segs], 'code<', hex(CODE_HI))

def scan(width, fnidx, nameidx, ok, minlen=20):
    rows = []
    va0, o0, fs0 = segs[0]
    for o in range(o0, o0 + fs0 - width, 4):
        w = struct.unpack_from('<%dI' % (width // 4), d, o)
        f, n = w[fnidx], w[nameidx]
        if CODE_LO <= f < CODE_HI and f % 4 == 0 and ok(w):
            t = cstr(n)
            if t:
                rows.append((o2v(o), w, t))
    runs, cur = [], []
    for r in rows:
        if cur and r[0] == cur[-1][0] + width:
            cur.append(r)
        else:
            if cur:
                runs.append(cur)
            cur = [r]
    if cur:
        runs.append(cur)
    return [r for r in runs if len(r) >= minlen]

fruns = scan(12, 0, 1, lambda w: True)
truns = scan(16, 2, 3, lambda w: w[0] > 0x1000000 and w[1] < 0x10000)
frows = [r for run in fruns for r in run]
trows = [r for run in truns for r in run]
print('function table runs', [(hex(r[0][0]), len(r)) for r in fruns])
print('trigger table runs', [(hex(r[0][0]), len(r)) for r in truns])

funcs = [dict(name=t, key=w[2], addr=hex(w[0]), entry=hex(e)) for e, w, t in frows]
trigs = [dict(entry=hex(e), file=w[0], index=w[1], addr=hex(w[2]), name=t) for e, w, t in trows]
os.makedirs(OUT, exist_ok=True)
json.dump(funcs, open(os.path.join(OUT, 'ai2c_functions_ps2.json'), 'w'), indent=0)
json.dump(trigs, open(os.path.join(OUT, 'ai2c_triggers_ps2.json'), 'w'), indent=0)

pcf = json.load(open(PCF)); pct = json.load(open(PCT))
pcmap = {}
for x in pcf + pct:
    pcmap.setdefault(x['name'], x['addr'])
ps = {}
for x in funcs + trigs:
    ps.setdefault(x['name'], x['addr'])
matched = sorted(set(pcmap) & set(ps))
pc_only = sorted(set(pcmap) - set(ps))
ps_only = sorted(set(ps) - set(pcmap))
fn_names = {x['name'] for x in pcf}; tr_names = {x['name'] for x in pct}
print('PC function names %d, PC trigger names %d' % (len(fn_names), len(tr_names)))
print('PC function names found on PS2: %d' % len(fn_names & set(ps)))
print('PC trigger names found on PS2: %d' % len(tr_names & set(ps)))
print('matched total %d, PC-only %d, PS2-only %d' % (len(matched), len(pc_only), len(ps_only)))
print('PS2-only:', ps_only)
# PC-only names whose string still exists in the ELF (referenced elsewhere)
instr = [n for n in pc_only if d.find(n.encode() + b'\0') >= 0]
print('PC-only names with a string in the ELF:', instr)
with open(MAP, 'w') as f:
    f.write('name\tpc_addr\tps2_addr\n')
    for n in matched:
        f.write('%s\t%s\t%s\n' % (n, pcmap[n], ps[n]))
with open(os.path.join(OUT, 'ai2c_pc_only_names.txt'), 'w') as f:
    f.write('\n'.join(pc_only) + '\n')
