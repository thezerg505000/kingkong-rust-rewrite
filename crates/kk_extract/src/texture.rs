//! Texture banks (`KKTextures.bf` ff80xxxx) -> RGBA / PNG. Port of `tools/tex_decode.py`.
//!
//! The PC files carry Xbox 360 (Xenos) texture data: after stream framing a bank is a list of
//! `{u32 size, data}` chunks. A "marked" type-11 chunk has a 32 byte Jade header (`ff ff ff ff | u16
//! flags | u8 type(11) | u8 fmt | u16 w | u16 h | u32 color | u32 key | 12 byte magic`) followed by a
//! 32 byte big-endian `D2KK` sub-header (w, h, D3DFORMAT, mips) and the base mip, Xenos 2D-tiled in
//! 32x32 block pages. DXT/DXN blocks need a 16-bit byte swap. Only mip 0 is decoded.

use crate::err;
use crate::error::Result;
use crate::stream::chunks;

pub const MARK: [u8; 12] = [0x34, 0x12, 0xd0, 0xca, 0xff, 0x00, 0xff, 0x00, 0xde, 0xc0, 0xde, 0xc0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fmt {
    Dxt1,
    Dxt3,
    Dxt5,
    Dxn,
    A8R8G8B8,
    L8,
}

impl Fmt {
    fn from_d3d(f: u32) -> Option<Fmt> {
        Some(match f {
            0x1a200152 => Fmt::Dxt1,
            0x1a200153 => Fmt::Dxt3,
            0x1a200154 => Fmt::Dxt5,
            0x1a200171 => Fmt::Dxn,
            0x18280186 => Fmt::A8R8G8B8,
            0x04900102 => Fmt::L8,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
    pub fmt: Fmt,
    pub mips: u32,
    /// key from the Jade header (0 for most textures)
    pub key: u32,
}

/// Xenos 2D untiling. `wb`,`hb` in blocks (texels for uncompressed); returns hb*wb texels of `1<<log_bpp` bytes.
pub fn untile(data: &[u8], wb: usize, hb: usize, log_bpp: u32) -> Result<Vec<u8>> {
    let bpp = 1usize << log_bpp;
    let aw = (wb + 31) & !31;
    let ah = (hb + 31) & !31;
    let need = aw * ah * bpp;
    let mut padded;
    let src: &[u8] = if data.len() >= need {
        &data[..need]
    } else {
        padded = vec![0u8; need];
        padded[..data.len()].copy_from_slice(data);
        &padded
    };
    let lb = log_bpp as i64;
    let mut out = vec![0u8; wb * hb * bpp];
    for y in 0..hb as i64 {
        let macro_y = ((y >> 5) * (aw as i64 >> 5)) << (lb + 7);
        let micro_y = ((y & 6) << 2) << lb;
        let base = macro_y + ((micro_y & !0xF) << 1) + (micro_y & 0xF) + ((y & 8) << (3 + lb)) + ((y & 1) << 4);
        for x in 0..wb as i64 {
            let macro_x = (x >> 5) << (lb + 7);
            let micro_x = (x & 7) << lb;
            let mut off = base + macro_x + ((micro_x & !0xF) << 1) + (micro_x & 0xF);
            off = ((off & !0x1FF) << 3) + ((off & 0x1C0) << 2) + (off & 0x3F) + ((y & 16) << 7) + (((((y & 8) >> 2) + (x >> 3)) & 3) << 6);
            off >>= lb;
            let o = off as usize * bpp;
            if off < 0 || o + bpp > need {
                return Err(err!("untile: offset out of range"));
            }
            let d = (y as usize * wb + x as usize) * bpp;
            out[d..d + bpp].copy_from_slice(&src[o..o + bpp]);
        }
    }
    Ok(out)
}

fn rgb565(v: u16) -> [i32; 3] {
    [((v >> 11) & 31) as i32 * 255 / 31, ((v >> 5) & 63) as i32 * 255 / 63, (v & 31) as i32 * 255 / 31]
}

/// 16 RGBA texels (row-major 4x4) of an 8 byte colour block.
fn dxt_colors(blk: &[u8], dxt1: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([blk[0], blk[1]]);
    let c1 = u16::from_le_bytes([blk[2], blk[3]]);
    let a = rgb565(c0);
    let b = rgb565(c1);
    let four = c0 > c1 || !dxt1;
    let mut pal = [[0i32; 4]; 4];
    pal[0] = [a[0], a[1], a[2], 255];
    pal[1] = [b[0], b[1], b[2], 255];
    for k in 0..3 {
        pal[2][k] = if four { (2 * a[k] + b[k]) / 3 } else { (a[k] + b[k]) / 2 };
        pal[3][k] = if four { (a[k] + 2 * b[k]) / 3 } else { 0 };
    }
    pal[2][3] = 255;
    pal[3][3] = if dxt1 && !four { 0 } else { 255 };
    let idx = u32::from_le_bytes([blk[4], blk[5], blk[6], blk[7]]);
    let mut px = [[0u8; 4]; 16];
    for (i, p) in px.iter_mut().enumerate() {
        let c = pal[((idx >> (2 * i)) & 3) as usize];
        *p = [c[0] as u8, c[1] as u8, c[2] as u8, c[3] as u8];
    }
    px
}

/// 16 alpha values of an 8 byte BC3-style alpha block.
fn alpha_block(blk: &[u8]) -> [i32; 16] {
    let a0 = blk[0] as i32;
    let a1 = blk[1] as i32;
    let mut al = [0i32; 8];
    al[0] = a0;
    al[1] = a1;
    if a0 > a1 {
        for i in 1..7 {
            al[i + 1] = ((7 - i as i32) * a0 + i as i32 * a1) / 7;
        }
    } else {
        for i in 1..5 {
            al[i + 1] = ((5 - i as i32) * a0 + i as i32 * a1) / 5;
        }
        al[6] = 0;
        al[7] = 255;
    }
    let mut bits = 0u64;
    for i in 0..6 {
        bits |= (blk[2 + i] as u64) << (8 * i);
    }
    let mut out = [0i32; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = al[((bits >> (3 * i)) & 7) as usize];
    }
    out
}

fn dec_dxt(data: &[u8], w: usize, h: usize, fmt: Fmt) -> Vec<u8> {
    let bw = (w + 3) / 4;
    let bh = (h + 3) / 4;
    let bs = if fmt == Fmt::Dxt1 { 8 } else { 16 };
    let mut buf;
    let data: &[u8] = if data.len() >= bw * bh * bs {
        data
    } else {
        buf = vec![0u8; bw * bh * bs];
        buf[..data.len()].copy_from_slice(data);
        &buf
    };
    let mut out = vec![0u8; w * h * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let blk = &data[(by * bw + bx) * bs..(by * bw + bx + 1) * bs];
            let mut px = [[0u8; 4]; 16];
            match fmt {
                Fmt::Dxt1 => px = dxt_colors(blk, true),
                Fmt::Dxt3 => {
                    px = dxt_colors(&blk[8..], false);
                    for (i, p) in px.iter_mut().enumerate() {
                        p[3] = ((blk[i / 2] >> (4 * (i % 2))) & 15) * 17;
                    }
                }
                Fmt::Dxt5 => {
                    px = dxt_colors(&blk[8..], false);
                    let al = alpha_block(&blk[0..8]);
                    for (i, p) in px.iter_mut().enumerate() {
                        p[3] = al[i] as u8;
                    }
                }
                Fmt::Dxn => {
                    let xs = alpha_block(&blk[0..8]);
                    let ys = alpha_block(&blk[8..16]);
                    for i in 0..16 {
                        let x = xs[i] as f32 / 255.0 * 2.0 - 1.0;
                        let y = ys[i] as f32 / 255.0 * 2.0 - 1.0;
                        let z = (1.0f32 - x * x - y * y).clamp(0.0, 1.0).sqrt();
                        px[i] = [((x + 1.0) * 127.5) as i32 as u8, ((y + 1.0) * 127.5) as i32 as u8, ((z + 1.0) * 127.5) as i32 as u8, 255];
                    }
                }
                _ => unreachable!(),
            }
            for j in 0..4 {
                for i in 0..4 {
                    let (x, y) = (bx * 4 + i, by * 4 + j);
                    if x < w && y < h {
                        out[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&px[j * 4 + i]);
                    }
                }
            }
        }
    }
    out
}

/// Decode a type-11 data chunk (`c` = 32 B Jade header + 32 B sub-header + pixels).
pub fn decode_xe(c: &[u8]) -> Result<Image> {
    if c.len() < 64 {
        return Err(err!("texture chunk too short"));
    }
    let be = |p: usize| u32::from_be_bytes([c[p], c[p + 1], c[p + 2], c[p + 3]]);
    let (w, h, d3d, mips) = (be(36) as usize, be(40) as usize, be(44), be(52));
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(err!("implausible texture size {w}x{h}"));
    }
    let fmt = Fmt::from_d3d(d3d).ok_or_else(|| err!("unsupported texture format {d3d:#x}"))?;
    let data = &c[64..];
    let (bwid, bhei) = ((w + 3) / 4, (h + 3) / 4);
    let rgba = match fmt {
        Fmt::Dxt1 | Fmt::Dxt3 | Fmt::Dxt5 | Fmt::Dxn => {
            let bs_log = if fmt == Fmt::Dxt1 { 3 } else { 4 };
            let mut lin = untile(data, bwid, bhei, bs_log)?;
            for pair in lin.chunks_exact_mut(2) {
                pair.swap(0, 1);
            }
            dec_dxt(&lin, w, h, fmt)
        }
        Fmt::A8R8G8B8 => {
            let v = untile(data, w, h, 2)?;
            // big-endian ARGB, stored upside down -> RGBA, flipped vertically
            let mut o = vec![0u8; w * h * 4];
            for y in 0..h {
                let sy = h - 1 - y;
                for x in 0..w {
                    let s = (sy * w + x) * 4;
                    let d = (y * w + x) * 4;
                    o[d..d + 4].copy_from_slice(&[v[s + 1], v[s + 2], v[s + 3], v[s]]);
                }
            }
            o
        }
        Fmt::L8 => {
            let v = untile(data, w, h, 0)?;
            let mut o = vec![255u8; w * h * 4];
            for i in 0..w * h {
                o[i * 4] = v[i];
                o[i * 4 + 1] = v[i];
                o[i * 4 + 2] = v[i];
            }
            o
        }
    };
    Ok(Image { w, h, rgba, fmt, mips, key: u32::from_le_bytes([c[16], c[17], c[18], c[19]]) })
}

/// Marked type-11 data chunks of a decompressed bank: (ordinal, chunk). The ordinal (the `idxNNN` of
/// tex_decode.py output names) counts every such chunk, keyed UI atlases included.
pub fn marked_chunks(bank: &[u8]) -> Vec<(usize, &[u8])> {
    chunks(bank)
        .into_iter()
        .filter(|(_, c)| c.len() > 64 && c[20..32] == MARK && c[6] == 11)
        .enumerate()
        .map(|(n, (_, c))| (n, c))
        .collect()
}

/// Decode the texture with the given ordinal (`idx063` -> 63).
pub fn bank_texture(bank: &[u8], ordinal: usize) -> Result<Image> {
    let mut n = 0;
    for (_, c) in chunks(bank) {
        if c.len() > 64 && c[20..32] == MARK && c[6] == 11 {
            if n == ordinal {
                return decode_xe(c);
            }
            n += 1;
        }
    }
    Err(err!("texture ordinal {ordinal} not found ({n} textures in bank)"))
}

pub fn encode_png(w: usize, h: usize, color: png::ColorType, data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(color);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header()?;
        wr.write_image_data(data)?;
    }
    Ok(out)
}

impl Image {
    pub fn to_png_rgba(&self) -> Result<Vec<u8>> {
        encode_png(self.w, self.h, png::ColorType::Rgba, &self.rgba)
    }
    /// Diffuse binding: RGB only (DXT5 alpha is not coverage in these atlases) -- texture_bind.diffuse_png.
    pub fn to_png_rgb(&self) -> Result<Vec<u8>> {
        let mut rgb = Vec::with_capacity(self.w * self.h * 3);
        for p in self.rgba.chunks_exact(4) {
            rgb.extend_from_slice(&p[..3]);
        }
        encode_png(self.w, self.h, png::ColorType::Rgb, &rgb)
    }
    /// Normal-map binding (texture_bind.normal_png): keep X,Y of the decoded DXN, rebuild Z, no green flip.
    pub fn to_png_normal(&self) -> Result<Vec<u8>> {
        let mut rgb = Vec::with_capacity(self.w * self.h * 3);
        for p in self.rgba.chunks_exact(4) {
            let x = p[0] as f32 / 255.0 * 2.0 - 1.0;
            let y = p[1] as f32 / 255.0 * 2.0 - 1.0;
            let z = (1.0f32 - x * x - y * y).clamp(0.0, 1.0).sqrt();
            for v in [x, y, z] {
                rgb.push((((v * 0.5 + 0.5) * 255.0 + 0.5) as i32).clamp(0, 255) as u8);
            }
        }
        encode_png(self.w, self.h, png::ColorType::Rgb, &rgb)
    }
}

pub fn decode_png(bytes: &[u8]) -> Result<(usize, usize, png::ColorType, Vec<u8>)> {
    let dec = png::Decoder::new(bytes);
    let mut r = dec.read_info().map_err(|e| err!("png: {e}"))?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|e| err!("png: {e}"))?;
    buf.truncate(info.buffer_size());
    Ok((info.width as usize, info.height as usize, info.color_type, buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxt1_blocks() {
        // c0 = pure red (565 0xf800), c1 = 0, all indices 0 -> red
        let px = dxt_colors(&[0x00, 0xf8, 0x00, 0x00, 0, 0, 0, 0], true);
        assert_eq!(px[0], [255, 0, 0, 255]);
        // c0 < c1: 3 colour mode, index 3 is transparent black
        let px = dxt_colors(&[0x00, 0x00, 0x00, 0xf8, 0xff, 0xff, 0xff, 0xff], true);
        assert_eq!(px[0], [0, 0, 0, 0]);
        // index 2 in 4 colour mode = (2*a+b)/3
        let px = dxt_colors(&[0x00, 0xf8, 0x00, 0x00, 0b10, 0, 0, 0], true);
        assert_eq!(px[0], [170, 0, 0, 255]);
    }

    #[test]
    fn alpha_block_modes() {
        // a0 > a1: 8 interpolated levels; pixel 0 -> idx5
        let al = alpha_block(&[200, 10, 0b101, 0, 0, 0, 0, 0]);
        assert_eq!(al[0], (3 * 200 + 4 * 10) / 7);
        assert_eq!(al[1], 200);
        // a0 <= a1: 6 levels + 0 and 255; pixel 0 -> idx6 (0), pixel 1 -> idx7 (255)
        let al = alpha_block(&[10, 200, 0b111_110, 0, 0, 0, 0, 0]);
        assert_eq!((al[0], al[1]), (0, 255));
    }

    #[test]
    fn untile_is_a_bijection() {
        for (w, h, lb) in [(128usize, 128usize, 0u32), (64, 64, 2), (32, 32, 3), (64, 32, 4)] {
            let bpp = 1usize << lb;
            let n = w * h * bpp;
            let data: Vec<u8> = (0..n).map(|i| (i / bpp % 251) as u8 ^ (i % bpp) as u8).collect();
            let out = untile(&data, w, h, lb).unwrap();
            assert_eq!(out.len(), n);
            let mut a = out.clone();
            a.sort();
            let mut b = data.clone();
            b.sort();
            assert_eq!(a, b, "{w}x{h} log_bpp {lb}");
        }
    }

    #[test]
    fn png_helpers_roundtrip() {
        let img = Image { w: 2, h: 1, rgba: vec![1, 2, 3, 255, 4, 5, 6, 255], fmt: Fmt::L8, mips: 1, key: 0 };
        let (w, h, ct, data) = decode_png(&img.to_png_rgb().unwrap()).unwrap();
        assert_eq!((w, h, ct), (2, 1, png::ColorType::Rgb));
        assert_eq!(data, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn bad_inputs_error() {
        assert!(decode_xe(&[0; 10]).is_err());
        let mut c = vec![0u8; 80];
        c[36..40].copy_from_slice(&16u32.to_be_bytes());
        c[40..44].copy_from_slice(&16u32.to_be_bytes());
        c[44..48].copy_from_slice(&0x1234u32.to_be_bytes());
        assert!(decode_xe(&c).is_err());
        assert!(bank_texture(&[], 0).is_err());
    }
}
