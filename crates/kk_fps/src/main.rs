//! King Kong (2005) — first-person Jack vs. V-Rex playable test slice.
//!
//! Assets (meshes, textures, skeletons, animation clips) are the game's own data,
//! extracted from the user's PC Gamer's Edition BF archives by the tools in
//! `research/pc/tools` and exported to glTF. Gameplay numbers come from static analysis of
//! KingKong8.exe (`research/pc/code/gameplay_spec.md`); see `spec.rs` for provenance.
//!
//! Assets are not part of this repository. They are read from `KK_ASSETS`, or by default
//! from `../research/pc/game_assets` next to this workspace.

mod anim;
mod atmos;
mod autotest;
mod batch;
mod creatures;
mod events;
mod fx;
mod godray;
mod sky;
mod sfx;
mod fightarena;
mod hud;
mod kong;
mod kong_cam;
mod kong_fx;
mod kong_fur;
mod meshcol;
mod breakable;
mod spears;
mod player;
mod rex;
mod scene;
mod swamp;
mod spec;
mod tbatch;
mod testarea;
mod weapons;
mod weather;
mod world;

use bevy::prelude::*;
use std::path::PathBuf;

/// Folder holding jack_fps_*.glb, trex_inplace.glb and trex_rootmotion.json.
pub fn asset_dir() -> PathBuf {
    if let Ok(p) = std::env::var("KK_ASSETS") {
        return PathBuf::from(p);
    }
    // crates/kk_fps -> workspace -> KongPS2/research/pc/game_assets
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidate = manifest.join("../../../research/pc/game_assets");
    if candidate.exists() {
        // not canonicalized: on Windows that yields a \\?\ verbatim path
        return candidate;
    }
    // fallback: an `assets` folder next to the executable
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("assets")))
        .unwrap_or_else(|| PathBuf::from("assets"))
}

/// Test-only: force Bevy's CPU batching path on software GL (llvmpipe has no usable
/// compute support for depth downsampling / GPU culling).
struct SoftwareGlPlugin;

impl Plugin for SoftwareGlPlugin {
    fn build(&self, _app: &mut App) {}
    fn finish(&self, app: &mut App) {
        if std::env::var("KK_SOFTWARE_GL").is_err() {
            return;
        }
        use bevy::render::batching::gpu_preprocessing::{GpuPreprocessingMode, GpuPreprocessingSupport};
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.insert_resource(GpuPreprocessingSupport {
                max_supported_mode: GpuPreprocessingMode::None,
            });
        }
    }
}

/// `KK_RES=WxH` shrinks the window (software-GL batches); default 1600x900.
fn window_res() -> (f32, f32) {
    std::env::var("KK_RES")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or((1600.0, 900.0))
}

fn main() {
    let assets = asset_dir();
    println!("King Kong FPS slice — assets from {}", assets.display());
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: assets.to_string_lossy().into_owned(),
                    ..default()
                })
                // Software GL (used by the KK_AUTOTEST smoke test) cannot run Bevy's GPU
                // culling compute shaders; real GPUs keep the default.
                .set(bevy::render::RenderPlugin {
                    synchronous_pipeline_compilation: std::env::var("KK_SOFTWARE_GL").is_ok(),
                    ..default()
                })
                .set(bevy::pbr::PbrPlugin {
                    use_gpu_instance_buffer_builder: std::env::var("KK_SOFTWARE_GL").is_err(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "King Kong (2005) — Jack vs. V-Rex test slice".into(),
                        resolution: window_res().into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins((
            events::EventsPlugin,
            sfx::SfxPlugin,
            fx::FxPlugin,
            anim::AnimPlugin,
            world::WorldPlugin,
            player::PlayerPlugin,
            weapons::WeaponsPlugin,
            rex::RexPlugin,
            hud::HudPlugin,
            autotest::AutoTestPlugin,
            autotest::GalleryPlugin,
            batch::BatchPlugin,
            weather::WeatherPlugin,
            (atmos::AtmosPlugin, godray::GodRayPlugin, sky::SkyPlugin),
            SoftwareGlPlugin,
        ))
        .add_plugins(kong::KongPlugin)
        .add_plugins(swamp::SwampPlugin)
        .add_plugins(kong_fur::KongFurPlugin)
        .add_plugins(breakable::BreakablePlugin)
        .add_plugins(spears::SpearPlugin)
        .add_plugins((creatures::CreaturePlugin, testarea::TestAreaPlugin, tbatch::TBatchPlugin))
        .run();
}
