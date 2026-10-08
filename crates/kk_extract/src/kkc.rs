//! Creatures (raptor, compy, raptor at the Kong level, brontosaurus, crab): ports of `tools/kkc_export.py`, `kkc_lib.py`,
//! `kkc_geo.py`, `kkc_runs.py`, `kkc_anim.py` (findings: `creature_asset_findings.md`).
//!
//! * rig: chain of `B_<prefix>_*` records from the GAO hierarchy (`skel::hier`), parents refined against the skin
//!   inverse-bind matrices; the skin id -> bone mapping is the best of three hypotheses (listing order, listing order
//!   without Snap/Base/Sang, listing + 1)
//! * clips: back-to-back track lists from the clip bank offset(s); in place (the root track is dropped; when the
//!   pelvis track carries the travel too, the linear root ramp is subtracted from the pelvis horizontally)
//! * outputs: `<name>.glb`, `<name>_rootmotion.json`

use crate::anim::{self, Rot, Trl};
use crate::build::{parse_num, BuildOpts, Built, Source};
use crate::err;
use crate::error::Result;
use crate::geo::{self, Geo};
use crate::glb::{jd, Glb};
use crate::mat::{self, M4};
use crate::records::iter_named;
use crate::skel::{self, world_matrices, Bone};
use crate::texture;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};

const FPS: f64 = 60.0;

#[derive(Clone, Debug)]
struct KBone {
    idx: usize,
    name: String,
    parent: Option<usize>,
    local: M4,
    helper: bool,
}

/// kkc_lib.build_rig
fn build_rig(d: &[u8], prefix: &str, first_name_off: Option<usize>) -> Result<Vec<KBone>> {
    let all = iter_named(d, 0, None);
    let names: Vec<_> = all.iter().filter(|g| g.name.starts_with(prefix)).filter(|g| first_name_off.map(|f| g.name_off >= f).unwrap_or(true)).collect();
    let (first, last) = (names.first().ok_or_else(|| err!("no bones with prefix {prefix}"))?, names.last().unwrap());
    let recs = iter_named(d, first.name_off.saturating_sub(0x2000), Some(last.name_off + 0x100));
    let idx: Vec<usize> = recs.iter().enumerate().filter(|(_, g)| g.name.starts_with(prefix)).map(|(i, _)| i).collect();
    let mut bones: Vec<(usize, String, Option<skel::Hier>)> = Vec::new();
    for (k, &i) in idx.iter().enumerate() {
        // python indexes recs[i - 1]: for i == 0 that is the LAST record of the window
        let prev = &recs[(i + recs.len() - 1) % recs.len()];
        let h = skel::hier(d, prev.payload)?;
        bones.push((k, recs[i].name[..recs[i].name.len() - 4].to_string(), h));
    }
    let keys: Vec<u32> = bones.iter().filter_map(|b| b.2.as_ref()).map(|h| h.key).filter(|k| k >> 24 == 0x8f).collect();
    let base = *keys.iter().min().ok_or_else(|| err!("no bone hierarchy keys for {prefix}"))?;
    let n = bones.len() as u32;
    let helper: Vec<bool> = bones.iter().map(|b| b.2.as_ref().map(|h| !(base <= h.key && h.key < base + n + 4)).unwrap_or(false)).collect();
    let seq: Vec<usize> = bones.iter().enumerate().filter(|(i, _)| !helper[*i]).map(|(_, b)| b.0).collect();
    let keymap: HashMap<u32, usize> = seq.iter().enumerate().map(|(i, &b)| (base + i as u32, b)).collect();
    Ok(bones
        .iter()
        .enumerate()
        .map(|(i, (idx, name, h))| {
            let (parent, local) = match h {
                Some(h) if !helper[i] => (keymap.get(&h.key).copied(), h.m2),
                Some(h) => (None, h.m2),
                None => (None, mat::IDENT),
            };
            KBone { idx: *idx, name: name.clone(), parent, local, helper: helper[i] }
        })
        .collect())
}

