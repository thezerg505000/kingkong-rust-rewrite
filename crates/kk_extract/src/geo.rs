//! GEO (mesh) parser: port of `tools/jadegeo.py` + `tools/meshutil.flatten`.
//! Layout is documented in mesh_findings.md (versioned header 0xC0DE2002, skin block, OK3 collision
//! block located by search, positions, normals, colours, UVs, element headers, triangles).

use crate::err;
use crate::error::{rd_f32, rd_u16, rd_u32, Result};
use std::collections::HashMap;

pub const MAGIC: u32 = 0xC0DE2002;

#[derive(Debug, Clone)]
pub struct SkinList {
    pub bone: u16,
    pub mat: [f32; 16],
    pub typ: i32,
    pub idx: Vec<u16>,
    pub w: Vec<f32>,
}

#[derive(Debug, Clone, Copy)]
pub struct Tri {
    pub v: [u16; 3],
    pub u: [u16; 3],
    pub sg: u32,
    pub fl: u32,
}

#[derive(Debug, Clone)]
pub struct Elem {
    pub mat: u32,
    pub a: u32,
    pub b: u32,
    pub tri: Vec<Tri>,
}

#[derive(Debug, Clone)]
pub struct Geo {
    pub off: usize,
    pub nverts: usize,
    pub mrm: u32,
    pub ncol: usize,
    pub nuv: usize,
    pub nelem: usize,
    pub f1: u32,
    pub f2: u32,
    pub skin: Option<Vec<SkinList>>,
    pub skin_flags: u16,
    pub ok3_at: Option<usize>,
    pub ok3_len: usize,
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub col: Vec<u32>,
    pub uv: Vec<[f32; 2]>,
    pub elems: Vec<Elem>,
    pub tail: Option<[u32; 3]>,
    pub end: usize,
}

fn finite_run(d: &[u8], p: usize, n: usize) -> bool {
    (0..n).all(|i| rd_f32(d, p + 4 * i).map(|f| f.is_finite()).unwrap_or(false))
}

/// Search for the start of the position array behind an OK3 block: `n` positions followed by `n`
/// unit normals (|len-1| < 2e-3). Mirrors jadegeo.py (step 2, up to 1 MiB).
fn find_positions(d: &[u8], p: usize, n0: usize) -> Option<usize> {
    let lim = d.len().checked_sub(24 * n0 + 16)?.min(p + (1 << 20));
    let mut q = p;
    while q < lim {
        let np = q + 12 * n0;
        if np + 12 <= d.len() {
            let (x, y, z) = (rd_f32(d, np).ok()?, rd_f32(d, np + 4).ok()?, rd_f32(d, np + 8).ok()?);
            if ((x * x + y * y + z * z) - 1.0).abs() < 2e-3 && finite_run(d, np, n0 * 3) {
                let mut ok = true;
                for i in 0..n0 {
                    let (x, y, z) = (rd_f32(d, np + 12 * i).ok()?, rd_f32(d, np + 12 * i + 4).ok()?, rd_f32(d, np + 12 * i + 8).ok()?);
                    let nn = ((x * x + y * y + z * z) as f64).sqrt();
                    if (nn - 1.0).abs() >= 2e-3 {
                        ok = false;
                        break;
                    }
                }
                if ok && finite_run(d, q, n0 * 3) {
                    return Some(q);
                }
            }
        }
        q += 2;
    }
    None
}


/// Skin block at `p`: `u16 flags | u16 nlists | lists`. Returns (flags, lists, end).
fn parse_skin(d: &[u8], mut p: usize) -> Result<(u16, Vec<SkinList>, usize)> {
    let skin_flags = rd_u16(d, p)?;
    let nl = rd_u16(d, p + 2)? as usize;
    p += 4;
    let mut lists = Vec::with_capacity(nl);
    for _ in 0..nl {
        let bone = rd_u16(d, p)?;
        let vc = rd_u16(d, p + 2)? as usize;
        p += 4;
        let mut mat = [0f32; 16];
        for (i, m) in mat.iter_mut().enumerate() {
            *m = rd_f32(d, p + 4 * i)?;
        }
        p += 64;
        let typ = rd_u32(d, p)? as i32;
        p += 4;
        let mut idx = Vec::with_capacity(vc);
        let mut w = Vec::with_capacity(vc);
        for i in 0..vc {
            idx.push(rd_u16(d, p + 4 * i)?);
            w.push(f32::from_bits((rd_u16(d, p + 4 * i + 2)? as u32) << 16));
        }
        p += 4 * vc;
        lists.push(SkinList { bone, mat, typ, idx, w });
    }
    Ok((skin_flags, lists, p))
}

