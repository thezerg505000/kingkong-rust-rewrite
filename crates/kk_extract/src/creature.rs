//! Kong and Ann (`B_Kong_`, `B_Ann_` rigs of stream ff00018c): ports of `tools/creature_rig.py`, `tools/kk_trl.py` and
//! `tools/export_creature.py` (findings: `kong_asset_findings.md`).
//!
//! * rig: bone chain of `B_<prefix>_*` GAO records, parent link found by `hier2` (fixed offset, or a search for the copy of m1)
//! * clips: action id (kit `u32[371]`) -> action record -> track list; every track list of the chain is exported, in place
//!   (the actor-root track is removed: `p_local = R^-1 (p - root_t)`, `q_local = conj(q_root) * q_pelvis`)
//! * outputs: glb, `<name>_rootmotion.json`, `<name>_actions.json` (Kong)

use crate::anim::{self, Rot, Trl};
use crate::build::{parse_num, BuildOpts, Built, Source};
use crate::err;
use crate::error::{rd_f32, rd_u32, Result};
use crate::geo::{self, Geo};
use crate::glb::{jd, Glb};
use crate::mat::{self, M4};
use crate::records::iter_named;
use crate::skel::{rd_m4, world_matrices, Bone};
use crate::texture;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const FPS: f64 = 60.0;

// ------------------------------------------------------------------ rig (creature_rig.py)

/// Parent key and local matrix of the object whose payload starts at `payload` (`creature_rig.hier2`).
pub fn hier2(d: &[u8], payload: usize, size: usize) -> Result<Option<(u32, M4)>> {
    let idf = rd_u32(d, payload + 12)?;
    let ko = payload + 94 + if idf & 0x80000 != 0 { 48 } else { 24 };
    let key = rd_u32(d, ko)?;
    if key >> 24 == 0x8f && idf & 0x400000 != 0 {
        return Ok(Some((key, rd_m4(d, ko + 4)?)));
    }
    let m1 = d.get(payload + 26..payload + 90).ok_or_else(|| err!("hier2: short payload at {payload:#x}"))?;
    let lo = payload + 100;
    let hi = (payload + size).min(d.len());
    if hi < lo + 64 {
        return Ok(None);
    }
    let found = d[lo..hi].windows(64).position(|w| w == m1);
    let Some(i) = found.map(|i| i + lo) else { return Ok(None) };
    Ok(Some((rd_u32(d, i - 4)?, rd_m4(d, i)?)))
}

#[derive(Debug, Clone)]
pub struct RigSpec {
    pub prefix: String,
    pub helpers: Vec<String>,
    pub skinskip: Vec<String>,
    pub first_name: Option<String>,
    pub allow: Vec<String>,
}

impl RigSpec {
    pub fn from_recipe(r: &Value) -> Result<RigSpec> {
        let list = |k: &str, def: &[&str]| -> Vec<String> {
            r.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_else(|| def.iter().map(|s| s.to_string()).collect())
        };
        Ok(RigSpec {
            prefix: r["prefix"].as_str().ok_or_else(|| err!("rig.prefix missing"))?.to_string(),
            helpers: list("helpers", &["Snap", "Deltoide", "Oeil", "Base", "Sang"]),
            skinskip: list("skinskip", &["Snap", "Base", "Sang"]),
            first_name: r["first_name"].as_str().map(String::from),
            allow: list("allow", &[]),
        })
    }
}

struct ChainBone {
    idx: usize,
    name: String,
    h: Option<(u32, M4)>,
}

