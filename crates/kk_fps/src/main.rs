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
mod audio_engine;
mod fsr;
mod graphics;
mod mods;
mod settings_menu;
#[cfg(feature = "raytracing")]
mod raytrace;
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
        // llvmpipe GL cannot compile some compute pipelines Bevy creates up front (SSAO depth prepass):
        // log and keep rendering instead of quitting; none of them is used by the software-GL batches
        app.insert_resource(bevy::render::error_handler::RenderErrorHandler(|e, _, _| {
            warn!("render error ignored on software GL: {:?}", e.ty);
            bevy::render::error_handler::RenderErrorPolicy::Ignore
        }));
        use bevy::render::batching::gpu_preprocessing::{GpuPreprocessingMode, GpuPreprocessingSupport};
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.insert_resource(GpuPreprocessingSupport {
                max_supported_mode: GpuPreprocessingMode::None,
            });
        }
    }
}

/// `KK_RES=WxH` shrinks the window (software-GL batches); default 1600x900.
fn window_res() -> (u32, u32) {
    std::env::var("KK_RES")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or((1600, 900))
}

fn main() {
    let assets = asset_dir();
    println!("King Kong FPS slice — assets from {}", assets.display());
    // DLSS (Bevy's dlss_wgpu) runs on the Vulkan backend only
    #[cfg(feature = "dlss")]
    if std::env::var("WGPU_BACKEND").is_err() {
        std::env::set_var("WGPU_BACKEND", "vulkan");
    }
    let mut app = App::new();
    // mods/ asset folders layered over the game assets (must precede the AssetPlugin)
    mods::register_asset_source(&mut app);
    // DLSS needs its project id before the render plugin and runs on Vulkan only
    #[cfg(feature = "dlss")]
    app.insert_resource(bevy::anti_alias::dlss::DlssProjectId(bevy::asset::uuid::uuid!("6a1c1d0e-7f3b-4c55-9b0e-4b4b2d5f8a11")));
    app.add_plugins(
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
        .add_plugins((mods::ModPlugin, audio_engine::AudioEnginePlugin, graphics::GraphicsPlugin));
    #[cfg(feature = "raytracing")]
    app.add_plugins(raytrace::RaytracePlugin);
    app.run();
}
