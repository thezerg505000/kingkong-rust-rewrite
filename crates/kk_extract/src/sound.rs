//! Sounds from `Sound_Common.bf`: sound definitions (`.smd`) -> `sound_defs.json`, and the MS-ADPCM waves they use -> PCM `.wav`.
//!
//! The Python pipeline that produced the current assets was an ad-hoc script chain (bf.py extract, ffmpeg to PCM, ffmpeg to Ogg);
//! this module reproduces its outputs from the format as recovered from the `.smd` files (see `smd_layout` below):
//!
//! * `.smd` = 256+ byte record, fixed offsets: `0x20` own key, `0x30 f32 volume`, `0x34 f32 f1`, `0x40 u32` fade-in key, `0x48 u32` fade-out
//!   key (`0xffffffff` / `0xcafedeca` = none), `0xe8 u32 nwav`, `0xec u32 nins`, `0xf0` `nwav * {u32 key, u32 0}`, then `nins * u32 key`,
//!   then the terminator `{0xffffffff, 0}`. Keys are BF file keys; they are resolved through the archive's key table.
//! * waves are MS-ADPCM (format 2, 4 bit) `.wav`, `.waa` (ambience, stereo) or `.wac`; they are decoded to 16 bit PCM `.wav`
//!   (the game's Ogg files were made from the same PCM). Decoding follows ffmpeg's `adpcm_ms` (truncating `/256`).
//! * output names: the BF path lower-cased with `/` and spaces replaced by `_` (`Object/Door/Breakable door hit.wav` ->
//!   `sounds/object_door_breakable_door_hit.wav`).

use crate::build::{BfEntry, Built, Source};
use crate::err;
use crate::error::{rd_u16, rd_u32, Result};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};

/// Parsed `.smd` record.
#[derive(Debug, Clone, PartialEq)]
pub struct Smd {
    pub own_key: u32,
    pub volume: f32,
    pub f1: f32,
    pub fade_in: u32,
    pub fade_out: u32,
    pub wav_keys: Vec<u32>,
    pub insert_keys: Vec<u32>,
}

pub const NO_KEY_A: u32 = 0xffff_ffff;
pub const NO_KEY_B: u32 = 0xcafe_deca;

/// Parse a `.smd` record (layout in the module docs). Strict about the trailer so a wrong file type is rejected.
pub fn parse_smd(b: &[u8]) -> Result<Smd> {
    if b.len() < 0xf0 + 8 {
        return Err(err!("smd: too short ({} bytes)", b.len()));
    }
    if rd_u32(b, 0)? != 2 {
        return Err(err!("smd: unknown version {}", rd_u32(b, 0)?));
    }
    let f = |o: usize| rd_u32(b, o).map(f32::from_bits);
    let nw = rd_u32(b, 0xe8)? as usize;
    let ni = rd_u32(b, 0xec)? as usize;
    if nw > 64 || ni > 64 || 0xf0 + nw * 8 + ni * 4 + 8 != b.len() {
        return Err(err!("smd: {} bytes do not fit {nw} waves + {ni} inserts", b.len()));
    }
    let mut p = 0xf0;
    let mut wav_keys = Vec::new();
    for _ in 0..nw {
        wav_keys.push(rd_u32(b, p)?);
        p += 8;
    }
    let mut insert_keys = Vec::new();
    for _ in 0..ni {
        insert_keys.push(rd_u32(b, p)?);
        p += 4;
    }
    if rd_u32(b, p)? != 0xffff_ffff || rd_u32(b, p + 4)? != 0 {
        return Err(err!("smd: bad terminator"));
    }
    Ok(Smd { own_key: rd_u32(b, 0x20)?, volume: f(0x30)?, f1: f(0x34)?, fade_in: rd_u32(b, 0x40)?, fade_out: rd_u32(b, 0x48)?, wav_keys, insert_keys })
}

/// Output file name for a BF path: `Object/Door/Breakable door hit.wav` -> `object_door_breakable_door_hit`.
pub fn out_stem(path: &str) -> String {
    let p = path.trim_start_matches('/');
    let stem = match p.rfind('.') {
        Some(i) if p[i..].len() <= 5 => &p[..i],
        _ => p,
    };
    stem.to_lowercase().replace(['/', ' ', '\\'], "_")
}

