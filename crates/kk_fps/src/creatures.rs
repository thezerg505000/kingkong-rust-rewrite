//! Generic creature plugin: spawn a creature kind from the exported glbs, play its clips by action id
//! or label, and run the recovered AI on the species that have one.
//!
//! * Venatosaurus raptors (species 0xe) and compies (0x16) run `kk_mechanics::creatures::raptor::Raptor`
//!   (X02 / X01 / A01-A15): perception of Jack (wide cone 100 m + line of sight, narrow cone 10 m), the
//!   target chain, HESITE -> FIGHT chase, bite start 3 m / 60 deg, bite hit 2.5 m / 30 deg inside the armed
//!   window of the bite clip, the first bite on Jack is a GRAB (counter), later bites are plain wounds
//!   (flags 0x4004 / 0x1000), flinch classes from Jack's Colt/Tommy/Shotgun/Sniper bands (check_paf),
//!   hp 50 (compy 3), A_TERRE -> MORT 5 s -> FADE 10 s.
//! * Brontosaurus walks a waypoint loop; each foot closer than 3 m to Jack sends a paf 0x10 (X14).
//! * Everything else (raptor_kong, crab, Kong) is a prop: clips only.
//!
//! Jack is touched only through the public `Player::paf` API (wound model H01..H04).
//! Presentation choices (gait speeds, clip picks, turn rates) are [G] and marked in place.

use crate::anim::{GameState, Rig};
use crate::events::GunEvent;
use crate::player::{MainCam, Player};
use crate::spec::WEAPONS;
use crate::world::Arena;
use bevy::prelude::*;
use kk_mechanics::creatures::raptor::{self as rp, HitIn, Perceived, Raptor, RaptorEvent, RaptorInput, RaptorState, Reaction, Variant};
use rand::Rng;
use std::collections::HashMap;
use std::time::Duration;

// ---------------------------------------------------------------------------------------------
// Kinds
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Species {
    Raptor,
    Compy,
    Bronto,
    Prop,
}

pub struct KindDef {
    pub name: &'static str,
    pub glb: &'static str,
    pub rootmotion: &'static str,
    pub actions: &'static str,
    pub species: Species,
    /// scale at spawn (compy: random in COMPY_SCALE)
    pub scale: f32,
    /// bind-pose height of the glb (m), manifest.json / measured
    pub height_m: f32,
    pub display: &'static str,
    /// clip names for idle, walk, run, attack, hit, death ("" = none)
    pub clips: [&'static str; 6],
    /// name fragments of the bones whose lowest point must touch the ground
    pub feet: &'static [&'static str],
    /// metres between the foot bone and the sole at scale 1 [G]
    pub sole: f32,
}

pub const KINDS: &[KindDef] = &[
    KindDef {
        name: "raptor",
        glb: "creatures/raptor.glb",
        rootmotion: "creatures/raptor_rootmotion.json",
        actions: "creatures/raptor_actions.json",
        species: Species::Raptor,
        scale: 1.0,
        height_m: 2.525,
        display: "Venatosaurus (raptor)",
        clips: ["idle__raptor_000", "walk__raptor_001", "run__raptor_002", "bite__raptor_070", "hit_flinch__raptor_031", "knockback_death__raptor_017"],
        feet: &["Orteil", "Griffe"],
        sole: 0.0,
    },
    KindDef {
        name: "compy",
        glb: "creatures/compy.glb",
        rootmotion: "creatures/compy_rootmotion.json",
        actions: "creatures/compy_actions.json",
        species: Species::Compy,
        scale: 0.3,
        height_m: 2.483,
        display: "compy (galiminus) x0.3",
        clips: ["idle__compy_000", "walk__compy_001", "run__compy_002", "bite__compy_070", "hit_flinch__compy_031", "knockback_death__compy_017"],
        feet: &["Orteil", "Griffe"],
        sole: 0.0,
    },
    KindDef {
        name: "raptor_kong",
        glb: "creatures/raptor_kong.glb",
        rootmotion: "creatures/raptor_kong_rootmotion.json",
        actions: "creatures/raptor_kong_actions.json",
        species: Species::Prop,
        scale: 1.0,
        height_m: 2.525,
        display: "raptor (Kong-level bank)",
        clips: ["raptor_kong_000", "raptor_kong_001", "raptor_kong_002", "raptor_kong_070", "raptor_kong_031", "raptor_kong_017"],
        feet: &["Orteil", "Griffe"],
        sole: 0.0,
    },
    KindDef {
        name: "brontosaurus",
        glb: "creatures/brontosaurus.glb",
        rootmotion: "creatures/brontosaurus_rootmotion.json",
        actions: "creatures/brontosaurus_actions.json",
        species: Species::Bronto,
        scale: 1.0,
        height_m: 12.467,
        display: "brontosaurus",
        clips: ["idle_pose__brontosaurus_002", "walk__brontosaurus_000", "run__brontosaurus_001", "", "", ""],
        feet: &["PiedG", "PiedD", "MainG", "MainD"],
        sole: 0.0,
    },
    KindDef {
        name: "crab",
        glb: "creatures/crab.glb",
        rootmotion: "creatures/crab_rootmotion.json",
        actions: "",
        species: Species::Prop,
        scale: 1.0,
        height_m: 1.377,
        display: "crab / scorpion (untextured)",
        // unlabelled bank: clips 0..2 stand in for idle/walk/run [G]
        clips: ["crab_000", "crab_001", "crab_002", "", "", ""],
        feet: &["Patte"],
        sole: 0.0,
    },
    KindDef {
        name: "kong",
        glb: "kong/kong.glb",
        rootmotion: "kong/kong_rootmotion.json",
        actions: "kong/kong_actions.json",
        species: Species::Prop,
        scale: 1.0,
        height_m: 0.0,
        display: "Kong",
        clips: ["idle__kong_000", "walk__kong_014", "run__kong_016", "punch_l__kong_026", "hit__kong_040", "death__kong_251"],
        feet: &["Orteil"],
        sole: 0.0,
    },
];

pub const ACTIONS: [&str; 6] = ["idle", "walk", "run", "attack", "hit", "death"];

pub fn kind_index(name: &str) -> Option<usize> {
    KINDS.iter().position(|k| k.name == name)
}

// ---------------------------------------------------------------------------------------------
// Assets
// ---------------------------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct CreatureAssets {
    pub wanted: Vec<usize>,
    pub gltf: Vec<Handle<Gltf>>,
    pub rigs: Vec<Rig>,
    pub scenes: Vec<Handle<Scene>>,
    pub ready: Vec<bool>,
    /// clip -> root speed (m/s), from *_rootmotion.json
    pub speeds: Vec<HashMap<String, f32>>,
    /// AI animation id ("0x46") -> clip name, from *_actions.json
    pub actions: Vec<HashMap<String, String>>,
}

