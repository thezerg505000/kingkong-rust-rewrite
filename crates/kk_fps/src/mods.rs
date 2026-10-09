//! Mod loader.
//!
//! A mod is a folder in `mods/` (next to the executable; `KK_MODS` overrides the folder) with a `mod.json`:
//!
//! ```json
//! { "name": "Sharper Kong", "version": "1.0", "author": "you", "description": "...", "priority": 10 }
//! ```
//!
//! and any of:
//! * `assets/` — files that replace the game assets of the same relative path (a texture, a glb, a sound,
//!   `sound_defs.json`, a level collision file...). Higher `priority` wins when two mods ship the same file.
//!   Both the asset server and the files the game reads directly go through the same lookup (`resolve`).
//! * `tunables.json` — `{ "key": number }` overrides of the presentation values the game exposes
//!   (`Tunables`, documented in docs/MODDING.md): FOV, fog density, rain density, time scale, ...
//!
//! Mods are listed in the F10 menu and can be switched on and off there (saved under `mods` in
//! `kk_settings.json`); the asset layers are fixed at start-up, so a change applies on the next start.
//! The game never ships game files: mods hold the modder's own files, or files the player made from their
//! own copy of the game.

use bevy::asset::io::file::FileAssetReader;
use bevy::asset::io::{AssetReader, AssetReaderError, AssetSourceBuilder, AssetSourceId, PathStream, Reader};
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq)]
pub struct ModInfo {
    /// folder name (the key in the settings file)
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub priority: i32,
    pub enabled: bool,
    pub dir: PathBuf,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ModJson {
    name: String,
    version: String,
    author: String,
    description: String,
    priority: i32,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct ModList {
    pub mods: Vec<ModInfo>,
}

impl ModList {
    pub fn save(&self) {
        let map: serde_json::Map<String, serde_json::Value> = self.mods.iter().map(|m| (m.id.clone(), serde_json::Value::Bool(m.enabled))).collect();
        crate::graphics::save_section("mods", serde_json::Value::Object(map));
    }
}

pub fn mods_dir() -> PathBuf {
    if let Ok(p) = std::env::var("KK_MODS") {
        return PathBuf::from(p);
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("mods"))).unwrap_or_else(|| PathBuf::from("mods"))
}

/// Scan `mods/` once: every folder with a readable `mod.json`, enabled unless the settings file says otherwise.
pub fn scan() -> ModList {
    let enabled: HashMap<String, bool> = std::fs::read_to_string(crate::graphics::GraphicsSettings::path())
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("mods").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let mut mods = Vec::new();
    if let Ok(rd) = std::fs::read_dir(mods_dir()) {
        for e in rd.flatten() {
            let dir = e.path();
            let Ok(txt) = std::fs::read_to_string(dir.join("mod.json")) else { continue };
            let id = e.file_name().to_string_lossy().into_owned();
            let j: ModJson = match serde_json::from_str(&txt) {
                Ok(j) => j,
                Err(err) => {
                    eprintln!("mod {id}: bad mod.json ({err}), skipped");
                    continue;
                }
            };
            mods.push(ModInfo {
                name: if j.name.is_empty() { id.clone() } else { j.name },
                version: j.version,
                author: j.author,
                description: j.description,
                priority: j.priority,
                enabled: *enabled.get(&id).unwrap_or(&true),
                id,
                dir,
            });
        }
    }
    // highest priority first, then by name for a stable order
    mods.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
    ModList { mods }
}

/// The asset roots in lookup order: enabled mods' `assets/` folders by priority, then the game assets.
fn layers() -> &'static Vec<PathBuf> {
    static L: OnceLock<Vec<PathBuf>> = OnceLock::new();
    L.get_or_init(|| {
        let mut v: Vec<PathBuf> = scan().mods.iter().filter(|m| m.enabled).map(|m| m.dir.join("assets")).filter(|p| p.is_dir()).collect();
        v.push(crate::asset_dir());
        v
    })
}

/// Path of an asset file after mod overrides (for the files the game reads with std::fs).
pub fn resolve(rel: impl AsRef<Path>) -> PathBuf {
    let rel = rel.as_ref();
    let ls = layers();
    for l in &ls[..ls.len() - 1] {
        let p = l.join(rel);
        if p.is_file() {
            return p;
        }
    }
    ls[ls.len() - 1].join(rel)
}

/// Asset reader for the default source: the same layered lookup as `resolve`.
struct ModReader {
    readers: Vec<(PathBuf, FileAssetReader)>,
}

impl ModReader {
    fn new() -> Self {
        Self { readers: layers().iter().map(|p| (p.clone(), FileAssetReader::new(p))).collect() }
    }
    fn pick(&self, path: &Path) -> &FileAssetReader {
        let n = self.readers.len();
        for (root, r) in &self.readers[..n - 1] {
            if root.join(path).is_file() {
                return r;
            }
        }
        &self.readers[n - 1].1
    }
}