// ------------------------------------------------------------------ wave decoding

#[derive(Debug, Clone)]
pub struct Pcm {
    pub channels: u16,
    pub rate: u32,
    /// interleaved 16 bit samples
    pub samples: Vec<i16>,
}

const ADAPT: [i32; 16] = [230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230];
const COEF_STD: [(i32, i32); 7] = [(256, 0), (512, -256), (0, 0), (192, 64), (240, 0), (460, -208), (392, -232)];

struct Fmt {
    tag: u16,
    channels: u16,
    rate: u32,
    block_align: usize,
    bits: u16,
    coefs: Vec<(i32, i32)>,
}

fn parse_riff(b: &[u8]) -> Result<(Fmt, &[u8])> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(err!("not a RIFF/WAVE file"));
    }
    let mut p = 12;
    let mut fmt: Option<Fmt> = None;
    while p + 8 <= b.len() {
        let id = &b[p..p + 4];
        let sz = rd_u32(b, p + 4)? as usize;
        let body = &b[(p + 8).min(b.len())..(p + 8 + sz).min(b.len())];
        if id == b"fmt " {
            if body.len() < 16 {
                return Err(err!("wav: short fmt chunk"));
            }
            let mut coefs = Vec::new();
            if body.len() >= 22 {
                // MS-ADPCM extension: cbSize u16 (+16, 0 in these files whatever the chunk size), wSamplesPerBlock u16 (+18),
                // wNumCoef u16 (+20), coefficient pairs (+22).
                let nc = rd_u16(body, 20)? as usize;
                for i in 0..nc.min(32) {
                    if 22 + 4 * i + 4 <= body.len() {
                        coefs.push((rd_u16(body, 22 + 4 * i)? as i16 as i32, rd_u16(body, 24 + 4 * i)? as i16 as i32));
                    }
                }
            }
            fmt = Some(Fmt { tag: rd_u16(body, 0)?, channels: rd_u16(body, 2)?, rate: rd_u32(body, 4)?, block_align: rd_u16(body, 12)? as usize, bits: rd_u16(body, 14)?, coefs });
        } else if id == b"data" {
            let f = fmt.ok_or_else(|| err!("wav: data before fmt"))?;
            return Ok((f, body));
        }
        p += 8 + sz + (sz & 1);
    }
    Err(err!("wav: no data chunk"))
}

fn adpcm_nibble(nib: i32, c: (i32, i32), s1: &mut i32, s2: &mut i32, delta: &mut i32) -> i16 {
    let mut pred = (*s1 * c.0 + *s2 * c.1) / 256;
    pred += (if nib & 8 != 0 { nib - 16 } else { nib }) * *delta;
    *s2 = *s1;
    *s1 = pred.clamp(-32768, 32767);
    *delta = ((ADAPT[nib as usize] * *delta) >> 8).max(16);
    *s1 as i16
}

fn decode_adpcm_block(blk: &[u8], ch: usize, coefs: &[(i32, i32)], out: &mut Vec<i16>) -> Result<()> {
    if blk.len() < 7 * ch {
        return Ok(()); // runt tail: nothing decodable (ffmpeg drops it as well)
    }
    let mut pred = [0usize; 2];
    let (mut delta, mut s1, mut s2) = ([0i32; 2], [0i32; 2], [0i32; 2]);
    for c in 0..ch {
        pred[c] = blk[c] as usize;
        if pred[c] >= coefs.len() {
            return Err(err!("adpcm: predictor index {} out of range", pred[c]));
        }
    }
    let mut p = ch;
    for c in 0..ch {
        delta[c] = rd_u16(blk, p + 2 * c)? as i16 as i32;
    }
    p += 2 * ch;
    for c in 0..ch {
        s1[c] = rd_u16(blk, p + 2 * c)? as i16 as i32;
    }
    p += 2 * ch;
    for c in 0..ch {
        s2[c] = rd_u16(blk, p + 2 * c)? as i16 as i32;
    }
    p += 2 * ch;
    for c in 0..ch {
        out.push(s2[c] as i16);
    }
    for c in 0..ch {
        out.push(s1[c] as i16);
    }
    let mut c = 0usize;
    for &byte in &blk[p..] {
        for nib in [(byte >> 4) as i32, (byte & 15) as i32] {
            out.push(adpcm_nibble(nib, coefs[pred[c]], &mut s1[c], &mut s2[c], &mut delta[c]));
            c = (c + 1) % ch;
        }
    }
    Ok(())
}

