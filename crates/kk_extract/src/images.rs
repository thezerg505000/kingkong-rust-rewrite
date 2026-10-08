//! Standalone images rebuilt from the texture banks: FX sprites, the sky, and the three V-Rex material maps.
//!
//! Recipe (`kind: "images"`): `outputs` is a list of `{out, bank, op, ...}`:
//! * `rgba`        decode the Xenos texture selected by `ordinal` (the `idxNNN` of tex_decode.py) or by embedded `key` -> RGBA PNG
//! * `pal`         palette texture of the level bank (FX sprites): `data` chunk index, `pal` chunk index (raw bank chunks)
//! * `rex_normal`  V-Rex normal map: DXN texture, Y flipped, XY scaled by 1.569 (the material's normal scale [C]), re-normalised, RGB
//! * `rex_rough_body` / `rex_rough_head`  glTF metallic-roughness maps (G = roughness) from the body spec map (L8) / head diffuse.
//!   The two roughness curves are fitted to the maps the game was tuned with (`rex_mr_*.png`), not read from code [L].

use crate::build::{Built, Source};
use crate::err;
use crate::error::{rd_u16, Result};
use crate::stream::chunks;
use crate::texture::{self, encode_png, Image};
use serde_json::Value;

/// Palette texture (`texmap2.pal_img`): 4/8 bit indices + BGRA palette -> RGBA.
pub fn decode_pal(c: &[u8], p: &[u8]) -> Result<(usize, usize, Vec<u8>)> {
    if c.len() < 32 {
        return Err(err!("palette texture chunk too short"));
    }
    let (w, h) = (rd_u16(c, 8)? as usize, rd_u16(c, 10)? as usize);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Err(err!("implausible palette texture size {w}x{h}"));
    }
    let a = &c[32..];
    let mut idx: Vec<u8> = if c[7] == 0x50 { a.iter().flat_map(|&b| [b >> 4, b & 15]).collect() } else { a.to_vec() };
    idx.resize(w * h, 0);
    let n = if p.len() == 48 || p.len() == 64 { 16 } else { 256 };
    let step = p.len() / n;
    if step < 3 {
        return Err(err!("palette chunk too short"));
    }
    let mut pal = vec![[0u8; 4]; n];
    let mut amax = 0u32;
    for (i, e) in pal.iter_mut().enumerate() {
        let q = &p[i * step..];
        let al = if step >= 4 { q[3] } else { 255 };
        amax = amax.max(al as u32);
        *e = [q[2], q[1], q[0], al];
    }
    if step >= 4 && amax <= 128 {
        for e in pal.iter_mut() {
            e[3] = (e[3] as u32 * 2).min(255) as u8;
        }
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for &i in &idx[..w * h] {
        out.extend_from_slice(&pal[(i as usize).min(n - 1)]);
    }
    Ok((w, h, out))
}