/// World matrices; None when the parent links contain a cycle (the Python version then hits RecursionError).
fn worlds(bones: &[KBone]) -> Option<Vec<M4>> {
    let mut w: Vec<Option<M4>> = vec![None; bones.len()];
    for start in 0..bones.len() {
        let mut chain = vec![start];
        let mut cur = start;
        while w[cur].is_none() {
            match bones[cur].parent {
                Some(p) if p < bones.len() => {
                    if chain.contains(&p) {
                        return None;
                    }
                    if w[p].is_some() {
                        break;
                    }
                    chain.push(p);
                    cur = p;
                }
                _ => break,
            }
        }
        for &i in chain.iter().rev() {
            let m = match bones[i].parent {
                Some(p) if p < bones.len() => mat::mul(&bones[i].local, &w[p]?),
                _ => bones[i].local,
            };
            w[i] = Some(m);
        }
    }
    w.into_iter().collect()
}

fn max_abs_diff(a: &M4, b: &M4) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
}

struct Best {
    hyp: &'static str,
    s2b: Vec<(u16, usize)>,
    bones: Vec<KBone>,
    t: M4,
    err: f64,
    refined: Vec<(String, Option<usize>, usize)>,
}

/// kkc_export.prepare_rig
fn prepare_rig(d: &[u8], g: &Geo, prefix: &str, first_name_off: Option<usize>, skin_skip: &[String]) -> Result<(BTreeMap<u16, M4>, Best)> {
    let bones = build_rig(d, prefix, first_name_off)?;
    let s: BTreeMap<u16, M4> = g.skin.as_ref().ok_or_else(|| err!("GEO has no skin"))?.iter().map(|l| (l.bone, mat::from_f32(&l.mat))).collect();
    let n = bones.len();
    let skipped = |name: &str| skin_skip.iter().any(|p| name.contains(p.as_str()));
    let rank: Vec<usize> = bones.iter().filter(|b| !skipped(&b.name)).map(|b| b.idx).collect();
    let hyps: Vec<(&'static str, Vec<(u16, usize)>)> = vec![
        ("listing", (0..n).map(|i| (i as u16, i)).collect()),
        ("rank", rank.iter().enumerate().map(|(k, &b)| (k as u16, b)).collect()),
        ("plus1", (0..n).map(|i| ((i + 1) as u16, i)).collect()),
    ];
    let mut best: Option<Best> = None;
    for (hn, s2b) in hyps {
        if s.keys().any(|k| !s2b.iter().any(|(sk, _)| sk == k)) {
            continue;
        }
        let b2s: Vec<(usize, u16)> = s2b.iter().map(|&(k, b)| (b, k)).collect();
        let b2s_get = |i: usize| b2s.iter().find(|(b, _)| *b == i).map(|x| x.1);
        let mut bb = bones.clone();
        let mut refined = Vec::new();
        for bi in 0..bb.len() {
            let i = bb[bi].idx;
            let Some(ki) = b2s_get(i).filter(|k| s.contains_key(k)) else { continue };
            if !bb[bi].local.iter().all(|x| x.is_finite()) {
                continue;
            }
            let si = mat::inv(&s[&ki]).ok_or_else(|| err!("singular skin matrix"))?;
            let mut bestp: Option<(f64, usize)> = None;
            for &(p, kp) in &b2s {
                if p == i || !s.contains_key(&kp) {
                    continue;
                }
                let sp = mat::inv(&s[&kp]).ok_or_else(|| err!("singular skin matrix"))?;
                let e = max_abs_diff(&mat::mul(&bb[bi].local, &sp), &si);
                if bestp.map(|b| e < b.0).unwrap_or(true) {
                    bestp = Some((e, p));
                }
            }
            if let Some((e, p)) = bestp {
                if e < 2e-3 && bb[bi].parent != Some(p) {
                    refined.push((bb[bi].name.clone(), bb[bi].parent, p));
                    bb[bi].parent = Some(p);
                }
            }
        }
        let Some(w0) = worlds(&bb) else { continue };
        let Some(f) = bb.iter().find(|b| b2s_get(b.idx).map(|k| s.contains_key(&k)).unwrap_or(false)) else { continue };
        let fk = b2s_get(f.idx).unwrap();
        let t = mat::mul(&mat::inv(&w0[f.idx]).ok_or_else(|| err!("singular world"))?, &mat::inv(&s[&fk]).ok_or_else(|| err!("singular skin"))?);
        let mut e = 0.0f64;
        for &(k, b) in &s2b {
            if let Some(sm) = s.get(&k) {
                e = e.max(max_abs_diff(&mat::inv(sm).ok_or_else(|| err!("singular skin"))?, &mat::mul(&w0[b], &t)));
            }
        }
        if best.as_ref().map(|b| e < b.err).unwrap_or(true) {
            best = Some(Best { hyp: hn, s2b, bones: bb, t, err: e, refined });
        }
    }
    let best = best.ok_or_else(|| err!("no skin hypothesis fits"))?;
    Ok((s, best))
}

