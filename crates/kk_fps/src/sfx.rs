//! Original King Kong sounds, driven by the game's own sound-definition table.
//!
//! `sound_defs.json` was decoded from Sound_Common.bf: every `.smd` sound definition
//! (e.g. "Jack luger shoot", "Trex alert roar") lists the `.wav` variations it picks from and
//! its volume. The wavs are the original MS-ADPCM files converted to PCM.
//! Distance attenuation and the Tommy gun loop handling are [G].

use crate::anim::GameState;
use crate::events::{GunEvent, JackEvent, RexEvent};
use crate::player::MainCam;
use crate::spec::WEAPONS;
use bevy::prelude::*;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct SoundDef {
    pub files: Vec<String>,
    pub volume: f32,
}

#[derive(Resource, Default)]
pub struct SoundDefs(pub HashMap<String, SoundDef>);

/// Every sound event the game requested this session (time, def name). Used by the tests.
#[derive(Resource, Default)]
pub struct SfxLog(pub Vec<(f32, String)>);

/// A request to play one sound definition. `pos` = world position (None = 2D / on Jack).
#[derive(Message, Clone, Debug)]
pub struct PlaySfx {
    pub def: &'static str,
    pub pos: Option<Vec3>,
    pub gain: f32,
}

impl PlaySfx {
    pub fn at(def: &'static str, pos: Vec3) -> Self {
        Self { def, pos: Some(pos), gain: 1.0 }
    }
    pub fn ui(def: &'static str) -> Self {
        Self { def, pos: None, gain: 1.0 }
    }
}

/// sound-definition names per weapon index (Colt uses the "luger" set, as in the game files)
pub const W_SHOOT: [&str; 4] = ["Jack luger shoot", "Jack Tommygun shoot loop A", "Jack Shotgun shoot", "Jack Sniper shoot"];
pub const W_RELOAD: [&str; 4] = ["Jack luger reload", "Jack Tommygun reload", "Jack Shotgun reload", "Jack Sniper reload"];
pub const W_EMPTY: [&str; 4] = [
    "Jack luger empty trigger",
    "Jack Tommygun empty trigger",
    "Jack Shotgun empty trigger",
    "Jack Sniper empty trigger",
];
pub const W_TAKE: [&str; 4] = ["Jack luger take", "Jack Tommygun take", "Jack Shotgun take", "Jack Sniper take"];

pub struct SfxPlugin;

impl Plugin for SfxPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load_defs())
            .init_resource::<SfxLog>()
            .add_message::<PlaySfx>()
            .add_systems(
                Update,
                (map_events, play).chain().after(crate::rex::RexSet).run_if(in_state(GameState::Playing)),
            );
        #[cfg(feature = "audio_engine")]
        app.init_resource::<audio::Loaded>();
    }
}

fn load_defs() -> SoundDefs {
    let path = crate::mods::resolve("sound_defs.json");
    let mut out = HashMap::new();
    match std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        Some(v) => {
            for (k, d) in v.as_object().into_iter().flatten() {
                let files = d["files"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|f| f.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let volume = d["volume"].as_f64().unwrap_or(1.0) as f32;
                out.insert(k.clone(), SoundDef { files, volume });
            }
            info!("loaded {} original sound definitions", out.len());
        }
        None => warn!("{} not found: game sounds disabled", path.display()),
    }
    SoundDefs(out)
}

#[derive(Default)]
struct LoopState {
    tommy_firing_t: f32,
    tommy_active: bool,
    sniper_rearm_at: Option<f32>,
    shotgun_rearm_at: Option<f32>,
}

