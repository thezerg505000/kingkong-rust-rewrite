//! Reading the user's game install: locating the .bf files and decoding streams / texture banks on demand.

use crate::bf::BigFile;
use crate::build::{BfEntry, Source};
use crate::err;
use crate::error::Result;
use crate::stream::unstream;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const MAPS_BF: &str = "KKMaps.bf";
pub const TEXTURES_BF: &str = "KKTextures.bf";
pub const SOUND_BF: &str = "Sound_Common.bf";

/// Find `file` (case-insensitive) in `dir` or up to three levels below it.
pub fn find_game_file(dir: &Path, file: &str) -> Option<PathBuf> {
    for e in walkdir::WalkDir::new(dir).max_depth(3).into_iter().filter_map(|e| e.ok()) {
        if e.file_type().is_file() && e.file_name().to_string_lossy().eq_ignore_ascii_case(file) {
            return Some(e.into_path());
        }
    }
    None
}

pub struct GameSource {
    maps_path: PathBuf,
    tex_path: PathBuf,
    snd_path: Option<PathBuf>,
    snd: Option<BigFile>,
    maps: Option<BigFile>,
    tex: Option<BigFile>,
    cache: VecDeque<(String, Arc<Vec<u8>>)>,
    /// number of decoded streams/banks kept in memory (each is 15-90 MB)
    pub capacity: usize,
}

impl GameSource {
    pub fn open(game_dir: &Path) -> Result<GameSource> {
        let maps_path = find_game_file(game_dir, MAPS_BF).ok_or_else(|| err!("{MAPS_BF} not found under {}", game_dir.display()))?;
        let tex_path = find_game_file(game_dir, TEXTURES_BF).ok_or_else(|| err!("{TEXTURES_BF} not found under {}", game_dir.display()))?;
        let snd_path = find_game_file(game_dir, SOUND_BF);
        Ok(GameSource { maps_path, tex_path, snd_path, snd: None, maps: None, tex: None, cache: VecDeque::new(), capacity: 3 })
    }
    pub fn maps_path(&self) -> &Path {
        &self.maps_path
    }
    pub fn sound_path(&self) -> Option<&Path> {
        self.snd_path.as_deref()
    }
    fn snd_bf(&mut self) -> Result<&mut BigFile> {
        if self.snd.is_none() {
            let p = self.snd_path.clone().ok_or_else(|| err!("{SOUND_BF} not found in the game directory"))?;
            self.snd = Some(BigFile::open(&p)?);
        }
        Ok(self.snd.as_mut().unwrap())
    }
    pub fn textures_path(&self) -> &Path {
        &self.tex_path
    }
    fn cached(&mut self, tag: &str) -> Option<Arc<Vec<u8>>> {
        let pos = self.cache.iter().position(|(k, _)| k == tag)?;
        let e = self.cache.remove(pos).unwrap();
        let a = e.1.clone();
        self.cache.push_back(e);
        Some(a)
    }
    fn put(&mut self, tag: String, d: Arc<Vec<u8>>) {
        self.cache.push_back((tag, d));
        while self.cache.len() > self.capacity {
            self.cache.pop_front();
        }
    }
}

fn parse_key(s: &str) -> Result<u32> {
    u32::from_str_radix(s, 16).map_err(|_| err!("bad resource key '{s}'"))
}

impl Source for GameSource {
    fn stream(&mut self, key: &str) -> Result<Arc<Vec<u8>>> {
        let tag = format!("maps:{key}");
        if let Some(a) = self.cached(&tag) {
            return Ok(a);
        }
        if self.maps.is_none() {
            self.maps = Some(BigFile::open(&self.maps_path)?);
        }
        let bf = self.maps.as_mut().unwrap();
        let k = parse_key(key)?;
        let fe = bf.find_bin(k).cloned().ok_or_else(|| err!("{MAPS_BF}: no Bin/{key}.bin (is this the PC Gamer's Edition?)"))?;
        let raw = bf.read(&fe)?;
        let d = Arc::new(unstream(&raw).map_err(|e| err!("{key}: {e}"))?);
        self.put(tag, d.clone());
        Ok(d)
    }
    fn bank(&mut self, key: &str) -> Result<Arc<Vec<u8>>> {
        let tag = format!("tex:{key}");
        if let Some(a) = self.cached(&tag) {
            return Ok(a);
        }
        if self.tex.is_none() {
            self.tex = Some(BigFile::open(&self.tex_path)?);
        }
        let bf = self.tex.as_mut().unwrap();
        let k = parse_key(key)?;
        let fe = bf.find_key(k).cloned().ok_or_else(|| err!("{TEXTURES_BF}: no bank {key}"))?;
        let raw = bf.read(&fe)?;
        let d = Arc::new(unstream(&raw).map_err(|e| err!("bank {key}: {e}"))?);
        self.put(tag, d.clone());
        Ok(d)
    }
    fn sound_index(&mut self) -> Result<Vec<BfEntry>> {
        let bf = self.snd_bf()?;
        Ok(bf.files.iter().map(|fe| BfEntry { path: bf.path(fe), key: fe.key }).collect())
    }
    fn sound_read(&mut self, i: usize) -> Result<Vec<u8>> {
        let bf = self.snd_bf()?;
        let fe = bf.files.get(i).cloned().ok_or_else(|| err!("sound entry {i} out of range"))?;
        bf.read(&fe)
    }
}
