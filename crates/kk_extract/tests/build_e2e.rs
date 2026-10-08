//! End-to-end: rebuild assets from the decoded streams and compare them structurally with the glb files
//! produced by the Python pipeline (`KK_TEST_DATA/ref_assets/*.glb`). Skipped without KK_TEST_DATA.
//! Textures cannot be compared here (the texture banks are not part of the test data); the texture
//! decoders have their own bit-exact tests in parity.rs and `png_binding_matches_reference_images` below.

use kk_extract::build::{self, BuildOpts, ClipDb, Source};
use kk_extract::glb::read_glb;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn data_dir() -> Option<PathBuf> {
    std::env::var_os("KK_TEST_DATA").map(PathBuf::from).filter(|p| p.is_dir())
}
macro_rules! skip_without_data {
    () => {
        match data_dir() {
            Some(d) => d,
            None => {
                eprintln!("KK_TEST_DATA not set: skipping");
                return;
            }
        }
    };
}

struct DirSource {
    dir: PathBuf,
    cache: HashMap<String, Arc<Vec<u8>>>,
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
        Err(kk_extract::Error(format!("bank {key} not available")))
    }
}

fn read_acc(js: &Value, bin: &[u8], i: usize) -> Vec<f32> {
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

fn close_all(a: &[f32], b: &[f32], tol: f32, what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: length");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!((x - y).abs() <= tol * (1.0 + x.abs().max(y.abs())), "{what}[{i}]: {x} vs {y}");
    }
}

fn fl(v: &Value) -> Vec<f32> {
    v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap() as f32).collect()).unwrap_or_default()
}

