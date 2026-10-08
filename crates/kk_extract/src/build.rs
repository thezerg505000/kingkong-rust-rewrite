//! Asset recipes -> files: skinned animated characters and static meshes as `.glb`.
//! Ports the parts of `export_meshes.py`, `build_anim_glb.py`, `texture_bind.py`, `make_game_assets.py`
//! (graft + labels + weapon socket) and the root-motion strip that produced `trex_inplace.glb`.

use crate::anim::{self, Rot, Trl};
use crate::err;
use crate::error::{Error, Result};
use crate::geo::{self, Geo};
use crate::glb::{jd, jf, jf_arr, Glb};
use crate::mat;
use crate::skel::{self, Bone};
use crate::texture;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

pub const FPS: f64 = 60.0;

/// Where decoded streams and texture banks come from (the game files, or test data).
pub trait Source {
    /// Decoded `ROOT/Bin/<key>.bin` of KKMaps.bf (e.g. "ff0003eb").
    fn stream(&mut self, key: &str) -> Result<Arc<Vec<u8>>>;
    /// Decoded texture bank of KKTextures.bf (e.g. "ff8003eb").
    fn bank(&mut self, key: &str) -> Result<Arc<Vec<u8>>>;
    /// Index of Sound_Common.bf (path + resource key per file).
    fn sound_index(&mut self) -> Result<Vec<BfEntry>> {
        Err(err!("this source has no Sound_Common.bf"))
    }
    /// Raw bytes of entry `i` of [`Source::sound_index`].
    fn sound_read(&mut self, _i: usize) -> Result<Vec<u8>> {
        Err(err!("this source has no Sound_Common.bf"))
    }
}

/// One file of a BF archive.
#[derive(Debug, Clone)]
pub struct BfEntry {
    pub path: String,
    pub key: u32,
}

#[derive(Clone, Debug)]
pub struct BuildOpts {
    /// embed textures (needs the bank); false builds the untextured grey materials
    pub with_textures: bool,
    /// skip clips whose stream cannot be loaded instead of failing (tests with partial data)
    pub skip_missing_clips: bool,
}
impl Default for BuildOpts {
    fn default() -> Self {
        BuildOpts { with_textures: true, skip_missing_clips: false }
    }
}

#[derive(Debug, Clone)]
pub struct ClipDef {
    pub name: String,
    pub stream: String,
    pub off: usize,
    pub frames: u32,
    pub label: String,
    pub conf: String,
}

pub struct ClipDb {
    pub sets: HashMap<String, Vec<ClipDef>>,
}

impl ClipDb {
    pub fn builtin() -> ClipDb {
        Self::parse(include_str!("../data/clips.json")).expect("built-in clips.json")
    }
    pub fn parse(s: &str) -> Result<ClipDb> {
        let v: Value = serde_json::from_str(s)?;
        let mut sets = HashMap::new();
        for (k, arr) in v.as_object().ok_or_else(|| err!("clips.json: not an object"))? {
            let mut list = Vec::new();
            for c in arr.as_array().ok_or_else(|| err!("clips.json: {k} not an array"))? {
                list.push(ClipDef {
                    name: sstr(c, "name")?.to_string(),
                    stream: sstr(c, "stream")?.to_string(),
                    off: c["off"].as_u64().ok_or_else(|| err!("clip off"))? as usize,
                    frames: c["frames"].as_u64().unwrap_or(0) as u32,
                    label: sstr(c, "label")?.to_string(),
                    conf: sstr(c, "conf")?.to_string(),
                });
            }
            sets.insert(k.clone(), list);
        }
        Ok(ClipDb { sets })
    }
}

pub struct Built {
    pub files: Vec<(String, Vec<u8>)>,
    pub notes: Vec<String>,
}

fn sstr<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().ok_or_else(|| err!("recipe: missing string '{k}'"))
}
pub fn parse_num(s: &str) -> Result<usize> {
    let t = s.trim();
    let r = if let Some(h) = t.strip_prefix("0x") { usize::from_str_radix(h, 16) } else { t.parse() };
    r.map_err(|_| err!("bad number '{s}'"))
}

