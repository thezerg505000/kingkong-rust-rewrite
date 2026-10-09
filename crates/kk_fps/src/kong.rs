//! King Kong in the playable slice: Kong and the V-Rex fight each other in the 03E arena,
//! driven by `kk_mechanics::kong::fight::Fight` (the recovered Kong-vs-KT-rex simulation, ledger B02),
//! and the player can become Kong at any moment (Tab / pad Select) without a reload.
//!
//! * As Jack: `KongBrain` plays Kong's pad (`KongInput`), Jack walks around and watches/shoots.
//! * As Kong: Jack is hidden and frozen where he stood, the main camera turns into Kong's third
//!   person camera (`kong_cam.rs`, parameters of `kong::camera` where recovered) and the keyboard/pad
//!   produce `KongInput`: stick = WASD / left stick (camera relative), JUMP_ROLL = Space / A,
//!   SPECIAL = Q / Y (chest pound, fury), ATTACK = Mouse 1 / X, CANCEL = E / B.
//! * The fight plane (metres, x/y) is laid on the arena ground (`CENTER`), the fight's KT rex state
//!   drives the existing rex entity (transform, `Rex` state for the sound/fx events, clips) while the
//!   Jack-level rex AI is off. `KK_NO_KONG=1` restores the old Jack-vs-rex behaviour entirely.
//!
//! Everything about presentation here is `[G]` (clip choice for the rex, camera, effects); the
//! action-id to clip table for Kong is the recovered `kong_actions.json` (`[L]`/`[C]` per clip).

use crate::anim::{GameState, Rig, RigPlayer, Rigs};
use crate::batch::CleanHud;
use crate::fx::{CameraShake, FxAssets};
use crate::kong_cam::KongCam;
use crate::player::{MainCam, Player, PlayerSet};
use crate::rex::{Rex, RexScene, RexSet, RexState};
use crate::world::Arena;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use kk_mechanics::kong::ai::KongBrain;
use kk_mechanics::kong::combat::Phase;
use kk_mechanics::kong::fight::*;
use kk_mechanics::kong::vrex::KtState;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

pub const KONG_GLB: &str = "kong/kong.glb";
const ACTIONS_JSON: &str = "kong/kong_actions.json";

/// Preferred fight centre on the 03E ground: the wide rubble field south of the passage (x 10..70, z -100..-70).
/// The real centre is the largest empty circle found by `fightarena.rs` in that region.
pub const CENTER_XZ: (f32, f32) = (40.0, -83.0);

