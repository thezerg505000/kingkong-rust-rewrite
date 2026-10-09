//! V-Rex (T-Rex rig `B_Rex_*`) encounter AI for Jack levels.
//!
//! Rules from `kk_mechanics::creatures::vrex_jack` (X04, species 0x10 of `PNJ_Raptor_*`):
//! * guns cannot hurt it (hp only moves by scripted paf flags); a hit only slows it (0.5 / 5 m/s when Jack
//!   is within 20 m and it is moving) and a single hit >= 20 makes it re-think (HESITE + roar); no flinch;
//! * perception: humans seen in a 261 degree cone out to 100 m with line of sight, every gunshot heard
//!   within 150 m, a shot passing within 8 m;
//! * HESITE turns to the target (blend 4*dt) and roars after Rand(0.5, 0.8) s; FIGHT chases at the walk
//!   gait 2.2 m/s or, for Jack, the run gait 14 m/s, speed rising by only 1.5 m/s per second;
//! * bite reach 3.8 m in a 40 degree cone: a healthy (life ratio > 0.15) Jack takes a paf 1000 / 0x4104
//!   (a WOUND), a wounded Jack - or anyone closer than 2.5 m - is grabbed and killed by paf 0x4a10;
//!   no bite cooldown, no contact damage.
//! The state composition and animation clip choice are slice glue [G]; see X04 Gaps.

use crate::anim::{self, GameState, RigPlayer, Rigs};
use crate::events::GunEvent;
use crate::player::Player;
use crate::spec::*;
use crate::weapons::{RexDamage, RexHitbox};
use crate::world::Arena;
use bevy::prelude::*;
use rand::Rng;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RexState {
    /// ATTENTE: stands, waits for a target
    Idle,
    /// HESITE: turns to the target, roar clip starts after `delay`
    Hesite { t: f32, delay: f32 },
    Roar { t: f32 },
    /// FIGHT: chase
    Chase,
    /// MORD: bite clip; the hit test runs at 45 % of it
    Bite { t: f32, resolved: bool },
    /// GRAB: hold Jack; the kill paf (0x4a10) is sent when the bite clip has played, then it eats
    Grab { t: f32, killed: bool },
    Dead { t: f32 },
}

#[derive(Component)]
pub struct Rex {
    pub state: RexState,
    pub hp: f32,
    pub max_hp: f32,
    pub yaw: f32,
    pub speed: f32,
    pub gait: &'static str,
    pub noticed: bool,
    pub total_damage: f32,
    /// per-attacker damage ledger of `check_paf` (`+0x1cdc[i]`, Jack bullet total)
    pub ledger: vrex::HitLedger,
    /// test scaffold (batch `RexForce::Hold`): stand still and ignore perception, like the
    /// game's script-held actors (A18); hits are still recorded
    pub scripted_hold: bool,
}

impl Rex {
    pub fn new(max_hp: f32) -> Self {
        Self {
            state: RexState::Idle,
            hp: max_hp,
            max_hp,
            yaw: 0.0,
            speed: 0.0,
            gait: REX_IDLE,
            noticed: false,
            total_damage: 0.0,
            ledger: vrex::HitLedger::default(),
            scripted_hold: false,
        }
    }
    pub fn label(&self) -> &'static str {
        match self.state {
            RexState::Idle => "idle",
            RexState::Hesite { .. } => "hesite",
            RexState::Roar { .. } => "roar",
            RexState::Chase => "chase",
            RexState::Bite { .. } => "bite",
            RexState::Grab { .. } => "grabbed Jack",
            RexState::Dead { .. } => "dead",
        }
    }
}

#[derive(Component)]
pub struct RexScene;