/// Build one manifest asset.
pub fn build_asset(src: &mut dyn Source, name: &str, recipe: &Value, clips: &ClipDb, opts: &BuildOpts, log: &mut dyn FnMut(&str)) -> Result<Built> {
    match sstr(recipe, "kind")? {
        "character" => build_character(src, name, recipe, clips, opts, log),
        "static" => build_static(src, name, recipe, opts, log),
        "kong" => crate::creature::build_kong_like(src, name, recipe, opts, log),
        "creature" => crate::kkc::build_creature(src, name, recipe, opts, log),
        "creature_meta" => crate::kkc::build_meta(),
        "images" => crate::images::build_images(src, name, recipe, log),
        "sounds" => crate::sound::build_sounds(src, recipe, log),
        k => Err(err!("{name}: unknown asset kind '{k}'")),
    }
}

// ------------------------------------------------------------------ shared pieces

fn plain_material(mat_id: u32) -> Value {
    json!({
        "name": format!("jade_mat_{mat_id}"),
        "pbrMetallicRoughness": {"baseColorFactor": [0.7, 0.7, 0.7, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.9},
        "doubleSided": true,
        "extras": {"jade_material_id": mat_id}
    })
}

fn flat_vertex_arrays(f: &geo::Flat) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    (f.pos.iter().flatten().copied().collect(), f.nrm.iter().flatten().copied().collect(), f.uv.iter().flatten().copied().collect())
}

