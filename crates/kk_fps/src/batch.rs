//! Mechanic test batches: `KK_BATCH=<name> KK_BATCH_OUT=<dir>` stages one scenario, drives
//! inputs on a fixed 30 Hz clock, checks recovered mechanics numerically, captures clean
//! (HUD-less) frames for visual review against original-game reference screenshots, writes
//! `<dir>/<name>.json`, then exits.

use crate::anim::GameState;
use crate::events::{GunEvent, RexEvent};
use crate::fx::{CameraShake, FxStats, RumbleLog};
use crate::player::{MainCam, Player};
use crate::rex::{Rex, RexState};
use crate::sfx::SfxLog;
use crate::spec::*;
use crate::weapons::Arsenal;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub enum Act {
    /// place Rex `dist` m in front of Jack, `side` m to the right, facing Jack
    PlaceRex { dist: f32, side: f32 },
    RexState(RexForce),
    Weapon(usize),
    Click,
    Hold(bool),
    Aim(bool),
    Reload,
    Pitch(f32),
    /// keep the camera on the V-Rex body (height above its root) every frame while on
    Track(Option<f32>),
    /// keep the camera on the V-Rex head bone, offset down by the value (metres)
    TrackHead(f32),
    Shot(&'static str),
    Mark(&'static str),
    /// Kong batches: press a key for one frame / hold or release it (Tab, WASD, Space, Q, E)
    Key(KeyCode),
    KeyHold(KeyCode, bool),
    /// Kong batches: stand Jack at world (x, z), on the ground
    PlaceJack { x: f32, z: f32 },
    /// any batch: stand Jack at world (x, z) on the ground, facing `yaw`
    JackAt { x: f32, z: f32, yaw: f32 },
    /// smash a breakable (`world::BREAKABLES` key) as a Kong blow would
    Break(&'static str),
    /// Kong batches: set the V-Rex's life (0 knocks it down: the finisher window)
    RexLife(f32),
    /// Kong batches: record Kong's world position under a name
    KongMark(&'static str),
    End,
}

#[derive(Clone, Copy, Debug)]
pub enum RexForce {
    Idle,
    /// scripted hold: idle and deaf/blind (weapon-only batches; the AI would otherwise notice Jack)
    Hold,
    Roar,
    Chase,
}

pub struct Batch {
    pub name: &'static str,
    pub reference: &'static str,
    pub about: &'static str,
    pub script: &'static [(f32, Act)],
}

pub const BATCHES: &[Batch] = &[
    Batch {
        name: "b1_colt_vrex_roar",
        reference: "ref_luger_vrex.jpg",
        about: "Colt at the hip, V-Rex roaring ~18 m ahead; semi-auto cadence, mid-band damage, roar sound/shake/breath, reload",
        script: &[
            (0.0, Act::Weapon(0)),
            (0.1, Act::PlaceRex { dist: 10.0, side: 3.0 }),
            (0.2, Act::RexState(RexForce::Idle)),
            (0.3, Act::TrackHead(1.8)),
            (1.0, Act::RexState(RexForce::Roar)),
            (2.2, Act::Shot("roar")),
            (2.6, Act::Track(Some(3.4))),
            (3.0, Act::Mark("fire_start")),
            (3.0, Act::Click), (3.1, Act::Click), (3.2, Act::Click), (3.3, Act::Click), (3.4, Act::Click),
            (3.5, Act::Click), (3.6, Act::Click), (3.7, Act::Click), (3.8, Act::Click),
            // requested on the shot frame: the readback lands 1-2 frames later, inside the 0.1 s flash
            (3.8, Act::Shot("fire")),
            (3.9, Act::Click),
            (4.0, Act::Mark("fire_end")),
            (4.2, Act::TrackHead(3.0)),
            (4.6, Act::Reload),
            (5.3, Act::Shot("reload")),
            (7.0, Act::End),
        ],
    },
    Batch {
        name: "b2_tommy_vrex_charge",
        reference: "ref_thompson_vrex.jpg",
        about: "Tommy gun burst at a charging V-Rex from 35 m; 10 rounds/s, far-band damage, muzzle light, loop/end sounds, footstep shake",
        script: &[
            (0.0, Act::Weapon(1)),
            (0.1, Act::PlaceRex { dist: 38.0, side: -2.0 }),
            (0.2, Act::RexState(RexForce::Chase)),
            (0.3, Act::Pitch(0.18)),
            (1.8, Act::Track(Some(3.4))),
            (2.0, Act::Mark("fire_start")),
            (2.0, Act::Hold(true)),
            (2.55, Act::Shot("burst")),
            (3.0, Act::Hold(false)),
            (3.0, Act::Mark("fire_end")),
            (3.6, Act::Shot("approach")),
            (5.0, Act::End),
        ],
    },
    Batch {
        name: "b3_shotgun_close",
        reference: "ref_trenchgun.jpg",
        about: "Shotgun at a V-Rex 7 m away; 25 pellets, near/mid damage bands, 0.5 s cooldown, shell-by-shell reload, sparks + smoke",
        script: &[
            (0.0, Act::Weapon(2)),
            (0.1, Act::PlaceRex { dist: 11.0, side: 1.5 }),
            (0.2, Act::RexState(RexForce::Idle)),
            (0.3, Act::TrackHead(2.2)),
            (1.5, Act::Shot("hip")),
            (2.0, Act::Mark("fire_start")),
            (2.0, Act::Click),
            (2.3, Act::Click),
            (2.6, Act::Click),
            (2.6, Act::Shot("fire")),
            (2.7, Act::Mark("fire_end")),
            (3.4, Act::Reload),
            (6.5, Act::End),
        ],
    },
    Batch {
        name: "b4_sniper_scope",
        reference: "ref_sniper.jpg",
        about: "Sniper rifle at a V-Rex 60 m away; hip view, scope FOV 0.3, far-band damage, 0.4 s bolt, rumble 100",
        script: &[
            (0.0, Act::Weapon(3)),
            (0.1, Act::PlaceRex { dist: 34.0, side: 3.0 }),
            (0.2, Act::RexState(RexForce::Hold)),
            (0.3, Act::Pitch(0.04)),
            (1.6, Act::Shot("hip")),
            (1.8, Act::TrackHead(1.2)),
            (2.0, Act::Aim(true)),
            (3.2, Act::Shot("scope")),
            (3.3, Act::Mark("fire_start")),
            (3.3, Act::Click),
            (3.5, Act::Click),
            (3.8, Act::Click),
            (3.9, Act::Mark("fire_end")),
            (4.2, Act::Aim(false)),
            (5.0, Act::End),
        ],
    },
    Batch {
        name: "b5_reload",
        reference: "ref_reload.jpg",
        about: "Sniper reload animation mid-way; reload commit transfers rounds from reserve",
        script: &[
            (0.0, Act::Weapon(3)),
            (0.1, Act::PlaceRex { dist: 45.0, side: -6.0 }),
            (0.2, Act::RexState(RexForce::Hold)),
            (0.3, Act::Pitch(0.15)),
            (1.0, Act::Click),
            (1.5, Act::Click),
            (2.0, Act::Mark("reload_start")),
            (2.0, Act::Reload),
            (2.3, Act::Shot("reload")),
            (4.5, Act::End),
        ],
    },
    Batch {
        name: "b6_rex_hunt",
        reference: "ref_luger_vrex.jpg",
        about: "V-Rex AI from 30 m: notice, hesite+roar, chase at 1.5 m/s^2 toward 14 m/s, bite inside 3.8 m / 40 deg cone wounds, second bite grabs and kills Jack",
        script: &[
            (0.0, Act::Weapon(0)),
            (0.1, Act::PlaceRex { dist: 30.0, side: 0.0 }),
            (0.2, Act::RexState(RexForce::Idle)),
            (0.3, Act::TrackHead(1.5)),
            (7.0, Act::Shot("chase")),
            (10.0, Act::Shot("close")),
            (10.9, Act::Shot("bite")),
            (14.5, Act::End),
        ],
    },
    Batch {
        name: "b7_kong_fight",
        reference: "kkassets/ref/video/s_0xx.jpg",
        about: "Kong AI vs the V-Rex with Jack watching: punch chain, repel, downward, side-step + counter lunge, grab/strike/throw, chest pound + fury, KO, jaw-break finisher, victory pound and roar; screenshots fire on the fight's own events",
        script: &[
            (0.0, Act::PlaceJack { x: f32::NAN, z: 0.0 }),
            (0.1, Act::Mark("start")),
            // the fight's own events take the screenshots (kong.rs SHOT_PLAN); the batch ends when the fight is over
            (330.0, Act::End),
        ],
    },
    Batch {
        name: "b8_kong_player",
        reference: "kkassets/ref/video/s_0xx.jpg",
        about: "Jack switches to Kong with Tab at t=1 s: Jack hidden, third-person camera, scripted pad (run, punch, dodge, special/repel) plays Kong clips by action id; Tab again restores Jack and his camera",
        script: &[
            (0.0, Act::PlaceJack { x: f32::NAN, z: 0.0 }),
            (0.1, Act::Mark("start")),
            (0.6, Act::Shot("jack_watching")),
            (1.0, Act::Key(KeyCode::Tab)),
            (1.1, Act::Mark("kong")),
            (2.6, Act::Shot("kong_cam")),
            (3.0, Act::KeyHold(KeyCode::KeyW, true)),
            (4.2, Act::Shot("kong_run")),
            (5.4, Act::KeyHold(KeyCode::KeyW, false)),
            (5.5, Act::Click),
            (5.7, Act::Shot("kong_punch")),
            (6.4, Act::Click),
            (7.3, Act::Key(KeyCode::Space)),
            (7.6, Act::Shot("kong_dodge")),
            (8.8, Act::Key(KeyCode::KeyQ)),
            (9.8, Act::Shot("kong_pound")),
            (11.8, Act::Shot("kong_after")),
            (13.0, Act::Key(KeyCode::Tab)),
            (13.4, Act::Shot("jack_again")),
            (14.2, Act::End),
        ],
    },
    Batch {
        name: "b13_kong_roam",
        reference: "k_ETAT_main after k_ETAT_finish (player Kong)",
        about: "The player takes Kong (Tab), the rex is knocked down (life 0), Kong walks in and mashes the jaw-break finisher; after the victory pound and roar the player walks Kong away: he must not stay in the victory clip",
        script: &[
            (0.0, Act::PlaceJack { x: f32::NAN, z: 0.0 }),
            (0.1, Act::Mark("start")),
            (1.0, Act::Key(KeyCode::Tab)),
            (1.4, Act::RexLife(0.0)),
            (1.6, Act::KeyHold(KeyCode::KeyW, true)),
            (3.3, Act::KeyHold(KeyCode::KeyW, false)),
            (3.6, Act::Click), (3.8, Act::Click), (3.9, Act::Click), (4.0, Act::Click), (4.2, Act::Click), (4.3, Act::Click), (4.5, Act::Click), (4.7, Act::Click), (4.8, Act::Click), (5.0, Act::Click), (5.1, Act::Click), (5.2, Act::Click), (5.4, Act::Click), (5.5, Act::Click), (5.7, Act::Click), (5.8, Act::Click), (6.0, Act::Click), (6.2, Act::Click), (6.3, Act::Click), (6.5, Act::Click), (6.6, Act::Click), (6.8, Act::Click), (6.9, Act::Click), (7.0, Act::Click), (7.2, Act::Click), (7.3, Act::Click), (7.5, Act::Click), (7.7, Act::Click), (7.8, Act::Click), (8.0, Act::Click), (8.1, Act::Click), (8.2, Act::Click), (8.4, Act::Click), (8.6, Act::Click), (8.7, Act::Click), (8.8, Act::Click), (9.0, Act::Click), (9.2, Act::Click), (9.3, Act::Click), (9.4, Act::Click), (9.6, Act::Click), (9.8, Act::Click), (9.9, Act::Click), (1e+01, Act::Click), (1e+01, Act::Click), (1e+01, Act::Click), (1e+01, Act::Click), (1.1e+01, Act::Click), (1.1e+01, Act::Click), (1.1e+01, Act::Click), (1.1e+01, Act::Click), (1.1e+01, Act::Click), (1.1e+01, Act::Click), (1.2e+01, Act::Click), (1.2e+01, Act::Click), (1.2e+01, Act::Click), (1.2e+01, Act::Click), (1.2e+01, Act::Click), (1.2e+01, Act::Click), (1.2e+01, Act::Click), (1.3e+01, Act::Click), (1.3e+01, Act::Click), (1.3e+01, Act::Click), (1.3e+01, Act::Click), (1.3e+01, Act::Click), (1.3e+01, Act::Click), (1.4e+01, Act::Click), (1.4e+01, Act::Click), (1.4e+01, Act::Click), (1.4e+01, Act::Click), (1.4e+01, Act::Click), (1.4e+01, Act::Click), (1.4e+01, Act::Click), (1.5e+01, Act::Click), (1.5e+01, Act::Click), (1.5e+01, Act::Click),
            (9.0, Act::Shot("finisher")),
            (23.0, Act::Shot("victory_over")),
            (23.2, Act::KongMark("roam0")),
            (23.3, Act::KeyHold(KeyCode::KeyS, true)),
            (26.0, Act::KeyHold(KeyCode::KeyS, false)),
            (26.1, Act::KongMark("roam1")),
            (26.2, Act::Shot("roam")),
            (27.0, Act::End),
        ],
    },
    Batch {
        name: "b10_swamp_fight",
        reference: "kkassets/ref/video/s_0xx.jpg",
        about: "Kong vs the V-Rex in the flooded 07D swamp (KK_SCENE=swamp07d): fog, rain, mist, water surface, big splashes at feet / hits / falls, Ann at the edge, cinematic low third-person camera with close-ups for the chest pound, roar and the jaw-break finisher; a frame every 3 s over the whole fight plus the key moments",
        script: &[
            (0.0, Act::PlaceJack { x: f32::NAN, z: 0.0 }),
            (0.1, Act::Mark("start")),
            (200.0, Act::End),
        ],
    },
    Batch {
        name: "b11_gate_walk",
        reference: "level03e gate LD_03E_ODE_block01..05",
        about: "Jack walks into the courtyard gate (blocked by the intact ODE blocks and the façade), the gate is smashed (the same break a Kong blow triggers), Jack walks through it; plus a walk up the gate's stone steps (DEC_C_EscalierPorte) and along a ruin wall (mesh wall faces)",
        script: &[
            (0.0, Act::JackAt { x: 48.0, z: -93.5, yaw: 0.0 }),
            (0.2, Act::RexState(RexForce::Hold)),
            (0.6, Act::Shot("gate_closed")),
            (0.8, Act::Mark("walk1")),
            (0.8, Act::KeyHold(KeyCode::KeyW, true)),
            (4.0, Act::KeyHold(KeyCode::KeyW, false)),
            (4.1, Act::Mark("blocked")),
            (4.2, Act::Break("porte")),
            (4.6, Act::Shot("gate_smash")),
            (6.5, Act::Shot("gate_broken")),
            (6.6, Act::Mark("walk2")),
            (6.6, Act::KeyHold(KeyCode::KeyW, true)),
            (9.8, Act::KeyHold(KeyCode::KeyW, false)),
            (9.9, Act::Mark("through")),
            (10.0, Act::Shot("through")),
            // stairs: from the courtyard floor up the steps in front of the gate (DEC_C_EscalierPorte, 36.3, -99.8)
            (10.2, Act::JackAt { x: 36.3, z: -92.8, yaw: 0.0 }),
            (10.4, Act::Mark("stairs0")),
            (10.4, Act::KeyHold(KeyCode::KeyW, true)),
            (12.6, Act::KeyHold(KeyCode::KeyW, false)),
            (12.7, Act::Mark("stairs1")),
            (12.8, Act::Shot("stairs")),
            // a ruin wall: walk west into murcass01_A (x 10..31 at z -100) from the courtyard
            (13.0, Act::JackAt { x: 33.5, z: -100.0, yaw: std::f32::consts::FRAC_PI_2 }),
            (13.2, Act::Mark("wall0")),
            (13.2, Act::KeyHold(KeyCode::KeyW, true)),
            (16.2, Act::KeyHold(KeyCode::KeyW, false)),
            (16.3, Act::Mark("wall1")),
            (16.4, Act::Shot("wall")),
            // the corridor wall before CHK03 (intact) and the entrance lintel, for a look
            (16.6, Act::JackAt { x: 35.5, z: -130.0, yaw: 0.0 }),
            (17.0, Act::Shot("couloir_wall")),
            (17.2, Act::JackAt { x: 35.0, z: -73.0, yaw: std::f32::consts::PI }),
            (17.6, Act::Pitch(0.35)),
            (17.8, Act::Shot("entree_lintel")),
            (18.0, Act::Pitch(0.0)),
            (18.2, Act::End),
        ],
    },
    Batch {
        name: "b12_slice_spears",
        reference: "level03e racks PFB_C_RackLanceSkel(01), bone pile DEC_C_OssementSquelette_01",
        about: "Jack picks a spear from the rack (E), aims and throws it at a raptor (run with KK_RAPTOR_AT=41.8,-78), picks a bone from the bone pile, stabs, drops it (G); the spears stick, wound and bleed the raptor",
        script: &[
            (0.0, Act::JackAt { x: 39.3, z: -86.6, yaw: std::f32::consts::PI }),
            (0.2, Act::RexState(RexForce::Hold)),
            (0.6, Act::Shot("rack")),
            (0.8, Act::Key(KeyCode::KeyE)),
            (1.2, Act::Mark("picked")),
            (1.3, Act::Shot("holding")),
            (1.4, Act::JackAt { x: 41.8, z: -86.6, yaw: std::f32::consts::PI }),
            (1.6, Act::Pitch(0.05)),
            (1.8, Act::Aim(true)),
            (2.6, Act::Click),
            (2.7, Act::Mark("thrown")),
            (2.75, Act::Shot("throw")),
            (3.6, Act::Shot("stuck")),
            (3.8, Act::Aim(false)),
            (4.0, Act::JackAt { x: 56.3, z: -86.6, yaw: std::f32::consts::PI }),
            (4.3, Act::Key(KeyCode::KeyE)),
            (4.7, Act::Mark("bone")),
            (4.8, Act::Shot("bone")),
            (5.0, Act::Click),
            (5.05, Act::Shot("stab")),
            (5.6, Act::Key(KeyCode::KeyG)),
            (6.0, Act::Mark("dropped")),
            (6.2, Act::Shot("dropped")),
            (6.5, Act::End),
        ],
    },
    Batch {
        name: "b9_kong_cinema",
        reference: "kkassets/ref/video/s_0xx.jpg",
        about: "Same AI fight as b7, filmed with Kong's third-person camera (the reference clip's view): splashes, shake, close-ups",
        script: &[
            (0.0, Act::PlaceJack { x: f32::NAN, z: 0.0 }),
            (0.1, Act::Mark("start")),
            (330.0, Act::End),
        ],
    },
];

#[derive(Resource)]
struct Runner {
    batch: &'static Batch,
    out: PathBuf,
    t: f32,
    step: usize,
    started: bool,
    marks: Vec<(&'static str, f32)>,
    shots: Vec<String>,
    hold: bool,
    aim: bool,
    track: Option<f32>,
    track_head: Option<f32>,
    // logs
    fired: Vec<(f32, usize)>,
    impacts: Vec<(f32, bool, f32, f32)>,
    commits: Vec<(f32, u32)>,
    rex_events: Vec<(f32, String)>,
    rex_states: Vec<(f32, String)>,
    jack_dead_at: Option<f32>,
    /// first time Jack's wound status left "healthy" (the 0x4104 bite wound)
    jack_wounded_at: Option<f32>,
    jack_death_cause: &'static str,
    /// (t, rex speed) while the rex is in its Chase state
    chase_trace: Vec<(f32, f32)>,
    rex_hp_min: f32,
    min_rex_head_dist: f32,
    /// (t, offset magnitude, freq_v of the running preset: 50 footstep/bite, 30 roar record)
    shake_samples: Vec<(f32, f32, f32)>,
    fov_min: f32,
    mag_trace: Vec<(f32, u32, u32)>,
    /// number of sound events logged before the batch clock started (excluded from counts)
    sfx_skip: Option<usize>,
    held_keys: Vec<KeyCode>,
    done: bool,
    /// Jack's position at every mark
    mark_pos: Vec<(&'static str, Vec3)>,
}

/// Hides every HUD node (original game has no HUD/crosshair during play).
#[derive(Resource)]
pub struct CleanHud(pub bool);

pub struct BatchPlugin;

impl Plugin for BatchPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CleanHud(false));
        let Ok(name) = std::env::var("KK_BATCH") else { return };
        // t1..t5: test-area batches live in tbatch.rs
        if crate::testarea::is_test_batch(&name) {
            return;
        }
        let Some(batch) = BATCHES.iter().find(|b| b.name == name || b.name.starts_with(&name)) else {
            eprintln!("unknown batch {name}; available: {:?}", BATCHES.iter().map(|b| b.name).collect::<Vec<_>>());
            std::process::exit(2);
        };
        let out = PathBuf::from(std::env::var("KK_BATCH_OUT").unwrap_or_else(|_| "batch_out".into()));
        std::fs::create_dir_all(&out).ok();
        app.insert_resource(CleanHud(true))
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / 30.0)))
            .insert_resource(Runner {
                batch,
                out,
                t: -1.5,
                step: 0,
                started: false,
                marks: vec![],
                shots: vec![],
                hold: false,
                aim: false,
                track: None,
                track_head: None,
                fired: vec![],
                impacts: vec![],
                commits: vec![],
                rex_events: vec![],
                rex_states: vec![],
                jack_dead_at: None,
                jack_wounded_at: None,
                jack_death_cause: "",
                chase_trace: vec![],
                rex_hp_min: f32::MAX,
                min_rex_head_dist: f32::MAX,
                shake_samples: vec![],
                fov_min: f32::MAX,
                mag_trace: vec![],
                sfx_skip: None,
                held_keys: vec![],
                done: false,
                mark_pos: vec![],
            })
            .add_systems(PreUpdate, drive.after(bevy::input::InputSystem).run_if(in_state(GameState::Playing)))
            .add_systems(Update, record.after(crate::rex::RexSet).run_if(in_state(GameState::Playing)));
    }
}

