//! LZO1X decompressor (port of `tools/lzo1x.c`, itself written from the public format description).
//! Bounds-checked: malformed input returns an error, never panics or reads out of range.

use crate::error::{Error, Result};

#[derive(Clone, Copy)]
enum St {
    Top,
    FirstLit,
    Match,
    CopyMatch,
    Done,
    Next,
}

/// Decompress one LZO1X block. `max_out` bounds the output (the block's known uncompressed length).
pub fn decompress(src: &[u8], max_out: usize) -> Result<Vec<u8>> {
    let eof = || Error("lzo: input truncated".into());
    let rd = |ip: usize| -> Result<usize> { src.get(ip).map(|&b| b as usize).ok_or_else(eof) };
    let mut out: Vec<u8> = Vec::with_capacity(max_out);
    let mut ip = 0usize;
    let mut t: usize;
    let mut m: usize = 0;

    macro_rules! lit {
        ($n:expr) => {{
            let n = $n;
            if out.len() + n > max_out {
                return Err(Error("lzo: output overrun".into()));
            }
            let s = src.get(ip..ip + n).ok_or_else(eof)?;
            out.extend_from_slice(s);
            ip += n;
        }};
    }
    macro_rules! copy_from {
        ($n:expr) => {{
            let n = $n;
            if out.len() + n > max_out {
                return Err(Error("lzo: output overrun".into()));
            }
            for _ in 0..n {
                let b = out[m];
                out.push(b);
                m += 1;
            }
        }};
    }
    macro_rules! back {
        ($dist:expr) => {{
            let dist: usize = $dist;
            if dist > out.len() || dist == 0 {
                return Err(Error("lzo: lookbehind overrun".into()));
            }
            out.len() - dist
        }};
    }

    let mut st;
    if rd(0)? > 17 {
        t = rd(0)? - 17;
        ip = 1;
        if t < 4 {
            st = St::Next;
        } else {
            lit!(t);
            st = St::FirstLit;
        }
    } else {
        t = 0;
        st = St::Top;
    }
    loop {
        match st {
            St::Top => {
                t = rd(ip)?;
                ip += 1;
                if t >= 16 {
                    st = St::Match;
                    continue;
                }
                if t == 0 {
                    while rd(ip)? == 0 {
                        t += 255;
                        ip += 1;
                    }
                    t += 15 + rd(ip)?;
                    ip += 1;
                }
                lit!(t + 3);
                st = St::FirstLit;
            }
            St::FirstLit => {
                t = rd(ip)?;
                ip += 1;
                if t >= 16 {
                    st = St::Match;
                    continue;
                }
                let b = rd(ip)?;
                ip += 1;
                m = back!(1 + 0x800 + (t >> 2) + (b << 2));
                copy_from!(3);
                st = St::Done;
            }
            St::Match => {
                if t >= 64 {
                    let b = rd(ip)?;
                    ip += 1;
                    m = back!(1 + ((t >> 2) & 7) + (b << 3));
                    t = (t >> 5) - 1;
                    st = St::CopyMatch;
                } else if t >= 32 {
                    t &= 31;
                    if t == 0 {
                        while rd(ip)? == 0 {
                            t += 255;
                            ip += 1;
                        }
                        t += 31 + rd(ip)?;
                        ip += 1;
                    }
                    let v = rd(ip)? | (rd(ip + 1)? << 8);
                    ip += 2;
                    m = back!(1 + (v >> 2));
                    st = St::CopyMatch;
                } else if t >= 16 {
                    let hi = (t & 8) << 11;
                    t &= 7;
                    if t == 0 {
                        while rd(ip)? == 0 {
                            t += 255;
                            ip += 1;
                        }
                        t += 7 + rd(ip)?;
                        ip += 1;
                    }
                    let v = rd(ip)? | (rd(ip + 1)? << 8);
                    ip += 2;
                    let d = hi + (v >> 2);
                    if d == 0 {
                        return Ok(out); // end-of-stream marker (17,0,0)
                    }
                    m = back!(d + 0x4000);
                    st = St::CopyMatch;
                } else {
                    let b = rd(ip)?;
                    ip += 1;
                    m = back!(1 + (t >> 2) + (b << 2));
                    copy_from!(2);
                    st = St::Done;
                }
            }
            St::CopyMatch => {
                copy_from!(t + 2);
                st = St::Done;
            }
            St::Done => {
                t = rd(ip - 2)? & 3;
                st = if t == 0 { St::Top } else { St::Next };
            }
            St::Next => {
                lit!(t);
                t = rd(ip)?;
                ip += 1;
                st = St::Match;
            }
        }
    }
}

