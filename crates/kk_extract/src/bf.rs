//! Jade BIG v36 BigFile reader (port of `tools/bf.py`).
//!
//! Layout (little endian): 0x2c byte header `BIG\0, ver(36), maxfile, maxdir, maxkey, root, ?, ?,
//! sizefat, numfat, ukey`; then `numfat` FAT blocks chained from 0x2c. Each FAT block starts with
//! six i32 `{maxfile, maxdir, posFat, nextFatHeader, firstIdx, lastIdx}`; at `posFat` follow
//! `sizefat*8` file table `{u32 pos, u32 key}`, `sizefat*88` file info `{len, prev, next, parentDir,
//! timestamp, name[64] at +20}` and `sizefat*84` dir info `{firstFile, firstSub, prev, next, parent,
//! name[64] at +20}`. File data at `pos` is `u32 len(&0x7fffffff)` + bytes.

use crate::err;
use crate::error::Result;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub pos: u64,
    pub key: u32,
    pub size: u32,
    pub parent: i32,
    pub name: String,
}
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub parent: i32,
    pub name: String,
}

pub struct BigFile<R: Read + Seek = File> {
    f: R,
    pub version: u32,
    pub files: Vec<FileEntry>,
    pub dirs: Vec<DirEntry>,
}

fn u32at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}
fn i32at(b: &[u8], p: usize) -> i32 {
    u32at(b, p) as i32
}
fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| c as char).collect() // latin1
}

impl BigFile<File> {
    pub fn open(path: &Path) -> Result<Self> {
        let f = File::open(path).map_err(|e| err!("{}: {e}", path.display()))?;
        Self::from_reader(f).map_err(|e| err!("{}: {e}", path.display()))
    }
}

impl<R: Read + Seek> BigFile<R> {
    pub fn from_reader(mut f: R) -> Result<Self> {
        let mut h = [0u8; 0x2c];
        f.read_exact(&mut h)?;
        if &h[0..4] != b"BIG\0" {
            return Err(err!("not a BIG file"));
        }
        let version = u32at(&h, 4);
        if version != 36 {
            return Err(err!("unsupported BIG version {version} (need 36)"));
        }
        let sizefat = u32at(&h, 32) as usize;
        let numfat = u32at(&h, 36) as usize;
        if sizefat == 0 || sizefat > (1 << 22) || numfat > 4096 {
            return Err(err!("implausible BIG header (sizefat {sizefat}, numfat {numfat})"));
        }
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        let mut pos = 0x2cu64;
        for _ in 0..numfat {
            f.seek(SeekFrom::Start(pos))?;
            let mut hd = [0u8; 24];
            f.read_exact(&mut hd)?;
            let (mf, md, pf, npf) = (i32at(&hd, 0), i32at(&hd, 4), i32at(&hd, 8), i32at(&hd, 12));
            if mf < 0 || md < 0 || mf as usize > sizefat || md as usize > sizefat || pf < 0 {
                return Err(err!("corrupt FAT header"));
            }
            f.seek(SeekFrom::Start(pf as u64))?;
            let mut ft = vec![0u8; sizefat * 8];
            let mut nt = vec![0u8; sizefat * 88];
            let mut dt = vec![0u8; sizefat * 84];
            f.read_exact(&mut ft)?;
            f.read_exact(&mut nt)?;
            f.read_exact(&mut dt)?;
            for i in 0..mf as usize {
                files.push(FileEntry {
                    pos: u32at(&ft, i * 8) as u64,
                    key: u32at(&ft, i * 8 + 4),
                    size: u32at(&nt, i * 88),
                    parent: u32at(&nt, i * 88 + 12) as i32,
                    name: cstr(&nt[i * 88 + 20..i * 88 + 84]),
                });
            }
            for i in 0..md as usize {
                dirs.push(DirEntry { parent: i32at(&dt, i * 84 + 16), name: cstr(&dt[i * 84 + 20..i * 84 + 84]) });
            }
            if npf == -1 {
                break;
            }
            pos = (npf as i64 - 24) as u64;
        }
        Ok(BigFile { f, version, files, dirs })
    }

