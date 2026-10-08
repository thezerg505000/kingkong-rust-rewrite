//! Track-list (TRL) parser: port of `tools/anim_parse.py`, `find_trls.py` and `clipfeat.py`
//! (layout in anim_findings.md section 2).
//!
//! TRL = `u32 size | u16 numTracks | u16 listFlags | tracks`;
//! TRACK = `u16 flags | u16 gizmo(0xffff = root) | u32 dataLen | u16 numEvents | u16 trackType | events`;
//! EVENT = `numFrames (u8 if flags&0x200 else u16) | [u16 evflags (first event, or every event unless
//! flags&0x800)] | payload`; payload (if evflags&0x80) = `[u16 size][u16 type]` (first event; later ones
//! omit size if flags&0x2000 and type if flags&0x1000) + `12*(type&3)` translation bytes + 6 (compressed
//! quaternion, type&0x80) / 16 (f32 quaternion, type&0x10) / 36 (3x3 matrix, type&4).

use crate::error::{rd_f32, rd_i16, rd_u16, rd_u32};

#[derive(Debug, Clone, PartialEq)]
pub struct Bad(pub String);
type R<T> = std::result::Result<T, Bad>;

fn bad<T>(s: &str) -> R<T> {
    Err(Bad(s.to_string()))
}
fn ck<T>(r: crate::error::Result<T>) -> R<T> {
    r.map_err(|e| Bad(e.0))
}

#[derive(Debug, Clone)]
pub enum Rot {
    /// x, y, z, w
    Q([f64; 4]),
    Mat([f64; 9]),
}

#[derive(Debug, Clone)]
pub struct Key {
    pub typ: u16,
    /// translation(s): 1 for types with &3 == 1, more for blended types
    pub t: Option<Vec<[f32; 3]>>,
    pub q: Option<Rot>,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub nf: u32,
    pub evfl: u16,
    pub key: Option<Key>,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub flags: u16,
    /// -1 = root
    pub gizmo: i32,
    pub dlen: u32,
    pub ttype: u16,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone)]
pub struct Trl {
    pub off: usize,
    pub size: u32,
    pub num_tracks: u16,
    pub n_anim: usize,
    pub list_flags: u16,
    pub tracks: Vec<Track>,
    pub consumed: usize,
    pub tail: i64,
}

fn payload_len(typ: u16) -> R<usize> {
    let mut n = (typ & 3) as usize * 12;
    if typ & 0x80 != 0 {
        n += 6;
    } else if typ & 0x10 != 0 {
        n += 16;
    } else if typ & 0x04 != 0 {
        n += 36;
    }
    if typ & 0x08 != 0 {
        return bad("timeblock");
    }
    Ok(n)
}

fn decode_key(typ: u16, body: &[u8]) -> R<Key> {
    let nt = (typ & 3) as usize;
    let mut p = 0;
    let mut t = None;
    if nt > 0 {
        let mut v = Vec::new();
        for i in 0..nt {
            v.push([ck(rd_f32(body, 12 * i))?, ck(rd_f32(body, 12 * i + 4))?, ck(rd_f32(body, 12 * i + 8))?]);
        }
        t = Some(v);
        p = 12 * nt;
    }
    let mut q = None;
    if typ & 0x80 != 0 {
        let x = ck(rd_i16(body, p))? as f64 / 32767.0;
        let y = ck(rd_i16(body, p + 2))? as f64 / 32767.0;
        let z = ck(rd_i16(body, p + 4))? as f64 / 32767.0;
        let w2 = 1.0 - x * x - y * y - z * z;
        q = Some(Rot::Q([x, y, z, w2.max(0.0).sqrt()]));
    } else if typ & 0x10 != 0 {
        q = Some(Rot::Q([ck(rd_f32(body, p))? as f64, ck(rd_f32(body, p + 4))? as f64, ck(rd_f32(body, p + 8))? as f64, ck(rd_f32(body, p + 12))? as f64]));
    } else if typ & 0x04 != 0 {
        let mut m = [0.0; 9];
        for (i, v) in m.iter_mut().enumerate() {
            *v = ck(rd_f32(body, p + 4 * i))? as f64;
        }
        q = Some(Rot::Mat(m));
    }
    Ok(Key { typ, t, q })
}