/// Bones of the spawned Rex rig used for hit spheres / bite reach.
#[derive(Component)]
pub struct RexBones {
    pub bones: Vec<(Entity, f32, &'static str)>,
    pub head: Entity,
    pub pelvis: Entity,
    pub jaw: Entity,
    pub feet: [Entity; 2],
}

/// Root speeds (m/s) of locomotion clips, measured from the original root motion.
#[derive(Resource)]
pub struct RexSpeeds(pub HashMap<String, f32>);

#[derive(Resource)]
pub struct RexSettings {
    pub mortal: bool,
    pub show_hitbox: bool,
}

/// Hit-sphere radii per bone [G] (sized to the skinned mesh).
const HIT_BONES: &[(&str, f32)] = &[
    ("B_Rex_Tete", 0.85),
    ("B_Rex_Machoire", 0.6),
    ("B_Rex_Cou", 0.9),
    ("B_Rex_Torse", 1.35),
    ("B_Rex_Ventre", 1.45),
    ("B_Rex_Bassin", 1.3),
    ("B_Rex_CuisseG", 0.85),
    ("B_Rex_CuisseD", 0.85),
    ("B_Rex_JambeG", 0.5),
    ("B_Rex_JambeD", 0.5),
    ("B_Rex_PiedG", 0.4),
    ("B_Rex_PiedD", 0.4),
    ("B_Rex_Queue01", 0.95),
    ("B_Rex_Queue02", 0.75),
    ("B_Rex_Queue03", 0.55),
    ("B_Rex_Queue04", 0.4),
    ("B_Rex_Queue05", 0.28),
];

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RexSet;

pub struct RexPlugin;

impl Plugin for RexPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(RexSettings { mortal: false, show_hitbox: false })
            .insert_resource(load_speeds())
            .add_systems(Startup, load_rex_materials)
            .add_systems(OnEnter(GameState::Playing), spawn_rex)
            .add_systems(
                Update,
                (
                    // while Kong fights the rex (kong.rs) the Jack-level rex AI is off; KK_NO_KONG=1 restores it
                    rex_damage.run_if(crate::kong::jack_rex_ai),
                    rex_ai.run_if(crate::kong::jack_rex_ai),
                    rex_animate.run_if(crate::kong::jack_rex_ai),
                    publish_hitbox,
                    rex_events,
                    rex_debug_keys,
                )
                    .chain()
                    .in_set(RexSet)
                    .after(crate::weapons::WeaponsSet)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

/// The V-Rex's own Jade materials (multi-material 0x4300621b in the 03E stream):
/// sub 0/2 = 0x7e0001d3 body (diffuse 56003af2, normal 56003af4, spec map 56017f62),
/// sub 1   = 0x83000071 head (diffuse 56003d8f, normal 56003d91, spec from diffuse alpha).
/// Decoded material extension block (MATERIAL_FORMAT.md) [C]:
///  - both: diffuse colour 0x645a5a5a, doubled and tinted by the Rex world tint 0x778677
///    -> (0.329, 0.371, 0.329); same zone ambient; spec colour 0.543; shininess 6.52;
///    normal XY scale 1.569 (baked into rex_nrm_*.png).
///  - body spec = max(specmap - 0.4, 0) (rex_mr_body.png); head spec = max(alpha - 0.6, 0) = 0.
///  - the old MT ambient/diffuse words (0x96.. / 0x19..,0x7f..) are never read by the per-pixel
///    path: using them was what made the head darker than the body.
///  - Rex RLI is flat (10,10,10): out *= 1 + 10*RLI = 1.39.
#[derive(Resource)]
pub struct RexMaterials {
    mr_body: Handle<Image>,
    nrm_body: Handle<Image>,
    nrm_head: Handle<Image>,
}

/// diffuse (0.329, 0.371, 0.329) x RLI 1.39 [C]
const REX_DIFFUSE: [f32; 3] = [0.329 * 1.39, 0.371 * 1.39, 0.329 * 1.39];

impl RexMaterials {
    fn apply(&self, h: &Handle<StandardMaterial>, label: Option<&str>, mats: &mut Assets<StandardMaterial>) {
        let Some(mut m) = mats.get_mut(h) else { return };
        let head = label == Some("Material1");
        // x1.5 for the engine's per-object light colour (DAT_00f4b154, not recovered) and a
        // slight blue trim against the teal key light, matched to the master reference [G]
        // the scene lights are balanced for the RLI-baked level (m = 1+10*RLI up to 6); the Rex
        // only has its flat RLI, so its response is lifted to the master frame's mid-tone [G]
        let lin = Color::srgb(REX_DIFFUSE[0], REX_DIFFUSE[1], REX_DIFFUSE[2]).to_linear();
        let k = 8.0;
        // the head texture is darker than the body's; lift it so the head/body step matches the
        // master (head ~55 vs body ~81) [G]
        let k = if head { k * 1.3 } else { k };
        m.base_color = Color::LinearRgba(LinearRgba::new(lin.red * k * 0.98, lin.green * k * 1.02, lin.blue * k * 1.1, 1.0));
        m.metallic = 0.0;
        m.occlusion_texture = None;
        // the game's per-object ambient term (out += ambient * base, MATERIAL_FORMAT.md):
        // Bevy has no per-object ambient, so it is added as base-texture emission [L form, G level]
        m.emissive_texture = m.base_color_texture.clone();
        m.emissive = LinearRgba::new(0.05, 0.056, 0.054, 1.0);
        // spec colour 0.543 -> a little under Bevy's default dielectric F0 [L]
        m.reflectance = 0.45;
        // one wet-skin response for head and body so they read as one creature (the body spec
        // map barely clears its -0.4 bias, the head has none) [L/G]
        m.perceptual_roughness = 0.62;
        m.metallic_roughness_texture = None;
        m.normal_map_texture = Some(if head { self.nrm_head.clone() } else { self.nrm_body.clone() });
    }
}

fn load_rex_materials(mut commands: Commands, assets: Res<AssetServer>) {
    let linear = |s: &mut bevy::image::ImageLoaderSettings| s.is_srgb = false;
    commands.insert_resource(RexMaterials {
        mr_body: assets.load_with_settings("rex_mr_body.png", linear),
        nrm_body: assets.load_with_settings("rex_nrm_body.png", linear),
        nrm_head: assets.load_with_settings("rex_nrm_head.png", linear),
    });
}

fn load_speeds() -> RexSpeeds {
    let mut m: HashMap<String, f32> = [
        ("walk_b__rex_005", 2.2),
        ("trot__rex_006", 4.72),
        ("trot_b__rex_007", 7.9),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let path = crate::mods::resolve("trex_rootmotion.json");
    if let Ok(txt) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            if let Some(obj) = v.as_object() {
                for (k, e) in obj {
                    if let Some(s) = e.get("speed_mps").and_then(|s| s.as_f64()) {
                        m.insert(k.clone(), s as f32);
                    }
                }
                info!("loaded {} Rex clip root speeds from {}", obj.len(), path.display());
            }
        }
    } else {
        warn!("{} not found; using built-in Rex gait speeds", path.display());
    }
    RexSpeeds(m)
}

fn spawn_rex(mut commands: Commands, rigs: Res<Rigs>, settings: Res<RexSettings>, arena: Res<Arena>) {
    let max_hp = if settings.mortal { REX_HP_MORTAL } else { REX_HP };
    let actor = commands
        .spawn((
            Name::new("V-Rex"),
            { let mut r = Rex::new(max_hp); r.yaw = arena.rex_yaw; r },
            Transform::from_translation(arena.rex_spawn).with_rotation(Quat::from_rotation_y(arena.rex_yaw)),
            Visibility::default(),
        ))
        .id();
    let scene = commands
        .spawn((Name::new("RexScene"), RexScene, WorldAssetRoot(rigs.rex_scene.clone()), Transform::default()))
        .observe(on_rex_ready)
        .id();
    commands.entity(actor).add_child(scene);
}

fn on_rex_ready(
    trigger: On<bevy::world_serialization::WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    names: Query<&Name>,
    meshes: Query<(), With<Mesh3d>>,
    players: Query<Entity, With<AnimationPlayer>>,
    rigs: Res<Rigs>,
    mat_q: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    rex_materials: Res<RexMaterials>,
    server: Res<AssetServer>,
) {
    let root = trigger.entity;
    let mut bones = Vec::new();
    let (mut head, mut pelvis, mut jaw, mut foot_l, mut foot_r) = (None, None, None, None, None);
    for e in children.iter_descendants(root) {
        if meshes.contains(e) {
            // skinned AABB is computed from the bind pose, which sits ~3 m off the animated body
            commands.entity(e).insert(bevy::camera::visibility::NoFrustumCulling);
            if let Ok(h) = mat_q.get(e) {
                let label = server.get_path(h.0.id()).and_then(|p| p.label().map(String::from));
                rex_materials.apply(&h.0, label.as_deref(), &mut mats);
            }
        }
        let Ok(n) = names.get(e) else { continue };
        if let Some((name, r)) = HIT_BONES.iter().find(|(b, _)| *b == n.as_str()) {
            bones.push((e, *r, *name));
        }
        match n.as_str() {
            "B_Rex_Tete" => head = Some(e),
            "B_Rex_Bassin" => pelvis = Some(e),
            "B_Rex_Machoire" => jaw = Some(e),
            "B_Rex_OrteilG" => foot_l = Some(e),
            "B_Rex_OrteilD" => foot_r = Some(e),
            _ => {}
        }
    }
    let player = anim::attach_graph(&mut commands, root, &children, &players, &rigs.rex.graph);
    match (head, pelvis, player, jaw.zip(foot_l).zip(foot_r)) {
        (Some(head), Some(pelvis), Some(player), Some(((jaw, fl), fr))) => {
            commands.entity(root).insert((
                RexBones { bones, head, pelvis, jaw, feet: [fl, fr] },
                RigPlayer { player, current: String::new() },
            ));
        }
        _ => error!("rex glb missing B_Rex_Tete / B_Rex_Bassin / AnimationPlayer"),
    }
}

fn start_hesite(r: &mut Rex) {
    let (lo, hi) = vrex::ROAR_DELAY_RANGE_S;
    let delay = rand::thread_rng().gen_range(lo..=hi);
    r.state = RexState::Hesite { t: 0.0, delay };
}

/// `check_paf@0x849910` for species 0x10 (G17/X04): bullets never change hp; they slow the rex and a single
/// hit >= 20 makes it re-think. Being shot always alerts it.
fn rex_damage(
    mut events: MessageReader<RexDamage>,
    mut rex: Query<(&mut Rex, &Transform)>,
    players: Query<&Player>,
    settings: Res<RexSettings>,
) {
    let Ok((mut r, _)) = rex.single_mut() else { return };
    let alive_player = players.single().map(|p| p.alive()).unwrap_or(false);
    for ev in events.read() {
        if matches!(r.state, RexState::Dead { .. }) {
            continue;
        }
        let jack_near = ev.dist_sq.sqrt() < vrex::HIT_REACT_NEAR_M;
        let moving = r.speed > 0.01;
        let mut ledger = r.ledger;
        // bullet paf flag word 0x44 (bit 0x40 + bit 0x4) [C check_paf]
        let eff = vrex::on_hit(r.hp, 0x44, ev.amount, true, jack_near, moving, &mut ledger);
        r.ledger = ledger;
        r.total_damage += ev.amount;
        r.hp = eff.hp;
        if settings.mortal {
            r.hp -= ev.amount; // debug cheat F2: the real rex is unkillable by guns
        }
        r.speed = (r.speed - eff.slow).max(0.0);
        if !r.noticed && alive_player {
            r.noticed = true;
        }
        if eff.rethink && !r.scripted_hold && matches!(r.state, RexState::Chase | RexState::Idle) {
            start_hesite(&mut r);
        }
    }
    if r.hp <= 0.0 && !matches!(r.state, RexState::Dead { .. }) {
        r.hp = 0.0;
        r.state = RexState::Dead { t: 0.0 };
    }
}

fn angle_wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Pick the gait clip whose root speed is nearest the current speed (walk 2.2 / trot 4.7 / trot_b 7.9 /
/// run_c 14.0 m/s). Only walk and run are AI gaits; the trots show while accelerating at 1.5 m/s^2 [G].
fn gait_for(speed: f32, rigs: &Rigs, speeds: &RexSpeeds) -> &'static str {
    let cands = [(REX_WALK, vrex::GAIT_WALK), (REX_TROT, 4.72), (REX_TROT_FAST, 7.9), (REX_RUN, vrex::GAIT_RUN)];
    let mut best = (REX_WALK, f32::MAX);
    for (label, fallback) in cands {
        let v = rigs.rex.find(label).and_then(|n| speeds.0.get(n)).copied().unwrap_or(fallback);
        let d = (v - speed).abs();
        if d < best.1 {
            best = (label, d);
        }
    }
    best.0
}

#[allow(clippy::too_many_arguments)]
fn rex_ai(
    time: Res<Time>,
    rigs: Res<Rigs>,
    speeds: Res<RexSpeeds>,
    arena: Res<Arena>,
    mut rex: Query<(&mut Rex, &mut Transform), Without<Player>>,
    bones: Query<&RexBones>,
    gts: Query<&GlobalTransform>,
    mut players: Query<(&mut Player, &mut Transform), Without<Rex>>,
    mut shots: MessageReader<GunEvent>,
) {
    let Ok((mut r, mut tf)) = rex.single_mut() else { return };
    let Ok((mut p, mut ptf)) = players.single_mut() else { return };
    let dt = time.delta_secs();
    if r.scripted_hold {
        r.state = RexState::Idle;
        r.speed = 0.0;
        r.gait = REX_IDLE;
        shots.clear();
        return;
    }
    let to_p = ptf.translation - tf.translation;
    let dist = Vec2::new(to_p.x, to_p.z).length();
    // Rex forward is +Z of its actor frame (Jade -Y).
    let want_yaw = to_p.x.atan2(to_p.z);
    let fwd = Vec3::new(r.yaw.sin(), 0.0, r.yaw.cos());
    let head = bones
        .single()
        .ok()
        .and_then(|b| gts.get(b.head).ok())
        .map(|g| g.translation())
        .unwrap_or(tf.translation + fwd * 6.0 + Vec3::Y * 4.0);
    let eye = ptf.translation + Vec3::Y * p.eye;
    let head_dist = head.distance(eye).min(Vec2::new(head.x - eye.x, head.z - eye.z).length() + 0.6);
    let cos_to_jack = {
        let d = Vec3::new(to_p.x, 0.0, to_p.z).normalize_or_zero();
        d.dot(fwd)
    };

    // ---- perception (exec_check_vision / check_sound / check_shoot) ----
    let mut alerted = false;
    for e in shots.read() {
        if let GunEvent::Fired { muzzle, dir, .. } = e {
            // every gunshot within 150 m is heard
            let heard = vrex::noise_flags(muzzle.distance(tf.translation), vrex::GUNSHOT_NOISE_RADIUS, true) != 0;
            // a shot passing within 8 m (closest approach of the shot ray to the rex)
            let t = (tf.translation - *muzzle).dot(*dir).max(0.0);
            let miss = (tf.translation - (*muzzle + *dir * t)).length();
            if heard || vrex::shot_flags_shooter(miss) {
                alerted = true;
            }
        }
    }
    if p.alive() && !r.noticed {
        let d3 = head.distance(eye);
        let los = d3 < 0.5
            || arena
                .raycast(head, (eye - head) / d3, d3)
                .map_or(true, |(t, _)| t >= d3 - 0.4);
        // class 1 = Jack
        if vrex::sees(1, d3, cos_to_jack, los, false, false) {
            alerted = true;
        }
    }
    if alerted && p.alive() {
        r.noticed = true;
    }

    let turn_rate = |speed: f32| if speed > 10.0 { REX_TURN_RATE * 0.6 } else { REX_TURN_RATE };
    let jack_alive = p.alive();
    let hard_grab = jack_alive
        && !matches!(r.state, RexState::Grab { .. } | RexState::Bite { .. } | RexState::Dead { .. })
        && vrex::hard_grab(dist, false, true);

    let st = r.state;
    r.state = match st {
        RexState::Dead { t } => RexState::Dead { t: t + dt },
        _ if hard_grab => {
            // exec_hard_grab: Jack within 2.5 m is held regardless of the bite rules
            r.speed = 0.0;
            RexState::Grab { t: 0.0, killed: false }
        }
        _ if !jack_alive && !matches!(st, RexState::Grab { .. }) => {
            r.speed = vrex::speed_step(r.speed, 0.0, dt, vrex::ACCEL);
            RexState::Idle
        }
        RexState::Grab { t, killed } => {
            r.speed = 0.0;
            let d = rigs.rex.duration(REX_BITE);
            if !killed && t + dt >= d {
                // ETAT_GRAB end of the hold: second paf, damage 1000, flags 0x4a10 (contains the 0x200 kill bit)
                p.paf("grabbed by the V-Rex", vrex::GRAB_KILL_PAF_FLAGS);
                RexState::Grab { t: 0.0, killed: true }
            } else {
                RexState::Grab { t: t + dt, killed }
            }
        }
        RexState::Idle => {
            r.speed = vrex::speed_step(r.speed, 0.0, dt, vrex::ACCEL);
            if r.noticed {
                let (lo, hi) = vrex::ROAR_DELAY_RANGE_S;
                RexState::Hesite { t: 0.0, delay: rand::thread_rng().gen_range(lo..=hi) }
            } else {
                RexState::Idle
            }
        }
        RexState::Hesite { t, delay } => {
            // ETAT_HESITE: face the target with blend 4*dt, roar after the random delay
            r.speed = vrex::speed_step(r.speed, 0.0, dt, vrex::ACCEL);
            r.yaw += angle_wrap(want_yaw - r.yaw) * (vrex::DECEL_BLEND_RATE * dt).min(1.0);
            if t + dt >= delay { RexState::Roar { t: 0.0 } } else { RexState::Hesite { t: t + dt, delay } }
        }
        RexState::Roar { t } => {
            r.speed = vrex::speed_step(r.speed, 0.0, dt, vrex::ACCEL);
            r.yaw += angle_wrap(want_yaw - r.yaw) * (vrex::DECEL_BLEND_RATE * dt).min(1.0);
            let d = rigs.rex.duration(REX_ROAR);
            if t + dt >= d { RexState::Chase } else { RexState::Roar { t: t + dt } }
        }
        RexState::Chase => {
            // ETAT_FIGHT/select_action: run flag set because the target is Jack -> target speed 14 m/s,
            // speed rises by at most 1.5 m/s per second [C]
            let target = vrex::chase_target_speed(true, false);
            r.speed = vrex::speed_step(r.speed, target, dt, vrex::ACCEL);
            r.gait = gait_for(r.speed, &rigs, &speeds);
            let d = want_yaw - r.yaw;
            let rate = turn_rate(r.speed);
            r.yaw += angle_wrap(d).clamp(-rate * dt, rate * dt);
            if vrex::bite_hits(vrex::RexState::Mord, head_dist, cos_to_jack) {
                r.speed = 0.0; // [G] the bite clip is played in place
                RexState::Bite { t: 0.0, resolved: false }
            } else {
                RexState::Chase
            }
        }
        RexState::Bite { t, mut resolved } => {
            r.speed = 0.0;
            let d = rigs.rex.duration(REX_BITE);
            let t = t + dt;
            let mut next = None;
            if !resolved && t >= d * 0.45 {
                resolved = true;
                // exec_bite: hit test (reach 3.8, 40 degrees), then the outcome from Jack's published life ratio
                if vrex::bite_hits(vrex::RexState::Mord, head_dist, cos_to_jack) {
                    match vrex::bite_outcome(1, 0, false, p.wounds.state.life_ratio(), false) {
                        vrex::BiteOutcome::Strike { flags, .. } => {
                            p.paf("bitten by the V-Rex", flags);
                        }
                        vrex::BiteOutcome::Grab => next = Some(RexState::Grab { t, killed: false }),
                    }
                }
            }
            match next {
                Some(g) => g,
                // no cooldown: the next bite waits only for the bite clip (check_requin is off for the rex)
                None if t >= d => RexState::Chase,
                None => RexState::Bite { t, resolved },
            }
        }
    };

    if !matches!(r.state, RexState::Dead { .. }) {
        let fwd = Vec3::new(r.yaw.sin(), 0.0, r.yaw.cos());
        let mut pos = tf.translation + fwd * r.speed * dt;
        pos = arena.collide(pos, 2.0);
        // stay on the mapped ground: no walking off ledges or the edge of the map; follow the ground
        let (mut pos, ground) = arena.creature_step(tf.translation, pos, 1.6, 2.5);
        if let Some(y) = ground {
            pos.y = tf.translation.y + (y - tf.translation.y) * (8.0 * dt).min(1.0);
        }
        tf.translation = pos;
    }
    tf.rotation = Quat::from_rotation_y(r.yaw);

    // Body vs Jack: `exec_check_collision` is an empty stub (X04), so contact does NOT hurt; the bodies
    // only push Jack out [slice glue, G].
    if p.alive() {
        let fwd = Vec3::new(r.yaw.sin(), 0.0, r.yaw.cos());
        let body: Vec<Vec3> = bones
            .single()
            .ok()
            .map(|b| {
                [b.pelvis]
                    .iter()
                    .filter_map(|e| gts.get(*e).ok())
                    .map(|g| g.translation())
                    .collect()
            })
            .unwrap_or_default();
        let centers = body.into_iter().chain([tf.translation + fwd * 2.0]);
        for c in centers {
            let d = Vec2::new(ptf.translation.x - c.x, ptf.translation.z - c.z);
            let min = 2.4;
            if d.length() < min {
                let n = d.normalize_or(Vec2::X) * min;
                ptf.translation.x = c.x + n.x;
                ptf.translation.z = c.z + n.y;
            }
        }
    }
}

fn rex_animate(
    rigs: Res<Rigs>,
    rex: Query<&Rex>,
    mut scenes: Query<&mut RigPlayer, With<RexScene>>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Ok(r) = rex.single() else { return };
    let Ok(mut rp) = scenes.single_mut() else { return };
    let Ok((mut player, mut tr)) = anim.get_mut(rp.player) else { return };
    let rig = &rigs.rex;
    let (clip, repeat) = match r.state {
        RexState::Idle | RexState::Hesite { .. } => (REX_IDLE, true),
        RexState::Roar { .. } => (REX_ROAR, false),
        RexState::Grab { killed: false, .. } => (REX_BITE, false),
        RexState::Grab { killed: true, .. } => (REX_ROAR, false),
        RexState::Chase => (r.gait, true),
        RexState::Bite { .. } => (REX_BITE, false),
        RexState::Dead { .. } => (REX_DEATH, false),
    };
    let restart = matches!(r.state, RexState::Roar { t } | RexState::Bite { t, .. } | RexState::Grab { t, .. } if t == 0.0);
    anim::play(rig, &mut rp, &mut player, &mut tr, clip, 0.25, repeat, restart);
}

fn publish_hitbox(
    rex: Query<(&Rex, &Transform)>,
    bones: Query<&RexBones>,
    gts: Query<&GlobalTransform>,
    mut hb: ResMut<RexHitbox>,
    settings: Res<RexSettings>,
    mut gizmos: Gizmos,
) {
    hb.spheres.clear();
    let Ok((r, rtf)) = rex.single() else { return };
    hb.alive = !matches!(r.state, RexState::Dead { .. });
    hb.root = rtf.translation;
    let Ok(b) = bones.single() else { return };
    for (e, rad, name) in &b.bones {
        if let Ok(g) = gts.get(*e) {
            hb.spheres.push((g.translation(), *rad, name));
        }
    }
    // fill gaps along the spine/tail with midpoints
    let pos: HashMap<&str, (Vec3, f32)> = hb.spheres.iter().map(|(p, r, n)| (*n, (*p, *r))).collect();
    let chain = [
        ("B_Rex_Bassin", "B_Rex_Ventre"),
        ("B_Rex_Ventre", "B_Rex_Torse"),
        ("B_Rex_Torse", "B_Rex_Cou"),
        ("B_Rex_Cou", "B_Rex_Tete"),
        ("B_Rex_Bassin", "B_Rex_Queue01"),
        ("B_Rex_Queue01", "B_Rex_Queue02"),
        ("B_Rex_Queue02", "B_Rex_Queue03"),
        ("B_Rex_Queue03", "B_Rex_Queue04"),
        ("B_Rex_CuisseG", "B_Rex_JambeG"),
        ("B_Rex_CuisseD", "B_Rex_JambeD"),
    ];
    for (a, b2) in chain {
        if let (Some((pa, ra)), Some((pb, rb))) = (pos.get(a), pos.get(b2)) {
            hb.spheres.push(((*pa + *pb) * 0.5, (ra + rb) * 0.5, a));
        }
    }
    if settings.show_hitbox {
        for (p, r, _) in &hb.spheres {
            gizmos.sphere(Isometry3d::from_translation(*p), *r, Color::srgba(1.0, 0.2, 0.2, 0.6));
        }
    }
}

fn rex_debug_keys(keys: Res<ButtonInput<KeyCode>>, mut settings: ResMut<RexSettings>, mut rex: Query<&mut Rex>) {
    if keys.just_pressed(KeyCode::F2) {
        settings.mortal = !settings.mortal;
        if let Ok(mut r) = rex.single_mut() {
            let frac = r.hp / r.max_hp;
            r.max_hp = if settings.mortal { REX_HP_MORTAL } else { REX_HP };
            r.hp = (frac * r.max_hp).max(1.0);
        }
    }
    if keys.just_pressed(KeyCode::F3) {
        settings.show_hitbox = !settings.show_hitbox;
    }
}


#[derive(Default)]
struct RexEventState {
    last: Option<std::mem::Discriminant<RexState>>,
    roared_once: bool,
    breath_t: f32,
    foot_down: [bool; 2],
    foot_min: [f32; 2],
    eat_t: f32,
}

/// Turns Rex state transitions and foot contacts into events for sound / effects.
/// Footsteps come from the animated toe bones touching down (relative height to the actor),
/// so they stay in sync with whichever gait clip is playing.
fn rex_events(
    time: Res<Time>,
    rex: Query<(&Rex, &GlobalTransform)>,
    bones: Query<&RexBones>,
    gts: Query<&GlobalTransform>,
    mut out: MessageWriter<crate::events::RexEvent>,
    mut st: Local<RexEventState>,
) {
    use crate::events::RexEvent as E;
    let Ok((r, gt)) = rex.single() else { return };
    let pos = gt.translation();
    let d = std::mem::discriminant(&r.state);
    let entered = st.last != Some(d);
    st.last = Some(d);
    let head = bones.single().ok().and_then(|b| gts.get(b.head).ok()).map(|g| g.translation()).unwrap_or(pos);
    if entered {
        match r.state {
            RexState::Roar { .. } => {
                out.write(E::Roar { alert: !st.roared_once, pos: head });
                st.roared_once = true;
            }
            RexState::Bite { .. } => { out.write(E::BiteStart { pos: head }); }
            RexState::Grab { .. } => {
                out.write(E::BiteHit { pos: head });
                st.eat_t = 0.8;
            }
            RexState::Dead { .. } => { out.write(E::Died { pos: head }); }
            _ => {}
        }
    }
    let dt = time.delta_secs();
    if matches!(r.state, RexState::Grab { killed: true, .. }) {
        st.eat_t -= dt;
        if st.eat_t <= 0.0 {
            st.eat_t = 99.0;
            out.write(E::Eat { pos: head });
        }
    }
    if matches!(r.state, RexState::Idle | RexState::Chase | RexState::Hesite { .. }) {
        st.breath_t -= dt;
        if st.breath_t <= 0.0 {
            st.breath_t = 4.5;
            out.write(E::Breath { pos: head });
        }
    }
    // toe contact: measured from the gait clips, planted toes sit at 0.40-0.45 m and lift to
    // 0.85-1.28 m above the actor root -> contact below 0.5 m after rising above 0.7 m
    let Ok(b) = bones.single() else { return };
    if matches!(r.state, RexState::Dead { .. }) { return; }
    for (i, f) in b.feet.iter().enumerate() {
        let Ok(g) = gts.get(*f) else { continue };
        let h = g.translation().y - pos.y;
        if st.foot_down[i] {
            if h > 0.7 { st.foot_down[i] = false; }
        } else if h < 0.5 {
            st.foot_down[i] = true;
            out.write(E::Footstep { pos: g.translation(), strong: r.speed > 5.0 });
        }
        st.foot_min[i] = h;
    }
}