#[allow(clippy::too_many_arguments)]
fn map_events(
    time: Res<Time>,
    mut gun: MessageReader<GunEvent>,
    mut rex: MessageReader<RexEvent>,
    mut jack: MessageReader<JackEvent>,
    mut out: MessageWriter<PlaySfx>,
    mut st: Local<LoopState>,
) {
    let now = time.elapsed_secs();
    // at most one flesh and one stone ricochet per frame (a shotgun shell is 25 impacts)
    let (mut flesh_done, mut stone_done) = (false, false);
    for e in gun.read() {
        match *e {
            GunEvent::Fired { w, muzzle, .. } => {
                if w == 1 {
                    // Tommy gun: loop A while firing, "shoot end" tail when released [L]
                    st.tommy_firing_t = now;
                    if !st.tommy_active {
                        st.tommy_active = true;
                        out.write(PlaySfx::ui(W_SHOOT[1]));
                    }
                } else {
                    out.write(PlaySfx::ui(W_SHOOT[w]));
                }
                let _ = muzzle;
                // bolt / pump after the shot: animation-driven in the game, timed here [G]
                if w == 3 { st.sniper_rearm_at = Some(now + 0.35); }
                if w == 2 { st.shotgun_rearm_at = Some(now + 0.3); }
            }
            GunEvent::Impact { pos, rex, .. } => {
                let done = if rex { &mut flesh_done } else { &mut stone_done };
                if !*done {
                    *done = true;
                    out.write(PlaySfx {
                        def: if rex { "Bullet ricochet flesh" } else { "Bullet ricochet stone" },
                        pos: Some(pos),
                        gain: 0.8,
                    });
                }
            }
            GunEvent::Empty { w } => { out.write(PlaySfx::ui(W_EMPTY[w])); }
            GunEvent::ReloadStart { w, first } => {
                if w == 3 {
                    // sniper: "reload" opens the bolt once, then one "bullet reload" per round
                    if first { out.write(PlaySfx::ui(W_RELOAD[3])); }
                    out.write(PlaySfx::ui("Jack Sniper bullet reload"));
                } else {
                    out.write(PlaySfx::ui(W_RELOAD[w]));
                }
            }
            GunEvent::ReloadEnd { w } => {
                if w == 3 { out.write(PlaySfx::ui("Jack Sniper reload end")); }
                if w == 2 { out.write(PlaySfx::ui("Jack Shotgun rearm")); }
            }
            GunEvent::ReloadCommit { .. } => {}
            GunEvent::Swap { to } => { out.write(PlaySfx::ui(W_TAKE[to.min(WEAPONS.len() - 1)])); }
        }
    }
    if st.tommy_active && now - st.tommy_firing_t > WEAPONS[1].auto_interval() * 1.6 {
        st.tommy_active = false;
        out.write(PlaySfx::ui("Jack Tommygun shoot end"));
    }
    if st.sniper_rearm_at.is_some_and(|t| now >= t) {
        st.sniper_rearm_at = None;
        out.write(PlaySfx::ui("Jack Sniper rearm"));
    }
    if st.shotgun_rearm_at.is_some_and(|t| now >= t) {
        st.shotgun_rearm_at = None;
        out.write(PlaySfx::ui("Jack Shotgun rearm"));
    }
    for e in rex.read() {
        match *e {
            RexEvent::Roar { alert, pos } => {
                out.write(PlaySfx::at(if alert { "Trex alert roar" } else { "Trex_roar" }, pos));
            }
            RexEvent::Footstep { pos, .. } => {
                out.write(PlaySfx::at("Trex_footsteps_near", pos));
            }
            RexEvent::BiteStart { pos } => { out.write(PlaySfx::at("Trex_attack_jack", pos)); }
            RexEvent::BiteHit { pos } => { out.write(PlaySfx::at("Trex_bite", pos)); }
            RexEvent::Flinch { pos } => { out.write(PlaySfx::at("Trex_take_shoot", pos)); }
            RexEvent::Eat { pos } => { out.write(PlaySfx::at("Trex_eat", pos)); }
            RexEvent::Breath { pos } => { out.write(PlaySfx::at("Trex_breath", pos)); }
            RexEvent::Died { pos } => { out.write(PlaySfx::at("Trex_growl", pos)); }
        }
    }
    for e in jack.read() {
        match e {
            JackEvent::Footstep => { out.write(PlaySfx { def: "Jack footsteps stone", pos: None, gain: 0.35 }); }
            JackEvent::Wounded => { out.write(PlaySfx::ui("Jack injured")); }
            JackEvent::Died => { out.write(PlaySfx::ui("Jack body fall death")); }
        }
    }
}