fn parse_track(d: &[u8], mut p: usize, end: usize, evmask: u16) -> R<(Track, usize)> {
    let flags = ck(rd_u16(d, p))?;
    let gz = ck(rd_u16(d, p + 2))?;
    let dl = ck(rd_u32(d, p + 4))?;
    p += 8;
    let ne = ck(rd_u16(d, p))?;
    let tt = ck(rd_u16(d, p + 2))?;
    p += 4;
    let mut events = Vec::with_capacity(ne as usize);
    let mut size: Option<u16> = None;
    let mut typ: Option<u16> = None;
    let mut evfl = 0u16;
    for i in 0..ne {
        if p + 3 > end {
            return bad("eof");
        }
        let nf;
        if flags & 0x200 != 0 {
            nf = *d.get(p).ok_or(Bad("eof".into()))? as u32;
            p += 1;
        } else {
            nf = ck(rd_u16(d, p))? as u32;
            p += 2;
        }
        if i == 0 || flags & 0x800 == 0 {
            evfl = ck(rd_u16(d, p))?;
            p += 2;
        }
        if evfl & !evmask != 0 {
            return bad("evflags");
        }
        let mut key = None;
        if evfl & 0x80 != 0 {
            if i == 0 {
                let s = ck(rd_u16(d, p))?;
                let t = ck(rd_u16(d, p + 2))?;
                p += 4;
                if payload_len(t)? + 4 != s as usize {
                    return bad("size/type mismatch");
                }
                size = Some(s);
                typ = Some(t);
            } else {
                if flags & 0x2000 == 0 {
                    size = Some(ck(rd_u16(d, p))?);
                    p += 2;
                }
                if flags & 0x1000 == 0 {
                    typ = Some(ck(rd_u16(d, p))?);
                    p += 2;
                }
            }
            let t = typ.ok_or(Bad("no type".into()))?;
            let _ = size;
            let pl = payload_len(t)?;
            if p + pl > end {
                return bad("overrun");
            }
            let body = d.get(p..p + pl).ok_or(Bad("overrun".into()))?;
            p += pl;
            key = Some(decode_key(t, body)?);
        }
        events.push(Event { nf, evfl, key });
    }
    Ok((Track { flags, gizmo: if gz == 0xffff { -1 } else { gz as i32 }, dlen: dl, ttype: tt, events }, p))
}

/// `o` = offset of the u32 size. Parses the leading animation tracks (flags & 0x8000); trailing
/// event/AI tracks are not decoded.
pub fn parse_trl(d: &[u8], o: usize) -> R<Trl> {
    parse_trl_with(d, o, false)
}

/// `lenient` = kk_trl.py / kkc_anim.py behaviour: event flag bit 0x02 is accepted (it carries no extra bytes;
/// it occurs in 5 of Kong's 254 track lists). The strict mode is the one used for the Jack arms / T-Rex clips.
pub fn parse_trl_with(d: &[u8], o: usize, lenient: bool) -> R<Trl> {
    let evmask: u16 = if lenient { 0x86 } else { 0x84 };
    let size = ck(rd_u32(d, o))?;
    if size < 12 || size > 2_000_000 || o + 4 + size as usize > d.len() {
        return bad("size");
    }
    let nt = ck(rd_u16(d, o + 4))?;
    let lf = ck(rd_u16(d, o + 6))?;
    let end = o + 4 + size as usize;
    if nt == 0 || nt > 200 {
        return bad("nt");
    }
    let mut p = o + 8;
    let mut tracks = Vec::new();
    for _ in 0..nt {
        if p + 12 > end {
            return bad("eof");
        }
        if ck(rd_u16(d, p))? & 0x8000 == 0 {
            break;
        }
        let (tr, np) = parse_track(d, p, end, evmask)?;
        p = np;
        tracks.push(tr);
        if p > end {
            return bad("overrun");
        }
    }
    if tracks.is_empty() {
        return bad("no anim tracks");
    }
    Ok(Trl { off: o, size, num_tracks: nt, n_anim: tracks.len(), list_flags: lf, tracks, consumed: p - o - 4, tail: end as i64 - p as i64 })
}

/// Event start times (frames) and total length of a track.
pub fn track_times(tr: &Track) -> (Vec<u32>, u32) {
    let mut t = 0;
    let mut out = Vec::new();
    for e in &tr.events {
        out.push(t);
        t += e.nf;
    }
    (out, t)
}

#[derive(Debug, Clone)]
pub struct TrlSummary {
    pub off: usize,
    pub size: u32,
    pub nt: u16,
    pub nanim: usize,
    pub lf: u16,
    pub tail: i64,
    pub gizmos: Vec<i32>,
    pub frames: u32,
}

