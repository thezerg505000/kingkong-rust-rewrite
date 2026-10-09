//! Jack's guns: hitscan fire, magazines/reload, switching, arm animation.
//!
//! Numbers and rules come from `kk_mechanics::weapons` (G02/G03/G10/G11): weapon table, damage
//! bands via `damage_at_distance_sq` (int-truncated, squared Jack-to-target-object distance),
//! the shotgun's 25-ray pattern (`shotgun_pattern`), reload transfer (`Ammo::reload`) and fire
//! timing (shot timer 0.4/0.5/0.4 for Colt/Shotgun/Sniper, Tommy 0.1 s auto interval).

use crate::anim::{self, GameState, RigPlayer, Rigs};
use crate::player::{ArmsRig, ArmsScene, MainCam, Player};
use crate::spec::*;
use crate::world::{Arena, VIEW_LAYER};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;
use kk_mechanics::weapons::{self as mech, Ammo};
use rand::Rng;
use std::f32::consts::FRAC_PI_2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArmsAction {
    Idle,
    Fire { t: f32 },
    Reload { t: f32, committed: bool },
    Swap { t: f32, to: usize, swapped: bool },
}

#[derive(Resource)]
pub struct Arsenal {
    pub index: usize,
    pub mag: [u32; 4],
    pub reserve: [u32; 4],
    /// Jack's shot timer (`+0x3bc8`, G02): counts down per frame; set to the weapon's stored value after a shot
    pub cooldown: f32,
    /// Tommy burst: shots fired since the trigger went down (`this[0x408]`) and seconds held since the first one
    pub burst_n: u32,
    pub burst_t: f32,
    pub action: ArmsAction,
    pub weapon_entity: Option<Entity>,
    pub wanted_weapon: Option<usize>,
    pub shots_fired: u32,
    pub hits: u32,
    pub last_hit: Option<(&'static str, f32)>,
    /// F1 debug: override the idle clip with an arbitrary arms clip
    pub debug_clip: Option<usize>,
    /// procedural viewmodel recoil (0..1), decays each frame
    pub recoil: f32,
    /// per shot: (rays fired, impacts on any surface or the rex, rays that hit nothing); G11 check
    pub ray_log: Vec<(u32, u32, u32)>,
    /// spears.rs: a spear / bone is in Jack's hand (guns blocked, gun hidden)
    pub spear_held: bool,
    /// spears.rs: what the arms do with the spear
    pub spear_pose: SpearPose,
}

/// Arms actions while a spear / bone is held (arms action kit of `_PJ_J`): hold = action 0x2d / 0x37
/// (weapon type 5) -> clip 31, throw = action 0x5c -> clips 55 (wind-up) + 56 (release), the
/// `H_ETAT_IA_lance` throw action [C ids]; stab = action 0x5d -> clip 5 [L].
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum SpearPose {
    #[default]
    Hold,
    /// right mouse / aim: wound up, arm back, held on the last frame of clip 55
    WindUp,
    Throw { t: f32 },
    Stab { t: f32 },
}

pub const CLIP_SPEAR_HOLD: &str = "idle_c18__arms_031";
pub const CLIP_SPEAR_WINDUP: &str = "move_c28__arms_055";
pub const CLIP_SPEAR_THROW: &str = "move_c28_b__arms_056";
pub const CLIP_SPEAR_STAB: &str = "action_c05__arms_005";

pub const SWAP_TIME: f32 = 0.55; // [G] no swap clip mapped per weapon yet

impl Default for Arsenal {
    fn default() -> Self {
        Self {
            index: 0,
            mag: WEAPONS.map(|w| w.clip()),
            reserve: WEAPONS.map(|w| w.reserve),
            cooldown: 0.0,
            burst_n: 0,
            burst_t: 0.0,
            action: ArmsAction::Idle,
            weapon_entity: None,
            wanted_weapon: Some(0),
            shots_fired: 0,
            hits: 0,
            last_hit: None,
            debug_clip: None,
            recoil: 0.0,
            ray_log: Vec::new(),
            spear_held: false,
            spear_pose: SpearPose::Hold,
        }
    }
}

impl Arsenal {
    pub fn current(&self) -> &'static WeaponDef {
        &WEAPONS[self.index]
    }
    /// 0..1 how far the viewmodel is lowered during a weapon swap
    pub fn swap_dip(&self) -> f32 {
        match self.action {
            ArmsAction::Swap { t, .. } => {
                let h = SWAP_TIME * 0.5;
                if t < h { t / h } else { (1.0 - (t - h) / h).max(0.0) }
            }
            _ => 0.0,
        }
    }
    pub fn refill(&mut self) {
        let keep = self.index;
        // the mounted gun entity survives the reset: forgetting it here left the old gun in the hands
        // when the next one was mounted (the "two guns overlapping" bug after a respawn / F8)
        let mounted = self.weapon_entity;
        *self = Arsenal::default();
        self.index = keep;
        self.weapon_entity = mounted;
        self.wanted_weapon = Some(keep);
    }
}