/// kkc_export.decompose (a mirrored matrix is flipped on x instead of rejected)
fn decompose(l: &M4) -> ([f64; 3], [f64; 4], [f64; 3]) {
    let t = [l[12], l[13], l[14]];
    let mut s = [0.0; 3];
    let mut rn = [[0.0; 3]; 3];
    for i in 0..3 {
        s[i] = (l[i * 4].powi(2) + l[i * 4 + 1].powi(2) + l[i * 4 + 2].powi(2)).sqrt();
        for j in 0..3 {
            rn[i][j] = l[i * 4 + j] / s[i];
        }
    }
    let det = rn[0][0] * (rn[1][1] * rn[2][2] - rn[1][2] * rn[2][1]) - rn[0][1] * (rn[1][0] * rn[2][2] - rn[1][2] * rn[2][0]) + rn[0][2] * (rn[1][0] * rn[2][1] - rn[1][1] * rn[2][0]);
    if det < 0.0 {
        for v in rn[0].iter_mut() {
            *v = -*v;
        }
        s[0] = -s[0];
    }
    let mut rt = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            rt[i][j] = rn[j][i];
        }
    }
    (t, mat::mat2quat(&rt), s)
}

// ------------------------------------------------------------------ clips

/// kkc_runs.walk
struct BankEntry {
    off: usize,
    r: Option<Trl>,
}
fn walk_bank(d: &[u8], mut o: usize) -> Vec<BankEntry> {
    let mut out = Vec::new();
    while out.len() < 1000 && o + 12 <= d.len() {
        let size = u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]) as usize;
        if o + 4 + size > d.len() {
            break;
        }
        let nt = u16::from_le_bytes([d[o + 4], d[o + 5]]);
        if size < 12 || size > 400_000 || nt > 200 || nt < 1 {
            break;
        }
        out.push(BankEntry { off: o, r: anim::parse_trl_with(d, o, true).ok() });
        o += 4 + size;
    }
    out
}

/// (frame, key) of the events that carry a translation or a rotation (kkc_export.track_keys)
fn track_keys(tr: &anim::Track) -> Vec<(u32, &anim::Key)> {
    let mut t = 0;
    let mut out = Vec::new();
    for e in &tr.events {
        if let Some(k) = &e.key {
            if k.t.is_some() || k.q.is_some() {
                out.push((t, k));
            }
        }
        t += e.nf;
    }
    out
}
fn key_q(k: &anim::Key) -> [f64; 4] {
    match &k.q {
        Some(Rot::Q(q)) => *q,
        _ => [0.0, 0.0, 0.0, 1.0],
    }
}
fn key_t(k: &anim::Key) -> [f64; 3] {
    let v = k.t.as_ref().map(|v| v[0]).unwrap_or([0.0; 3]);
    [v[0] as f64, v[1] as f64, v[2] as f64]
}
fn qa(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs();
    2.0 * d.min(1.0).acos().to_degrees()
}

struct Feat {
    frames: u32,
    root: [f64; 3],
    dist_h: f64,
    jaw: f64,
    pz: Option<[f64; 2]>,
    loop_: f64,
    root_yaw: f64,
    secs: f64,
    speed: f64,
}