/// find_trls.py: scan a decoded stream for valid track lists (prefilter + exact-size parse).
pub fn find_trls(d: &[u8]) -> Vec<TrlSummary> {
    let n = d.len();
    let mut out = Vec::new();
    if n < 32 {
        return out;
    }
    for o in 0..n - 16 {
        let size = u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
        if !(20..2_000_000).contains(&size) {
            continue;
        }
        let nt = u16::from_le_bytes([d[o + 4], d[o + 5]]);
        if !(1..=200).contains(&nt) {
            continue;
        }
        let fl = u16::from_le_bytes([d[o + 8], d[o + 9]]);
        let gz = u16::from_le_bytes([d[o + 10], d[o + 11]]);
        let dl = u32::from_le_bytes([d[o + 12], d[o + 13], d[o + 14], d[o + 15]]);
        if fl & 0x8000 == 0 || !(gz == 0xffff || gz < 256) || !(8..200_000).contains(&dl) {
            continue;
        }
        if let Ok(r) = parse_trl(d, o) {
            if r.tail < 0 {
                continue;
            }
            let frames = r.tracks.iter().map(|t| track_times(t).1).max().unwrap_or(0);
            out.push(TrlSummary {
                off: o,
                size: r.size,
                nt: r.num_tracks,
                nanim: r.n_anim,
                lf: r.list_flags,
                tail: r.tail,
                gizmos: r.tracks.iter().map(|t| t.gizmo).collect(),
                frames,
            });
        }
    }
    out
}

// ---------------------------------------------------------------- clip features (clipfeat.py)

#[derive(Debug, Clone, Default)]
pub struct Features {
    pub frames: u32,
    pub root_travel: [f64; 3],
    pub root_dist: f64,
    pub jaw_range_deg: Option<f64>,
    pub pelvis_z: Option<[f64; 2]>,
    pub loop_err_deg: f64,
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round_ties_even() / 100.0
}

fn qangle(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs();
    2.0 * d.min(1.0).acos().to_degrees()
}

fn first_t(k: &Key) -> Option<[f32; 3]> {
    k.t.as_ref().and_then(|v| v.first().copied())
}
fn quat(k: &Key) -> Option<[f64; 4]> {
    match &k.q {
        Some(Rot::Q(q)) => Some(*q),
        _ => None,
    }
}