    pub fn dir_path(&self, mut d: i32) -> String {
        let mut parts = Vec::new();
        let mut guard = 0;
        while d != -1 && d >= 0 && (d as usize) < self.dirs.len() && guard < 256 {
            parts.push(self.dirs[d as usize].name.as_str());
            d = self.dirs[d as usize].parent;
            guard += 1;
        }
        parts.reverse();
        parts.join("/")
    }

    pub fn path(&self, fe: &FileEntry) -> String {
        if fe.name.is_empty() {
            format!("_nokey/{:08x}", fe.key)
        } else {
            format!("{}/{}", self.dir_path(fe.parent), fe.name)
        }
    }

    /// Entry whose path equals `path` (case-insensitive, `/` separated).
    pub fn find_path(&self, path: &str) -> Option<&FileEntry> {
        self.files.iter().find(|fe| self.path(fe).eq_ignore_ascii_case(path))
    }

    /// `ROOT/Bin/<key>.bin` entry with the given resource key (extract_bin.py).
    pub fn find_bin(&self, key: u32) -> Option<&FileEntry> {
        self.files.iter().find(|fe| {
            fe.key == key && {
                let p = self.path(fe);
                p.ends_with(".bin") && p.contains("/Bin/")
            }
        })
    }

    /// First entry with `key` regardless of path (tex_decode.load_bank).
    pub fn find_key(&self, key: u32) -> Option<&FileEntry> {
        self.files.iter().find(|fe| fe.key == key)
    }

    pub fn read(&mut self, fe: &FileEntry) -> Result<Vec<u8>> {
        self.f.seek(SeekFrom::Start(fe.pos))?;
        let mut l = [0u8; 4];
        self.f.read_exact(&mut l)?;
        let n = (u32::from_le_bytes(l) & 0x7fff_ffff) as usize;
        let mut v = vec![0u8; n];
        self.f.read_exact(&mut v)?;
        Ok(v)
    }
}

