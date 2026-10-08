//! Whole-pipeline test on a synthetic game directory: KKMaps.bf / KKTextures.bf are written with the
//! BIG v36 layout, the Bin stream holds a real decoded test stream (raw framed), the texture bank holds
//! random Xenos chunks. Checks discovery, BF reads, stream decoding, texture embedding, the stamp-file
//! idempotency, `--only`, failure isolation and the CLI. Needs KK_TEST_DATA for the stream.

use kk_extract::bf::testutil::{build as build_bf, F};
use kk_extract::glb::read_glb;
use std::path::{Path, PathBuf};

fn data_dir() -> Option<PathBuf> {
    std::env::var_os("KK_TEST_DATA").map(PathBuf::from).filter(|p| p.is_dir())
}

fn frame_raw(d: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    for blk in d.chunks(512_000) {
        f.extend_from_slice(&(blk.len() as u32).to_le_bytes());
        f.extend_from_slice(&(blk.len() as u32).to_le_bytes());
        f.extend_from_slice(blk);
        f.extend_from_slice(&[0, 0, 0, 0]);
    }
    f.extend_from_slice(&[0u8; 8]);
    f
}

fn xe_chunk(fmt: u32, w: usize, h: usize, seed: u64) -> Vec<u8> {
    let mut c = vec![0xffu8; 4];
    c.extend_from_slice(&0x4000u16.to_le_bytes());
    c.extend_from_slice(&[11, 0x10]);
    c.extend_from_slice(&(w as u16).to_le_bytes());
    c.extend_from_slice(&(h as u16).to_le_bytes());
    c.extend_from_slice(&0x20u32.to_le_bytes());
    c.extend_from_slice(&0u32.to_le_bytes());
    c.extend_from_slice(&kk_extract::texture::MARK);
    c.extend_from_slice(b"D2KK");
    for v in [w as u32, h as u32, fmt, 1, 0, 0, 0] {
        c.extend_from_slice(&v.to_be_bytes());
    }
    let bs = if fmt == 0x1a200152 { 8 } else { 16 };
    let n = 32 * 32 * bs;
    let mut s = seed;
    for _ in 0..n {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        c.push((s >> 40) as u8);
    }
    c
}

fn make_game(root: &Path, stream: &[u8]) {
    let dir = root.join("Game").join("Data");
    std::fs::create_dir_all(&dir).unwrap();
    let maps = build_bf(&[F { key: 0xff0003eb, name: "ff0003eb.bin", dir: 1, data: frame_raw(stream) }], &[("ROOT", -1), ("Bin", 0)], 4, 4);
    std::fs::write(dir.join("KKMaps.bf"), maps).unwrap();
    // bank with 80 type-11 chunks (+ a header-only record and an unmarked chunk in front)
    let mut bank = Vec::new();
    let mut push = |c: &[u8]| {
        bank.extend_from_slice(&(c.len() as u32).to_le_bytes());
        bank.extend_from_slice(c);
    };
    push(&[0xff; 32]);
    push(&[1u8; 48]);
    for i in 0..80u64 {
        let c = if i % 2 == 0 { xe_chunk(0x1a200152, 64, 64, i) } else { xe_chunk(0x1a200171, 64, 64, i) };
        push(&c);
    }
    let tex = build_bf(&[F { key: 0xff8003eb, name: "ff8003eb.bin", dir: 1, data: frame_raw(&bank) }], &[("ROOT", -1), ("Bin", 0)], 4, 4);
    std::fs::write(dir.join("kktextures.bf"), tex).unwrap(); // lower-case: lookup is case-insensitive
}