/// Distance gain [G]: full volume inside 8 m, then 1/d rolloff, silent past ~150 m.
pub fn distance_gain(d: f32) -> f32 {
    if d < 8.0 { 1.0 } else { (8.0 / d).powf(0.9) }
}

fn play(
    mut ev: MessageReader<PlaySfx>,
    defs: Res<SoundDefs>,
    mut log: ResMut<SfxLog>,
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<MainCam>>,
    #[cfg(feature = "audio_engine")] mut commands: Commands,
    #[cfg(feature = "audio_engine")] mut loaded: ResMut<audio::Loaded>,
    #[cfg(feature = "audio_engine")] assets: Res<AssetServer>,
    #[cfg(feature = "audio_engine")] tommy: Query<Entity, With<audio::TommyLoop>>,
    #[cfg(feature = "audio_engine")] settings: Res<crate::audio_engine::AudioSettings>,
    #[cfg(feature = "audio_engine")] reverb: Res<crate::audio_engine::backend::ReverbBus>,
) {
    let listener = cam.single().map(|g| g.translation()).unwrap_or(Vec3::ZERO);
    for e in ev.read() {
        log.0.push((time.elapsed_secs(), e.def.to_string()));
        let Some(def) = defs.0.get(e.def) else {
            continue;
        };
        if def.files.is_empty() {
            continue;
        }
        let dist = e.pos.map(|p| p.distance(listener)).unwrap_or(0.0);
        let gain = def.volume * e.gain * distance_gain(dist);
        #[cfg(feature = "audio_engine")]
        {
            // the remaster engine attenuates positional voices itself
            let g = if settings.remaster() && e.pos.is_some() { def.volume * e.gain } else { gain };
            audio::spawn(&mut commands, &mut loaded, &assets, &tommy, &settings, &reverb, e.def, def, g, e.pos);
        }
        #[cfg(not(feature = "audio_engine"))]
        let _ = gain;
    }
}

#[cfg(feature = "audio_engine")]
mod audio {
    use super::SoundDef;
    use crate::audio_engine::{backend, AudioSettings};
    use bevy::prelude::*;
    use bevy_seedling::prelude::AudioSample;
    use rand::Rng;
    use std::collections::HashMap;

    #[derive(Resource, Default)]
    pub struct Loaded(pub HashMap<String, Handle<AudioSample>>);

    #[derive(Component)]
    pub struct TommyLoop;

    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        commands: &mut Commands,
        loaded: &mut Loaded,
        assets: &AssetServer,
        tommy: &Query<Entity, With<TommyLoop>>,
        settings: &AudioSettings,
        reverb: &backend::ReverbBus,
        name: &str,
        def: &SoundDef,
        gain: f32,
        pos: Option<Vec3>,
    ) {
        let file = &def.files[rand::thread_rng().gen_range(0..def.files.len())];
        let h = loaded.0.entry(file.clone()).or_insert_with(|| assets.load(file.clone())).clone();
        // pitch variation of ±3% so repeated shots do not sound identical [G]
        let speed = rand::thread_rng().gen_range(0.97..1.03);
        if name == "Jack Tommygun shoot loop A" {
            let e = backend::spawn_voice(commands, settings, reverb, h, gain, 1.0, None, true);
            commands.entity(e).insert(TommyLoop);
            return;
        }
        if name == "Jack Tommygun shoot end" {
            for e in tommy.iter() {
                commands.entity(e).despawn();
            }
        }
        backend::spawn_voice(commands, settings, reverb, h, gain, speed, pos, false);
    }
}