/// `o` = offset of the 0xC0DE2002 magic.
pub fn parse_geo(d: &[u8], o: usize) -> Result<Geo> {
    if rd_u32(d, o)? != MAGIC || rd_u32(d, o + 4)? != 3 {
        return Err(err!("not a GEO at {o:#x}"));
    }
    let nverts = rd_u32(d, o + 8)? as usize;
    let mrm = rd_u32(d, o + 12)?;
    let ncol = rd_u32(d, o + 16)? as usize;
    let nuv = rd_u32(d, o + 20)? as usize;
    let nelem = rd_u32(d, o + 24)? as usize;
    let f1 = rd_u32(d, o + 28)?;
    let f2 = rd_u32(d, o + 32)?;
    if nverts > 2_000_000 || nuv > 4_000_000 || nelem > 4096 || ncol > 2_000_000 {
        return Err(err!("implausible GEO header at {o:#x}"));
    }
    let mut p = o + 36;
    let mut skin = None;
    let mut skin_flags = 0;
    if f2 & MAGIC == MAGIC {
        let (fl, lists, np) = parse_skin(d, p)?;
        skin_flags = fl;
        skin = Some(lists);
        p = np;
    }
    let mut ok3_at = None;
    let mut ok3_len = 0;
    if f2 & 1 != 0 {
        ok3_at = Some(p);
        let found = find_positions(d, p, nverts).ok_or_else(|| err!("ok3: positions not found at {o:#x}"))?;
        ok3_len = found - p;
        p = found;
    }
    let n = nverts;
    let mut pos = Vec::with_capacity(n);
    let mut nrm = Vec::with_capacity(n);
    for i in 0..n {
        pos.push([rd_f32(d, p + 12 * i)?, rd_f32(d, p + 12 * i + 4)?, rd_f32(d, p + 12 * i + 8)?]);
    }
    p += 12 * n;
    for i in 0..n {
        nrm.push([rd_f32(d, p + 12 * i)?, rd_f32(d, p + 12 * i + 4)?, rd_f32(d, p + 12 * i + 8)?]);
    }
    p += 12 * n;
    let mut col = Vec::new();
    if ncol > 0 {
        for i in 0..ncol {
            col.push(rd_u32(d, p + 4 * i)?);
        }
        p += 4 * ncol;
    }
    let mut uv = Vec::with_capacity(nuv);
    for i in 0..nuv {
        uv.push([rd_f32(d, p + 8 * i)?, rd_f32(d, p + 8 * i + 4)?]);
    }
    p += 8 * nuv;
    let mut hdr = Vec::with_capacity(nelem);
    for _ in 0..nelem {
        hdr.push([rd_u32(d, p)?, rd_u32(d, p + 4)?, rd_u32(d, p + 8)?, rd_u32(d, p + 12)?]);
        p += 16;
    }
    let mut elems = Vec::with_capacity(nelem);
    for h in hdr {
        let nt = h[0] as usize;
        if p + 20 * nt > d.len() {
            return Err(err!("GEO at {o:#x}: triangle data past end"));
        }
        let mut tri = Vec::with_capacity(nt);
        for i in 0..nt {
            let q = p + 20 * i;
            tri.push(Tri {
                v: [rd_u16(d, q)?, rd_u16(d, q + 2)?, rd_u16(d, q + 4)?],
                u: [rd_u16(d, q + 6)?, rd_u16(d, q + 8)?, rd_u16(d, q + 10)?],
                sg: rd_u32(d, q + 12)?,
                fl: rd_u32(d, q + 16)?,
            });
        }
        p += 20 * nt;
        elems.push(Elem { mat: h[1], a: h[2], b: h[3], tri });
    }
    let tail = if p + 12 <= d.len() { Some([rd_u32(d, p)?, rd_u32(d, p + 4)?, rd_u32(d, p + 8)?]) } else { None };
    Ok(Geo { off: o, nverts, mrm, ncol, nuv, nelem, f1, f2, skin, skin_flags, ok3_at, ok3_len, pos, nrm, col, uv, elems, tail, end: p })
}