/// Synthetic archive builder, used by the unit tests and the game-directory integration test.
#[doc(hidden)]
pub mod testutil {
    /// Build a synthetic BIG v36 archive. `fats`: groups of file indices stored per FAT block.
    pub struct F {
        pub key: u32,
        pub name: &'static str,
        pub dir: i32,
        pub data: Vec<u8>,
    }
    pub fn build(files: &[F], dirs: &[(&str, i32)], sizefat: usize, per_fat: usize) -> Vec<u8> {
        let nfat = (files.len().max(1) + per_fat - 1) / per_fat;
        let hdr = 0x2c;
        let fat_hdr_size = 24;
        let table = sizefat * (8 + 88 + 84);
        // layout: header | fat headers at 0x2c + i*(24+table)?  The chain uses npf-24 as the next header,
        // so place each FAT header immediately before its tables: [hdr(24)][tables]...
        let blk = fat_hdr_size + table;
        let mut out = vec![0u8; hdr + nfat * blk];
        let data_start = out.len();
        let datapos = data_start;
        let mut file_pos = Vec::new();
        let mut blob = Vec::new();
        for f in files {
            file_pos.push(datapos + blob.len());
            blob.extend_from_slice(&(f.data.len() as u32).to_le_bytes());
            blob.extend_from_slice(&f.data);
        }
        out[0..4].copy_from_slice(b"BIG\0");
        let put = |o: &mut Vec<u8>, p: usize, v: u32| o[p..p + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut out, 4, 36);
        put(&mut out, 32, sizefat as u32);
        put(&mut out, 36, nfat as u32);
        for fi in 0..nfat {
            let base = hdr + fi * blk;
            let lo = fi * per_fat;
            let hi = ((fi + 1) * per_fat).min(files.len());
            let nf = hi - lo.min(hi);
            let nd = if fi == 0 { dirs.len() } else { 0 };
            let pf = base + fat_hdr_size;
            put(&mut out, base, nf as u32);
            put(&mut out, base + 4, nd as u32);
            put(&mut out, base + 8, pf as u32);
            let next: i32 = if fi + 1 < nfat { (hdr + (fi + 1) * blk + 24) as i32 } else { -1 };
            put(&mut out, base + 12, next as u32);
            let ft = pf;
            let nt = ft + sizefat * 8;
            let dt = nt + sizefat * 88;
            for (k, f) in files[lo..hi].iter().enumerate() {
                put(&mut out, ft + k * 8, file_pos[lo + k] as u32);
                put(&mut out, ft + k * 8 + 4, f.key);
                put(&mut out, nt + k * 88, f.data.len() as u32);
                put(&mut out, nt + k * 88 + 12, f.dir as u32);
                out[nt + k * 88 + 20..nt + k * 88 + 20 + f.name.len()].copy_from_slice(f.name.as_bytes());
            }
            if fi == 0 {
                for (k, (n, par)) in dirs.iter().enumerate() {
                    put(&mut out, dt + k * 84 + 16, *par as u32);
                    out[dt + k * 84 + 20..dt + k * 84 + 20 + n.len()].copy_from_slice(n.as_bytes());
                }
            }
        }
        out.extend_from_slice(&blob);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;
    use std::io::Cursor;

    #[test]
    fn synthetic_archive_two_fats() {
        let files = vec![
            F { key: 0xff0003eb, name: "ff0003eb.bin", dir: 2, data: vec![1, 2, 3, 4, 5] },
            F { key: 0x11, name: "readme.txt", dir: 0, data: b"hello".to_vec() },
            F { key: 0xff80018c, name: "ff80018c.bin", dir: 2, data: vec![9; 1000] },
            F { key: 0x22, name: "", dir: -1, data: vec![] },
            F { key: 0xff0003eb, name: "other.dat", dir: 1, data: vec![7] },
        ];
        // dirs: 0 ROOT, 1 ROOT/Misc, 2 ROOT/Bin
        let dirs = [("ROOT", -1), ("Misc", 0), ("Bin", 0)];
        let img = build(&files, &dirs, 3, 2); // 3 FAT blocks, 2 files each
        let mut bf = BigFile::from_reader(Cursor::new(img)).unwrap();
        assert_eq!(bf.files.len(), 5);
        assert_eq!(bf.dirs.len(), 3);
        let e = bf.find_bin(0xff0003eb).unwrap().clone();
        assert_eq!(bf.path(&e), "ROOT/Bin/ff0003eb.bin");
        assert_eq!(bf.read(&e).unwrap(), vec![1, 2, 3, 4, 5]);
        let t = bf.find_path("root/misc/other.dat").unwrap().clone();
        assert_eq!(bf.read(&t).unwrap(), vec![7]);
        let r = bf.find_path("ROOT/readme.txt").unwrap().clone();
        assert_eq!(bf.read(&r).unwrap(), b"hello");
        let k = bf.find_key(0xff80018c).unwrap().clone();
        assert_eq!(bf.read(&k).unwrap().len(), 1000);
        let nk = bf.files.iter().find(|f| f.key == 0x22).unwrap();
        assert_eq!(bf.path(nk), "_nokey/00000022");
        assert!(bf.find_bin(0x99).is_none());
        // key shared by a .bin and another file: find_bin must pick the .bin
        assert_eq!(bf.find_bin(0xff0003eb).unwrap().name, "ff0003eb.bin");
    }

    #[test]
    fn rejects_bad_headers() {
        assert!(BigFile::from_reader(Cursor::new(vec![0u8; 100])).is_err());
        let mut img = build(&[], &[], 2, 1);
        img[4] = 35;
        assert!(BigFile::from_reader(Cursor::new(img)).is_err());
        assert!(BigFile::from_reader(Cursor::new(vec![1u8; 10])).is_err());
    }

    #[test]
    fn dir_cycle_is_bounded() {
        let files = vec![F { key: 1, name: "a", dir: 0, data: vec![] }];
        let dirs = [("A", 1), ("B", 0)];
        let img = build(&files, &dirs, 2, 4);
        let bf = BigFile::from_reader(Cursor::new(img)).unwrap();
        let _ = bf.path(&bf.files[0]); // terminates
    }
}