/// kkc_export.clip_features
fn clip_features(r: &Trl, jaw: i64) -> Feat {
    let mut ts: Vec<((i32, char), (&anim::Track, Vec<(u32, &anim::Key)>))> = Vec::new();
    let mut fr = 0;
    for t in &r.tracks {
        let ks = track_keys(t);
        if ks.is_empty() {
            continue;
        }
        let kind = if ks[0].1.t.is_some() { 't' } else { 'r' };
        fr = fr.max(anim::track_times(t).1);
        if let Some(slot) = ts.iter_mut().find(|(k, _)| *k == (t.gizmo, kind)) {
            slot.1 = (t, ks);
        } else {
            ts.push(((t.gizmo, kind), (t, ks)));
        }
    }
    let get = |g: i32, k: char| ts.iter().find(|(kk, _)| *kk == (g, k)).map(|x| &x.1);
    let root = match get(-1, 't') {
        Some((_, ks)) => {
            let (a, b) = (key_t(ks[0].1), key_t(ks.last().unwrap().1));
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]]
        }
        None => [0.0; 3],
    };
    let dist_h = root[0].hypot(root[1]);
    let jawv = match get(jaw as i32, 'r').filter(|_| jaw >= -1) {
        Some((_, ks)) => {
            let q0 = key_q(ks[0].1);
            ks.iter().map(|(_, k)| qa(&q0, &key_q(k))).fold(f64::NEG_INFINITY, f64::max)
        }
        None => 0.0,
    };
    let pz = get(0, 't').map(|(_, ks)| {
        let z: Vec<f64> = ks.iter().map(|(_, k)| key_t(k)[2]).collect();
        [z.iter().cloned().fold(f64::INFINITY, f64::min), z.iter().cloned().fold(f64::NEG_INFINITY, f64::max)]
    });
    let cl: Vec<f64> = ts.iter().filter(|((g, kind), (_, ks))| *kind == 'r' && *g >= 0 && ks.len() > 1).map(|(_, (_, ks))| qa(&key_q(ks[0].1), &key_q(ks.last().unwrap().1))).collect();
    let loop_ = if cl.is_empty() { 0.0 } else { cl.iter().sum::<f64>() / cl.len() as f64 };
    let root_yaw = get(-1, 'r').map(|(_, ks)| qa(&key_q(ks[0].1), &key_q(ks.last().unwrap().1))).unwrap_or(0.0);
    let secs = fr as f64 / FPS;
    Feat { frames: fr, root, dist_h, jaw: jawv, pz, loop_, root_yaw, secs, speed: if secs > 0.0 { dist_h / secs } else { 0.0 } }
}

/// np.interp semantics for a (possibly non-strictly increasing) key list: last interval with xp[j] <= x < xp[j+1]
fn lin_interp(xp: &[f64], fp: &[[f64; 3]], x: f64) -> [f64; 3] {
    let n = xp.len();
    if x < xp[0] {
        return fp[0];
    }
    if x >= xp[n - 1] {
        return fp[n - 1];
    }
    let j = xp.partition_point(|&v| v <= x) - 1;
    let mut out = [0.0; 3];
    for c in 0..3 {
        let slope = (fp[j + 1][c] - fp[j][c]) / (xp[j + 1] - xp[j]);
        let mut r = slope * (x - xp[j]) + fp[j][c];
        if r.is_nan() {
            r = slope * (x - xp[j + 1]) + fp[j + 1][c];
            if r.is_nan() && fp[j][c] == fp[j + 1][c] {
                r = fp[j][c];
            }
        }
        out[c] = r;
    }
    out
}

struct OutChan {
    node: usize,
    rotation: bool,
    times: Vec<f32>,
    vals: Vec<f32>,
}

fn json_f(x: f64) -> Value {
    jd(x)
}

/// Texture entry for a material id.
struct MatTex {
    diffuse: Option<usize>,
    normal: Option<usize>,
}

fn mat_tex(recipe: &Value, mid: u32) -> MatTex {
    let m = &recipe["mats"];
    let e = m.get(mid.to_string()).or_else(|| m.get("*"));
    MatTex { diffuse: e.and_then(|e| e["diffuse"].as_u64()).map(|x| x as usize), normal: e.and_then(|e| e["normal"].as_u64()).map(|x| x as usize) }
}

#[derive(Debug)]
pub struct Table {
    pub labels: HashMap<usize, (String, String)>,
}

fn label_table(which: &str) -> Result<HashMap<usize, (String, String)>> {
    let v: Value = serde_json::from_str(include_str!("../data/creature_meta.json"))?;
    let t = v["labels"].get(which).ok_or_else(|| err!("no label table '{which}'"))?;
    Ok(t.as_object().unwrap().iter().map(|(k, a)| (k.parse().unwrap_or(usize::MAX), (a[0].as_str().unwrap_or("").to_string(), a[1].as_str().unwrap_or("").to_string()))).collect())
}

