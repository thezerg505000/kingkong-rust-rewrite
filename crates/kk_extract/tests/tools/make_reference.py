#!/usr/bin/env python3
"""Produce reference JSON from the original Python tools for the Rust parity tests.
usage: make_reference.py <kkpc dir> <out dir>      (needs numpy; reads <kkpc>/decoded/*.dec and <kkpc>/tools)
Writes into <out dir>: ref_geo.json, ref_skel.json, ref_names.json, ref_trls.json, ref_clips.json  and symlinks the .dec files."""
import sys, os, json, re, struct
import numpy as np
K = sys.argv[1]; OUT = sys.argv[2]
sys.path.insert(0, K + '/tools')
import jadegeo, jadegao, jade_skel, anim_parse as A, clipfeat as C, meshutil
os.makedirs(OUT, exist_ok=True)
STREAMS = {'ff0003eb': 0x2073a6, 'ff00018c': 0xcb7d61}
def dec(k): return open(f'{K}/decoded/{k}.dec', 'rb').read()
def f(x):
    x = float(x)
    return x if np.isfinite(x) else None
def geo_summary(g):
    s = dict(off=g.off, nverts=g.nverts, ncol=g.ncol, nuv=g.nuv, nelem=g.nelem, f2=g.f2, end=g.end,
             pos_sum=f(g.pos.astype('f8').sum()), pos_abs=f(np.abs(g.pos.astype('f8')).sum()), nrm_sum=f(g.nrm.astype('f8').sum()),
             uv_sum=f(g.uv.astype('f8').sum()) if g.nuv else 0.0, ok3_len=int(getattr(g, 'ok3_len', 0) or 0),
             elems=[dict(mat=int(e['mat']), ntri=len(e['tri']), vsum=int(e['tri']['v'].astype('i8').sum()), usum=int(e['tri']['u'].astype('i8').sum())) for e in g.elems],
             tail=list(map(int, g.tail)) if g.tail else None)
    if g.skin:
        s['skin'] = [dict(bone=int(l['bone']), n=len(l['idx']), typ=int(l['type']), mat=[f(x) for x in l['mat'].flatten()], idx_sum=int(l['idx'].sum()), w_sum=f(l['w'].astype('f8').sum())) for l in g.skin]
    try:
        pos, nrm, uv, el, src = meshutil.flatten(g)
        s['flat'] = dict(nv=len(pos), idx_sums=[int(e[1].astype('i8').sum()) for e in el], pos_sum=f(pos.astype('f8').sum()))
    except Exception as e:   # Python flatten fails for GEOs without UVs (float index array)
        s['flat'] = None
    return s
geo = {}; skel = {}; names = {}
for st, off in STREAMS.items():
    d = dec(st)
    # all GEO magics
    pat = struct.pack('<II', 0xC0DE2002, 3)
    lst = []; i = 0
    while True:
        i = d.find(pat, i)
        if i < 0: break
        try:
            g = jadegeo.parse_geo(d, i); lst.append(dict(ok=True, **geo_summary(g)))
        except Exception as e:
            lst.append(dict(ok=False, off=i, err=str(e)[:60]))
        i += 8
    geo[st] = lst
    # names over the whole stream
    names[st] = [dict(name=r['name'], name_off=r['name_off'], payload=r['payload'], size=r['size'], pre=list(map(int, r['pre']))) for r in jadegao.iter_named(d)]
    print(st, 'geos', len(lst), 'ok', sum(1 for x in lst if x['ok']), 'names', len(names[st]))
for st, off, pre, skipt in (('ff0003eb', 0x2073a6, 'B_Jaf_', ()), ('ff00018c', 0xcb7d61, 'B_Rex_', ('Snap', 'Base', 'Sang'))):
    d = dec(st); bones = jade_skel.rig(d, off, pre, skipt)
    g = jadegeo.parse_geo(d, off); S = {l['bone']: l['mat'].astype(np.float64) for l in g.skin}
    W0 = {}
    def world(i):
        if i in W0: return W0[i]
        b = bones[i]; W0[i] = b['local'] if b['parent'] is None else b['local'] @ world(b['parent']); return W0[i]
    for b in bones: world(b['idx'])
    fb = [b for b in bones if b['skin_id'] in S][0]
    T = np.linalg.inv(W0[fb['idx']]) @ np.linalg.inv(S[fb['skin_id']])
    root = [b for b in bones if b['parent'] is None][0]
    root['local'] = T @ root['local']
    skel[st] = [dict(idx=b['idx'], name=b['name'], parent=b['parent'], skin_id=b['skin_id'], parent_key=b['parent_key'], local=[f(x) for x in b['local'].flatten()]) for b in bones]
def trk_sum(tr):
    ts, tot = A.track_times(tr)
    tsum = 0.0; qsum = 0.0; nk = 0
    for nf, fl, k in tr['events']:
        if k:
            nk += 1
            if k['t'] is not None:
                t = k['t'] if not isinstance(k['t'][0], tuple) else k['t'][0]
                tsum += sum(t)
            if k['q'] is not None and k['q'][0] != 'mat': qsum += float(sum(k['q']))
    return dict(gizmo=tr['gizmo'], flags=tr['flags'], ttype=tr['ttype'], dlen=tr['dlen'], nev=len(tr['events']), total=tot, nkeys=nk, tsum=tsum, qsum=qsum)
clips = []
for which in ('arms', 'rex'):
    cat = json.load(open(f'{K}/anims/{which}_catalog.json'))
    for c in cat:
        p = f'{K}/decoded/{c["stream"]}.dec'
        if not os.path.exists(p): continue
        d = dec(c['stream']); r = A.parse_trl(d, int(c['offset'], 16)); ft = C.features(r)
        clips.append(dict(which=which, name=c['name'], stream=c['stream'], off=int(c['offset'], 16), frames=c['frames'], nAnim=r['nAnim'], numTracks=r['numTracks'], consumed=r['consumed'], tail=r['tail'], size=r['size'],
                          tracks=[trk_sum(t) for t in r['tracks']], features={k: v for k, v in ft.items()}))
print('clips', len(clips))
trls = {}
for st in ('ff0003eb', 'ff00018c', 'ff001793', 'ff001b17'):
    p = f'{K}/anims/{st}_trls.json'
    if os.path.exists(p) and os.path.exists(f'{K}/decoded/{st}.dec'): trls[st] = json.load(open(p))
for n, o in (('geo', geo), ('skel', skel), ('names', names), ('clips', clips), ('trls', trls)):
    json.dump(o, open(f'{OUT}/ref_{n}.json', 'w'), allow_nan=False)
for st in ('ff0003eb', 'ff00018c', 'ff001793', 'ff001b17'):
    dst = f'{OUT}/{st}.dec'
    if not os.path.exists(dst): os.symlink(f'{K}/decoded/{st}.dec', dst)