/// Compare a rebuilt glb with the reference; `only_anims`: restrict animation comparison to names present in ours.
fn compare(ours: &[u8], reference: &[u8], label: &str, textured_ref: bool) -> usize {
    let (a, abin) = read_glb(ours).unwrap();
    let (b, bbin) = read_glb(reference).unwrap();
    // nodes
    let (na, nb) = (a["nodes"].as_array().unwrap(), b["nodes"].as_array().unwrap());
    assert_eq!(na.len(), nb.len(), "{label}: node count");
    for (i, (x, y)) in na.iter().zip(nb).enumerate() {
        assert_eq!(x["name"], y["name"], "{label}: node {i} name");
        assert_eq!(x["children"], y["children"], "{label}: node {i} children");
        assert_eq!(x["mesh"], y["mesh"]);
        assert_eq!(x["skin"], y["skin"]);
        for k in ["translation", "rotation", "scale"] {
            close_all(&fl(&x[k]), &fl(&y[k]), 1e-6, &format!("{label}: node {} {k}", x["name"]));
        }
        assert_eq!(x["extras"]["jade_skin_id"], y["extras"]["jade_skin_id"]);
        assert_eq!(x["extras"]["jade_listing_index"], y["extras"]["jade_listing_index"]);
    }
    // skin
    if let Some(sa) = a["skins"].get(0) {
        let sb = &b["skins"][0];
        assert_eq!(sa["joints"], sb["joints"], "{label}: joints");
        assert_eq!(sa["skeleton"], sb["skeleton"]);
        close_all(&read_acc(&a, &abin, sa["inverseBindMatrices"].as_u64().unwrap() as usize), &read_acc(&b, &bbin, sb["inverseBindMatrices"].as_u64().unwrap() as usize), 0.0, "ibm");
    }
    // meshes
    let (ma, mb) = (&a["meshes"][0], &b["meshes"][0]);
    assert_eq!(ma["primitives"].as_array().unwrap().len(), mb["primitives"].as_array().unwrap().len(), "{label}: primitive count");
    for (pa, pb) in ma["primitives"].as_array().unwrap().iter().zip(mb["primitives"].as_array().unwrap()) {
        assert_eq!(pa["material"], pb["material"]);
        for attr in ["POSITION", "NORMAL", "TEXCOORD_0", "JOINTS_0", "WEIGHTS_0"] {
            if pa["attributes"].get(attr).is_none() {
                assert!(pb["attributes"].get(attr).is_none(), "{label}: {attr}");
                continue;
            }
            let va = read_acc(&a, &abin, pa["attributes"][attr].as_u64().unwrap() as usize);
            let vb = read_acc(&b, &bbin, pb["attributes"][attr].as_u64().unwrap() as usize);
            close_all(&va, &vb, if attr == "WEIGHTS_0" { 1e-6 } else { 0.0 }, &format!("{label}: {attr}"));
        }
        close_all(&read_acc(&a, &abin, pa["indices"].as_u64().unwrap() as usize), &read_acc(&b, &bbin, pb["indices"].as_u64().unwrap() as usize), 0.0, "indices");
    }
    let pos_acc = ma["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize;
    let pos_acc_b = mb["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize;
    close_all(&fl(&a["accessors"][pos_acc]["min"]), &fl(&b["accessors"][pos_acc_b]["min"]), 0.0, "pos min");
    close_all(&fl(&a["accessors"][pos_acc]["max"]), &fl(&b["accessors"][pos_acc_b]["max"]), 0.0, "pos max");
    // materials
    assert_eq!(a["materials"].as_array().unwrap().len(), b["materials"].as_array().unwrap().len());
    for (x, y) in a["materials"].as_array().unwrap().iter().zip(b["materials"].as_array().unwrap()) {
        assert_eq!(x["extras"]["jade_material_id"], y["extras"]["jade_material_id"]);
        assert_eq!(x["name"], y["name"]);
        let _ = textured_ref;
    }
    // animations: by name
    let mut compared = 0;
    let bmap: HashMap<&str, &Value> = b["animations"].as_array().map(|v| v.iter().map(|x| (x["name"].as_str().unwrap(), x)).collect()).unwrap_or_default();
    for x in a["animations"].as_array().unwrap_or(&vec![]) {
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
            close_all(&o1, &o2, 1e-5, &format!("{name}: values node {}", c1["target"]["node"]));
        }
        for k in ["original_name", "label", "label_confidence", "source", "frames"] {
            assert_eq!(x["extras"][k], y["extras"][k], "{name}: extras.{k}");
        }
        assert_eq!(x["extras"]["missing_gizmos"], y["extras"]["missing_gizmos"], "{name}: missing gizmos");
        compared += 1;
    }
    compared
}

fn ref_asset(dir: &PathBuf, name: &str) -> Option<Vec<u8>> {
    std::fs::read(dir.join("ref_assets").join(name)).ok()
}

#[test]
fn jack_arms_matches_python_glb() {
    let dir = skip_without_data!();
    let Some(reference) = ref_asset(&dir, "jack_fps_arms.glb") else {
        eprintln!("no ref_assets/jack_fps_arms.glb: skipping");
        return;
    };
    let mut src = DirSource { dir: dir.clone(), cache: HashMap::new() };
    let recipes: HashMap<String, Value> = build::manifest().unwrap().into_iter().collect();
    let opts = BuildOpts { with_textures: false, skip_missing_clips: false };
    let built = build::build_asset(&mut src, "jack_fps_arms.glb", &recipes["jack_fps_arms.glb"], &ClipDb::builtin(), &opts, &mut |_| {}).unwrap();
    let n = compare(&built.files[0].1, &reference, "arms", true);
    assert_eq!(n, 101, "arms clips");
    eprintln!("jack_fps_arms.glb: nodes, skin, mesh (all vertex attributes), {n} clips match the Python glb");
}

#[test]
fn weapons_match_python_glb() {
    let dir = skip_without_data!();
    let mut src = DirSource { dir: dir.clone(), cache: HashMap::new() };
    let recipes: HashMap<String, Value> = build::manifest().unwrap().into_iter().collect();
    let opts = BuildOpts { with_textures: false, skip_missing_clips: false };
    let mut n = 0;
    for w in ["luger", "tommygun", "shotgun", "sniperrifle"] {
        let name = format!("jack_fps_{w}.glb");
        let Some(reference) = ref_asset(&dir, &name) else { continue };
        let built = build::build_asset(&mut src, &name, &recipes[&name], &ClipDb::builtin(), &opts, &mut |_| {}).unwrap();
        compare(&built.files[0].1, &reference, &name, true);
        n += 1;
    }
    eprintln!("{n} weapon glbs match");
}

#[test]
fn trex_inplace_matches_python_glb() {
    let dir = skip_without_data!();
    let Some(reference) = ref_asset(&dir, "trex_inplace.glb") else {
        eprintln!("no ref_assets/trex_inplace.glb: skipping");
        return;
    };
    let mut src = DirSource { dir: dir.clone(), cache: HashMap::new() };
    let recipes: HashMap<String, Value> = build::manifest().unwrap().into_iter().collect();
    // only 3 of the 6 rex clip streams are in the test data: the other clips are skipped
    let opts = BuildOpts { with_textures: false, skip_missing_clips: true };
    let built = build::build_asset(&mut src, "trex_inplace.glb", &recipes["trex_inplace.glb"], &ClipDb::builtin(), &opts, &mut |_| {}).unwrap();
    let n = compare(&built.files[0].1, &reference, "trex", true);
    assert_eq!(n, 45, "rex clips from the available streams");
    // root motion json
    let rm: Value = serde_json::from_slice(&built.files[1].1).unwrap();
    let want: Value = serde_json::from_slice(&std::fs::read(dir.join("ref_assets/trex_rootmotion.json")).unwrap()).unwrap();
    let mut cmp = 0;
    for (k, v) in rm.as_object().unwrap() {
        let w = &want[k];
        assert!(!w.is_null(), "{k} missing in reference rootmotion");
        for f in ["duration_s", "speed_mps"] {
            assert!((v[f].as_f64().unwrap() - w[f].as_f64().unwrap()).abs() < 1e-5, "{k}.{f}: {} vs {}", v[f], w[f]);
        }
        for f in ["jade_displacement", "gltf_displacement"] {
            for i in 0..3 {
                assert!((v[f][i].as_f64().unwrap() - w[f][i].as_f64().unwrap()).abs() < 1e-5, "{k}.{f}[{i}]");
            }
        }
        assert_eq!(v["keys_actor"], w["keys_actor"], "{k}");
        cmp += 1;
    }
    eprintln!("trex_inplace.glb: skeleton, mesh, {n} in-place clips and {cmp} root-motion entries match the Python output");
}

#[test]
fn png_binding_matches_reference_images() {
    // The Python-decoded PNGs of the arms textures (kkpc/textures/ff8003eb/idx063 + idx064) must
    // reproduce the diffuse/normal images embedded in the Python glb when run through our binders.
    let dir = skip_without_data!();
    let (Some(reference), Ok(d63), Ok(d64)) = (ref_asset(&dir, "jack_fps_arms.glb"), std::fs::read(dir.join("ref_assets/idx063.png")), std::fs::read(dir.join("ref_assets/idx064.png"))) else {
        eprintln!("reference textures not present: skipping");
        return;
    };
    let (js, bin) = read_glb(&reference).unwrap();
    let image_bytes = |i: usize| {
        let bv = &js["bufferViews"][js["images"][i]["bufferView"].as_u64().unwrap() as usize];
        let o = bv["byteOffset"].as_u64().unwrap() as usize;
        bin[o..o + bv["byteLength"].as_u64().unwrap() as usize].to_vec()
    };
    let to_img = |png: &[u8]| {
        let (w, h, ct, data) = kk_extract::texture::decode_png(png).unwrap();
        let rgba = match ct {
            png::ColorType::Rgba => data,
            png::ColorType::Rgb => data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            c => panic!("{c:?}"),
        };
        kk_extract::texture::Image { w, h, rgba, fmt: kk_extract::texture::Fmt::Dxt1, mips: 1, key: 0 }
    };
    let diff = to_img(&d63).to_png_rgb().unwrap();
    let norm = to_img(&d64).to_png_normal().unwrap();
    let pix = |png: &[u8]| kk_extract::texture::decode_png(png).unwrap();
    let (rw, rh, _, rd) = pix(&image_bytes(0));
    let (ow, oh, _, od) = pix(&diff);
    assert_eq!((rw, rh), (ow, oh));
    assert_eq!(rd, od, "diffuse image");
    let (rw, rh, _, rn) = pix(&image_bytes(1));
    let (ow, oh, _, on) = pix(&norm);
    assert_eq!((rw, rh), (ow, oh));
    let bad = rn.iter().zip(&on).filter(|(a, b)| (**a as i32 - **b as i32).abs() > 1).count();
    assert_eq!(bad, 0, "normal image: {bad} bytes differ by more than 1");
}