pub fn build_creature(src: &mut dyn Source, name: &str, recipe: &Value, opts: &BuildOpts, log: &mut dyn FnMut(&str)) -> Result<Built> {
    let cname = recipe["model"].as_str().ok_or_else(|| err!("{name}: recipe has no model"))?;
    let stream = recipe["stream"].as_str().ok_or_else(|| err!("{name}: no stream"))?;
    let prefix = recipe["prefix"].as_str().ok_or_else(|| err!("{name}: no prefix"))?;
    let geo_off = parse_num(recipe["geo"].as_str().ok_or_else(|| err!("{name}: no geo"))?)?;
    log(&format!("loading stream {stream}"));
    let dd = src.stream(stream)?;
    let d: &[u8] = &dd;
    let g = geo::parse_geo_kkc(d, geo_off).map_err(|e| err!("{name}: GEO {geo_off:#x}: {e}"))?;
    let skin_skip: Vec<String> = recipe["skin_skip"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_else(|| vec!["Snap".into(), "Base".into(), "Sang".into()]);
    let (smats, rig) = prepare_rig(d, &g, prefix, None, &skin_skip).map_err(|e| err!("{name}: {e}"))?;
    let mut bones = rig.bones.clone();
    let root = bones.iter().position(|b| b.parent.is_none() && !b.helper).ok_or_else(|| err!("{name}: no root bone"))?;
    bones[root].local = mat::mul(&rig.t, &bones[root].local);
    let flat = geo::flatten(&g)?;

    let mut glb = Glb::default();
    glb.samplers = vec![json!({"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497})];
    let bank_key = recipe["bank"].as_str();
    let want_tex = opts.with_textures && g.elems.iter().any(|e| mat_tex(recipe, e.mat).diffuse.is_some());
    let bank = if want_tex { Some(src.bank(bank_key.ok_or_else(|| err!("{name}: textures need a bank"))?)?) } else { None };
    let mut tex_cache: HashMap<(usize, bool), usize> = HashMap::new();
    let mut matdefs: HashMap<u32, usize> = HashMap::new();
    for e in &g.elems {
        let mt = mat_tex(recipe, e.mat);
        let mut m = json!({
            "name": format!("{cname}_mat{}", e.mat),
            "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.0, "roughnessFactor": 0.9},
            "doubleSided": true,
            "extras": {"jade_material_id": e.mat}
        });
        if let Some(bk) = bank_key {
            if let Some(di) = mt.diffuse {
                m["extras"]["kk_diffuse_png"] = json!(format!("{bk}/idx{di:03}.png"));
            }
            if let Some(ni) = mt.normal {
                m["extras"]["kk_normal_png"] = json!(format!("{bk}/idx{ni:03}.png"));
            }
        }
        if let Some(b) = &bank {
            for (idx, normal) in [(mt.diffuse, false), (mt.normal, true)] {
                let Some(idx) = idx else { continue };
                let t = match tex_cache.get(&(idx, normal)) {
                    Some(&t) => t,
                    None => {
                        log(&format!("decoding texture {idx}"));
                        let img = texture::bank_texture(b, idx).map_err(|e| err!("{name}: texture idx{idx:03}: {e}"))?;
                        let png = if normal { img.to_png_normal()? } else { img.to_png_rgb()? };
                        let t = glb.add_png_texture(&format!("{}_idx{idx:03}", if normal { "normal" } else { "diffuse" }), &png);
                        tex_cache.insert((idx, normal), t);
                        t
                    }
                };
                if normal {
                    m["normalTexture"] = json!({"index": t});
                } else {
                    m["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": t});
                }
            }
        }
        matdefs.insert(e.mat, glb.materials.len());
        glb.materials.push(m);
    }
    glb.nodes.push(json!({"name": "JadeZup_to_GltfYup", "rotation": [-0.70710678, 0.0, 0.0, 0.70710678], "children": []}));
    glb.scene_roots.push(0);
    glb.nodes.push(json!({"name": "JadeActor", "children": [], "extras": {"note": "gizmo -1 track (root motion) would animate this node; clips are exported in place"}}));
    glb.nodes[0]["children"].as_array_mut().unwrap().push(json!(1));
    let b2s = |i: usize| rig.s2b.iter().find(|(_, b)| *b == i).map(|x| x.0);
    let mut node: HashMap<usize, usize> = HashMap::new();
    for b in &bones {
        let (t, q, s) = decompose(&b.local);
        let mut n = json!({
            "name": b.name,
            "translation": [jd(t[0]), jd(t[1]), jd(t[2])],
            "rotation": [jd(q[0]), jd(q[1]), jd(q[2]), jd(q[3])],
            "extras": {"jade_listing_index": b.idx, "jade_skin_id": b2s(b.idx)}
        });
        if s.iter().any(|x| (x - 1.0).abs() > 1e-3) {
            n["scale"] = json!([jd(s[0]), jd(s[1]), jd(s[2])]);
        }
        node.insert(b.idx, glb.nodes.len());
        glb.nodes.push(n);
    }
    for b in &bones {
        let ni = node[&b.idx];
        let par = match b.parent {
            None => 1,
            Some(p) => node[&p],
        };
        let kids = glb.nodes[par].as_object_mut().unwrap().entry("children").or_insert_with(|| json!([]));
        kids.as_array_mut().unwrap().push(json!(ni));
    }
    let ids: Vec<u16> = smats.keys().copied().collect();
    let jindex: HashMap<u16, usize> = ids.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let ibm_data: Vec<f32> = ids.iter().flat_map(|k| smats[k].iter().map(|&x| x as f32).collect::<Vec<_>>()).collect();
    let ibm = glb.acc_f32(&ibm_data, 16, "MAT4", None, false);
    let s2b_of = |k: u16| rig.s2b.iter().find(|(s, _)| *s == k).map(|x| x.1).unwrap();
    glb.skins.push(json!({"name": format!("{cname}_skin"), "joints": ids.iter().map(|&k| node[&s2b_of(k)]).collect::<Vec<_>>(), "inverseBindMatrices": ibm, "skeleton": 1}));

    let n = g.nverts;
    let mut infl: Vec<Vec<(f32, u16)>> = vec![Vec::new(); n];
    for l in g.skin.as_ref().unwrap() {
        let j = jindex[&l.bone] as u16;
        for (&vi, &w) in l.idx.iter().zip(&l.w) {
            infl[vi as usize].push((w, j));
        }
    }
    let mut jn = vec![[0u16; 4]; n];
    let mut wt = vec![[0f32; 4]; n];
    for (v, lst) in infl.iter_mut().enumerate() {
        if lst.is_empty() {
            continue;
        }
        lst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(b.1.cmp(&a.1)));
        lst.truncate(4);
        let tot: f64 = lst.iter().map(|x| x.0 as f64).sum();
        for (k, (w, j)) in lst.iter().enumerate() {
            jn[v][k] = *j;
            wt[v][k] = (*w as f64 / tot) as f32;
        }
    }
    let pos: Vec<f32> = flat.pos.iter().flatten().copied().collect();
    let nrm: Vec<f32> = flat.nrm.iter().flatten().copied().collect();
    let uv: Vec<f32> = flat.uv.iter().flatten().copied().collect();
    let joints: Vec<u16> = flat.src.iter().flat_map(|&s| jn[s]).collect();
    let weights: Vec<f32> = flat.src.iter().flat_map(|&s| wt[s]).collect();
    let a = glb.acc_f32(&pos, 3, "VEC3", Some(34962), true);
    let nn = glb.acc_f32(&nrm, 3, "VEC3", Some(34962), false);
    let tt = glb.acc_f32(&uv, 2, "VEC2", Some(34962), false);
    let attrs = json!({"POSITION": a, "NORMAL": nn, "TEXCOORD_0": tt, "JOINTS_0": glb.acc_u16(&joints, 4, "VEC4", Some(34962)), "WEIGHTS_0": glb.acc_f32(&weights, 4, "VEC4", Some(34962), false)});
    let mut prims = Vec::new();
    for (mid, idx) in &flat.elems {
        if idx.is_empty() {
            continue;
        }
        let ia = glb.acc_u32(idx, Some(34963));
        prims.push(json!({"attributes": attrs, "indices": ia, "material": matdefs[mid], "mode": 4}));
    }
    glb.meshes.push(json!({"name": format!("{cname}_mesh"), "primitives": prims}));
    glb.nodes.push(json!({"name": format!("{cname}_mesh"), "mesh": 0, "skin": 0}));
    let mesh_node = glb.nodes.len() - 1;
    glb.nodes[0]["children"].as_array_mut().unwrap().push(json!(mesh_node));

    // ---- clips
    let anim_stream = recipe["clip_stream"].as_str().unwrap_or(stream);
    let dd2 = if anim_stream == stream { dd.clone() } else { src.stream(anim_stream)? };
    let da: &[u8] = &dd2;
    let offs: Vec<usize> = match &recipe["clips"] {
        Value::String(s) => vec![parse_num(s)?],
        Value::Array(a) => a.iter().map(|x| parse_num(x.as_str().unwrap_or(""))).collect::<Result<Vec<_>>>()?,
        _ => return Err(err!("{name}: no clips")),
    };
    let single = matches!(recipe["clips"], Value::String(_));
    let bank_entries: Vec<BankEntry> = if single { walk_bank(da, offs[0]) } else { offs.iter().flat_map(|&o| walk_bank(da, o).into_iter().take(1)).collect() };
    let labels = match recipe["labels"].as_str() {
        Some(w) => Some(label_table(w)?),
        None => None,
    };
    let jaw: i64 = bones.iter().find(|b| b.name.ends_with("Machoire")).map(|b| b.idx as i64).unwrap_or(-99);
    let pelvis = recipe["pelvis"].as_i64().unwrap_or(0) as i32;
    let mut rootmotion = Map::new();
    let mut nclips = 0;
    for (ci, e) in bank_entries.iter().enumerate() {
        let Some(r) = &e.r else { continue };
        let f = clip_features(r, jaw);
        let source = format!("{anim_stream}@{:#x}", e.off);
        let lab = labels.as_ref().and_then(|l| l.get(&ci));
        let cn = format!("{cname}_{ci:03}");
        let full = match lab {
            Some(l) => format!("{}__{cn}", l.0),
            None => cn.clone(),
        };
        // root translation sampler
        let root_keys: Option<Vec<(f64, [f64; 3])>> = r.tracks.iter().filter(|tr| tr.gizmo == -1).filter_map(|tr| {
            let ks = track_keys(tr);
            if !ks.is_empty() && ks[0].1.t.is_some() {
                Some(ks.iter().map(|(t, k)| (*t as f64, key_t(k))).collect::<Vec<_>>())
            } else {
                None
            }
        }).last();
        let travel = f.root;
        let mut baked = false;
        if let Some(tr) = r.tracks.iter().find(|tr| tr.gizmo == pelvis && {
            let ks = track_keys(tr);
            !ks.is_empty() && ks[0].1.t.is_some()
        }) {
            let ks = track_keys(tr);
            let (a, b) = (key_t(ks[0].1), key_t(ks.last().unwrap().1));
            let pd = [b[0] - a[0], b[1] - a[1]];
            if travel[0].hypot(travel[1]) > 0.05 && (pd[0] - travel[0]).hypot(pd[1] - travel[1]) < 0.25 * travel[0].hypot(travel[1]) + 0.05 {
                baked = true;
            }
        }
        let mut chans: Vec<OutChan> = Vec::new();
        let mut miss: std::collections::BTreeSet<i32> = Default::default();
        for tr in &r.tracks {
            let gz = tr.gizmo;
            if gz == -1 {
                continue;
            }
            let Some(&ni) = node.get(&(gz as usize)).filter(|_| gz >= 0) else {
                miss.insert(gz);
                continue;
            };
            let ks = track_keys(tr);
            if ks.is_empty() {
                continue;
            }
            let mut dd: BTreeMap<u32, usize> = BTreeMap::new();
            for (i, (t, _)) in ks.iter().enumerate() {
                dd.insert(*t, i);
            }
            let keep: Vec<usize> = dd.values().copied().collect();
            let times: Vec<f32> = keep.iter().map(|&i| (ks[i].0 as f64 / FPS) as f32).collect();
            if ks[0].1.q.is_some() && ks[0].1.t.is_none() {
                let mut vals: Vec<[f32; 4]> = keep.iter().map(|&i| { let q = key_q(ks[i].1); [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32] }).collect();
                for i in 1..vals.len() {
                    let (a, b) = (vals[i], vals[i - 1]);
                    if a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 {
                        vals[i] = [-a[0], -a[1], -a[2], -a[3]];
                    }
                }
                chans.push(OutChan { node: ni, rotation: true, times, vals: vals.into_iter().flatten().collect() });
            } else if ks[0].1.t.is_some() {
                let mut vals: Vec<[f32; 3]> = keep.iter().map(|&i| { let t = key_t(ks[i].1); [t[0] as f32, t[1] as f32, t[2] as f32] }).collect();
                if baked && gz == pelvis {
                    if let Some(rk) = &root_keys {
                        let xp: Vec<f64> = rk.iter().map(|x| x.0).collect();
                        let fp: Vec<[f64; 3]> = rk.iter().map(|x| x.1).collect();
                        for (j, &i) in keep.iter().enumerate() {
                            let rv = lin_interp(&xp, &fp, ks[i].0 as f64);
                            vals[j][0] = (vals[j][0] as f64 - rv[0]) as f32;
                            vals[j][1] = (vals[j][1] as f64 - rv[1]) as f32;
                        }
                    }
                }
                chans.push(OutChan { node: ni, rotation: false, times, vals: vals.into_iter().flatten().collect() });
            }
        }
        let mut samplers = Vec::new();
        let mut channels = Vec::new();
        for c in &chans {
            let input = glb.acc_f32(&c.times, 1, "SCALAR", None, true);
            let output = glb.acc_f32(&c.vals, if c.rotation { 4 } else { 3 }, if c.rotation { "VEC4" } else { "VEC3" }, None, false);
            samplers.push(json!({"input": input, "output": output, "interpolation": "LINEAR"}));
            channels.push(json!({"sampler": samplers.len() - 1, "target": {"node": c.node, "path": if c.rotation { "rotation" } else { "translation" }}}));
        }
        let features = json!({
            "frames": f.frames, "root": [json_f(f.root[0]), json_f(f.root[1]), json_f(f.root[2])], "dist_h": json_f(f.dist_h), "jaw": json_f(f.jaw),
            "pz": f.pz.map(|p| json!([json_f(p[0]), json_f(p[1])])).unwrap_or(Value::Null), "loop": json_f(f.loop_), "root_yaw": json_f(f.root_yaw), "secs": json_f(f.secs), "speed": json_f(f.speed)
        });
        glb.animations.push(json!({"name": full, "samplers": samplers, "channels": channels, "extras": {
            "source": source, "frames": f.frames, "fps": FPS, "index": ci, "features": features, "missing_gizmos": miss.iter().collect::<Vec<_>>(),
            "root_motion_in_pelvis": baked, "label": lab.map(|l| l.0.clone()), "label_confidence": lab.map(|l| l.1.clone())
        }}));
        rootmotion.insert(
            full,
            json!({
                "duration_s": json_f(f.secs), "jade_displacement": [json_f(travel[0]), json_f(travel[1]), json_f(travel[2])],
                "gltf_displacement": [json_f(travel[0]), json_f(travel[2]), json_f(-travel[1])], "speed_mps": json_f(f.speed), "root_yaw_deg": json_f(f.root_yaw),
                "travel_baked_in_pelvis_track": baked, "forward_axis_jade": "-Y"
            }),
        );
        nclips += 1;
    }
    let ext: Vec<f64> = (0..3).map(|c| {
        let mx = flat.pos.iter().map(|p| p[c]).fold(f32::NEG_INFINITY, f32::max);
        let mn = flat.pos.iter().map(|p| p[c]).fold(f32::INFINITY, f32::min);
        (mx - mn) as f64
    }).collect();
    let extras = json!({
        "source": "King Kong 2005 PC (Jade), rebuilt from the user's own copy by kk-extract; see creature_asset_findings.md", "creature": cname,
        "rig": prefix, "fps": FPS, "units": "metres", "up": "Y (Jade Z-up -> glTF Y-up via root node)", "forward": "glTF +Z (Jade -Y)", "clips_in_place": true,
        "skin_hypothesis": rig.hyp, "skin_hierarchy_max_error": rig.err, "runtime_scale": recipe["runtime_scale"], "extent_jade": ext
    });
    let _ = &rig.refined;
    let files = vec![(name.to_string(), glb.finish(extras, "kk-extract")), (recipe["rootmotion"].as_str().ok_or_else(|| err!("{name}: no rootmotion output"))?.to_string(), serde_json::to_vec_pretty(&Value::Object(rootmotion))?)];
    Ok(Built { files, notes: vec![format!("{nclips} clips, rig error {:.2e} ({})", rig.err, rig.hyp)] })
}

#[allow(dead_code)]
fn unused(_: &Bone, _: &dyn Fn(&[Bone]) -> Vec<M4>) {
    let _ = world_matrices;
}

/// `creatures/manifest.json` and `creatures/<name>_actions.json` (kkc_finalize.py): static analysis tables, no game data.
pub fn build_meta() -> Result<Built> {
    let v: Value = serde_json::from_str(include_str!("../data/creature_meta.json"))?;
    let mut files = vec![("creatures/manifest.json".to_string(), serde_json::to_vec_pretty(&v["manifest"])?)];
    for (k, a) in v["actions"].as_object().ok_or_else(|| err!("creature_meta: no actions"))? {
        files.push((format!("creatures/{k}_actions.json"), serde_json::to_vec_pretty(a)?));
    }
    Ok(Built { files, notes: vec![] })
}
