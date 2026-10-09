//! Level 03E weather: rain (the level places SFX_*_RainSnowStatic emitters and plays the
//! "Amb_03E_area_a_rain" ambience), occasional thunder (Wac/Thunder.wac) with a light flash.
//! Drop density, speed and thunder timing are [G]; the sounds are the originals.

use crate::anim::GameState;
use crate::player::MainCam;
use crate::sfx::PlaySfx;
use bevy::prelude::*;
use rand::{Rng, SeedableRng};

const DROPS: usize = 5200;
const BOX: f32 = 22.0;

#[derive(Resource)]
pub struct Rain {
    drops: Vec<Vec3>,
    pub enabled: bool,
}

#[derive(Resource)]
struct Thunder {
    next: f32,
    flash: f32,
    boom_at: Option<f32>,
    base_ambient: Option<f32>,
}

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let drops = (0..DROPS)
            .map(|_| Vec3::new(rng.gen_range(-BOX..BOX), rng.gen_range(-2.0..14.0), rng.gen_range(-BOX..BOX)))
            .collect();
        app.insert_resource(Rain { drops, enabled: true })
            .insert_resource(Thunder { next: 18.0, flash: 0.0, boom_at: None, base_ambient: None })
            .add_systems(OnEnter(GameState::Playing), start_ambience)
            .add_systems(Update, (rain, thunder).run_if(in_state(GameState::Playing)));
    }
}

fn start_ambience(
    arena: Res<crate::world::Arena>,
    tunables: Res<crate::mods::Tunables>,
    mut rain: ResMut<Rain>,
    mut gizmo_cfg: ResMut<GizmoConfigStore>,
    #[cfg(feature = "audio_engine")] mut commands: Commands,
    #[cfg(feature = "audio_engine")] assets: Res<AssetServer>,
    #[cfg(feature = "audio_engine")] defs: Res<crate::sfx::SoundDefs>,
    #[cfg(feature = "audio_engine")] settings: Res<crate::audio_engine::AudioSettings>,
    #[cfg(feature = "audio_engine")] reverb: Res<crate::audio_engine::backend::ReverbBus>,
) {
    rain.enabled = arena.level.is_some() && std::env::var("KK_NO_RAIN").is_err() && tunables.get("rain", 1.0) > 0.5;
    let (cfg, _) = gizmo_cfg.config_mut::<DefaultGizmoConfigGroup>();
    cfg.line.width = if crate::scene::swamp() { 1.0 } else { 1.2 };
    #[cfg(feature = "audio_engine")]
    if rain.enabled {
        // 05C has its own rain ambience (Amb_05C_area_c_rain, "05 River" bank) [C name]
        let amb = if crate::scene::marsh05c() && defs.0.contains_key("Amb_05C_area_c_rain") { "Amb_05C_area_c_rain" } else { "Amb_03E_area_a_rain" };
        if let Some(d) = defs.0.get(amb) {
            if let Some(f) = d.files.first() {
                let e = crate::audio_engine::backend::spawn_voice(&mut commands, &settings, &reverb, assets.load(f.clone()), d.volume, 1.0, None, true);
                commands.entity(e).insert(Name::new("Ambience"));
            }
        }
    }
}

fn rain(time: Res<Time>, mut rain: ResMut<Rain>, cam: Query<&GlobalTransform, With<MainCam>>, mut gizmos: Gizmos) {
    if !rain.enabled {
        return;
    }
    let Ok(cam) = cam.single() else { return };
    let c = cam.translation();
    let dt = time.delta_secs();
    let vel = Vec3::new(1.2, -13.0, 0.6);
    // the swamp clip's rain is a fine, grey veil rather than bright streaks [G]
    let (color, streak) = if crate::scene::swamp() { (Color::srgba(0.72, 0.80, 0.78, 0.20), 0.055) } else { (Color::srgba(0.82, 0.90, 0.90, 0.42), 0.07) };
    for d in rain.drops.iter_mut() {
        *d += vel * dt;
        // wrap inside a box that follows the camera
        let mut rel = *d - c;
        if rel.y < -3.0 {
            rel.y += 17.0;
        }
        rel.x = (rel.x + BOX).rem_euclid(2.0 * BOX) - BOX;
        rel.z = (rel.z + BOX).rem_euclid(2.0 * BOX) - BOX;
        *d = c + rel;
        gizmos.line(*d, *d + vel * streak, color);
    }
}

fn thunder(
    time: Res<Time>,
    mut th: ResMut<Thunder>,
    rain: Res<Rain>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    if !rain.enabled {
        return;
    }
    let now = time.elapsed_secs();
    let base = *th.base_ambient.get_or_insert(ambient.brightness);
    if now >= th.next {
        th.next = now + rand::thread_rng().gen_range(25.0..45.0);
        th.flash = 0.18;
        th.boom_at = Some(now + rand::thread_rng().gen_range(0.6..1.8));
    }
    if th.flash > 0.0 {
        th.flash -= time.delta_secs();
        ambient.brightness = base * if th.flash > 0.09 { 3.0 } else { 1.8 };
    } else {
        ambient.brightness = base;
    }
    if th.boom_at.is_some_and(|t| now >= t) {
        th.boom_at = None;
        sfx.write(PlaySfx { def: "Thunder", pos: None, gain: 0.8 });
    }
}