impl AssetReader for ModReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.pick(path).read(path).await
    }
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.pick(path).read_meta(path).await
    }
    async fn read_directory<'a>(&'a self, path: &'a Path) -> Result<Box<PathStream>, AssetReaderError> {
        self.readers[self.readers.len() - 1].1.read_directory(path).await
    }
    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        self.readers[self.readers.len() - 1].1.is_directory(path).await
    }
}

/// Register the layered reader as the default asset source. Call before `DefaultPlugins`.
pub fn register_asset_source(app: &mut App) {
    app.register_asset_source(AssetSourceId::Default, AssetSourceBuilder::new(|| Box::new(ModReader::new())));
}

/// Merged `tunables.json` of the enabled mods (higher priority wins).
#[derive(Resource, Clone, Debug, Default)]
pub struct Tunables(pub HashMap<String, f64>);

impl Tunables {
    pub fn get(&self, key: &str, default: f32) -> f32 {
        self.0.get(key).map(|v| *v as f32).unwrap_or(default)
    }
}

fn load_tunables(list: &ModList) -> Tunables {
    let mut t = HashMap::new();
    // lowest priority first so higher priorities overwrite
    for m in list.mods.iter().rev().filter(|m| m.enabled) {
        let Ok(txt) = std::fs::read_to_string(m.dir.join("tunables.json")) else { continue };
        match serde_json::from_str::<HashMap<String, f64>>(&txt) {
            Ok(map) => t.extend(map),
            Err(e) => warn!("mod {}: bad tunables.json ({e})", m.id),
        }
    }
    Tunables(t)
}

pub struct ModPlugin;

impl Plugin for ModPlugin {
    fn build(&self, app: &mut App) {
        let list = scan();
        for m in &list.mods {
            info!("mod: {} {} by {} (priority {}, {})", m.name, m.version, m.author, m.priority, if m.enabled { "on" } else { "off" });
        }
        let t = load_tunables(&list);
        if !t.0.is_empty() {
            info!("mod tunables: {:?}", t.0);
        }
        app.insert_resource(list).insert_resource(t).add_systems(Update, (apply_time_scale, apply_scene_tunables));
    }
}

/// `time_scale` tunable: slow motion / fast forward of the whole game.
fn apply_time_scale(t: Res<Tunables>, mut time: ResMut<Time<Virtual>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    *done = true;
    let s = t.get("time_scale", 1.0).clamp(0.05, 4.0);
    if (s - 1.0).abs() > 1e-3 {
        time.set_relative_speed(s);
    }
}

/// `fog_density_scale`, `sun_intensity_scale`, `ambient_scale`: scale the scene's recovered values once they exist.
fn apply_scene_tunables(
    t: Res<Tunables>,
    mut fogs: Query<&mut DistanceFog, Added<DistanceFog>>,
    mut suns: Query<&mut DirectionalLight, Added<DirectionalLight>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut ambient_done: Local<bool>,
) {
    let fs = t.get("fog_density_scale", 1.0);
    if (fs - 1.0).abs() > 1e-4 {
        for mut f in &mut fogs {
            f.falloff = match f.falloff.clone() {
                FogFalloff::Exponential { density } => FogFalloff::Exponential { density: density * fs },
                FogFalloff::ExponentialSquared { density } => FogFalloff::ExponentialSquared { density: density * fs },
                FogFalloff::Linear { start, end } => FogFalloff::Linear { start: start / fs.max(0.01), end: end / fs.max(0.01) },
                other => other,
            };
        }
    }
    let ss = t.get("sun_intensity_scale", 1.0);
    if (ss - 1.0).abs() > 1e-4 {
        for mut l in &mut suns {
            l.illuminance *= ss;
        }
    }
    let am = t.get("ambient_scale", 1.0);
    if !*ambient_done && ambient.is_changed() && (am - 1.0).abs() > 1e-4 {
        ambient.brightness *= am;
        *ambient_done = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_orders_by_priority_and_reads_tunables() {
        let dir = std::env::temp_dir().join(format!("kk_mods_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (id, prio, fog) in [("a_low", 1, 0.5), ("b_high", 9, 0.8)] {
            let d = dir.join(id);
            std::fs::create_dir_all(d.join("assets")).unwrap();
            std::fs::write(d.join("mod.json"), format!(r#"{{"name":"{id}","version":"1","priority":{prio}}}"#)).unwrap();
            std::fs::write(d.join("tunables.json"), format!(r#"{{"fog_density_scale":{fog}}}"#)).unwrap();
        }
        std::fs::create_dir_all(dir.join("not_a_mod")).unwrap();
        std::env::set_var("KK_MODS", &dir);
        std::env::set_var("KK_SETTINGS", dir.join("none.json"));
        let list = scan();
        assert_eq!(list.mods.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["b_high", "a_low"]);
        let t = load_tunables(&list);
        // the higher priority mod wins
        assert!((t.get("fog_density_scale", 1.0) - 0.8).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
