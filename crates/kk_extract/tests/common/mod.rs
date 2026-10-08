//! Helpers shared by the parity tests: data directory, a Source over decoded `.dec` streams, glb comparison.
#![allow(dead_code)]
use kk_extract::build::Source;
use kk_extract::glb::read_glb;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("KK_TEST_DATA").map(PathBuf::from).filter(|p| p.is_dir())
}

#[macro_export]
macro_rules! skip_without_data {
    () => {
        match common::data_dir() {
            Some(d) => d,
            None => {
                eprintln!("KK_TEST_DATA not set: skipping");
                return;
            }
        }
    };
}

pub struct DirSource {
    pub dir: PathBuf,
    pub cache: HashMap<String, Arc<Vec<u8>>>,
}
impl DirSource {
    pub fn new(dir: &PathBuf) -> DirSource {
        DirSource { dir: dir.clone(), cache: HashMap::new() }
    }
}
impl Source for DirSource {
    fn stream(&mut self, key: &str) -> kk_extract::Result<Arc<Vec<u8>>> {
        if let Some(a) = self.cache.get(key) {
            return Ok(a.clone());
        }
        let d = Arc::new(std::fs::read(self.dir.join(format!("{key}.dec"))).map_err(|e| kk_extract::Error(format!("{key}: {e}")))?);
        self.cache.insert(key.to_string(), d.clone());
        Ok(d)
    }
    fn bank(&mut self, key: &str) -> kk_extract::Result<Arc<Vec<u8>>> {
        // decoded banks are optional test data (<key>.dec, the bank keys are ff80xxxx)
        match std::fs::read(self.dir.join(format!("{key}.dec"))) {
            Ok(d) => Ok(Arc::new(d)),
            Err(_) => Err(kk_extract::Error(format!("bank {key} not available"))),
        }
    }
}

pub fn read_acc(js: &Value, bin: &[u8], i: usize) -> Vec<f32> {
    let a = &js["accessors"][i];
    let bv = &js["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
    let off = bv["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
    let comps = match a["type"].as_str().unwrap() {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT4" => 16,
        t => panic!("{t}"),
    };
    let n = a["count"].as_u64().unwrap() as usize * comps;
    match a["componentType"].as_u64().unwrap() {
        5126 => (0..n).map(|k| f32::from_le_bytes(bin[off + 4 * k..off + 4 * k + 4].try_into().unwrap())).collect(),
        5123 => (0..n).map(|k| u16::from_le_bytes(bin[off + 2 * k..off + 2 * k + 2].try_into().unwrap()) as f32).collect(),
        5125 => (0..n).map(|k| u32::from_le_bytes(bin[off + 4 * k..off + 4 * k + 4].try_into().unwrap()) as f32).collect(),
        c => panic!("{c}"),
    }
}

pub fn close_all(a: &[f32], b: &[f32], tol: f32, what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: length");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!((x - y).abs() <= tol * (1.0 + x.abs().max(y.abs())), "{what}[{i}]: {x} vs {y}");
    }
}

pub fn fl(v: &Value) -> Vec<f32> {
    v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap() as f32).collect()).unwrap_or_default()
}

pub struct Cmp {
    /// extras keys of animations to compare for equality
    pub anim_extras: &'static [&'static str],
    /// extras keys compared numerically with the given absolute tolerance
    pub anim_extras_num: &'static [(&'static str, f64)],
    /// compare only the animations present in ours (a subset of the reference)
    pub subset: bool,
    pub vertex_tol: f32,
    pub anim_tol: f32,
}
impl Default for Cmp {
    fn default() -> Self {
        Cmp { anim_extras: &["label", "label_confidence", "source", "frames"], anim_extras_num: &[], subset: true, vertex_tol: 0.0, anim_tol: 1e-5 }
    }
}

