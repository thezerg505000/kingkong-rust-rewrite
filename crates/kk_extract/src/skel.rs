//! Bone hierarchy from the GAO payloads (port of `tools/jade_skel.py`, anim_findings.md section 1).
//!
//! Each bone is a named GAO record; the payload that *precedes* a bone's name describes that bone
//! (equivalently, the payload following name_k describes object k+1). Hierarchy payload offsets:
//! `+12 idf`, `+26 mat4 m1`, `+94 + (48 if idf&0x80000 else 24)`: parent key u32 then `mat4 m2` (local
//! relative to the parent). Hierarchy flag = `idf & 0x400000`. Bone keys 0x8f008xxx are sequential in
//! listing order, so parent index = key - base. Helper bones (Snap_*, Base, Sang*) have foreign keys and
//! are skipped when numbering.

use crate::err;
use crate::error::{rd_f32, rd_u32, Result};
use crate::mat::{self, M4};
use crate::records::{iter_named, Named};

#[derive(Debug, Clone)]
pub struct Hier {
    pub idf: u32,
    pub m1: M4,
    pub key: u32,
    pub m2: M4,
}

pub fn rd_m4(d: &[u8], p: usize) -> Result<M4> {
    let mut m = [0.0; 16];
    for (i, v) in m.iter_mut().enumerate() {
        *v = rd_f32(d, p + 4 * i)? as f64;
    }
    Ok(m)
}

pub fn hier(d: &[u8], payload: usize) -> Result<Option<Hier>> {
    let idf = rd_u32(d, payload + 12)?;
    if idf & 0x400000 == 0 {
        return Ok(None);
    }
    let m1 = rd_m4(d, payload + 26)?;
    let ko = payload + 94 + if idf & 0x80000 != 0 { 48 } else { 24 };
    let key = rd_u32(d, ko)?;
    let m2 = rd_m4(d, ko + 4)?;
    Ok(Some(Hier { idf, m1, key, m2 }))
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub idx: usize,
    pub name: String,
    pub parent: Option<usize>,
    /// parent-relative local matrix, row-vector convention
    pub local: M4,
    pub skin_id: Option<usize>,
    pub parent_key: Option<u32>,
}

/// Bones named `prefix*` in the 128 KiB window before `geo_off`, in listing order, with parents.
pub fn rig(d: &[u8], geo_off: usize, prefix: &str, skip: &[&str]) -> Result<Vec<Bone>> {
    let all: Vec<Named> = iter_named(d, geo_off.saturating_sub(0x20000), Some(geo_off));
    let mut sk: Vec<(usize, String, Option<Hier>)> = Vec::new(); // (listing idx, name, hier)
    for (i, g) in all.iter().enumerate() {
        if !g.name.starts_with(prefix) {
            continue;
        }
        let h = if i > 0 { hier(d, all[i - 1].payload)? } else { None };
        let name = g.name[..g.name.len() - 4].to_string();
        sk.push((sk.len(), name, h));
    }
    if sk.is_empty() {
        return Err(err!("no bones with prefix {prefix} before {geo_off:#x}"));
    }
    let keys: Vec<u32> = sk.iter().filter_map(|(_, _, h)| h.as_ref()).map(|h| h.key).filter(|k| k >> 24 == 0x8f).collect();
    let base = *keys.iter().min().ok_or_else(|| err!("no bone hierarchy keys"))?;
    let order: Vec<usize> = sk.iter().filter(|(_, n, _)| !skip.iter().any(|s| n.contains(s))).map(|(i, _, _)| *i).collect();
    let mut keymap = std::collections::HashMap::new();
    let mut sid = std::collections::HashMap::new();
    for (i, &idx) in order.iter().enumerate() {
        keymap.insert(base + i as u32, idx);
        sid.insert(idx, i);
    }
    Ok(sk
        .iter()
        .map(|(idx, name, h)| {
            let (parent, local, parent_key) = match h {
                Some(h) => (keymap.get(&h.key).copied(), h.m2, Some(h.key)),
                None => (None, mat::IDENT, None),
            };
            Bone { idx: *idx, name: name.clone(), parent, local, skin_id: sid.get(idx).copied(), parent_key }
        })
        .collect())
}

/// World matrices (row-vector): `W = local @ W(parent)`.
pub fn world_matrices(bones: &[Bone]) -> Vec<M4> {
    let mut w: Vec<Option<M4>> = vec![None; bones.len()];
    fn go(i: usize, bones: &[Bone], w: &mut Vec<Option<M4>>, depth: usize) -> M4 {
        if let Some(m) = w[i] {
            return m;
        }
        let b = &bones[i];
        let m = match b.parent {
            Some(p) if depth < 256 && p != i => mat::mul(&b.local, &go(p, bones, w, depth + 1)),
            _ => b.local,
        };
        w[i] = Some(m);
        m
    }
    for i in 0..bones.len() {
        go(i, bones, &mut w, 0);
    }
    w.into_iter().map(|m| m.unwrap()).collect()
}

/// build_anim_glb.load_rig: fix the root so that the first skinned bone's world equals inverse(skin matrix).
/// Returns the bones with the corrected root local matrix.
pub fn load_rig(d: &[u8], geo: &crate::geo::Geo, prefix: &str, skip: &[&str], geo_off: usize) -> Result<Vec<Bone>> {
    let mut bones = rig(d, geo_off, prefix, skip)?;
    let skin = geo.skin.as_ref().ok_or_else(|| err!("GEO has no skin block"))?;
    let w0 = world_matrices(&bones);
    let f = bones
        .iter()
        .find(|b| b.skin_id.map(|s| skin.iter().any(|l| l.bone as usize == s)).unwrap_or(false))
        .ok_or_else(|| err!("no skinned bone found"))?;
    let sm = skin.iter().find(|l| l.bone as usize == f.skin_id.unwrap()).unwrap();
    let sinv = mat::inv(&mat::from_f32(&sm.mat)).ok_or_else(|| err!("singular skin matrix"))?;
    let t = mat::mul(&mat::inv(&w0[f.idx]).ok_or_else(|| err!("singular bone world"))?, &sinv);
    let root = bones.iter().position(|b| b.parent.is_none()).ok_or_else(|| err!("no root bone"))?;
    bones[root].local = mat::mul(&t, &bones[root].local);
    Ok(bones)
}