pub fn features(r: &Trl) -> Features {
    // dict keyed by (gizmo, kind), insertion ordered, later tracks overwrite
    let mut tr: Vec<((i32, char), &Track)> = Vec::new();
    for t in &r.tracks {
        let kind = if t.events.first().and_then(|e| e.key.as_ref()).map(|k| k.t.is_some()).unwrap_or(false) { 't' } else { 'r' };
        if let Some(slot) = tr.iter_mut().find(|(k, _)| *k == (t.gizmo, kind)) {
            slot.1 = t;
        } else {
            tr.push(((t.gizmo, kind), t));
        }
    }
    let get = |g: i32, k: char| tr.iter().find(|(kk, _)| *kk == (g, k)).map(|(_, t)| *t);
    let mut f = Features { frames: r.tracks.iter().map(|t| track_times(t).1).max().unwrap_or(0), ..Default::default() };
    if let Some(root) = get(-1, 't') {
        let a = root.events.first().and_then(|e| e.key.as_ref()).and_then(first_t);
        let b = root.events.last().and_then(|e| e.key.as_ref()).and_then(first_t);
        if let (Some(a), Some(b)) = (a, b) {
            let dv = [b[0] as f64 - a[0] as f64, b[1] as f64 - a[1] as f64, b[2] as f64 - a[2] as f64];
            f.root_travel = [round2(dv[0]), round2(dv[1]), round2(dv[2])];
            f.root_dist = (dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]).sqrt();
        }
    }
    if let Some(jaw) = get(21, 'r') {
        let qs: Vec<[f64; 4]> = jaw.events.iter().filter_map(|e| e.key.as_ref().and_then(quat)).collect();
        if !qs.is_empty() {
            f.jaw_range_deg = Some(qs.iter().map(|q| qangle(&qs[0], q)).fold(0.0, f64::max));
        }
    }
    if let Some(ps) = get(0, 't') {
        let z: Vec<f64> = ps.events.iter().filter_map(|e| e.key.as_ref().and_then(first_t)).map(|t| t[2] as f64).collect();
        if !z.is_empty() {
            let mn = z.iter().cloned().fold(f64::INFINITY, f64::min);
            let mx = z.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            f.pelvis_z = Some([round2(mn), round2(mx)]);
        }
    }
    let mut cl = Vec::new();
    for ((g, kind), t) in &tr {
        if *kind == 'r' && *g >= 0 && t.events.len() > 1 {
            let a = t.events.first().and_then(|e| e.key.as_ref()).and_then(quat);
            let b = t.events.last().and_then(|e| e.key.as_ref()).and_then(quat);
            if let (Some(a), Some(b)) = (a, b) {
                cl.push(qangle(&a, &b));
            }
        }
    }
    f.loop_err_deg = if cl.is_empty() { 0.0 } else { cl.iter().sum::<f64>() / cl.len() as f64 };
    f
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build a TRL: one translation track (type 1) and one compressed-quaternion track (type 0x80).
    pub fn sample() -> Vec<u8> {
        let mut tr = Vec::new();
        // track A: gizmo -1, flags 0x8000|0x200 (u8 frames), 3 events, translation f32x3
        let mut a = Vec::new();
        let evs = [(0u8, [0f32, 0., 0.]), (10, [0., -2., 0.]), (0, [0., -2., 0.])];
        for (i, (nf, t)) in evs.iter().enumerate() {
            a.push(*nf);
            a.extend_from_slice(&0x80u16.to_le_bytes());
            if i == 0 {
                a.extend_from_slice(&16u16.to_le_bytes());
                a.extend_from_slice(&1u16.to_le_bytes());
            } else {
                a.extend_from_slice(&16u16.to_le_bytes());
                a.extend_from_slice(&1u16.to_le_bytes());
            }
            for v in t {
                a.extend_from_slice(&v.to_le_bytes());
            }
        }
        tr.extend_from_slice(&0x8200u16.to_le_bytes());
        tr.extend_from_slice(&0xffffu16.to_le_bytes());
        tr.extend_from_slice(&(a.len() as u32).to_le_bytes());
        tr.extend_from_slice(&3u16.to_le_bytes());
        tr.extend_from_slice(&1u16.to_le_bytes());
        tr.extend_from_slice(&a);
        // track B: gizmo 3, flags 0x8000|0x200|0x800|0x2000|0x1000 (shared evflags/size/type)
        let mut b = Vec::new();
        let qs = [[0i16, 0, 0], [16384, 0, 0], [0, 0, 0]];
        for (i, q) in qs.iter().enumerate() {
            b.push(if i == 1 { 10u8 } else { 0 });
            if i == 0 {
                b.extend_from_slice(&0x80u16.to_le_bytes());
                b.extend_from_slice(&10u16.to_le_bytes());
                b.extend_from_slice(&0x80u16.to_le_bytes());
            }
            for v in q {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        tr.extend_from_slice(&0xBA00u16.to_le_bytes());
        tr.extend_from_slice(&3u16.to_le_bytes());
        tr.extend_from_slice(&(b.len() as u32).to_le_bytes());
        tr.extend_from_slice(&3u16.to_le_bytes());
        tr.extend_from_slice(&0x80u16.to_le_bytes());
        tr.extend_from_slice(&b);
        // trailing non-anim track header (flags without 0x8000) + junk
        tr.extend_from_slice(&[0u8; 12]);
        let mut d = Vec::new();
        d.extend_from_slice(&((4 + tr.len()) as u32).to_le_bytes());
        d.extend_from_slice(&3u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&tr);
        d
    }

    #[test]
    fn parses_synthetic_list() {
        let d = sample();
        let r = parse_trl(&d, 0).unwrap();
        assert_eq!(r.n_anim, 2);
        assert_eq!(r.tracks[0].gizmo, -1);
        assert_eq!(r.tracks[1].gizmo, 3);
        let (ts, tot) = track_times(&r.tracks[0]);
        assert_eq!(ts, vec![0, 0, 10]);
        assert_eq!(tot, 10);
        let k = r.tracks[1].events[1].key.as_ref().unwrap();
        match k.q.as_ref().unwrap() {
            Rot::Q(q) => {
                assert!((q[0] - 16384.0 / 32767.0).abs() < 1e-12);
                assert!((q[3] - (1.0 - q[0] * q[0]).sqrt()).abs() < 1e-12);
            }
            _ => panic!(),
        }
        assert!(r.tail >= 0);
        let f = features(&r);
        assert_eq!(f.frames, 10);
        assert!((f.root_dist - 2.0).abs() < 1e-9);
        assert_eq!(f.root_travel, [0.0, -2.0, 0.0]);
        // found by scan
        let mut buf = vec![0u8; 64];
        buf.extend_from_slice(&d);
        buf.extend_from_slice(&[0u8; 64]);
        let found = find_trls(&buf);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].off, 64);
        assert_eq!(found[0].frames, 10);
    }

    #[test]
    fn rejects_garbage() {
        let mut d = sample();
        d[8] = 0; // first track flags lose 0x8000
        d[9] = 0;
        assert!(parse_trl(&d, 0).is_err());
        let mut d = sample();
        d[20] = 0xff; // corrupt event flags
        let _ = parse_trl(&d, 0);
        assert!(parse_trl(&d[..10], 0).is_err());
        let z = vec![0u8; 100];
        assert!(parse_trl(&z, 0).is_err());
    }
}