impl CreatureAssets {
    pub fn is_ready(&self, kind: usize) -> bool {
        self.ready.get(kind).copied().unwrap_or(false)
    }
    pub fn all_ready(&self) -> bool {
        self.wanted.iter().all(|k| self.is_ready(*k))
    }
    pub fn rig(&self, kind: usize) -> &Rig {
        &self.rigs[kind]
    }
    /// Clip name of a script action: a label (idle..death), a numeric bank index "#31", an AI animation id
    /// "0x46" (from *_actions.json) or an exact / prefix clip name.
    pub fn resolve(&self, kind: usize, id: &str) -> Option<String> {
        let rig = &self.rigs[kind];
        if let Some(i) = ACTIONS.iter().position(|a| *a == id) {
            let c = KINDS[kind].clips[i];
            if !c.is_empty() {
                return rig.find(c).map(String::from);
            }
            return None;
        }
        if let Some(n) = id.strip_prefix('#').and_then(|s| s.parse::<usize>().ok()) {
            let suffix = format!("_{n:03}");
            return rig.names.iter().find(|c| c.ends_with(&suffix)).cloned();
        }
        if id.starts_with("0x") {
            return self.actions[kind].get(id).and_then(|c| rig.find(c)).map(String::from);
        }
        rig.find(id).map(String::from)
    }
    pub fn clip_idx(&self, kind: usize, idx: usize) -> Option<String> {
        self.resolve(kind, &format!("#{idx}"))
    }
    pub fn speed_of(&self, kind: usize, clip: &str) -> f32 {
        self.speeds[kind].get(clip).copied().unwrap_or(0.0)
    }
}

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

fn read_json(path: &str) -> Option<serde_json::Value> {
    let txt = std::fs::read_to_string(crate::asset_dir().join(path)).ok()?;
    serde_json::from_str(&txt).ok()
}

fn start_loading(mut assets: ResMut<CreatureAssets>, server: Res<AssetServer>, mode: Res<CreatureMode>) {
    let n = KINDS.len();
    assets.gltf = vec![Handle::default(); n];
    assets.scenes = vec![Handle::default(); n];
    assets.rigs = (0..n).map(|_| Rig::default()).collect();
    assets.ready = vec![false; n];
    assets.speeds = vec![HashMap::new(); n];
    assets.actions = vec![HashMap::new(); n];
    assets.wanted = mode.kinds.clone();
    let wanted = assets.wanted.clone();
    for k in wanted {
        assets.gltf[k] = server.load(KINDS[k].glb);
        if let Some(serde_json::Value::Object(o)) = read_json(KINDS[k].rootmotion) {
            for (clip, e) in o {
                if let Some(s) = e.get("speed_mps").and_then(|s| s.as_f64()) {
                    assets.speeds[k].insert(clip, s as f32);
                }
            }
        }
        if !KINDS[k].actions.is_empty() {
            if let Some(serde_json::Value::Object(o)) = read_json(KINDS[k].actions) {
                for (id, e) in o {
                    if let Some(c) = e.get("clip").or_else(|| e.get("clip_name")).and_then(|c| c.as_str()) {
                        assets.actions[k].insert(id, c.to_string());
                    }
                }
            }
        }
    }
}

fn finish_loading(
    mut assets: ResMut<CreatureAssets>,
    gltfs: Res<Assets<Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    server: Res<AssetServer>,
) {
    let wanted = assets.wanted.clone();
    for k in wanted {
        if assets.ready[k] || !server.is_loaded_with_dependencies(assets.gltf[k].id()) {
            continue;
        }
        let Some(g) = gltfs.get(&assets.gltf[k]) else { continue };
        let mut rig = Rig::default();
        rig.gltf = assets.gltf[k].clone();
        build_rig(&mut rig, g, &clips, &mut graphs);
        info!("creature '{}' ready: {} clips", KINDS[k].name, rig.names.len());
        assets.scenes[k] = g.scenes[0].clone();
        assets.rigs[k] = rig;
        assets.ready[k] = true;
    }
}

// ---------------------------------------------------------------------------------------------
// Mode (which creatures exist)
// ---------------------------------------------------------------------------------------------

#[derive(Resource)]
pub struct CreatureMode {
    /// kinds to load
    pub kinds: Vec<usize>,
    /// spawn the 03E pack + compies once the assets are in
    pub slice: bool,
}

/// `KK_NO_CREATURES=1`, any `KK_BATCH` that is not a test-area batch (b1-b6 keep their numeric checks valid)
/// and `KK_AUTOTEST` switch the slice creatures off. The test area always loads every kind.
fn mode() -> CreatureMode {
    let all: Vec<usize> = (0..KINDS.len()).collect();
    if crate::testarea::active() {
        return CreatureMode { kinds: all, slice: false };
    }
    let off = std::env::var("KK_NO_CREATURES").map(|v| v != "0").unwrap_or(false)
        || std::env::var("KK_BATCH").is_ok_and(|b| !b.contains("slice"))
        || crate::scene::swamp()
        || std::env::var("KK_AUTOTEST").is_ok();
    if off {
        return CreatureMode { kinds: vec![], slice: false };
    }
    CreatureMode { kinds: vec![kind_index("raptor").unwrap(), kind_index("compy").unwrap()], slice: true }
}

// ---------------------------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------------------------

#[derive(Component)]
pub struct Creature {
    pub kind: usize,
    pub scale: f32,
    pub yaw: f32,
    pub speed: f32,
    pub label: String,
    pub clip: String,
    pub dead: bool,
    pub ground_calibrated: u32,
    pub scene: Entity,
    pub sole: f32,
}

#[derive(Component)]
pub struct CreatureRig {
    pub player: Entity,
    pub head: Option<Entity>,
    pub pelvis: Option<Entity>,
    pub feet: Vec<Entity>,
    /// every B_* bone (pose checks)
    pub bones: Vec<Entity>,
    /// (bone, radius at scale 1, is head)
    pub hit: Vec<(Entity, f32, bool)>,
    pub stomp: Vec<Entity>,
}

/// Creature has just been told to play a script clip: AI must not override it.
#[derive(Component)]
pub struct ScriptedClip;