/// Structural comparison of two glb files. Returns the number of animations compared.
pub fn compare_glb(ours: &[u8], reference: &[u8], label: &str, c: &Cmp) -> usize {
    let (a, abin) = read_glb(ours).unwrap();
    let (b, bbin) = read_glb(reference).unwrap();
    let (na, nb) = (a["nodes"].as_array().unwrap(), b["nodes"].as_array().unwrap());
    assert_eq!(na.len(), nb.len(), "{label}: node count");
    for (i, (x, y)) in na.iter().zip(nb).enumerate() {
        assert_eq!(x["name"], y["name"], "{label}: node {i} name");
        assert_eq!(x["children"], y["children"], "{label}: node {i} children");
        assert_eq!(x["mesh"], y["mesh"]);
        assert_eq!(x["skin"], y["skin"]);
        for k in ["translation", "rotation", "scale"] {
            close_all(&fl(&x[k]), &fl(&y[k]), 1e-5, &format!("{label}: node {} {k}", x["name"]));
        }
        assert_eq!(x["extras"]["jade_skin_id"], y["extras"]["jade_skin_id"], "{label}: node {i} skin id");
        assert_eq!(x["extras"]["jade_listing_index"], y["extras"]["jade_listing_index"]);
    }
    if let Some(sa) = a["skins"].get(0) {
        let sb = &b["skins"][0];
        assert_eq!(sa["joints"], sb["joints"], "{label}: joints");
        assert_eq!(sa["skeleton"], sb["skeleton"]);
        close_all(&read_acc(&a, &abin, sa["inverseBindMatrices"].as_u64().unwrap() as usize), &read_acc(&b, &bbin, sb["inverseBindMatrices"].as_u64().unwrap() as usize), 0.0, "ibm");
    }
    assert_eq!(a["meshes"].as_array().unwrap().len(), b["meshes"].as_array().unwrap().len(), "{label}: mesh count");
    let (ma, mb) = (&a["meshes"][0], &b["meshes"][0]);
    assert_eq!(ma["primitives"].as_array().unwrap().len(), mb["primitives"].as_array().unwrap().len(), "{label}: primitive count");
    for (pa, pb) in ma["primitives"].as_array().unwrap().iter().zip(mb["primitives"].as_array().unwrap()) {
        assert_eq!(pa["material"], pb["material"], "{label}: primitive material");
        for attr in ["POSITION", "NORMAL", "TEXCOORD_0", "JOINTS_0", "WEIGHTS_0"] {
            if pa["attributes"].get(attr).is_none() {
                assert!(pb["attributes"].get(attr).is_none(), "{label}: {attr}");
                continue;
            }
            let va = read_acc(&a, &abin, pa["attributes"][attr].as_u64().unwrap() as usize);
            let vb = read_acc(&b, &bbin, pb["attributes"][attr].as_u64().unwrap() as usize);
            close_all(&va, &vb, if attr == "WEIGHTS_0" { 1e-6 } else { c.vertex_tol }, &format!("{label}: {attr}"));
        }
        close_all(&read_acc(&a, &abin, pa["indices"].as_u64().unwrap() as usize), &read_acc(&b, &bbin, pb["indices"].as_u64().unwrap() as usize), 0.0, "indices");
    }
    let pos_acc = ma["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize;
    let pos_acc_b = mb["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize;
    close_all(&fl(&a["accessors"][pos_acc]["min"]), &fl(&b["accessors"][pos_acc_b]["min"]), 0.0, "pos min");
    close_all(&fl(&a["accessors"][pos_acc]["max"]), &fl(&b["accessors"][pos_acc_b]["max"]), 0.0, "pos max");
    assert_eq!(a["materials"].as_array().unwrap().len(), b["materials"].as_array().unwrap().len(), "{label}: material count");
    for (x, y) in a["materials"].as_array().unwrap().iter().zip(b["materials"].as_array().unwrap()) {
        assert_eq!(x["name"], y["name"]);
        for k in ["kk_diffuse_png", "kk_normal_png"] {
            if !y["extras"][k].is_null() && !y["extras"][k].as_str().unwrap_or("").starts_with("ann_") {
                assert_eq!(x["extras"][k], y["extras"][k], "{label}: material {} extras.{k}", x["name"]);
            }
        }
    }
    let anims_a = a["animations"].as_array().cloned().unwrap_or_default();
    let anims_b = b["animations"].as_array().cloned().unwrap_or_default();
    if !c.subset {
        assert_eq!(anims_a.len(), anims_b.len(), "{label}: animation count");
    }
    let bmap: HashMap<&str, &Value> = anims_b.iter().map(|x| (x["name"].as_str().unwrap(), x)).collect();
    let mut compared = 0;
    for x in &anims_a {
        let name = x["name"].as_str().unwrap();
        let y = bmap.get(name).unwrap_or_else(|| panic!("{label}: clip {name} not in reference"));
        let (cx, cy) = (x["channels"].as_array().unwrap(), y["channels"].as_array().unwrap());
        assert_eq!(cx.len(), cy.len(), "{name}: channel count");
        for (c1, c2) in cx.iter().zip(cy) {
            assert_eq!(c1["target"], c2["target"], "{name}: target");
            let (s1, s2) = (&x["samplers"][c1["sampler"].as_u64().unwrap() as usize], &y["samplers"][c2["sampler"].as_u64().unwrap() as usize]);
            let i1 = read_acc(&a, &abin, s1["input"].as_u64().unwrap() as usize);
            let i2 = read_acc(&b, &bbin, s2["input"].as_u64().unwrap() as usize);
            close_all(&i1, &i2, 1e-6, &format!("{name}: times"));
            let o1 = read_acc(&a, &abin, s1["output"].as_u64().unwrap() as usize);
            let o2 = read_acc(&b, &bbin, s2["output"].as_u64().unwrap() as usize);
            close_all(&o1, &o2, c.anim_tol, &format!("{name}: values node {}", c1["target"]["node"]));
        }
        for k in c.anim_extras {
            assert_eq!(x["extras"][*k], y["extras"][*k], "{name}: extras.{k}");
        }
        for (k, tol) in c.anim_extras_num {
            let (p, q) = (x["extras"][*k].as_f64().unwrap_or(f64::NAN), y["extras"][*k].as_f64().unwrap_or(f64::NAN));
            assert!((p - q).abs() <= *tol, "{name}: extras.{k}: {p} vs {q}");
        }
        assert_eq!(x["extras"]["missing_gizmos"], y["extras"]["missing_gizmos"], "{name}: missing gizmos");
        compared += 1;
    }
    compared
}

/// Compare two rootmotion-style JSON maps: every entry of `ours` must exist in the reference with equal numeric fields.
pub fn compare_json_numbers(ours: &Value, reference: &Value, what: &str, tol: f64) -> usize {
    fn walk(a: &Value, b: &Value, path: &str, tol: f64) {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => {
                let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
                assert!((x - y).abs() <= tol * (1.0 + x.abs().max(y.abs())), "{path}: {x} vs {y}");
            }
            (Value::Array(x), Value::Array(y)) => {
                assert_eq!(x.len(), y.len(), "{path}: array length");
                for (i, (p, q)) in x.iter().zip(y).enumerate() {
                    walk(p, q, &format!("{path}[{i}]"), tol);
                }
            }
            (Value::Object(x), Value::Object(y)) => {
                assert_eq!(x.len(), y.len(), "{path}: keys {:?} vs {:?}", x.keys().collect::<Vec<_>>(), y.keys().collect::<Vec<_>>());
                for (k, v) in x {
                    walk(v, y.get(k).unwrap_or_else(|| panic!("{path}.{k} missing in reference")), &format!("{path}.{k}"), tol);
                }
            }
            _ => assert_eq!(a, b, "{path}"),
        }
    }
    let mut n = 0;
    for (k, v) in ours.as_object().unwrap() {
        let w = reference.get(k).unwrap_or_else(|| panic!("{what}: {k} missing in reference"));
        walk(v, w, &format!("{what}.{k}"), tol);
        n += 1;
    }
    n
}