fn half_up(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// V-Rex normal map from the decoded DXN texture (RGBA in, RGB out).
pub fn rex_normal(img: &Image) -> Vec<u8> {
    let mut o = Vec::with_capacity(img.w * img.h * 3);
    for p in img.rgba.chunks_exact(4) {
        let x = (p[0] as f64 / 255.0 * 2.0 - 1.0) * 1.569;
        let y = -(p[1] as f64 / 255.0 * 2.0 - 1.0) * 1.569;
        let l = (x * x + y * y).sqrt();
        let k = if l > 1.0 { 1.0 / l.max(1e-9) } else { 1.0 };
        let (x, y) = (x * k, y * k);
        let z = (1.0 - x * x - y * y).clamp(0.0, 1.0).sqrt();
        for v in [x, y, z] {
            o.push(half_up(v * 127.5 + 127.5).clamp(0.0, 255.0) as u8);
        }
    }
    o
}

/// Body: roughness from the L8 specular map.
pub fn rex_rough_body(img: &Image) -> Vec<u8> {
    let mut o = Vec::with_capacity(img.w * img.h * 4);
    for p in img.rgba.chunks_exact(4) {
        let g = half_up(255.0 - 0.8675 * (p[0] as f64 - 101.5).max(0.0)).clamp(0.0, 255.0) as u8;
        o.extend_from_slice(&[0, g, 0, 255]);
    }
    o
}

/// Head: roughness from the diffuse brightness (spec comes from the diffuse alpha, which is flat here).
pub fn rex_rough_head(img: &Image) -> Vec<u8> {
    let mut o = Vec::with_capacity(img.w * img.h * 4);
    for p in img.rgba.chunks_exact(4) {
        let mean = (p[0] as f32 + p[1] as f32 + p[2] as f32) / (3.0 * 255.0);
        let r = (1.0 - (5.0f32 / 6.0) * (mean - 0.03)).clamp(0.4, 0.9);
        o.extend_from_slice(&[0, (r * 255.0).floor() as u8, 0, 255]);
    }
    o
}

fn pick<'a>(bank: &'a [u8], spec: &Value) -> Result<Image> {
    if let Some(n) = spec["ordinal"].as_u64() {
        return texture::bank_texture(bank, n as usize);
    }
    if let Some(k) = spec["key"].as_str() {
        let want = u32::from_str_radix(k, 16).map_err(|_| err!("bad texture key '{k}'"))?;
        for (_, c) in texture::marked_chunks(bank) {
            if c.len() >= 20 && u32::from_le_bytes([c[16], c[17], c[18], c[19]]) == want {
                return texture::decode_xe(c);
            }
        }
        return Err(err!("texture key {k} not in the bank"));
    }
    Err(err!("image spec needs 'ordinal' or 'key'"))
}

pub fn build_images(src: &mut dyn Source, name: &str, recipe: &Value, log: &mut dyn FnMut(&str)) -> Result<Built> {
    let outs = recipe["outputs"].as_array().ok_or_else(|| err!("{name}: recipe has no outputs"))?;
    let mut files = Vec::new();
    for (n, o) in outs.iter().enumerate() {
        let out = o["out"].as_str().ok_or_else(|| err!("{name}: output {n} has no 'out'"))?;
        let bank_key = o["bank"].as_str().ok_or_else(|| err!("{out}: no bank"))?;
        log(&format!("[{}/{}] {out}", n + 1, outs.len()));
        let bank = src.bank(bank_key)?;
        let png = match o["op"].as_str().unwrap_or("rgba") {
            "rgba" => pick(&bank, o)?.to_png_rgba()?,
            "pal" => {
                let cs = chunks(&bank);
                let (d, p) = (o["data"].as_u64().unwrap_or(u64::MAX) as usize, o["pal"].as_u64().unwrap_or(u64::MAX) as usize);
                let (Some(dc), Some(pc)) = (cs.get(d), cs.get(p)) else { return Err(err!("{out}: chunk {d}/{p} not in bank {bank_key}")) };
                let (w, h, rgba) = decode_pal(dc.1, pc.1).map_err(|e| err!("{out}: {e}"))?;
                encode_png(w, h, png::ColorType::Rgba, &rgba)?
            }
            "rex_normal" => {
                let i = pick(&bank, o)?;
                encode_png(i.w, i.h, png::ColorType::Rgb, &rex_normal(&i))?
            }
            "rex_rough_body" => {
                let i = pick(&bank, o)?;
                encode_png(i.w, i.h, png::ColorType::Rgba, &rex_rough_body(&i))?
            }
            "rex_rough_head" => {
                let i = pick(&bank, o)?;
                encode_png(i.w, i.h, png::ColorType::Rgba, &rex_rough_head(&i))?
            }
            op => return Err(err!("{out}: unknown image op '{op}'")),
        };
        files.push((out.to_string(), png));
    }
    Ok(Built { files, notes: vec![] })
}
