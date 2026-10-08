#!/usr/bin/env python3 -I
"""Linear-sweep float-constant extractor for PS2 (R5900) functions. Pure Python, no emulation.
Tracks lui/ori/addiu/addu-free constant building per register plus gp (from .reginfo), and resolves
lw/lwc1 loads from known addresses in the image. Collects float constants and immediate integers.
usage: ps2_parity_probe.py <ps2_addr_hex> [end_hex]"""
import struct, sys, json
ELF='/home/claude/kkps2/SLUS_213.11'
d=open(ELF,'rb').read()
BASE=0x100000; OFF=0x100
def rd(v,n=4):
    o=v-BASE+OFF
    return d[o:o+n] if 0<=o<len(d) else None
def u32(v):
    b=rd(v); return struct.unpack('<I',b)[0] if b and len(b)==4 else None
def f32(u): return struct.unpack('<f',struct.pack('<I',u))[0]
# gp from .reginfo (ri_gp_value at +20)
shoff,=struct.unpack_from('<I',d,0x20);shn,=struct.unpack_from('<H',d,0x30);ss,=struct.unpack_from('<H',d,0x32)
shs=[struct.unpack_from('<10I',d,shoff+40*i) for i in range(shn)]
GP=0
for s in shs:
    nm=d[shs[ss][4]+s[0]:d.find(b'\0',shs[ss][4]+s[0])]
    if nm==b'.reginfo': GP=struct.unpack_from('<I',d,s[4]+20)[0]
def sx(x): return x-0x10000 if x&0x8000 else x

def sweep(start,end):
    """returns dict: float value -> list of (addr, how)"""
    regs={28:GP}; fl={}; ints={}
    def note(a,u,how):
        fl.setdefault(round(f32(u),6),[]).append((a,how))
    for a in range(start,end,4):
        w=u32(a); op=w>>26; rs=(w>>21)&31; rt=(w>>16)&31; imm=w&0xffff
        if op==0x0f: regs[rt]=imm<<16
        elif op==0x0d and rs in regs:   # ori
            regs[rt]=regs[rs]|imm
            if regs[rt]&0xffff0000 and (regs[rt]>>23)&0xff in range(100,160): note(a,regs[rt]&0xffffffff,'lui+ori')
        elif op in (0x08,0x09) and rs in regs:
            regs[rt]=(regs[rs]+sx(imm))&0xffffffff
        elif op==0x31:   # lwc1 ft,off(rs)
            if rs in regs:
                ea=(regs[rs]+sx(imm))&0xffffffff
                u=u32(ea)
                if u is not None: note(a,u,'lwc1 %08x'%ea)
        elif op==0x23 and rs in regs:
            ea=(regs[rs]+sx(imm))&0xffffffff; u=u32(ea)
            regs.pop(rt,None)
        elif op==0x11 and rs==4:  # mtc1 rt,fs  (cop1 sub=4)
            if rt in regs: note(a,regs[rt]&0xffffffff,'mtc1')
        else:
            # any other write invalidates rt for I-type ALU/loads; coarse
            if op in (0x0a,0x0b,0x0c,0x0e,0x20,0x21,0x22,0x24,0x25,0x26,0x37,0x1e,0x07 if False else 0x23): regs.pop(rt,None)
            if op==0: regs.pop((w>>11)&31,None)
        # branch targets: keep regs (linear approximation)
    return fl

if __name__=='__main__':
    s=int(sys.argv[1],16); e=int(sys.argv[2],16) if len(sys.argv)>2 else s+0x4000
    r=sweep(s,e)
    for k in sorted(r): print(k,[(hex(a),h) for a,h in r[k]][:4])
