//! FX sprites / Rex maps vs the PNGs produced by the Python tools. Skipped without KK_TEST_DATA.
mod common;
use common::*;
use kk_extract::build::{self, BuildOpts, ClipDb};
use kk_extract::texture::{decode_png, Fmt, Image};
use std::path::PathBuf;

fn load(dir: &PathBuf, name: &str) -> Option<(usize, usize, Vec<u8>)> {
    let b = std::fs::read(dir.join("ref_assets").join(name)).ok()?;
    let (w, h, ct, buf) = decode_png(&b).unwrap();
    // normalise to RGBA
    let rgba = match ct {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        c => panic!("unexpected png type {c:?} for {name}"),
    };
    Some((w, h, rgba))
}
fn as_image(w: usize, h: usize, rgba: Vec<u8>) -> Image {
    Image { w, h, rgba, fmt: Fmt::Dxn, mips: 1, key: 0 }
}
fn rgb_to_rgba(w: &[u8]) -> Vec<u8> {
    w.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect()
}

#[test]
fn rex_maps_match_python() {
    let dir = skip_without_data!();
    if load(&dir, "idx189.png").is_none() {
        return eprintln!("no rex source textures");
    }
    for (src, refn, kind) in [("idx189.png", "rex_nrm_body.png", 0), ("idx192.png", "rex_nrm_head.png", 0), ("idx190.png", "rex_mr_body.png", 1), ("idx191.png", "rex_mr_head.png", 2)] {
        let (w, h, px) = load(&dir, src).unwrap();
        let (_, _, want) = load(&dir, refn).unwrap();
        let img = as_image(w, h, px);
        let got = match kind {
            0 => rgb_to_rgba(&kk_extract::images::rex_normal(&img)),
            1 => kk_extract::images::rex_rough_body(&img),
            _ => kk_extract::images::rex_rough_head(&img),
        };
        let bad = got.iter().zip(&want).filter(|(a, b)| a != b).count();
        eprintln!("{refn}: {bad} differing bytes of {}", want.len());
        assert_eq!(bad, 0, "{refn}");
    }
}

#[test]
fn palette_sprites_match_python() {
    let dir = skip_without_data!();
    if !dir.join("ff801b17.dec").is_file() {
        return eprintln!("no ff801b17 bank");
    }
    let recipe = build::manifest().unwrap().into_iter().find(|(n, _)| n == "fx_palette_sprites").unwrap().1;
    let mut src = DirSource::new(&dir);
    let b = build::build_asset(&mut src, "fx_palette_sprites", &recipe, &ClipDb::builtin(), &BuildOpts::default(), &mut |_| {}).unwrap();
    assert_eq!(b.files.len(), 8);
    for (name, bytes) in &b.files {
        let (w, h, ct, got) = decode_png(bytes).unwrap();
        assert!(matches!(ct, png::ColorType::Rgba));
        let (rw, rh, want) = load(&dir, name).unwrap_or_else(|| panic!("no reference {name}"));
        assert_eq!((w, h), (rw, rh), "{name}");
        assert!(got == want, "{name}: pixels differ");
    }
}