/// Hit spheres published each frame by the Rex (world space).
#[derive(Resource, Default)]
pub struct RexHitbox {
    pub spheres: Vec<(Vec3, f32, &'static str)>,
    pub alive: bool,
    /// the rex actor's origin: `fn@0x004150e0(shooter, target)` measures between object origins (G02)
    pub root: Vec3,
}

#[derive(Message)]
#[allow(dead_code)] // point/bone kept for hit reactions and debugging
pub struct RexDamage {
    pub amount: f32,
    /// squared Jack-to-rex-object distance the damage band was chosen with
    pub dist_sq: f32,
    pub point: Vec3,
    pub bone: &'static str,
}



#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WeaponsSet;

pub struct WeaponsPlugin;

impl Plugin for WeaponsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Arsenal>()
            .init_resource::<RexHitbox>()
            .add_message::<RexDamage>()
            .add_systems(Update, split_luger_toggle)
            .add_systems(
                Update,
                (weapon_input, mount_weapon, drive_arms, drive_gun_parts, luger_toggle)
                    .chain()
                    .in_set(WeaponsSet)
                    .after(crate::player::PlayerSet)
                    .run_if(in_state(GameState::Playing))
                    .run_if(crate::kong::jack_active),
            );
    }
}

/// Ray vs. sphere, returns distance along the (unit) ray.
fn ray_sphere(o: Vec3, d: Vec3, c: Vec3, r: f32) -> Option<f32> {
    let oc = o - c;
    let b = oc.dot(d);
    let cc = oc.length_squared() - r * r;
    let disc = b * b - cc;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    if t > 0.0 { Some(t) } else { None }
}