/// `kkc_geo.parse_geo`: [`parse_geo`], accepted only when it ends exactly 12 bytes before the record end; otherwise the
/// layout is located from the record END (trailer, triangles 20 B, element headers 16 B, uv, colours, normals, positions),
/// which does not need the heuristic search for the OK3 block. `o` = offset of the magic; the record size sits at `o - 8`.
pub fn parse_geo_kkc(d: &[u8], o: usize) -> Result<Geo> {
    let size = rd_u32(d, o.checked_sub(8).ok_or_else(|| err!("GEO at {o:#x}: no record header"))?)? as usize;
    let end = o - 4 + size;
    if let Ok(g) = parse_geo(d, o) {
        if g.end + 12 == end {
            return Ok(g);
        }
    }
    if rd_u32(d, o)? != MAGIC || rd_u32(d, o + 4)? != 3 {
        return Err(err!("not a GEO at {o:#x}"));
    }
    let nverts = rd_u32(d, o + 8)? as usize;
    let mrm = rd_u32(d, o + 12)?;
    let ncol = rd_u32(d, o + 16)? as usize;
    let nuv = rd_u32(d, o + 20)? as usize;
    let nelem = rd_u32(d, o + 24)? as usize;
    let f1 = rd_u32(d, o + 28)?;
    let f2 = rd_u32(d, o + 32)?;
    if nverts > 2_000_000 || nuv > 4_000_000 || nelem > 4096 || ncol > 2_000_000 {
        return Err(err!("implausible GEO header at {o:#x}"));
    }
    let mut p = o + 36;
    let (mut skin, mut skin_flags) = (None, 0);
    if f2 & MAGIC == MAGIC {
        let (fl, lists, np) = parse_skin(d, p)?;
        skin_flags = fl;
        skin = Some(lists);
        p = np;
    }
    let skin_end = p;
    let n = nverts;
    let tr = end.checked_sub(12).ok_or_else(|| err!("short GEO"))?;
    let base_after = 24 * n + 4 * ncol + 8 * nuv;
    let mut found = None;
    let mut h = tr as i64 - 16 * nelem as i64;
    let lo = (skin_end + base_after) as i64 - 1;
    while h > lo {
        let hu = h as usize;
        let mut ok = true;
        let mut sum = 0usize;
        let mut hdr = Vec::with_capacity(nelem);
        for e in 0..nelem {
            let q = hu + 16 * e;
            let (Ok(a), Ok(m), Ok(b0), Ok(b1)) = (rd_u32(d, q), rd_u32(d, q + 4), rd_u32(d, q + 8), rd_u32(d, q + 12)) else {
                ok = false;
                break;
            };
            sum += a as usize;
            hdr.push([a, m, b0, b1]);
        }
        if ok && sum * 20 + 16 * nelem == tr - hu && hdr.iter().all(|x| x[2] == 0 && x[3] == 0) && hu >= skin_end + base_after {
            let q = hu - base_after;
            if q >= skin_end {
                let cnt = n.min(64) * 3;
                let mut good = true;
                let mut vals = Vec::with_capacity(cnt);
                for i in 0..cnt {
                    match rd_f32(d, q + 12 * n + 4 * i) {
                        Ok(v) if v.is_finite() => vals.push(v),
                        _ => {
                            good = false;
                            break;
                        }
                    }
                }
                if good {
                    let maxdev = vals.chunks(3).map(|c| ((c[0] * c[0] + c[1] * c[1] + c[2] * c[2]) as f64).sqrt()).map(|l| (l - 1.0).abs()).fold(0.0, f64::max);
                    if maxdev < 0.02 {
                        found = Some((q, hu, hdr));
                        break;
                    }
                }
            }
        }
        h -= 4;
    }
    let (mut q, h, hdr) = found.ok_or_else(|| err!("geo: layout not found at {o:#x}"))?;
    let ok3_len = q - skin_end;
    let mut pos = Vec::with_capacity(n);
    let mut nrm = Vec::with_capacity(n);
    for i in 0..n {
        pos.push([rd_f32(d, q + 12 * i)?, rd_f32(d, q + 12 * i + 4)?, rd_f32(d, q + 12 * i + 8)?]);
    }
    q += 12 * n;
    for i in 0..n {
        nrm.push([rd_f32(d, q + 12 * i)?, rd_f32(d, q + 12 * i + 4)?, rd_f32(d, q + 12 * i + 8)?]);
    }
    q += 12 * n;
    let mut col = Vec::new();
    for i in 0..ncol {
        col.push(rd_u32(d, q + 4 * i)?);
    }
    q += 4 * ncol;
    let mut uv = Vec::with_capacity(nuv);
    for i in 0..nuv {
        uv.push([rd_f32(d, q + 8 * i)?, rd_f32(d, q + 8 * i + 4)?]);
    }
    let mut p = h + 16 * nelem;
    let mut elems = Vec::with_capacity(nelem);
    for hd in hdr {
        let nt = hd[0] as usize;
        let mut tri = Vec::with_capacity(nt);
        for i in 0..nt {
            let q = p + 20 * i;
            tri.push(Tri {
                v: [rd_u16(d, q)?, rd_u16(d, q + 2)?, rd_u16(d, q + 4)?],
                u: [rd_u16(d, q + 6)?, rd_u16(d, q + 8)?, rd_u16(d, q + 10)?],
                sg: rd_u32(d, q + 12)?,
                fl: rd_u32(d, q + 16)?,
            });
        }
        p += 20 * nt;
        elems.push(Elem { mat: hd[1], a: hd[2], b: hd[3], tri });
    }
    let tail = if p + 12 <= d.len() { Some([rd_u32(d, p)?, rd_u32(d, p + 4)?, rd_u32(d, p + 8)?]) } else { None };
    Ok(Geo { off: o, nverts, mrm, ncol, nuv, nelem, f1, f2, skin, skin_flags, ok3_at: Some(skin_end), ok3_len, pos, nrm, col, uv, elems, tail, end: p })
}

