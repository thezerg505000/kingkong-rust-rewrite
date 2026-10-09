#!/usr/bin/env python3
"""export_rex_kt.py : Kong-level V-Rex (KT) animation set, by the game's own action ids (research tool).

Source: level 07D stream ff00f858 of the user's own game files. The rex GAO J_PNJ_KTREX_2 owns an action kit
(resource cb 0x947070 at 0x27f93d2, u32 len + 162 action keys); each action record (cb 0x946bf0) lists items whose
k0 is a track-list key (cb 0xa78560). Writes <assets>/trex_kt.glb (skeleton + mesh of trex_inplace.glb, clips named
kt_0xNN), trex_kt_rootmotion.json, trex_kt_actions.json. Root track removed like trex_inplace.

Inputs (environment, nothing is shipped with the repo):
  KK_RESEARCH_TOOLS  folder with anim_parse.py / export_creature.py / kk_trl.py (research/pc/tools)
  KK_DEC07D          decoded stream ff00f858.dec (tools/extract_bin.py ff00f858)
  KK_RECS07D         pickled resource records of that stream from the loader emulation
                     (research/pc/keys/tools/lvl/runall.py, KK_WORLD=07D)
  KK_ASSETS          asset folder holding trex_inplace.glb (kk_extract output); trex_kt.glb is written there
kk_fps loads trex_kt.glb when present and falls back to the 03E clip names otherwise.
"""
import sys,os,json,struct,pickle,collections,numpy as np
sys.path.insert(0,os.environ.get('KK_RESEARCH_TOOLS','research/pc/tools'))
import anim_parse as A, export_creature as EC, kk_trl as K
KK=os.environ.get('KK_ASSETS','assets')
d=open(os.environ['KK_DEC07D'],'rb').read()
R=pickle.load(open(os.environ['KK_RECS07D'],'rb'))
reg={}
for i,r in enumerate(R): reg.setdefault(r[2],i)
children=collections.defaultdict(list)
for i,r in enumerate(R): children[r[6].get('par')].append(i)
KIT=4600
bykey={R[a][2]:a for a in children[KIT]}
ko=R[KIT][0]; L,=struct.unpack_from('<I',d,ko); kit=struct.unpack_from('<%dI'%(L//4),d,ko+4)
def items(a):
    p=R[a][0]; n=d[p+4]; return [struct.unpack_from('<3I',d,p+6+19*i)+(d[p+6+19*i+12:p+6+19*i+19].hex(),) for i in range(n)]
acts={}
for aid,k in enumerate(kit):
    if k in (0,1,0xffffffff) or k not in bykey: continue
    trls=[]
    for k0,k1,k2,fl in items(bykey[k]):
        j=reg.get(k0)
        if j is not None and R[j][3]==0xa78560: trls.append((R[j][0],fl))
    if trls: acts[aid]=trls
# glb template
src=open(f'{KK}/trex_inplace.glb','rb').read(); jl=struct.unpack_from('<I',src,12)[0]; js=json.loads(src[20:20+jl]); bo=20+jl+8
binlen=struct.unpack_from('<I',src,20+jl)[0]; BIN=bytearray(src[bo:bo+binlen])
node_of={n['extras']['jade_listing_index']:i for i,n in enumerate(js['nodes']) if n.get('extras',{}).get('jade_listing_index') is not None}
pel=node_of[0]; pn=js['nodes'][pel]; t0=np.array(pn['translation']); q0=np.array(pn['rotation'])
def add_acc(arr,typ):
    global BIN
    while len(BIN)%4: BIN.append(0)
    off=len(BIN); a=np.ascontiguousarray(arr,dtype='<f4'); BIN+=a.tobytes()
    js['bufferViews'].append(dict(buffer=0,byteOffset=off,byteLength=a.nbytes)); bv=len(js['bufferViews'])-1
    cnt=a.shape[0]; acc=dict(bufferView=bv,componentType=5126,count=cnt,type=typ)
    if typ=='SCALAR': acc['min']=[float(a.min())]; acc['max']=[float(a.max())]
    js['accessors'].append(acc); return len(js['accessors'])-1
anims=[]; rootm={}; table={}
for aid,trls in sorted(acts.items()):
    for n_,(off,fl) in enumerate(trls):
        r=K.parse_trl(d,off); ch,fr=EC.parse_clip(r)
        # pelvis bind in jade convention is not needed: inplace_clip re-expresses pelvis relative to the root track
        chn,rm=EC.inplace_clip(ch,(t0,q0))
        name=f'kt_{aid:#04x}' + (f'_{n_}' if n_ else '')
        samplers=[];channels=[]
        for gz,e in sorted(chn.items()):
            if gz not in node_of: continue
            for path,key in (('rotation','q'),('translation','t')):
                if e[key] is None: continue
                ts,vs=e[key]
                if len(ts)==0: continue
                keep=[0]+[i for i in range(1,len(ts)) if ts[i]>ts[i-1]]
                ts=np.array(ts)[keep]/60.0; vs=np.array(vs)[keep]
                if key=='q':
                    for i in range(1,len(vs)):
                        if np.dot(vs[i],vs[i-1])<0: vs[i]=-vs[i]
                samplers.append(dict(input=add_acc(ts,'SCALAR'),output=add_acc(vs,'VEC4' if key=='q' else 'VEC3'),interpolation='LINEAR'))
                channels.append(dict(sampler=len(samplers)-1,target=dict(node=node_of[gz],path=path)))
        # root (JadeActor) identity like trex_inplace
        for path,val,typ in (('rotation',[0,0,0,1.0],'VEC4'),('translation',[0,0,0.0],'VEC3')):
            samplers.append(dict(input=add_acc(np.array([0.0,max(fr,1)/60.0]),'SCALAR'),output=add_acc(np.array([val,val]),typ),interpolation='LINEAR'))
            channels.append(dict(sampler=len(samplers)-1,target=dict(node=1,path=path)))
        anims.append(dict(name=name,samplers=samplers,channels=channels,extras=dict(source=f'ff00f858.bin@{hex(off)}',action_id=hex(aid),item=n_,item_flags=fl,frames=fr,fps=60.0)))
        dur=fr/60.0; disp=rm['jade_displacement']
        rootm[name]=dict(duration_s=dur,jade_displacement=disp,gltf_displacement=[disp[0],disp[2],-disp[1]],yaw_deg=rm['yaw_deg'],speed_mps=float(np.hypot(disp[0],disp[1])/dur) if dur>0 else 0.0,keys_actor=rm['keys_actor'])
        table.setdefault(hex(aid),[]).append(dict(clip=name,frames=fr,source=hex(off),item_flags=fl))
js['animations']=anims
js['asset']['extras']=dict(source="King Kong 2005 PC level 07D (ff00f858): Kong-level V-Rex action kit of J_PNJ_KTREX_2, clips by action id (kt_0xNN)",fps=60,units='metres')
while len(BIN)%4: BIN.append(0)
js['buffers']=[dict(byteLength=len(BIN))]
J=json.dumps(js,separators=(',',':')).encode()
while len(J)%4: J+=b' '
out=f'{KK}/trex_kt.glb'
open(out,'wb').write(struct.pack('<4sII',b'glTF',2,12+8+len(J)+8+len(BIN))+struct.pack('<I4s',len(J),b'JSON')+J+struct.pack('<I4s',len(BIN),b'BIN\0')+bytes(BIN))
json.dump(rootm,open(f'{KK}/trex_kt_rootmotion.json','w'),indent=1)
json.dump(table,open(f'{KK}/trex_kt_actions.json','w'),indent=1)
print('clips',len(anims),'actions',len(table),os.path.getsize(out))