#[test]
fn synthetic_install_end_to_end() {
    let Some(dd) = data_dir() else {
        eprintln!("KK_TEST_DATA not set: skipping");
        return;
    };
    let stream = std::fs::read(dd.join("ff0003eb.dec")).unwrap();
    let tmp = std::env::temp_dir().join(format!("kk_extract_game_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    make_game(&tmp, &stream);
    let out = tmp.join("assets");
    let mut msgs = Vec::new();

    // build one asset
    let opts = kk_extract::Options { only: Some("jack_fps_luger.glb".into()), force: false };
    let rep = kk_extract::build_with(&tmp, &out, &opts, &mut |p| msgs.push(format!("{}/{} {} {}", p.step, p.total, p.asset, p.message))).unwrap();
    assert_eq!(rep.built, vec!["jack_fps_luger.glb"], "{:?}", rep);
    assert!(rep.failed.is_empty());
    assert!(msgs.iter().any(|m| m.contains("building")));
    let bytes = std::fs::read(out.join("jack_fps_luger.glb")).unwrap();
    let (js, _) = read_glb(&bytes).unwrap();
    assert_eq!(js["images"].as_array().unwrap().len(), 2, "diffuse + normal embedded");
    assert_eq!(js["meshes"][0]["primitives"].as_array().unwrap().len() > 0, true);
    assert_eq!(js["materials"][0]["extras"]["kk_diffuse_key"], "830029df");

    // idempotent: second run skips
    msgs.clear();
    let rep = kk_extract::build_with(&tmp, &out, &opts, &mut |p| msgs.push(p.message.to_string())).unwrap();
    assert_eq!(rep.skipped, vec!["jack_fps_luger.glb"]);
    assert!(rep.built.is_empty());
    assert_eq!(std::fs::read(out.join("jack_fps_luger.glb")).unwrap(), bytes);
    // deleted output is rebuilt, identical bytes (deterministic)
    std::fs::remove_file(out.join("jack_fps_luger.glb")).unwrap();
    let rep = kk_extract::build_with(&tmp, &out, &opts, &mut |_| {}).unwrap();
    assert_eq!(rep.built.len(), 1);
    assert_eq!(std::fs::read(out.join("jack_fps_luger.glb")).unwrap(), bytes, "deterministic output");
    // --force rebuilds
    let rep = kk_extract::build_with(&tmp, &out, &kk_extract::Options { force: true, ..opts.clone() }, &mut |_| {}).unwrap();
    assert_eq!(rep.built.len(), 1);

    // full manifest: weapons + arms succeed, the T-Rex fails (3 of its clip streams are not in the synthetic bf)
    // without taking the others down
    let rep = kk_extract::build_all(&tmp, &out, &mut |_| {}).unwrap();
    assert!(rep.built.contains(&"jack_fps_arms.glb".to_string()), "{:?}", rep);
    assert!(rep.skipped.contains(&"jack_fps_luger.glb".to_string()));
    // every asset whose streams / banks are not in the synthetic install fails on its own (12 of them), the rest is built
    let failed: Vec<&str> = rep.failed.iter().map(|f| f.0.as_str()).collect();
    for n in ["trex_inplace.glb", "kong/kong.glb", "creatures/raptor.glb", "sound_defs.json", "rex_maps", "fx_palette_sprites"] {
        assert!(failed.contains(&n), "{n} should fail: {:?}", rep.failed);
    }
    assert!(rep.built.contains(&"sky".to_string()), "{:?}", rep);
    assert!(!out.join("trex_inplace.glb").exists(), "no partial output on failure");
    assert!(!kk_extract::is_complete(&out), "T-Rex missing: not complete");
    assert!(!kk_extract::is_complete(&tmp.join("nonexistent")));

    // CLI
    let exe = env!("CARGO_BIN_EXE_kk-extract");
    let o = std::process::Command::new(exe).args(["--game", tmp.to_str().unwrap(), "--out", out.to_str().unwrap(), "--only", "jack_fps_arms.glb"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stderr).contains("up to date"));
    let o = std::process::Command::new(exe).args(["--game", tmp.to_str().unwrap(), "--out", out.to_str().unwrap(), "--only", "nope.glb"]).output().unwrap();
    assert!(!o.status.success());
    let o = std::process::Command::new(exe).args(["--game", "/definitely/not/here", "--out", out.to_str().unwrap()]).output().unwrap();
    assert!(!o.status.success());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_basics_without_game_data() {
    let exe = env!("CARGO_BIN_EXE_kk-extract");
    let o = std::process::Command::new(exe).arg("--list").output().unwrap();
    assert!(o.status.success());
    let s = String::from_utf8_lossy(&o.stdout);
    assert!(s.contains("jack_fps_arms.glb") && s.contains("trex_inplace.glb"));
    let o = std::process::Command::new(exe).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    let o = std::process::Command::new(exe).arg("--bogus").output().unwrap();
    assert_eq!(o.status.code(), Some(2));
}
