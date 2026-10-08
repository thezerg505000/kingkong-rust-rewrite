//! Parity tests: the Rust parsers against reference output of the original Python tools.
//! Needs KK_TEST_DATA=<dir> holding ff*.dec streams and ref_*.json (see tests/tools/make_reference.py);
//! every test is skipped (passes) when the variable is unset, so release CI needs no game data.

use kk_extract::{anim, geo, mat, records, skel, texture};
use serde_json::Value;
use std::path::PathBuf;

fn data_dir() -> Option<PathBuf> {
    std::env::var_os("KK_TEST_DATA").map(PathBuf::from).filter(|p| p.is_dir())
}
fn load_json(dir: &PathBuf, n: &str) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(n)).unwrap_or_else(|e| panic!("{n}: {e}"))).unwrap()
}
fn load_dec(dir: &PathBuf, st: &str) -> Vec<u8> {
    std::fs::read(dir.join(format!("{st}.dec"))).unwrap_or_else(|e| panic!("{st}.dec: {e}"))
}
fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * (1.0 + a.abs().max(b.abs()))
}
/// reference value or null (NaN in the Python output: only require that ours is not finite either)
fn closev(a: f64, w: &Value, rel: f64) -> bool {
    match w.as_f64() {
        Some(b) => close(a, b, rel),
        None => !a.is_finite(),
    }
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

#[test]
fn named_records_match_python() {
    let dir = skip_without_data!();
    let refs = load_json(&dir, "ref_names.json");
    for st in ["ff0003eb", "ff00018c"] {
        let d = load_dec(&dir, st);
        let got = records::iter_named(&d, 0, None);
        let want = refs[st].as_array().unwrap();
        assert_eq!(got.len(), want.len(), "{st}: record count");
        for (g, w) in got.iter().zip(want) {
            assert_eq!(g.name, w["name"].as_str().unwrap());
            assert_eq!(g.name_off as u64, w["name_off"].as_u64().unwrap());
            assert_eq!(g.payload as u64, w["payload"].as_u64().unwrap());
            assert_eq!(g.size as u64, w["size"].as_u64().unwrap());
            let pre: Vec<u64> = w["pre"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
            assert_eq!(g.pre.iter().map(|&x| x as u64).collect::<Vec<_>>(), pre);
        }
        eprintln!("{st}: {} named records identical", got.len());
    }
}

#[test]
fn geo_matches_python() {
    let dir = skip_without_data!();
    let refs = load_json(&dir, "ref_geo.json");
    for st in ["ff0003eb", "ff00018c"] {
        let d = load_dec(&dir, st);
        let offs = records::find_geo_magics(&d);
        let want = refs[st].as_array().unwrap();
        assert_eq!(offs.len(), want.len(), "{st}: GEO magic count");
        let (mut ok, mut skin, mut ok3) = (0, 0, 0);
        for (&o, w) in offs.iter().zip(want) {
            assert_eq!(o as u64, w["off"].as_u64().unwrap());
            let r = geo::parse_geo(&d, o);
            let want_ok = w["ok"].as_bool().unwrap();
            let g = match (r, want_ok) {
                (Ok(g), true) => g,
                (Err(_), false) => continue,
                (Ok(_), false) => panic!("{st}@{o:#x}: Rust parsed a GEO Python rejected ({})", w["err"]),
                (Err(e), true) => panic!("{st}@{o:#x}: Rust failed ({e}) where Python parsed"),
            };
            ok += 1;
            assert_eq!(g.nverts as u64, w["nverts"].as_u64().unwrap());
            assert_eq!(g.ncol as u64, w["ncol"].as_u64().unwrap());
            assert_eq!(g.nuv as u64, w["nuv"].as_u64().unwrap());
            assert_eq!(g.nelem as u64, w["nelem"].as_u64().unwrap());
            assert_eq!(g.f2 as u64, w["f2"].as_u64().unwrap());
            assert_eq!(g.end as u64, w["end"].as_u64().unwrap(), "{st}@{o:#x} end");
            if g.ok3_at.is_some() {
                ok3 += 1;
                assert_eq!(g.ok3_len as u64, w["ok3_len"].as_u64().unwrap(), "ok3 len");
            }
            let ps: f64 = g.pos.iter().flat_map(|p| p.iter()).map(|&x| x as f64).sum();
            let pa: f64 = g.pos.iter().flat_map(|p| p.iter()).map(|&x| (x as f64).abs()).sum();
            let ns: f64 = g.nrm.iter().flat_map(|p| p.iter()).map(|&x| x as f64).sum();
            let us: f64 = g.uv.iter().flat_map(|p| p.iter()).map(|&x| x as f64).sum();
            assert!(closev(ps, &w["pos_sum"], 1e-9), "pos sum");
            assert!(closev(pa, &w["pos_abs"], 1e-9));
            assert!(closev(ns, &w["nrm_sum"], 1e-9));
            assert!(closev(us, &w["uv_sum"], 1e-9));
            let we = w["elems"].as_array().unwrap();
            assert_eq!(g.elems.len(), we.len());
            for (e, x) in g.elems.iter().zip(we) {
                assert_eq!(e.mat as u64, x["mat"].as_u64().unwrap());
                assert_eq!(e.tri.len() as u64, x["ntri"].as_u64().unwrap());
                let vs: i64 = e.tri.iter().flat_map(|t| t.v.iter()).map(|&v| v as i64).sum();
                let us: i64 = e.tri.iter().flat_map(|t| t.u.iter()).map(|&v| v as i64).sum();
                assert_eq!(vs, x["vsum"].as_i64().unwrap());
                assert_eq!(us, x["usum"].as_i64().unwrap());
            }
            match (&g.tail, w["tail"].as_array()) {
                (Some(t), Some(wt)) => assert_eq!(t.iter().map(|&x| x as u64).collect::<Vec<_>>(), wt.iter().map(|v| v.as_u64().unwrap()).collect::<Vec<_>>()),
                (None, None) => {}
                _ => panic!("tail"),
            }
            if let Some(ws) = w.get("skin") {
                skin += 1;
                let sk = g.skin.as_ref().expect("skin");
                let ws = ws.as_array().unwrap();
                assert_eq!(sk.len(), ws.len());
                for (l, x) in sk.iter().zip(ws) {
                    assert_eq!(l.bone as u64, x["bone"].as_u64().unwrap());
                    assert_eq!(l.idx.len() as u64, x["n"].as_u64().unwrap());
                    assert_eq!(l.typ as i64, x["typ"].as_i64().unwrap());
                    assert_eq!(l.idx.iter().map(|&i| i as u64).sum::<u64>(), x["idx_sum"].as_u64().unwrap());
                    assert!(closev(l.w.iter().map(|&w| w as f64).sum(), &x["w_sum"], 1e-9));
                    for (a, b) in l.mat.iter().zip(x["mat"].as_array().unwrap()) {
                        assert!(b.is_null() && !a.is_finite() || b.as_f64().map(|v| v as f32) == Some(*a), "{st}@{o:#x} bone {} mat {a} vs {b}", l.bone);
                    }
                }
            } else {
                assert!(g.skin.is_none());
            }
            let fl = geo::flatten(&g).unwrap();
            let wf = &w["flat"];
            if wf.is_null() {
                assert_eq!(g.nuv, 0, "Python flatten only fails without UVs");
                continue;
            }
            assert_eq!(fl.pos.len() as u64, wf["nv"].as_u64().unwrap(), "flatten vertex count");
            let fs: f64 = fl.pos.iter().flat_map(|p| p.iter()).map(|&x| x as f64).sum();
            assert!(closev(fs, &wf["pos_sum"], 1e-9));
            for (e, s) in fl.elems.iter().zip(wf["idx_sums"].as_array().unwrap()) {
                assert_eq!(e.1.iter().map(|&i| i as i64).sum::<i64>(), s.as_i64().unwrap());
            }
        }
        eprintln!("{st}: {} GEO headers, {ok} parsed identically ({skin} skinned, {ok3} with OK3), {} rejected by both", offs.len(), offs.len() - ok);
    }
}

#[test]
fn skeleton_matches_python() {
    let dir = skip_without_data!();
    let refs = load_json(&dir, "ref_skel.json");
    for (st, off, pre, skip) in [("ff0003eb", 0x2073a6usize, "B_Jaf_", vec![]), ("ff00018c", 0xcb7d61, "B_Rex_", vec!["Snap", "Base", "Sang"])] {
        let d = load_dec(&dir, st);
        let g = geo::parse_geo(&d, off).unwrap();
        let bones = skel::load_rig(&d, &g, pre, &skip, off).unwrap();
        let want = refs[st].as_array().unwrap();
        assert_eq!(bones.len(), want.len());
        for (b, w) in bones.iter().zip(want) {
            assert_eq!(b.name, w["name"].as_str().unwrap());
            assert_eq!(b.idx as u64, w["idx"].as_u64().unwrap());
            assert_eq!(b.parent.map(|p| p as i64), w["parent"].as_i64());
            assert_eq!(b.skin_id.map(|p| p as i64), w["skin_id"].as_i64());
            assert_eq!(b.parent_key.map(|p| p as i64), w["parent_key"].as_i64());
            let wl = w["local"].as_array().unwrap();
            for i in 0..16 {
                assert!((b.local[i] - wl[i].as_f64().unwrap()).abs() < 1e-9, "{} local[{i}]: {} vs {}", b.name, b.local[i], wl[i]);
            }
        }
        // chain product reproduces the inverse skin matrices (anim_findings: 4e-7 / 2e-6)
        let w = skel::world_matrices(&bones);
        let mut worst: f64 = 0.0;
        for b in &bones {
            if let Some(sid) = b.skin_id {
                if let Some(l) = g.skin.as_ref().unwrap().iter().find(|l| l.bone as usize == sid) {
                    let inv = mat::inv(&mat::from_f32(&l.mat)).unwrap();
                    for i in 0..16 {
                        worst = worst.max((inv[i] - w[b.idx][i]).abs());
                    }
                }
            }
        }
        eprintln!("{st}: {} bones identical to Python, max |inv(skin) - chain world| = {worst:.2e}", bones.len());
        assert!(worst < 1e-3);
    }
}

#[test]
fn trl_scan_matches_python() {
    let dir = skip_without_data!();
    let refs = load_json(&dir, "ref_trls.json");
    for (st, want) in refs.as_object().unwrap() {
        let d = load_dec(&dir, st);
        let got = anim::find_trls(&d);
        let want = want.as_array().unwrap();
        assert_eq!(got.len(), want.len(), "{st}: track list count");
        for (g, w) in got.iter().zip(want) {
            assert_eq!(g.off as u64, w["off"].as_u64().unwrap());
            assert_eq!(g.size as u64, w["size"].as_u64().unwrap());
            assert_eq!(g.nt as u64, w["nt"].as_u64().unwrap());
            assert_eq!(g.nanim as u64, w["nanim"].as_u64().unwrap());
            assert_eq!(g.lf as u64, w["lf"].as_u64().unwrap());
            assert_eq!(g.tail, w["tail"].as_i64().unwrap());
            assert_eq!(g.frames as u64, w["frames"].as_u64().unwrap());
            let gz: Vec<i64> = w["gizmos"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
            assert_eq!(g.gizmos.iter().map(|&x| x as i64).collect::<Vec<_>>(), gz);
        }
        eprintln!("{st}: {} track lists identical", got.len());
    }
}

#[test]
fn clips_match_python_and_catalog() {
    let dir = skip_without_data!();
    let refs = load_json(&dir, "ref_clips.json");
    let (mut n_arms, mut n_rex) = (0, 0);
    for w in refs.as_array().unwrap() {
        let st = w["stream"].as_str().unwrap();
        let d = load_dec(&dir, st);
        let r = anim::parse_trl(&d, w["off"].as_u64().unwrap() as usize).unwrap();
        let name = w["name"].as_str().unwrap();
        assert_eq!(r.n_anim as u64, w["nAnim"].as_u64().unwrap(), "{name}");
        assert_eq!(r.num_tracks as u64, w["numTracks"].as_u64().unwrap());
        assert_eq!(r.consumed as u64, w["consumed"].as_u64().unwrap());
        assert_eq!(r.tail, w["tail"].as_i64().unwrap());
        let wt = w["tracks"].as_array().unwrap();
        assert_eq!(r.tracks.len(), wt.len());
        for (t, x) in r.tracks.iter().zip(wt) {
            assert_eq!(t.gizmo as i64, x["gizmo"].as_i64().unwrap());
            assert_eq!(t.flags as u64, x["flags"].as_u64().unwrap());
            assert_eq!(t.events.len() as u64, x["nev"].as_u64().unwrap());
            assert_eq!(anim::track_times(t).1 as u64, x["total"].as_u64().unwrap());
            let (mut ts, mut qs, mut nk) = (0.0f64, 0.0f64, 0u64);
            for e in &t.events {
                if let Some(k) = &e.key {
                    nk += 1;
                    if let Some(tv) = &k.t {
                        ts += tv[0].iter().map(|&v| v as f64).sum::<f64>();
                    }
                    if let Some(anim::Rot::Q(q)) = &k.q {
                        qs += q.iter().sum::<f64>();
                    }
                }
            }
            assert_eq!(nk, x["nkeys"].as_u64().unwrap());
            assert!(close(ts, x["tsum"].as_f64().unwrap(), 1e-9), "{name} tsum");
            assert!(close(qs, x["qsum"].as_f64().unwrap(), 1e-9), "{name} qsum");
        }
        let f = anim::features(&r);
        let wf = &w["features"];
        assert_eq!(f.frames as u64, wf["frames"].as_u64().unwrap());
        assert_eq!(f.frames as u64, w["frames"].as_u64().unwrap(), "{name}: catalog frame count");
        assert!(close(f.root_dist, wf["root_dist"].as_f64().unwrap(), 1e-9));
        // angles near 0 are acos-sensitive (1 ulp in the dot product = ~1e-6 deg)
        assert!((f.loop_err_deg - wf["loop_err_deg"].as_f64().unwrap()).abs() < 1e-3, "{name} loop_err {} vs {}", f.loop_err_deg, wf["loop_err_deg"]);
        if let Some(j) = wf.get("jaw_range_deg") {
            assert!((f.jaw_range_deg.unwrap() - j.as_f64().unwrap()).abs() < 1e-3, "{name} jaw");
        }
        if w["which"] == "arms" {
            n_arms += 1
        } else {
            n_rex += 1
        }
    }
    eprintln!("{n_arms} arms clips and {n_rex} rex clips (of those whose streams are available) identical to Python");
    assert_eq!(n_arms, 101, "arms: 101 clips (anim_findings)");
}

#[test]
fn texture_decoders_match_python() {
    let dir = skip_without_data!();
    if !dir.join("tex_cases.json").exists() {
        eprintln!("no tex_cases.json: skipping");
        return;
    }
    let cases = load_json(&dir, "tex_cases.json");
    let bank = std::fs::read(dir.join("tex_bank.bin")).unwrap();
    let marked = texture::marked_chunks(&bank);
    assert_eq!(marked.len(), cases.as_array().unwrap().len(), "marked chunk count (header-only and unmarked chunks must be skipped)");
    let (mut n_ok, mut n_fail) = (0, 0);
    for c in cases.as_array().unwrap() {
        let ord = c["ordinal"].as_u64().unwrap() as usize;
        let r = texture::bank_texture(&bank, ord);
        if !c["ok"].as_bool().unwrap() {
            assert!(r.is_err(), "ordinal {ord}: Python fails, Rust must too");
            n_fail += 1;
            continue;
        }
        let img = r.unwrap_or_else(|e| panic!("ordinal {ord} {c}: {e}"));
        let want = std::fs::read(dir.join(format!("tex_{ord:02}.rgba"))).unwrap();
        assert_eq!((img.w as u64, img.h as u64), (c["w"].as_u64().unwrap(), c["h"].as_u64().unwrap()));
        assert_eq!(img.key as u64, c["key"].as_u64().unwrap());
        assert_eq!(img.rgba.len(), want.len());
        let bad = img.rgba.iter().zip(&want).filter(|(a, b)| a != b).count();
        assert_eq!(bad, 0, "ordinal {ord} {} {}x{}: {bad} differing bytes", c["fmt"], img.w, img.h);
        n_ok += 1;
    }
    eprintln!("{n_ok} synthetic textures (DXT1/3/5, DXN, A8R8G8B8, L8) bit-identical to tex_decode.py; {n_fail} failing identically");
}