#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    time: Res<Time>,
    mut r: ResMut<Runner>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut players: Query<(&mut Player, &mut Transform), Without<Rex>>,
    mut rex: Query<(&mut Rex, &mut Transform), Without<Player>>,
    mut arsenal: ResMut<Arsenal>,
    arena: Res<crate::world::Arena>,
    (mut exit, mut breaks): (EventWriter<AppExit>, EventWriter<crate::breakable::BreakRequest>),
    bones: Query<&crate::rex::RexBones>,
    gts: Query<&GlobalTransform>,
    mut kong: Option<ResMut<crate::kong::KongCtl>>,
) {
    r.t += time.delta_secs();
    // release one-frame presses from the previous frame
    for k in [KeyCode::KeyR, KeyCode::Tab, KeyCode::Space, KeyCode::KeyQ, KeyCode::KeyE, KeyCode::KeyG] {
        if keys.pressed(k) && !keys.just_pressed(k) && !r.held_keys.contains(&k) {
            keys.release(k);
        }
    }
    if !r.hold && mouse.pressed(MouseButton::Left) && !mouse.just_pressed(MouseButton::Left) {
        mouse.release(MouseButton::Left);
    }
    if r.hold {
        mouse.press(MouseButton::Left);
    }
    if r.aim {
        mouse.press(MouseButton::Right);
    } else if mouse.pressed(MouseButton::Right) {
        mouse.release(MouseButton::Right);
    }
    if r.t < 0.0 {
        return;
    }
    r.started = true;
    let script = r.batch.script;
    while r.step < script.len() && script[r.step].0 <= r.t {
        let act = script[r.step].1;
        r.step += 1;
        match act {
            Act::PlaceRex { dist, side } => {
                if let (Ok((p, ptf)), Ok((mut rx, mut rtf))) = (players.single(), rex.single_mut()) {
                    let fwd = Vec3::new(-p.yaw.sin(), 0.0, -p.yaw.cos());
                    let right = Vec3::new(-fwd.z, 0.0, fwd.x);
                    let mut pos = ptf.translation + fwd * dist + right * side;
                    pos.y = arena.ground_at(pos + Vec3::Y * 30.0).unwrap_or(ptf.translation.y);
                    rtf.translation = pos;
                    let to = ptf.translation - pos;
                    rx.yaw = to.x.atan2(to.z);
                    rtf.rotation = Quat::from_rotation_y(rx.yaw);
                }
            }
            Act::RexState(f) => {
                if let Ok((mut rx, _)) = rex.single_mut() {
                    match f {
                        RexForce::Idle => {
                            rx.state = RexState::Idle;
                            rx.noticed = false;
                            rx.scripted_hold = false;
                        }
                        RexForce::Hold => {
                            rx.state = RexState::Idle;
                            rx.noticed = false;
                            rx.scripted_hold = true;
                        }
                        RexForce::Roar => {
                            rx.scripted_hold = false;
                            rx.state = RexState::Roar { t: 0.0 };
                            rx.noticed = true;
                        }
                        RexForce::Chase => {
                            rx.scripted_hold = false;
                            rx.state = RexState::Chase;
                            rx.noticed = true;
                            // scaffold: already part-way up the 1.5 m/s^2 ramp (trot_b root speed)
                            rx.speed = 7.9;
                        }
                    }
                }
            }
            Act::Weapon(i) => {
                arsenal.index = i;
                arsenal.wanted_weapon = Some(i);
                arsenal.action = crate::weapons::ArmsAction::Idle;
            }
            Act::Click => mouse.press(MouseButton::Left),
            Act::Hold(h) => {
                r.hold = h;
                if !h {
                    mouse.release(MouseButton::Left);
                }
            }
            Act::Aim(a) => r.aim = a,
            Act::Reload => keys.press(KeyCode::KeyR),
            Act::Pitch(pt) => {
                if let Ok((mut p, _)) = players.single_mut() {
                    p.pitch = pt;
                }
            }
            Act::Track(h) => {
                r.track = h;
                r.track_head = None;
            }
            Act::TrackHead(d) => {
                r.track_head = Some(d);
                r.track = None;
            }
            Act::Shot(name) => {
                let file = format!("{}_{}.png", r.batch.name, name);
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(r.out.join(&file)));
                r.shots.push(file);
            }
            Act::Mark(m) => {
                let t = r.t;
                r.marks.push((m, t));
                if let Ok((_, tf)) = players.single() {
                    r.mark_pos.push((m, tf.translation));
                }
            }
            Act::JackAt { x, z, yaw } => {
                if let Ok((mut p, mut tf)) = players.single_mut() {
                    let y = arena.ground_at(Vec3::new(x, tf.translation.y + 30.0, z)).unwrap_or(tf.translation.y);
                    tf.translation = arena.settle(Vec3::new(x, y, z), 0.35);
                    p.yaw = yaw;
                    p.vel = Vec3::ZERO;
                }
            }
            Act::Break(key) => {
                breaks.write(crate::breakable::BreakRequest(key));
            }
            Act::Key(k) => keys.press(k),
            Act::KeyHold(k, down) => {
                if down {
                    if !r.held_keys.contains(&k) {
                        r.held_keys.push(k);
                    }
                    keys.press(k);
                } else {
                    r.held_keys.retain(|x| *x != k);
                    keys.release(k);
                }
            }
            Act::RexLife(l) => {
                if let Some(k) = kong.as_deref_mut() {
                    k.fight.rex.machine.life.cur = l;
                }
            }
            Act::KongMark(m) => {
                if let Some(k) = kong.as_deref_mut() {
                    let w = k.kong_world();
                    let p = Vec3::new(w.x, k.kong_y, w.z);
                    k.kong_marks.push((m, p));
                }
            }
            Act::PlaceJack { x, z } => {
                if let Some(k) = kong.as_deref_mut() {
                    k.pending_jack = Some((x, z));
                }
            }
            Act::End => {
                r.done = true;
            }
        }
    }
    if let Some(off) = r.track_head {
        let head = bones.single().ok().and_then(|b| gts.get(b.head).ok()).map(|g| g.translation());
        if let (Ok((mut p, ptf)), Some(head)) = (players.single_mut(), head) {
            let eye = ptf.translation + Vec3::Y * p.eye;
            let to = head - Vec3::Y * off - eye;
            p.yaw = (-to.x).atan2(-to.z);
            p.pitch = (to.y / Vec2::new(to.x, to.z).length()).atan();
        }
    }
    if let Some(h) = r.track {
        if let (Ok((mut p, ptf)), Ok((_, rtf))) = (players.single_mut(), rex.single()) {
            let eye = ptf.translation + Vec3::Y * p.eye;
            let to = rtf.translation + Vec3::Y * h - eye;
            p.yaw = (-to.x).atan2(-to.z);
            p.pitch = (to.y / Vec2::new(to.x, to.z).length()).atan();
        }
    }
    // Kong fight batches end themselves a few seconds after the fight is over
    let kong_over = r.batch.name.contains("kong_fight") || r.batch.name.contains("kong_cinema") || r.batch.name.contains("swamp_fight");
    if kong_over && kong.as_deref().is_some_and(|k| k.over_at.is_some_and(|t| k.t > t + 3.5 && (!k.post_busy || k.t > t + 25.0))) {
        r.done = true;
    }
    if std::env::var("KK_BATCH_MAXT").ok().and_then(|v| v.parse::<f32>().ok()).is_some_and(|m| r.t > m) {
        r.done = true;
    }
    if r.done {
        // let the last screenshot flush one more frame before exiting
        if r.t > script.last().map(|s| s.0).unwrap_or(0.0) + 0.2 || ((kong_over || std::env::var("KK_BATCH_MAXT").is_ok()) && kong.is_some()) {
            exit.write(AppExit::Success);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn record(
    mut r: ResMut<Runner>,
    mut gun: EventReader<GunEvent>,
    mut rex_ev: EventReader<RexEvent>,
    rex: Query<&Rex>,
    players: Query<(&Player, &Transform)>,
    bones: Query<&crate::rex::RexBones>,
    gts: Query<&GlobalTransform>,
    shake: Res<CameraShake>,
    arsenal: Res<Arsenal>,
    cam: Query<&Projection, With<MainCam>>,
    sfx: Res<SfxLog>,
    rumble: Res<RumbleLog>,
    stats: Res<FxStats>,
    kong: Option<Res<crate::kong::KongCtl>>,
    breakables: Option<Res<crate::breakable::Breakables>>,
    spears: Option<Res<crate::spears::SpearKit>>,
) {
    if !r.started {
        gun.clear();
        rex_ev.clear();
        return;
    }
    if r.sfx_skip.is_none() {
        r.sfx_skip = Some(sfx.0.len());
    }
    let t = r.t;
    for e in gun.read() {
        match *e {
            GunEvent::Fired { w, .. } => r.fired.push((t, w)),
            GunEvent::Impact { rex, damage, dist, .. } => r.impacts.push((t, rex, damage, dist)),
            GunEvent::ReloadCommit { rounds, .. } => r.commits.push((t, rounds)),
            _ => {}
        }
    }
    for e in rex_ev.read() {
        let s = format!("{e:?}");
        let name = s.split([' ', '{']).next().unwrap_or("").to_string();
        r.rex_events.push((t, name));
    }
    if let Ok(rx) = rex.single() {
        let lbl = rx.label().to_string();
        if r.rex_states.last().map(|s| &s.1) != Some(&lbl) {
            r.rex_states.push((t, lbl));
        }
        r.rex_hp_min = r.rex_hp_min.min(rx.hp);
        if matches!(rx.state, RexState::Chase) {
            r.chase_trace.push((t, rx.speed));
        }
    }
    if let Ok((p, ptf)) = players.single() {
        if p.wounds.state != kk_mechanics::wounds::WoundState::Healthy && r.jack_wounded_at.is_none() {
            r.jack_wounded_at = Some(t);
        }
        if !p.alive() && r.jack_dead_at.is_none() {
            r.jack_dead_at = Some(t);
            r.jack_death_cause = p.death_cause;
        }
        if let Some(h) = bones.single().ok().and_then(|b| gts.get(b.head).ok()) {
            let d = h.translation().distance(ptf.translation + Vec3::Y * p.eye);
            r.min_rex_head_dist = r.min_rex_head_dist.min(d);
        }
    }
    if shake.enabled {
        let a = shake.last.length();
        r.shake_samples.push((t, a, shake.last_freq));
    }
    if let Ok(Projection::Perspective(pp)) = cam.single() {
        let h = 2.0 * ((pp.fov * 0.5).tan() / 0.75).atan();
        r.fov_min = r.fov_min.min(h);
    }
    let i = arsenal.index;
    let entry = (t, arsenal.mag[i], arsenal.reserve[i]);
    if r.mag_trace.last().map(|m| (m.1, m.2)) != Some((entry.1, entry.2)) {
        r.mag_trace.push(entry);
    }
    if r.done {
        write_report(&r, &sfx, &rumble, &stats, &shake, &arsenal, kong.as_deref(), breakables.as_deref(), spears.as_deref());
    }
}

fn check(name: &str, expected: Value, actual: Value, pass: bool) -> Value {
    json!({ "check": name, "expected": expected, "actual": actual, "pass": pass })
}

fn approx(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

#[allow(clippy::too_many_arguments)]
fn write_report(r: &Runner, sfx: &SfxLog, rumble: &RumbleLog, stats: &FxStats, shake: &CameraShake, arsenal: &Arsenal, kong: Option<&crate::kong::KongCtl>, breakables: Option<&crate::breakable::Breakables>, spears: Option<&crate::spears::SpearKit>) {
    let path = r.out.join(format!("{}.json", r.batch.name));
    if path.exists() {
        return;
    }
    let mark = |m: &str| r.marks.iter().find(|x| x.0 == m).map(|x| x.1);
    let in_window = |t: f32| match (mark("fire_start"), mark("fire_end")) {
        (Some(a), Some(b)) => t >= a - 0.01 && t <= b + 0.01,
        _ => true,
    };
    let skip = r.sfx_skip.unwrap_or(0).min(sfx.0.len());
    let sounds: Vec<&str> = sfx.0[skip..].iter().map(|s| s.1.as_str()).collect();
    let count = |n: &str| sounds.iter().filter(|s| **s == n).count();
    let shots: Vec<&(f32, usize)> = r.fired.iter().filter(|f| in_window(f.0)).collect();
    let mut checks = vec![];
    let rex_hits: Vec<&(f32, bool, f32, f32)> = r.impacts.iter().filter(|i| i.1).collect();
    let band = |w: &WeaponDef, d: f32| damage_at(w, d);
    match r.batch.name {
        "b1_colt_vrex_roar" => {
            let w = &WEAPONS[0];
            // 10 clicks 0.1 s apart against the 0.4 s stored shot timer (G02) -> every 4th click fires
            checks.push(check("Colt semi-auto cadence: 10 clicks @0.1 s vs shot timer 0.4 s (G02)", json!("3"), json!(shots.len()), shots.len() == 3));
            let gaps: Vec<f32> = shots.windows(2).map(|p| p[1].0 - p[0].0).collect();
            let min_gap = gaps.iter().cloned().fold(f32::MAX, f32::min);
            checks.push(check("min interval between Colt shots >= 0.4 s (G02)", json!(0.4), json!(min_gap), min_gap >= 0.39));
            let ok_dmg = !rex_hits.is_empty() && rex_hits.iter().all(|h| h.2 == band(w, h.3));
            checks.push(check("Colt damage band vs Jack-rex object distance (R1 5 / R2 25 -> 8/4/2, int) (G02)", json!(rex_hits.iter().map(|h| band(w, h.3)).collect::<Vec<_>>()), json!(rex_hits.iter().map(|h| h.2).collect::<Vec<_>>()), ok_dmg));
            let refill = r.commits.iter().map(|c| c.1).sum::<u32>();
            checks.push(check("reload transfers min(clip-mag, reserve) (G03)", json!(shots.len()), json!(refill), refill as usize == shots.len()));
            checks.push(check("magazine full after reload (G03)", json!(w.clip()), json!(arsenal.mag[0]), arsenal.mag[0] == w.clip()));
            checks.push(check("guns never reduce the rex's hp (X04: no hp change for species 0x10)", json!(REX_HP), json!(r.rex_hp_min), r.rex_hp_min == REX_HP && !rex_hits.is_empty()));
            // the V-Rex notices Jack (dist < 64 m) and roars on its own, possibly before the
            // batch clock starts: count over the whole session
            let all_roars = sfx.0.iter().filter(|s| s.1 == "Trex alert roar").count();
            checks.push(check("roar plays original 'Trex alert roar'", json!(">=1"), json!(all_roars), all_roars >= 1));
            checks.push(check("each Colt shot plays 'Jack luger shoot'", json!(r.fired.len()), json!(count("Jack luger shoot")), count("Jack luger shoot") == r.fired.len()));
            checks.push(check("reload plays 'Jack luger reload'", json!(1), json!(count("Jack luger reload")), count("Jack luger reload") == 1));
            let d = 18.0_f32.hypot(2.5);
            let k = crate::fx::rex_shake_k(d);
            let want = crate::fx::ROAR_SHAKE.amp_v * k;
            checks.push(check("roar camera shake = 03E ShakeCam record (0.075*k, 30, 0.15*k, 20, 0.15, 1.02)", json!(want), json!(shake.peak_request), approx(shake.peak_request, want, 0.01)));
            let roar_rumbles = rumble.0.iter().filter(|x| x.3 == "rex_roar").count();
            checks.push(check("no roar rumble (record rumble vector is 0,0,0)", json!(0), json!(roar_rumbles), roar_rumbles == 0));
            let peak = r.shake_samples.iter().map(|s| s.1).fold(0.0, f32::max);
            checks.push(check("camera actually shook during roar (rad)", json!(">0.005"), json!(peak), peak > 0.005));
            let gun_rumbles = rumble.0.iter().filter(|x| x.3 == "gun" && x.1 == 50.0 && x.2 == 2.0).count();
            checks.push(check("gun rumble strength 50 per Colt shot", json!(r.fired.len()), json!(gun_rumbles), gun_rumbles == r.fired.len()));
            checks.push(check("muzzle flash per shot", json!(r.fired.len()), json!(stats.flashes), stats.flashes as usize == r.fired.len()));
            checks.push(check("no muzzle light for Colt (Tommy/Shotgun only)", json!(0), json!(stats.lights), stats.lights == 0));
            checks.push(check("roar breath particles emitted", json!(">0"), json!(stats.breath), stats.breath > 0));
        }
        "b2_tommy_vrex_charge" => {
            let w = &WEAPONS[1];
            checks.push(check("Tommy auto interval 0.1 s (shot n+1 when n*0.1 <= held time), 1.0 s held (G02)", json!("10 +/- 1"), json!(shots.len()), (9..=11).contains(&shots.len())));
            let ok_dmg = !rex_hits.is_empty() && rex_hits.iter().all(|h| h.2 == band(w, h.3));
            checks.push(check("Tommy damage band (R1=R2=10 -> 2/1/1) (G02)", json!("band(dist)"), json!(rex_hits.iter().map(|h| (h.2, h.3)).collect::<Vec<_>>()), ok_dmg));
            checks.push(check("magazine decremented per round", json!(w.clip() as usize - r.fired.len()), json!(arsenal.mag[1]), arsenal.mag[1] as usize == w.clip() as usize - r.fired.len()));
            checks.push(check("muzzle light per shot (Tommy)", json!(r.fired.len()), json!(stats.lights), stats.lights as usize == r.fired.len()));
            checks.push(check("loop A starts once per burst", json!(1), json!(count("Jack Tommygun shoot loop A")), count("Jack Tommygun shoot loop A") == 1));
            checks.push(check("'shoot end' tail after release", json!(1), json!(count("Jack Tommygun shoot end")), count("Jack Tommygun shoot end") == 1));
            let steps = r.rex_events.iter().filter(|e| e.1 == "Footstep").count();
            checks.push(check("V-Rex footsteps detected from animated toes while charging", json!(">=2"), json!(steps), steps >= 2));
            checks.push(check("footsteps play 'Trex_footsteps_near'", json!(steps), json!(count("Trex_footsteps_near")), count("Trex_footsteps_near") == steps));
            let foot_rumbles: Vec<(f32, f32)> = rumble.0.iter().filter(|x| x.3 == "rex_footstep").map(|x| (x.1, x.2)).collect();
            checks.push(check("footstep rumble = (floor(100*k), 7)", json!("strength<=100, duration 7"), json!(foot_rumbles), !foot_rumbles.is_empty() && foot_rumbles.iter().all(|s| s.0 <= 100.0 && s.1 == 7.0)));
            let gun = rumble.0.iter().filter(|x| x.3 == "gun").all(|x| x.1 == 50.0 && x.2 == 2.0);
            checks.push(check("Tommy gun rumble (50, 2)", json!([50, 2]), json!(gun), gun));
            // footstep preset only (freq 50); the roar uses the 03E record (freq 30, up to 0.15)
            let peak = r.shake_samples.iter().filter(|s| s.2 == 50.0).map(|s| s.1).fold(0.0, f32::max);
            checks.push(check("footstep camera shake present (<= 0.05 rad)", json!("(0, 0.05]"), json!(peak), peak > 0.0 && peak <= 0.051));
        }
        "b3_shotgun_close" => {
            let w = &WEAPONS[2];
            checks.push(check("shot timer 0.5 s: clicks at +0, +0.3, +0.6 -> 2 shots (G02)", json!(2), json!(shots.len()), shots.len() == 2));
            // G11: the shell casts 25 RAYS on the +-10 deg grid; at 7 m part of them miss the rex, so impacts on the rex are not 25.
            // Count rays fired and require every ray to be accounted for (impact on any surface, or a miss into the sky).
            let (rays, imp, miss) = arsenal.ray_log.first().copied().unwrap_or((0, 0, 0));
            checks.push(check("25 pellet RAYS per shell, each accounted as impact (rex/surface) or sky miss (G11)", json!({"rays": 25, "impacts+misses": 25}), json!({"rays": rays, "impacts": imp, "misses": miss}), rays == 25 && imp + miss == 25));
            let ok_dmg = !rex_hits.is_empty() && rex_hits.iter().all(|h| h.2 == band(w, h.3));
            checks.push(check("pellet damage band by object distance (R1 5 / R2 10 -> 20/10/5) (G02)", json!("band(dist)"), json!(rex_hits.iter().take(6).map(|h| (h.2, h.3)).collect::<Vec<_>>()), ok_dmg));
            let shells = r.commits.len();
            checks.push(check("shell-by-shell reload: shotgun offers 1 round per cycle (G03)", json!("all 1"), json!(r.commits.iter().map(|c| c.1).collect::<Vec<_>>()), shells >= 1 && r.commits.iter().all(|c| c.1 == 1)));
            checks.push(check("'Jack Shotgun shoot' per shell", json!(r.fired.len()), json!(count("Jack Shotgun shoot")), count("Jack Shotgun shoot") == r.fired.len()));
            checks.push(check("pump 'Jack Shotgun rearm' after shots", json!(">=1"), json!(count("Jack Shotgun rearm")), count("Jack Shotgun rearm") >= 1));
            checks.push(check("'Jack Shotgun reload' per shell cycle", json!(">=1"), json!(count("Jack Shotgun reload")), count("Jack Shotgun reload") >= 1));
            checks.push(check("muzzle light per shell (Shotgun)", json!(r.fired.len()), json!(stats.lights), stats.lights as usize == r.fired.len()));
            checks.push(check("flesh ricochet sound on V-Rex hits", json!(">=1"), json!(count("Bullet ricochet flesh")), count("Bullet ricochet flesh") >= 1));
        }
        "b4_sniper_scope" => {
            let w = &WEAPONS[3];
            checks.push(check("shot timer 0.4 s: clicks at +0, +0.2, +0.5 -> 2 shots (G02)", json!(2), json!(shots.len()), shots.len() == 2));
            checks.push(check("scope FOV reaches 0.3 (horizontal, 4:3), smoothed 5*dt (jack::fov_target/smooth_fov)", json!(FOV_SNIPER_AIM), json!(r.fov_min), approx(r.fov_min, FOV_SNIPER_AIM, 0.03)));
            let ok_dmg = !rex_hits.is_empty() && rex_hits.iter().all(|h| h.2 == band(w, h.3));
            checks.push(check("sniper damage at ~34 m (R1 5 / R2 50 -> mid 10) (G02)", json!(10.0), json!(rex_hits.iter().map(|h| (h.2, h.3)).collect::<Vec<_>>()), ok_dmg));
            let big = rumble.0.iter().filter(|x| x.3 == "gun" && x.1 == 100.0 && x.2 == 4.0).count();
            checks.push(check("sniper rumble 100 per shot", json!(r.fired.len()), json!(big), big == r.fired.len()));
            checks.push(check("'Jack Sniper shoot' + 'Jack Sniper rearm'", json!("both"), json!([count("Jack Sniper shoot"), count("Jack Sniper rearm")]), count("Jack Sniper shoot") == r.fired.len() && count("Jack Sniper rearm") >= 1));
        }
        "b5_reload" => {
            let w = &WEAPONS[3];
            let rs = mark("reload_start").unwrap_or(0.0);
            let commit = r.commits.first().map(|c| c.0 - rs);
            checks.push(check("first round commits at action frame (frame/60 s)", json!(w.reload_commit_frame / ANIM_HZ), json!(commit), commit.is_some_and(|c| approx(c, w.reload_commit_frame / ANIM_HZ, 0.1))));
            // H_exec_loading_weapon: only the shotgun is limited to 1 round per cycle; the sniper moves min(clip-mag, reserve)
            checks.push(check("sniper reload moves min(clip-mag, reserve) in one cycle (G03)", json!([r.fired.len()]), json!(r.commits.iter().map(|c| c.1).collect::<Vec<_>>()), r.commits.len() == 1 && r.commits[0].1 as usize == r.fired.len()));
            checks.push(check("magazine refilled to 5 (G03)", json!(5), json!(arsenal.mag[3]), arsenal.mag[3] == w.clip()));
            checks.push(check("sounds: 'Sniper reload' once, 'bullet reload' per round, 'reload end'", json!([1, r.commits.len(), 1]), json!([count("Jack Sniper reload"), count("Jack Sniper bullet reload"), count("Jack Sniper reload end")]), count("Jack Sniper reload") == 1 && count("Jack Sniper bullet reload") == r.commits.len() && count("Jack Sniper reload end") == 1));
        }
        "b6_rex_hunt" => {
            let states: Vec<&str> = r.rex_states.iter().map(|s| s.1.as_str()).collect();
            let order = ["roar", "chase", "bite"];
            let mut i = 0;
            for s in &states {
                if i < order.len() && *s == order[i] {
                    i += 1;
                }
            }
            checks.push(check("state order idle -> (hesite) -> roar -> chase -> bite (X04)", json!(order), json!(states), i == order.len()));
            // first bite on a healthy Jack = paf 0x4104 = a wound; the next bite finds him wounded (life ratio 0.15)
            // -> grab -> kill paf 0x4a10 (X04, H02)
            let wounded_then_dead = matches!((r.jack_wounded_at, r.jack_dead_at), (Some(w), Some(d)) if d > w + 0.2);
            checks.push(check("bite on healthy Jack only wounds (0x4104); the next bite grabs and kills (0x4a10) (X04/H02)", json!("wounded, then dead > 0.2 s later"), json!([r.jack_wounded_at, r.jack_dead_at]), wounded_then_dead));
            checks.push(check("death cause is the grab (0x4a10), state 'grabbed Jack' reached (X04)", json!("grabbed by the V-Rex"), json!([r.jack_death_cause, states.contains(&"grabbed Jack")]), r.jack_death_cause == "grabbed by the V-Rex" && states.contains(&"grabbed Jack")));
            checks.push(check("'Jack injured' plays on the wound (H02)", json!(">=1"), json!(count("Jack injured")), count("Jack injured") >= 1));
            // speed rises by at most 1.5 m/s per second (select_action accel 1.5) and reaches > 8 m/s
            let (t0, s0) = r.chase_trace.first().copied().unwrap_or((0.0, 0.0));
            let over = r.chase_trace.iter().map(|(t, v)| v - (s0 + vrex::ACCEL * (t - t0))).fold(f32::MIN, f32::max);
            let peak = r.chase_trace.iter().map(|x| x.1).fold(0.0, f32::max);
            checks.push(check("chase speed ramps at <= 1.5 m/s^2 toward the 14 m/s run gait (X04)", json!({"slope": vrex::ACCEL, "peak": ">8"}), json!({"over": over, "peak": peak}), over <= 0.1 && peak > 8.0));
            checks.push(check("closest head distance <= bite reach 3.8 m (+eye offset) (X04)", json!("<= 4.6"), json!(r.min_rex_head_dist), r.min_rex_head_dist <= 4.6));
            let all_roars = sfx.0.iter().filter(|s| s.1 == "Trex alert roar").count();
            checks.push(check("'Trex alert roar' then 'Trex_attack_jack' then 'Trex_bite'", json!("all"), json!([all_roars, count("Trex_attack_jack"), count("Trex_bite")]), all_roars >= 1 && count("Trex_attack_jack") >= 1 && count("Trex_bite") >= 1));
            checks.push(check("death sound 'Jack body fall death'", json!(1), json!(count("Jack body fall death")), count("Jack body fall death") >= 1));
        }
        "b7_kong_fight" | "b8_kong_player" | "b9_kong_cinema" | "b10_swamp_fight" | "b13_kong_roam" => {
            if let Some(k) = kong {
                checks.extend(crate::kong::batch_checks(r.batch.name, k, &r.marks));
            }
            if r.batch.name == "b7_kong_fight" {
                if let Some(b) = breakables {
                    let chk = |name: &str, exp: serde_json::Value, act: serde_json::Value, pass: bool| json!({"check": name, "expected": exp, "actual": act, "pass": pass});
                    checks.push(chk("courtyard gate blocks Jack while intact (walk probe)", json!("stopped before the gate"), json!(b.probe_intact), b.probe_intact.is_some_and(|p| !p.1)));
                    checks.push(chk("Kong smashes the courtyard gate after the victory", json!("porte"), json!(b.log), b.is_broken("porte")));
                    checks.push(chk("Jack walks through the broken gate (walk probe)", json!("through"), json!(b.probe_broken), b.probe_broken.is_some_and(|p| p.1)));
                }
            }
        }
        _ => {}
    }
    let mp = |m: &str| r.mark_pos.iter().find(|x| x.0 == m).map(|x| x.1);
    let chk2 = |name: &str, exp: serde_json::Value, act: serde_json::Value, pass: bool| json!({"check": name, "expected": exp, "actual": act, "pass": pass});
    if r.batch.name == "b11_gate_walk" {
        let (b, th) = (mp("blocked"), mp("through"));
        checks.push(chk2("intact gate stops Jack (z of the gate face -99.8 + radius)", json!("z >= -99.7"), json!(b.map(|v| v.z)), b.is_some_and(|v| v.z >= -99.7)));
        if let Some(bk) = breakables {
            checks.push(chk2("gate smashed", json!("porte"), json!(bk.log), bk.is_broken("porte")));
        }
        checks.push(chk2("Jack walks through the broken gate", json!("z < -103"), json!(th.map(|v| v.z)), th.is_some_and(|v| v.z < -103.0)));
        let (s0, s1) = (mp("stairs0"), mp("stairs1"));
        checks.push(chk2(
            "Jack climbs the gate's stone steps (DEC_C_EscalierPorte)",
            json!("moved > 2 m and rose > 0.4 m"),
            json!({"start": s0.map(|v| [v.x, v.y, v.z]), "end": s1.map(|v| [v.x, v.y, v.z])}),
            matches!((s0, s1), (Some(a), Some(b)) if a.z - b.z > 2.0 && b.y - a.y > 0.4),
        ));
        let (w0, w1) = (mp("wall0"), mp("wall1"));
        checks.push(chk2(
            "a ruin wall (murcass01_A, east face x 31.3) stops Jack",
            json!("x >= 30.8"),
            json!({"start": w0.map(|v| [v.x, v.y, v.z]), "end": w1.map(|v| [v.x, v.y, v.z])}),
            w1.is_some_and(|v| v.x >= 30.8) && w0.is_some(),
        ));
    }
    if r.batch.name == "b12_slice_spears" {
        if let Some(k) = spears {
            let has = |s: &str| k.log.iter().any(|x| x.1.contains(s));
            let log: Vec<String> = k.log.iter().map(|x| format!("{:.2} {}", x.0, x.1)).collect();
            checks.push(chk2("spear picked from the rack (E)", json!("pickup Developed"), json!(log), has("pickup Developed")));
            checks.push(chk2("spear thrown at 50 m/s (2.5 x 20)", json!("throw"), json!(k.log.iter().find(|x| x.1.starts_with("throw")).map(|x| x.1.clone())), has("throw Developed 50.0")));
            checks.push(chk2("thrown spear hits the raptor or sticks in the level", json!("hit/stuck"), json!(k.log.iter().filter(|x| x.1.contains("spear ")).map(|x| x.1.clone()).collect::<Vec<_>>()), has("spear hit creature") || has("spear stuck")));
            checks.push(chk2("bone spear taken from the bone pile", json!("pickup Bone"), json!(true), has("pickup Bone")));
            checks.push(chk2("bone dropped (G)", json!("drop"), json!(true), has("drop")));
        }
    }
    let passed = checks.iter().filter(|c| c["pass"] == true).count();
    let report = json!({
        "batch": r.batch.name,
        "about": r.batch.about,
        "reference": r.batch.reference,
        "screenshots": r.shots.iter().cloned().chain(kong.map(|k| k.shots.clone()).unwrap_or_default()).collect::<Vec<_>>(),
        "checks": checks,
        "passed": passed,
        "total": checks.len(),
        "shots_fired": r.fired.len(),
        "rex_states": r.rex_states,
        "rex_events": r.rex_events,
        "sounds": sfx.0.iter().map(|s| format!("{:.2} {}", s.0, s.1)).collect::<Vec<_>>(),
        "mag_trace": r.mag_trace,
    });
    std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap()).ok();
    println!("BATCH {} : {}/{} mechanic checks passed -> {}", r.batch.name, passed, checks.len(), path.display());
}
