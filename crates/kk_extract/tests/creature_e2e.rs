//! Kong / Ann / creature rebuilds vs the glb/json produced by the Python tools. Skipped without KK_TEST_DATA.
mod common;
use common::*;
use kk_extract::build::{self, BuildOpts, ClipDb};
use serde_json::Value;
use std::collections::HashMap;

fn recipes() -> HashMap<String, Value> {
    build::manifest().unwrap().into_iter().collect()
}
fn ref_bytes(dir: &std::path::PathBuf, name: &str) -> Option<Vec<u8>> {
    std::fs::read(dir.join("ref_assets").join(name)).ok()
}
fn ref_json(dir: &std::path::PathBuf, name: &str) -> Option<Value> {
    ref_bytes(dir, name).and_then(|b| serde_json::from_slice(&b).ok())
}
fn build_one(dir: &std::path::PathBuf, name: &str) -> build::Built {
    let mut src = DirSource::new(dir);
    let opts = BuildOpts { with_textures: false, skip_missing_clips: false };
    build::build_asset(&mut src, name, &recipes()[name], &ClipDb::builtin(), &opts, &mut |_| {}).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn kong_matches_python() {
    let dir = skip_without_data!();
    let Some(reference) = ref_bytes(&dir, "kong.glb") else { return eprintln!("no ref kong.glb") };
    let built = build_one(&dir, "kong/kong.glb");
    let c = Cmp {
        anim_extras: &["label", "label_confidence", "label_evidence", "source", "frames", "trl_index", "loop"],
        anim_extras_num: &[("pose_loop_err_deg", 0.011), ("pose_loop_dt_m", 0.0011), ("seconds", 1e-4)],
        subset: false,
        ..Default::default()
    };
    let n = compare_glb(&built.files[0].1, &reference, "kong", &c);
    assert_eq!(n, 254);
    let get = |suffix: &str| built.files.iter().find(|(f, _)| f.ends_with(suffix)).map(|x| serde_json::from_slice::<Value>(&x.1).unwrap()).unwrap();
    let rm = get("kong_rootmotion.json");
    let k = compare_json_numbers(&rm, &ref_json(&dir, "kong_rootmotion.json").unwrap(), "kong_rootmotion", 1e-5);
    assert_eq!(k, 254);
    let ac = get("kong_actions.json");
    let want = ref_json(&dir, "kong_actions.json").unwrap();
    assert_eq!(ac, want, "kong_actions.json");
    eprintln!("kong.glb: rig, 4-part skinned mesh, {n} clips, {k} rootmotion entries, {} actions match the Python output", ac.as_object().unwrap().len());
}

#[test]
fn ann_matches_python() {
    let dir = skip_without_data!();
    let Some(reference) = ref_bytes(&dir, "ann.glb") else { return eprintln!("no ref ann.glb") };
    let built = build_one(&dir, "kong/ann.glb");
    let c = Cmp { anim_extras: &["source", "frames"], anim_extras_num: &[("seconds", 1e-4)], subset: false, ..Default::default() };
    let n = compare_glb(&built.files[0].1, &reference, "ann", &c);
    assert_eq!(n, 55);
    let rm: Value = serde_json::from_slice(&built.files[1].1).unwrap();
    let k = compare_json_numbers(&rm, &ref_json(&dir, "ann_rootmotion.json").unwrap(), "ann_rootmotion", 1e-5);
    assert_eq!(k, 55);
    eprintln!("ann.glb: rig, mesh, {n} clips and {k} rootmotion entries match");
}

/// The Python-decoded PNGs of the Kong textures (idx171/172 body+arms, idx175/176 head) run through our binders must
/// reproduce the images embedded in kong.glb (diffuse exact, normal +-1: the Python normal rebuild truncates, ours rounds).
#[test]
fn kong_texture_binding_matches_reference_images() {
    let dir = skip_without_data!();
    let Some(reference) = ref_bytes(&dir, "kong.glb") else { return eprintln!("no ref kong.glb") };
    let (js, bin) = kk_extract::glb::read_glb(&reference).unwrap();
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
    let mut checked = 0;
    for (img_d, img_n, idx_d, idx_n) in [(0usize, 1usize, 171, 172), (6, 7, 175, 176)] {
        let (Ok(d), Ok(n)) = (std::fs::read(dir.join(format!("ref_assets/idx{idx_d}.png"))), std::fs::read(dir.join(format!("ref_assets/idx{idx_n}.png")))) else {
            return eprintln!("reference textures not present: skipping");
        };
        let pix = |png: &[u8]| kk_extract::texture::decode_png(png).unwrap();
        let diff = to_img(&d).to_png_rgb().unwrap();
        let norm = to_img(&n).to_png_normal().unwrap();
        let (rw, rh, _, rd) = pix(&image_bytes(img_d));
        let (ow, oh, _, od) = pix(&diff);
        assert_eq!((rw, rh), (ow, oh));
        assert_eq!(rd, od, "diffuse idx{idx_d}");
        let (rw, rh, _, rn) = pix(&image_bytes(img_n));
        let (ow, oh, _, on) = pix(&norm);
        assert_eq!((rw, rh), (ow, oh));
        let bad = rn.iter().zip(&on).filter(|(a, b)| (**a as i32 - **b as i32).abs() > 1).count();
        assert_eq!(bad, 0, "normal idx{idx_n}: {bad} bytes differ by more than 1");
        checked += 1;
    }
    assert_eq!(checked, 2);
}

fn check_creature(dir: &std::path::PathBuf, model: &str, want_clips: usize) {
    let glb_name = format!("creatures/{model}.glb");
    let Some(reference) = ref_bytes(dir, &format!("{model}.glb")) else { return eprintln!("no ref {model}.glb") };
    let built = build_one(dir, &glb_name);
    let c = Cmp {
        anim_extras: &["label", "label_confidence", "source", "frames", "index", "root_motion_in_pelvis"],
        anim_extras_num: &[],
        subset: false,
        ..Default::default()
    };
    let n = compare_glb(&built.files[0].1, &reference, model, &c);
    assert_eq!(n, want_clips, "{model} clips");
    let rm: Value = serde_json::from_slice(&built.files[1].1).unwrap();
    let k = compare_json_numbers(&rm, &ref_json(dir, &format!("{model}_rootmotion.json")).unwrap(), &format!("{model}_rootmotion"), 1e-5);
    assert_eq!(k, want_clips);
    eprintln!("{model}.glb: rig, mesh, {n} clips and {k} rootmotion entries match the Python output ({})", built.notes.join("; "));
}

#[test]
fn raptor_matches_python() {
    let dir = skip_without_data!();
    check_creature(&dir, "raptor", 88);
}
#[test]
fn compy_matches_python() {
    let dir = skip_without_data!();
    check_creature(&dir, "compy", 88);
}
#[test]
fn raptor_kong_matches_python() {
    let dir = skip_without_data!();
    check_creature(&dir, "raptor_kong", 78);
}
#[test]
fn brontosaurus_matches_python() {
    let dir = skip_without_data!();
    check_creature(&dir, "brontosaurus", 3);
}
#[test]
fn crab_matches_python() {
    let dir = skip_without_data!();
    if !dir.join("ff002001.dec").exists() {
        return eprintln!("no ff002001.dec: skipping");
    }
    check_creature(&dir, "crab", 21);
}

#[test]
fn creature_tables_match_python() {
    let dir = skip_without_data!();
    let Some(want_man) = ref_json(&dir, "creatures_manifest.json") else { return eprintln!("no ref manifest") };
    let built = build_one(&dir, "creatures/manifest.json");
    let mut seen = 0;
    for (f, bytes) in &built.files {
        let got: Value = serde_json::from_slice(bytes).unwrap();
        if f.ends_with("/manifest.json") {
            for (k, v) in got.as_object().unwrap() {
                for field in ["species_id", "height_m", "length_m", "width_m", "runtime_scale", "clips", "clip_count"] {
                    assert_eq!(v[field], want_man[k][field], "manifest {k}.{field}");
                }
                assert_eq!(v["glb"], format!("creatures/{k}.glb"));
            }
        } else {
            let base = f.rsplit('/').next().unwrap();
            let want = ref_json(&dir, base).unwrap_or_else(|| panic!("no ref {base}"));
            assert_eq!(got, want, "{f}");
        }
        seen += 1;
    }
    assert_eq!(seen, 5);
}