fn weapon_input(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    gamepads: Query<&Gamepad>,
    mut arsenal: ResMut<Arsenal>,
    mut players: Query<(&mut Player, &Transform)>,
    cam: Query<&GlobalTransform, With<MainCam>>,
    arena: Res<Arena>,
    hitbox: Res<RexHitbox>,
    mut dmg: MessageWriter<RexDamage>,
    mut gun: MessageWriter<crate::events::GunEvent>,
    (rigs, spheres): (Res<Rigs>, Option<Res<crate::creatures::CreatureSpheres>>),
    arms: Query<&ArmsRig, With<ArmsScene>>,
    gts: Query<&GlobalTransform>,
) {
    use crate::events::GunEvent;
    let Ok((mut p, jack_t)) = players.single_mut() else { return };
    let Ok(cam) = cam.single() else { return };
    let jack_pos = jack_t.translation;
    let dt = time.delta_secs();
    arsenal.cooldown = (arsenal.cooldown - dt).max(0.0);
    arsenal.recoil = (arsenal.recoil - dt * 9.0).max(0.0);
    if !p.alive() {
        return;
    }
    // a spear in hand blocks the gun until it is thrown or dropped (`Jack_IsNonFirearmHeld`, I03) [C]
    if arsenal.spear_held {
        return;
    }

    let mut fire_held = mouse.pressed(MouseButton::Left);
    let mut fire_pressed = mouse.just_pressed(MouseButton::Left);
    let mut reload = keys.just_pressed(KeyCode::KeyR);
    let mut next = 0i32;
    for g in &gamepads {
        fire_held |= g.pressed(GamepadButton::RightTrigger2);
        fire_pressed |= g.just_pressed(GamepadButton::RightTrigger2);
        reload |= g.just_pressed(GamepadButton::West);
        if g.just_pressed(GamepadButton::RightTrigger) || g.just_pressed(GamepadButton::North) { next += 1; }
        if g.just_pressed(GamepadButton::LeftTrigger) { next -= 1; }
    }
    if scroll.delta.y > 0.0 { next -= 1; }
    if scroll.delta.y < 0.0 { next += 1; }
    let mut want = None;
    for (i, k) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4].iter().enumerate() {
        if keys.just_pressed(*k) { want = Some(i); }
    }
    if next != 0 {
        want = Some(((arsenal.index as i32 + next).rem_euclid(4)) as usize);
    }
    if keys.just_pressed(KeyCode::F1) {
        let n = rigs.arms.names.len();
        arsenal.debug_clip = match arsenal.debug_clip {
            None => Some(0),
            Some(i) if i + 1 < n => Some(i + 1),
            Some(_) => None,
        };
    }
    if let Some(w) = want {
        if w != arsenal.index && !matches!(arsenal.action, ArmsAction::Swap { .. }) {
            arsenal.action = ArmsAction::Swap { t: 0.0, to: w, swapped: false };
            gun.write(GunEvent::Swap { to: w });
        }
    }

    let w = arsenal.current();
    let idx = arsenal.index;
    match arsenal.action {
        ArmsAction::Swap { mut t, to, mut swapped } => {
            t += dt;
            if !swapped && t >= SWAP_TIME * 0.5 {
                arsenal.index = to;
                arsenal.wanted_weapon = Some(to);
                swapped = true;
            }
            arsenal.action = if t >= SWAP_TIME { ArmsAction::Idle } else { ArmsAction::Swap { t, to, swapped } };
            return;
        }
        ArmsAction::Reload { mut t, mut committed } => {
            t += dt;
            let dur = rigs.arms.duration(w.clip_reload);
            if !committed && t * ANIM_HZ >= w.reload_commit_frame.min(dur * ANIM_HZ * 0.9) {
                // H_exec_loading_weapon via kk_mechanics Ammo::reload: min(clip-mag, min(reserve, clip)),
                // shotgun offers one round per cycle (G03) [C]
                let mut ammo = Ammo { mag: arsenal.mag[idx], reserve: arsenal.reserve[idx], reserve_max: w.reserve };
                let n = ammo.reload(w.mech());
                arsenal.mag[idx] = ammo.mag;
                arsenal.reserve[idx] = ammo.reserve;
                committed = true;
                gun.write(GunEvent::ReloadCommit { w: idx, rounds: n });
            }
            if fire_pressed && w.one_at_a_time() && arsenal.mag[idx] > 0 && committed {
                arsenal.action = ArmsAction::Idle; // shotgun reload can be interrupted
                gun.write(GunEvent::ReloadEnd { w: idx });
            } else if t >= dur {
                let more = w.one_at_a_time() && arsenal.mag[idx] < w.clip() && arsenal.reserve[idx] > 0;
                arsenal.action = if more { ArmsAction::Reload { t: 0.0, committed: false } } else { ArmsAction::Idle };
                if more {
                    gun.write(GunEvent::ReloadStart { w: idx, first: false });
                } else {
                    gun.write(GunEvent::ReloadEnd { w: idx });
                }
                return;
            } else {
                arsenal.action = ArmsAction::Reload { t, committed };
                return;
            }
        }
        ArmsAction::Fire { t } => {
            let t = t + dt;
            let dur = if w.clip_fire.is_empty() { w.shot_timer() } else { rigs.arms.duration(w.clip_fire) };
            arsenal.action = if t >= dur { ArmsAction::Idle } else { ArmsAction::Fire { t } };
        }
        ArmsAction::Idle => {}
    }

    let wants_reload = reload || (fire_pressed && arsenal.mag[idx] == 0);
    if wants_reload && arsenal.mag[idx] < w.clip() && arsenal.reserve[idx] > 0 && !matches!(arsenal.action, ArmsAction::Reload { .. }) {
        arsenal.action = ArmsAction::Reload { t: 0.0, committed: false };
        gun.write(GunEvent::ReloadStart { w: idx, first: true });
        return;
    }
    if fire_pressed && arsenal.mag[idx] == 0 && arsenal.reserve[idx] == 0 {
        gun.write(GunEvent::Empty { w: idx });
    }

    // ---- trigger (H_callback_tir, G02) ----
    // Tommy: while the trigger is held, shot n+1 fires when n * 0.1 <= held time since the first shot [C];
    // a new burst (or any other weapon's shot) waits for the stored shot timer [C value, L meaning].
    let interval = w.auto_interval();
    if !fire_held {
        arsenal.burst_n = 0;
        arsenal.burst_t = 0.0;
    } else if arsenal.burst_n > 0 {
        arsenal.burst_t += dt;
    }
    let trigger = if interval > 0.0 {
        fire_held && (arsenal.burst_n as f32 * interval <= arsenal.burst_t + 1e-4)
    } else {
        fire_pressed
    };
    let timer_gate = !(interval > 0.0 && arsenal.burst_n > 0) && arsenal.cooldown > 1e-3;
    if !trigger || timer_gate || arsenal.mag[idx] == 0 {
        return;
    }
    // ---- fire ----
    arsenal.cooldown = w.shot_timer();
    if interval > 0.0 {
        if arsenal.burst_n == 0 {
            arsenal.burst_t = 0.0;
        }
        arsenal.burst_n += 1;
    }
    arsenal.mag[idx] -= 1;
    arsenal.shots_fired += 1;
    arsenal.action = ArmsAction::Fire { t: 0.0 };
    arsenal.recoil = 1.0;
    p.kick += w.kick;

    let origin = cam.translation();
    let fwd = cam.forward().as_vec3();
    let right = cam.right().as_vec3();
    let up = cam.up().as_vec3();
    // world-space muzzle: the barrel tip of the mounted weapon mesh
    let muzzle = arms
        .single()
        .ok()
        .and_then(|r| gts.get(r.socket).ok())
        .map(|g| g.transform_point(crate::fx::MUZZLE_LOCAL[idx]))
        .unwrap_or(origin + fwd * 0.6 + right * 0.18 - up * 0.12);
    gun.write(GunEvent::Fired { w: idx, muzzle, dir: fwd });
    let mut rng = rand::thread_rng();
    let pellets = w.pellets().max(1);
    // shotgun: the recovered 25-ray grid (-10..+10 deg in 5 deg steps, +-0.05 rad jitter except the centre ray) [C]
    let pattern = if pellets > 1 {
        mech::shotgun_pattern(|lo, hi| rng.gen_range(lo.min(hi)..=lo.max(hi)))
    } else {
        Vec::new()
    };
    let mut shot_rec = (0u32, 0u32, 0u32);
    for i in 0..pellets {
        shot_rec.0 += 1;
        let (ax, ay) = if pellets > 1 {
            pattern[i as usize % pattern.len()]
        } else if w.spread_deg > 0.0 && !p.aiming {
            let s = w.spread_deg.to_radians();
            (rng.gen_range(-s..s), rng.gen_range(-s..s))
        } else {
            (0.0, 0.0)
        };
        let dir = (fwd + right * ax.tan() + up * ay.tan()).normalize();
        let mut world_t = arena.raycast(origin, dir, w.range());
        // a creature in front of the wall takes the bullet (creatures.rs spawns its flesh impact)
        if let Some(cs) = spheres.as_ref() {
            let creature_t = cs.0.iter().filter_map(|(c, r)| ray_sphere(origin, dir, *c, *r)).filter(|t| *t <= w.range()).fold(f32::MAX, f32::min);
            if creature_t < world_t.map_or(f32::MAX, |x| x.0) {
                world_t = None;
            }
        }
        let mut rex_t: Option<(f32, &'static str)> = None;
        if hitbox.alive {
            for (c, r, bone) in &hitbox.spheres {
                if let Some(t) = ray_sphere(origin, dir, *c, *r) {
                    if t <= w.range() && rex_t.map_or(true, |(b, _)| t < b) {
                        rex_t = Some((t, bone));
                    }
                }
            }
        }
        match (rex_t, world_t) {
            (Some((rt, bone)), wt) if wt.map_or(true, |(wt, _)| rt < wt) => {
                let point = origin + dir * rt;
                // damage band by the SQUARED distance between Jack's and the rex's object origins (fn@0x004150e0), G02
                let dist_sq = (hitbox.root - jack_pos).length_squared();
                let amount = mech::damage_at_distance_sq(w.mech(), dist_sq, false) as f32;
                dmg.write(RexDamage { amount, dist_sq, point, bone });
                arsenal.hits += 1;
                arsenal.last_hit = Some((bone, amount));
                shot_rec.1 += 1;
                gun.write(GunEvent::Impact { pos: point, normal: -dir, rex: true, damage: amount, dist: dist_sq.sqrt() });
            }
            (_, Some((wt, n))) => {
                shot_rec.1 += 1;
                gun.write(GunEvent::Impact { pos: origin + dir * wt, normal: n, rex: false, damage: 0.0, dist: wt });
            }
            _ => {
                shot_rec.2 += 1;
            }
        }
        let _ = i;
    }
    arsenal.ray_log.push(shot_rec);
    let _ = &mut commands;
}