/// Fight plane rotation about the fight centre (set once at setup from the arena's longest free chord).
static FIGHT_ROT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub fn fight_rot() -> f32 {
    f32::from_bits(FIGHT_ROT.load(std::sync::atomic::Ordering::Relaxed))
}
fn set_fight_rot(r: f32) {
    FIGHT_ROT.store(r.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

// ---------------------------------------------------------------------------------------------
// Enabling
// ---------------------------------------------------------------------------------------------

/// Kong is in the scene unless `KK_NO_KONG` is set. Batches b1..b6 and the autotest/gallery runs stay
/// Jack-vs-rex: Kong only appears for batches whose name contains "kong" (or with `KK_KONG=1`).
pub fn kong_enabled() -> bool {
    if std::env::var("KK_NO_KONG").is_ok_and(|v| v != "0") {
        return false;
    }
    if std::env::var("KK_KONG").is_ok_and(|v| v == "1") {
        return true;
    }
    if std::env::var("KK_AUTOTEST").is_ok() || std::env::var("KK_GALLERY").is_ok() {
        return false;
    }
    match std::env::var("KK_BATCH") {
        Ok(b) => b.contains("kong") || b.contains("swamp"),
        Err(_) => true,
    }
}

fn batch_name() -> Option<String> {
    std::env::var("KK_BATCH").ok()
}

/// Run condition: the Jack-level rex AI/animation runs (no Kong fight owns the rex).
pub fn jack_rex_ai(kong: Option<Res<KongCtl>>) -> bool {
    kong.is_none()
}

/// Run condition: Jack's controls, camera and weapons run (the player is not Kong).
pub fn jack_active(kong: Option<Res<KongCtl>>) -> bool {
    kong.map_or(true, |k| !k.player_control && !k.cinematic)
}

// ---------------------------------------------------------------------------------------------
// Resources and components
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Action {
    pub clips: Vec<String>,
    pub looping: bool,
}

#[derive(Resource)]
pub struct KongAssets {
    pub rig: Rig,
    pub actions: HashMap<u32, Action>,
}

#[derive(Default, Debug, Clone)]
pub struct KongStats {
    pub splashes: u32,
    pub flashes: u32,
    pub shakes: u32,
    pub footsteps: u32,
    pub rex_footsteps: u32,
    pub ring_splashes: u32,
}

#[derive(Resource)]
pub struct KongCtl {
    pub fight: Fight,
    pub brain: KongBrain,
    pub seed: u32,
    pub player_control: bool,
    /// camera follows Kong while the brain plays (batch b9 / `KK_KONG_CAM=1`)
    pub cinematic: bool,
    /// Jack's camera tracks the fight (batch b7 / `KK_KONG_WATCH=1`)
    pub watch: bool,
    pub center: Vec3,
    /// the fight's free 2D arena (clearance grid from the level collision), None in the stand-in arena
    pub farena: Option<crate::fightarena::FightArena>,
    pub t: f32,
    pub frame_events: Vec<FightEvent>,
    pub log: Vec<(f32, FightEvent)>,
    /// (time, action id, clip) of every Kong clip started
    pub played: Vec<(f32, u32, String)>,
    pub missing_ids: Vec<u32>,
    pub rex_clips: Vec<(f32, String)>,
    pub switches: Vec<(f32, bool)>,
    pub jack_pos_at_switch: Option<Vec3>,
    pub input: KongInput,
    pub kong_yaw: f32,
    pub rex_yaw: f32,
    pub kong_y: f32,
    pub rex_y: f32,
    pub kong_head: Vec3,
    pub kong_pos_prev: Vec3,
    pub kong_speed: f32,
    pub cam: KongCam,
    pub cam_fwd: Vec2,
    pub finished_at: Option<f32>,
    pub over_at: Option<f32>,
    pub stats: KongStats,
    /// per-frame samples (t, player_control, jack hidden, dist(cam, Jack), dist(cam, Kong))
    pub samples: Vec<(f32, bool, bool, f32, f32)>,
    pub jack_pos_end: Option<Vec3>,
    /// batch scaffold: stand Jack at this world (x, z) on the next frame
    pub pending_jack: Option<(f32, f32)>,
    pub max_sync_err: f32,
    /// swamp batch: last 3 s slot filmed; min clearance of the fighters' centres seen after the slide; frames a fighter had to be held
    pub last_slot: u32,
    pub min_clear_kong: f32,
    pub min_clear_rex: f32,
    pub held_frames: u32,
    pub shots: Vec<String>,
    shot_done: Vec<bool>,
    shot_due: Vec<Option<f32>>,
    inited: bool,
    rex_enter_t: f32,
    rex_state_prev: Option<KtState>,
    rex_pos_prev: Vec2,
    rex_choice: Option<(KtState, Option<RexMove>, bool)>,
    pub rex_moving: bool,
    rex_speed: f32,
    /// F8 pressed: restart the fight
    pub respawn_requested: bool,
    /// after the victory Kong smashes the 03E courtyard gate open for Jack: 0 waiting, 1 walking,
    /// 2 punching, 3 done (or not applicable)
    pub gate_phase: u8,
    gate_t: f32,
    /// the post-fight sequence is still running (batches wait for it)
    pub post_busy: bool,
    /// KT clip queue (second clip of 0x33, 0x1c after the death 0x15)
    rex_queue: VecDeque<String>,
    /// KT anim id the rex plays (from the fight's anim events / state loops)
    pub rex_kt_id: Option<u32>,
    clean_prev: bool,
}

/// The fight with the level's rex life profile: the 07D swamp rex J_PNJ_KTREX_2 overrides life to
/// 50/50/25, other levels use the KT default 80/80/50 (instance data, research/pc/instances/tunables.json) [C].
fn new_fight(seed: u32) -> Fight {
    {
        use kk_mechanics::kong::fight::RexProfile;
        let prof = match crate::scene::swamp_level() {
            Some(crate::scene::Swamp::L05C) => RexProfile::MARSH_05C,
            Some(crate::scene::Swamp::L07D) => RexProfile::ARENA_07D,
            None => RexProfile::KT_DEFAULT,
        };
        Fight::with_profile(seed, prof)
    }
}

impl KongCtl {
    pub fn new(center: Vec3, seed: u32) -> Self {
        let watch = std::env::var("KK_KONG_WATCH").is_ok_and(|v| v == "1")
            || batch_name().is_some_and(|b| b.contains("kong_fight") || b.contains("kong_cinema") || b.contains("kong_player") || b.contains("swamp_fight"));
        let cinematic = std::env::var("KK_KONG_CAM").is_ok_and(|v| v == "1")
            || batch_name().is_some_and(|b| b.contains("kong_cinema") || b.contains("swamp_fight"));
        let fight = new_fight(seed);
        let (kp, rp) = (fight.kong.pos, fight.rex.pos);
        let mut c = Self {
            fight,
            brain: KongBrain::new(seed),
            seed,
            player_control: false,
            cinematic,
            watch,
            center,
            farena: None,
            t: 0.0,
            frame_events: vec![],
            log: vec![],
            played: vec![],
            missing_ids: vec![],
            rex_clips: vec![],
            switches: vec![],
            jack_pos_at_switch: None,
            input: KongInput::default(),
            kong_yaw: 0.0,
            rex_yaw: 0.0,
            kong_y: center.y,
            rex_y: center.y,
            kong_head: center,
            kong_pos_prev: center,
            kong_speed: 0.0,
            cam: KongCam::default(),
            cam_fwd: Vec2::new(0.0, -1.0),
            finished_at: None,
            over_at: None,
            stats: KongStats::default(),
            samples: vec![],
            jack_pos_end: None,
            pending_jack: None,
            max_sync_err: 0.0,
            last_slot: 0,
            min_clear_kong: f32::MAX,
            min_clear_rex: f32::MAX,
            held_frames: 0,
            shots: vec![],
            shot_done: vec![false; SHOT_PLAN.len()],
            shot_due: vec![None; SHOT_PLAN.len()],
            inited: false,
            rex_enter_t: 0.0,
            rex_state_prev: None,
            rex_pos_prev: Vec2::ZERO,
            rex_choice: None,
            rex_queue: VecDeque::new(),
            rex_speed: 0.0,
            respawn_requested: false,
            gate_phase: 0,
            gate_t: 0.0,
            post_busy: false,
            rex_kt_id: None,
            rex_moving: false,
            clean_prev: false,
        };
        c.kong_yaw = yaw_of(c.fight.kong.facing);
        c.rex_yaw = yaw_of(c.fight.rex.facing);
        let _ = (kp, rp);
        c
    }

    /// Fight plane (x, y metres) to world: fight +x = world +X, fight +y = world -Z (orientation preserving),
    /// turned by the arena rotation about the centre.
    pub fn world(&self, p: V2, y: f32) -> Vec3 {
        let (s, c) = fight_rot().sin_cos();
        let (x, yy) = (p.0 * c - p.1 * s, p.0 * s + p.1 * c);
        Vec3::new(self.center.x + x, y, self.center.z - yy)
    }
    pub fn plane(&self, w: Vec3) -> V2 {
        let (x, y) = (w.x - self.center.x, -(w.z - self.center.z));
        let (s, c) = fight_rot().sin_cos();
        (x * c + y * s, -x * s + y * c)
    }
    pub fn kong_world(&self) -> Vec3 {
        self.world(self.fight.kong.pos, self.kong_y)
    }
    pub fn rex_world(&self) -> Vec3 {
        self.world(self.fight.rex.pos, self.rex_y)
    }
    /// world XZ unit direction of a fight-plane facing angle
    pub fn dir_xz(angle: f32) -> Vec2 {
        let a = angle + fight_rot();
        Vec2::new(a.cos(), -a.sin())
    }
    pub fn has(&self, f: impl Fn(&FightEvent) -> bool) -> bool {
        self.log.iter().any(|(_, e)| f(e))
    }
    pub fn count(&self, f: impl Fn(&FightEvent) -> bool) -> usize {
        self.log.iter().filter(|(_, e)| f(e)).count()
    }
}

/// model yaw (about +Y) that makes the +Z-forward rigs face a fight-plane angle
pub fn yaw_of(angle: f32) -> f32 {
    let d = KongCtl::dir_xz(angle);
    d.x.atan2(d.y)
}

fn turn_toward(cur: f32, target: f32, max_step: f32) -> f32 {
    let mut d = (target - cur) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    cur + d.clamp(-max_step, max_step)
}

#[derive(Component)]
pub struct KongRoot;
#[derive(Component)]
pub struct KongSceneRoot;

/// Bones of the spawned Kong rig.
#[derive(Component)]
pub struct KongBones {
    pub player: Entity,
    pub pelvis: Entity,
    pub head: Entity,
    pub jaw: Entity,
    /// PiedG, PiedD, MainG, MainD (feet and knuckles touch the ground)
    pub contacts: [Entity; 4],
    pub hands: [Entity; 2],
}

#[derive(Component, Default)]
pub struct KongAnim {
    pub queue: VecDeque<(String, f32)>,
    pub cur_id: u32,
}

// ---------------------------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------------------------

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct KongSet;

pub struct KongPlugin;

impl Plugin for KongPlugin {
    fn build(&self, app: &mut App) {
        if !kong_enabled() {
            return;
        }
        use bevy::render::view::VisibilitySystems;
        app.add_systems(OnEnter(GameState::Playing), setup_kong)
            .add_systems(
                Update,
                (
                    kong_switch,
                    kong_fight,
                    apply_kong,
                    apply_rex,
                    jack_watch,
                    crate::kong_cam::kong_camera,
                    survey.run_if(|| std::env::var("KK_SURVEY").is_ok()),
                    bake_kong_normals,
                )
                    .chain()
                    .in_set(KongSet)
                    .before(PlayerSet)
                    .run_if(in_state(GameState::Playing))
                    .run_if(resource_exists::<KongCtl>),
            )
            .add_systems(
                Update,
                (
                    crate::kong_fx::kong_effects,
                    crate::kong_fx::kong_footsteps,
                    crate::kong_fx::rex_splash,
                    crate::kong_fx::motion_blur_update,
                    kong_sample,
                    kong_shots,
                    kong_hud_update,
                )
                    .chain()
                    .after(RexSet)
                    .after(KongSet)
                    .run_if(in_state(GameState::Playing))
                    .run_if(resource_exists::<KongCtl>),
            )
            .add_systems(
                PostUpdate,
                (
                    pelvis_lock.after(bevy::transform::TransformSystem::TransformPropagate),
                    kong_hud_vis.before(VisibilitySystems::VisibilityPropagate),
                )
                    .run_if(resource_exists::<KongCtl>),
            );
    }
}

// ---------------------------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------------------------

fn build_rig(rig: &mut Rig, gltf: &Gltf, clips: &Assets<AnimationClip>, graphs: &mut Assets<AnimationGraph>) {
    let mut names: Vec<String> = gltf.named_animations.keys().map(|k| k.to_string()).collect();
    names.sort();
    let handles: Vec<Handle<AnimationClip>> = names.iter().map(|n| gltf.named_animations[n.as_str()].clone()).collect();
    let (graph, idx) = AnimationGraph::from_clips(handles.iter().cloned());
    rig.graph = graphs.add(graph);
    for ((n, i), h) in names.iter().zip(idx).zip(handles.iter()) {
        rig.nodes.insert(n.clone(), i);
        rig.durations.insert(n.clone(), clips.get(h).map(|c| c.duration()).unwrap_or(1.0));
    }
    rig.names = names;
}

/// kong_actions.json: action id -> clip sequence. Ids are hex strings ("0xab").
fn load_actions(rig: &Rig) -> HashMap<u32, Action> {
    let path = crate::asset_dir().join(ACTIONS_JSON);
    let mut out = HashMap::new();
    let Some(v) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
        warn!("{} missing: Kong clips fall back to label search", path.display());
        return out;
    };
    for (k, e) in v.as_object().into_iter().flatten() {
        let Some(id) = u32::from_str_radix(k.trim_start_matches("0x"), 16).ok() else { continue };
        let label = e["label"].as_str().unwrap_or("");
        let mut clips: Vec<String> = e["items"]
            .as_array()
            .map(|a| a.iter().filter_map(|i| i["clip_name"].as_str().map(String::from)).collect())
            .unwrap_or_default();
        if clips.is_empty() {
            if let Some(c) = e["clip_name"].as_str() {
                clips.push(c.to_string());
            }
        }
        // idle fidget chains (id 0x8) and loops play their first clip only
        if label == "idle" || e["loop"].as_bool().unwrap_or(false) {
            clips.truncate(1);
        }
        clips.retain(|c| rig.find(c).is_some());
        if clips.is_empty() {
            continue;
        }
        out.insert(id, Action { clips, looping: e["loop"].as_bool().unwrap_or(false) });
    }
    out
}

/// Fight ids the game code does not name, or whose clip table entry is not the move we want [G]:
/// 0x1/0x2 (walk/run placeholders) -> gait clips 0xa/0xc, grab reach 0xa0 -> 0xa6, jaw-break lock/mash
/// 0xe0/0xe1 -> the finisher_jaw clips 0xe6/0xe7 (`k_ETAT_finish` ids).
fn resolve_id(id: u32) -> u32 {
    match id {
        ANIM_WALK => 0x0a,
        ANIM_RUN => 0x0c,
        ANIM_GRAB_START => 0xa6,
        ANIM_FINISH_LOCK => 0xe6,
        ANIM_FINISH_MASH => 0xe7,
        i => i,
    }
}

/// Ground height under (x, z): the highest ground not more than 1.2 m above `fallback` (the height we
/// stood at), so Kong never climbs onto the cliff ledges around the field; `fallback` where there is none.
fn ground_y(arena: &Arena, _center: Vec3, x: f32, z: f32, fallback: f32) -> f32 {
    match &arena.level {
        Some(l) => {
            let g = l.ground_base(x, z, fallback, 1.2).unwrap_or(fallback);
            // the swamp: the fighters wade, never deeper than about knee height under the surface (the pools'
            // render floors drop away under the water planes) [G]
            if crate::scene::swamp() { g.max(crate::swamp::water_y() - 0.9) } else { g }
        }
        None => 0.0,
    }
}

/// Is there open ground at this world point, at about the field's height?
pub fn open_ground(arena: &Arena, c: &KongCtl, x: f32, z: f32) -> bool {
    match &arena.level {
        Some(l) => l.ground_base(x, z, c.center.y + 1.8, 0.0).is_some_and(|y| (y - c.center.y).abs() < 3.0),
        None => true,
    }
}

pub fn ground_y_pub(arena: &Arena, c: &KongCtl, x: f32, z: f32, fallback: f32) -> f32 {
    ground_y(arena, c.center, x, z, fallback)
}

#[allow(clippy::too_many_arguments)]
/// Search region, floor window and preferred centre of the fight arena per scene [G].
fn arena_spec(l: &crate::world::LevelCollision) -> crate::fightarena::ArenaSpec {
    use crate::fightarena::ArenaSpec;
    if crate::scene::swamp() {
        let pref = std::env::var("KK_ARENA")
            .ok()
            .and_then(|v| v.split_once(',').and_then(|(a, b)| Some((a.trim().parse().ok()?, b.trim().parse().ok()?))))
            .unwrap_or(if crate::scene::marsh05c() { MARSH_PREFER } else { SWAMP_PREFER });
        let wy = crate::swamp::water_y();
        let yref = l.ground_base(pref.0, pref.1, wy + 0.85, 0.0).unwrap_or(wy - 0.35);
        ArenaSpec { region: (pref.0 - 45.0, pref.1 - 40.0, pref.0 + 45.0, pref.1 + 40.0), yref, below: 1.3, above: 0.8, prefer: pref, max_radius: 26.0 }
    } else {
        let y = l.ground_base(CENTER_XZ.0, CENTER_XZ.1, 50.0, 0.0).unwrap_or(5.5);
        ArenaSpec { region: (5.0, -112.0, 80.0, -55.0), yref: y, below: 1.5, above: 0.6, prefer: CENTER_XZ, max_radius: 30.0 }
    }
}

/// Preferred fight centre in the swamp: the flooded lowland west of arena 2 (water plane -1.65, Part5_Water) [G].
pub const SWAMP_PREFER: (f32, f32) = (96.0, -290.0);
/// 05C marsh: between the two marsh rexes' instance positions (118.5,-181.5) and (140.5,-187.3) [C], nudged
/// toward the open water south of Ann's branch (135.7,-171.7) [G].
pub const MARSH_PREFER: (f32, f32) = (130.0, -190.0);

#[allow(clippy::too_many_arguments)]
fn setup_kong(
    mut commands: Commands,
    rigs: Res<Rigs>,
    gltfs: Res<Assets<Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    arena: Res<Arena>,
) {
    let Some(h) = rigs.kong_gltf.clone() else { return };
    let Some(g) = gltfs.get(&h) else {
        error!("kong.glb not loaded; Kong disabled");
        return;
    };
    let mut rig = Rig::default();
    build_rig(&mut rig, g, &clips, &mut graphs);
    let actions = load_actions(&rig);
    info!("Kong ready: {} clips, {} action ids", rig.names.len(), actions.len());
    // the 05C marsh showcase uses a seed whose fight shows every checked move with the knock-back
    // (kk_mechanics `print_seed_coverage`): seed 3 has the shoulder strike, a rex hit on Kong, throw, fury
    let default_seed = if batch_name().is_some_and(|b| b.contains("swamp_fight")) && crate::scene::marsh05c() { 3 } else { 1 };
    let seed = std::env::var("KK_KONG_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(default_seed);
    let farena = arena.level.as_ref().map(|l| {
        let spec = arena_spec(l);
        let fa = crate::fightarena::FightArena::build(l, &spec);
        info!(
            "fight arena: centre ({:.1}, {:.1}, {:.1}) clearance {:.1} m, axis {:.0} deg, Kong x {:.1} rex x {:.1}, Jack vantage ({:.1}, {:.1}, {:.1})",
            fa.center.x, fa.center.y, fa.center.z, fa.best_clearance, fa.rot.to_degrees(), fa.kong_x, fa.rex_x, fa.jack.x, fa.jack.y, fa.jack.z
        );
        if let Ok(p) = std::env::var("KK_ARENA_DUMP") {
            fa.dump_pgm(&p);
        }
        fa
    });
    let center = match &farena {
        Some(fa) => {
            set_fight_rot(fa.rot);
            fa.center
        }
        None => Vec3::new(0.0, 0.0, -5.0),
    };
    let mut ctl = KongCtl::new(center, seed);
    if let Some(fa) = &farena {
        ctl.fight.kong.pos = (fa.kong_x, 0.0);
        ctl.fight.rex.pos = (fa.rex_x, 0.0);
    }
    ctl.farena = farena;
    let start = ctl.world(ctl.fight.kong.pos, center.y);
    let root = commands
        .spawn((
            Name::new("Kong"),
            KongRoot,
            Transform::from_translation(start).with_rotation(Quat::from_rotation_y(ctl.kong_yaw)),
            Visibility::default(),
        ))
        .id();
    let scene = commands
        .spawn((
            Name::new("KongScene"),
            KongSceneRoot,
            SceneRoot(g.scenes[0].clone()),
            KongAnim::default(),
            Transform::default(),
        ))
        .observe(on_kong_ready)
        .id();
    commands.entity(root).add_child(scene);
    commands.insert_resource(KongAssets { rig, actions });
    commands.insert_resource(ctl);
}

#[allow(clippy::too_many_arguments)]
fn on_kong_ready(
    trigger: Trigger<bevy::scene::SceneInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    names: Query<&Name>,
    meshes: Query<(), With<Mesh3d>>,
    players: Query<Entity, With<AnimationPlayer>>,
    assets: Res<KongAssets>,
    mat_q: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let root = trigger.target();
    let mut found: HashMap<&'static str, Entity> = HashMap::new();
    const WANT: [&str; 9] = [
        "B_Kong_Bassin", "B_Kong_Tete", "B_Kong_Machoire", "B_Kong_PiedG", "B_Kong_PiedD", "B_Kong_MainG", "B_Kong_MainD", "B_Kong_Cou",
        "B_Kong_Torse",
    ];
    for e in children.iter_descendants(root) {
        if meshes.contains(e) {
            // skinned AABBs are computed from the bind pose
            commands.entity(e).insert((bevy::render::view::NoFrustumCulling, KongMesh));
            if let Ok(h) = mat_q.get(e) {
                if let Some(m) = mats.get_mut(&h.0) {
                    tune_material(m);
                }
            }
        }
        if let Ok(n) = names.get(e) {
            if let Some(w) = WANT.iter().find(|w| **w == n.as_str()) {
                found.insert(w, e);
            }
        }
    }
    let player = crate::anim::attach_graph(&mut commands, root, &children, &players, &assets.rig.graph);
    let g = |k: &str| found.get(k).copied();
    match (
        player, g("B_Kong_Bassin"), g("B_Kong_Tete"), g("B_Kong_Machoire"), g("B_Kong_PiedG"), g("B_Kong_PiedD"), g("B_Kong_MainG"),
        g("B_Kong_MainD"),
    ) {
        (Some(player), Some(pelvis), Some(head), Some(jaw), Some(fl), Some(fr), Some(hl), Some(hr)) => {
            commands.entity(root).insert((
                KongBones { player, pelvis, head, jaw, contacts: [fl, fr, hl, hr], hands: [hl, hr] },
                RigPlayer { player, current: String::new() },
            ));
        }
        _ => error!("kong glb is missing bones / AnimationPlayer"),
    }
}

/// Kong's own Jade material (record 0xa8ee0b in ff00018c, a type-5 multitexture material, layout of
/// `atmos/MATERIAL_FORMAT.md`), decoded [C bytes, L use]:
///  - extension flags 0x6c000000 (bits 29 and 30 set), diffuse colour c1 = 0x4d4c4c4c -> RGB 0.298, A 0.302;
///    specular colour c2 = 0x405c5c5c -> RGB 0.361; shininess 6.52 (0x40d0a3d7); normal map 0x5600d6b9; no spec map;
///  - bit 29: diffuse = min(2 * c1 * world diffuse tint, 1); the tint of Kong's world record is not recovered, the
///    engine default 0xff808080 (0.502) is used -> 0.299 (the rex world's 0xfd778677 gives its 0.329/0.371/0.329);
///  - bit 30: spec = min(2 * c2 * 0.502, 1) = 0.363, shininess 6.52 -> Blinn exponent -> roughness ~ 0.48;
///  - normal XY scale = c1.A * 4 = 1.21 (baked into the normal maps by `bake_kong_normals`);
///  - actors take no RLI (the multiplier 1 + 10 RLI is a level-mesh term), they get the zone ambient instead,
///    which Bevy cannot add per object, so it is added as base-texture emission like the rex's [L form].
/// The fur atlas is very dark (mean sRGB 30/255), the reference close-ups read ~58/255: the per-light colour and
/// multipliers (`L.rgb * L[0x48]`, not recovered) are replaced by one gain per scene, fitted to the reference [G].
pub const KONG_DIFFUSE: f32 = 0.299;
pub const KONG_SPEC: f32 = 0.363;
pub const KONG_NORMAL_SCALE: f32 = 1.21;

fn kong_gain() -> (f32, f32) {
    let env = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    if crate::scene::marsh05c() {
        // the 05C grade is brighter (exposure +0.62): Kong stays the near-black mass of the clip [G]
        (env("KK_KONG_GAIN", 7.0), env("KK_KONG_EMIS", 0.2))
    } else if crate::scene::swamp() {
        (env("KK_KONG_GAIN", 11.0), env("KK_KONG_EMIS", 0.35))
    } else {
        (env("KK_KONG_GAIN", 11.0), env("KK_KONG_EMIS", 0.35))
    }
}

fn tune_material(m: &mut StandardMaterial) {
    let (gain, emis) = kong_gain();
    m.metallic = 0.0;
    // wet fur: shininess 6.52 -> roughness 0.48, spec colour 0.363 -> Bevy reflectance ~0.4 [L]
    // Jade lights fur with fixed-function Blinn (no Fresnel): Bevy's Fresnel at reflectance 0.4 turned the silhouette and
    // the legs into a pale grey sheen under the swamp's bright ambient, so the F0 is kept low [L]
    m.perceptual_roughness = 0.62;
    m.reflectance = 0.12;
    m.occlusion_texture = None;
    m.metallic_roughness_texture = None;
    m.emissive_texture = m.base_color_texture.clone();
    m.emissive = LinearRgba::new(emis * 1.1, emis, emis * 0.9, 1.0);
    let d = KONG_DIFFUSE * gain;
    m.base_color = Color::linear_rgb(d * 1.15, d * 0.99, d * 0.9);
    m.double_sided = false;
}

/// Contrast of Kong's fur atlas around its mean (linear), baked into the diffuse maps [G]: the atlas has a bright
/// chest blotch and near-black arms, the reference fur reads as one dark grey-brown with a narrow spread
/// (p10/p90 = 34/83 in s_056).
fn kong_contrast() -> f32 {
    // with the shell fur (kong_fur.rs) the atlas is used as the game has it
    std::env::var("KK_KONG_CONTRAST").ok().and_then(|v| v.parse().ok()).unwrap_or(if crate::kong_fur::enabled() { 1.0 } else { 0.6 })
}

fn srgb_to_lin(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
fn lin_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// Bake the material's normal XY scale (1.21) into Kong's normal maps and the contrast into the fur atlas once loaded.
fn bake_kong_normals(
    mut done: Local<std::collections::HashSet<AssetId<Image>>>,
    mut detail: Local<Option<Handle<Image>>>,
    server: Res<AssetServer>,
    mats: Res<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    q: Query<&MeshMaterial3d<StandardMaterial>, With<KongMesh>>,
) {
    if q.is_empty() {
        return;
    }
    let dh = detail.get_or_insert_with(|| server.load("kong/fur_detail.png")).clone();
    // the fur detail layer of Kong's material (idx173, vertical strands, 512 px) as a luminance tile
    let tile: Option<(Vec<f32>, usize)> = images.get(&dh).and_then(|im| {
        let d = im.data.as_ref()?;
        let n = im.width() as usize;
        let lum: Vec<f32> = d.chunks_exact(4).map(|p| srgb_to_lin(p[1] as f32 / 255.0)).collect();
        let mean = lum.iter().sum::<f32>() / lum.len().max(1) as f32;
        Some((lum.iter().map(|v| v / mean.max(1e-5)).collect(), n))
    });
    if tile.is_none() {
        return;
    }
    let (tile, tn) = tile.unwrap();
    for h in &q {
        let Some(m) = mats.get(&h.0) else { continue };
        if let Some(d) = &m.base_color_texture {
            if !done.contains(&d.id()) {
                if let Some(img) = images.get_mut(d) {
                    if let Some(data) = img.data.as_mut() {
                        done.insert(d.id());
                        let k = kong_contrast();
                        // mean luminance (linear) of the atlas, then scale each pixel's deviation from it
                        let mut sum = 0.0f64;
                        let mut n = 0usize;
                        for px in data.chunks_exact(4) {
                            sum += srgb_to_lin(px[1] as f32 / 255.0) as f64;
                            n += 1;
                        }
                        let mean = (sum / n.max(1) as f64) as f32;
                        let w = img.texture_descriptor.size.width as usize;
                        let amp = std::env::var("KK_KONG_FUR").ok().and_then(|v| v.parse().ok()).unwrap_or(if crate::kong_fur::enabled() { 0.0 } else { 1.3f32 });
                        let reps = if w >= 1024 { 2usize } else { 1 };
                        for (i, px) in data.chunks_exact_mut(4).enumerate() {
                            let (x, y) = (i % w, i / w);
                            let l = [srgb_to_lin(px[0] as f32 / 255.0), srgb_to_lin(px[1] as f32 / 255.0), srgb_to_lin(px[2] as f32 / 255.0)];
                            let lum = 0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2];
                            // fur detail tile multiplies the atlas (the flat torso / arm regions get their strands)
                            let dn = tile[((y * reps) % tn) * tn + ((x * reps) % tn)];
                            let detail = (1.0 + (dn - 1.0) * amp).clamp(0.25, 2.6);
                            let nl = ((mean + (lum - mean) * k) * detail).max(0.0005);
                            let sc = if lum > 1e-5 { nl / lum } else { 1.0 };
                            for c in 0..3 {
                                px[c] = (lin_to_srgb((l[c] * sc).clamp(0.0, 1.0)) * 255.0).round().clamp(0.0, 255.0) as u8;
                            }
                        }
                    }
                }
            }
        }
        let Some(n) = &m.normal_map_texture else { continue };
        if done.contains(&n.id()) {
            continue;
        }
        let Some(img) = images.get_mut(n) else { continue };
        let Some(data) = img.data.as_mut() else { continue };
        done.insert(n.id());
        for px in data.chunks_exact_mut(4) {
            let x = (px[0] as f32 / 255.0 * 2.0 - 1.0) * KONG_NORMAL_SCALE;
            let y = (px[1] as f32 / 255.0 * 2.0 - 1.0) * KONG_NORMAL_SCALE;
            let l2 = x * x + y * y;
            let z = (1.0 - l2).max(0.0).sqrt();
            let k = if l2 > 1.0 { 1.0 / l2.sqrt() } else { 1.0 };
            px[0] = (((x * k) * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
            px[1] = (((y * k) * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
            px[2] = ((z * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

#[derive(Component)]
pub struct KongMesh;

// ---------------------------------------------------------------------------------------------
// Switch Jack <-> Kong
// ---------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn kong_switch(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut ctl: ResMut<KongCtl>,
    mut jack: Query<(&Transform, &mut Visibility), With<Player>>,
    cams: Query<Entity, With<MainCam>>,
    mut clean: ResMut<CleanHud>,
) {
    let mut toggle = keys.just_pressed(KeyCode::Tab);
    for g in &gamepads {
        toggle |= g.just_pressed(GamepadButton::Select);
    }
    if !toggle {
        return;
    }
    let Ok((jtf, mut vis)) = jack.single_mut() else { return };
    let c = &mut *ctl;
    c.cinematic = false;
    if !c.player_control {
        // Jack -> Kong: Jack stays where he stands, hidden; the fight state is untouched
        c.player_control = true;
        c.jack_pos_at_switch = Some(jtf.translation);
        c.cam.init = false;
        *vis = Visibility::Hidden;
        c.clean_prev = clean.0;
        clean.0 = true;
        if crate::kong_fx::motion_blur_ok() {
            for e in &cams {
                commands.entity(e).insert(crate::kong_fx::new_motion_blur());
            }
        }
    } else {
        c.player_control = false;
        *vis = Visibility::Inherited;
        clean.0 = c.clean_prev;
        for e in &cams {
            commands.entity(e).remove::<bevy::core_pipeline::motion_blur::MotionBlur>();
        }
    }
    let t = c.t;
    let to_kong = c.player_control;
    c.switches.push((t, to_kong));
    info!("player is now {}", if to_kong { "Kong" } else { "Jack" });
}

// ---------------------------------------------------------------------------------------------
// Fight step
// ---------------------------------------------------------------------------------------------

fn read_pad(
    keys: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    gamepads: &Query<&Gamepad>,
    cam_fwd: Vec2,
    cursor_grabbed: bool,
) -> KongInput {
    let mut inp = KongInput::default();
    let mut stick = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) { stick.y += 1.0; }
    if keys.pressed(KeyCode::KeyS) { stick.y -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { stick.x += 1.0; }
    if keys.pressed(KeyCode::KeyA) { stick.x -= 1.0; }
    let mut set = |slot: usize, held: bool, pressed: bool| {
        if held { inp.buttons[slot].held = true; }
        if pressed { inp.buttons[slot].pressed = true; }
    };
    set(SLOT_JUMP_ROLL, keys.pressed(KeyCode::Space), keys.just_pressed(KeyCode::Space));
    set(SLOT_SPECIAL, keys.pressed(KeyCode::KeyQ), keys.just_pressed(KeyCode::KeyQ));
    set(SLOT_CANCEL, keys.pressed(KeyCode::KeyE), keys.just_pressed(KeyCode::KeyE));
    // the first click only grabs the cursor
    let _ = cursor_grabbed;
    set(SLOT_ATTACK, mouse.pressed(MouseButton::Left), mouse.just_pressed(MouseButton::Left));
    if stick.length() > 1.0 {
        stick = stick.normalize();
    }
    for g in gamepads {
        let s = g.left_stick();
        if s.length() > stick.length() {
            stick = s;
        }
        set(SLOT_JUMP_ROLL, g.pressed(GamepadButton::South), g.just_pressed(GamepadButton::South));
        set(SLOT_SPECIAL, g.pressed(GamepadButton::North), g.just_pressed(GamepadButton::North));
        set(SLOT_ATTACK, g.pressed(GamepadButton::West), g.just_pressed(GamepadButton::West));
        set(SLOT_CANCEL, g.pressed(GamepadButton::East), g.just_pressed(GamepadButton::East));
    }
    // camera relative: right = (-fz, fx) for a camera looking along (fx, fz)
    let right = Vec2::new(-cam_fwd.y, cam_fwd.x);
    let w = cam_fwd * stick.y + right * stick.x;
    // world (X, Z) -> fight plane (x, y) = (X, -Z), then undo the arena rotation
    let (sr, cr) = fight_rot().sin_cos();
    let (px, py) = (w.x, -w.y);
    inp.stick = (px * cr + py * sr, -px * sr + py * cr);
    inp
}

/// Clearance (m) the fighters' centres keep from blocked cells [G].
const KONG_MARGIN: f32 = 1.8;
const REX_MARGIN: f32 = 2.4;

#[allow(clippy::too_many_arguments)]
fn kong_fight(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    arena: Res<Arena>,
    mut ctl: ResMut<KongCtl>,
    mut jack: Query<(&mut Player, &mut Transform)>,
    mut respawn: EventReader<crate::hud::RespawnAll>,
    breakables: Option<Res<crate::breakable::Breakables>>,
) {
    let gate_intact = breakables.as_ref().is_some_and(|b| !b.broken.is_empty() && !b.is_broken("porte")) && arena.level.is_some() && !crate::scene::swamp();
    let c = &mut *ctl;
    let dt = time.delta_secs().min(0.1);
    c.t += dt;
    c.frame_events.clear();
    if respawn.read().count() > 0 {
        c.respawn_requested = true;
    }
    if std::env::var("KK_FREEZE").is_ok() {
        return;
    }
    if let Some((x, z)) = c.pending_jack.take() {
        let (x, z) = match (&c.farena, x.is_nan()) {
            (Some(fa), true) => (fa.jack.x, fa.jack.z),
            _ => (x, z),
        };
        if let Ok((mut p, mut tf)) = jack.single_mut() {
            let y = ground_y(&arena, c.center, x, z, c.center.y);
            tf.translation = Vec3::new(x, y, z);
            let d = c.center - tf.translation;
            p.yaw = (-d.x).atan2(-d.z);
            p.pitch = 0.0;
        }
    }
    if !c.inited {
        c.inited = true;
        // Jack starts at the arena's vantage point looking at the fight
        if let (Ok((mut p, mut tf)), Some(fa)) = (jack.single_mut(), &c.farena) {
            // run the vantage through Jack's own collision so he is not shoved off it later
            let jp = arena.settle(fa.jack, 0.6);
            tf.translation = jp;
            p.yaw = fa.jack_yaw;
            p.pitch = 0.05;
        } else if let Ok((mut p, tf)) = jack.single_mut() {
            let d = c.center - tf.translation;
            p.yaw = (-d.x).atan2(-d.z);
        }
    }
    let grabbed = windows
        .single()
        .map(|w| w.cursor_options.grab_mode != bevy::window::CursorGrabMode::None)
        .unwrap_or(false);
    let input = if c.player_control {
        let mut i = read_pad(&keys, &mouse, &gamepads, c.cam_fwd, grabbed);
        // the field has edges and pillars: the stick is cut when the 5 m ahead is not free (the slide below
        // does the exact clamping)
        let s = Vec2::new(i.stick.0, i.stick.1);
        if s.length() > 0.2 {
            let w = c.kong_world();
            let sw = c.world((i.stick.0, i.stick.1), 0.0) - c.world((0.0, 0.0), 0.0);
            let d = Vec2::new(sw.x, sw.z).normalize_or_zero() * 5.0;
            let free = match &c.farena {
                Some(fa) => fa.clearance(w.x + d.x, w.z + d.y) >= 1.6,
                None => open_ground(&arena, c, w.x + d.x, w.z + d.y),
            };
            if !free {
                i.stick = (0.0, 0.0);
            }
        }
        i
    } else {
        c.brain.think(&c.fight, dt)
    };
    c.input = input;
    // F8: a new fight (Kong and the rex back at their marks); nothing restarts by itself
    if std::mem::take(&mut c.respawn_requested) {
        c.seed = c.seed.wrapping_add(1);
        c.fight = new_fight(c.seed);
        c.brain = KongBrain::new(c.seed);
        c.finished_at = None;
        c.over_at = None;
        c.rex_state_prev = None;
        c.rex_choice = None;
        c.gate_phase = 0;
        c.post_busy = false;
        c.kong_yaw = yaw_of(c.fight.kong.facing);
        c.rex_yaw = yaw_of(c.fight.rex.facing);
        c.frame_events.push(FightEvent::Anim { actor: Actor::Kong, id: 0, speed: 1.0 });
        return;
    }
    if c.fight.over == Some(Actor::Kong) && c.over_at.is_some() && !c.player_control {
        gate_smash(c, dt, gate_intact);
    }
    if !c.fight.is_finished() {
        let (k0, r0) = (c.kong_world(), c.rex_world());
        let evs = c.fight.step(dt, &input);
        // collide-and-slide against the free cells: neither fighter enters a pillar, rock or bank
        if let Some(fa) = &c.farena {
            let (k1, r1) = (c.kong_world(), c.rex_world());
            let k = fa.slide(Vec2::new(k0.x, k0.z), Vec2::new(k1.x, k1.z), KONG_MARGIN);
            let r = fa.slide(Vec2::new(r0.x, r0.z), Vec2::new(r1.x, r1.z), REX_MARGIN);
            if (k - Vec2::new(k1.x, k1.z)).length() > 0.02 || (r - Vec2::new(r1.x, r1.z)).length() > 0.02 {
                c.held_frames += 1;
            }
            c.min_clear_kong = c.min_clear_kong.min(fa.clearance(k.x, k.y));
            c.min_clear_rex = c.min_clear_rex.min(fa.clearance(r.x, r.y));
            c.fight.kong.pos = c.plane(Vec3::new(k.x, 0.0, k.y));
            c.fight.rex.pos = c.plane(Vec3::new(r.x, 0.0, r.y));
        }
        let t = c.t;
        for e in evs {
            if matches!(e, FightEvent::FightOver { .. }) && c.over_at.is_none() {
                c.over_at = Some(t);
            }
            c.log.push((t, e.clone()));
            c.frame_events.push(e);
        }
        if c.fight.is_finished() {
            c.finished_at = Some(c.t);
        }
    }
}

/// Marker bit on the gate punch's `KongSwing` anim (screenshots / logs tell it from fight blows).
pub const GATE_SWING: u32 = 0x1_0000;

/// After the victory Kong walks to the courtyard gate (the ODE blocks Jack's way south, its level
/// trigger `LD_03E_Activate_ODE_Porte` stands in front of it) and punches it open; the blow's
/// `KongSwing` is what `breakable.rs` reacts to, exactly as for a blow during the fight [G scripted walk].
fn gate_smash(c: &mut KongCtl, dt: f32, intact: bool) {
    use kk_mechanics::kong::combat::{ANIM_IDLE, ANIM_PUNCH_A};
    let Some((front, centre)) = crate::breakable::approach_point("porte") else {
        c.gate_phase = 3;
        return;
    };
    c.gate_t += dt;
    match c.gate_phase {
        0 => {
            if !intact {
                c.gate_phase = 3;
                c.post_busy = false;
                return;
            }
            c.post_busy = true;
            // let the victory roar finish
            if c.gate_t > 0.5 {
                c.gate_phase = 1;
                c.gate_t = 0.0;
                c.frame_events.push(FightEvent::Anim { actor: Actor::Kong, id: ANIM_WALK, speed: 1.0 });
            }
        }
        1 => {
            let target = c.plane(front);
            let p = c.fight.kong.pos;
            let d = (target.0 - p.0, target.1 - p.1);
            let l = (d.0 * d.0 + d.1 * d.1).sqrt();
            if l < 0.4 || c.gate_t > 12.0 {
                let to = c.plane(centre);
                c.fight.kong.facing = (to.1 - c.fight.kong.pos.1).atan2(to.0 - c.fight.kong.pos.0);
                c.gate_phase = 2;
                c.gate_t = 0.0;
                c.frame_events.push(FightEvent::Anim { actor: Actor::Kong, id: ANIM_PUNCH_A, speed: 1.0 });
            } else {
                let step = (4.5 * dt).min(l);
                c.fight.kong.pos = (p.0 + d.0 / l * step, p.1 + d.1 / l * step);
                c.fight.kong.facing = d.1.atan2(d.0);
            }
        }
        2 => {
            // the punch's hit window opens at its recovered event frame
            let hit = kk_mechanics::kong::anims::action(ANIM_PUNCH_A).and_then(|a| a.hit_start60()).unwrap_or(20.0) / 60.0;
            if c.gate_t >= hit && c.gate_t - dt < hit {
                let (pos, facing) = (c.fight.kong.pos, c.fight.kong.facing);
                let e = FightEvent::KongSwing { anim: ANIM_PUNCH_A | GATE_SWING, pos, facing };
                let t = c.t;
                c.log.push((t, e.clone()));
                c.frame_events.push(e);
            }
            if c.gate_t > 3.6 {
                c.gate_phase = 3;
                c.post_busy = false;
                c.frame_events.push(FightEvent::Anim { actor: Actor::Kong, id: ANIM_IDLE, speed: 1.0 });
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Kong presentation
// ---------------------------------------------------------------------------------------------

fn play_clip(
    assets: &KongAssets,
    name: &str,
    speed: f32,
    fade: f32,
    repeat: bool,
    rp: &mut RigPlayer,
    player: &mut AnimationPlayer,
    tr: &mut AnimationTransitions,
) -> bool {
    let Some(full) = assets.rig.find(name) else { return false };
    let node = assets.rig.nodes[full];
    let active = tr.play(player, node, std::time::Duration::from_secs_f32(fade));
    active.set_speed(speed);
    if repeat {
        active.repeat();
    }
    active.replay();
    rp.current = full.to_string();
    true
}

#[allow(clippy::too_many_arguments)]
fn apply_kong(
    time: Res<Time>,
    arena: Res<Arena>,
    mut ctl: ResMut<KongCtl>,
    assets: Res<KongAssets>,
    mut roots: Query<&mut Transform, With<KongRoot>>,
    mut scenes: Query<(&mut RigPlayer, &KongBones, &mut KongAnim), With<KongSceneRoot>>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    gts: Query<&GlobalTransform>,
) {
    let Ok(mut tf) = roots.single_mut() else { return };
    let Ok((mut rp, bones, mut ka)) = scenes.single_mut() else { return };
    let Ok((mut player, mut tr)) = anim.get_mut(bones.player) else { return };
    let c = &mut *ctl;
    let dt = time.delta_secs().min(0.1);
    // transform
    let kp = c.fight.kong.pos;
    let w = c.world(kp, 0.0);
    let ty = ground_y(&arena, c.center, w.x, w.z, c.kong_y);
    c.kong_y += (ty - c.kong_y) * (12.0 * dt).min(1.0);
    let target = yaw_of(c.fight.kong.facing);
    c.kong_yaw = turn_toward(c.kong_yaw, target, 16.0 * dt);
    // jaw-break: the paired clips (Kong 0xe6/0xe7/0xe8, rex 0x37/0x38) are authored on one root, so
    // Kong's root sits on the rex's root with the rex's axis (k_ETAT_finish) [C]
    if is_finish_anim(ka.cur_id) {
        c.kong_yaw = target;
        c.kong_y = c.rex_y;
    }
    tf.translation = Vec3::new(w.x, c.kong_y, w.z);
    tf.rotation = Quat::from_rotation_y(c.kong_yaw);
    let pos = tf.translation;
    c.kong_speed = Vec2::new(pos.x - c.kong_pos_prev.x, pos.z - c.kong_pos_prev.z).length() / dt.max(1e-4);
    c.kong_pos_prev = pos;
    if let Ok(g) = gts.get(bones.head) {
        c.kong_head = g.translation();
    }
    // clips from the fight's anim events
    let events: Vec<FightEvent> = c.frame_events.clone();
    for e in &events {
        match e {
            FightEvent::Anim { actor: Actor::Kong, id, speed } => {
                start_action(c, &assets, *id, *speed, &mut ka, &mut rp, &mut player, &mut tr);
            }
            FightEvent::FinisherSuccess => {
                start_action(c, &assets, 0xe8, 1.0, &mut ka, &mut rp, &mut player, &mut tr);
            }
            _ => {}
        }
    }
    // the mash clip 0xe7 follows the same cursor as the rex's 0x37 (`fn@0x00425cb0(kong, cursor/len)`) [C]
    if ka.cur_id == 0xe7 {
        if let (Some(f), Some(&node)) = (c.fight.kong.finisher.as_ref(), assets.rig.nodes.get(&rp.current)) {
            if f.won_t.is_none() && f.mash.elapsed > 0.0 {
                let d = assets.rig.durations.get(&rp.current).copied().unwrap_or(1.0);
                let u = (f.mash.progress / kk_mechanics::kong::fight::FINISH_ANIM_LEN).clamp(0.0, 1.0);
                if let Some(a) = player.animation_mut(node) {
                    a.set_speed(0.0);
                    a.seek_to(u * d * 0.999);
                }
            }
        }
    }
    // sequences (recover tails, death): next clip when the current one ended
    if !ka.queue.is_empty() {
        if let Some(&node) = assets.rig.nodes.get(&rp.current) {
            let done = player.animation(node).map_or(true, |a| a.is_finished());
            if done {
                if let Some((next, speed)) = ka.queue.pop_front() {
                    play_clip(&assets, &next, speed, 0.06, false, &mut rp, &mut player, &mut tr);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn start_action(
    c: &mut KongCtl,
    assets: &KongAssets,
    id: u32,
    speed: f32,
    ka: &mut KongAnim,
    rp: &mut RigPlayer,
    player: &mut AnimationPlayer,
    tr: &mut AnimationTransitions,
) {
    let rid = resolve_id(id);
    let Some(act) = assets.actions.get(&rid) else {
        if !c.missing_ids.contains(&id) {
            c.missing_ids.push(id);
        }
        return;
    };
    ka.cur_id = id;
    ka.queue.clear();
    let first = &act.clips[0];
    // fit the clip to the fight's [G] phase length: attacks are timed by the simulation, not by the clip
    let info = anim_info(id);
    let dur = assets.rig.duration(first);
    let mut scale = 1.0;
    if !act.looping && info.len < 1.0e8 && info.len > 0.0 && dur > 0.0 {
        scale = (dur * FPS / info.len).clamp(0.55, 2.4);
    }
    let fade = if act.looping { 0.18 } else { 0.07 };
    if play_clip(assets, first, speed * scale, fade, act.looping, rp, player, tr) {
        c.played.push((c.t, id, rp.current.clone()));
        for n in act.clips.iter().skip(1) {
            ka.queue.push_back((n.clone(), speed));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Rex presentation: the fight's KT rex drives the rex entity
// ---------------------------------------------------------------------------------------------

/// Rex clip for a KT state [G]: the Kong level only carries idle / run / bite / bite_down / roar-ish tracks in
/// the original (see `trex_kong_actions.json`); the other clips of `trex_inplace.glb` fill the remaining states.
fn rex_clip_for(st: KtState, mv: Option<RexMove>, moving: bool) -> (&'static str, bool, f32) {
    match st {
        KtState::Charge => ("run_c__rex_002", true, 1.0),
        KtState::Attaque => match mv {
            Some(RexMove::Sweep) => ("other__rex_093", false, 0.85),
            Some(RexMove::Tail) => ("turn_b__rex_050", false, 1.2),
            _ => ("bite__rex_039", false, 0.75),
        },
        KtState::Cri => ("roar__rex_010", false, 1.35),
        KtState::Paf => ("hurt__rex_011", false, 1.4),
        KtState::Derap => ("other__rex_064", false, 1.0),
        KtState::KoAuSol => ("fall_b__rex_060", false, 1.0),
        KtState::Projectile | KtState::Chute => ("hurt_d__rex_014", true, 1.6),
        KtState::Grabbed | KtState::Choppe => ("hurt_b__rex_012", true, 1.0),
        KtState::Finish | KtState::IFinish => ("bite_down__rex_008", false, 1.0),
        KtState::Mort => ("death__rex_070", false, 1.0),
        _ => {
            if moving {
                ("walk_b__rex_005", true, 1.7)
            } else {
                ("idle__rex_000", true, 1.0)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_rex(
    time: Res<Time>,
    arena: Res<Arena>,
    rigs: Res<Rigs>,
    mut ctl: ResMut<KongCtl>,
    mut rex: Query<(&mut Rex, &mut Transform), (Without<KongRoot>, Without<Player>)>,
    mut scenes: Query<&mut RigPlayer, With<RexScene>>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Ok((mut r, mut tf)) = rex.single_mut() else { return };
    let c = &mut *ctl;
    let dt = time.delta_secs().min(0.1);
    let st = c.fight.rex.state();
    if c.rex_state_prev != Some(st) {
        c.rex_state_prev = Some(st);
        c.rex_enter_t = c.t;
    }
    let in_state = c.t - c.rex_enter_t;
    // position and facing
    let p = c.fight.rex.pos;
    let w = c.world(p, 0.0);
    let ty = ground_y(&arena, c.center, w.x, w.z, c.rex_y);
    c.rex_y += (ty - c.rex_y) * (12.0 * dt).min(1.0);
    let target = yaw_of(c.fight.rex.facing);
    c.rex_yaw = turn_toward(c.rex_yaw, target, 10.0 * dt);
    let mut lift = 0.0;
    match st {
        KtState::Grabbed => lift = 2.2,
        KtState::Projectile => lift = 3.5 * (std::f32::consts::PI * (in_state / 1.1).min(1.0)).sin(),
        _ => {}
    }
    tf.translation = Vec3::new(w.x, c.rex_y + lift, w.z);
    tf.rotation = Quat::from_rotation_y(c.rex_yaw);
    let sync = Vec2::new(tf.translation.x - w.x, tf.translation.z - w.z).length();
    c.max_sync_err = c.max_sync_err.max(sync);
    let pv = Vec2::new(p.0, p.1);
    let speed = (pv - c.rex_pos_prev).length() / dt.max(1e-4);
    c.rex_pos_prev = pv;
    c.rex_moving = speed > 0.8 && speed < 200.0;
    c.rex_speed = if speed < 200.0 { speed } else { 0.0 };
    // state for the Jack-level components (sound / fx events read these)
    r.speed = speed.min(20.0);
    r.yaw = c.rex_yaw;
    r.noticed = true;
    r.hp = (c.fight.rex.life() / REX_MAX_LIFE * r.max_hp).max(0.0);
    let was = std::mem::discriminant(&r.state);
    r.state = match st {
        KtState::Cri => match r.state {
            RexState::Roar { t } => RexState::Roar { t: t + dt },
            _ => RexState::Roar { t: 0.0 },
        },
        KtState::Attaque => RexState::Bite { t: in_state, resolved: true },
        KtState::Charge => RexState::Chase,
        KtState::Mort => match r.state {
            RexState::Dead { t } => RexState::Dead { t: t + dt },
            _ => RexState::Dead { t: 0.0 },
        },
        _ if c.rex_moving && matches!(st, KtState::FightKong | KtState::Attente) => RexState::Chase,
        _ => RexState::Idle,
    };
    r.gait = if st == KtState::Charge { crate::spec::REX_RUN } else { crate::spec::REX_WALK };
    let _ = was;
    // clips. With trex_kt.glb the rex plays its own KT clips by id: one-shots come from the fight's
    // `Anim` events (paf / KO sequence / attacks / roar / finisher / death), loops from the state.
    let Ok(mut rp) = scenes.single_mut() else { return };
    let Ok((mut player, mut tr)) = anim.get_mut(rp.player) else { return };
    let rig = &rigs.rex;
    let has_kt = rig.find("kt_0x00").is_some();
    let moving = c.rex_moving && matches!(st, KtState::FightKong | KtState::Attente | KtState::FightAnn | KtState::FightCibleHauteur);
    if has_kt {
        let events: Vec<FightEvent> = c.frame_events.clone();
        for e in &events {
            if let FightEvent::Anim { actor: Actor::Rex, id, speed } = e {
                rex_play_kt(c, rig, *id, *speed, &mut rp, &mut player, &mut tr);
            }
        }
        let choice = (st, None, moving);
        if c.rex_choice != Some(choice) {
            c.rex_choice = Some(choice);
            if let Some((id, speed)) = rex_state_loop(st, moving, speed_of(c)) {
                rex_play_kt(c, rig, id, speed, &mut rp, &mut player, &mut tr);
            }
        } else if moving || st == KtState::Charge {
            // gait speed follows the rex's real speed (walk 4.72 m/s, charge 24.8 m/s clip speeds [C])
            if let Some(&node) = rig.nodes.get(&rp.current) {
                if let Some(a) = player.animation_mut(node) {
                    let (_, sp) = rex_state_loop(st, moving, speed_of(c)).unwrap_or((0, 1.0));
                    a.set_speed(sp);
                }
            }
        }
        // the jaw-break lock clip 0x37 is driven by the mash cursor (`fn@0x00425cb0(rex, cursor/len)`) [C]
        if c.rex_kt_id == Some(0x37) {
            if let (Some(f), Some(&node)) = (c.fight.kong.finisher.as_ref(), rig.nodes.get(&rp.current)) {
                if f.won_t.is_none() {
                    let d = rig.durations.get(&rp.current).copied().unwrap_or(1.0);
                    let u = (f.mash.progress / kk_mechanics::kong::fight::FINISH_ANIM_LEN).clamp(0.0, 1.0);
                    if let Some(a) = player.animation_mut(node) {
                        a.set_speed(0.0);
                        a.seek_to(u * d * 0.999);
                    }
                }
            }
        }
        // queued follow-up clips
        if !c.rex_queue.is_empty() {
            if let Some(&node) = rig.nodes.get(&rp.current) {
                if player.animation(node).map_or(true, |a| a.is_finished()) {
                    if let Some(next) = c.rex_queue.pop_front() {
                        if let Some(&n2) = rig.nodes.get(&next) {
                            let a = tr.play(&mut player, n2, std::time::Duration::from_secs_f32(0.05));
                            a.replay();
                            rp.current = next.clone();
                            let t = c.t;
                            c.rex_clips.push((t, next));
                        }
                    }
                }
            }
        }
        return;
    }
    // fallback without trex_kt.glb: the 03E clip names [G]
    let choice = (st, c.fight.rex.last_move, moving);
    if c.rex_choice != Some(choice) {
        c.rex_choice = Some(choice);
        let (clip, repeat, speed_mul) = rex_clip_for(st, choice.1, choice.2);
        if let Some(full) = rig.find(clip) {
            let node = rig.nodes[full];
            let fade = if repeat { 0.25 } else { 0.1 };
            let active = tr.play(&mut player, node, std::time::Duration::from_secs_f32(fade));
            active.set_speed(speed_mul);
            if repeat {
                active.repeat();
            }
            active.replay();
            rp.current = full.to_string();
            let t = c.t;
            c.rex_clips.push((t, full.to_string()));
        }
    }
}

fn speed_of(c: &KongCtl) -> f32 {
    let pv = Vec2::new(c.fight.rex.pos.0, c.fight.rex.pos.1);
    let _ = pv;
    c.rex_speed
}

/// KT loops the fight states play by themselves [C ids]: attente 0, walk 1 (4.72 m/s), charge 5
/// (24.8 m/s), derap 0x92, grabbed 0x2c, chute 0x17, choppe 0x50, I_Finish 0x73, JumpAttak 0x29.
fn rex_state_loop(st: KtState, moving: bool, speed: f32) -> Option<(u32, f32)> {
    Some(match st {
        KtState::FightKong | KtState::Attente | KtState::FightAnn | KtState::FightCibleHauteur => {
            if moving {
                (0x01, (speed / 4.72).clamp(0.6, 2.0))
            } else {
                (0x00, 1.0)
            }
        }
        // clip speed byte 48/64: 18.6 m/s at that rate [C]
        KtState::Charge => (0x05, (speed / 24.8).clamp(0.4, 1.4)),
        KtState::Derap => (0x92, 1.0),
        KtState::Grabbed => (0x2c, 1.0),
        KtState::Chute => (0x17, 1.0),
        KtState::Choppe => (0x50, 1.0),
        KtState::IFinish => (0x73, 1.0),
        KtState::JumpAttak => (0x29, 1.0),
        _ => return None,
    })
}

/// Play the rex's KT clip `id` (`kt_0xNN`); loops for the locomotion / lying ids, clip chains for
/// the multi-clip actions (0x33 = two clips) and the death (0x15 then 0x1c, `KT_ETAT_mort`) [C].
fn rex_play_kt(
    c: &mut KongCtl,
    rig: &crate::anim::Rig,
    id: u32,
    speed: f32,
    rp: &mut RigPlayer,
    player: &mut AnimationPlayer,
    tr: &mut AnimationTransitions,
) {
    let name = format!("kt_0x{id:02x}");
    let Some(full) = rig.find(&name).map(String::from) else {
        if !c.missing_ids.contains(&(0x1000 | id)) {
            c.missing_ids.push(0x1000 | id);
        }
        return;
    };
    let node = rig.nodes[&full];
    let looping = matches!(id, 0x00 | 0x01 | 0x03 | 0x05 | 0x3c | 0x2c);
    // hit reactions cut in fast, locomotion blends
    let fade = match id {
        0x64..=0x6c | 0x33 | 0x20 => 0.05,
        _ if looping => 0.2,
        _ => 0.1,
    };
    let active = tr.play(player, node, std::time::Duration::from_secs_f32(fade));
    // event clips play at their kit speed byte (b2/64, e.g. tail 0xe at 0.75) [C]
    let base = if matches!(id, 0x01 | 0x05) { 1.0 } else { kk_mechanics::kong::vrex::kt_anim_speed_byte(id) / 64.0 };
    active.set_speed(speed * base);
    if looping {
        active.repeat();
    }
    active.replay();
    rp.current = full.clone();
    c.rex_kt_id = Some(id);
    c.rex_queue.clear();
    let second = format!("{full}_1");
    if rig.nodes.contains_key(&second) {
        c.rex_queue.push_back(second);
    }
    if id == 0x15 {
        if let Some(n) = rig.find("kt_0x1c") {
            c.rex_queue.push_back(n.to_string());
        }
    }
    let t = c.t;
    c.rex_clips.push((t, full));
}

// ---------------------------------------------------------------------------------------------
// Pelvis lock: the in-place clips are authored relative to different origins (kong_asset_findings.md
// section 3), so the pelvis is pinned horizontally over the fight position.
// ---------------------------------------------------------------------------------------------

fn is_finish_anim(id: u32) -> bool {
    matches!(id, 0xe6 | 0xe7 | 0xe8)
}

fn pelvis_lock(
    kong: Query<(Entity, &KongBones, &KongAnim), With<KongSceneRoot>>,
    gts: Query<&GlobalTransform>,
    mut tfs: Query<&mut Transform, With<KongSceneRoot>>,
) {
    let Ok((scene, bones, ka)) = kong.single() else { return };
    if is_finish_anim(ka.cur_id) {
        // the jaw-break clips place Kong's body relative to the shared root: no pelvis pinning
        if let Ok(mut t) = tfs.get_mut(scene) {
            t.translation = Vec3::ZERO;
        }
        return;
    }
    let (Ok(p), Ok(s)) = (gts.get(bones.pelvis), gts.get(scene)) else { return };
    let rel = s.affine().inverse().transform_point3(p.translation());
    if let Ok(mut t) = tfs.get_mut(scene) {
        t.translation = Vec3::new(-rel.x, 0.0, -rel.z);
    }
}

// ---------------------------------------------------------------------------------------------
// Jack watching: the first-person camera follows the fight (batch b7, KK_KONG_WATCH=1)
// ---------------------------------------------------------------------------------------------

fn jack_watch(ctl: Res<KongCtl>, arena: Res<Arena>, time: Res<Time>, mut players: Query<(&mut Player, &mut Transform)>) {
    // once the player has switched to Kong and back, Jack is the player's again: no auto drift / aim
    if !ctl.watch || ctl.player_control || !ctl.switches.is_empty() {
        return;
    }
    let Ok((mut p, mut tf)) = players.single_mut() else { return };
    let (k, r) = (ctl.kong_world(), ctl.rex_world());
    // keep the spectating Jack within sight of the fight: drift toward it along open ground
    let mid = k.lerp(r, 0.5);
    let flat = Vec2::new(mid.x - tf.translation.x, mid.z - tf.translation.z);
    if flat.length() > 20.0 {
        let step = flat.normalize() * 6.0 * time.delta_secs();
        let (nx, nz) = (tf.translation.x + step.x, tf.translation.z + step.y);
        if open_ground(&arena, &ctl, nx, nz) {
            let y = ground_y(&arena, ctl.center, nx, nz, tf.translation.y);
            tf.translation = Vec3::new(nx, y, nz);
        }
    }
    let mut aim = k.lerp(r, 0.5) + Vec3::Y * 3.2;
    // Kong walking to / smashing the gate: watch Kong and the gate
    if (1..=3).contains(&ctl.gate_phase) {
        if let Some((_, g)) = crate::breakable::approach_point("porte") {
            aim = k.lerp(g, 0.5) + Vec3::Y * 2.5;
        }
    }
    let eye = tf.translation + Vec3::Y * p.eye;
    let to = aim - eye;
    p.yaw = (-to.x).atan2(-to.z);
    p.pitch = (to.y / Vec2::new(to.x, to.z).length().max(0.1)).atan().clamp(-0.5, 0.5);
}

// ---------------------------------------------------------------------------------------------
// Logging for the batches
// ---------------------------------------------------------------------------------------------

fn kong_sample(
    mut ctl: ResMut<KongCtl>,
    jack: Query<(&Transform, &Visibility), With<Player>>,
    cam: Query<&GlobalTransform, With<MainCam>>,
) {
    let (Ok((jt, jv)), Ok(cg)) = (jack.single(), cam.single()) else { return };
    let cam_pos = cg.translation();
    let d_jack = cam_pos.distance(jt.translation + Vec3::Y * crate::spec::EYE_STAND);
    let d_kong = cam_pos.distance(ctl.kong_world() + Vec3::Y * 3.0);
    let t = ctl.t;
    if std::env::var("KK_KONG_DEBUG").is_ok() && (t * 2.0).floor() != ((t - 0.034) * 2.0).floor() {
        info!(
            "kong dbg t={:.2} ctl={} kong={:?} rex={:?} cam={:?} head={:?} speed={:.1} kong_anim={:#x} phase={:?} rexst={:?} d(kong,rex)={:.1}",
            t, ctl.player_control, ctl.kong_world(), ctl.rex_world(), cam_pos, ctl.kong_head, ctl.kong_speed,
            ctl.fight.kong.anim, ctl.fight.kong.phase, ctl.fight.rex.state(), ctl.fight.dist()
        );
    }
    let pc = ctl.player_control;
    ctl.samples.push((t, pc, *jv == Visibility::Hidden, d_jack, d_kong));
    if !pc && !ctl.switches.is_empty() {
        ctl.jack_pos_end = Some(jt.translation);
    }
}

/// Screenshots of the key moments of a Kong batch, triggered by the fight's own events (the fight is
/// deterministic, but its clock starts when the rigs are ready, not at the batch's t = 0).
#[derive(Clone, Copy)]
enum Trig {
    KongHitClass(u32),
    KongHitAnim(u32),
    Ev(fn(&FightEvent) -> bool),
}

const SHOT_PLAN: &[(&str, Trig, f32)] = &[
    ("punch", Trig::KongHitAnim(0x17), 0.10),
    ("punch2", Trig::KongHitAnim(0x1d), 0.10),
    ("downward", Trig::KongHitAnim(0x1f), 0.10),
    ("counter", Trig::KongHitAnim(0x16), 0.10),
    ("rex_charge", Trig::Ev(|e| matches!(e, FightEvent::RexAttack { kind: RexMove::Charge })), 0.55),
    ("rex_bite", Trig::Ev(|e| matches!(e, FightEvent::RexAttack { kind: RexMove::Bite })), 0.65),
    ("grab", Trig::Ev(|e| matches!(e, FightEvent::GrabStart)), 0.55),
    ("grab_strike", Trig::Ev(|e| matches!(e, FightEvent::GrabStrike)), 0.35),
    ("throw", Trig::Ev(|e| matches!(e, FightEvent::Throw { .. })), 0.40),
    ("pound", Trig::Ev(|e| matches!(e, FightEvent::PoundStart)), 1.10),
    ("fury", Trig::Ev(|e| matches!(e, FightEvent::FuryShout { .. })), 0.35),
    ("ko", Trig::Ev(|e| matches!(e, FightEvent::KoStart { .. })), 0.35),
    ("finisher", Trig::Ev(|e| matches!(e, FightEvent::FinisherStart)), 2.2),
    ("finisher_win", Trig::Ev(|e| matches!(e, FightEvent::FinisherSuccess)), 0.5),
    ("victory_pound", Trig::Ev(|e| matches!(e, FightEvent::VictoryPound)), 0.9),
    ("victory_roar", Trig::Ev(|e| matches!(e, FightEvent::VictoryRoar)), 0.7),
    ("gate_smash", Trig::Ev(|e| matches!(e, FightEvent::KongSwing { anim, .. } if *anim & GATE_SWING != 0)), 0.35),
    ("gate_open", Trig::Ev(|e| matches!(e, FightEvent::KongSwing { anim, .. } if *anim & GATE_SWING != 0)), 2.6),
];

fn kong_shots(mut commands: Commands, mut ctl: ResMut<KongCtl>) {
    let Some(name) = batch_name() else { return };
    if !(name.contains("kong_fight") || name.contains("kong_cinema") || name.contains("swamp_fight")) {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("KK_BATCH_OUT").unwrap_or_else(|_| "batch_out".into()));
    let c = &mut *ctl;
    let t = c.t;
    let evs = c.frame_events.clone();
    // the swamp batch also films a frame every 3 s of the fight (clock starts with the first fight step)
    if name.contains("swamp_fight") && t > 1.0 {
        let slot = (t / 3.0).floor() as u32;
        if slot != c.last_slot && !c.fight.is_finished() {
            c.last_slot = slot;
            let file = format!("{name}_t{:03}.png", slot * 3);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(out.join(&file)));
            c.shots.push(file);
        }
    }
    for (i, (label, trig, delay)) in SHOT_PLAN.iter().enumerate() {
        if c.shot_done[i] {
            continue;
        }
        if c.shot_due[i].is_none() {
            let hit = evs.iter().any(|e| match (trig, e) {
                (Trig::KongHitClass(k), FightEvent::Hit { attacker: Actor::Kong, class, .. }) => class == k,
                (Trig::KongHitAnim(a), FightEvent::Hit { attacker: Actor::Kong, anim, .. }) => anim == a,
                (Trig::Ev(f), e) => f(e),
                _ => false,
            });
            if hit {
                c.shot_due[i] = Some(t + delay);
            }
        }
        if let Some(due) = c.shot_due[i] {
            if t >= due {
                c.shot_done[i] = true;
                let file = format!("{name}_{label}.png");
                info!("shot {file} at fight t {t:.3}");
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(out.join(&file)));
                c.shots.push(file);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Small HUD: who you are, life/fury
// ---------------------------------------------------------------------------------------------

#[derive(Component)]
struct KongHud;

fn kong_hud_update(mut commands: Commands, ctl: Res<KongCtl>, mut q: Query<&mut Text, With<KongHud>>, mut spawned: Local<bool>) {
    if !*spawned {
        *spawned = true;
        commands.spawn((
            KongHud,
            Text::new(""),
            TextFont { font_size: 18.0, ..default() },
            TextColor(Color::srgba(0.95, 0.95, 0.85, 0.9)),
            Node { position_type: PositionType::Absolute, top: Val::Px(10.0), left: Val::Px(14.0), ..default() },
        ));
        return;
    }
    let Ok(mut t) = q.single_mut() else { return };
    let f = &ctl.fight;
    let who = if ctl.player_control { "KONG" } else { "JACK (Kong is AI)" };
    **t = format!(
        "{who}    [Tab / Select] switch\nKong {:>3.0}/{:.0}   Rex {:>3.0}/{:.0}{}",
        f.kong.life,
        f.kong.max_life,
        f.rex.life().max(0.0),
        REX_MAX_LIFE,
        if f.kong.fury.is_active() { "   FURY" } else { "" }
    );
}

fn kong_hud_vis(ctl: Res<KongCtl>, mut q: Query<&mut Visibility, With<KongHud>>) {
    let show = batch_name().is_none() && std::env::var("KK_SURVEY").is_err();
    let _ = &ctl;
    for mut v in &mut q {
        *v = if show { Visibility::Visible } else { Visibility::Hidden };
    }
}

// ---------------------------------------------------------------------------------------------
// Batch checks (called from batch.rs)
// ---------------------------------------------------------------------------------------------

fn chk(name: &str, expected: Value, actual: Value, pass: bool) -> Value {
    json!({ "check": name, "expected": expected, "actual": actual, "pass": pass })
}

fn clip_played(c: &KongCtl, sub: &str) -> bool {
    c.played.iter().any(|p| p.2.contains(sub))
}

fn id_played(c: &KongCtl, ids: &[u32]) -> bool {
    c.played.iter().any(|p| ids.contains(&p.1))
}

pub fn batch_checks(name: &str, c: &KongCtl, marks: &[(&'static str, f32)]) -> Vec<Value> {
    let mut v = vec![];
    let phase = |p: Phase| c.has(|e| matches!(e, FightEvent::KongPhase { phase } if *phase == p));
    let rexmove = |m: RexMove| c.has(|e| matches!(e, FightEvent::RexAttack { kind } if *kind == m));
    v.push(chk("no Kong clip id without a clip (kong_actions.json)", json!([]), json!(c.missing_ids), c.missing_ids.is_empty()));
    v.push(chk("the fight plane stays glued to the rex entity (max XZ error m)", json!("<0.01"), json!(c.max_sync_err), c.max_sync_err < 0.01));
    let swamp07d = crate::scene::swamp() && !crate::scene::marsh05c();
    if name.contains("kong_fight") || name.contains("kong_cinema") || name.contains("swamp_fight") {
        v.push(chk("V-Rex dies (KT mort)", json!("RexDied + state Mort"), json!(c.fight.rex.state() == KtState::Mort), c.has(|e| matches!(e, FightEvent::RexDied)) && c.fight.rex.state() == KtState::Mort));
        v.push(chk("Kong survives", json!(0), json!(c.count(|e| matches!(e, FightEvent::KongDied))), !c.has(|e| matches!(e, FightEvent::KongDied))));
        v.push(chk("fight is over with Kong the winner", json!("Kong"), json!(format!("{:?}", c.fight.over)), c.fight.over == Some(Actor::Kong)));
        for (label, p) in [
            ("punch 1 (0x17/0x19/0x1b)", Phase::Punch1),
            ("punch 2 (0x1d)", Phase::Punch2),
            ("repel (0xac)", Phase::Repel),
            ("downward strike (0x1f)", Phase::Downward),
            ("counter lunge / shoulder strike (0x16)", Phase::CounterLunge),
            ("side-step dodge (6/7)", Phase::DodgeSide),
            ("chest pound (0xab)", Phase::ChestPound),
        ] {
            // the 07D swamp rex (life 50/50/25) dies before the demo brain's shoulder strike (mechanics sim:
            // 0 of 40 seeds, `print_seed_coverage`): not required there
            let ok = phase(p) || (p == Phase::CounterLunge && swamp07d);
            v.push(chk(&format!("Kong move: {label}"), json!(">=1"), json!(phase(p)), ok));
        }
        for (label, ok) in [
            ("blow landed on the rex", c.has(|e| matches!(e, FightEvent::Hit { attacker: Actor::Kong, victim: Actor::Rex, .. }))),
            ("rex attack avoided by the side-step", c.has(|e| matches!(e, FightEvent::HitAvoided { .. }))),
            ("grab", c.has(|e| matches!(e, FightEvent::GrabStart))),
            ("grab strike", c.has(|e| matches!(e, FightEvent::GrabStrike))),
            ("throw + impact", c.has(|e| matches!(e, FightEvent::Throw { .. })) && c.has(|e| matches!(e, FightEvent::ThrowImpact { .. }))),
            ("fury start + shout", c.has(|e| matches!(e, FightEvent::FuryStart { .. })) && c.has(|e| matches!(e, FightEvent::FuryShout { .. }))),
            ("rex knocked out", c.has(|e| matches!(e, FightEvent::KoStart { .. }))),
            ("jaw-break finisher start + success", c.has(|e| matches!(e, FightEvent::FinisherStart)) && c.has(|e| matches!(e, FightEvent::FinisherSuccess))),
            ("victory pound + roar", c.has(|e| matches!(e, FightEvent::VictoryPound)) && c.has(|e| matches!(e, FightEvent::VictoryRoar))),
            ("Kong was hit by the rex (stun)", c.has(|e| matches!(e, FightEvent::Hit { attacker: Actor::Rex, .. })) || c.has(|e| matches!(e, FightEvent::KongStunned { .. }))),
        ] {
            v.push(chk(&format!("Kong game behaviour: {label}"), json!(true), json!(ok), ok));
        }
        let small_arena = c.farena.as_ref().is_some_and(|f| f.best_clearance < 12.5);
        for (label, m) in [("charge", RexMove::Charge), ("sweep", RexMove::Sweep), ("tail", RexMove::Tail), ("bite", RexMove::Bite)] {
            // the sim starts a charge only from 20 m (10 m with Kong turned away): arenas narrower than ~24 m cannot reach it
            // tail needs Kong at the rex's side at mid range (choose_attack): one seeded AI fight may not
            // produce it since the knock-back keeps the rex in front; sweep covers the side swing then
            let ok = rexmove(m) || (m == RexMove::Charge && small_arena) || (m == RexMove::Tail && rexmove(RexMove::Sweep));
            v.push(chk(&format!("rex move seen: {label}"), json!(true), json!(rexmove(m)), ok));
        }
        let skip_clip = |l: &str| l == "lunge" && swamp07d;
        for (label, sub) in [
            ("run", "run__kong_016"),
            ("punch", "punch_"),
            ("punch2", "punch2__kong_032"),
            ("repel", "repel__kong_155"),
            ("downward", "downward__kong_034"),
            ("lunge", "lunge__kong_025"),
            ("side-step", "jump__kong_"),
            ("grab strike", "grab_strike__kong_145"),
            ("chest pound", "chest_pound__kong_154"),
            ("roar", "roar__kong_157"),
            ("finisher", "finisher_jaw__kong_"),
        ] {
            let ok = clip_played(c, sub) || skip_clip(label);
            v.push(chk(&format!("Kong clip played by action id: {label}"), json!(sub), json!(clip_played(c, sub)), ok));
        }
        // the sim only starts a charge from 20 m (10 m with Kong turned away): the 07D pool is ~23 m across, so the swamp run
        // cannot reach it and the charge clip is not required there
        let kt = c.rex_clips.iter().any(|x| x.1.starts_with("kt_0x"));
        let need: Vec<&str> = if kt {
            // KT ids: roar 0x24/0x6e, KO fall 0x16 + lying 0x3c + get-up 0x1e, jaw-break 0x37 + 0x38,
            // hit reactions 0x64..0x6c / 0x33 [C]
            let mut n = vec!["kt_0x37", "kt_0x38"];
            if !small_arena {
                n.push("kt_0x05");
            }
            n
        } else if small_arena {
            vec!["roar__rex_010", "death__rex_070", "bite_down__rex_008"]
        } else {
            vec!["run_c__rex_002", "roar__rex_010", "death__rex_070", "bite_down__rex_008"]
        };
        let ko_seen = !kt || c.rex_clips.iter().any(|x| x.1.starts_with("kt_0x16") || x.1.starts_with("kt_0x3c") || x.1.starts_with("kt_0x26"));
        let rex_ok = ko_seen && need.iter().all(|n| c.rex_clips.iter().any(|x| x.1.contains(n)));
        v.push(chk("rex clips follow the KT state (charge, roar, jaw-break, death)", json!(need), json!(c.rex_clips.iter().map(|x| x.1.clone()).collect::<Vec<_>>()), rex_ok));
        if kt {
            let paf = c.rex_clips.iter().any(|x| ["kt_0x6", "kt_0x33"].iter().any(|p| x.1.starts_with(p)));
            v.push(chk("rex plays its hit reactions (fn@0x0055a020 paf clips)", json!(true), json!(paf), paf));
            let roar = c.rex_clips.iter().any(|x| x.1.starts_with("kt_0x24") || x.1.starts_with("kt_0x6e"));
            v.push(chk("rex roar clip 0x24 / 0x6e", json!(true), json!(roar), roar));
        }
        v.push(chk("Jack stayed visible while the AI fought", json!(true), json!(c.samples.iter().all(|s| !s.2)), c.samples.iter().all(|s| !s.2)));
        v.push(chk("water splashes spawned (feet + impacts)", json!(">=40"), json!(c.stats.splashes), c.stats.splashes >= 40));
        v.push(chk("Kong footsteps detected from the animated feet/knuckles", json!(">=6"), json!(c.stats.footsteps), c.stats.footsteps >= 6));
        v.push(chk("rex footsteps splash", json!(">=2"), json!(c.stats.rex_footsteps), c.stats.rex_footsteps >= 2));
        // one flash per damaging blow: the low-life swamp rexes take fewer blows before the KO
        let min_flash = if crate::scene::swamp() { 5 } else { 8 };
        v.push(chk("impact flashes", json!(format!(">={min_flash}")), json!(c.stats.flashes), c.stats.flashes >= min_flash));
        v.push(chk("camera shakes requested by the fight", json!(">=4"), json!(c.stats.shakes), c.stats.shakes >= 4));
        v.push(chk("screenshots of the key moments", json!(">=12"), json!(c.shots.len()), c.shots.len() >= 12));
        if c.farena.is_some() {
            v.push(chk(
                "fight stays on open ground: both fighters keep their clearance from pillars / rocks / banks",
                json!({"kong": format!(">={KONG_MARGIN}"), "rex": format!(">={REX_MARGIN}")}),
                json!({"kong": c.min_clear_kong, "rex": c.min_clear_rex, "held_frames": c.held_frames}),
                c.min_clear_kong >= KONG_MARGIN - 0.05 && c.min_clear_rex >= REX_MARGIN - 0.05,
            ));
            let fa = c.farena.as_ref().unwrap();
            v.push(chk(
                "fight centre is open ground (largest empty circle >= 6 m)",
                json!(">=6"),
                json!({"centre": [fa.center.x, fa.center.z], "clearance": fa.best_clearance, "axis_deg": fa.rot.to_degrees()}),
                fa.best_clearance >= 6.0,
            ));
        }
        if c.cam.frames > 0 {
            v.push(chk(
                "Kong camera never ends up behind a solid (ray from Kong's head to the camera stays clear)",
                json!(0),
                json!({"blocked_frames": c.cam.blocked, "pull_in_frames": c.cam.hits, "frames": c.cam.frames, "closest_m": c.cam.min_dist}),
                c.cam.blocked == 0 && c.cam.min_dist > 2.5,
            ));
        }
    }
    if name.contains("kong_player") {
        let mark = |m: &str| marks.iter().find(|x| x.0 == m).map(|x| x.1);
        let sw_on = c.switches.iter().find(|s| s.1).map(|s| s.0);
        let sw_off = c.switches.iter().find(|s| !s.1).map(|s| s.0);
        v.push(chk("Tab switches to Kong and back (two switches)", json!([true, false]), json!(c.switches.iter().map(|s| s.1).collect::<Vec<_>>()), c.switches.len() == 2 && c.switches[0].1 && !c.switches[1].1));
        let (a, b) = (sw_on.unwrap_or(f32::MAX), sw_off.unwrap_or(f32::MAX));
        let ctl_samples: Vec<_> = c.samples.iter().filter(|s| s.0 > a + 0.1 && s.0 < b - 0.05).collect();
        v.push(chk("Jack is hidden for the whole time the player is Kong", json!(true), json!({"samples": ctl_samples.len()}), !ctl_samples.is_empty() && ctl_samples.iter().all(|s| s.1 && s.2)));
        // third person: after the camera has eased in (1.5 s) it stays 8..40 m from Kong and > 5 m from Jack
        let settled: Vec<_> = ctl_samples.iter().filter(|s| s.0 > a + 1.5).collect();
        let (dk_min, dk_max) = settled.iter().fold((f32::MAX, 0.0f32), |m, s| (m.0.min(s.4), m.1.max(s.4)));
        let dj_min = settled.iter().fold(f32::MAX, |m, s| m.min(s.3));
        // (the camera may pull in toward Kong when a ruin wall is behind it: clip_ray keeps >= 1.5 m)
        v.push(chk("camera is Kong's third-person camera (3..40 m from Kong, > 5 m from Jack's eye)", json!("3..40 / >5"), json!({"kong": [dk_min, dk_max], "jack_min": dj_min}), !settled.is_empty() && dk_min > 3.0 && dk_max < 40.0 && dj_min > 5.0));
        let after: Vec<_> = c.samples.iter().filter(|s| s.0 > b + 0.15).collect();
        v.push(chk("after switching back Jack is visible and the camera is on his eye again", json!(true), json!({"samples": after.len(), "dist_min": after.iter().map(|s| s.3).fold(f32::MAX, f32::min), "dist_last": after.last().map(|s| s.3)}), !after.is_empty() && after.iter().all(|s| !s.1 && !s.2) && after.last().is_some_and(|s| s.3 < 0.2)));
        v.push(chk("Kong clips played by action id: punch (0x17/0x19/0x1b)", json!(true), json!(id_played(c, &[0x17, 0x19, 0x1b])), id_played(c, &[0x17, 0x19, 0x1b]) && c.played.iter().any(|p| p.0 > a && [0x17, 0x19, 0x1b].contains(&p.1))));
        v.push(chk("Kong clips played by action id: dodge (0xe/6/7)", json!(true), json!(c.played.iter().filter(|p| p.0 > a && [0x0e, 6, 7].contains(&p.1)).count()), c.played.iter().any(|p| p.0 > a && [0x0e, 6, 7].contains(&p.1))));
        v.push(chk("Kong clips played by action id: special (Q) with no target -> repel 0xac", json!(true), json!(c.played.iter().filter(|p| p.0 > a && p.1 == 0xac).map(|p| p.2.clone()).collect::<Vec<_>>()), c.played.iter().any(|p| p.0 > a && p.1 == 0xac)));
        v.push(chk("pad: run (0xc) while the stick is held", json!(true), json!(id_played(c, &[ANIM_RUN, 0x0c]) || clip_played(c, "run__kong_016") || clip_played(c, "walk__kong_014")), clip_played(c, "run__kong_016") || clip_played(c, "walk__kong_014")));
        v.push(chk("fight state did not restart at the switch (clock continuous)", json!(true), json!(c.fight.time), c.log.iter().filter(|(_, e)| matches!(e, FightEvent::RexState { .. })).count() > 0 && c.fight.time > b));
        let same = matches!((c.jack_pos_at_switch, c.jack_pos_end), (Some(a), Some(b)) if a.distance(b) < 0.01);
        v.push(chk("Jack is back at the position he was frozen at", json!(c.jack_pos_at_switch.map(|p| p.to_array())), json!(c.jack_pos_end.map(|p| p.to_array())), same));
        let _ = mark("start");
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Survey (debug): KK_SURVEY="x,y,z,yaw_deg,pitch_deg;..." KK_SURVEY_OUT=<dir> teleports Jack's eye to each
// pose, saves a screenshot and exits after the last one.
// ---------------------------------------------------------------------------------------------

pub fn survey(
    mut commands: Commands,
    mut state: Local<(usize, u32)>,
    mut players: Query<(&mut Player, &mut Transform, &mut Visibility)>,
    mut exit: EventWriter<AppExit>,
    mut ctl: ResMut<KongCtl>,
    mut clean: ResMut<CleanHud>,
) {
    let Ok(spec) = std::env::var("KK_SURVEY") else { return };
    clean.0 = true;
    let poses: Vec<[f32; 5]> = spec
        .split(';')
        .filter_map(|p| {
            let v: Vec<f32> = p.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (v.len() == 5).then(|| [v[0], v[1], v[2], v[3], v[4]])
        })
        .collect();
    let out = std::path::PathBuf::from(std::env::var("KK_SURVEY_OUT").unwrap_or_else(|_| "survey".into()));
    std::fs::create_dir_all(&out).ok();
    let Ok((mut p, mut tf, mut vis)) = players.single_mut() else { return };
    *vis = Visibility::Hidden;
    ctl.watch = false;
    ctl.pending_jack = None;
    if state.0 >= poses.len() {
        exit.write(AppExit::Success);
        return;
    }
    let mut pose = poses[state.0];
    // KK_SURVEY_REL=kong|rex|centre: x, z are offsets from that actor (y stays absolute height above its floor)
    let rel = match std::env::var("KK_SURVEY_REL").as_deref() {
        Ok("kong") => Some(ctl.kong_world()),
        Ok("rex") => Some(ctl.rex_world()),
        Ok("centre") => Some(ctl.center),
        _ => None,
    };
    if let Some(r) = rel {
        pose[0] += r.x;
        pose[1] += r.y;
        pose[2] += r.z;
    }
    if matches!(std::env::var("KK_SURVEY_REL").as_deref(), Ok("kongf") | Ok("rexf")) {
        // camera in the actor's frame (x forward, z right), looking at its head / chest
        let rex = std::env::var("KK_SURVEY_REL").as_deref() == Ok("rexf");
        let (a, yaw) = if rex { (ctl.rex_world(), ctl.rex_yaw) } else { (ctl.kong_world(), ctl.kong_yaw) };
        let f = Vec3::new(yaw.sin(), 0.0, yaw.cos());
        let r = Vec3::new(f.z, 0.0, -f.x);
        let eye = a + f * pose[0] + r * pose[2] + Vec3::Y * pose[1];
        let tgt = if rex { a + Vec3::Y * 3.0 } else { ctl.kong_head.lerp(a + Vec3::Y * 3.5, 0.3) };
        let d = tgt - eye;
        tf.translation = eye - Vec3::Y * p.eye;
        p.yaw = (-d.x).atan2(-d.z) + pose[3].to_radians();
        p.pitch = (d.y / Vec2::new(d.x, d.z).length().max(0.1)).atan() + pose[4].to_radians();
    } else {
        tf.translation = Vec3::new(pose[0], pose[1] - p.eye, pose[2]);
        p.yaw = pose[3].to_radians();
        p.pitch = pose[4].to_radians();
    }
    state.1 += 1;
    if state.1 == 19 {
        let (k, r) = (ctl.kong_world(), ctl.rex_world());
        info!(
            "survey {}: kong ({:.1}, {:.1}, {:.1}) yaw {:.0} head {:?}; rex ({:.1}, {:.1}, {:.1}) yaw {:.0}; centre {:?}",
            state.0, k.x, k.y, k.z, ctl.kong_yaw.to_degrees(), ctl.kong_head, r.x, r.y, r.z, ctl.rex_yaw.to_degrees(), ctl.center
        );
    }
    if state.1 == 20 {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(out.join(format!("s{:02}.png", state.0))));
    }
    if state.1 > 32 {
        state.0 += 1;
        state.1 = 0;
    }
}
