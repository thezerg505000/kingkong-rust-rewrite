//! Audio engine: the game's own sound definitions (`sfx.rs`) played through Firewheel (`bevy_seedling`).
//!
//! * `AudioEngine::Original`: what the 2005 game does as far as recovered — one voice per sound definition,
//!   volume from the definition, a simple distance roll-off `sfx::distance_gain` [G], ±3 % pitch variation.
//! * `AudioEngine::Remaster`: positional sounds become 3D voices (HRTF binaural or panned spatialisation with
//!   distance attenuation), sounds behind level geometry are muffled (a ray through the level collision drives
//!   a low-pass cutoff), and every voice sends into an environmental reverb sized to the scene (stone
//!   courtyard / ruins in 03E, open swamp in 05C/07D).
//!
//! Every remaster feature is a setting (F10 menu, `audio` section of `kk_settings.json`). Values are
//! presentation choices [G]; the sound files and their volumes are the game's [C].

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioEngine {
    Original,
    Remaster,
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub engine: AudioEngine,
    /// binaural 3D audio (headphones); off = speaker panning
    pub hrtf: bool,
    pub reverb: bool,
    pub occlusion: bool,
    /// 0..1
    pub master: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { engine: AudioEngine::Remaster, hrtf: true, reverb: true, occlusion: true, master: 0.9 }
    }
}

impl AudioSettings {
    pub fn load() -> Self {
        let v: Option<serde_json::Value> = std::fs::read_to_string(crate::graphics::GraphicsSettings::path()).ok().and_then(|t| serde_json::from_str(&t).ok());
        v.and_then(|v| serde_json::from_value(v["audio"].clone()).ok()).unwrap_or_default()
    }
    pub fn remaster(&self) -> bool {
        self.engine == AudioEngine::Remaster
    }
}

/// Reverb character of the current scene [G].
pub fn scene_reverb() -> (f32, f32, f32) {
    // (room size, damping, send level)
    if crate::scene::swamp() {
        (0.55, 0.7, 0.18)
    } else {
        (0.78, 0.45, 0.28)
    }
}

pub struct AudioEnginePlugin;

impl Plugin for AudioEnginePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(AudioSettings::load());
        #[cfg(feature = "audio_engine")]
        {
            app.add_plugins(bevy_seedling::SeedlingPlugins)
                .init_resource::<backend::ReverbBus>()
                .add_systems(Update, (backend::listener, backend::buses, backend::occlusion));
        }
    }
}

#[cfg(feature = "audio_engine")]
pub mod backend {
    use super::AudioSettings;
    use crate::player::MainCam;
    use bevy::prelude::*;
    use bevy_seedling::prelude::*;

    #[derive(Resource, Default)]
    pub struct ReverbBus(pub Option<Entity>);

    /// A positional voice whose occlusion is re-tested while it plays.
    #[derive(Component)]
    pub struct Occludable {
        pub pos: Vec3,
    }

    /// One voice of a sound definition. `pos` None = 2D (Jack's own sounds, UI).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_voice(
        commands: &mut Commands,
        settings: &AudioSettings,
        reverb: &ReverbBus,
        handle: Handle<AudioSample>,
        gain: f32,
        speed: f64,
        pos: Option<Vec3>,
        looping: bool,
    ) -> Entity {
        let mut player = SamplePlayer::new(handle).with_volume(Volume::Linear(gain));
        if looping {
            player = player.looping();
        }
        let playback = PlaybackSettings::default().with_speed(speed);
        let send = super::scene_reverb().2;
        match (settings.remaster(), pos) {
            (true, Some(p)) => {
                let bus = reverb.0.filter(|_| settings.reverb);
                let spatial = SpatialBasicNode { muffle_cutoff_hz: 20_000.0, ..default() };
                // HRTF has no muffle stage of its own: a low-pass in front of it carries the occlusion
                let lp = FastLowpassNode::<2> { cutoff_hz: 20_000.0, ..default() };
                let mut e = if settings.hrtf {
                    match bus {
                        Some(b) => commands.spawn((player, playback, Transform::from_translation(p), sample_effects![lp, HrtfNode::default(), SendNode::new(Volume::Linear(send), b)])),
                        None => commands.spawn((player, playback, Transform::from_translation(p), sample_effects![lp, HrtfNode::default()])),
                    }
                } else {
                    match bus {
                        Some(b) => commands.spawn((player, playback, Transform::from_translation(p), sample_effects![spatial, SendNode::new(Volume::Linear(send), b)])),
                        None => commands.spawn((player, playback, Transform::from_translation(p), sample_effects![spatial])),
                    }
                };
                if settings.occlusion {
                    e.insert(Occludable { pos: p });
                }
                e.id()
            }
            (true, None) => match reverb.0.filter(|_| settings.reverb) {
                // Jack's own sounds (gunshots, reload) still ring in the space around him
                Some(b) => commands.spawn((player, playback, sample_effects![SendNode::new(Volume::Linear(send * 0.6), b)])).id(),
                None => commands.spawn((player, playback)).id(),
            },
            (false, _) => commands.spawn((player, playback)).id(),
        }
    }

    pub fn listener(mut commands: Commands, cams: Query<Entity, Added<MainCam>>) {
        for e in &cams {
            commands.entity(e).insert(SpatialListener3D);
        }
    }

    /// Reverb bus and master volume follow the settings.
    pub fn buses(mut commands: Commands, settings: Res<AudioSettings>, mut bus: ResMut<ReverbBus>, mut main: Query<&mut VolumeNode, With<MainBus>>) {
        if !settings.is_changed() && (bus.0.is_some() || !settings.remaster() || !settings.reverb) {
            return;
        }
        let want = settings.remaster() && settings.reverb;
        match (want, bus.0) {
            (true, None) => {
                let (room, damping, _) = super::scene_reverb();
                bus.0 = Some(commands.spawn((Name::new("ReverbBus"), FreeverbNode { room_size: room, damping, width: 0.85, ..default() })).id());
            }
            (false, Some(e)) => {
                commands.entity(e).despawn();
                bus.0 = None;
            }
            _ => {}
        }
        for mut v in &mut main {
            v.volume = Volume::Linear(settings.master.clamp(0.0, 1.0));
        }
    }

    /// Muffle positional voices that the level geometry hides from the listener (8 checks a second).
    pub fn occlusion(
        time: Res<Time>,
        mut acc: Local<f32>,
        arena: Res<crate::world::Arena>,
        cam: Query<&GlobalTransform, With<MainCam>>,
        voices: Query<(&Occludable, &SampleEffects)>,
        mut spatial: Query<&mut SpatialBasicNode>,
        mut lowpass: Query<&mut FastLowpassNode<2>>,
    ) {
        *acc += time.delta_secs();
        if *acc < 0.125 {
            return;
        }
        *acc = 0.0;
        let Ok(c) = cam.single() else { return };
        let ear = c.translation();
        for (o, fx) in &voices {
            let d = o.pos - ear;
            let len = d.length();
            let blocked = len > 1.0 && arena.raycast(ear, d / len, len - 0.5).is_some();
            let cutoff = if blocked { 900.0 } else { 20_000.0 };
            for e in fx.iter() {
                if let Ok(mut s) = spatial.get_mut(e) {
                    if (s.muffle_cutoff_hz - cutoff).abs() > 1.0 {
                        s.muffle_cutoff_hz = cutoff;
                    }
                }
                if let Ok(mut l) = lowpass.get_mut(e) {
                    if (l.cutoff_hz - cutoff).abs() > 1.0 {
                        l.cutoff_hz = cutoff;
                    }
                }
            }
        }
    }
}