/// The gun (and its parts) currently in Jack's hands.
#[derive(Component)]
pub struct MountedWeapon;

/// A mounted gun part (`WeaponDef::parts[i]` of weapon `w`), a child of the WeaponSocket.
#[derive(Component)]
pub struct GunPart {
    pub w: usize,
    pub i: usize,
}

/// Keep the right weapon mesh parented under the arms' WeaponSocket (B_Jaf_Anex01).
fn mount_weapon(
    mut commands: Commands,
    mut arsenal: ResMut<Arsenal>,
    rigs: Res<Rigs>,
    assets: Res<AssetServer>,
    arms: Query<&ArmsRig, With<ArmsScene>>,
    mounted: Query<Entity, With<MountedWeapon>>,
) {
    let Some(w) = arsenal.wanted_weapon else { return };
    let Ok(rig) = arms.single() else { return };
    // everything mounted before goes, not only the entity we remember
    for e in &mounted {
        commands.entity(e).despawn();
    }
    if let Some(old) = arsenal.weapon_entity.take() {
        if let Ok(mut ec) = commands.get_entity(old) {
            ec.despawn();
        }
    }
    // The weapon glbs carry the same Z-up->Y-up root rotation; the socket already lives in
    // Jade space, so cancel it (rotate +90° about X).
    let e = commands
        .spawn((
            Name::new(WEAPONS[w].name),
            MountedWeapon,
            WorldAssetRoot(rigs.weapon_scenes[w].clone()),
            Transform::from_rotation(Quat::from_rotation_x(FRAC_PI_2)),
            Visibility::default(),
        ))
        .observe(on_weapon_ready)
        .id();
    commands.entity(rig.socket).add_child(e);
    // magazine / pump / bolt / round: modelled in the same weapon space, so their rest transform is
    // the identity under the socket (parts missing from an older asset folder are skipped)
    for (i, part) in WEAPONS[w].parts.iter().enumerate() {
        if !crate::mods::resolve(part.glb).exists() {
            continue;
        }
        let holder = commands
            .spawn((Name::new(part.glb), MountedWeapon, GunPart { w, i }, Transform::default(), Visibility::default()))
            .id();
        let scene = commands
            .spawn((
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(part.glb))),
                Transform::from_rotation(Quat::from_rotation_x(FRAC_PI_2)),
            ))
            .observe(on_weapon_ready)
            .id();
        commands.entity(holder).add_child(scene);
        commands.entity(rig.socket).add_child(holder);
    }
    arsenal.weapon_entity = Some(e);
    arsenal.wanted_weapon = None;
}