/// `creature_rig.bone_chain`: the longest contiguous run of `prefix` records.
fn bone_chain(d: &[u8], spec: &RigSpec) -> Result<Vec<ChainBone>> {
    let recs = iter_named(d, 0, None);
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    for (i, g) in recs.iter().enumerate() {
        let stem = &g.name[..g.name.len().saturating_sub(4)];
        if g.name.starts_with(&spec.prefix) || spec.allow.iter().any(|a| a == stem) {
            cur.push(i);
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs.retain(|r| r.len() >= 5);
    if let Some(f) = &spec.first_name {
        let sel: Vec<Vec<usize>> = runs.iter().filter(|r| &recs[r[0]].name == f).cloned().collect();
        if !sel.is_empty() {
            runs = sel;
        }
    }
    let mut best: Option<&Vec<usize>> = None;
    for r in &runs {
        if best.map(|b| r.len() > b.len()).unwrap_or(true) {
            best = Some(r);
        }
    }
    let run = best.ok_or_else(|| err!("no bone chain with prefix {}", spec.prefix))?;
    let mut out = Vec::new();
    for (k, &i) in run.iter().enumerate() {
        let prev = &recs[i - 1];
        let h = hier2(d, prev.payload, prev.size as usize)?;
        out.push(ChainBone { idx: k, name: recs[i].name[..recs[i].name.len() - 4].to_string(), h });
    }
    Ok(out)
}

/// `creature_rig.build_rig`
pub fn build_rig(d: &[u8], spec: &RigSpec) -> Result<Vec<Bone>> {
    let ch = bone_chain(d, spec)?;
    let seq: Vec<&ChainBone> = ch.iter().filter(|b| !spec.helpers.iter().any(|h| b.name.contains(h.as_str()))).collect();
    let skinseq: Vec<&ChainBone> = ch.iter().filter(|b| !spec.skinskip.iter().any(|h| b.name.contains(h.as_str()))).collect();
    let base = ch.iter().filter_map(|b| b.h.as_ref()).map(|h| h.0).filter(|k| k >> 24 == 0x8f).min().ok_or_else(|| err!("no bone hierarchy keys"))?;
    let keymap: HashMap<u32, usize> = seq.iter().enumerate().map(|(i, b)| (base + i as u32, b.idx)).collect();
    let sid: HashMap<usize, usize> = skinseq.iter().enumerate().map(|(i, b)| (b.idx, i)).collect();
    Ok(ch
        .iter()
        .map(|b| {
            let (parent, local, parent_key) = match &b.h {
                Some((k, m)) => (keymap.get(k).copied(), *m, Some(*k)),
                None => (None, mat::IDENT, None),
            };
            Bone { idx: b.idx, name: b.name.clone(), parent, local, skin_id: sid.get(&b.idx).copied(), parent_key }
        })
        .collect())
}

pub struct Check {
    /// bone name -> max |inv(skin) - world|
    pub err: Vec<(String, f64)>,
    pub skin: BTreeMap<u16, M4>,
    pub world: Vec<M4>,
}

/// `creature_rig.check`
pub fn check(d: &[u8], bones: &[Bone], geo_offs: &[usize]) -> Result<Check> {
    let mut skin: BTreeMap<u16, M4> = BTreeMap::new();
    for &o in geo_offs {
        let g = geo::parse_geo(d, o)?;
        for l in g.skin.as_ref().ok_or_else(|| err!("GEO {o:#x} has no skin"))? {
            skin.insert(l.bone, mat::from_f32(&l.mat));
        }
    }
    let world = world_matrices(bones);
    let mut errs = Vec::new();
    for b in bones {
        if let Some(s) = b.skin_id.and_then(|s| skin.get(&(s as u16))) {
            let inv = mat::inv(s).ok_or_else(|| err!("singular skin matrix"))?;
            let e = inv.iter().zip(&world[b.idx]).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
            errs.push((b.name.clone(), e));
        }
    }
    Ok(Check { err: errs, skin, world })
}

// ------------------------------------------------------------------ quaternion helpers (export_creature.py)

pub type Q4 = [f64; 4];
pub fn qmul(a: &Q4, b: &Q4) -> Q4 {
    let (x1, y1, z1, w1) = (a[0], a[1], a[2], a[3]);
    let (x2, y2, z2, w2) = (b[0], b[1], b[2], b[3]);
    [w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2, w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2, w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2, w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2]
}
pub fn qconj(q: &Q4) -> Q4 {
    [-q[0], -q[1], -q[2], q[3]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn qrot(q: &Q4, v: [f64; 3]) -> [f64; 3] {
    let u = [q[0], q[1], q[2]];
    let c1 = cross(u, v);
    let inner = [c1[0] + q[3] * v[0], c1[1] + q[3] * v[1], c1[2] + q[3] * v[2]];
    let c2 = cross(u, inner);
    [v[0] + 2.0 * c2[0], v[1] + 2.0 * c2[1], v[2] + 2.0 * c2[2]]
}
fn dot4(a: &Q4, b: &Q4) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}
pub fn slerp(a: &Q4, b: &Q4, t: f64) -> Q4 {
    let mut b = *b;
    let mut d = dot4(a, &b);
    if d < 0.0 {
        b = [-b[0], -b[1], -b[2], -b[3]];
        d = -d;
    }
    let r: Q4 = if d > 0.9995 {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
    } else {
        let th = d.acos();
        let (s0, s1, s) = (((1.0 - t) * th).sin(), (t * th).sin(), th.sin());
        [(s0 * a[0] + s1 * b[0]) / s, (s0 * a[1] + s1 * b[1]) / s, (s0 * a[2] + s1 * b[2]) / s, (s0 * a[3] + s1 * b[3]) / s]
    };
    let n = dot4(&r, &r).sqrt();
    [r[0] / n, r[1] / n, r[2] / n, r[3] / n]
}

/// Sorted (frame, value) keys with interpolation (`export_creature.Chan`). Translations use the first three components.
#[derive(Clone, Debug)]
pub struct Chan {
    pub quat: bool,
    pub t: Vec<f64>,
    pub v: Vec<Q4>,
}
impl Chan {
    pub fn at(&self, t: f64) -> Q4 {
        let n = self.t.len();
        if t <= self.t[0] {
            return self.v[0];
        }
        if t >= self.t[n - 1] {
            return self.v[n - 1];
        }
        let i = self.t.partition_point(|&x| x <= t) - 1;
        let f = (t - self.t[i]) / (self.t[i + 1] - self.t[i]);
        let (a, b) = (&self.v[i], &self.v[i + 1]);
        if self.quat {
            slerp(a, b, f)
        } else {
            [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, 0.0]
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct GzChan {
    pub t: Option<Chan>,
    pub q: Option<Chan>,
}

/// `parse_clip`: gizmo -> channels, plus the frame count.
pub fn parse_clip(r: &Trl) -> Result<(BTreeMap<i32, GzChan>, u32)> {
    let mut out: BTreeMap<i32, GzChan> = BTreeMap::new();
    for tr in &r.tracks {
        let mut tt = 0u32;
        let mut tl: Vec<(u32, Q4)> = Vec::new();
        let mut ql: Vec<(u32, Q4)> = Vec::new();
        for ev in &tr.events {
            if let Some(k) = &ev.key {
                if let Some(t) = &k.t {
                    let v = t[0];
                    tl.push((tt, [v[0] as f64, v[1] as f64, v[2] as f64, 0.0]));
                }
                match &k.q {
                    Some(Rot::Q(q)) => ql.push((tt, *q)),
                    Some(Rot::Mat(_)) => return Err(err!("clip: matrix rotation key (gizmo {})", tr.gizmo)),
                    None => {}
                }
            }
            tt += ev.nf;
        }
        let dd = |l: Vec<(u32, Q4)>| -> Vec<(u32, Q4)> {
            let mut m: BTreeMap<u32, Q4> = BTreeMap::new();
            for (f, v) in l {
                m.insert(f, v);
            }
            m.into_iter().collect()
        };
        let tl = dd(tl);
        let mut ql = dd(ql);
        for i in 1..ql.len() {
            if dot4(&ql[i].1, &ql[i - 1].1) < 0.0 {
                let v = ql[i].1;
                ql[i].1 = [-v[0], -v[1], -v[2], -v[3]];
            }
        }
        let e = out.entry(tr.gizmo).or_default();
        if !tl.is_empty() {
            e.t = Some(Chan { quat: false, t: tl.iter().map(|x| x.0 as f64).collect(), v: tl.iter().map(|x| x.1).collect() });
        }
        if !ql.is_empty() {
            e.q = Some(Chan { quat: true, t: ql.iter().map(|x| x.0 as f64).collect(), v: ql.iter().map(|x| x.1).collect() });
        }
    }
    let frames = r.tracks.iter().map(|t| anim::track_times(t).1).max().unwrap_or(0);
    Ok((out, frames))
}

/// Exported channel of one gizmo: (frames, values).
#[derive(Clone, Debug, Default)]
pub struct GzOut {
    pub t: Option<(Vec<f64>, Vec<[f64; 3]>)>,
    pub q: Option<(Vec<f64>, Vec<Q4>)>,
}

#[derive(Clone, Debug, Default)]
pub struct RootMotion {
    pub disp: [f64; 3],
    pub yaw_deg: f64,
    pub keys_actor: usize,
}

/// `inplace_clip`: remove gizmo -1; `bind0` = (t, q) of the pelvis bind (used when the pelvis lacks a channel).
pub fn inplace_clip(ch: &BTreeMap<i32, GzChan>, bind0: ([f64; 3], Q4)) -> (BTreeMap<i32, GzOut>, RootMotion) {
    let mut res: BTreeMap<i32, GzOut> = BTreeMap::new();
    for (g, e) in ch {
        if *g == -1 {
            continue;
        }
        res.insert(
            *g,
            GzOut {
                t: e.t.as_ref().map(|c| (c.t.clone(), c.v.iter().map(|v| [v[0], v[1], v[2]]).collect())),
                q: e.q.as_ref().map(|c| (c.t.clone(), c.v.clone())),
            },
        );
    }
    let mut rm = RootMotion::default();
    if let Some(root) = ch.get(&-1) {
        let (rt, rq) = (root.t.as_ref(), root.q.as_ref());
        rm.keys_actor = rt.map(|c| c.t.len()).unwrap_or(0).max(rq.map(|c| c.t.len()).unwrap_or(0));
        let t0 = rt.map(|c| c.v[0]).unwrap_or([0.0; 4]);
        let t1 = rt.map(|c| *c.v.last().unwrap()).unwrap_or([0.0; 4]);
        rm.disp = [t1[0] - t0[0], t1[1] - t0[1], t1[2] - t0[2]];
        if let Some(rq) = rq {
            let q0 = rq.v[0];
            let q1 = *rq.v.last().unwrap();
            let mut dq = qmul(&q1, &qconj(&q0));
            if dq[3] < 0.0 {
                dq = [-dq[0], -dq[1], -dq[2], -dq[3]];
            }
            rm.yaw_deg = (2.0 * dq[2].atan2(dq[3])).to_degrees();
        }
        if let Some(p) = ch.get(&0) {
            let mut times: Vec<f64> = Vec::new();
            for c in [p.t.as_ref(), p.q.as_ref(), rt, rq].into_iter().flatten() {
                times.extend_from_slice(&c.t);
            }
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            times.dedup();
            let mut tv = Vec::new();
            let mut qv: Vec<Q4> = Vec::new();
            for &t in &times {
                let pt = p.t.as_ref().map(|c| c.at(t)).unwrap_or([bind0.0[0], bind0.0[1], bind0.0[2], 0.0]);
                let pq = p.q.as_ref().map(|c| c.at(t)).unwrap_or(bind0.1);
                let r = rq.map(|c| c.at(t)).unwrap_or([0.0, 0.0, 0.0, 1.0]);
                let tt = rt.map(|c| c.at(t)).unwrap_or([0.0; 4]);
                tv.push(qrot(&qconj(&r), [pt[0] - tt[0], pt[1] - tt[1], pt[2] - tt[2]]));
                qv.push(qmul(&qconj(&r), &pq));
            }
            for i in 1..qv.len() {
                if dot4(&qv[i], &qv[i - 1]) < 0.0 {
                    qv[i] = [-qv[i][0], -qv[i][1], -qv[i][2], -qv[i][3]];
                }
            }
            res.insert(0, GzOut { t: Some((times.clone(), tv)), q: Some((times, qv)) });
        }
    }
    (res, rm)
}

// ------------------------------------------------------------------ action table (export_creature.action_table)

pub struct ActionItem {
    pub trl: Option<usize>,
    pub b: [u8; 7],
}
pub struct Action {
    pub rec: usize,
    pub flag: u8,
    pub items: Vec<ActionItem>,
}
pub struct ActionTable {
    pub trl: Vec<usize>,
    pub kit: Vec<u32>,
    recs: Vec<(usize, u8, Vec<(u32, [u8; 7])>)>,
    rank: HashMap<u32, usize>,
    trlidx: HashMap<u32, usize>,
    pub n_seen: usize,
}

impl ActionTable {
    pub fn load(d: &[u8], kit_off: usize, kit_n: usize, rec_off: usize, trl_start: usize, trl_end: usize) -> Result<ActionTable> {
        let mut recs = Vec::new();
        let mut p = rec_off;
        loop {
            let sz = rd_u32(d, p)? as usize;
            let n = *d.get(p + 4).ok_or_else(|| err!("action record past end"))? as usize;
            if sz < 2 || sz > 2000 || n == 0 || n > 8 || sz < 2 + 19 * n {
                break;
            }
            let flag = d[p + 5];
            let mut items = Vec::new();
            for i in 0..n {
                let q = p + 6 + 19 * i;
                let k0 = rd_u32(d, q)?;
                let mut b = [0u8; 7];
                b.copy_from_slice(d.get(q + 12..q + 19).ok_or_else(|| err!("action item past end"))?);
                items.push((k0, b));
            }
            recs.push((p, flag, items));
            p += 4 + sz;
        }
        // chain of track lists walked by size
        let mut trl = Vec::new();
        let mut o = trl_start;
        while o < trl_end {
            let sz = rd_u32(d, o)? as usize;
            trl.push(o);
            o += 4 + sz;
        }
        if o != trl_end {
            return Err(err!("TRL chain does not end at {trl_end:#x} (ends at {o:#x})"));
        }
        let mut seen: Vec<u32> = Vec::new();
        for r in &recs {
            for it in &r.2 {
                if it.0 != 0 && it.0 != 0xffff_ffff && !seen.contains(&it.0) {
                    seen.push(it.0);
                }
            }
        }
        let trlidx: HashMap<u32, usize> = seen.iter().enumerate().map(|(i, &k)| (k, i)).collect();
        let mut kit = Vec::with_capacity(kit_n);
        for i in 0..kit_n {
            kit.push(rd_u32(d, kit_off + 4 * i)?);
        }
        let mut rank = HashMap::new();
        let mut order = 0;
        for &k in &kit {
            if k == 0 || k == 0xffff_ffff || k == 1 {
                continue;
            }
            rank.entry(k).or_insert_with(|| {
                order += 1;
                order - 1
            });
        }
        Ok(ActionTable { trl, kit, recs, rank, trlidx, n_seen: seen.len() })
    }

    pub fn action(&self, i: usize) -> Option<Action> {
        let k = self.kit[i];
        if k == 0 || k == 0xffff_ffff || k == 1 {
            return None;
        }
        let r = self.recs.get(*self.rank.get(&k)?)?;
        Some(Action { rec: r.0, flag: r.1, items: r.2.iter().map(|(k0, b)| ActionItem { trl: self.trlidx.get(k0).copied(), b: *b }).collect() })
    }
}

// ------------------------------------------------------------------ glb (export_creature.build_glb)

pub struct Part {
    pub geo_off: usize,
    pub name: String,
    /// bank ordinals of the diffuse / normal textures; None = flat grey placeholder (Ann)
    pub diffuse: Option<usize>,
    pub normal: Option<usize>,
}

pub struct ClipOut {
    pub name: String,
    pub ch: BTreeMap<i32, GzOut>,
    pub extras: Value,
}

fn flat_png(rgb: [u8; 3]) -> Result<Vec<u8>> {
    let data: Vec<u8> = (0..16).flat_map(|_| rgb).collect();
    texture::encode_png(4, 4, png::ColorType::Rgb, &data)
}

pub fn build_glb(
    src: &mut dyn Source,
    bank_key: Option<&str>,
    d: &[u8],
    bones: &[Bone],
    skin_mats: &BTreeMap<u16, M4>,
    parts: &[Part],
    clips: &[ClipOut],
    name: &str,
    extras: Value,
    with_textures: bool,
    log: &mut dyn FnMut(&str),
) -> Result<Vec<u8>> {
    let mut glb = Glb::default();
    glb.nodes.push(json!({"name": "JadeZup_to_GltfYup", "rotation": [-0.70710678, 0.0, 0.0, 0.70710678], "children": [1]}));
    glb.scene_roots.push(0);
    glb.nodes.push(json!({"name": "JadeActor", "children": [], "extras": {"note": "gizmo -1 track (root motion) animates this node; clips here are in place (see rootmotion json)"}}));
    let mut node: HashMap<usize, usize> = HashMap::new();
    for b in bones {
        let (t, q, s) = mat::decompose(&b.local).map_err(|e| err!("{name}: bone {}: {e}", b.name))?;
        let mut n = json!({
            "name": b.name,
            "translation": [jd(t[0]), jd(t[1]), jd(t[2])],
            "rotation": [jd(q[0]), jd(q[1]), jd(q[2]), jd(q[3])],
            "extras": {"jade_listing_index": b.idx, "jade_skin_id": b.skin_id}
        });
        if s.iter().any(|x| (x - 1.0).abs() > 1e-3) {
            n["scale"] = json!([jd(s[0]), jd(s[1]), jd(s[2])]);
        }
        node.insert(b.idx, glb.nodes.len());
        glb.nodes.push(n);
    }
    for b in bones {
        let ni = node[&b.idx];
        let par = match b.parent {
            None => 1,
            Some(p) => node[&p],
        };
        let kids = glb.nodes[par].as_object_mut().unwrap().entry("children").or_insert_with(|| json!([]));
        kids.as_array_mut().unwrap().push(json!(ni));
    }
    let mut skinned: Vec<&Bone> = bones.iter().filter(|b| b.skin_id.map(|s| skin_mats.contains_key(&(s as u16))).unwrap_or(false)).collect();
    skinned.sort_by_key(|b| b.skin_id);
    let jindex: HashMap<usize, usize> = skinned.iter().enumerate().map(|(k, b)| (b.skin_id.unwrap(), k)).collect();
    let ibm_data: Vec<f32> = skinned.iter().flat_map(|b| skin_mats[&(b.skin_id.unwrap() as u16)].iter().map(|&x| x as f32).collect::<Vec<_>>()).collect();
    let ibm = glb.acc_f32(&ibm_data, 16, "MAT4", None, false);
    glb.skins.push(json!({
        "name": format!("{name}_skin"),
        "joints": skinned.iter().map(|b| node[&b.idx]).collect::<Vec<_>>(),
        "inverseBindMatrices": ibm, "skeleton": 1
    }));
    glb.samplers = vec![json!({"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497})];

    // bank textures decoded once per ordinal, embedded once per (ordinal, kind)
    let bank = match (bank_key, with_textures) {
        (Some(k), true) if parts.iter().any(|p| p.diffuse.is_some()) => Some(src.bank(k)?),
        _ => None,
    };
    let mut tex_cache: HashMap<(usize, bool), usize> = HashMap::new();
    let mut flat_cache: HashMap<bool, usize> = HashMap::new();
    let mut decoded: HashMap<usize, texture::Image> = HashMap::new();

    let (mut pos, mut nrm, mut uvs): (Vec<f32>, Vec<f32>, Vec<f32>) = (vec![], vec![], vec![]);
    let (mut jn, mut wt): (Vec<u16>, Vec<f32>) = (vec![], vec![]);
    let mut prim_idx: Vec<(Vec<u32>, usize)> = Vec::new();
    let mut base = 0u32;
    for part in parts {
        let g: Geo = geo::parse_geo(d, part.geo_off).map_err(|e| err!("{name}: {} GEO {:#x}: {e}", part.name, part.geo_off))?;
        let flat = geo::flatten(&g)?;
        let skin = g.skin.as_ref().ok_or_else(|| err!("{name}: GEO {:#x} has no skin", part.geo_off))?;
        let mut infl: Vec<Vec<(f32, u16)>> = vec![Vec::new(); g.nverts];
        for l in skin {
            let j = *jindex.get(&(l.bone as usize)).ok_or_else(|| err!("{name}: skin list for unknown bone {}", l.bone))?;
            for (&vi, &w) in l.idx.iter().zip(&l.w) {
                if (vi as usize) < infl.len() {
                    infl[vi as usize].push((w, j as u16));
                }
            }
        }
        let mut jv = vec![[0u16; 4]; g.nverts];
        let mut wv = vec![[0f32; 4]; g.nverts];
        for (v, lst) in infl.iter_mut().enumerate() {
            if lst.is_empty() {
                continue;
            }
            lst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(b.1.cmp(&a.1)));
            lst.truncate(4);
            let tot: f64 = lst.iter().map(|x| x.0 as f64).sum();
            for (k, (w, j)) in lst.iter().enumerate() {
                jv[v][k] = *j;
                wv[v][k] = (*w as f64 / tot) as f32;
            }
        }
        pos.extend(flat.pos.iter().flatten());
        nrm.extend(flat.nrm.iter().flatten());
        uvs.extend(flat.uv.iter().flatten());
        jn.extend(flat.src.iter().flat_map(|&s| jv[s]));
        wt.extend(flat.src.iter().flat_map(|&s| wv[s]));
        // material
        let mi = glb.materials.len();
        let mut m = json!({
            "name": part.name,
            "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.0, "roughnessFactor": 0.85},
            "doubleSided": true,
            "extras": {"geo": format!("{:#x}", part.geo_off)}
        });
        if let (Some(di), Some(ni)) = (part.diffuse, part.normal) {
            let bk = bank_key.unwrap_or("");
            m["extras"]["kk_diffuse_png"] = json!(format!("idx{di:03}.png"));
            m["extras"]["kk_normal_png"] = json!(format!("idx{ni:03}.png"));
            if let Some(b) = &bank {
                let mut tex = |glb: &mut Glb, idx: usize, normal: bool| -> Result<usize> {
                    if let Some(&t) = tex_cache.get(&(idx, normal)) {
                        return Ok(t);
                    }
                    if !decoded.contains_key(&idx) {
                        log(&format!("decoding {bk} texture {idx}"));
                        decoded.insert(idx, texture::bank_texture(b, idx).map_err(|e| err!("{bk} idx{idx:03}: {e}"))?);
                    }
                    let img = &decoded[&idx];
                    let png = if normal { img.to_png_normal()? } else { img.to_png_rgb()? };
                    let t = glb.add_png_texture(&format!("{}_idx{idx:03}", if normal { "normal" } else { "diffuse" }), &png);
                    tex_cache.insert((idx, normal), t);
                    Ok(t)
                };
                let dt = tex(&mut glb, di, false)?;
                let nt = tex(&mut glb, ni, true)?;
                m["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": dt});
                m["normalTexture"] = json!({"index": nt});
            }
        } else if part.diffuse.is_none() && with_textures {
            // flat placeholder textures (Ann: no texture binding was decoded)
            let dt = match flat_cache.get(&false) {
                Some(&t) => t,
                None => {
                    let t = glb.add_png_texture("diffuse_flat", &flat_png([150, 120, 105])?);
                    flat_cache.insert(false, t);
                    t
                }
            };
            let nt = match flat_cache.get(&true) {
                Some(&t) => t,
                None => {
                    // tex_png(normal=True) of a (128,128,255) image
                    let x = 128.0f32 / 255.0 * 2.0 - 1.0;
                    let z = (1.0f32 - x * x - x * x).clamp(0.0, 1.0).sqrt();
                    let f = |v: f32| (v * 0.5 * 255.0 + 127.5) as u8;
                    let t = glb.add_png_texture("normal_flat", &flat_png([f(x), f(x), f(z)])?);
                    flat_cache.insert(true, t);
                    t
                }
            };
            m["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": dt});
            m["normalTexture"] = json!({"index": nt});
        }
        glb.materials.push(m);
        for (_, idx) in &flat.elems {
            prim_idx.push((idx.iter().map(|&i| i + base).collect(), mi));
        }
        base += flat.pos.len() as u32;
    }
    let a = glb.acc_f32(&pos, 3, "VEC3", Some(34962), true);
    let n = glb.acc_f32(&nrm, 3, "VEC3", Some(34962), false);
    let t = glb.acc_f32(&uvs, 2, "VEC2", Some(34962), false);
    let attrs = json!({"POSITION": a, "NORMAL": n, "TEXCOORD_0": t,
        "JOINTS_0": glb.acc_u16(&jn, 4, "VEC4", Some(34962)), "WEIGHTS_0": glb.acc_f32(&wt, 4, "VEC4", Some(34962), false)});
    let mut prims = Vec::new();
    for (idx, mi) in &prim_idx {
        if idx.is_empty() {
            continue;
        }
        let ia = glb.acc_u32(idx, Some(34963));
        prims.push(json!({"attributes": attrs, "indices": ia, "material": mi, "mode": 4}));
    }
    glb.meshes.push(json!({"name": format!("{name}_mesh"), "primitives": prims}));
    glb.nodes.push(json!({"name": format!("{name}_mesh"), "mesh": 0, "skin": 0}));
    let mesh_node = glb.nodes.len() - 1;
    glb.nodes[0]["children"].as_array_mut().unwrap().push(json!(mesh_node));

    for c in clips {
        let mut samplers = Vec::new();
        let mut channels = Vec::new();
        let mut ex = c.extras.clone();
        let mut missing: Vec<i32> = Vec::new();
        for (gz, e) in &c.ch {
            let Some(&ni) = node.get(&(*gz as usize)).filter(|_| *gz >= 0) else {
                missing.push(*gz);
                continue;
            };
            if let Some((ts, vs)) = &e.q {
                if !ts.is_empty() {
                    let times: Vec<f32> = ts.iter().map(|&f| (f / FPS) as f32).collect();
                    let vals: Vec<f32> = vs.iter().flatten().map(|&x| x as f32).collect();
                    let input = glb.acc_f32(&times, 1, "SCALAR", None, true);
                    let output = glb.acc_f32(&vals, 4, "VEC4", None, false);
                    samplers.push(json!({"input": input, "output": output, "interpolation": "LINEAR"}));
                    channels.push(json!({"sampler": samplers.len() - 1, "target": {"node": ni, "path": "rotation"}}));
                }
            }
            if let Some((ts, vs)) = &e.t {
                if !ts.is_empty() {
                    let times: Vec<f32> = ts.iter().map(|&f| (f / FPS) as f32).collect();
                    let vals: Vec<f32> = vs.iter().flatten().map(|&x| x as f32).collect();
                    let input = glb.acc_f32(&times, 1, "SCALAR", None, true);
                    let output = glb.acc_f32(&vals, 3, "VEC3", None, false);
                    samplers.push(json!({"input": input, "output": output, "interpolation": "LINEAR"}));
                    channels.push(json!({"sampler": samplers.len() - 1, "target": {"node": ni, "path": "translation"}}));
                }
            }
        }
        if !missing.is_empty() {
            ex["missing_gizmos"] = json!(missing);
        }
        glb.animations.push(json!({"name": c.name, "samplers": samplers, "channels": channels, "extras": ex}));
    }
    Ok(glb.finish(extras, "kk-extract"))
}

// ------------------------------------------------------------------ recipes

fn sstr<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().ok_or_else(|| err!("recipe: missing string '{k}'"))
}
fn num(v: &Value, k: &str) -> Result<usize> {
    parse_num(sstr(v, k)?)
}

fn round_to(x: f64, n: i32) -> f64 {
    let p = 10f64.powi(n);
    (x * p).round_ties_even() / p
}

fn load_parts(recipe: &Value) -> Result<Vec<Part>> {
    recipe["parts"]
        .as_array()
        .ok_or_else(|| err!("recipe: no parts"))?
        .iter()
        .map(|p| {
            Ok(Part {
                geo_off: num(p, "geo_off")?,
                name: sstr(p, "name")?.to_string(),
                diffuse: p["diffuse"].as_u64().map(|x| x as usize),
                normal: p["normal"].as_u64().map(|x| x as usize),
            })
        })
        .collect()
}

fn rootmotion_entry(dur: f64, disp: [f64; 3], rm: &RootMotion) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("duration_s".into(), jd(dur));
    m.insert("jade_displacement".into(), json!([jd(disp[0]), jd(disp[1]), jd(disp[2])]));
    m.insert("gltf_displacement".into(), json!([jd(disp[0]), jd(disp[2]), jd(-disp[1])]));
    m.insert("yaw_deg".into(), jd(rm.yaw_deg));
    m.insert("speed_mps".into(), jd(if dur > 0.0 { disp[0].hypot(disp[1]) / dur } else { 0.0 }));
    m.insert("keys_actor".into(), json!(rm.keys_actor));
    m
}

fn loop_guess(chn: &BTreeMap<i32, GzOut>) -> (f64, f64) {
    let mut ang = Vec::new();
    for e in chn.values() {
        let Some((ts, vs)) = &e.q else { continue };
        if ts.len() < 2 {
            continue;
        }
        let a = &vs[0];
        let b = vs.last().unwrap();
        ang.push((2.0 * dot4(a, b).abs().min(1.0).acos()).to_degrees());
    }
    let mut dt = 0.0;
    if let Some(GzOut { t: Some((_, v)), .. }) = chn.get(&0) {
        let (a, b) = (v[0], v.last().unwrap());
        dt = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
    }
    (if ang.is_empty() { 0.0 } else { ang.iter().sum::<f64>() / ang.len() as f64 }, dt)
}

/// Common prelude: rig, skin matrices, root fix.
struct Prepared {
    bones: Vec<Bone>,
    check: Check,
    t0: [f64; 3],
    q0: Q4,
    max_err: f64,
}

fn prepare(d: &[u8], recipe: &Value, parts: &[Part], name: &str) -> Result<Prepared> {
    let spec = RigSpec::from_recipe(&recipe["rig"])?;
    let mut bones = build_rig(d, &spec)?;
    // parent overrides (Ann: Meche02G -> Meche01G ...)
    if let Some(rp) = recipe["rig"]["reparent"].as_array() {
        let by_name: HashMap<String, usize> = bones.iter().map(|b| (b.name.strip_prefix(spec.prefix.as_str()).unwrap_or(&b.name).to_string(), b.idx)).collect();
        for pair in rp {
            let (a, p) = (pair[0].as_str().unwrap_or(""), pair[1].as_str().unwrap_or(""));
            let (Some(&ai), Some(&pi)) = (by_name.get(a), by_name.get(p)) else { return Err(err!("{name}: reparent {a} -> {p}: unknown bone")) };
            bones[ai].parent = Some(pi);
        }
    }
    let offs: Vec<usize> = parts.iter().map(|p| p.geo_off).collect();
    let mut ck = check(d, &bones, &offs)?;
    // root: world(first skinned bone 0) == inv(skin[0])
    let s0 = ck.skin.get(&0).ok_or_else(|| err!("{name}: no skin matrix 0"))?;
    bones[0].local = mat::inv(s0).ok_or_else(|| err!("{name}: singular skin matrix 0"))?;
    ck = check(d, &bones, &offs)?;
    if recipe["rig"]["torsion_fix"].as_bool().unwrap_or(false) {
        let errs: HashMap<&str, f64> = ck.err.iter().map(|(n, e)| (n.as_str(), *e)).collect();
        let mut fixes = Vec::new();
        for b in &bones {
            if errs.get(b.name.as_str()).copied().unwrap_or(0.0) > 1e-3 && b.name.contains("Torsion") {
                if let (Some(sid), Some(p)) = (b.skin_id, b.parent) {
                    if let Some(s) = ck.skin.get(&(sid as u16)) {
                        let l = mat::mul(&mat::inv(s).ok_or_else(|| err!("singular"))?, &mat::inv(&ck.world[p]).ok_or_else(|| err!("singular"))?);
                        fixes.push((b.idx, l));
                    }
                }
            }
        }
        for (i, l) in fixes {
            bones[i].local = l;
        }
        ck = check(d, &bones, &offs)?;
    }
    let (t0, q0, _) = mat::decompose(&bones[0].local).map_err(|e| err!("{name}: root: {e}"))?;
    let skip_names = recipe["rig"]["err_skip"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()).unwrap_or_default();
    let max_err = ck.err.iter().filter(|(n, _)| !skip_names.iter().any(|s| n.contains(s.as_str()))).map(|x| x.1).fold(0.0, f64::max);
    Ok(Prepared { bones, check: ck, t0, q0, max_err })
}

fn labels_table(which: &str) -> Result<HashMap<usize, (String, String, String)>> {
    let text = match which {
        "kong" => include_str!("../data/kong_labels.json"),
        _ => return Err(err!("unknown label table '{which}'")),
    };
    let v: Value = serde_json::from_str(text)?;
    Ok(v.as_object()
        .ok_or_else(|| err!("labels: not an object"))?
        .iter()
        .map(|(k, a)| (k.parse::<usize>().unwrap_or(usize::MAX), (a[0].as_str().unwrap_or("").to_string(), a[1].as_str().unwrap_or("").to_string(), a[2].as_str().unwrap_or("").to_string())))
        .collect())
}

/// `kind: "kong"` (kit-driven clip chain, action table) and `kind: "ann"` (plain TRL chain).
pub fn build_kong_like(src: &mut dyn Source, name: &str, recipe: &Value, opts: &BuildOpts, log: &mut dyn FnMut(&str)) -> Result<Built> {
    let stream = sstr(recipe, "stream")?;
    let model = sstr(recipe, "model")?; // "kong" / "ann": mesh_node + clip name prefix
    log(&format!("loading stream {stream}"));
    let dd = src.stream(stream)?;
    let d: &[u8] = &dd;
    let parts = load_parts(recipe)?;
    let prep = prepare(d, recipe, &parts, name)?;
    let mut clips: Vec<ClipOut> = Vec::new();
    let mut rootmotion = Map::new();
    let mut actions = Map::new();
    let lenient = true;
    if let Some(kit) = recipe.get("kit") {
        let tab = ActionTable::load(d, num(kit, "kit_off")?, kit["kit_n"].as_u64().unwrap_or(371) as usize, num(kit, "rec_off")?, num(kit, "trl_start")?, num(kit, "trl_end")?)?;
        let labels = labels_table(sstr(recipe, "labels")?)?;
        let mut lab: HashMap<usize, (String, String, String)> = HashMap::new();
        let mut acts: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..tab.kit.len() {
            let Some(a) = tab.action(i) else { continue };
            let tl: Vec<usize> = a.items.iter().filter_map(|x| x.trl).collect();
            if let Some(l) = labels.get(&i) {
                if let Some(&first) = tl.first() {
                    lab.entry(first).or_insert_with(|| l.clone());
                }
            }
            acts.insert(i, tl);
        }
        struct Info {
            name: String,
            frames: u32,
            loop_: bool,
        }
        let mut info: HashMap<usize, Info> = HashMap::new();
        for (ti, &o) in tab.trl.iter().enumerate() {
            let r = anim::parse_trl_with(d, o, lenient).map_err(|e| err!("{name}: TRL {ti} @ {o:#x}: {}", e.0))?;
            let (ch, fr) = parse_clip(&r)?;
            let (chn, rm) = inplace_clip(&ch, (prep.t0, prep.q0));
            let lb = lab.get(&ti);
            let cname = match lb {
                Some(l) => format!("{}__{model}_{ti:03}", l.0),
                None => format!("{model}_{ti:03}"),
            };
            let (ang, dt) = loop_guess(&chn);
            let isloop = ang < 4.0 && dt < 0.15;
            let lp = isloop && lb.map(|l| matches!(l.0.as_str(), "idle" | "walk" | "run")).unwrap_or(true);
            let dur = fr as f64 / FPS;
            let mut e = rootmotion_entry(dur, rm.disp, &rm);
            let pel = chn.get(&0).and_then(|g| g.t.as_ref());
            e.insert("pelvis_start_jade".into(), pel.map(|(_, v)| json!(v[0])).unwrap_or(Value::Null));
            e.insert("pelvis_end_jade".into(), pel.map(|(_, v)| json!(v.last().unwrap())).unwrap_or(Value::Null));
            rootmotion.insert(cname.clone(), Value::Object(e));
            let ex = json!({
                "source": format!("{stream}.bin@{o:#x}"), "trl_index": ti, "frames": fr, "fps": FPS, "seconds": round_to(dur, 4), "loop": lp,
                "pose_loop_err_deg": round_to(ang, 2), "pose_loop_dt_m": round_to(dt, 3),
                "label": lb.map(|l| l.0.clone()), "label_confidence": lb.map(|l| l.1.clone()), "label_evidence": lb.map(|l| l.2.clone())
            });
            info.insert(ti, Info { name: cname.clone(), frames: fr, loop_: lp });
            clips.push(ClipOut { name: cname, ch: chn, extras: ex });
        }
        for (i, tl) in &acts {
            let Some(&first) = tl.first() else { continue };
            let f = &info[&first];
            let lbl = labels.get(i);
            actions.insert(
                format!("{i:#x}"),
                json!({
                    "clip_name": f.name, "frames": f.frames,
                    "loop": if lbl.map(|l| matches!(l.0.as_str(), "idle" | "walk" | "run")).unwrap_or(true) { f.loop_ } else { false },
                    "confidence": lbl.map(|l| l.1.clone()).unwrap_or_else(|| "unlabelled".into()),
                    "label": lbl.map(|l| l.0.clone()),
                    "items": tl.iter().map(|t| json!({"clip_name": info[t].name, "frames": info[t].frames})).collect::<Vec<_>>()
                }),
            );
        }
    } else {
        let start = num(&recipe["chain"], "start")?;
        let end = num(&recipe["chain"], "end")?;
        let mut o = start;
        let mut ti = 0;
        while o < end {
            let r = anim::parse_trl_with(d, o, lenient).map_err(|e| err!("{name}: TRL {ti} @ {o:#x}: {}", e.0))?;
            let (ch, fr) = parse_clip(&r)?;
            let (chn, rm) = inplace_clip(&ch, (prep.t0, prep.q0));
            let cname = format!("{model}_{ti:03}");
            let dur = fr as f64 / FPS;
            rootmotion.insert(cname.clone(), Value::Object(rootmotion_entry(dur, rm.disp, &rm)));
            let ex = json!({"source": format!("{stream}.bin@{o:#x}"), "frames": fr, "fps": FPS, "seconds": round_to(dur, 4)});
            clips.push(ClipOut { name: cname, ch: chn, extras: ex });
            o += 4 + r.size as usize;
            ti += 1;
        }
    }
    let extras = recipe["asset_extras"].clone();
    let bank = recipe["bank"].as_str();
    let glb = build_glb(src, bank, d, &prep.bones, &prep.check.skin, &parts, &clips, model, extras, opts.with_textures, log)?;
    let mut files = vec![(name.to_string(), glb)];
    if let Some(f) = recipe["outputs"]["rootmotion"].as_str() {
        files.push((f.to_string(), serde_json::to_vec_pretty(&Value::Object(rootmotion))?));
    }
    if let Some(f) = recipe["outputs"]["actions"].as_str() {
        files.push((f.to_string(), serde_json::to_vec_pretty(&Value::Object(actions))?));
    }
    if let Some(f) = recipe["outputs"]["fur_rli"].as_str() {
        files.push((f.to_string(), fur_rli(d, &parts, name)?));
    }
    let _: BTreeSet<i32> = BTreeSet::new();
    Ok(Built { files, notes: vec![format!("rig error max {:.2e}", prep.max_err)] })
}

/// Kong's per-vertex fur length mask: the alpha of each part's RLI record (`u32 size; "RLI\x80"; u32 count;
/// u32 colour[count]`, one D3DCOLOR per GEO vertex, stored right after the GEO in the stream), expanded to the
/// glb's render vertices (the same `flatten` order as the mesh). One byte per vertex, 0 = full length, 255 = bare
/// (`vsfur.hlsl`: `offset = g_fFurNormalOffset * (1 - RLI.a)`).
fn fur_rli(d: &[u8], parts: &[Part], name: &str) -> Result<Vec<u8>> {
    const TAG: [u8; 4] = [0x80, 0x52, 0x4c, 0x49];
    let mut out = Vec::new();
    for part in parts {
        let g: Geo = geo::parse_geo(d, part.geo_off).map_err(|e| err!("{name}: {} GEO {:#x}: {e}", part.name, part.geo_off))?;
        let flat = geo::flatten(&g)?;
        let end = (part.geo_off + 0x40_0000).min(d.len().saturating_sub(8));
        let mut found = None;
        let mut o = part.geo_off;
        while o < end {
            if d[o..o + 4] == TAG && u32::from_le_bytes([d[o + 4], d[o + 5], d[o + 6], d[o + 7]]) as usize == g.nverts {
                found = Some(o + 8);
                break;
            }
            o += 1;
        }
        let at = found.ok_or_else(|| err!("{name}: no RLI record with {} colours after {} GEO {:#x}", g.nverts, part.name, part.geo_off))?;
        if at + 4 * g.nverts > d.len() {
            return Err(err!("{name}: RLI of {} runs past the stream", part.name));
        }
        out.extend(flat.src.iter().map(|&v| d[at + 4 * v + 3]));
    }
    Ok(out)
}

#[allow(dead_code)]
fn unused(_: f32) -> Result<f32> {
    rd_f32(&[0; 4], 0)
}
