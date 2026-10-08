//! Bin stream framing (port of `tools/extract_bin.py` + the framing loop of `tools/lzo1x.c`).
//!
//! A stream is a list of blocks `{u32 ulen, u32 clen, data[clen]}`; `clen < ulen` means LZO1X,
//! otherwise the data is stored raw. The PC files put 4 extra zero bytes after every block; the
//! reader tolerates them exactly like the C tool (a zero `ulen` followed by a plausible block header
//! is skipped, anything else terminates the stream).

use crate::error::{rd_u32, Error, Result};
use crate::lzo;

pub fn unstream(b: &[u8]) -> Result<Vec<u8>> {
    let n = b.len();
    let mut p = 0usize;
    let mut out = Vec::new();
    let mut nb = 0usize;
    while p + 8 <= n {
        let u = rd_u32(b, p)? as usize;
        let cl = rd_u32(b, p + 4)? as usize;
        p += 8;
        if u == 0 {
            if p + 4 <= n {
                let u2 = rd_u32(b, p - 4)? as usize;
                let c2 = rd_u32(b, p)? as usize;
                if u2 != 0 && u2 <= (1 << 20) && c2 != 0 && c2 <= u2 + 64 {
                    p -= 4;
                    continue;
                }
            }
            break;
        }
        if cl < u {
            let src = b.get(p..p + cl).ok_or_else(|| Error(format!("stream: block {nb} truncated")))?;
            let blk = lzo::decompress(src, u).map_err(|e| Error(format!("stream: block {nb}: {e}")))?;
            if blk.len() != u {
                return Err(Error(format!("stream: block {nb}: size {} != {u}", blk.len())));
            }
            out.extend_from_slice(&blk);
        } else {
            let src = b.get(p..p + u).ok_or_else(|| Error(format!("stream: raw block {nb} truncated")))?;
            out.extend_from_slice(src);
        }
        p += cl;
        nb += 1;
    }
    Ok(out)
}

/// Decompressed bank/stream = `{u32 size, data}` chunks (tex_decode.chunks). Returns (offset, bytes).
pub fn chunks(d: &[u8]) -> Vec<(usize, &[u8])> {
    let mut p = 0;
    let mut cs = Vec::new();
    while p + 4 <= d.len() {
        let sz = u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]) as usize;
        let end = (p + 4 + sz).min(d.len());
        cs.push((p, &d[p + 4..end]));
        p += 4 + sz;
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lzo::gen;

    /// Frame blocks as the game files do (zero u32 after every block).
    pub fn frame(blocks: &[(Vec<u8>, Option<Vec<u8>>)]) -> Vec<u8> {
        let mut f = Vec::new();
        for (raw, comp) in blocks {
            f.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            match comp {
                Some(c) => {
                    f.extend_from_slice(&(c.len() as u32).to_le_bytes());
                    f.extend_from_slice(c);
                }
                None => {
                    f.extend_from_slice(&(raw.len() as u32).to_le_bytes());
                    f.extend_from_slice(raw);
                }
            }
            f.extend_from_slice(&[0, 0, 0, 0]);
        }
        f.extend_from_slice(&[0u8; 8]);
        f
    }

    #[test]
    fn mixed_blocks() {
        let mut blocks = Vec::new();
        let mut want = Vec::new();
        for seed in 1..12u64 {
            if seed % 3 == 0 {
                let raw: Vec<u8> = (0..300).map(|i| (i * seed as usize) as u8).collect();
                want.extend_from_slice(&raw);
                blocks.push((raw, None));
            } else {
                let (c, e) = gen::stream(seed, 40);
                assert!(c.len() < e.len());
                want.extend_from_slice(&e);
                blocks.push((e, Some(c)));
            }
        }
        let f = frame(&blocks);
        assert_eq!(unstream(&f).unwrap(), want);
        // a stream that just ends (no terminator) and trailing junk are tolerated
        let mut g = f.clone();
        g.truncate(g.len() - 8);
        assert_eq!(unstream(&g).unwrap(), want);
        g.extend_from_slice(&[0xde, 0xad]);
        assert_eq!(unstream(&g).unwrap(), want);
    }

    #[test]
    fn corrupt_block_errors() {
        let (c, e) = gen::stream(5, 40);
        let mut f = frame(&[(e, Some(c))]);
        f[12] ^= 0xff;
        // either an error or a different length; never a panic
        let _ = unstream(&f);
        let mut t = frame(&[(vec![1; 100], None)]);
        t.truncate(50);
        assert!(unstream(&t).is_err());
    }

    #[test]
    fn chunk_walk() {
        let mut d = Vec::new();
        for n in [3u32, 0, 5] {
            d.extend_from_slice(&n.to_le_bytes());
            d.extend(std::iter::repeat(7u8).take(n as usize));
        }
        let cs = chunks(&d);
        assert_eq!(cs.iter().map(|c| c.1.len()).collect::<Vec<_>>(), vec![3, 0, 5]);
    }
}
