//! Sound rebuild vs the Python-era outputs (sound_defs.json, ffmpeg-decoded PCM). A synthetic Sound_Common.bf index is
//! assembled from KK_TEST_DATA/snd (raw .smd files, original ADPCM .wav files, smd_refs.json = BF paths per .smd).
//! Skipped without KK_TEST_DATA.
mod common;
use kk_extract::build::{self, BfEntry, BuildOpts, ClipDb, Source};
use kk_extract::sound::{decode_wav, parse_smd};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

struct SndSource {
    index: Vec<BfEntry>,
    data: Vec<Vec<u8>>,
}
impl Source for SndSource {
    fn stream(&mut self, k: &str) -> kk_extract::Result<Arc<Vec<u8>>> {
        Err(kk_extract::Error(format!("no stream {k}")))
    }
    fn bank(&mut self, k: &str) -> kk_extract::Result<Arc<Vec<u8>>> {
        Err(kk_extract::Error(format!("no bank {k}")))
    }
    fn sound_index(&mut self) -> kk_extract::Result<Vec<BfEntry>> {
        Ok(self.index.clone())
    }
    fn sound_read(&mut self, i: usize) -> kk_extract::Result<Vec<u8>> {
        Ok(self.data[i].clone())
    }
}

fn synth(dir: &PathBuf) -> Option<SndSource> {
    let snd = dir.join("snd");
    let refs: HashMap<String, Vec<String>> = serde_json::from_slice(&std::fs::read(snd.join("smd_refs.json")).ok()?).ok()?;
    let mut s = SndSource { index: vec![], data: vec![] };
    let mut seen = std::collections::HashSet::new();
    let mut add = |s: &mut SndSource, path: &str, key: u32, bytes: Vec<u8>| {
        if seen.insert((path.to_string(), key)) {
            s.index.push(BfEntry { path: path.to_string(), key });
            s.data.push(bytes);
        }
    };
    for (file, paths) in &refs {
        let smd = std::fs::read(snd.join("raw").join(file)).ok()?;
        let d = parse_smd(&smd).ok()?;
        add(&mut s, &format!("Synth/{file}"), d.own_key, smd);
        let (mut wi, mut fi, mut ii) = (0, 0, 0);
        let fades = [d.fade_in, d.fade_out].into_iter().filter(|&k| k != 0xffff_ffff && k != 0xcafe_deca).collect::<Vec<_>>();
        for p in paths.iter().filter(|p| !p.ends_with(".smd")) {
            let ext = p.rsplit('.').next().unwrap();
            match ext {
                "wav" => {
                    let key = d.wav_keys[wi];
                    wi += 1;
                    let f = snd.join("wav").join(p.replace('/', "__"));
                    add(&mut s, p, key, std::fs::read(f).ok()?);
                }
                "fad" => {
                    if let Some(&k) = fades.get(fi) {
                        add(&mut s, p, k, vec![0; 4]);
                    }
                    fi += 1;
                }
                "ins" => {
                    if let Some(&k) = d.insert_keys.get(ii) {
                        add(&mut s, p, k, vec![0; 4]);
                    }
                    ii += 1;
                }
                _ => {}
            }
        }
    }
    // ambience / thunder (wac/waa)
    for n in ["Amb_03E_area_a_forest.waa", "Amb_03E_area_b_arena.waa", "Rain.wac", "Thunder.wac"] {
        add(&mut s, &format!("Ambience/{n}"), 0x1234_0000, std::fs::read(snd.join("amb").join(n)).ok()?);
    }
    Some(s)
}

#[test]
fn sounds_match_the_python_outputs() {
    let dir = skip_without_data!();
    let Some(mut src) = synth(&dir) else { return eprintln!("no snd test data") };
    let recipe: Value = build::manifest().unwrap().into_iter().find(|(n, _)| n == "sound_defs.json").unwrap().1;
    let b = build::build_asset(&mut src, "sound_defs.json", &recipe, &ClipDb::builtin(), &BuildOpts::default(), &mut |_| {}).unwrap();
    let files: HashMap<_, _> = b.files.iter().cloned().collect();
    // definitions: reference with .ogg -> .wav
    let want: Value = serde_json::from_slice(&std::fs::read(dir.join("ref_assets/sound_defs.json")).unwrap()).unwrap();
    let got: Value = serde_json::from_slice(&files["sound_defs.json"]).unwrap();
    let (w, g) = (want.as_object().unwrap(), got.as_object().unwrap());
    let mut missing = vec![];
    let mut bad = vec![];
    for (k, v) in w {
        let Some(x) = g.get(k) else { missing.push(k.clone()); continue };
        let wf: Vec<String> = v["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().replace(".ogg", ".wav")).collect();
        let gf: Vec<String> = x["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect();
        let num = |v: &Value, k: &str| v[k].as_f64().unwrap();
        if wf != gf || (num(v, "volume") - num(x, "volume")).abs() > 1e-9 || (num(v, "f1") - num(x, "f1")).abs() > 1e-9 {
            bad.push(k.clone());
        }
        if v["fades"] != x["fades"] || v["inserts"] != x["inserts"] {
            bad.push(format!("{k} (fades/inserts)"));
        }
    }
    eprintln!("sound defs: {} reference, {} built, missing {:?}, mismatching {:?}", w.len(), g.len(), missing, bad);
    assert!(missing.is_empty() && bad.is_empty());
    // every referenced wave exists and the ffmpeg-decoded PCM matches sample for sample
    let mut cmp = 0;
    for (name, bytes) in &b.files {
        let Some(stem) = name.strip_prefix("sounds/") else { continue };
        let ours = decode_wav(bytes).unwrap();
        // ffmpeg reference named by BF path (Dir__Sub__Name.wav); compare by trailing name when unique
        let Ok(rd) = std::fs::read_dir(dir.join("snd/pcm")) else { continue };
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().to_lowercase().replace(' ', "_");
            if fname.trim_end_matches(".wav").replace("__", "_") == stem.trim_end_matches(".wav") {
                let theirs = decode_wav(&std::fs::read(e.path()).unwrap()).unwrap();
                assert_eq!((ours.channels, ours.rate), (theirs.channels, theirs.rate), "{name}");
                assert_eq!(ours.samples.len(), theirs.samples.len(), "{name} length");
                assert_eq!(ours.samples, theirs.samples, "{name} samples");
                cmp += 1;
            }
        }
    }
    eprintln!("{cmp} waves identical to the ffmpeg decode");
    assert!(cmp > 50);
}