#[doc(hidden)]
pub mod gen {
    //! Random *valid* LZO1X stream generator (test only). It emits every token form the decoder
    //! handles (literal runs incl. the >17 start byte and extended lengths, M1/M2/M3/M4 matches,
    //! extended match lengths, trailing literals, the end marker) and tracks the expected output.
    pub struct Rng(pub u64);
    impl Rng {
        pub fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) as u32
        }
        pub fn below(&mut self, n: u32) -> u32 {
            self.next() % n.max(1)
        }
    }

    fn lits(rng: &mut Rng, n: usize, comp: &mut Vec<u8>, exp: &mut Vec<u8>, compressible: bool) {
        for _ in 0..n {
            let b = if compressible { b"abcab"[rng.below(5) as usize] } else { rng.next() as u8 };
            comp.push(b);
            exp.push(b);
        }
    }
    fn copy(exp: &mut Vec<u8>, dist: usize, len: usize) {
        for _ in 0..len {
            let b = exp[exp.len() - dist];
            exp.push(b);
        }
    }
    fn ext(comp: &mut Vec<u8>, mut v: usize) {
        // encodes v (>=0) as zero bytes of 255 plus final byte
        while v > 255 {
            comp.push(0);
            v -= 255;
        }
        comp.push(v as u8);
    }

    /// Returns (compressed, expected).
    pub fn stream(seed: u64, tokens: usize) -> (Vec<u8>, Vec<u8>) {
        let mut rng = Rng(seed);
        let (mut c, mut e) = (Vec::new(), Vec::new());
        // `state`: 0 = loop top, 1 = first_literal_run (after literal run), 2 = expect match code,
        // trailing literal count handled inline.
        let mut first = true;
        let mut state = 0;
        let mut n_tok = 0;
        // optional initial long literal run (>17 start byte)
        if rng.below(2) == 0 {
            let n = 1 + rng.below(200) as usize; // 17+n must fit in a byte
            let n = n.min(238);
            c.push((17 + n) as u8);
            lits(&mut rng, n, &mut c, &mut e, true);
            state = if n < 4 { 2 } else { 1 };
            first = false;
        }
        while n_tok < tokens {
            n_tok += 1;
            match state {
                0 => {
                    // literal run token (t<16), or directly a match when e is non-empty
                    if !e.is_empty() && !first && rng.below(3) == 0 {
                        state = 2;
                        continue;
                    }
                    let big = rng.below(4) == 0;
                    let n = 3 + rng.below(if big { 700 } else { 16 }) as usize; // 3+t, t>=1 => n>=4
                    let n = n.max(4);
                    let t = n - 3;
                    if t <= 15 {
                        c.push(t as u8);
                    } else {
                        c.push(0);
                        ext(&mut c, t - 15);
                    }
                    let comp = rng.below(2) == 0;
                    lits(&mut rng, n, &mut c, &mut e, comp);
                    first = false;
                    state = 1;
                }
                1 => {
                    // after literal run: either 3-byte M1 with dist > 2048, or a normal match code
                    if e.len() > 0x801 + 1023 && rng.below(3) == 0 {
                        let off = rng.below(1024) as usize; // (t>>2) + (b<<2)
                        let dist = 1 + 0x800 + off;
                        let tr = rng.below(4) as usize;
                        c.push((((off & 3) << 2) | tr) as u8);
                        c.push((off >> 2) as u8);
                        copy(&mut e, dist, 3);
                        state = 0;
                        if tr > 0 {
                            lits(&mut rng, tr, &mut c, &mut e, true);
                            state = 2;
                        }
                        continue;
                    }
                    state = 2;
                }
                _ => {
                    // match code
                    let maxd = e.len();
                    let kind = rng.below(4);
                    let tr = rng.below(4) as usize;
                    match kind {
                        0 => {
                            // M2: dist 1..=2048 (1+((t>>2)&7)+(b<<3)), len 3..=8
                            let len = 3 + rng.below(6) as usize;
                            let dist = 1 + rng.below(2048.min(maxd as u32)) as usize;
                            let off = dist - 1;
                            let t = ((len - 1) << 5) | ((off & 7) << 2) | tr;
                            c.push(t as u8);
                            c.push((off >> 3) as u8);
                            copy(&mut e, dist, len);
                        }
                        1 => {
                            // M3: dist 1..=16384, len>=3 (t&31 = len-2, ext if 0)
                            let len = 3 + if rng.below(3) == 0 { rng.below(600) as usize } else { rng.below(30) as usize };
                            let dist = 1 + rng.below(16384.min(maxd as u32)) as usize;
                            if len - 2 <= 31 {
                                c.push((32 | (len - 2)) as u8);
                            } else {
                                c.push(32);
                                ext(&mut c, len - 2 - 31);
                            }
                            let v = ((dist - 1) << 2) | tr;
                            c.push((v & 255) as u8);
                            c.push((v >> 8) as u8);
                            copy(&mut e, dist, len);
                        }
                        2 => {
                            // M4: dist 0x4000+ ... up to 0x4000+0x7fff; need maxd large enough
                            if maxd < 0x4001 {
                                // fall back to M2
                                let dist = 1 + rng.below(2048.min(maxd as u32)) as usize;
                                let off = dist - 1;
                                let t = ((3 - 1) << 5) | ((off & 7) << 2) | tr; // len 3
                                c.push(t as u8);
                                c.push((off >> 3) as u8);
                                copy(&mut e, dist, 3);
                            } else {
                                let len = 3 + if rng.below(3) == 0 { rng.below(400) as usize } else { rng.below(6) as usize };
                                let maxoff = (maxd - 0x4000).min(0x7fff);
                                let d = 1 + rng.below(maxoff as u32) as usize; // d>0 so not the end marker
                                let dist = 0x4000 + d;
                                let (h, low) = (d >> 14, d & 0x3fff);
                                let l = len - 2;
                                let base = 16 | (h << 3);
                                if l <= 7 {
                                    c.push((base | l) as u8);
                                } else {
                                    c.push(base as u8);
                                    ext(&mut c, l - 7);
                                }
                                let v = (low << 2) | tr;
                                c.push((v & 255) as u8);
                                c.push((v >> 8) as u8);
                                copy(&mut e, dist, len);
                            }
                        }
                        _ => {
                            // M1 2-byte (dist 1..=1024, len 2) only legal in "match_next" state (after 1-3 literals);
                            // here use M2 with len 3 instead unless the previous token left trailing literals
                            let dist = 1 + rng.below(1024.min(maxd as u32)) as usize;
                            let off = dist - 1;
                            let t = ((3 - 1) << 5) | ((off & 7) << 2) | tr;
                            c.push(t as u8);
                            c.push((off >> 3) as u8);
                            copy(&mut e, dist, 3);
                        }
                    }
                    state = 0;
                    if tr > 0 {
                        lits(&mut rng, tr, &mut c, &mut e, true);
                        // after trailing literals the next byte is a match code; t<16 is the M1 form
                        if rng.below(3) == 0 && e.len() > 1100 {
                            let dist = 1 + rng.below(1024) as usize;
                            let off = dist - 1;
                            let t2 = (off & 3) << 2 | (rng.below(4) as usize);
                            c.push(t2 as u8);
                            c.push((off >> 2) as u8);
                            copy(&mut e, dist, 2);
                            let tr2 = t2 & 3;
                            if tr2 > 0 {
                                lits(&mut rng, tr2, &mut c, &mut e, true);
                                state = 2;
                            } else {
                                state = 0;
                            }
                        } else {
                            state = 2;
                        }
                    }
                }
            }
        }
        // terminate: pending states need a valid prefix before the end marker (17,0,0), which is an
        // M4-class code and so valid wherever a match code is. At loop top / first-literal states it
        // is also read as a match code (t>=16).
        c.extend_from_slice(&[17, 0, 0]);
        (c, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_only_and_marker() {
        // 17+5 literals then end marker
        let mut s = vec![17 + 5];
        s.extend_from_slice(b"hello");
        s.extend_from_slice(&[17, 0, 0]);
        assert_eq!(decompress(&s, 5).unwrap(), b"hello");
    }

    #[test]
    fn truncated_and_overrun_are_errors() {
        let mut s = vec![17 + 5];
        s.extend_from_slice(b"hello");
        assert!(decompress(&s, 5).is_err()); // no end marker
        s.extend_from_slice(&[17, 0, 0]);
        assert!(decompress(&s, 4).is_err()); // output cap
        assert!(decompress(&[], 4).is_err());
        assert!(decompress(&[0x40, 0xff, 0xff], 100).is_err()); // lookbehind before start
    }

    #[test]
    fn random_streams_roundtrip() {
        for seed in 1..400u64 {
            let (c, e) = gen::stream(seed, 1 + (seed as usize % 60) * 3);
            let got = decompress(&c, e.len()).unwrap_or_else(|er| panic!("seed {seed}: {er}"));
            assert_eq!(got, e, "seed {seed}");
        }
    }

    #[test]
    fn random_streams_large() {
        for seed in 1000..1010u64 {
            let (c, e) = gen::stream(seed, 4000);
            assert_eq!(decompress(&c, e.len()).unwrap(), e, "seed {seed}");
        }
    }

    #[test]
    fn garbage_never_panics() {
        let mut r = gen::Rng(7);
        for _ in 0..2000 {
            let n = r.below(200) as usize;
            let v: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
            let _ = decompress(&v, 4096);
        }
    }
}