impl Geo {
    pub fn ntri(&self) -> usize {
        self.elems.iter().map(|e| e.tri.len()).sum()
    }
}

/// Render mesh: vertices expanded per (position index, uv index) pair (meshutil.flatten).
pub struct Flat {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    /// (material id, indices) per element
    pub elems: Vec<(u32, Vec<u32>)>,
    /// index into `Geo::pos` of each output vertex
    pub src: Vec<usize>,
}

pub fn flatten(g: &Geo) -> Result<Flat> {
    let mut keys: HashMap<(u16, u16), u32> = HashMap::new();
    let mut src = Vec::new();
    let mut uvi = Vec::new();
    let mut elems = Vec::new();
    for e in &g.elems {
        let mut idx = Vec::with_capacity(e.tri.len() * 3);
        for t in &e.tri {
            for c in 0..3 {
                let k = (t.v[c], t.u[c]);
                let j = match keys.get(&k) {
                    Some(&j) => j,
                    None => {
                        if k.0 as usize >= g.nverts || (g.nuv > 0 && k.1 as usize >= g.nuv) {
                            return Err(err!("GEO triangle index out of range"));
                        }
                        let j = src.len() as u32;
                        keys.insert(k, j);
                        src.push(k.0 as usize);
                        uvi.push(k.1 as usize);
                        j
                    }
                };
                idx.push(j);
            }
        }
        elems.push((e.mat, idx));
    }
    Ok(Flat {
        pos: src.iter().map(|&i| g.pos[i]).collect(),
        nrm: src.iter().map(|&i| g.nrm[i]).collect(),
        uv: if g.nuv > 0 { uvi.iter().map(|&i| g.uv[i]).collect() } else { vec![[0.0, 0.0]; src.len()] },
        elems,
        src,
    })
}