#[derive(Component)]
pub struct RaptorAi {
    pub m: Raptor,
    pub compy: bool,
    pub home: Vec3,
    pub wp: Vec3,
    pub hits: Vec<HitIn>,
    pub hurt_me: f32,
    pub last_hit_dir: Vec3,
    pub knock: Vec3,
    pub knock_t: f32,
    pub clip_clock: f32,
    pub bite_clock: f32,
    pub last_seen: Vec3,
    pub orbit: f32,
    pub eat_target: Option<Entity>,
    pub want: (&'static str, bool),
    pub sink: f32,
    /// spears stuck in the body (`exec_check_javelin`: bleed per spear, max 3) — spears.rs keeps it
    pub spears: u32,
}

#[derive(Component)]
pub struct BrontoWalk {
    pub path: Vec<Vec3>,
    pub idx: usize,
    /// rising-edge latch per stomp bone
    pub inside: Vec<bool>,
}

/// Marks a dead raptor/compy as food for the others (rule 6 of the target chain).
#[derive(Component, Default)]
pub struct Corpse {
    pub eaten_t: f32,
    pub eaten: bool,
}

#[derive(Resource, Default)]
pub struct Corpses(pub Vec<(Entity, Vec3)>);

#[derive(Clone, Debug)]
pub enum CEv {
    Acquired { dist: f32 },
    State { from: RaptorState, to: RaptorState },
    Damaged(f32),
    Flinch(Reaction),
    BiteStart { dist: f32 },
    BiteHit { damage: f32, flags: u32, head_dist: f32 },
    Grab { head_dist: f32 },
    GrabKill,
    Died,
    Faded,
    Stomp { foot: usize, dist: f32, flags: u32 },
    Shot { dist: f32, damage: f32, head: bool, hp_after: f32 },
}

pub struct LogEntry {
    pub t: f32,
    pub who: Entity,
    pub ev: CEv,
}

#[derive(Resource, Default)]
pub struct CreatureLog(pub Vec<LogEntry>);

/// Hit spheres of the living creatures this frame (world centre, radius): weapons.rs stops a bullet's
/// world impact at a creature in front of it (the creature shot test spawns the flesh impact).
#[derive(Resource, Default)]
pub struct CreatureSpheres(pub Vec<(Vec3, f32)>);

fn publish_spheres(mut out: ResMut<CreatureSpheres>, q: Query<(&Creature, &CreatureRig, Option<&RaptorAi>)>, gts: Query<&GlobalTransform>) {
    out.0.clear();
    for (c, rig, ai) in &q {
        if c.dead || ai.is_some_and(|a| a.m.hp <= 0.0) {
            continue;
        }
        for (bone, r, _) in &rig.hit {
            if let Ok(g) = gts.get(*bone) {
                out.0.push((g.translation(), r * c.scale));
            }
        }
    }
}

/// Impact events produced by the creature shot test, forwarded as `GunEvent::Impact` (blood fx, sounds).
#[derive(Resource, Default)]
struct PendingImpacts(Vec<GunEvent>);

pub struct SpawnOpts {
    pub ai: bool,
    pub scale: Option<f32>,
    pub variant: Variant,
    pub path: Option<Vec<Vec3>>,
    pub label: Option<String>,
}

impl Default for SpawnOpts {
    fn default() -> Self {
        SpawnOpts { ai: true, scale: None, variant: Variant::Standard, path: None, label: None }
    }
}

/// Spawn one creature (the assets of `kind` must be ready). Returns the actor entity.
pub fn spawn_creature(commands: &mut Commands, assets: &CreatureAssets, kind: usize, pos: Vec3, yaw: f32, opts: SpawnOpts) -> Option<Entity> {
    if !assets.is_ready(kind) {
        return None;
    }
    let def = &KINDS[kind];
    let scale = opts.scale.unwrap_or_else(|| match def.species {
        Species::Compy => rand::thread_rng().gen_range(kk_mechanics::kong::ann::COMPY_SCALE.0..kk_mechanics::kong::ann::COMPY_SCALE.1),
        _ => def.scale,
    });
    let scene = commands.spawn((Name::new("CreatureScene"), SceneRoot(assets.scenes[kind].clone()), Transform::default())).id();
    let label = opts.label.unwrap_or_else(|| def.name.to_string());
    let mut e = commands.spawn((
        Name::new(format!("Creature {}", def.name)),
        Creature {
            kind,
            scale,
            yaw,
            speed: 0.0,
            label,
            clip: String::new(),
            dead: false,
            ground_calibrated: 0,
            scene,
            sole: def.sole,
        },
        Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(scale)),
        Visibility::default(),
    ));
    if opts.ai {
        match def.species {
            Species::Raptor | Species::Compy => {
                let compy = def.species == Species::Compy;
                let mut m = Raptor::new(if compy { Variant::Standard } else { opts.variant });
                if compy {
                    m.hp = kk_mechanics::kong::ann::COMPY_HP;
                }
                e.insert(RaptorAi {
                    m,
                    compy,
                    home: pos,
                    wp: pos,
                    hits: vec![],
                    hurt_me: 1e9,
                    last_hit_dir: Vec3::Z,
                    knock: Vec3::ZERO,
                    knock_t: 0.0,
                    clip_clock: 0.0,
                    bite_clock: 0.0,
                    last_seen: pos,
                    orbit: 1.0,
                    eat_target: None,
                    want: ("", true),
                    sink: 0.0,
                    spears: 0,
                });
            }
            Species::Bronto => {
                let path = opts.path.unwrap_or_else(|| default_bronto_path(pos, yaw));
                let n = 4;
                e.insert(BrontoWalk { path, idx: 1, inside: vec![false; n] });
            }
            Species::Prop => {}
        }
    }
    let id = e.id();
    commands.entity(id).add_child(scene);
    commands.entity(scene).observe(on_ready);
    Some(id)
}

/// Rectangular loop in front of the spawn point: 220 m ahead, 90 m across [G].
pub fn default_bronto_path(pos: Vec3, yaw: f32) -> Vec<Vec3> {
    let f = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let r = Vec3::new(f.z, 0.0, -f.x);
    vec![pos, pos + f * 220.0, pos + f * 220.0 + r * 90.0, pos + r * 90.0]
}

fn on_ready(
    trigger: Trigger<bevy::scene::SceneInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    names: Query<&Name>,
    meshes: Query<(), With<Mesh3d>>,
    players: Query<Entity, With<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    creatures: Query<&Creature>,
    assets: Res<CreatureAssets>,
) {
    let root = trigger.target();
    let Ok(actor) = parents.get(root).map(|p| p.parent()) else { return };
    let Ok(c) = creatures.get(actor) else { return };
    let def = &KINDS[c.kind];
    let mut rig = CreatureRig { player: root, head: None, pelvis: None, feet: vec![], bones: vec![], hit: vec![], stomp: vec![] };
    for e in children.iter_descendants(root) {
        if meshes.contains(e) {
            // skinned AABB is the bind pose: never cull
            commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
        }
        let Ok(n) = names.get(e) else { continue };
        let n = n.as_str();
        if !n.starts_with("B_") {
            continue;
        }
        rig.bones.push(e);
        let ends = |s: &str| n.ends_with(s);
        if ends("_Tete") {
            rig.head = Some(e);
        }
        if ends("_Bassin") || ends("_Corps01") {
            rig.pelvis = Some(e);
        }
        if def.feet.iter().any(|f| n.contains(f)) {
            rig.feet.push(e);
        }
        if def.species == Species::Bronto && (ends("PiedG") || ends("PiedD") || ends("MainG") || ends("MainD")) {
            rig.stomp.push(e);
        }
        if matches!(def.species, Species::Raptor | Species::Compy | Species::Prop) {
            // hit spheres, radius at scale 1 [G] sized to the skinned mesh
            let table: &[(&str, f32, bool)] = &[
                ("_Tete", 0.34, true),
                ("_Machoire", 0.2, true),
                ("_Cou01", 0.28, false),
                ("_Cou02", 0.28, false),
                ("_Torse", 0.5, false),
                ("_Ventre", 0.5, false),
                ("_Bassin", 0.5, false),
                ("_CuisseG", 0.3, false),
                ("_CuisseD", 0.3, false),
                ("_JambeG", 0.2, false),
                ("_JambeD", 0.2, false),
                ("_Queue01", 0.3, false),
                ("_Queue02", 0.22, false),
            ];
            if let Some((_, r, h)) = table.iter().find(|(s, _, _)| ends(s)) {
                rig.hit.push((e, *r, *h));
            }
        }
    }
    let Some(player) = children.iter_descendants(root).chain(std::iter::once(root)).find(|e| players.contains(*e)) else {
        error!("creature glb {} has no AnimationPlayer", def.glb);
        return;
    };
    rig.player = player;
    commands.entity(player).insert((
        AnimationGraphHandle(assets.rigs[c.kind].graph.clone()),
        AnimationTransitions::new(),
    ));
    commands.entity(actor).insert(rig);
}

