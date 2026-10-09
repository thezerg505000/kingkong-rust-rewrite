//! Test-area batches (`KK_BATCH=t1..t5`, scene `testarea.rs`): creature and Kong checks on a plain flat
//! ground, same machinery as `batch.rs` (30 Hz fixed clock, clean frames, JSON report, exit) but with
//! a small generic script language: spawn a creature kind, play an action id, place Jack, aim and fire.
//!
//! * t1_creature_lineup  every creature + Kong in a row with labels: glbs loaded, clip counts, scale, grounding
//! * t2_raptor_hunt      a raptor perceives Jack, chases, grabs/bites; Jack shoots it dead (X02, A03, A05, A07, A10)
//! * t3_compy_swarm      compies (hp 3, bite 1) chase; one Colt hit kills (X01)
//! * t4_bronto_walk      a brontosaurus walks its loop; feet within 3 m send paf 0x10 (X14)
//! * t5_creature_anims   idle / walk / run / attack / hit / death on each creature, one screenshot per action

use crate::anim::GameState;
use crate::creatures::*;
use crate::player::Player;
use crate::spec::*;
use crate::testarea::ShowLabels;
use crate::weapons::Arsenal;
use bevy::prelude::*;
use bevy::camera::primitives::Aabb;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use kk_mechanics::creatures::raptor::{self as rp, RaptorState, Reaction};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub enum TAct {
    /// spawn a creature kind (manifest name or "kong") at (x, z) facing yaw (degrees, 0 = +Z, 90 = +X)
    Put { kind: &'static str, tag: &'static str, x: f32, z: f32, yaw: f32, ai: bool, scale: f32 },
    /// play an action id: label (idle..death), "#31" bank index, "0x46" AI animation id or a clip name; tag "*" = all
    Play { tag: &'static str, action: &'static str, repeat: bool },
    Face { tag: &'static str, yaw: f32 },
    /// put Jack (camera) at (x, z) with yaw / pitch in degrees (yaw 0 looks down -Z, 90 looks down -X)
    Jack { x: f32, z: f32, yaw: f32, pitch: f32 },
    /// keep the aim on a tagged creature's pelvis + h (m); "" stops
    Aim { tag: &'static str, h: f32 },
    /// put Jack `dist` m from a tagged creature, on the bearing `side` degrees (0 = its +Z side), looking at it at height h
    Frame { tag: &'static str, dist: f32, side: f32, h: f32 },
    /// aim at the nearest living AI creature
    AimNearest { h: f32 },
    Weapon(usize),
    Auto(bool),
    /// start auto-fire when the first creature grab on Jack is logged
    AutoOnGrab,
    /// restore Jack to healthy 0.25 s after each wound (the wound onsets are logged first)
    AutoHeal(bool),
    Labels(bool),
    Measure(&'static str),
    Sample(&'static str),
    Shot(&'static str),
    Mark(&'static str),
    End,
}

pub struct TBatch {
    pub name: &'static str,
    pub about: &'static str,
    pub script: &'static [(f32, TAct)],
}

use TAct::*;

const fn sp(kind: &'static str, tag: &'static str, x: f32, z: f32, yaw: f32) -> TAct {
    Put { kind, tag, x, z, yaw, ai: false, scale: 0.0 }
}

pub const TBATCHES: &[TBatch] = &[
    TBatch {
        name: "t1_creature_lineup",
        about: "every creature and Kong in a row with labels; glbs loaded, clip counts as in the manifest, scale, feet on the ground",
        script: &[
            (0.0, Jack { x: 0.0, z: 6.0, yaw: 0.0, pitch: 0.0 }),
            (0.0, sp("raptor", "raptor", -11.0, -14.0, 0.0)),
            (0.0, Put { kind: "compy", tag: "compy_s", x: -7.0, z: -14.0, yaw: 0.0, ai: false, scale: 0.2 }),
            (0.0, Put { kind: "compy", tag: "compy_l", x: -5.2, z: -14.0, yaw: 0.0, ai: false, scale: 0.35 }),
            (0.0, sp("raptor_kong", "rkong", -2.5, -14.0, 0.0)),
            (0.0, sp("crab", "crab", 1.5, -14.0, 0.0)),
            (0.0, sp("kong", "kong", 7.5, -14.0, 0.0)),
            (0.0, sp("brontosaurus", "bronto", 55.0, -60.0, -90.0)),
            (1.8, Measure("a")),
            (2.2, Shot("front")),
            (2.6, Measure("b")),
            (2.7, Jack { x: 0.0, z: 6.0, yaw: 0.0, pitch: 0.0 }),
            (2.8, Face { tag: "raptor", yaw: 90.0 }),
            (2.8, Face { tag: "compy_s", yaw: 90.0 }),
            (2.8, Face { tag: "compy_l", yaw: 90.0 }),
            (2.8, Face { tag: "rkong", yaw: 90.0 }),
            (2.8, Face { tag: "crab", yaw: 90.0 }),
            (2.8, Face { tag: "kong", yaw: 90.0 }),
            (3.4, Shot("side")),
            (3.5, Jack { x: 12.0, z: 52.0, yaw: -10.0, pitch: 0.0 }),
            (4.5, Shot("bronto")),
            (4.6, Jack { x: 0.0, z: -3.0, yaw: 0.0, pitch: 4.0 }),
            (4.8, Labels(false)),
            (5.3, Shot("close")),
            (5.8, End),
        ],
    },
    TBatch {
        name: "t2_raptor_hunt",
        about: "a raptor 38 m away perceives Jack, hesitates, chases at the run gait, first bite = grab (wound), Jack shoots it dead with the Colt; death timers",
        script: &[
            (0.0, Labels(false)),
            (0.0, Jack { x: 0.0, z: 0.0, yaw: 0.0, pitch: 0.0 }),
            (0.0, Weapon(0)),
            (0.0, Put { kind: "raptor", tag: "r", x: 6.0, z: -38.0, yaw: 180.0, ai: true, scale: 0.0 }),
            (0.2, Aim { tag: "r", h: 0.9 }),
            (0.2, AutoHeal(true)),
            (0.2, AutoOnGrab),
            (4.0, Shot("chase")),
            (7.0, Shot("approach")),
            (8.1, Shot("bite")),
            (11.0, Shot("fight")),
            (24.0, Frame { tag: "r", dist: 7.0, side: 60.0, h: 0.6 }),
            (26.0, Shot("dead")),
            (44.0, End),
        ],
    },
    TBatch {
        name: "t3_compy_swarm",
        about: "five compies (scale 0.2-0.35, hp 3) chase Jack and bite for 1; the Colt kills each with one hit",
        script: &[
            (0.0, Labels(false)),
            (0.0, Jack { x: 0.0, z: 0.0, yaw: 0.0, pitch: 0.0 }),
            (0.0, Weapon(0)),
            (0.0, Put { kind: "compy", tag: "c0", x: -4.0, z: -12.0, yaw: 180.0, ai: true, scale: 0.0 }),
            (0.0, Put { kind: "compy", tag: "c1", x: 4.0, z: -13.0, yaw: 180.0, ai: true, scale: 0.0 }),
            (0.0, Put { kind: "compy", tag: "c2", x: 0.0, z: -14.0, yaw: 180.0, ai: true, scale: 0.0 }),
            (0.0, Put { kind: "compy", tag: "c3", x: -8.0, z: -11.0, yaw: 180.0, ai: true, scale: 0.0 }),
            (0.0, Put { kind: "compy", tag: "c4", x: 8.0, z: -12.0, yaw: 180.0, ai: true, scale: 0.0 }),
            (0.2, AutoHeal(true)),
            (1.3, Shot("chase")),
            (4.3, Shot("bite")),
            (5.0, AimNearest { h: 0.1 }),
            (5.0, Auto(true)),
            (6.4, Shot("shoot")),
            (10.0, Shot("dead")),
            (11.5, End),
        ],
    },
    TBatch {
        name: "t4_bronto_walk",
        about: "a brontosaurus walks its waypoint loop past Jack; each foot within 3 m sends paf 0x10",
        script: &[
            (0.0, Labels(false)),
            (0.0, Jack { x: 0.0, z: -1.5, yaw: 90.0, pitch: 8.0 }),
            (0.0, Put { kind: "brontosaurus", tag: "b", x: -50.0, z: 0.0, yaw: 90.0, ai: true, scale: 0.0 }),
            (2.0, Shot("approach")),
            (9.0, Shot("pass")),
            (15.0, Shot("trample")),
            (19.0, Jack { x: 0.0, z: 60.0, yaw: 0.0, pitch: 0.0 }),
            (20.0, Shot("side")),
            (21.0, End),
        ],
    },
    TBatch {
        name: "t5_creature_anims",
        about: "idle / walk / run / attack / hit / death on every creature, one screenshot per action (grid via montage)",
        script: &[
            (0.0, Jack { x: 0.0, z: 6.0, yaw: 0.0, pitch: 0.0 }),
            (0.0, sp("raptor", "raptor", -9.0, -12.0, 0.0)),
            (0.0, Put { kind: "compy", tag: "compy", x: -4.5, z: -12.0, yaw: 0.0, ai: false, scale: 0.3 }),
            (0.0, sp("raptor_kong", "rkong", -1.0, -12.0, 0.0)),
            (0.0, sp("crab", "crab", 3.0, -12.0, 0.0)),
            (0.0, sp("kong", "kong", 9.0, -12.0, 0.0)),
            (0.0, sp("brontosaurus", "bronto", 50.0, -50.0, -90.0)),
            (1.0, Play { tag: "*", action: "idle", repeat: true }),
            (1.6, Sample("idle")),
            (1.7, Shot("idle")),
            (2.2, Play { tag: "*", action: "walk", repeat: true }),
            (2.8, Sample("walk")),
            (2.9, Shot("walk")),
            (3.4, Play { tag: "*", action: "run", repeat: true }),
            (4.0, Sample("run")),
            (4.1, Shot("run")),
            (4.6, Play { tag: "*", action: "attack", repeat: false }),
            (5.0, Sample("attack")),
            (5.1, Shot("attack")),
            (6.2, Play { tag: "*", action: "hit", repeat: false }),
            (6.5, Sample("hit")),
            (6.6, Shot("hit")),
            (7.4, Play { tag: "*", action: "death", repeat: false }),
            (8.4, Sample("death")),
            (8.5, Shot("death")),
            (9.4, End),
        ],
    },
];

// ---------------------------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Default, Debug)]
struct Sampled {
    name: String,
    tag: String,
    kind: usize,
    clip: String,
    seek: f32,
    pose: Vec<Vec3>,
    extent: Option<(Vec3, Vec3)>,
    foot_low: Option<f32>,
    clips: usize,
    rig_ok: bool,
    scale: f32,
}

#[derive(Resource)]
struct TRunner {
    batch: &'static TBatch,
    out: PathBuf,
    t: f32,
    step: usize,
    started: bool,
    off: f32,
    tags: Vec<(&'static str, Entity)>,
    aim: Option<(&'static str, f32)>,
    aim_nearest: Option<f32>,
    auto: bool,
    auto_on_grab: bool,
    auto_heal: bool,
    reload_cd: f32,
    heal_cd: f32,
    want_sample: Option<(&'static str, bool)>,
    samples: Vec<Sampled>,
    shots: Vec<String>,
    marks: Vec<(&'static str, f32)>,
    wound_onsets: Vec<f32>,
    jack_dead_at: Option<f32>,
    last_wound: bool,
    /// (t, tag, hp, speed, state, x, z)
    trace: Vec<(f32, &'static str, f32, f32, String, f32, f32)>,
    done: bool,
    frames: u32,
}

pub struct TBatchPlugin;

impl Plugin for TBatchPlugin {
    fn build(&self, app: &mut App) {
        let Ok(name) = std::env::var("KK_BATCH") else { return };
        if !crate::testarea::is_test_batch(&name) {
            return;
        }
        let Some(batch) = TBATCHES.iter().find(|b| b.name == name || b.name.starts_with(&name)) else {
            eprintln!("unknown test batch {name}; available: {:?}", TBATCHES.iter().map(|b| b.name).collect::<Vec<_>>());
            std::process::exit(2);
        };
        let out = PathBuf::from(std::env::var("KK_BATCH_OUT").unwrap_or_else(|_| "batch_out".into()));
        std::fs::create_dir_all(&out).ok();
        app.insert_resource(crate::batch::CleanHud(true))
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / 30.0)))
            .insert_resource(TRunner {
                batch,
                out,
                t: 0.0,
                step: 0,
                started: false,
                off: 0.0,
                tags: vec![],
                aim: None,
                aim_nearest: None,
                auto: false,
                auto_on_grab: false,
                auto_heal: false,
                reload_cd: 0.0,
                heal_cd: 0.0,
                want_sample: None,
                samples: vec![],
                shots: vec![],
                marks: vec![],
                wound_onsets: vec![],
                jack_dead_at: None,
                last_wound: false,
                trace: vec![],
                done: false,
                frames: 0,
            })
            .add_systems(PreUpdate, drive.after(bevy::input::InputSystems).run_if(in_state(GameState::Playing)))
            .add_systems(Update, (sampler, record).chain().after(CreatureSet).run_if(in_state(GameState::Playing)));
    }
}

fn yaw_rad(deg: f32) -> f32 {
    deg.to_radians()
}

#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    time: Res<Time>,
    mut r: ResMut<TRunner>,
    assets: Res<CreatureAssets>,
    mut show: ResMut<ShowLabels>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut arsenal: ResMut<Arsenal>,
    mut players: Query<(&mut Player, &mut Transform), Without<Creature>>,
    mut creatures: Query<(Entity, &mut Creature, &mut Transform, Option<&RaptorAi>, Option<&CreatureRig>), Without<Player>>,
    gts: Query<&GlobalTransform>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    log: Res<CreatureLog>,
    mut exit: MessageWriter<AppExit>,
) {
    // release one-frame presses
    if keys.pressed(KeyCode::KeyR) && !keys.just_pressed(KeyCode::KeyR) {
        keys.release(KeyCode::KeyR);
    }
    if mouse.pressed(MouseButton::Left) && !mouse.just_pressed(MouseButton::Left) {
        mouse.release(MouseButton::Left);
    }
    // hold the clock until every creature glb is in (software GL loads slowly)
    r.frames += 1;
    if !assets.all_ready() || r.frames < 12 {
        return;
    }
    let dt = time.delta_secs();
    if !r.started {
        r.started = true;
        r.t = 0.0;
        r.off = time.elapsed_secs();
    } else {
        r.t += dt;
    }
    let Ok((mut p, mut jtf)) = players.single_mut() else { return };
    let script = r.batch.script;
    while r.step < script.len() && script[r.step].0 <= r.t {
        let act = script[r.step].1;
        r.step += 1;
        match act {
            Put { kind, tag, x, z, yaw, ai, scale } => {
                let Some(k) = kind_index(kind) else { continue };
                let opts = SpawnOpts {
                    ai,
                    scale: (scale > 0.0).then_some(scale),
                    label: Some(tag.to_string()),
                    ..default()
                };
                if let Some(e) = spawn_creature(&mut commands, &assets, k, Vec3::new(x, 0.0, z), yaw_rad(yaw), opts) {
                    r.tags.push((tag, e));
                }
            }
            Play { tag, action, repeat } => {
                for (t, e) in r.tags.clone() {
                    if tag != "*" && tag != t {
                        continue;
                    }
                    let Ok((_, mut c, _, _, rig)) = creatures.get_mut(e) else { continue };
                    let Some(rig) = rig else { continue };
                    let Ok((mut pl, mut tr)) = anim.get_mut(rig.player) else { continue };
                    if let Some(clip) = assets.resolve(c.kind, action) {
                        play_clip(&assets, &mut c, &mut pl, &mut tr, &clip, 0.1, repeat, 1.0, true);
                    }
                }
            }
            Face { tag, yaw } => {
                for (t, e) in r.tags.clone() {
                    if tag != "*" && tag != t {
                        continue;
                    }
                    if let Ok((_, mut c, mut tf, _, _)) = creatures.get_mut(e) {
                        c.yaw = yaw_rad(yaw);
                        tf.rotation = Quat::from_rotation_y(c.yaw);
                    }
                }
            }
            Jack { x, z, yaw, pitch } => {
                jtf.translation = Vec3::new(x, 0.0, z);
                p.yaw = yaw_rad(yaw);
                p.pitch = yaw_rad(pitch);
            }
            Aim { tag, h } => {
                r.aim = (!tag.is_empty()).then_some((tag, h));
                r.aim_nearest = None;
            }
            Frame { tag, dist, side, h } => {
                if let Some((_, e)) = r.tags.iter().find(|t| t.0 == tag) {
                    if let Ok((_, _, tf, _, _)) = creatures.get(*e) {
                        let a = yaw_rad(side);
                        let at = tf.translation + Vec3::new(a.sin(), 0.0, a.cos()) * dist;
                        jtf.translation = Vec3::new(at.x, 0.0, at.z);
                        let to = tf.translation + Vec3::Y * h - (jtf.translation + Vec3::Y * p.eye);
                        p.yaw = (-to.x).atan2(-to.z);
                        p.pitch = (to.y / Vec2::new(to.x, to.z).length().max(0.01)).atan();
                        r.aim = None;
                        r.aim_nearest = None;
                    }
                }
            }
            AimNearest { h } => {
                r.aim_nearest = Some(h);
                r.aim = None;
            }
            Weapon(i) => {
                arsenal.index = i;
                arsenal.wanted_weapon = Some(i);
                arsenal.action = crate::weapons::ArmsAction::Idle;
            }
            Auto(a) => r.auto = a,
            AutoOnGrab => r.auto_on_grab = true,
            AutoHeal(h) => r.auto_heal = h,
            Labels(l) => show.0 = l,
            Measure(n) => r.want_sample = Some((n, true)),
            Sample(n) => r.want_sample = Some((n, false)),
            Shot(n) => {
                let file = format!("{}_{}.png", r.batch.name, n);
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(r.out.join(&file)));
                r.shots.push(file);
            }
            Mark(m) => {
                let t = r.t;
                r.marks.push((m, t));
            }
            End => r.done = true,
        }
    }
    // aiming
    let aim_point = |tag_e: Entity, h: f32, creatures: &Query<(Entity, &mut Creature, &mut Transform, Option<&RaptorAi>, Option<&CreatureRig>), Without<Player>>| -> Option<Vec3> {
        let (_, _, tf, _, rig) = creatures.get(tag_e).ok()?;
        let base = rig.and_then(|r| r.pelvis).and_then(|b| gts.get(b).ok()).map(|g| g.translation()).unwrap_or(tf.translation + Vec3::Y);
        Some(base + Vec3::Y * h)
    };
    let mut aim_at: Option<Vec3> = None;
    if let Some((tag, h)) = r.aim {
        if let Some((_, e)) = r.tags.iter().find(|t| t.0 == tag) {
            if creatures.get(*e).map_or(false, |c| c.1.dead) {
                r.auto = false;
            } else {
                aim_at = aim_point(*e, h, &creatures);
            }
        }
    } else if let Some(h) = r.aim_nearest {
        let mut best: Option<(f32, Entity)> = None;
        for (e, c, tf, ai, _) in creatures.iter() {
            if c.dead || ai.is_none() {
                continue;
            }
            let d = Vec2::new(tf.translation.x - jtf.translation.x, tf.translation.z - jtf.translation.z).length();
            if best.map_or(true, |b| d < b.0) {
                best = Some((d, e));
            }
        }
        if let Some((_, e)) = best {
            aim_at = aim_point(e, h, &creatures);
        }
    }
    if let Some(target) = aim_at {
        let eye = jtf.translation + Vec3::Y * p.eye;
        let to = target - eye;
        p.yaw = (-to.x).atan2(-to.z);
        p.pitch = (to.y / Vec2::new(to.x, to.z).length().max(0.01)).atan();
    }
    // fire control
    if r.auto_on_grab && !r.auto && log.0.iter().any(|l| matches!(l.ev, CEv::Grab { .. })) {
        r.auto = true;
    }
    r.reload_cd = (r.reload_cd - dt).max(0.0);
    if r.auto {
        let i = arsenal.index;
        if arsenal.mag[i] == 0 {
            if r.reload_cd == 0.0 && arsenal.reserve[i] > 0 {
                keys.press(KeyCode::KeyR);
                r.reload_cd = 2.5;
            }
        } else if r.reload_cd == 0.0 {
            mouse.press(MouseButton::Left);
        }
    }
    // wound bookkeeping + heal scaffold
    let wounded = p.wounds.state != kk_mechanics::wounds::WoundState::Healthy;
    let tnow = r.t;
    if wounded && !r.last_wound {
        r.wound_onsets.push(tnow);
        r.heal_cd = 0.25;
    }
    r.last_wound = wounded;
    if wounded && r.auto_heal && p.alive() {
        r.heal_cd -= dt;
        if r.heal_cd <= 0.0 {
            let deaths = p.wounds.deaths;
            p.wounds = kk_mechanics::wounds::Wounds::new(deaths);
            r.last_wound = false;
        }
    }
    if !p.alive() && r.jack_dead_at.is_none() {
        r.jack_dead_at = Some(r.t);
    }
    if r.done && r.t > script.last().map(|s| s.0).unwrap_or(0.0) + 0.2 {
        exit.write(AppExit::Success);
    }
}

#[allow(clippy::too_many_arguments)]
fn sampler(
    mut r: ResMut<TRunner>,
    assets: Res<CreatureAssets>,
    creatures: Query<(&Creature, &GlobalTransform, Option<&CreatureRig>)>,
    players: Query<&AnimationPlayer>,
    gts: Query<&GlobalTransform>,
    children: Query<&Children>,
    aabbs: Query<(&Aabb, &GlobalTransform), With<Mesh3d>>,
) {
    let Some((name, _full)) = r.want_sample.take() else { return };
    let tags = r.tags.clone();
    for (tag, e) in tags {
        let Ok((c, agt, rig)) = creatures.get(e) else { continue };
        let mut s = Sampled { name: name.to_string(), tag: tag.to_string(), kind: c.kind, clip: c.clip.clone(), scale: c.scale, ..default() };
        s.clips = assets.rigs[c.kind].names.len();
        s.rig_ok = rig.is_some();
        if let Some(rig) = rig {
            if let Ok(pl) = players.get(rig.player) {
                if let Some(node) = assets.rigs[c.kind].nodes.get(&c.clip) {
                    s.seek = pl.animation(*node).map(|a| a.seek_time()).unwrap_or(-1.0);
                }
            }
            let inv = agt.affine().inverse();
            s.pose = rig.bones.iter().filter_map(|b| gts.get(*b).ok()).map(|g| inv.transform_point3(g.translation())).collect();
            let low = rig.feet.iter().filter_map(|f| gts.get(*f).ok()).map(|g| g.translation().y).fold(f32::MAX, f32::min);
            s.foot_low = (low != f32::MAX).then_some(low);
        }
        let (mut mn, mut mx) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let mut any = false;
        for d in children.iter_descendants(e) {
            if let Ok((a, gt)) = aabbs.get(d) {
                for sx in [-1.0, 1.0] {
                    for sy in [-1.0, 1.0] {
                        for sz in [-1.0, 1.0] {
                            let corner = Vec3::from(a.center) + Vec3::from(a.half_extents) * Vec3::new(sx, sy, sz);
                            let w = gt.transform_point(corner);
                            mn = mn.min(w);
                            mx = mx.max(w);
                            any = true;
                        }
                    }
                }
            }
        }
        if any {
            s.extent = Some((mn, mx));
        }
        r.samples.push(s);
    }
}

#[allow(clippy::too_many_arguments)]
fn record(
    mut r: ResMut<TRunner>,
    time: Res<Time>,
    log: Res<CreatureLog>,
    assets: Res<CreatureAssets>,
    creatures: Query<(&Creature, &Transform, Option<&RaptorAi>)>,
    players: Query<&Player>,
    sfx: Res<crate::sfx::SfxLog>,
) {
    if !r.started {
        return;
    }
    let t = r.t;
    for (tag, e) in r.tags.clone() {
        if let Ok((c, tf, ai)) = creatures.get(e) {
            let (hp, st) = ai.map(|a| (a.m.hp, format!("{:?}", a.m.state))).unwrap_or((0.0, String::new()));
            r.trace.push((t, tag, hp, c.speed, st, tf.translation.x, tf.translation.z));
        }
    }
    let _ = time;
    if r.done && r.t > r.batch.script.last().map(|s| s.0).unwrap_or(0.0) {
        write_report(&r, &log, &assets, &players, &sfx);
    }
}

// ---------------------------------------------------------------------------------------------
// Reports
// ---------------------------------------------------------------------------------------------

fn check(name: &str, evidence: &str, expected: Value, actual: Value, pass: bool) -> Value {
    json!({ "check": name, "evidence": evidence, "expected": expected, "actual": actual, "pass": pass })
}

struct Ctx<'a> {
    r: &'a TRunner,
    log: &'a CreatureLog,
    assets: &'a CreatureAssets,
    sfx: &'a crate::sfx::SfxLog,
}

impl Ctx<'_> {
    fn ent(&self, tag: &str) -> Option<Entity> {
        self.r.tags.iter().find(|t| t.0 == tag).map(|t| t.1)
    }
    fn evs(&self, tag: &str) -> Vec<(f32, &CEv)> {
        let Some(e) = self.ent(tag) else { return vec![] };
        self.log.0.iter().filter(|l| l.who == e).map(|l| (l.t - self.r.off, &l.ev)).collect()
    }
    fn sample(&self, name: &str, tag: &str) -> Option<&Sampled> {
        self.r.samples.iter().find(|s| s.name == name && s.tag == tag)
    }
}

fn pose_delta(a: &[Vec3], b: &[Vec3]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x.distance(*y)).sum::<f32>() / a.len() as f32
}

fn write_report(r: &TRunner, log: &CreatureLog, assets: &CreatureAssets, players: &Query<&Player>, sfx: &crate::sfx::SfxLog) {
    let path = r.out.join(format!("{}.json", r.batch.name));
    if path.exists() {
        return;
    }
    let cx = Ctx { r, log, assets, sfx };
    let _ = players;
    let checks = match r.batch.name {
        "t1_creature_lineup" => t1(&cx),
        "t2_raptor_hunt" => t2(&cx),
        "t3_compy_swarm" => t3(&cx),
        "t4_bronto_walk" => t4(&cx),
        "t5_creature_anims" => t5(&cx),
        _ => vec![],
    };
    let passed = checks.iter().filter(|c| c["pass"] == true).count();
    let events: Vec<String> = log.0.iter().map(|l| format!("{:.2} {} {:?}", l.t - r.off, r.tags.iter().find(|t| t.1 == l.who).map(|t| t.0).unwrap_or("?"), l.ev)).collect();
    let report = json!({
        "batch": r.batch.name,
        "about": r.batch.about,
        "screenshots": r.shots,
        "checks": checks,
        "passed": passed,
        "total": checks.len(),
        "wound_onsets": r.wound_onsets,
        "jack_dead_at": r.jack_dead_at,
        "events": events,
        "samples": r.samples.iter().map(|s| json!({
            "name": s.name, "tag": s.tag, "clip": s.clip, "seek": s.seek, "clips": s.clips, "scale": s.scale,
            "extent": s.extent.map(|(a, b)| json!({"min": [a.x, a.y, a.z], "max": [b.x, b.y, b.z], "size": [b.x - a.x, b.y - a.y, b.z - a.z]})),
            "foot_low": s.foot_low,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap()).ok();
    println!("BATCH {} : {}/{} checks passed -> {}", r.batch.name, passed, checks.len(), path.display());
}

fn manifest() -> Value {
    std::fs::read_to_string(crate::mods::resolve("creatures/manifest.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

// ---- t1 --------------------------------------------------------------------------------------

fn t1(cx: &Ctx) -> Vec<Value> {
    let man = manifest();
    let mut checks = vec![];
    let tags = ["raptor", "compy_s", "compy_l", "rkong", "crab", "kong", "bronto"];
    let mut ok = vec![];
    for t in tags {
        let s = cx.sample("a", t);
        ok.push((t, s.map(|s| s.rig_ok && s.extent.is_some()).unwrap_or(false)));
    }
    checks.push(check(
        "every creature glb loaded, spawned and rigged (raptor, compy x2, raptor_kong, crab, Kong, brontosaurus)",
        "creature_asset_findings.md, manifest.json",
        json!("all true"),
        json!(ok),
        ok.iter().all(|x| x.1),
    ));
    for (tag, name) in [("raptor", "raptor"), ("compy_s", "compy"), ("rkong", "raptor_kong"), ("bronto", "brontosaurus"), ("crab", "crab"), ("kong", "kong")] {
        let want = if name == "kong" { Some(254) } else { man[name]["clip_count"].as_u64().map(|n| n as usize) };
        let got = cx.sample("a", tag).map(|s| s.clips);
        checks.push(check(
            &format!("{name}: clip count in the glb equals the manifest"),
            if name == "kong" { "kong.glb (254 clips; kong_actions.json)" } else { "creatures/manifest.json clip_count" },
            json!(want),
            json!(got),
            want.is_some() && want == got,
        ));
    }
    let size = |tag: &str| cx.sample("a", tag).and_then(|s| s.extent).map(|(a, b)| b - a);
    for (tag, name, scale) in [("raptor", "raptor", 1.0f32), ("compy_s", "compy", 0.2), ("compy_l", "compy", 0.35), ("rkong", "raptor_kong", 1.0), ("bronto", "brontosaurus", 1.0), ("crab", "crab", 1.0)] {
        let h = man[name]["height_m"].as_f64().unwrap_or(0.0) as f32 * scale;
        let got = size(tag).map(|s| s.y);
        checks.push(check(
            &format!("{tag}: standing height {h:.2} m (manifest height_m x runtime scale {scale})"),
            "manifest.json height_m; X01 compy scale Rand(0.2, 0.35)",
            json!(h),
            json!(got),
            got.is_some_and(|g| (g - h).abs() <= 0.2 * h),
        ));
    }
    let kh = size("kong").map(|s| s.y);
    checks.push(check("Kong: standing height plausible for a 25 ft gorilla (5-12 m)", "kong.glb bind pose [L]", json!("5..12"), json!(kh), kh.is_some_and(|h| (5.0..=12.0).contains(&h))));
    let mut feet = vec![];
    for t in tags {
        feet.push((t, cx.sample("a", t).and_then(|s| s.foot_low)));
    }
    checks.push(check(
        "feet stand on the ground: lowest foot bone within -0.1..+0.25 m of y = 0",
        "slice glue (ground calibration)",
        json!("-0.1..0.25"),
        json!(feet),
        feet.iter().all(|f| f.1.is_some_and(|y| (-0.1..=0.25).contains(&y))),
    ));
    let mut anim = vec![];
    for t in tags {
        let (a, b) = (cx.sample("a", t), cx.sample("b", t));
        let moved = match (a, b) {
            (Some(a), Some(b)) => (b.seek - a.seek).abs() > 0.01 || pose_delta(&a.pose, &b.pose) > 0.002,
            _ => false,
        };
        anim.push((t, a.map(|s| s.clip.clone()), moved));
    }
    checks.push(check(
        "idle clip is playing on every creature (clip time or pose advances between two samples)",
        "AnimationPlayer seek_time",
        json!("all true"),
        json!(anim),
        anim.iter().all(|x| x.2),
    ));
    checks
}

// ---- t2 --------------------------------------------------------------------------------------

fn state_name(s: RaptorState) -> String {
    format!("{s:?}")
}

fn t2(cx: &Ctx) -> Vec<Value> {
    let mut checks = vec![];
    let ev = cx.evs("r");
    let acquired = ev.iter().find_map(|(t, e)| if let CEv::Acquired { dist } = e { Some((*t, *dist)) } else { None });
    checks.push(check(
        "raptor perceives Jack: wide-cone sight (130.8 deg half angle, 100 m + 1 m near, line of sight) -> target Jack at ~38 m",
        "X02 Perception A03 (check_vision, Perception_ConeTest)",
        json!("acquired at <= 101 m"),
        json!(acquired),
        acquired.is_some_and(|a| a.1 <= 101.0 && a.1 > 30.0),
    ));
    let seq: Vec<(f32, RaptorState)> = ev.iter().filter_map(|(t, e)| if let CEv::State { to, .. } = e { Some((*t, *to)) } else { None }).collect();
    let order = [RaptorState::Hesite, RaptorState::Fight, RaptorState::Mord, RaptorState::Grab];
    let mut i = 0;
    for (_, s) in &seq {
        if i < order.len() && *s == order[i] {
            i += 1;
        }
    }
    checks.push(check(
        "state order ATTENTE -> HESITE -> FIGHT -> MORD -> GRAB (first bite on Jack grabs)",
        "X02 state machine, A05/A07 (grab counter +0x1d64)",
        json!(["Hesite", "Fight", "Mord", "Grab"]),
        json!(seq.iter().map(|(t, s)| format!("{t:.2} {}", state_name(*s))).collect::<Vec<_>>()),
        i == order.len(),
    ));
    let t_h = seq.iter().find(|s| s.1 == RaptorState::Hesite).map(|s| s.0);
    let t_f = seq.iter().find(|s| s.1 == RaptorState::Fight).map(|s| s.0);
    let hes = t_h.zip(t_f).map(|(a, b)| b - a);
    checks.push(check(
        "hesitation before the chase = Rand(0.5, 1.0) x data (midpoint 0.75 s used) [G]",
        "X02 target acquisition (+0x3764/+0x3768)",
        json!("0.5..1.0 s"),
        json!(hes),
        hes.is_some_and(|h| (0.5..=1.0).contains(&h)),
    ));
    let peak = cx.r.trace.iter().filter(|t| t.1 == "r" && t.4 == "Fight").map(|t| t.3).fold(0.0, f32::max);
    checks.push(check(
        "chase gait = run clip root speed 6.19 m/s (raptor_rootmotion.json)",
        "creature_asset_findings.md (run 2: 6.2 m/s), raptor_rootmotion.json",
        json!(6.1867),
        json!(peak),
        (peak - 6.1867).abs() < 0.2,
    ));
    let bs = ev.iter().find_map(|(_, e)| if let CEv::BiteStart { dist } = e { Some(*dist) } else { None });
    checks.push(check(
        "bite starts inside radius 3.0 m x scale and the 60 degree cone (test_bite)",
        "X02 Attack A07 (exec_test_bite@0x866ed0), kk_mechanics::creatures::raptor::bite_start_ok",
        json!("<= 3.0"),
        json!(bs),
        bs.is_some_and(|d| d <= 3.0),
    ));
    let grab = ev.iter().find_map(|(t, e)| if let CEv::Grab { head_dist } = e { Some((*t, *head_dist)) } else { None });
    checks.push(check(
        "bite reaches Jack: head to Jack's chest <= 2.5 m reach + capsule 0.35 + height offset",
        "X02 exec_bite@0x84cae0: reach 2.5 m, cone 30 deg",
        json!("<= 3.2"),
        json!(grab),
        grab.is_some_and(|g| g.1 <= 3.2),
    ));
    let wound_after_grab = grab.and_then(|g| cx.r.wound_onsets.iter().find(|w| **w >= g.0 - 0.05).map(|w| *w - g.0));
    checks.push(check(
        "the grab wounds Jack through the paf API (Healthy -> Wounded within 0.1 s)",
        "X02 GRAB sends flags 0x4/0x204; wounds.rs H01-H04",
        json!("0..0.1 s"),
        json!(wound_after_grab),
        wound_after_grab.is_some_and(|d| (-0.05..=0.1).contains(&d)),
    ));
    // Colt bands
    let colt = &WEAPONS[0];
    let shots: Vec<(f32, f32, bool)> = ev.iter().filter_map(|(_, e)| if let CEv::Shot { dist, damage, head, .. } = e { Some((*dist, *damage, *head)) } else { None }).collect();
    let band_ok = !shots.is_empty() && shots.iter().all(|s| s.1 == kk_mechanics::weapons::damage_at_distance(colt.mech(), s.0, false));
    checks.push(check(
        "every Colt hit carries the weapon's distance-band damage (R1 5 / R2 25 -> 8/4/2)",
        "G02 weapons.rs, X02 A10 (damage of the paf message)",
        json!("band(dist)"),
        json!(shots.iter().take(8).collect::<Vec<_>>()),
        band_ok,
    ));
    // flinch thresholds: replay the hits through classify_hit
    let mut acc = 0.0;
    let mut hp = rp::HP_MAX_STANDARD;
    let mut expect: Vec<Reaction> = vec![];
    for (_, dmg, head) in &shots {
        if hp <= 0.0 {
            break;
        }
        let o = rp::classify_hit(
            rp::Variant::Standard,
            &rp::HitIn { damage: *dmg, flags: 0x44, head: *head, dir_z: 0.0, from_jack: true, bite_leg_latch: false },
            &mut acc,
            false,
            false,
        );
        hp -= o.damage.min(hp);
        if o.reaction != Reaction::None && hp > 0.0 {
            expect.push(o.reaction);
        }
    }
    let got: Vec<Reaction> = ev.iter().filter_map(|(_, e)| if let CEv::Flinch(r) = e { Some(*r) } else { None }).collect();
    checks.push(check(
        "flinch reactions follow check_paf thresholds 8/11/20/acc 10 (Colt <5 m = 8 -> light every hit; 4 dmg needs accumulated 10)",
        "X02 Hit reception A10 (exec_check_paf@0x849910)",
        json!(expect.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()),
        json!(got.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()),
        got.len() == expect.len() && !expect.is_empty() || (expect.is_empty() && got.is_empty() && !shots.is_empty()),
    ));
    let first_close = shots.iter().find(|s| s.0 < 5.0).map(|s| s.1);
    let died = ev.iter().find(|(_, e)| matches!(e, CEv::Died)).map(|x| x.0);
    let dmg_total: f32 = ev.iter().filter_map(|(_, e)| if let CEv::Damaged(d) = e { Some(*d) } else { None }).sum();
    checks.push(check(
        "hp 50 (variant 0): total damage taken equals hp_max when it dies (head hits x2)",
        "X02 init@0x8315a0 hp_max 50; check_paf mult 2 on head",
        json!(50.0),
        json!({"damage_sum": dmg_total, "died_at": died, "first_close_hit": first_close}),
        died.is_some() && (dmg_total - 50.0).abs() < 0.01,
    ));
    // death timers
    let tm = seq.iter().find(|s| s.1 == RaptorState::Mort).map(|s| s.0);
    let tf = seq.iter().find(|s| s.1 == RaptorState::Fade).map(|s| s.0);
    let tfd = ev.iter().find(|(_, e)| matches!(e, CEv::Faded)).map(|x| x.0);
    let t_ata = seq.iter().find(|s| s.1 == RaptorState::ATerre).map(|s| s.0);
    checks.push(check(
        "death: hp 0 -> A_TERRE -> MORT immediately, MORT lasts 5.0 s, FADE 10.0 s, then the corpse is released",
        "X02 ETAT_MORT@0x8535c0 (5.0), ETAT_FADE@0x846020 (10.0)",
        json!({"mort->fade": 5.0, "fade->faded": 10.0}),
        json!({"died": died, "aterre": t_ata, "mort": tm, "fade": tf, "faded": tfd,
               "mort_len": tm.zip(tf).map(|(a, b)| b - a), "fade_len": tf.zip(tfd).map(|(a, b)| b - a)}),
        tm.zip(tf).is_some_and(|(a, b)| (b - a - 5.0).abs() < 0.12) && tf.zip(tfd).is_some_and(|(a, b)| (b - a - 10.0).abs() < 0.12),
    ));
    // corpse left on the ground between Died and Faded
    let corpse_there = died.map(|d| cx.r.trace.iter().any(|t| t.1 == "r" && t.0 > d + 3.0 && t.0 < d + 5.0));
    checks.push(check("the corpse stays in the world after death (entity traced 3-5 s after the kill)", "X02 MORT/FADE", json!(true), json!(corpse_there), corpse_there == Some(true)));
    checks.push(check(
        "Jack was wounded by the creature (>= 1 wound onset) and the paf API killed nobody unexpectedly (scaffold heals Jack 0.25 s after each wound)",
        "H01-H04 wound model via Player::paf",
        json!(">=1 onset"),
        json!({"onsets": cx.r.wound_onsets, "jack_dead_at": cx.r.jack_dead_at}),
        !cx.r.wound_onsets.is_empty() && cx.r.jack_dead_at.is_none(),
    ));
    let _ = cx.sfx;
    checks
}

// ---- t3 --------------------------------------------------------------------------------------

fn t3(cx: &Ctx) -> Vec<Value> {
    let mut checks = vec![];
    let tags = ["c0", "c1", "c2", "c3", "c4"];
    let hp0: Vec<f32> = tags.iter().filter_map(|t| cx.r.trace.iter().find(|x| x.1 == *t).map(|x| x.2)).collect();
    checks.push(check("compy hp 3.0", "X01 init@0x8315a0 (0x40400000)", json!(3.0), json!(hp0), hp0.len() == tags.len() && hp0.iter().all(|h| *h == 3.0)));
    let scales: Vec<Option<f32>> = tags.iter().map(|t| cx.sample("end", t).map(|s| s.scale)).collect();
    let _ = scales;
    let acq = tags.iter().filter(|t| cx.evs(t).iter().any(|(_, e)| matches!(e, CEv::Acquired { .. }))).count();
    checks.push(check("all compies perceive Jack and chase (target acquired)", "X02 A03 / X01 (same class)", json!(5), json!(acq), acq == 5));
    let fights = tags.iter().filter(|t| cx.evs(t).iter().any(|(_, e)| matches!(e, CEv::State { to: RaptorState::Fight, .. }))).count();
    checks.push(check("all compies reach FIGHT", "X02 state machine", json!(5), json!(fights), fights == 5));
    let mut bites = vec![];
    for t in tags {
        for (tm, e) in cx.evs(t) {
            if let CEv::BiteHit { damage, flags, .. } = e {
                bites.push((t, tm, *damage, *flags));
            }
        }
    }
    checks.push(check(
        "compy bite sends paf flags 0x1000 with damage 1",
        "X01 table (COMPY_BITE_FLAGS 0x1000, COMPY_BITE_DAMAGE 1), exec_bite@0x84cae0",
        json!({"flags": 0x1000, "damage": 1.0, "count": ">=1"}),
        json!(bites),
        !bites.is_empty() && bites.iter().all(|b| b.2 == 1.0 && b.3 == 0x1000),
    ));
    let start_ok = tags.iter().all(|t| {
        cx.evs(t).iter().all(|(_, e)| match e {
            CEv::BiteStart { dist } => *dist <= 3.0,
            _ => true,
        })
    });
    checks.push(check("compy bites start inside the (scaled) bite radius (3 m x scale or < sqrt 2 m)", "X02 test_bite", json!("<= 3.0"), json!(start_ok), start_ok));
    let wounded_by_compy = bites.first().map(|b| cx.r.wound_onsets.iter().any(|w| *w >= b.1 - 0.05 && *w <= b.1 + 0.15));
    checks.push(check("a compy bite wounds Jack (light class, flags 0x1000)", "H01-H04 via Player::paf", json!(true), json!({"bite": bites.first(), "onsets": cx.r.wound_onsets}), wounded_by_compy == Some(true)));
    let died: Vec<&str> = tags.iter().copied().filter(|t| cx.evs(t).iter().any(|(_, e)| matches!(e, CEv::Died))).collect();
    checks.push(check("all five compies die", "X01 hp 3", json!(5), json!(died), died.len() == 5));
    let one_hit = died.iter().all(|t| {
        let shots: Vec<f32> = cx.evs(t).iter().filter_map(|(_, e)| if let CEv::Shot { damage, .. } = e { Some(*damage) } else { None }).collect();
        shots.len() == 1 && shots[0] >= 3.0
    });
    checks.push(check(
        "each compy dies from exactly one Colt hit (band damage 4-8 >= hp 3)",
        "X01 hp 3.0 + G02 Colt bands",
        json!("1 hit, damage >= 3"),
        json!(died.iter().map(|t| (t, cx.evs(t).iter().filter_map(|(_, e)| if let CEv::Shot { damage, dist, .. } = e { Some((*dist, *damage)) } else { None }).collect::<Vec<_>>())).collect::<Vec<_>>()),
        one_hit && !died.is_empty(),
    ));
    let waits: Vec<f32> = tags.iter().filter_map(|t| cx.r.trace.iter().find(|x| x.1 == *t && x.0 > 0.2).map(|x| x.3)).collect();
    let _ = waits;
    checks
}

// ---- t4 --------------------------------------------------------------------------------------

fn t4(cx: &Ctx) -> Vec<Value> {
    let mut checks = vec![];
    let tr: Vec<_> = cx.r.trace.iter().filter(|t| t.1 == "b").collect();
    let a = tr.iter().find(|t| t.0 >= 2.0);
    let b = tr.iter().find(|t| t.0 >= 12.0);
    let walk_speed = cx.assets.speed_of(kind_index("brontosaurus").unwrap(), "walk__brontosaurus_000");
    let measured = a.zip(b).map(|(a, b)| (((b.5 - a.5).powi(2) + (b.6 - a.6).powi(2)).sqrt()) / (b.0 - a.0));
    checks.push(check(
        "the brontosaurus walks at the walk clip's root speed (3.26 m/s) along its path",
        "brontosaurus_rootmotion.json (walk 5.21 m / 1.6 s); X14 path walker",
        json!(walk_speed),
        json!(measured),
        measured.is_some_and(|m| (m - walk_speed).abs() < 0.15),
    ));
    let stomps: Vec<(f32, usize, f32, u32)> = cx.evs("b").iter().filter_map(|(t, e)| if let CEv::Stomp { foot, dist, flags } = e { Some((*t, *foot, *dist, *flags)) } else { None }).collect();
    checks.push(check(
        "a foot closer than 3 m (dist^2 < 9) to Jack sends paf 0x10",
        "X14 pnjbronto_cb_afterblend@0x7f22f0 l.231-234",
        json!({"flags": 0x10, "dist": "< 3.0", "count": ">=1"}),
        json!(stomps),
        !stomps.is_empty() && stomps.iter().all(|s| s.2 < 3.0 && s.3 == 0x10),
    ));
    let w = stomps.first().and_then(|s| cx.r.wound_onsets.iter().find(|w| **w >= s.0 - 0.05).map(|w| *w - s.0));
    checks.push(check("the stomp wounds Jack through the paf API", "H01-H04 via Player::paf", json!("<= 0.1 s"), json!(w), w.is_some_and(|d| d <= 0.1)));
    let feet: std::collections::BTreeSet<usize> = stomps.iter().map(|s| s.1).collect();
    checks.push(check("more than one foot passes within 3 m (4 stomp bones: both hands and both feet)", "X14 per-foot test", json!(">=2"), json!(feet), feet.len() >= 2));
    // heading: straight leg towards +X first
    let yaw_ok = a.is_some_and(|a| a.5 < 0.0 + 1.0e4);
    let _ = yaw_ok;
    let dz = a.zip(b).map(|(a, b)| (b.6 - a.6).abs());
    checks.push(check("it follows the lane (no drift off the straight first leg)", "X14 walk/exec_move along waypoints", json!("< 1 m"), json!(dz), dz.is_some_and(|d| d < 1.0)));
    checks
}

// ---- t5 --------------------------------------------------------------------------------------

fn t5(cx: &Ctx) -> Vec<Value> {
    let mut checks = vec![];
    let tags = ["raptor", "compy", "rkong", "crab", "kong", "bronto"];
    let man = manifest();
    for tag in tags {
        let mut row = vec![];
        let mut ok = true;
        let mut prev: Option<&Sampled> = None;
        for act in ACTIONS {
            let Some(s) = cx.sample(act, tag) else { ok = false; continue };
            let kind = s.kind;
            let defined = !KINDS[kind].clips[ACTIONS.iter().position(|a| *a == act).unwrap()].is_empty();
            // the manifest says which actions exist
            let in_manifest = match KINDS[kind].name {
                "kong" | "crab" => defined,
                n => !man[n]["clips"][act].is_null(),
            };
            let delta = prev.map(|p| pose_delta(&p.pose, &s.pose)).unwrap_or(0.0);
            let playing = s.seek > 0.0 || !defined;
            row.push(json!({"action": act, "clip": s.clip, "defined": defined, "seek": s.seek, "pose_delta": delta}));
            if defined != in_manifest {
                ok = false;
            }
            if defined && !playing {
                ok = false;
            }
            if defined && act != "idle" && delta < 0.01 && KINDS[kind].name != "crab" {
                ok = false;
            }
            if defined {
                prev = Some(s);
            }
        }
        checks.push(check(
            &format!("{tag}: every defined action plays its clip (seek time advances) and changes the pose; undefined actions are absent from the manifest"),
            "manifest.json clips{}; *_actions.json",
            json!("clips as in the manifest"),
            json!(row),
            ok,
        ));
    }
    checks
}