/// Decode a RIFF/WAVE file (MS-ADPCM or 16 bit PCM) to PCM.
pub fn decode_wav(b: &[u8]) -> Result<Pcm> {
    let (f, data) = parse_riff(b)?;
    let ch = f.channels as usize;
    if ch == 0 || ch > 2 {
        return Err(err!("wav: {ch} channels unsupported"));
    }
    match f.tag {
        1 if f.bits == 16 => Ok(Pcm { channels: f.channels, rate: f.rate, samples: data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect() }),
        2 if f.bits == 4 => {
            let coefs = if f.coefs.is_empty() { COEF_STD.to_vec() } else { f.coefs.clone() };
            if f.block_align < 7 * ch + 1 {
                return Err(err!("adpcm: block align {} too small", f.block_align));
            }
            let mut samples = Vec::with_capacity(data.len() * 2);
            for blk in data.chunks(f.block_align) {
                decode_adpcm_block(blk, ch, &coefs, &mut samples)?;
            }
            Ok(Pcm { channels: f.channels, rate: f.rate, samples })
        }
        t => Err(err!("wav: unsupported format tag {t} ({} bits)", f.bits)),
    }
}

/// 44 byte header PCM16 `.wav`.
pub fn write_wav(p: &Pcm) -> Vec<u8> {
    let data_len = p.samples.len() * 2;
    let mut o = Vec::with_capacity(44 + data_len);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    o.extend_from_slice(b"WAVEfmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&p.channels.to_le_bytes());
    o.extend_from_slice(&p.rate.to_le_bytes());
    o.extend_from_slice(&(p.rate * p.channels as u32 * 2).to_le_bytes());
    o.extend_from_slice(&(p.channels * 2).to_le_bytes());
    o.extend_from_slice(&16u16.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&(data_len as u32).to_le_bytes());
    for s in &p.samples {
        o.extend_from_slice(&s.to_le_bytes());
    }
    o
}

// ------------------------------------------------------------------ the asset

/// Ambience / one-shot wave files that no `.smd` references (their definitions are fixed in the recipe).
#[derive(Debug, Clone)]
struct Extra {
    /// file name in the archive (any directory, case-insensitive)
    bf_name: String,
    out: String,
}

fn round3(x: f32) -> f64 {
    ((x as f64) * 1000.0).round() / 1000.0
}

pub fn build_sounds(src: &mut dyn Source, recipe: &Value, log: &mut dyn FnMut(&str)) -> Result<Built> {
    log("reading the Sound_Common.bf index");
    let index: Vec<BfEntry> = src.sound_index()?;
    let mut by_key: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, e) in index.iter().enumerate() {
        by_key.entry(e.key).or_default().push(i);
    }
    let ext = |p: &str| p.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    // key -> entry index, preferring the entry with the wanted extension
    let resolve = |k: u32, want: &str| -> Option<usize> {
        let v = by_key.get(&k)?;
        v.iter().copied().find(|&i| ext(&index[i].path) == want)
    };
    let mut smds: Vec<usize> = index.iter().enumerate().filter(|(_, e)| ext(&e.path) == "smd").map(|(i, _)| i).collect();
    smds.sort_by(|&a, &b| index[a].path.cmp(&index[b].path));
    let mut defs = Map::new();
    let mut waves: BTreeMap<String, usize> = BTreeMap::new(); // out stem -> entry
    let mut notes = Vec::new();
    for &si in &smds {
        let e = &index[si];
        let name = e.path.rsplit('/').next().unwrap_or(&e.path);
        let name = &name[..name.len() - 4];
        if defs.contains_key(name) {
            continue;
        }
        let smd = parse_smd(&src.sound_read(si)?).map_err(|x| err!("{}: {x}", e.path))?;
        let mut files = Vec::new();
        for &k in &smd.wav_keys {
            if let Some(wi) = resolve(k, "wav") {
                let stem = out_stem(&index[wi].path);
                waves.insert(stem.clone(), wi);
                files.push(format!("sounds/{stem}.wav"));
            } else {
                notes.push(format!("{name}: wave key {k:08x} not found"));
            }
        }
        let fades: Vec<String> = [smd.fade_in, smd.fade_out].iter().filter_map(|&k| resolve(k, "fad")).map(|i| index[i].path.clone()).collect();
        let inserts: Vec<String> = smd.insert_keys.iter().filter_map(|&k| resolve(k, "ins")).map(|i| index[i].path.clone()).collect();
        defs.insert(name.to_string(), json!({"files": files, "volume": round3(smd.volume), "f1": round3(smd.f1), "fades": fades, "inserts": inserts}));
    }
    if smds.is_empty() {
        return Err(err!("Sound_Common.bf holds no .smd sound definitions"));
    }
    // ambience / thunder: fixed definitions over waves found by file name
    let mut extras: Vec<Extra> = Vec::new();
    for x in recipe["extra_waves"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        extras.push(Extra { bf_name: x["bf_name"].as_str().unwrap_or("").to_string(), out: x["out"].as_str().unwrap_or("").to_string() });
    }
    let mut extra_found: HashMap<String, String> = HashMap::new();
    for x in &extras {
        let hit = index.iter().position(|e| e.path.rsplit('/').next().map(|n| n.eq_ignore_ascii_case(&x.bf_name)).unwrap_or(false));
        match hit {
            Some(i) => {
                waves.insert(x.out.clone(), i);
                extra_found.insert(x.out.clone(), x.bf_name.clone());
            }
            None => notes.push(format!("{}: not found in Sound_Common.bf", x.bf_name)),
        }
    }
    for d in recipe["extra_defs"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let name = d["name"].as_str().unwrap_or("");
        let files: Vec<String> = d["files"].as_array().map(|a| a.iter().filter_map(|f| f.as_str().map(String::from)).collect()).unwrap_or_default();
        // only when every wave of the definition was found
        if files.iter().all(|f| waves.contains_key(f.trim_start_matches("sounds/").trim_end_matches(".wav"))) && !name.is_empty() {
            defs.insert(name.to_string(), json!({"files": files, "volume": d["volume"], "f1": d["f1"], "fades": d["fades"], "inserts": d["inserts"]}));
        }
    }
    let total = waves.len();
    let mut files = Vec::new();
    for (n, (stem, wi)) in waves.iter().enumerate() {
        log(&format!("[{}/{}] {}", n + 1, total, index[*wi].path));
        let raw = src.sound_read(*wi)?;
        let pcm = decode_wav(&raw).map_err(|e| err!("{}: {e}", index[*wi].path))?;
        files.push((format!("sounds/{stem}.wav"), write_wav(&pcm)));
    }
    files.push(("sound_defs.json".to_string(), serde_json::to_vec_pretty(&Value::Object(defs))?));
    Ok(Built { files, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems() {
        assert_eq!(out_stem("Object/Door/Breakable door hit.wav"), "object_door_breakable_door_hit");
        assert_eq!(out_stem("Animal/Kong/Kong_mvt_ann.wav"), "animal_kong_kong_mvt_ann");
    }

    fn smd_bytes(nw: usize, ni: usize) -> Vec<u8> {
        let mut b = vec![0u8; 0xf0];
        b[0] = 2;
        b[0x20..0x24].copy_from_slice(&0x8700_1111u32.to_le_bytes());
        b[0x30..0x34].copy_from_slice(&0.55f32.to_le_bytes());
        b[0x34..0x38].copy_from_slice(&0.5f32.to_le_bytes());
        b[0x40..0x44].copy_from_slice(&0x8700_0001u32.to_le_bytes());
        b[0x48..0x4c].copy_from_slice(&NO_KEY_B.to_le_bytes());
        b[0xe8..0xec].copy_from_slice(&(nw as u32).to_le_bytes());
        b[0xec..0xf0].copy_from_slice(&(ni as u32).to_le_bytes());
        for i in 0..nw {
            b.extend_from_slice(&(0x8700_2000 + i as u32).to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
        }
        for i in 0..ni {
            b.extend_from_slice(&(0x8700_3000 + i as u32).to_le_bytes());
        }
        b.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b
    }

    #[test]
    fn smd_roundtrip() {
        let s = parse_smd(&smd_bytes(3, 2)).unwrap();
        assert_eq!(s.wav_keys, vec![0x8700_2000, 0x8700_2001, 0x8700_2002]);
        assert_eq!(s.insert_keys, vec![0x8700_3000, 0x8700_3001]);
        assert_eq!((s.volume, s.f1, s.fade_in, s.fade_out), (0.55, 0.5, 0x8700_0001, NO_KEY_B));
        assert_eq!(round3(s.volume), 0.55);
        let mut bad = smd_bytes(1, 0);
        bad.push(0);
        assert!(parse_smd(&bad).is_err());
        let mut bad = smd_bytes(1, 0);
        bad[0] = 3;
        assert!(parse_smd(&bad).is_err());
        assert!(parse_smd(&[0u8; 10]).is_err());
    }

    /// One mono block: header (pred 0, delta 16, s1 100, s2 50) + 2 bytes of nibbles.
    fn adpcm_wav(block: &[u8]) -> Vec<u8> {
        let mut fmt = Vec::new();
        for v in [2u16, 1] {
            fmt.extend_from_slice(&v.to_le_bytes());
        }
        fmt.extend_from_slice(&8000u32.to_le_bytes());
        fmt.extend_from_slice(&4000u32.to_le_bytes());
        for v in [block.len() as u16, 4, 0, 4, 7] {
            fmt.extend_from_slice(&v.to_le_bytes());
        }
        for (a, b) in COEF_STD {
            fmt.extend_from_slice(&(a as i16).to_le_bytes());
            fmt.extend_from_slice(&(b as i16).to_le_bytes());
        }
        let mut o = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        o.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        o.extend_from_slice(&fmt);
        o.extend_from_slice(b"data");
        o.extend_from_slice(&(block.len() as u32).to_le_bytes());
        o.extend_from_slice(block);
        o
    }

    #[test]
    fn adpcm_block_by_hand() {
        let mut blk = vec![0u8]; // predictor 0: (256, 0)
        blk.extend_from_slice(&16i16.to_le_bytes());
        blk.extend_from_slice(&100i16.to_le_bytes());
        blk.extend_from_slice(&50i16.to_le_bytes());
        blk.push(0x2f); // nibbles 2, -1
        let p = decode_wav(&adpcm_wav(&blk)).unwrap();
        // header samples: s2 = 50, s1 = 100; then pred = 100 + 2*16 = 132, delta = max(16, 230*16>>8 = 14) = 16;
        // next: pred = 132 + (-1)*16 = 116 (adapt[15] = 230)
        assert_eq!(p.samples, vec![50, 100, 132, 116]);
        assert_eq!((p.channels, p.rate), (1, 8000));
        let w = write_wav(&p);
        assert_eq!(decode_wav(&w).unwrap().samples, p.samples);
        assert_eq!(w.len(), 44 + 8);
    }

    #[test]
    fn adpcm_clamps_and_rejects_bad_predictor() {
        let mut blk = vec![0u8];
        blk.extend_from_slice(&30000i16.to_le_bytes());
        blk.extend_from_slice(&32000i16.to_le_bytes());
        blk.extend_from_slice(&32000i16.to_le_bytes());
        blk.push(0x77); // +7 * 30000 -> clamps
        let p = decode_wav(&adpcm_wav(&blk)).unwrap();
        assert_eq!(p.samples[2], 32767);
        let mut blk2 = blk.clone();
        blk2[0] = 9;
        assert!(decode_wav(&adpcm_wav(&blk2)).is_err());
        assert!(decode_wav(b"RIFFxxxx").is_err());
    }
}