// ---------------------------------------------------------------------------------------------
// Animation helper
// ---------------------------------------------------------------------------------------------

/// Cross-fade the creature to `clip` (exact clip name). Returns false when the clip does not exist.
pub fn play_clip(
    assets: &CreatureAssets,
    c: &mut Creature,
    pl: &mut AnimationPlayer,
    tr: &mut AnimationTransitions,
    clip: &str,
    fade: f32,
    repeat: bool,
    speed: f32,
    restart: bool,
) -> bool {
    let rig = &assets.rigs[c.kind];
    let Some(name) = rig.find(clip) else { return false };
    let node = rig.nodes[name];
    if c.clip == name && !restart {
        if let Some(a) = pl.animation_mut(node) {
            a.set_speed(speed);
        }
        return true;
    }
    let a = tr.play(pl, node, Duration::from_secs_f32(fade));
    a.set_speed(speed);
    if repeat {
        a.repeat();
    }
    if restart {
        a.replay();
    }
    c.clip = name.to_string();
    true
}

fn fwd(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

fn turn_to(cur: f32, want: f32, rate: f32, dt: f32) -> f32 {
    let mut d = (want - cur) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    cur + d.clamp(-rate * dt, rate * dt)
}

fn g(v: Vec3) -> [f32; 3] {
    // Bevy (x, y up, z) -> game frame (x, z, up): only relative geometry matters
    [v.x, v.z, v.y]
}

// ---------------------------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------------------------

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CreatureSet;

pub struct CreaturePlugin;

impl Plugin for CreaturePlugin {
    fn build(&self, app: &mut App) {
        let m = mode();
        info!("creatures: kinds {:?}, slice pack {}", m.kinds.iter().map(|k| KINDS[*k].name).collect::<Vec<_>>(), m.slice);
        app.insert_resource(m)
            .init_resource::<CreatureAssets>()
            .init_resource::<CreatureLog>()
            .init_resource::<Corpses>()
            .init_resource::<PendingImpacts>()
            .init_resource::<CreatureSpheres>()
            .add_systems(Update, publish_spheres.before(crate::weapons::WeaponsSet).run_if(in_state(GameState::Playing)))
            .add_systems(Startup, start_loading)
            .add_systems(Update, finish_loading)
            .add_systems(
                Update,
                (
                    calibrate_ground,
                    collect_corpses,
                    creature_shots,
                    raptor_ai,
                    separation,
                    bronto_walk,
                    flush_impacts,
                )
                    .chain()
                    .in_set(CreatureSet)
                    .after(crate::weapons::WeaponsSet)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(Update, spawn_slice_pack.run_if(in_state(GameState::Playing)));
    }
}

/// Put the lowest foot bone on the ground once the idle pose is up (the glbs are authored around the
/// pelvis, not the soles).
fn calibrate_ground(
    assets: Res<CreatureAssets>,
    mut q: Query<(&mut Creature, &CreatureRig, &Transform)>,
    gts: Query<&GlobalTransform>,
    mut scenes: Query<&mut Transform, Without<Creature>>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (mut c, rig, tf) in &mut q {
        // every creature starts in its idle clip (the AI and the scripts take over from there)
        if c.clip.is_empty() {
            if let (Some(idle), Ok((mut pl, mut tr))) = (assets.resolve(c.kind, "idle"), anim.get_mut(rig.player)) {
                play_clip(&assets, &mut c, &mut pl, &mut tr, &idle, 0.0, true, 1.0, false);
            }
        }
        if c.ground_calibrated >= 4 || rig.feet.is_empty() {
            continue;
        }
        c.ground_calibrated += 1;
        if c.ground_calibrated < 4 {
            continue;
        }
        let low = rig.feet.iter().filter_map(|f| gts.get(*f).ok()).map(|g| g.translation().y).fold(f32::MAX, f32::min);
        if low == f32::MAX {
            continue;
        }
        let err = low - tf.translation.y - c.sole * c.scale;
        if let Ok(mut st) = scenes.get_mut(c.scene) {
            st.translation.y -= err / c.scale;
        }
    }
}

fn collect_corpses(mut corpses: ResMut<Corpses>, q: Query<(Entity, &Transform, &Corpse)>) {
    corpses.0.clear();
    for (e, tf, c) in &q {
        if !c.eaten {
            corpses.0.push((e, tf.translation));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Jack's gunfire against creatures
// ---------------------------------------------------------------------------------------------

fn ray_sphere(o: Vec3, d: Vec3, c: Vec3, r: f32) -> Option<f32> {
    let oc = o - c;
    let b = oc.dot(d);
    let cc = oc.dot(oc) - r * r;
    let disc = b * b - cc;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    if t > 0.0 {
        Some(t)
    } else {
        let t2 = -b + disc.sqrt();
        (t2 > 0.0).then_some(t2)
    }
}

#[allow(clippy::too_many_arguments)]
fn creature_shots(
    mut gun: EventReader<GunEvent>,
    time: Res<Time>,
    arena: Res<Arena>,
    cam: Query<&GlobalTransform, With<MainCam>>,
    jack: Query<&Transform, (With<Player>, Without<Creature>)>,
    mut q: Query<(Entity, &Creature, &CreatureRig, &mut RaptorAi, &Transform)>,
    gts: Query<&GlobalTransform>,
    mut pend: ResMut<PendingImpacts>,
    mut log: ResMut<CreatureLog>,
) {
    let (Ok(cam), Ok(jtf)) = (cam.single(), jack.single()) else {
        gun.clear();
        return;
    };
    let origin = cam.translation();
    let right = cam.right().as_vec3();
    let up = cam.up().as_vec3();
    let mut rng = rand::thread_rng();
    for e in gun.read() {
        let GunEvent::Fired { w, dir, .. } = *e else { continue };
        let def = &WEAPONS[w];
        // same ray set as weapons.rs: the shotgun's recovered 25-ray grid, one ray for the others
        let dirs: Vec<Vec3> = if def.pellets() > 1 {
            kk_mechanics::weapons::shotgun_pattern(|lo, hi| rng.gen_range(lo.min(hi)..=lo.max(hi)))
                .into_iter()
                .map(|(ax, ay)| (dir + right * ax.tan() + up * ay.tan()).normalize())
                .collect()
        } else {
            vec![dir]
        };
        for d in dirs {
            let wall = arena.raycast(origin, d, def.range()).map(|h| h.0).unwrap_or(f32::MAX);
            // nearest creature sphere
            let mut best: Option<(f32, Entity, bool)> = None;
            for (ent, c, rig, ai, _) in q.iter() {
                if c.dead || ai.m.hp <= 0.0 {
                    continue;
                }
                for (bone, r, head) in &rig.hit {
                    let Ok(bg) = gts.get(*bone) else { continue };
                    if let Some(t) = ray_sphere(origin, d, bg.translation(), r * c.scale) {
                        if t <= def.range() && t < wall && best.map_or(true, |b| t < b.0) {
                            best = Some((t, ent, *head));
                        }
                    }
                }
            }
            let Some((t, ent, head)) = best else { continue };
            let Ok((_, c, _, mut ai, tf)) = q.get_mut(ent) else { continue };
            // damage band by the SQUARED distance between Jack's and the creature's object origins (fn@0x004150e0), G02
            let dist_sq = (tf.translation - jtf.translation).length_squared();
            let amount = kk_mechanics::weapons::damage_at_distance_sq(def.mech(), dist_sq, false) as f32;
            let hd = Vec3::new(d.x, 0.0, d.z).normalize_or_zero();
            ai.last_hit_dir = hd;
            ai.hurt_me = 0.0;
            ai.hits.push(HitIn {
                damage: amount,
                // bullet paf flag word 0x44 (bit 0x40 + bit 0x4), as rex.rs / check_paf [C]
                flags: 0x44,
                head,
                dir_z: d.y,
                from_jack: true,
                bite_leg_latch: false,
            });
            let point = origin + d * t;
            pend.0.push(GunEvent::Impact { pos: point, normal: -d, rex: true, damage: amount, dist: dist_sq.sqrt() });
            log.0.push(LogEntry { t: time.elapsed_secs(), who: ent, ev: CEv::Shot { dist: dist_sq.sqrt(), damage: amount, head, hp_after: -1.0 } });
            let _ = c;
        }
    }
}

fn flush_impacts(mut pend: ResMut<PendingImpacts>, mut out: EventWriter<GunEvent>) {
    for e in pend.0.drain(..) {
        out.write(e);
    }
}

// ---------------------------------------------------------------------------------------------
// Raptor / compy AI
// ---------------------------------------------------------------------------------------------

/// Gait speeds (m/s): walk and run clip root speeds of raptor.glb (raptor_rootmotion.json) [C data];
/// the compy runs at 0.8 of that [G] (gait tables of species 0x16 not separated, X01 gap).
const WALK: f32 = 3.2217;
const RUN: f32 = 6.1867;
const COMPY_RUN_K: f32 = 0.8;
/// speed smoothing (m/s^2) [G]; the V-Rex ramp is 1.5 (X04), X02 select_action smoothing not recovered
const ACCEL: f32 = 5.0;
const TURN_RATE: f32 = 5.0;

fn reaction_clip(r: RaptorState) -> usize {
    match r {
        RaptorState::PafSlide => 31,
        RaptorState::PafFall => 33,
        _ => 71, // PAF_FLY: recoil_back
    }
}

#[allow(clippy::too_many_arguments)]
fn raptor_ai(
    time: Res<Time>,
    arena: Res<Arena>,
    assets: Res<CreatureAssets>,
    corpses: Res<Corpses>,
    mut log: ResMut<CreatureLog>,
    mut jack: Query<(&mut Player, &mut Transform), Without<Creature>>,
    mut q: Query<(Entity, &mut Creature, &mut RaptorAi, &mut Transform, &CreatureRig), Without<Player>>,
    gts: Query<&GlobalTransform>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    mut commands: Commands,
    mut dead_feed: Query<&mut Corpse>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let Ok((mut p, mut jtf)) = jack.single_mut() else { return };
    let jack_pos = jtf.translation;
    let chest = jack_pos + Vec3::Y * 1.2;
    let eye = jack_pos + Vec3::Y * p.eye;
    let mut rng = rand::thread_rng();
    // eating: a corpse is consumed after 8 s of company [G]
    let mut eaters: Vec<Entity> = vec![];

    for (ent, mut c, mut ai, mut tf, rig) in &mut q {
        let kind = c.kind;
        let Ok((mut pl, mut tr)) = anim.get_mut(rig.player) else { continue };
        let Some(head_gt) = rig.head.and_then(|h| gts.get(h).ok()) else { continue };
        let head = head_gt.translation();
        let scale = c.scale;
        let f = fwd(c.yaw);
        let pos = tf.translation;
        let compy = ai.compy;
        let walk_clip = assets.resolve(kind, "walk").unwrap_or_default();
        let run_clip = assets.resolve(kind, "run").unwrap_or_default();
        ai.hurt_me += dt;
        ai.clip_clock += dt;

        // ---- perception ----
        let mut slots: Vec<Perceived> = Vec::with_capacity(4);
        let to_eye = eye - head;
        let dist_eye = to_eye.length();
        let los = dist_eye < 1e-3 || arena.raycast(head, to_eye / dist_eye, dist_eye - 0.3).is_none();
        let alive = p.alive();
        let wound_ratio = p.wounds.state.life_ratio();
        let mut flags = 0u32;
        let jc = g(chest);
        let hg = g(head);
        let fg = [f.x, f.z, 0.0];
        if alive {
            if rp::sees_human_wide(hg, fg, jc, los) {
                flags |= rp::flag::JACK | rp::flag::SEEN;
            }
            if rp::sees_in_cone(hg, fg, jc) && los {
                flags |= rp::flag::IN_CONE;
            }
            if ai.hurt_me < 10.0 {
                flags |= rp::flag::HURT_ME | rp::flag::BULLET;
            }
        } else {
            flags |= rp::flag::CORPSE;
        }
        if flags & rp::flag::SEEN != 0 {
            ai.last_seen = jack_pos;
        }
        slots.push(Perceived {
            actor: 1,
            class: 1,
            flags,
            dist: dist_eye,
            life_ratio: wound_ratio,
            pos: jc,
            reachable: true,
            changed: true,
            same_territory: true,
            dist_to_jack_sq: 0.0,
        });
        let mut corpse_ents = vec![None];
        for (ce, cp) in &corpses.0 {
            if *ce == ent {
                continue;
            }
            let d = Vec2::new(cp.x - pos.x, cp.z - pos.z).length();
            slots.push(Perceived {
                actor: ce.index(),
                class: 0xe,
                flags: rp::flag::CORPSE,
                dist: d,
                life_ratio: 0.0,
                pos: g(*cp + Vec3::Y * 0.3),
                reachable: d < 60.0,
                changed: true,
                same_territory: true,
                dist_to_jack_sq: (*cp - jack_pos).length_squared(),
            });
            corpse_ents.push(Some(*ce));
        }

        // ---- inputs of the state machine ----
        let st = ai.m.state;
        let reaction_len = assets.clip_idx(kind, reaction_clip(st)).map(|n| assets.rigs[kind].duration(&n)).unwrap_or(0.5).max(0.1);
        let bite_len = assets.clip_idx(kind, 70).map(|n| assets.rigs[kind].duration(&n)).unwrap_or(0.83);
        let anim_done = match st {
            RaptorState::PafSlide | RaptorState::PafFall | RaptorState::PafFly => ai.clip_clock >= reaction_len,
            RaptorState::Mord => ai.bite_clock >= bite_len,
            RaptorState::Grab => false,
            _ => false,
        };
        let bite_frame = (ai.bite_clock * 60.0) as u32;
        // compies do not grab: their bite is a plain damage bite (flags 0x1000) [G]
        let jack_target = !compy && ai.m.target.map_or(false, |i| i == 0);
        // raptors bite at the chest, compies at the legs (their head is 0.5 m up) [G target height]
        let bite_pt = if compy { g(jack_pos + Vec3::Y * 0.45) } else { jc };
        let bite_connects = alive && rp::bite_hits(hg, fg, bite_pt, 0.35);
        let at_wp = Vec2::new(ai.wp.x - pos.x, ai.wp.z - pos.z).length() < 1.2;
        let inp = RaptorInput {
            perceived: slots.clone(),
            jack_slot: Some(0),
            head: hg,
            forward: fg,
            scale_override: Some(scale),
            hits: std::mem::take(&mut ai.hits),
            spears: ai.spears,
            anim_done,
            bite_frame,
            bite_connects,
            bite_target_is_jack: jack_target,
            at_waypoint: at_wp,
            idle_expired_override: false,
        };
        let had_hits = !inp.hits.is_empty();
        let events = ai.m.step(&inp, dt);
        let _ = had_hits;

        // lost for good: drop the target after the search clamp + 4 s [G, the interest handle expires]
        if ai.m.state == RaptorState::Search && ai.m.sighting_age >= rp::SEARCH_AGE_CLAMP_S && ai.m.state_time > 6.0 {
            ai.m.target = None;
        }
        // compy waits 0.5 s after a bite instead of 3 s (X01)
        if compy && ai.m.wait > 0.5 {
            ai.m.wait = 0.5;
        }

        let mut restart_clip = false;
        for ev in &events {
            match *ev {
                RaptorEvent::TargetAcquired(i) => {
                    let d = if i == 0 { dist_eye } else { slots.get(i).map_or(0.0, |s| s.dist) };
                    log.0.push(LogEntry { t: now, who: ent, ev: CEv::Acquired { dist: d } });
                }
                RaptorEvent::StateChanged { from, to } => {
                    log.0.push(LogEntry { t: now, who: ent, ev: CEv::State { from, to } });
                    ai.clip_clock = 0.0;
                    restart_clip = true;
                    if to == RaptorState::Mord {
                        ai.bite_clock = 0.0;
                    }
                    if matches!(to, RaptorState::PafSlide | RaptorState::PafFall | RaptorState::PafFly) {
                        let k = rp::KNOCKBACK_SPEED * if compy { 0.75 } else { 1.0 };
                        ai.knock = ai.last_hit_dir * k;
                        ai.knock_t = 0.0;
                    }
                }
                RaptorEvent::Damaged(d) => log.0.push(LogEntry { t: now, who: ent, ev: CEv::Damaged(d) }),
                RaptorEvent::Flinch(r) => log.0.push(LogEntry { t: now, who: ent, ev: CEv::Flinch(r) }),
                RaptorEvent::BiteStarted => {
                    let d = Vec2::new(chest.x - head.x, chest.z - head.z).length();
                    log.0.push(LogEntry { t: now, who: ent, ev: CEv::BiteStart { dist: d } });
                }
                RaptorEvent::BiteHit { damage, kind: hk } => {
                    let flags = if compy {
                        kk_mechanics::kong::ann::COMPY_BITE_FLAGS
                    } else {
                        0x4000 | hk.flags()
                    };
                    p.paf(if compy { "bitten by a compy" } else { "bitten by a raptor" }, flags);
                    log.0.push(LogEntry { t: now, who: ent, ev: CEv::BiteHit { damage: if compy { kk_mechanics::kong::ann::COMPY_BITE_DAMAGE } else { damage }, flags, head_dist: head.distance(chest) } });
                }
                RaptorEvent::GrabStarted => {
                    // the grab hand-shake sends Jack the wound flag 0x4 (heavy), the hold ends with 0x204 [C X02, L mapping]
                    p.paf("grabbed by a raptor", 0x4);
                    log.0.push(LogEntry { t: now, who: ent, ev: CEv::Grab { head_dist: head.distance(chest) } });
                }
                RaptorEvent::GrabKill => {
                    p.paf("grabbed by a raptor", 0x204);
                    log.0.push(LogEntry { t: now, who: ent, ev: CEv::GrabKill });
                }
                RaptorEvent::Died => {
                    c.dead = true;
                    ai.knock = ai.last_hit_dir * rp::KNOCKBACK_SPEED * if compy { 0.75 } else { 1.0 };
                    ai.knock_t = 0.0;
                    ai.clip_clock = 0.0;
                    restart_clip = true;
                    commands.entity(ent).insert(Corpse::default());
                    log.0.push(LogEntry { t: now, who: ent, ev: CEv::Died });
                }
                RaptorEvent::Faded => {
                    log.0.push(LogEntry { t: now, who: ent, ev: CEv::Faded });
                    commands.entity(ent).despawn();
                }
            }
        }
        if matches!(ai.m.state, RaptorState::Mord) {
            ai.bite_clock += dt;
        }
        let st = ai.m.state;

        // ---- movement ----
        let tgt = ai.m.target.and_then(|i| slots.get(i)).copied();
        let tgt_pos = tgt.map(|t| Vec3::new(t.pos[0], pos.y, t.pos[1]));
        let run_speed = RUN * if compy { COMPY_RUN_K } else { 1.0 };
        let mut desired = 0.0;
        let mut face: Option<Vec3> = None;
        let mut gait_run = false;
        let mut clip_key: (&'static str, bool) = ("idle", true);
        let mut grab_attach = false;
        let mut move_dir: Option<Vec3> = None;
        match st {
            RaptorState::Attente | RaptorState::Lance => {
                if let Some(t) = tgt_pos {
                    face = Some(t);
                }
            }
            RaptorState::Vala => {
                if at_wp || ai.wp == ai.home && ai.wp.distance(pos) < 0.5 {
                    let a = rng.gen_range(0.0..std::f32::consts::TAU);
                    let r = rng.gen_range(5.0..9.0);
                    ai.wp = ai.home + Vec3::new(a.cos() * r, 0.0, a.sin() * r);
                }
                face = Some(ai.wp);
                desired = WALK;
                clip_key = ("walk", true);
            }
            RaptorState::Hesite => {
                if let Some(t) = tgt_pos {
                    face = Some(t);
                }
            }
            RaptorState::Search => {
                face = Some(ai.last_seen);
                desired = WALK;
                clip_key = ("walk", true);
                if Vec2::new(ai.last_seen.x - pos.x, ai.last_seen.z - pos.z).length() < 1.5 {
                    desired = 0.0;
                    clip_key = ("idle", true);
                }
            }
            RaptorState::Fight => {
                if let Some(t) = tgt_pos {
                    let to = Vec2::new(t.x - pos.x, t.z - pos.z);
                    let d = to.length();
                    face = Some(t);
                    let stop = if compy { 0.8 } else { 2.0 };
                    if ai.m.wait > 0.0 && !compy {
                        // post-bite wait ("requin"): keep a ring of 5.0 m around the target [C radius], facing it so it
                        // stays in the sight cone: walk in, back off, or strafe round [G speeds = clip root speeds]
                        let ring = rp::CIRCLE_RING_RADIUS;
                        let radial = d - ring;
                        let to_t = Vec3::new(to.x, 0.0, to.y).normalize_or(Vec3::Z);
                        if radial > 0.7 {
                            desired = WALK;
                            clip_key = ("walk", true);
                        } else if radial < -0.7 {
                            move_dir = Some(-to_t);
                            desired = 1.7;
                            clip_key = ("back", true);
                        } else {
                            let side = fwd(c.yaw).cross(Vec3::Y);
                            move_dir = Some(side);
                            desired = 2.3;
                            clip_key = ("strafeR", true);
                        }
                    } else if d > stop {
                        desired = run_speed;
                        gait_run = true;
                        clip_key = ("run", true);
                    }
                }
            }
            RaptorState::Mord => {
                if let Some(t) = tgt_pos {
                    face = Some(t);
                }
                clip_key = ("attack", false);
            }
            RaptorState::Grab => {
                grab_attach = true;
                clip_key = ("attack", true);
            }
            RaptorState::Devore => {
                if let Some(ce) = ai.m.target.and_then(|i| corpse_ents.get(i).copied().flatten()) {
                    ai.eat_target = Some(ce);
                }
                if let Some(t) = tgt_pos {
                    face = Some(t);
                    if Vec2::new(t.x - pos.x, t.z - pos.z).length() > 2.0 {
                        desired = WALK;
                        clip_key = ("walk", true);
                    } else {
                        clip_key = ("attack", true);
                        if let Some(ce) = ai.eat_target {
                            eaters.push(ce);
                        }
                    }
                }
            }
            RaptorState::PafSlide | RaptorState::PafFall | RaptorState::PafFly => {
                clip_key = ("hit", false);
            }
            RaptorState::ATerre | RaptorState::Mort | RaptorState::Fade => {
                clip_key = ("death", false);
            }
            _ => {}
        }
        let _ = gait_run;
        ai.orbit += dt;

        // speed smoothing, heading, translation
        let mut sp = c.speed;
        if sp < desired {
            sp = (sp + ACCEL * dt).min(desired);
        } else {
            sp = (sp - 2.0 * ACCEL * dt).max(desired);
        }
        c.speed = sp;
        if let Some(t) = face {
            let d = Vec2::new(t.x - pos.x, t.z - pos.z);
            if d.length() > 0.05 {
                c.yaw = turn_to(c.yaw, d.x.atan2(d.y), TURN_RATE, dt);
            }
        }
        let mut np = pos + move_dir.unwrap_or_else(|| fwd(c.yaw)) * sp * dt;
        if grab_attach {
            let away = Vec3::new(pos.x - jack_pos.x, 0.0, pos.z - jack_pos.z).normalize_or(Vec3::Z);
            np = jack_pos + away * 1.3;
            c.yaw = turn_to(c.yaw, (-away.x).atan2(-away.z), TURN_RATE * 2.0, dt);
        }
        // knockback (PAF_*: hit direction * 10, decaying over the reaction) and the death slide
        if ai.knock.length_squared() > 1e-4 {
            np += ai.knock * dt;
            ai.knock_t += dt;
            let life = if c.dead { 0.45 } else { 0.35 };
            ai.knock *= (1.0 - dt / life).max(0.0);
            if ai.knock_t > life * 2.0 {
                ai.knock = Vec3::ZERO;
            }
        }
        if st == RaptorState::Fade {
            ai.sink += dt;
        }
        if arena.level.is_some() {
            np = arena.move_to(pos, np, 0.5 * scale.max(0.4));
        } else {
            np.y = 0.0;
        }
        tf.translation = np;
        tf.rotation = Quat::from_rotation_y(c.yaw);

        // body vs Jack: push him out (no contact damage, X04) [slice glue, G]
        if alive && !c.dead {
            let d = Vec2::new(jack_pos.x - np.x, jack_pos.z - np.z);
            let min = 0.35 + 0.55 * scale.max(0.35);
            if d.length() < min && !grab_attach {
                let n = d.normalize_or(Vec2::X) * min;
                jtf.translation.x = np.x + n.x;
                jtf.translation.z = np.z + n.y;
            }
        }

        // ---- animation ----
        if c.dead && !matches!(st, RaptorState::ATerre | RaptorState::Mort | RaptorState::Fade) {
            clip_key = ("death", false);
        }
        let clip = match clip_key.0 {
            "attack" => assets.clip_idx(kind, 70),
            "hit" => assets.clip_idx(kind, reaction_clip(st)),
            "death" => assets.clip_idx(kind, 17),
            "back" => assets.clip_idx(kind, 16),
            "strafeL" => assets.clip_idx(kind, 19),
            "strafeR" => assets.clip_idx(kind, 20),
            "run" => Some(run_clip.clone()),
            "walk" => Some(walk_clip.clone()),
            _ => assets.resolve(kind, "idle"),
        };
        if let Some(clip) = clip {
            let base = if matches!(clip_key.0, "run" | "walk" | "back" | "strafeL" | "strafeR") { assets.speed_of(kind, &clip) * scale } else { 0.0 };
            let aspeed = if base > 0.0 { (sp / base).clamp(0.25, 6.0) } else { 1.0 };
            let restart = restart_clip && !clip_key.1;
            play_clip(&assets, &mut c, &mut pl, &mut tr, &clip, 0.15, clip_key.1, aspeed, restart);
        }
        ai.want = clip_key;
    }
    // corpses being eaten
    for ce in eaters {
        if let Ok(mut co) = dead_feed.get_mut(ce) {
            co.eaten_t += dt;
            if co.eaten_t > 8.0 {
                co.eaten = true;
            }
        }
    }
}

/// Keep raptors and compies from standing inside each other [G].
fn separation(mut q: Query<(&Creature, &mut Transform), With<RaptorAi>>) {
    let mut v: Vec<(Vec3, f32)> = q.iter().map(|(c, t)| (t.translation, 0.35 + 0.45 * c.scale.max(0.3))).collect();
    let n = v.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let d = Vec2::new(v[j].0.x - v[i].0.x, v[j].0.z - v[i].0.z);
            let min = v[i].1 + v[j].1;
            let l = d.length();
            if l < min && l > 1e-4 {
                let push = d / l * (min - l) * 0.5;
                v[i].0.x -= push.x;
                v[i].0.z -= push.y;
                v[j].0.x += push.x;
                v[j].0.z += push.y;
            }
        }
    }
    for ((_, mut t), (p, _)) in q.iter_mut().zip(v) {
        t.translation.x = p.x;
        t.translation.z = p.z;
    }
}

// ---------------------------------------------------------------------------------------------
// Brontosaurus
// ---------------------------------------------------------------------------------------------

/// Max yaw rate while following the loop [G]: a 35 m animal.
const BRONTO_TURN: f32 = 0.2;
/// Waypoint reached radius [G].
const BRONTO_WP_RADIUS: f32 = 8.0;

#[allow(clippy::too_many_arguments)]
fn bronto_walk(
    time: Res<Time>,
    assets: Res<CreatureAssets>,
    mut log: ResMut<CreatureLog>,
    mut jack: Query<&mut Player, Without<Creature>>,
    jack_tf: Query<&Transform, (With<Player>, Without<Creature>)>,
    mut q: Query<(Entity, &mut Creature, &mut BrontoWalk, &mut Transform, &CreatureRig), Without<Player>>,
    gts: Query<&GlobalTransform>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let (Ok(mut p), Ok(jtf)) = (jack.single_mut(), jack_tf.single()) else { return };
    let jpos = jtf.translation;
    for (ent, mut c, mut b, mut tf, rig) in &mut q {
        let Ok((mut pl, mut tr)) = anim.get_mut(rig.player) else { continue };
        let walk = assets.resolve(c.kind, "walk").unwrap_or_default();
        let speed = assets.speed_of(c.kind, &walk) * c.scale;
        let target = b.path[b.idx % b.path.len()];
        let pos = tf.translation;
        let d = Vec2::new(target.x - pos.x, target.z - pos.z);
        if d.length() < BRONTO_WP_RADIUS {
            b.idx = (b.idx + 1) % b.path.len();
        }
        c.yaw = turn_to(c.yaw, d.x.atan2(d.y), BRONTO_TURN, dt);
        c.speed = speed;
        tf.translation += fwd(c.yaw) * speed * dt;
        tf.rotation = Quat::from_rotation_y(c.yaw);
        play_clip(&assets, &mut c, &mut pl, &mut tr, &walk, 0.2, true, 1.0, false);
        // X14 pnjbronto_cb_afterblend: every foot within 3 m (dist^2 < 9) of Jack sends a paf 0x10 [C]
        for (i, foot) in rig.stomp.iter().enumerate() {
            let Ok(fg) = gts.get(*foot) else { continue };
            let dist = fg.translation().distance(jpos);
            let inside = dist * dist < kk_mechanics::kong::ann::BRONTO_STOMP_RADIUS.powi(2);
            if inside && !b.inside.get(i).copied().unwrap_or(false) && p.alive() {
                p.paf("trampled by a brontosaurus", kk_mechanics::kong::ann::BRONTO_STOMP_FLAGS);
                log.0.push(LogEntry { t: now, who: ent, ev: CEv::Stomp { foot: i, dist, flags: kk_mechanics::kong::ann::BRONTO_STOMP_FLAGS } });
            }
            if let Some(s) = b.inside.get_mut(i) {
                *s = inside;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 03E pack
// ---------------------------------------------------------------------------------------------

/// Small raptor pack behind Jack's start and a few compies to the east, in the 03E courtyard (positions read from
/// the level's ground map, [G] placement); the stand-in clearing gets the same relative layout.
fn spawn_slice_pack(
    mut done: Local<bool>,
    mode: Res<CreatureMode>,
    assets: Res<CreatureAssets>,
    arena: Res<Arena>,
    mut commands: Commands,
    mut respawn: EventReader<crate::hud::RespawnAll>,
    existing: Query<Entity, With<Creature>>,
) {
    // creatures are placed once; only F8 (RespawnAll) clears the field and places the pack again
    if respawn.read().count() > 0 && mode.slice {
        for e in &existing {
            commands.entity(e).despawn();
        }
        *done = false;
    }
    if *done || !mode.slice || !assets.all_ready() || assets.wanted.is_empty() {
        return;
    }
    *done = true;
    let rap = kind_index("raptor").unwrap();
    let com = kind_index("compy").unwrap();
    let j = arena.player_spawn;
    let (pack, compies): (Vec<Vec3>, Vec<Vec3>) = if arena.level.is_some() {
        (
            vec![Vec3::new(14.0, 0.0, -114.0), Vec3::new(19.0, 0.0, -118.0), Vec3::new(11.0, 0.0, -119.0)],
            vec![Vec3::new(56.0, 0.0, -100.0), Vec3::new(58.0, 0.0, -97.0), Vec3::new(54.0, 0.0, -96.0), Vec3::new(10.0, 0.0, -80.0)],
        )
    } else {
        (
            vec![j + Vec3::new(-30.0, 0.0, 28.0), j + Vec3::new(-26.0, 0.0, 32.0), j + Vec3::new(-34.0, 0.0, 33.0)],
            vec![j + Vec3::new(30.0, 0.0, -20.0), j + Vec3::new(33.0, 0.0, -17.0), j + Vec3::new(28.0, 0.0, -16.0)],
        )
    };
    let mut rng = rand::thread_rng();
    // KK_RAPTOR_AT=x,z: one more raptor there (batches / tests), facing Jack's start
    if let Some((x, z)) = std::env::var("KK_RAPTOR_AT").ok().and_then(|v| v.split_once(',').and_then(|(a, b)| Some((a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?)))) {
        let y = arena.ground_at(Vec3::new(x, j.y + 20.0, z)).unwrap_or(j.y);
        let p = Vec3::new(x, y, z);
        let to = (j - p).normalize_or(Vec3::Z);
        spawn_creature(&mut commands, &assets, rap, p, to.x.atan2(to.z), SpawnOpts { label: Some("raptor test".into()), ..default() });
        info!("  test raptor at ({x:.1}, {y:.1}, {z:.1})");
    }
    // slice batches with a test raptor keep the field to that one raptor
    let (pack, compies) = if std::env::var("KK_BATCH").is_ok() && std::env::var("KK_RAPTOR_AT").is_ok() { (vec![], vec![]) } else { (pack, compies) };
    info!("03E creature pack: {} raptors, {} compies (KK_NO_CREATURES=1 disables)", pack.len(), compies.len());
    for (i, mut p) in pack.into_iter().enumerate() {
        if arena.level.is_some() {
            match level_spot(&arena, p, j.y) {
                Some(q) => p = q,
                None => continue,
            }
        }
        let away = (p - j).normalize_or(Vec3::Z);
        let yaw = away.x.atan2(away.z) + rng.gen_range(-0.4..0.4);
        let opts = SpawnOpts { label: Some(format!("raptor {i}")), ..default() };
        info!("  raptor {i} at ({:.1}, {:.1}, {:.1})", p.x, p.y, p.z);
        spawn_creature(&mut commands, &assets, rap, p, yaw, opts);
    }
    for (i, mut p) in compies.into_iter().enumerate() {
        if arena.level.is_some() {
            match level_spot(&arena, p, j.y) {
                Some(q) => p = q,
                None => continue,
            }
        }
        let away = (p - j).normalize_or(Vec3::Z);
        let yaw = away.x.atan2(away.z) + rng.gen_range(-0.6..0.6);
        let opts = SpawnOpts { label: Some(format!("compy {i}")), ..default() };
        info!("  compy {i} at ({:.1}, {:.1}, {:.1})", p.x, p.y, p.z);
        spawn_creature(&mut commands, &assets, com, p, yaw, opts);
    }
}

/// Nearest spot (within 12 m, 2 m grid) whose ground is at Jack's tier (+-1.5 m), so the pack never lands on a ledge.
fn level_spot(arena: &Arena, p: Vec3, tier: f32) -> Option<Vec3> {
    let mut best: Option<(f32, Vec3)> = None;
    for ix in -6..=6 {
        for iz in -6..=6 {
            let q = Vec3::new(p.x + ix as f32 * 2.0, tier + 1.0, p.z + iz as f32 * 2.0);
            if let Some(y) = arena.ground_at(q) {
                if (y - tier).abs() <= 1.5 {
                    let d = (ix * ix + iz * iz) as f32;
                    if best.map_or(true, |b| d < b.0) {
                        best = Some((d, Vec3::new(q.x, y, q.z)));
                    }
                }
            }
        }
    }
    best.map(|b| b.1)
}