/// Bind textures (texture_bind.bind): replaces the grey materials by textured ones, embeds PNGs.
fn bind_textures(glb: &mut Glb, src: &mut dyn Source, recipe: &Value, log: &mut dyn FnMut(&str)) -> Result<()> {
    let Some(table) = recipe.get("textures").and_then(|t| t.as_object()) else { return Ok(()) };
    let bank_key = sstr(recipe, "bank")?;
    let bank = src.bank(bank_key)?;
    glb.samplers = vec![json!({"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497})];
    let mut cache: HashMap<String, usize> = HashMap::new();
    let mut decoded: HashMap<usize, texture::Image> = HashMap::new();
    for mi in 0..glb.materials.len() {
        let mid = glb.materials[mi]["extras"]["jade_material_id"].as_u64().unwrap_or(0);
        let Some(ent) = table.get(&mid.to_string()).or_else(|| table.get("*")) else { continue };
        let mut tex_index = |glb: &mut Glb, kind: &str, spec: &Value| -> Result<usize> {
            let idx = spec["idx"].as_u64().ok_or_else(|| err!("texture idx"))? as usize;
            let key = spec["key"].as_str().unwrap_or("");
            let tag = format!("{kind}_{key}");
            if let Some(&t) = cache.get(&tag) {
                return Ok(t);
            }
            if !decoded.contains_key(&idx) {
                log(&format!("decoding {bank_key} texture {idx}"));
                decoded.insert(idx, texture::bank_texture(&bank, idx).map_err(|e| err!("{bank_key} idx{idx:03}: {e}"))?);
            }
            let img = &decoded[&idx];
            let png = if kind == "normal" { img.to_png_normal()? } else { img.to_png_rgb()? };
            let t = glb.add_png_texture(&tag, &png);
            cache.insert(tag, t);
            Ok(t)
        };
        let d = tex_index(glb, "diffuse", &ent["diffuse"])?;
        let n = tex_index(glb, "normal", &ent["normal"])?;
        let m = &mut glb.materials[mi];
        m["pbrMetallicRoughness"] = json!({"baseColorTexture": {"index": d}, "baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.0, "roughnessFactor": 0.85});
        m["normalTexture"] = json!({"index": n});
        let ex = m["extras"].as_object_mut().unwrap();
        ex.insert("kk_world".into(), ent["world"].clone());
        ex.insert("kk_diffuse_key".into(), ent["diffuse"]["key"].clone());
        ex.insert("kk_normal_key".into(), ent["normal"]["key"].clone());
        ex.insert("kk_diffuse_texture".into(), json!(format!("{bank_key}/idx{:03}", ent["diffuse"]["idx"].as_u64().unwrap_or(0))));
        ex.insert("kk_normal_texture".into(), json!(format!("{bank_key}/idx{:03}", ent["normal"]["idx"].as_u64().unwrap_or(0))));
    }
    Ok(())
}

// ------------------------------------------------------------------ static meshes

fn build_static(src: &mut dyn Source, name: &str, recipe: &Value, opts: &BuildOpts, log: &mut dyn FnMut(&str)) -> Result<Built> {
    let stream = sstr(recipe, "stream")?;
    let off = parse_num(sstr(recipe, "geo_off")?)?;
    log(&format!("loading stream {stream}"));
    let d = src.stream(stream)?;
    let g = geo::parse_geo(&d, off).map_err(|e| err!("{name}: {e}"))?;
    let flat = geo::flatten(&g)?;
    let mut glb = Glb::default();
    let mut matdefs = HashMap::new();
    for e in &g.elems {
        matdefs.insert(e.mat, glb.materials.len());
        glb.materials.push(plain_material(e.mat));
    }
    glb.nodes.push(json!({"name": "JadeZup_to_GltfYup", "rotation": [-0.70710678, 0.0, 0.0, 0.70710678], "children": [1]}));
    glb.scene_roots.push(0);
    let mesh = add_mesh(&mut glb, recipe["mesh_node"].as_str().unwrap_or("mesh"), &flat, &matdefs, None);
    glb.nodes.push(json!({"name": recipe["mesh_node"].as_str().unwrap_or("mesh"), "mesh": mesh}));
    if opts.with_textures {
        bind_textures(&mut glb, src, recipe, log)?;
    }
    let extras = json!({
        "source": format!("{stream}.bin"), "geo_offset": format!("{off:#x}"), "note": recipe["note"],
        "nverts_src": g.nverts, "ntri": g.ntri(), "kk_texture_binding": recipe["binding_note"]
    });
    Ok(Built { files: vec![(name.to_string(), glb.finish(extras, "kk-extract"))], notes: vec![] })
}

fn add_mesh(glb: &mut Glb, name: &str, f: &geo::Flat, matdefs: &HashMap<u32, usize>, skinning: Option<(&[u16], &[f32])>) -> usize {
    let (pos, nrm, uv) = flat_vertex_arrays(f);
    let a = glb.acc_f32(&pos, 3, "VEC3", Some(34962), true);
    let n = glb.acc_f32(&nrm, 3, "VEC3", Some(34962), false);
    let t = glb.acc_f32(&uv, 2, "VEC2", Some(34962), false);
    let mut attrs = json!({"POSITION": a, "NORMAL": n, "TEXCOORD_0": t});
    if let Some((j, w)) = skinning {
        attrs["JOINTS_0"] = json!(glb.acc_u16(j, 4, "VEC4", Some(34962)));
        attrs["WEIGHTS_0"] = json!(glb.acc_f32(w, 4, "VEC4", Some(34962), false));
    }
    let mut prims = Vec::new();
    for (mid, idx) in &f.elems {
        if idx.is_empty() {
            continue;
        }
        let ia = glb.acc_u32(idx, Some(34963));
        prims.push(json!({"attributes": attrs, "indices": ia, "material": matdefs[mid], "mode": 4}));
    }
    glb.meshes.push(json!({"name": name, "primitives": prims}));
    glb.meshes.len() - 1
}

// ------------------------------------------------------------------ characters

struct Chan {
    node: usize,
    rotation: bool,
    times: Vec<f32>,
    vals: Vec<f32>,
}

struct ClipData {
    def: ClipDef,
    frames: u32,
    features: Value,
    missing: BTreeSet<i32>,
    chans: Vec<Chan>,
    ntracks: usize,
}

fn features_json(f: &anim::Features) -> Value {
    let mut m = Map::new();
    m.insert("frames".into(), json!(f.frames));
    m.insert("root_travel".into(), json!([jd(f.root_travel[0]), jd(f.root_travel[1]), jd(f.root_travel[2])]));
    m.insert("root_dist".into(), jd(f.root_dist));
    if let Some(j) = f.jaw_range_deg {
        m.insert("jaw_range_deg".into(), jd(j));
    }
    if let Some(p) = f.pelvis_z {
        m.insert("pelvis_z".into(), json!([jd(p[0]), jd(p[1])]));
    }
    m.insert("loop_err_deg".into(), jd(f.loop_err_deg));
    Value::Object(m)
}

/// build_anim_glb: tracks -> glTF channels (translation and rotation separate, strictly increasing times,
/// continuous quaternions).
fn channels_of(r: &Trl, node_of_gizmo: &HashMap<i32, usize>) -> (Vec<Chan>, BTreeSet<i32>) {
    let mut chans = Vec::new();
    let mut missing = BTreeSet::new();
    for tr in &r.tracks {
        let ni = if tr.gizmo == -1 {
            1
        } else if let Some(&n) = node_of_gizmo.get(&tr.gizmo) {
            n
        } else {
            missing.insert(tr.gizmo);
            continue;
        };
        let mut times: Vec<u32> = Vec::new();
        let mut tv: Vec<Option<[f32; 3]>> = Vec::new();
        let mut rv: Vec<Option<[f64; 4]>> = Vec::new();
        let mut tt = 0u32;
        for ev in &tr.events {
            if let Some(k) = &ev.key {
                if k.t.is_some() || k.q.is_some() {
                    times.push(tt);
                    tv.push(k.t.as_ref().and_then(|v| v.first().copied()));
                    rv.push(match &k.q {
                        Some(Rot::Q(q)) => Some(*q),
                        _ => None,
                    });
                }
            }
            tt += ev.nf;
        }
        if times.is_empty() {
            continue;
        }
        let keep: Vec<usize> = {
            let strictly: Vec<usize> = std::iter::once(0).chain((1..times.len()).filter(|&i| times[i] > times[i - 1])).collect();
            if strictly.len() == times.len() {
                strictly
            } else {
                let mut dd: std::collections::BTreeMap<u32, usize> = Default::default();
                for (i, &t) in times.iter().enumerate() {
                    dd.insert(t, i);
                }
                dd.values().copied().collect()
            }
        };
        let tf: Vec<f32> = keep.iter().map(|&i| (times[i] as f64 / FPS) as f32).collect();
        if rv[0].is_some() {
            let mut vals: Vec<[f32; 4]> = Vec::new();
            let mut prev = [0.0f32, 0.0, 0.0, 1.0];
            for &i in &keep {
                let q = rv[i].map(|q| [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32]).unwrap_or(prev);
                prev = q;
                vals.push(q);
            }
            for i in 1..vals.len() {
                let (a, b) = (vals[i], vals[i - 1]);
                if a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 {
                    vals[i] = [-a[0], -a[1], -a[2], -a[3]];
                }
            }
            chans.push(Chan { node: ni, rotation: true, times: tf, vals: vals.into_iter().flatten().collect() });
        } else if tv[0].is_some() {
            let mut vals: Vec<f32> = Vec::new();
            let mut prev = [0.0f32; 3];
            for &i in &keep {
                let v = tv[i].unwrap_or(prev);
                prev = v;
                vals.extend_from_slice(&v);
            }
            chans.push(Chan { node: ni, rotation: false, times: tf, vals });
        }
    }
    (chans, missing)
}

fn lerp_at(times: &[f32], vals: &[f32], t: f32) -> [f32; 3] {
    let n = times.len();
    let g = |i: usize| [vals[i * 3], vals[i * 3 + 1], vals[i * 3 + 2]];
    if n == 0 {
        return [0.0; 3];
    }
    if t <= times[0] {
        return g(0);
    }
    if t >= times[n - 1] {
        return g(n - 1);
    }
    let mut i = 0;
    while i + 1 < n && times[i + 1] < t {
        i += 1;
    }
    let (t0, t1) = (times[i], times[i + 1]);
    let (a, b) = (g(i), g(i + 1));
    let u = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
    [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u, a[2] + (b[2] - a[2]) * u]
}

/// Root-motion strip: the pelvis translation carries the actor travel again, so subtract the (linearly
/// interpolated) JadeActor translation from it and zero the actor track. Returns the rootmotion entry.
fn strip_root_motion(c: &mut ClipData, pelvis_node: usize) -> Value {
    let actor = c.chans.iter().position(|ch| ch.node == 1 && !ch.rotation);
    let mut entry = json!({"duration_s": 0.0, "jade_displacement": [0.0, 0.0, 0.0], "gltf_displacement": [0.0, 0.0, 0.0], "speed_mps": 0.0, "keys_actor": 0});
    let dur = c.chans.iter().filter_map(|ch| ch.times.last().copied()).fold(0.0f32, f32::max) as f64;
    entry["duration_s"] = jd(dur);
    let Some(ai) = actor else { return entry };
    let (at, av) = (c.chans[ai].times.clone(), c.chans[ai].vals.clone());
    let n = at.len();
    let disp = [(av[(n - 1) * 3] - av[0]) as f64, (av[(n - 1) * 3 + 1] - av[1]) as f64, (av[(n - 1) * 3 + 2] - av[2]) as f64];
    if let Some(pi) = c.chans.iter().position(|ch| ch.node == pelvis_node && !ch.rotation) {
        let ch = &mut c.chans[pi];
        for (k, &t) in ch.times.iter().enumerate() {
            let lin = lerp_at(&at, &av, t);
            for a in 0..3 {
                ch.vals[k * 3 + a] -= lin[a];
            }
        }
    }
    for v in c.chans[ai].vals.iter_mut() {
        *v = 0.0;
    }
    let speed = if dur > 0.0 { (disp[0] * disp[0] + disp[1] * disp[1] + disp[2] * disp[2]).sqrt() / dur } else { 0.0 };
    entry["jade_displacement"] = json!([jd(disp[0]), jd(disp[1]), jd(disp[2])]);
    entry["gltf_displacement"] = json!([jd(disp[0]), jd(disp[2]), jd(-disp[1])]);
    entry["speed_mps"] = jd(speed);
    entry["keys_actor"] = json!(n);
    entry
}

fn write_clip(glb: &mut Glb, c: &ClipData) -> Value {
    let mut samplers = Vec::new();
    let mut channels = Vec::new();
    for ch in &c.chans {
        let input = glb.acc_f32(&ch.times, 1, "SCALAR", None, true);
        let output = glb.acc_f32(&ch.vals, if ch.rotation { 4 } else { 3 }, if ch.rotation { "VEC4" } else { "VEC3" }, None, false);
        samplers.push(json!({"input": input, "output": output, "interpolation": "LINEAR"}));
        channels.push(json!({"sampler": samplers.len() - 1, "target": {"node": ch.node, "path": if ch.rotation { "rotation" } else { "translation" }}}));
    }
    json!({
        "name": format!("{}__{}", c.def.label, c.def.name),
        "samplers": samplers,
        "channels": channels,
        "extras": {
            "source": format!("{}.bin@{:#x}", c.def.stream, c.def.off), "frames": c.frames, "fps": FPS,
            "features": c.features, "missing_gizmos": c.missing.iter().collect::<Vec<_>>(),
            "original_name": c.def.name, "label": c.def.label, "label_confidence": c.def.conf
        }
    })
}

fn build_character(src: &mut dyn Source, name: &str, recipe: &Value, clipdb: &ClipDb, opts: &BuildOpts, log: &mut dyn FnMut(&str)) -> Result<Built> {
    let stream = sstr(recipe, "stream")?;
    let geo_off = parse_num(sstr(recipe, "geo_off")?)?;
    let prefix = sstr(&recipe["rig"], "prefix")?;
    let skip_owned: Vec<String> = recipe["rig"]["skip"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    let skip: Vec<&str> = skip_owned.iter().map(|s| s.as_str()).collect();
    log(&format!("loading stream {stream}"));
    let (g, bones, flat): (Geo, Vec<Bone>, geo::Flat) = {
        let d = src.stream(stream)?;
        let g = geo::parse_geo(&d, geo_off).map_err(|e| err!("{name}: {e}"))?;
        let bones = skel::load_rig(&d, &g, prefix, &skip, geo_off).map_err(|e| err!("{name}: {e}"))?;
        let flat = geo::flatten(&g)?;
        (g, bones, flat)
    };
    let skin = g.skin.as_ref().ok_or_else(|| err!("{name}: GEO has no skin"))?;
    let smat = |id: usize| skin.iter().find(|l| l.bone as usize == id);

    let mut glb = Glb::default();
    let mut matdefs = HashMap::new();
    for e in &g.elems {
        matdefs.insert(e.mat, glb.materials.len());
        glb.materials.push(plain_material(e.mat));
    }
    glb.nodes.push(json!({"name": "JadeZup_to_GltfYup", "rotation": [-0.70710678, 0.0, 0.0, 0.70710678], "children": [1]}));
    glb.scene_roots.push(0);
    glb.nodes.push(json!({"name": "JadeActor", "children": [], "extras": {"note": "gizmo -1 track (root motion) animates this node"}}));
    let mut node_of: HashMap<i32, usize> = HashMap::new();
    for b in &bones {
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
        node_of.insert(b.idx as i32, glb.nodes.len());
        glb.nodes.push(n);
    }
    for b in &bones {
        let ni = node_of[&(b.idx as i32)];
        let par = match b.parent {
            None => 1,
            Some(p) => node_of[&(p as i32)],
        };
        let kids = glb.nodes[par].as_object_mut().unwrap().entry("children").or_insert_with(|| json!([]));
        kids.as_array_mut().unwrap().push(json!(ni));
    }
    let mut skinned: Vec<&Bone> = bones.iter().filter(|b| b.skin_id.map(|s| smat(s).is_some()).unwrap_or(false)).collect();
    skinned.sort_by_key(|b| b.skin_id);
    let jindex: HashMap<usize, usize> = skinned.iter().enumerate().map(|(k, b)| (b.skin_id.unwrap(), k)).collect();
    let ibm_data: Vec<f32> = skinned.iter().flat_map(|b| smat(b.skin_id.unwrap()).unwrap().mat.iter().copied()).collect();
    let ibm = glb.acc_f32(&ibm_data, 16, "MAT4", None, false);
    glb.skins.push(json!({
        "name": sstr(recipe, "skin_name").unwrap_or("skin"),
        "joints": skinned.iter().map(|b| node_of[&(b.idx as i32)]).collect::<Vec<_>>(),
        "inverseBindMatrices": ibm, "skeleton": 1
    }));
    // top-4 influences per source vertex
    let mut infl: Vec<Vec<(f32, u16)>> = vec![Vec::new(); g.nverts];
    for l in skin {
        let j = *jindex.get(&(l.bone as usize)).ok_or_else(|| err!("{name}: skin list for unknown bone {}", l.bone))?;
        for (&vi, &w) in l.idx.iter().zip(&l.w) {
            if (vi as usize) < infl.len() {
                infl[vi as usize].push((w, j as u16));
            }
        }
    }
    let mut jn = vec![[0u16; 4]; g.nverts];
    let mut wt = vec![[0f32; 4]; g.nverts];
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
    let joints: Vec<u16> = flat.src.iter().flat_map(|&s| jn[s]).collect();
    let weights: Vec<f32> = flat.src.iter().flat_map(|&s| wt[s]).collect();
    let mesh_name = recipe["mesh_node"].as_str().unwrap_or("mesh");
    let mi = add_mesh(&mut glb, mesh_name, &flat, &matdefs, Some((&joints, &weights)));
    glb.nodes.push(json!({"name": mesh_name, "mesh": mi, "skin": 0}));
    let mesh_node = glb.nodes.len() - 1;
    glb.nodes[0]["children"].as_array_mut().unwrap().push(json!(mesh_node));

    // ---- clips
    let set = recipe["clips"].as_str().ok_or_else(|| err!("{name}: no clip set"))?;
    let defs = clipdb.sets.get(set).ok_or_else(|| err!("{name}: unknown clip set '{set}'"))?;
    let mut notes = Vec::new();
    let mut clip_data: Vec<ClipData> = Vec::new();
    let node_of_gizmo: HashMap<i32, usize> = node_of.clone();
    let mut last_stream = String::new();
    for def in defs {
        if def.stream != last_stream {
            log(&format!("clips: loading stream {}", def.stream));
            last_stream = def.stream.clone();
        }
        let d = match src.stream(&def.stream) {
            Ok(d) => d,
            Err(e) if opts.skip_missing_clips => {
                notes.push(format!("skipped {} ({e})", def.name));
                continue;
            }
            Err(e) => return Err(err!("{name}: clip {}: {e}", def.name)),
        };
        let r = anim::parse_trl(&d, def.off).map_err(|e| err!("{name}: clip {} @ {}:{:#x}: {}", def.name, def.stream, def.off, e.0))?;
        let ft = anim::features(&r);
        let (chans, missing) = channels_of(&r, &node_of_gizmo);
        clip_data.push(ClipData { def: def.clone(), frames: ft.frames, features: features_json(&ft), missing, chans, ntracks: r.n_anim });
    }
    let mut extra_files = Vec::new();
    if let Some(ip) = recipe.get("inplace") {
        let pg = ip["pelvis_gizmo"].as_i64().unwrap_or(0) as i32;
        let pelvis = *node_of.get(&pg).ok_or_else(|| err!("{name}: pelvis gizmo {pg} has no node"))?;
        let mut rm = Map::new();
        for c in clip_data.iter_mut() {
            let e = strip_root_motion(c, pelvis);
            rm.insert(format!("{}__{}", c.def.label, c.def.name), e);
        }
        if let Some(fname) = ip["rootmotion"].as_str() {
            extra_files.push((fname.to_string(), serde_json::to_vec_pretty(&Value::Object(rm))?));
        }
    }
    for c in &clip_data {
        let a = write_clip(&mut glb, c);
        glb.animations.push(a);
    }
    let _ = clip_data.iter().map(|c| c.ntracks).sum::<usize>();

    // ---- weapon socket (make_game_assets)
    if let Some(ws) = recipe.get("weapon_socket") {
        let parent = sstr(ws, "parent")?;
        let pi = glb.nodes.iter().position(|n| n["name"] == parent).ok_or_else(|| err!("{name}: socket parent {parent} not found"))?;
        let bind_t = glb.nodes[pi]["translation"].clone();
        let bind_r = glb.nodes[pi]["rotation"].clone();
        glb.nodes.push(json!({"name": "WeaponSocket", "translation": [0, 0, 0], "rotation": [0, 0, 0, 1], "extras": {
            "note": "parent weapon-mesh nodes here (weapon origin = socket origin, no extra offset). Child of the right-hand attach bone.",
            "hand_parent": ws["hand"], "anex01_bind_translation_in_MainD": bind_t, "anex01_bind_rotation_xyzw": bind_r }}));
        let si = glb.nodes.len() - 1;
        glb.nodes[pi].as_object_mut().unwrap().entry("children").or_insert_with(|| json!([])).as_array_mut().unwrap().push(json!(si));
    }
    if opts.with_textures {
        bind_textures(&mut glb, src, recipe, log)?;
    }
    let extras = json!({
        "source": "King Kong 2005 PC (Jade), rebuilt from the user's own copy by kk-extract", "rig": prefix, "fps": 60, "units": "metres",
        "note": "clip names are '<label>__<orig>'; labels are heuristic (anim_findings.md)",
        "textures": "baseColor+normal embedded (uv v not flipped); see texture_binding.md"
    });
    let mut files = vec![(name.to_string(), glb.finish(extras, "kk-extract"))];
    files.extend(extra_files);
    let _ = jf;
    let _ = jf_arr;
    Ok(Built { files, notes })
}

/// Convenience: all manifest assets (name, recipe).
pub fn manifest() -> Result<Vec<(String, Value)>> {
    let v: Value = serde_json::from_str(include_str!("manifest.json"))?;
    let a = v["assets"].as_object().ok_or_else(|| Error("manifest: no assets".into()))?;
    Ok(a.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_clip_tables() {
        let db = ClipDb::builtin();
        assert_eq!(db.sets["arms"].len(), 101);
        assert_eq!(db.sets["rex"].len(), 123);
        for set in db.sets.values() {
            let mut names: Vec<_> = set.iter().map(|c| format!("{}__{}", c.label, c.name)).collect();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), set.len(), "clip names must be unique");
        }
        assert_eq!(db.sets["arms"][0].stream, "ff0003eb");
        assert_eq!(db.sets["rex"][122].stream, "ff001793");
    }

    #[test]
    fn manifest_is_wellformed() {
        let m = manifest().unwrap();
        assert!(m.len() >= 6);
        let db = ClipDb::builtin();
        for (name, r) in &m {
            if r["kind"] != "static" && r["kind"] != "character" {
                continue; // kong / creature / sounds recipes have their own shapes
            }
            assert!(name.ends_with(".glb"));
            parse_num(r["geo_off"].as_str().unwrap()).unwrap();
            assert!(r["stream"].as_str().unwrap().starts_with("ff00"));
            assert!(r["bank"].as_str().unwrap().starts_with("ff80"));
            if r["kind"] == "character" {
                assert!(db.sets.contains_key(r["clips"].as_str().unwrap()));
            }
            for (_, t) in r["textures"].as_object().unwrap() {
                assert!(t["diffuse"]["idx"].is_u64() && t["normal"]["idx"].is_u64());
            }
        }
        assert_eq!(parse_num("0x10").unwrap(), 16);
        assert!(parse_num("zz").is_err());
    }

    #[test]
    fn channels_follow_the_python_rules() {
        let d = crate::anim::tests::sample();
        let r = anim::parse_trl(&d, 0).unwrap();
        let mut map = HashMap::new();
        map.insert(3, 7usize);
        let (ch, missing) = channels_of(&r, &map);
        assert!(missing.is_empty());
        assert_eq!(ch.len(), 2);
        // root translation -> JadeActor (node 1); times 0, 0 -> dedup keeps the later duplicate, then 10 frames
        assert_eq!((ch[0].node, ch[0].rotation), (1, false));
        assert_eq!(ch[0].times, vec![0.0, 10.0 / 60.0]);
        assert_eq!(ch[0].vals, vec![0.0, -2.0, 0.0, 0.0, -2.0, 0.0]);
        assert_eq!((ch[1].node, ch[1].rotation), (7, true));
        assert_eq!(ch[1].vals.len(), 8); // same dedup: 3 events at frames 0,0,10 -> 2 keys
        let (ch, missing) = channels_of(&r, &HashMap::new());
        assert_eq!(ch.len(), 1);
        assert_eq!(missing.into_iter().collect::<Vec<_>>(), vec![3]);
    }

    #[test]
    fn root_motion_strip() {
        let def = ClipDef { name: "rex_x".into(), stream: "ff".into(), off: 0, frames: 60, label: "walk".into(), conf: "low".into() };
        let mut c = ClipData {
            def,
            frames: 60,
            features: Value::Null,
            missing: BTreeSet::new(),
            ntracks: 2,
            chans: vec![
                Chan { node: 1, rotation: false, times: vec![0.0, 1.0], vals: vec![0.0, 0.0, 0.0, 0.0, -4.0, 0.0] },
                Chan { node: 2, rotation: false, times: vec![0.0, 0.25, 1.0], vals: vec![0.0, 1.0, 3.0, 0.0, 0.0, 3.0, 0.0, -3.0, 3.0] },
            ],
        };
        let e = strip_root_motion(&mut c, 2);
        assert_eq!(c.chans[0].vals, vec![0.0; 6]);
        // pelvis y = old - actor(t): t=.25 -> 0 - (-1) = 1, t=1 -> -3 + 4 = 1
        assert_eq!(c.chans[1].vals, vec![0.0, 1.0, 3.0, 0.0, 1.0, 3.0, 0.0, 1.0, 3.0]);
        assert_eq!(e["keys_actor"], 2);
        assert_eq!(e["speed_mps"].as_f64().unwrap(), 4.0);
        assert_eq!(e["jade_displacement"][1].as_f64().unwrap(), -4.0);
        assert_eq!(e["gltf_displacement"][2].as_f64().unwrap(), 4.0);
    }
}
