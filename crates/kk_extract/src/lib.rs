//! Asset extractor: reads the user's own King Kong (2005) PC Gamer's Edition files (KKMaps.bf,
//! KKTextures.bf, Sound_Common.bf) and writes the asset tree the game loads. Contains no game data.
//!
//! * [`bf`]: BIG v36 archive index and reads
//! * [`lzo`], [`stream`]: LZO1X and the block framing of Bin streams / texture banks
//! * [`records`], [`geo`], [`skel`], [`anim`]: Jade record scanning, GEO meshes, bone hierarchy, track lists
//! * [`texture`]: Xenos texture banks -> RGBA/PNG
//! * [`glb`], [`build`]: glTF writer and the asset recipes ([`manifest`])
//!
//! Entry points: [`build_all`] (library, used by the game on first run) and the `kk-extract` binary.

pub mod anim;
pub mod bf;
pub mod build;
pub mod creature;
pub mod error;
pub mod geo;
pub mod images;
pub mod kkc;
pub mod sound;
pub mod glb;
pub mod lzo;
pub mod mat;
pub mod records;
pub mod skel;
pub mod source;
pub mod stream;
pub mod texture;

pub use error::{Error, Result};

use build::{BuildOpts, ClipDb, Source};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Bumped when the output of a recipe changes for the same input (invalidates the stamp file).
pub const BUILDER_VERSION: u32 = 1;
pub const STAMP_FILE: &str = ".kk_extract.json";

/// Names of the assets in the built-in manifest.
pub fn manifest_names() -> Vec<String> {
    build::manifest().map(|m| m.into_iter().map(|(k, _)| k).collect()).unwrap_or_default()
}

/// True when `out_dir` holds every manifest asset recorded by a previous successful build (cheap check
/// for the game's first-run logic; it does not look at the game files, [`build_all`] does the exact check).
pub fn is_complete(out_dir: &Path) -> bool {
    let Some(stamp) = std::fs::read(out_dir.join(STAMP_FILE)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return false };
    let names = manifest_names();
    !names.is_empty()
        && names.iter().all(|n| {
            let files = stamp[n]["files"].as_array();
            files.map(|f| !f.is_empty() && f.iter().all(|x| x.as_str().map(|x| out_dir.join(x).is_file()).unwrap_or(false))).unwrap_or(false)
        })
}

/// Progress callback payload.
#[derive(Debug, Clone)]
pub struct Progress<'a> {
    /// 1-based index of the asset being built
    pub step: usize,
    pub total: usize,
    pub asset: &'a str,
    pub message: &'a str,
}

#[derive(Debug, Default)]
pub struct Report {
    pub built: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<(String, String)>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.failed.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// build only this asset (file name, e.g. "trex_inplace.glb")
    pub only: Option<String>,
    /// rebuild even if the output is up to date
    pub force: bool,
}

fn fnv(s: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &b in s {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn file_sig(p: &Path) -> String {
    match std::fs::metadata(p) {
        Ok(m) => {
            let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            format!("{}:{}", m.len(), t)
        }
        Err(_) => "missing".into(),
    }
}

/// Build every manifest asset from the game install into `out_dir`.
pub fn build_all(game_dir: &Path, out_dir: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Report> {
    build_with(game_dir, out_dir, &Options::default(), progress)
}

pub fn build_with(game_dir: &Path, out_dir: &Path, opts: &Options, progress: &mut dyn FnMut(&Progress)) -> Result<Report> {
    let mut src = source::GameSource::open(game_dir)?;
    let game_sig = format!("{}|{}|{}", file_sig(src.maps_path()), file_sig(src.textures_path()), src.sound_path().map(file_sig).unwrap_or_default());
    build_from_source(&mut src, &game_sig, out_dir, opts, progress)
}

/// Same as [`build_with`] on an already opened [`Source`] (tests, alternative storage).
pub fn build_from_source(src: &mut dyn Source, game_sig: &str, out_dir: &Path, opts: &Options, progress: &mut dyn FnMut(&Progress)) -> Result<Report> {
    let mut assets = build::manifest()?;
    if let Some(only) = &opts.only {
        assets.retain(|(k, _)| k == only);
        if assets.is_empty() {
            return Err(err!("no asset named '{only}' (known: {})", manifest_names().join(", ")));
        }
    }
    std::fs::create_dir_all(out_dir)?;
    let stamp_path = out_dir.join(STAMP_FILE);
    let mut stamp: Value = std::fs::read(&stamp_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}));
    if !stamp.is_object() {
        stamp = json!({});
    }
    let clips = ClipDb::builtin();
    let mut rep = Report::default();
    let total = assets.len();
    for (i, (name, recipe)) in assets.iter().enumerate() {
        let step = i + 1;
        let recipe_hash = format!("{:016x}", fnv(format!("{BUILDER_VERSION}|{recipe}").as_bytes()));
        let sig = format!("{recipe_hash}|{game_sig}");
        let prev = &stamp[name];
        let outputs: Vec<String> = prev["files"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        if !opts.force && prev["sig"] == sig && !outputs.is_empty() && outputs.iter().all(|f| out_dir.join(f).is_file()) {
            progress(&Progress { step, total, asset: name, message: "up to date" });
            rep.skipped.push(name.clone());
            continue;
        }
        progress(&Progress { step, total, asset: name, message: "building" });
        let mut log = |m: &str| progress(&Progress { step, total, asset: name, message: m });
        match build::build_asset(src, name, recipe, &clips, &BuildOpts::default(), &mut log) {
            Ok(b) => {
                let mut names = Vec::new();
                let mut werr = None;
                for (f, data) in &b.files {
                    let path: PathBuf = out_dir.join(f);
                    let tmp = out_dir.join(format!("{f}.tmp"));
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, &path)) {
                        werr = Some(format!("{}: {e}", path.display()));
                        break;
                    }
                    names.push(f.clone());
                }
                match werr {
                    None => {
                        stamp[name] = json!({"sig": sig, "files": names});
                        // persist after each asset so an interrupted run resumes
                        let _ = std::fs::write(&stamp_path, serde_json::to_vec_pretty(&stamp)?);
                        rep.built.push(name.clone());
                    }
                    Some(e) => rep.failed.push((name.clone(), e)),
                }
            }
            Err(e) => rep.failed.push((name.clone(), e.0)),
        }
    }
    Ok(rep)
}
