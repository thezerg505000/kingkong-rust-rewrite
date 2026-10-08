//! Record scanning in decoded Bin streams (ports `tools/jadegao.py` and the GRO notes of
//! mesh_findings.md).
//!
//! * Named records: `u32 nameLen(incl NUL) | name | u32 payloadSize | payload`; GAO payloads start with `.gao`.
//!   payloadSize is not reliable for finding the next record, so records are found by scanning.
//! * GRO records: `u32 size | u32 type | body`, next record at `off + 4 + size`
//!   (type 1 = GEO, 4 = multi material, 5 = multi texture material).

use crate::error::rd_u32;

#[derive(Debug, Clone)]
pub struct Named {
    pub name: String,
    pub name_off: usize,
    pub payload: usize,
    pub size: u32,
    /// the three u32 before the name start minus 4: (-1, boneListIndex, nameLen)
    pub pre: [u32; 3],
}

fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b' ' | b'@')
}

/// All `<name>.gao\0` records in `d[lo..hi]` (hi = None: end of data), in stream order.
/// Equivalent to the regex `([A-Za-z0-9_\-\. @]{3,100}\.gao)\x00` of jadegao.iter_named followed by its
/// consistency checks (nameLen field before the name, `.gao` magic at the payload, offset >= 16).
pub fn iter_named(d: &[u8], lo: usize, hi: Option<usize>) -> Vec<Named> {
    let hi = hi.unwrap_or(d.len()).min(d.len());
    let mut out = Vec::new();
    if hi < lo + 5 {
        return out;
    }
    let mut e = lo;
    while e + 5 <= hi {
        // e = index of '.' in ".gao\0"
        if &d[e..e + 5] == b".gao\0" {
            // maximal run of name chars ending at e+4 (exclusive)
            let end = e + 4;
            let mut s = end;
            while s > lo && is_name_char(d[s - 1]) && end - (s - 1) <= 104 {
                s -= 1;
            }
            if end - s >= 7 && s >= 16 {
                let nl = end - s + 1;
                if let (Ok(l), Ok(sz)) = (rd_u32(d, s - 4), rd_u32(d, s + nl)) {
                    let p = s + nl + 4;
                    if l as usize == nl && d.get(p..p + 4) == Some(b".gao") {
                        let g = |q: usize| rd_u32(d, q).unwrap_or(0);
                        out.push(Named {
                            name: d[s..end].iter().map(|&c| c as char).collect(),
                            name_off: s,
                            payload: p,
                            size: sz,
                            pre: [g(s - 12), g(s - 8), g(s - 4)],
                        });
                        e = p; // matches are non-overlapping
                        continue;
                    }
                }
            }
        }
        e += 1;
    }
    out
}

/// One GRO record header.
#[derive(Debug, Clone, Copy)]
pub struct Gro {
    pub off: usize,
    pub size: u32,
    pub typ: u32,
}

/// Walk GRO records starting at `off` until the chain stops making sense.
pub fn iter_gro(d: &[u8], mut off: usize, max: usize) -> Vec<Gro> {
    let mut v = Vec::new();
    while v.len() < max {
        let (Ok(size), Ok(typ)) = (rd_u32(d, off), rd_u32(d, off + 4)) else { break };
        if size < 4 || off + 4 + size as usize > d.len() {
            break;
        }
        v.push(Gro { off, size, typ });
        off += 4 + size as usize;
    }
    v
}

/// Offsets of every `0xC0DE2002, 3` GEO header in `d` (scan_geos.py style candidate list).
pub fn find_geo_magics(d: &[u8]) -> Vec<usize> {
    let pat = [0x02u8, 0x20, 0xDE, 0xC0, 3, 0, 0, 0];
    let mut v = Vec::new();
    if d.len() < 8 {
        return v;
    }
    let mut i = 0;
    while i + 8 <= d.len() {
        if d[i] == 0x02 && d[i..i + 8] == pat {
            v.push(i);
            i += 8;
        } else {
            i += 1;
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&(-1i32).to_le_bytes());
        v.extend_from_slice(&7u32.to_le_bytes());
        v.extend_from_slice(&((name.len() + 1) as u32).to_le_bytes());
        v.extend_from_slice(name.as_bytes());
        v.push(0);
        v.extend_from_slice(&(payload.len() as u32 + 4).to_le_bytes());
        v.extend_from_slice(b".gao");
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn finds_named_records() {
        let mut d = vec![0xAAu8; 40];
        d.extend(rec("B_Jaf_Camera.gao", &[1; 30]));
        d.extend(rec("B_Jaf_EpauleG.gao", &[2; 30]));
        // name long enough that its length byte is a printable character (0x2e == '.')
        let long = format!("{}.gao", "N".repeat(41));
        assert_eq!(long.len() + 1, 0x2e);
        d.extend(rec(&long, &[3; 10]));
        // decoy: right text but wrong length field
        let mut bad = rec("Decoy.gao", &[]);
        bad[12] = 99;
        d.extend(bad);
        let r = iter_named(&d, 0, None);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].name, "B_Jaf_Camera.gao");
        assert_eq!(&d[r[1].payload..r[1].payload + 4], b".gao");
        assert_eq!(r[1].pre[1], 7);
        assert_eq!(r[2].name, long);
        // window restricts
        assert_eq!(iter_named(&d, 0, Some(r[1].name_off)).len(), 1);
    }

    #[test]
    fn gro_chain_and_magic() {
        let mut d = Vec::new();
        for (t, n) in [(1u32, 10usize), (4, 3), (5, 20)] {
            d.extend_from_slice(&((n + 4) as u32).to_le_bytes());
            d.extend_from_slice(&t.to_le_bytes());
            d.extend(std::iter::repeat(0u8).take(n));
        }
        let g = iter_gro(&d, 0, 99);
        assert_eq!(g.iter().map(|x| x.typ).collect::<Vec<_>>(), vec![1, 4, 5]);
        let mut m = vec![0u8; 5];
        m.extend_from_slice(&[0x02, 0x20, 0xDE, 0xC0, 3, 0, 0, 0]);
        assert_eq!(find_geo_magics(&m), vec![5]);
    }
}