/// Gun parts: identity (seated) by default; during the reload clip they follow their prop bone
/// relative to the weapon bone, loose rounds only show while the clip carries them; the pump / bolt
/// cycle after each shot [G]. The whole gun hides while a spear is in hand.
fn drive_gun_parts(
    arsenal: Res<Arsenal>,
    arms: Query<&ArmsRig, With<ArmsScene>>,
    gts: Query<&GlobalTransform>,
    mut parts: Query<(&GunPart, &mut Transform, &mut Visibility)>,
    mut guns: Query<&mut Visibility, (With<MountedWeapon>, Without<GunPart>)>,
) {
    let hide_all = arsenal.spear_held;
    for mut v in &mut guns {
        let want = if hide_all { Visibility::Hidden } else { Visibility::Inherited };
        if *v != want {
            *v = want;
        }
    }
    let rig = arms.single().ok();
    let bone = |k: usize| rig.and_then(|r| r.anex[k]).and_then(|e| gts.get(e).ok());
    for (gp, mut tf, mut vis) in &mut parts {
        let def = &WEAPONS[gp.w].parts[gp.i];
        let mut t = Transform::IDENTITY;
        let mut shown = !def.hidden_at_rest;
        match arsenal.action {
            // past the 0.12 s cross-fade the reload clip alone drives the prop bones
            ArmsAction::Reload { t: rt, .. } if rt > 0.12 && gp.w == arsenal.index => {
                if let (Some(k), Some(a1)) = (def.anex, bone(0)) {
                    if let Some(ak) = bone(k as usize - 1) {
                        let rel = a1.affine().inverse() * ak.affine();
                        let rt = Transform::from_matrix(Mat4::from(rel));
                        // a prop parked far from the gun is off screen: the clip's way of hiding it
                        let near = rt.translation.length() < 0.6;
                        shown = near || !def.hidden_at_rest;
                        if near {
                            t = rt;
                        }
                    }
                }
            }
            ArmsAction::Fire { t: ft } if gp.w == arsenal.index => {
                let k = ((ft - 0.1) / 0.3).clamp(0.0, 1.0);
                let stroke = (k * std::f32::consts::PI).sin();
                // Jade weapon space: the barrel points down -Y, the receiver is toward +Y
                match def.motion {
                    PartMotion::Pump => t.translation.y += 0.075 * stroke,
                    PartMotion::Bolt => t.translation.y += 0.06 * stroke,
                    PartMotion::None => {}
                }
            }
            _ => {}
        }
        *tf = t;
        let want = if shown && !hide_all { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

/// Luger toggle (breech block + toggle links) as two separate pieces of the Luger mesh, so the toggle
/// can stand open when the magazine is empty (the Luger's hold-open). Split from primitive 0 of the
/// recovered GEO by position [G: the GEO has no separate toggle object].
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum LugerToggle {
    Front,
    Rear,
}

const TOGGLE_REAR_PIVOT: Vec2 = Vec2::new(0.033, 0.096);
const TOGGLE_JOINT: Vec2 = Vec2::new(-0.005, 0.096);
const TOGGLE_FRONT: Vec2 = Vec2::new(-0.085, 0.09);
const TOGGLE_OPEN_DEG: f32 = 55.0;

/// (y, z) of a triangle centroid in the Luger's Jade mesh space -> which toggle piece it belongs to.
fn toggle_piece(c: Vec3) -> Option<LugerToggle> {
    if c.z > 0.084 && c.y > -0.075 {
        Some(if c.y >= TOGGLE_JOINT.x { LugerToggle::Rear } else { LugerToggle::Front })
    } else {
        None
    }
}

/// Toggle piece transforms (mesh space) for `open` in 0..1: the rear link turns up about the rear pivot,
/// the front link follows the joint while its front end slides back along the bore.
pub fn toggle_transforms(open: f32) -> (Transform, Transform) {
    let th = (TOGGLE_OPEN_DEG * open.clamp(0.0, 1.0)).to_radians();
    let l1 = (TOGGLE_JOINT - TOGGLE_REAR_PIVOT).length();
    let j = TOGGLE_REAR_PIVOT + Vec2::new(-th.cos(), th.sin()) * l1;
    let l2 = (TOGGLE_JOINT - TOGGLE_FRONT).length();
    let dz = j.y - TOGGLE_FRONT.y;
    let f = Vec2::new(j.x - (l2 * l2 - dz * dz).max(0.0).sqrt(), TOGGLE_FRONT.y);
    let ang = |v: Vec2| v.y.atan2(v.x);
    let rear_a = ang(j - TOGGLE_REAR_PIVOT) - ang(TOGGLE_JOINT - TOGGLE_REAR_PIVOT);
    let front_a = ang(j - f) - ang(TOGGLE_JOINT - TOGGLE_FRONT);
    let about = |a: f32, from: Vec2, to: Vec2| {
        let r = Quat::from_rotation_x(a);
        let from3 = Vec3::new(0.0, from.x, from.y);
        let to3 = Vec3::new(0.0, to.x, to.y);
        Transform { translation: to3 - r * from3, rotation: r, scale: Vec3::ONE }
    };
    (about(front_a, TOGGLE_FRONT, f), about(rear_a, TOGGLE_REAR_PIVOT, TOGGLE_REAR_PIVOT))
}

fn split_mesh(m: &Mesh, keep: impl Fn(Vec3) -> bool) -> Option<Mesh> {
    use bevy::mesh::{Indices, VertexAttributeValues};
    let Some(VertexAttributeValues::Float32x3(pos)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let idx: Vec<u32> = match m.indices()? {
        Indices::U16(v) => v.iter().map(|&i| i as u32).collect(),
        Indices::U32(v) => v.clone(),
    };
    let out: Vec<u32> = idx
        .chunks_exact(3)
        .filter(|t| keep((Vec3::from(pos[t[0] as usize]) + Vec3::from(pos[t[1] as usize]) + Vec3::from(pos[t[2] as usize])) / 3.0))
        .flatten()
        .copied()
        .collect();
    let mut n = m.clone();
    n.insert_indices(Indices::U32(out));
    Some(n)
}

/// Once the Luger's scene is up: cut the toggle out of primitive 0 into two child meshes.
fn split_luger_toggle(
    mut commands: Commands,
    arsenal: Res<Arsenal>,
    new_meshes: Query<(Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>, &ChildOf), Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    mounted: Query<(), (With<MountedWeapon>, Without<GunPart>)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if arsenal.index != 0 {
        return;
    }
    for (e, m3, mat, _) in &new_meshes {
        let is_prim0 = m3.0.path().and_then(|p| p.label().map(|l| l.ends_with("Primitive0"))).unwrap_or(false);
        if !is_prim0 || !parents.iter_ancestors(e).any(|a| mounted.contains(a)) {
            continue;
        }
        let Some(src) = meshes.get(&m3.0).cloned() else { continue };
        let (Some(base), Some(front), Some(rear)) = (
            split_mesh(&src, |c| toggle_piece(c).is_none()),
            split_mesh(&src, |c| toggle_piece(c) == Some(LugerToggle::Front)),
            split_mesh(&src, |c| toggle_piece(c) == Some(LugerToggle::Rear)),
        ) else {
            continue;
        };
        commands.entity(e).insert(Mesh3d(meshes.add(base)));
        for (piece, mesh) in [(LugerToggle::Front, front), (LugerToggle::Rear, rear)] {
            let c = commands
                .spawn((
                    Name::new(format!("Luger toggle {piece:?}")),
                    piece,
                    Mesh3d(meshes.add(mesh)),
                    mat.clone(),
                    Transform::default(),
                    RenderLayers::layer(VIEW_LAYER),
                    bevy::light::NotShadowCaster,
                    bevy::light::NotShadowReceiver,
                    bevy::camera::visibility::NoFrustumCulling,
                ))
                .id();
            commands.entity(e).add_child(c);
        }
    }
}

/// Hold-open: the toggle stands open while the Luger's magazine is empty, and closes when a new
/// magazine is seated (reload commit) [G: rule of the real pistol, no recovered code].
fn luger_toggle(time: Res<Time>, arsenal: Res<Arsenal>, mut open: Local<f32>, mut q: Query<(&LugerToggle, &mut Transform)>) {
    let empty = arsenal.mag[0] == 0 && !matches!(arsenal.action, ArmsAction::Reload { committed: true, .. });
    let target = if empty { 1.0 } else { 0.0 };
    // snaps open with the last shot, closes a little slower as the toggle runs forward
    let rate = if target > *open { 30.0 } else { 12.0 };
    *open += (target - *open) * (rate * time.delta_secs()).min(1.0);
    let (front, rear) = toggle_transforms(*open);
    for (piece, mut tf) in &mut q {
        *tf = match piece {
            LugerToggle::Front => front,
            LugerToggle::Rear => rear,
        };
    }
}

fn on_weapon_ready(
    trigger: On<bevy::world_serialization::WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    mat_q: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    for e in children.iter_descendants(trigger.entity) {
        if meshes.contains(e) {
            // oiled blued steel: glossier than the exported 0.85 roughness (the PC shader has a
            // spec map per weapon) [G]
            if let Some(mut m) = mat_q.get(e).ok().and_then(|h| mats.get_mut(&h.0)) {
                m.perceptual_roughness = 0.6;
                m.reflectance = 0.5;
            }
            commands.entity(e).insert((
                RenderLayers::layer(VIEW_LAYER),
                bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver,
                // skinned AABBs stay at the bind pose; the animated arms sit ~0.6 m higher
                bevy::camera::visibility::NoFrustumCulling,
            ));
        }
    }
}

fn drive_arms(
    arsenal: Res<Arsenal>,
    rigs: Res<Rigs>,
    players: Query<&Player>,
    mut arms: Query<&mut RigPlayer, With<ArmsScene>>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Ok(p) = players.single() else { return };
    let Ok(mut rp) = arms.single_mut() else { return };
    let Ok((mut player, mut tr)) = anim.get_mut(rp.player) else { return };
    let w = arsenal.current();
    let rig = &rigs.arms;
    if let Some(i) = arsenal.debug_clip {
        let name = rig.names[i].clone();
        anim::play(rig, &mut rp, &mut player, &mut tr, &name, 0.1, true, false);
        return;
    }
    if arsenal.spear_held {
        match arsenal.spear_pose {
            SpearPose::Hold => anim::play(rig, &mut rp, &mut player, &mut tr, CLIP_SPEAR_HOLD, 0.2, true, false),
            SpearPose::WindUp => anim::play(rig, &mut rp, &mut player, &mut tr, CLIP_SPEAR_WINDUP, 0.08, false, false),
            SpearPose::Throw { t } => anim::play(rig, &mut rp, &mut player, &mut tr, CLIP_SPEAR_THROW, 0.05, false, t == 0.0),
            SpearPose::Stab { t } => anim::play(rig, &mut rp, &mut player, &mut tr, CLIP_SPEAR_STAB, 0.05, false, t == 0.0),
        };
        return;
    }
    let stance = if p.aiming { w.clip_aim } else { w.clip_idle };
    match arsenal.action {
        // Most fire clips are aimed poses, so they play while aiming; hip fire keeps the hip pose and
        // uses procedural recoil (the shotgun's fire clip is a hip pose and always plays).
        ArmsAction::Fire { t } if (p.aiming || w.fire_hip) && !w.clip_fire.is_empty() => {
            anim::play(rig, &mut rp, &mut player, &mut tr, w.clip_fire, 0.04, false, t == 0.0);
        }
        ArmsAction::Reload { t, .. } => {
            anim::play(rig, &mut rp, &mut player, &mut tr, w.clip_reload, 0.12, false, t == 0.0);
        }
        _ => {
            anim::play(rig, &mut rp, &mut player, &mut tr, stance, 0.2, true, false);
        }
    }
}
