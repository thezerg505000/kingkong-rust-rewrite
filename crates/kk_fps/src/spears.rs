//! Jack's spears and bones in the 03E slice (the game's `Javelin_*` objects), on top of
//! `kk_mechanics::spears` (launch speed 2.5 x 20 m toward camera + 20 m, gravity -10, damage by the
//! squared distance flown 11/11/5 (spear) and 7/7/3 (bone), wear +2 per embed out of 6/4, embed 0.6 m
//! and follow the host, 3 m pickup, slots, 32-spear cap, 15 m / 3 s clean-up) [C there].
//!
//! Placement [C positions from the level's GAOs]: the two spear racks `PFB_C_RackLanceSkel` (39.3, 5.0,
//! -84.6) and `PFB_C_RackLanceSkel01` (20.5, 4.3, -86.1) each hold five `S_LanceBig01..05` spears;
//! the bone pile `DEC_C_OssementSquelette_01` (56.3, 4.7, -84.4) hands out bone spears (renewable [L]).
//! The level glb already draws `RackLanceSkel01` and the bone pile; the other rack uses the rack prop.
//!
//! Controls (PS2 layout in spec/GAMEPLAY_SPEC.md): E / pad South picks up, G / pad North (Triangle)
//! drops, fire while aiming throws, fire without aiming stabs (reach 5 m, damage 1, every 0.4 s) [C].
//! Held spears sit in Jack's right hand on the arms' WeaponSocket (`OBJ_LanceSmall` spear /
//! `OBJ_LanceMed` bone javelin, both modelled in hand space) and the arms play the spear actions of the
//! `_PJ_J` arms kit (hold, wind-up + release, stab: see `weapons::SpearPose`). Right mouse / aim winds
//! up the throw without zooming; fire while wound up throws, fire otherwise stabs.

use crate::anim::{GameState, Rigs};
use crate::creatures::{Creature, CreatureRig, RaptorAi};
use crate::player::{MainCam, Player, ViewModel};
use crate::weapons::{Arsenal, RexDamage, RexHitbox, SpearPose};
use crate::world::{Arena, VIEW_LAYER};
use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;
use kk_mechanics::creatures::raptor::HitIn;
use kk_mechanics::spears::{self as sp, HitClass, ImpactResult, Slots, Spear, SpearKind, SpearState};
use rand::Rng;

pub struct SpearPlugin;

impl Plugin for SpearPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpearKit>().init_resource::<JackSpear>().add_systems(
            Update,
            (setup, input, flight, follow_hosts, settle, viewmodel)
                .chain()
                .after(crate::player::PlayerSet)
                .before(crate::weapons::WeaponsSet)
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// Rack / bone-pile sites of 03E: (GAO name, glTF position, rack yaw).
const RACKS: [(&str, Vec3, f32); 2] = [("PFB_C_RackLanceSkel", Vec3::new(39.3, 5.0, -84.6), 0.0), ("PFB_C_RackLanceSkel01", Vec3::new(20.5, 4.3, -86.1), 0.0)];
const BONE_PILES: [(&str, Vec3); 1] = [("DEC_C_OssementSquelette_01", Vec3::new(56.3, 4.7, -84.4))];
const SPEARS_PER_RACK: usize = 5;
/// Javelin hit message flags (`Javelin_launch`: `Msg_BuildHit(.., 0x400400, ..)`) [L: Ghidra shows the
/// immediate as `&DAT_00400400`].
const SPEAR_HIT_FLAGS: u32 = 0x0040_0400;
/// Hand-held models (kk_extract recipes, hand-socket space) [C geometry].
const SPEAR_GLB: &str = "jack_fps_spear.glb";
const BONE_GLB: &str = "jack_fps_bone_spear.glb";

#[derive(Default, Clone)]
struct Model {
    parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>, Transform)>,
}

#[derive(Resource, Default)]
pub struct SpearKit {
    ready: bool,
    /// the bone javelin glb, loading
    bone_gltf: Option<Handle<Gltf>>,
    spear: Model,
    bone: Model,
    spawned: bool,
    /// (time, what) log for batches
    pub log: Vec<(f32, String)>,
    t: f32,
    next_id: u32,
}

#[derive(Resource, Default)]
pub struct JackSpear {
    pub slots: Slots,
    pub held: Option<SpearKind>,
    melee_cd: f32,
    stab_t: f32,
    view: Option<Entity>,
    view_kind: Option<SpearKind>,
    /// the old free-floating viewmodel (no hand model extracted)
    view_floating: bool,
}

/// A spear / bone in the world.
#[derive(Component)]
pub struct SpearObj {
    pub s: Spear,
    pub id: u32,
    /// stands in a rack (never cleaned up)
    pub on_rack: bool,
    /// embedded in a creature bone: (bone entity, local transform)
    host: Option<(Entity, Mat4, Entity)>,
    /// seconds resting far from Jack
    far_t: f32,
    rest: bool,
}

#[derive(Component)]
pub struct BonePile;

fn to_m(v: Vec3) -> sp::V3 {
    [v.x, v.z, v.y]
}
fn from_m(a: sp::V3) -> Vec3 {
    Vec3::new(a[0], a[2], a[1])
}

/// Spear axis is the entity's local +Y.
fn along(dir: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::Y, dir.normalize_or(Vec3::Y))
}

/// The StandardMaterial of a glTF primitive (glTF materials load as GltfMaterial; the StandardMaterial
/// lives under the "<label>/std" sub-asset).
fn std_material(p: &bevy::gltf::GltfPrimitive, mats: &mut Assets<StandardMaterial>, server: &AssetServer) -> Handle<StandardMaterial> {
    p.material
        .as_ref()
        .and_then(|h| h.path())
        .and_then(|path| {
            let label = path.label()?.to_string();
            Some(server.load(path.clone().with_label(format!("{label}/std"))))
        })
        .unwrap_or_else(|| mats.add(StandardMaterial::from(Color::srgb(0.5, 0.4, 0.3))))
}

/// A long object's parts re-laid with its main axis (principal component of the vertices) along +Y,
/// centred on the middle of its extent. Positions are Jade (Z-up) mesh space.
fn axis_model(gm: &bevy::gltf::GltfMesh, meshes: &Assets<Mesh>, mats: &mut Assets<StandardMaterial>, server: &AssetServer) -> Model {
    let mut pts: Vec<Vec3> = Vec::new();
    for p in &gm.primitives {
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) = meshes.get(&p.mesh).and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION)) {
            pts.extend(v.iter().map(|q| Vec3::from(*q)));
        }
    }
    if pts.len() < 3 {
        return Model::default();
    }
    let mean = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
    // power iteration on the covariance
    let mut axis = Vec3::Z;
    for _ in 0..24 {
        let mut n = Vec3::ZERO;
        for q in &pts {
            let d = *q - mean;
            n += d * d.dot(axis);
        }
        axis = n.normalize_or(Vec3::Z);
    }
    let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| {
        let s = (*q - mean).dot(axis);
        (lo.min(s), hi.max(s))
    });
    let centre = mean + axis * (lo + hi) * 0.5;
    let r = Quat::from_rotation_arc(axis, Vec3::Y);
    let t = Transform { translation: r * -centre, rotation: r, scale: Vec3::ONE };
    Model { parts: gm.primitives.iter().map(|p| (p.mesh.clone(), std_material(p, mats, server), t)).collect() }
}

#[allow(clippy::too_many_arguments)]
fn build_models(rigs: &Rigs, bone_gltf: Option<&Handle<Gltf>>, gltfs: &Assets<Gltf>, gmeshes: &Assets<bevy::gltf::GltfMesh>, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, server: &AssetServer) -> (Model, Model) {
    let mut spear = Model::default();
    // the level's own spear mesh (S_LanceBig01): positions are in the rack skeleton's Jade frame,
    // so the parts are recentred and turned Z-up -> Y-up
    if let Some(g) = rigs.level.as_ref().and_then(|h| gltfs.get(h)) {
        if let Some(gm) = g.named_meshes.get("S_LanceBig01").and_then(|h| gmeshes.get(h)) {
            let mut lo = Vec3::splat(f32::MAX);
            let mut hi = Vec3::splat(f32::MIN);
            for p in &gm.primitives {
                if let Some(m) = meshes.get(&p.mesh) {
                    if let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(v)) = m.attribute(Mesh::ATTRIBUTE_POSITION) {
                        for q in v {
                            lo = lo.min(Vec3::from(*q));
                            hi = hi.max(Vec3::from(*q));
                        }
                    }
                }
            }
            if lo.x < f32::MAX {
                let c = (lo + hi) * 0.5;
                // Jade z (long axis) -> +Y
                let r = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
                let t = Transform { translation: r * -c, rotation: r, scale: Vec3::ONE };
                for p in &gm.primitives {
                    let mat = std_material(p, mats, server);
                    spear.parts.push((p.mesh.clone(), mat, t));
                }
            }
        }
    }
    if spear.parts.is_empty() {
        // stand-in: wooden shaft and a stone point [G]
        let shaft = meshes.add(Cylinder::new(0.025, 2.2));
        let tip = meshes.add(Cone { radius: 0.05, height: 0.25 });
        let wood = mats.add(StandardMaterial { base_color: Color::srgb(0.42, 0.32, 0.2), perceptual_roughness: 0.9, ..default() });
        let stone = mats.add(StandardMaterial { base_color: Color::srgb(0.35, 0.34, 0.32), perceptual_roughness: 0.8, ..default() });
        spear.parts.push((shaft, wood, Transform::default()));
        spear.parts.push((tip, stone, Transform::from_xyz(0.0, 1.22, 0.0)));
    }
    // bone javelin: OBJ_LanceMed (the bone texture 1f007960), laid along +Y like the level spear
    let mut bone = bone_gltf.and_then(|h| gltfs.get(h)).and_then(|g| g.meshes.first()).and_then(|h| gmeshes.get(h)).map(|gm| axis_model(gm, meshes, mats, server)).unwrap_or_default();
    if bone.parts.is_empty() {
        warn!("{BONE_GLB} missing (re-run the asset extraction): bone javelins use a plain stand-in");
        let bone_mesh = meshes.add(Capsule3d::new(0.028, 0.95));
        let ivory = mats.add(StandardMaterial { base_color: Color::srgb(0.56, 0.52, 0.44), perceptual_roughness: 0.85, ..default() });
        bone.parts.push((bone_mesh, ivory, Transform::default()));
    }
    (spear, bone)
}

fn spawn_model(commands: &mut Commands, parent: Entity, m: &Model, layer: Option<usize>) {
    for (mesh, mat, t) in &m.parts {
        let mut e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), *t));
        if let Some(l) = layer {
            e.insert((RenderLayers::layer(l), bevy::light::NotShadowCaster));
        }
        let id = e.id();
        commands.entity(parent).add_child(id);
    }
}

fn spawn_spear(commands: &mut Commands, kit: &mut SpearKit, s: Spear, pos: Vec3, rot: Quat, on_rack: bool) -> Entity {
    kit.next_id += 1;
    let id = kit.next_id;
    let model = if s.kind == SpearKind::Bone { kit.bone.clone() } else { kit.spear.clone() };
    let e = commands
        .spawn((Name::new(format!("Spear {id}")), SpearObj { s, id, on_rack, host: None, far_t: 0.0, rest: on_rack }, Transform::from_translation(pos).with_rotation(rot), Visibility::default()))
        .id();
    spawn_model(commands, e, &model, None);
    e
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    mut kit: ResMut<SpearKit>,
    rigs: Res<Rigs>,
    gltfs: Res<Assets<Gltf>>,
    gmeshes: Res<Assets<bevy::gltf::GltfMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    arena: Res<Arena>,
    assets: Res<AssetServer>,
    time: Res<Time>,
    mut respawn: MessageReader<crate::hud::RespawnAll>,
    spears: Query<Entity, With<SpearObj>>,
    mut jack: ResMut<JackSpear>,
) {
    kit.t += time.delta_secs();
    if arena.level.is_none() || crate::scene::swamp() || std::env::var("KK_NO_SPEARS").is_ok() {
        return;
    }
    if !kit.ready {
        if kit.bone_gltf.is_none() && crate::mods::resolve(BONE_GLB).exists() {
            kit.bone_gltf = Some(assets.load(BONE_GLB));
        }
        if let Some(h) = kit.bone_gltf.as_ref() {
            if !assets.is_loaded_with_dependencies(h.id()) && !matches!(assets.load_state(h.id()), bevy::asset::LoadState::Failed(_)) {
                return;
            }
        }
        let bone_h = kit.bone_gltf.clone();
        let (s, b) = build_models(&rigs, bone_h.as_ref(), &gltfs, &gmeshes, &mut meshes, &mut mats, &assets);
        kit.spear = s;
        kit.bone = b;
        kit.ready = true;
        // the rack the level glb does not draw: the rack prop, when exported
        let prop = crate::mods::resolve("level03e/props/PFB_C_RackLanceSkel01.glb");
        if prop.exists() {
            let (_, p, yaw) = RACKS[0];
            let y = arena.ground_at(p + Vec3::Y * 2.0).unwrap_or(p.y);
            commands.spawn((
                Name::new("RackLanceSkel (prop)"),
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset("level03e/props/PFB_C_RackLanceSkel01.glb"))),
                // the prop glb is centred on its bbox: lift by half its 2.03 m height
                Transform::from_xyz(p.x, y + 1.015, p.z).with_rotation(Quat::from_rotation_y(yaw)),
            ));
        }
        for (name, p) in BONE_PILES {
            commands.spawn((Name::new(format!("BonePile {name}")), BonePile, Transform::from_translation(p)));
        }
    }
    if respawn.read().count() > 0 {
        for e in &spears {
            commands.entity(e).despawn();
        }
        kit.spawned = false;
        jack.held = None;
        jack.slots = Slots::default();
    }
    if kit.spawned {
        return;
    }
    kit.spawned = true;
    let mut rng = rand::thread_rng();
    for (_, p, yaw) in RACKS {
        let y = arena.ground_at(p + Vec3::Y * 2.0).unwrap_or(p.y);
        let side = Quat::from_rotation_y(yaw) * Vec3::X;
        for i in 0..SPEARS_PER_RACK {
            let off = (i as f32 - 2.0) * 0.2;
            // leaning in the rack, points up [G layout: the rack skeleton's bone poses are not decoded]
            let lean = Quat::from_axis_angle(side, 0.12) * Quat::from_rotation_z(off * 0.25 + rng.gen_range(-0.03..0.03));
            let pos = Vec3::new(p.x, y + 1.25, p.z) + side * off;
            spawn_spear(&mut commands, &mut kit, Spear { state: SpearState::Dropped, ..Spear::held(SpearKind::Developed, 0, false) }, pos, lean, true);
        }
    }
    info!("spears: {} racks x {SPEARS_PER_RACK} spears, {} bone pile(s)", RACKS.len(), BONE_PILES.len());
}

fn held_gun_id(arsenal: &Arsenal) -> u8 {
    arsenal.index as u8 + 1
}

#[allow(clippy::too_many_arguments)]
fn input(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    mut kit: ResMut<SpearKit>,
    mut jack: ResMut<JackSpear>,
    mut arsenal: ResMut<Arsenal>,
    players: Query<(&Player, &Transform)>,
    cam: Query<&GlobalTransform, With<MainCam>>,
    mut objs: Query<(Entity, &mut SpearObj, &Transform)>,
    piles: Query<&Transform, With<BonePile>>,
    mut creatures: Query<(Entity, &Creature, &CreatureRig, &mut RaptorAi)>,
    gts: Query<&GlobalTransform>,
    mut sfx: MessageWriter<crate::sfx::PlaySfx>,
) {
    let dt = time.delta_secs();
    jack.melee_cd = (jack.melee_cd - dt).max(0.0);
    jack.stab_t = (jack.stab_t - dt).max(0.0);
    // arms spear actions run their clip once (durations of clips 56 / 5) [C]
    arsenal.spear_pose = match arsenal.spear_pose {
        SpearPose::Throw { t } if t + dt < THROW_CLIP_S => SpearPose::Throw { t: t + dt },
        SpearPose::Stab { t } if t + dt < STAB_CLIP_S => SpearPose::Stab { t: t + dt },
        SpearPose::Throw { .. } | SpearPose::Stab { .. } => SpearPose::Hold,
        p => p,
    };
    let Ok((p, jt)) = players.single() else { return };
    let Ok(cam) = cam.single() else { return };
    let t = kit.t;
    if !p.alive() {
        // a dying Jack drops what he holds
        if let Some(k) = jack.held.take() {
            let wear = jack.slots.wear;
            let s = Spear { state: SpearState::Dropped, wear, ..Spear::held(k, wear, false) };
            spawn_spear(&mut commands, &mut kit, s, jt.translation + Vec3::Y * 0.3, along(Vec3::X), false);
            jack.slots = Slots::default();
        }
        arsenal.spear_held = false;
        return;
    }
    if jack.held.is_none() {
        jack.slots.held = held_gun_id(&arsenal);
    }
    let mut pick = keys.just_pressed(KeyCode::KeyE);
    let mut drop = keys.just_pressed(KeyCode::KeyG);
    let mut fire = mouse.just_pressed(MouseButton::Left);
    for g in &gamepads {
        pick |= g.just_pressed(GamepadButton::South);
        drop |= g.just_pressed(GamepadButton::North);
        fire |= g.just_pressed(GamepadButton::RightTrigger2);
    }
    let jpos = jt.translation;
    let approaching = p.vel.length() > 1.0;
    let had_spear = jack.held.is_some();
    // ---- pickup (`Javelin_waittaken` radius 3 m, +1 approaching; H_TRACK_Reflex slot rules) [C]
    if pick {
        let held_id = jack.slots.held;
        let mut best: Option<(f32, Entity)> = None;
        for (e, o, tf) in objs.iter() {
            let d = Vec2::new(tf.translation.x - jpos.x, tf.translation.z - jpos.z).length();
            if (tf.translation.y - jpos.y).abs() < 2.5 && o.s.can_be_picked_up(d, approaching, false, held_id) && best.map_or(true, |b| d < b.0) {
                best = Some((d, e));
            }
        }
        let pile = piles.iter().any(|tf| Vec2::new(tf.translation.x - jpos.x, tf.translation.z - jpos.z).length() <= sp::PICKUP_RADIUS + 0.5);
        let taken: Option<(SpearKind, i32, bool)> = if let Some((_, e)) = best {
            let (_, o, _) = objs.get(e).unwrap();
            let r = (o.s.kind, o.s.wear, o.s.flaming);
            if let Some((host, _, _)) = o.host {
                if let Ok((_, _, _, mut ai)) = creatures.get_mut(host) {
                    ai.spears = ai.spears.saturating_sub(1);
                }
            }
            commands.entity(e).despawn();
            Some(r)
        } else if pile && held_id != SpearKind::Bone.weapon_id() as u8 && held_id != SpearKind::Developed.weapon_id() as u8 {
            Some((SpearKind::Bone, 0, false))
        } else {
            None
        };
        if let Some((kind, wear, fire_flag)) = taken {
            // a spear already in hand goes to the ground first (`Jack_DropHeldThrowable`)
            if let Some(k) = jack.held.take() {
                let w = jack.slots.wear;
                let s = Spear { state: SpearState::Dropped, ..Spear::held(k, w, false) };
                spawn_spear(&mut commands, &mut kit, s, jpos + Vec3::Y * 0.3, along(Vec3::X), false);
            }
            jack.slots.pickup(kind.weapon_id() as u8, wear, fire_flag, 1);
            jack.held = Some(kind);
            kit.log.push((t, format!("pickup {kind:?}")));
            sfx.write(crate::sfx::PlaySfx::ui("Jack weapon get"));
        }
    }
    // ---- drop (Triangle)
    if drop {
        if let Some(k) = jack.held {
            if let Some((_, wear, burning, _)) = jack.slots.drop_held() {
                let s = Spear { state: SpearState::Dropped, ..Spear::held(k, wear, burning) };
                let f = cam.forward().as_vec3().with_y(0.0).normalize_or(Vec3::Z);
                spawn_spear(&mut commands, &mut kit, s, jpos + f * 0.6 + Vec3::Y * 0.4, along(f.cross(Vec3::Y)), false);
                jack.held = None;
                kit.log.push((t, "drop".into()));
                sfx.write(crate::sfx::PlaySfx::ui("Jack Spear swap"));
            }
        }
    }
    // ---- throw (aiming) / stab
    if let (Some(k), true) = (jack.held, fire) {
        let fwd = cam.forward().as_vec3();
        let origin = cam.translation();
        if p.aiming {
            // H_ETAT_IA_lance: speed 2.5 x 20, aim at camera + forward x 20, gravity -10 [C]
            let mut s = Spear::held(k, jack.slots.wear, jack.slots.burning);
            let spawn = origin + cam.right().as_vec3() * 0.22 - cam.up().as_vec3() * 0.12 + fwd * 0.5;
            s.launch(to_m(spawn), to_m(origin), to_m(fwd));
            let v = from_m(s.vel);
            spawn_spear(&mut commands, &mut kit, s, spawn, along(v), false);
            // the release frame: action 0x5c lets go when its wind-up clip hands over to clip 56 [L]
            arsenal.spear_pose = SpearPose::Throw { t: 0.0 };
            jack.slots.consume_after_throw();
            jack.held = None;
            kit.log.push((t, format!("throw {k:?} {:.1} m/s", v.length())));
            sfx.write(crate::sfx::PlaySfx::ui("Jack Spear fired"));
        } else if jack.melee_cd <= 0.0 {
            // H_exec_test_ZDE_FIGHT: damage 1 every 0.4 s, reach 5, flag 0x10 (0x40 burning) [C]
            jack.melee_cd = sp::MELEE_COOLDOWN;
            jack.stab_t = 0.18;
            arsenal.spear_pose = SpearPose::Stab { t: 0.0 };
            sfx.write(crate::sfx::PlaySfx::ui("Jack weapon whoosh"));
            let mut best: Option<(f32, Entity, bool)> = None;
            for (ce, c, rig, ai) in creatures.iter() {
                if c.dead || ai.m.hp <= 0.0 {
                    continue;
                }
                for (bone, r, head) in &rig.hit {
                    let Ok(bg) = gts.get(*bone) else { continue };
                    if let Some(tt) = ray_sphere(origin, fwd, bg.translation(), r * c.scale + 0.15) {
                        if tt <= sp::MELEE_REACH && best.map_or(true, |b| tt < b.0) {
                            best = Some((tt, ce, *head));
                        }
                    }
                }
            }
            if let Some((tt, e, head)) = best {
                if let Ok((_, _, _, mut ai)) = creatures.get_mut(e) {
                    ai.hits.push(HitIn { damage: sp::MELEE_DAMAGE as f32, flags: sp::melee_flag(jack.slots.burning), head, dir_z: fwd.y, from_jack: true, bite_leg_latch: false });
                    ai.last_hit_dir = fwd.with_y(0.0).normalize_or_zero();
                    ai.hurt_me = 0.0;
                }
                kit.log.push((t, format!("stab hit at {tt:.1} m")));
                sfx.write(crate::sfx::PlaySfx::at("Jack spear hit", origin + fwd * tt));
            }
        }
    }
    // the press that threw / stabbed / dropped must not also fire the gun that comes back up
    if (fire || drop) && had_spear {
        arsenal.cooldown = arsenal.cooldown.max(0.35);
    }
    // the arms finish the throw before the gun comes back up
    let follow_through = matches!(arsenal.spear_pose, SpearPose::Throw { .. });
    arsenal.spear_held = jack.held.is_some() || follow_through;
    if jack.held.is_some() {
        // right mouse / aim winds the throw up (no zoom while a spear is held: player.rs)
        arsenal.spear_pose = match arsenal.spear_pose {
            SpearPose::Hold if p.aiming => SpearPose::WindUp,
            SpearPose::WindUp if !p.aiming => SpearPose::Hold,
            q => q,
        };
    } else if !follow_through {
        arsenal.spear_pose = SpearPose::Hold;
    }
}

const THROW_CLIP_S: f32 = 0.53;
const STAB_CLIP_S: f32 = 0.73;

fn ray_sphere(o: Vec3, d: Vec3, c: Vec3, r: f32) -> Option<f32> {
    let oc = o - c;
    let b = oc.dot(d);
    let cc = oc.length_squared() - r * r;
    let h = b * b - cc;
    if h < 0.0 {
        return None;
    }
    let t = -b - h.sqrt();
    (t >= 0.0).then_some(t)
}

/// Flight (`Javelin_launch`): ray sweep against creatures, the V-Rex and the level; stick, glance or drop.
#[allow(clippy::too_many_arguments)]
fn flight(
    time: Res<Time>,
    arena: Res<Arena>,
    mut kit: ResMut<SpearKit>,
    mut objs: Query<(Entity, &mut SpearObj, &mut Transform)>,
    mut creatures: Query<(Entity, &Creature, &CreatureRig, &mut RaptorAi, &GlobalTransform)>,
    gts: Query<&GlobalTransform>,
    hitbox: Res<RexHitbox>,
    mut rex_dmg: MessageWriter<RexDamage>,
    mut gun: MessageWriter<crate::events::GunEvent>,
    mut sfx: MessageWriter<crate::sfx::PlaySfx>,
) {
    let dt = time.delta_secs().min(0.05);
    let t = kit.t;
    for (_e, mut o, mut tf) in objs.iter_mut() {
        if o.s.state != SpearState::Thrown {
            continue;
        }
        let old = from_m(o.s.pos);
        o.s.integrate(dt);
        if o.s.state == SpearState::Broken {
            continue;
        }
        let new = from_m(o.s.pos);
        let seg = new - old;
        let len = seg.length();
        if len < 1e-5 {
            continue;
        }
        let dir = seg / len;
        // tip leads by half the spear
        let reach = len + 1.0;
        let mut best_c: Option<(f32, Entity, Entity, bool)> = None;
        for (ce, c, rig, ai, _) in creatures.iter() {
            if c.dead || ai.m.hp <= 0.0 {
                continue;
            }
            for (bone, r, head) in &rig.hit {
                let Ok(bg) = gts.get(*bone) else { continue };
                if let Some(tt) = ray_sphere(old, dir, bg.translation(), r * c.scale) {
                    if tt <= reach && best_c.map_or(true, |b| tt < b.0) {
                        best_c = Some((tt, ce, *bone, *head));
                    }
                }
            }
        }
        let mut best_rex: Option<f32> = None;
        if hitbox.alive {
            for (c, r, _) in &hitbox.spheres {
                if let Some(tt) = ray_sphere(old, dir, *c, *r) {
                    if tt <= reach && best_rex.map_or(true, |b| tt < b) {
                        best_rex = Some(tt);
                    }
                }
            }
        }
        let world = arena.raycast(old, dir, reach);
        let wt = world.map_or(f32::MAX, |w| w.0);
        let ct = best_c.map_or(f32::MAX, |b| b.0);
        let rt = best_rex.unwrap_or(f32::MAX);
        if ct < wt && ct <= rt {
            let (tt, ce, bone, head) = best_c.unwrap();
            let hit = old + dir * tt;
            o.s.pos = to_m(hit);
            let res = o.s.impact(HitClass::Creature, Some(ce.index_u32()), false);
            let dmg = match res {
                ImpactResult::Embedded { damage } | ImpactResult::Glanced { damage } => damage,
                _ => 0,
            };
            if let Ok((_, _, _, mut ai, _)) = creatures.get_mut(ce) {
                ai.hits.push(HitIn { damage: dmg as f32, flags: SPEAR_HIT_FLAGS, head, dir_z: dir.y, from_jack: true, bite_leg_latch: false });
                ai.last_hit_dir = dir.with_y(0.0).normalize_or_zero();
                ai.hurt_me = 0.0;
                if matches!(res, ImpactResult::Embedded { .. }) {
                    ai.spears += 1;
                }
            }
            if matches!(res, ImpactResult::Embedded { .. }) {
                // `hit_point - axis * 0.6`, then `Javelin_Plug` follows the bone [C]
                let at = hit - dir * sp::EMBED_DEPTH;
                if let Ok(bg) = gts.get(bone) {
                    let local = bg.to_matrix().inverse() * Mat4::from_rotation_translation(along(dir), at);
                    o.host = Some((ce, local, bone));
                }
                tf.translation = at;
                tf.rotation = along(dir);
            }
            kit.log.push((t, format!("spear hit creature dmg {dmg}")));
            gun.write(crate::events::GunEvent::Impact { pos: hit, normal: -dir, rex: true, damage: dmg as f32, dist: len });
            sfx.write(crate::sfx::PlaySfx::at("Jack spear hit", hit));
            continue;
        }
        if rt < wt {
            let hit = old + dir * rt;
            o.s.pos = to_m(hit);
            let d = o.s.hit_damage();
            // the rex is not something a spear sticks in for long: glance off [G]
            o.s.impact(HitClass::Glance, None, false);
            rex_dmg.write(RexDamage { amount: d as f32, dist_sq: 0.0, point: hit, bone: "spear" });
            gun.write(crate::events::GunEvent::Impact { pos: hit, normal: -dir, rex: true, damage: d as f32, dist: len });
            kit.log.push((t, format!("spear hit rex dmg {d}")));
            continue;
        }
        if let Some((wt, n)) = world {
            if wt <= len + 0.6 {
                let hit = old + dir * wt;
                o.s.pos = to_m(hit);
                // every level surface counts as stickable ground/rock/wood (material classes of the
                // collision mesh are not decoded) [G]
                let res = o.s.impact(HitClass::Stick, None, false);
                match res {
                    ImpactResult::Embedded { .. } => {
                        tf.translation = hit - dir * sp::EMBED_DEPTH;
                        tf.rotation = along(dir);
                        o.rest = true;
                        sfx.write(crate::sfx::PlaySfx::at("Bullet ricochet wood", hit));
                    }
                    _ => {
                        // spent / glancing: bounce off the surface at 10 % speed
                        let v = from_m(o.s.vel);
                        let r = v - 2.0 * v.dot(n) * n;
                        o.s.vel = to_m(r);
                        tf.translation = hit + n * 0.1;
                    }
                }
                gun.write(crate::events::GunEvent::Impact { pos: hit, normal: n, rex: false, damage: 0.0, dist: len });
                kit.log.push((t, format!("spear stuck in the level ({res:?})")));
                continue;
            }
        }
        tf.translation = new;
        tf.rotation = along(from_m(o.s.vel));
    }
}

/// Spears in creatures ride on the hit bone; when the creature is gone they fall.
fn follow_hosts(mut objs: Query<(&mut SpearObj, &mut Transform)>, gts: Query<&GlobalTransform>, creatures: Query<&Creature>) {
    for (mut o, mut tf) in objs.iter_mut() {
        let Some((host, local, bone)) = o.host else { continue };
        let alive = creatures.get(host).is_ok();
        match (alive, gts.get(bone)) {
            (true, Ok(bg)) => {
                let m = bg.to_matrix() * local;
                let (_, r, t) = m.to_scale_rotation_translation();
                tf.translation = t;
                tf.rotation = r;
            }
            _ => {
                o.host = None;
                o.s.state = SpearState::Dropped;
                o.s.vel = [0.0; 3];
                o.rest = false;
            }
        }
    }
}

/// Dropped / bounced spears fall and lie down; spent ones break; far resting thrown spears are
/// cleaned up after 3 s (15 m), the 33rd evicts the farthest (`Javelin_waittaken`,
/// `Javelin_exec_addinworld`) [C].
#[allow(clippy::too_many_arguments)]
fn settle(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<Arena>,
    mut objs: Query<(Entity, &mut SpearObj, &mut Transform)>,
    players: Query<&Transform, (With<Player>, Without<SpearObj>)>,
) {
    let dt = time.delta_secs().min(0.05);
    let jpos = players.single().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    let newest = objs.iter().filter(|o| !o.1.on_rack).map(|o| o.1.id).max().unwrap_or(0);
    let mut free: Vec<(Entity, f32, bool)> = Vec::new();
    for (e, mut o, mut tf) in objs.iter_mut() {
        if o.s.state == SpearState::Broken {
            commands.entity(e).despawn();
            continue;
        }
        if o.s.state == SpearState::Dropped && !o.rest && o.host.is_none() {
            let mut v = from_m(o.s.vel);
            v.y -= 20.0 * dt;
            tf.translation += v * dt;
            o.s.vel = to_m(v);
            let floor = arena.ground_at(tf.translation + Vec3::Y * 1.0).unwrap_or(tf.translation.y - 100.0);
            if tf.translation.y <= floor + 0.05 {
                tf.translation.y = floor + 0.05;
                // lie flat along the ground
                let flat = (tf.rotation * Vec3::Y).with_y(0.0).normalize_or(Vec3::X);
                tf.rotation = along(flat);
                o.rest = true;
                o.s.vel = [0.0; 3];
                if o.s.is_spent() {
                    o.s.state = SpearState::Broken;
                }
            }
        }
        if !o.on_rack && o.host.is_none() && o.rest {
            let d = tf.translation.distance(jpos);
            if d > sp::DESPAWN_DISTANCE && o.id != newest {
                o.far_t += dt;
                if o.far_t > sp::DESPAWN_DELAY {
                    commands.entity(e).despawn();
                    continue;
                }
            } else {
                o.far_t = 0.0;
            }
        }
        if !o.on_rack {
            free.push((e, tf.translation.distance_squared(jpos), o.s.state == SpearState::Thrown));
        }
    }
    let d: Vec<f32> = free.iter().map(|f| f.1).collect();
    let fl: Vec<bool> = free.iter().map(|f| f.2).collect();
    if free.len() > sp::WORLD_CAP {
        if let Some(i) = sp::eviction_index(&d, &fl) {
            commands.entity(free[i].0).despawn();
        }
    }
}

/// The held spear: the hand-space model on the arms' WeaponSocket (the arms play the spear clips,
/// `weapons::drive_arms`). Without the extracted model (old asset folder) the level spear / stand-in
/// floats in front of the camera with the arms hidden, as before.
#[allow(clippy::too_many_arguments)]
fn viewmodel(
    mut commands: Commands,
    mut jack: ResMut<JackSpear>,
    kit: Res<SpearKit>,
    assets: Res<AssetServer>,
    vm: Query<Entity, With<ViewModel>>,
    rig: Query<&crate::player::ArmsRig>,
    mut arms: Query<&mut Visibility, With<crate::player::ArmsScene>>,
    mut tfs: Query<&mut Transform>,
    players: Query<&Player>,
    time: Res<Time>,
) {
    let want = jack.held;
    if jack.view_kind != want {
        if let Some(e) = jack.view.take() {
            commands.entity(e).despawn();
        }
        jack.view_kind = want;
        jack.view_floating = false;
        let mut hide_arms = false;
        if let Some(k) = want {
            let glb = if k == SpearKind::Bone { BONE_GLB } else { SPEAR_GLB };
            match rig.single() {
                Ok(r) if crate::mods::resolve(glb).exists() => {
                    // same Z-up -> Y-up cancel as the guns: the socket lives in Jade space
                    let e = commands
                        .spawn((
                            Name::new("HeldSpear"),
                            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(glb))),
                            Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                            Visibility::default(),
                        ))
                        .observe(on_held_ready)
                        .id();
                    commands.entity(r.socket).add_child(e);
                    jack.view = Some(e);
                }
                _ => {
                    if let Ok(vm) = vm.single() {
                        let e = commands.spawn((Name::new("HeldSpear"), Transform::default(), Visibility::default(), RenderLayers::layer(VIEW_LAYER))).id();
                        let m = if k == SpearKind::Bone { kit.bone.clone() } else { kit.spear.clone() };
                        spawn_model(&mut commands, e, &m, Some(VIEW_LAYER));
                        commands.entity(vm).add_child(e);
                        jack.view = Some(e);
                        jack.view_floating = true;
                        hide_arms = true;
                    }
                }
            }
        }
        for mut v in arms.iter_mut() {
            *v = if hide_arms { Visibility::Hidden } else { Visibility::Inherited };
        }
    }
    if let (Some(e), true) = (jack.view, jack.view_floating) {
        let aiming = players.single().map(|p| p.aiming).unwrap_or(false);
        let stab = (jack.stab_t / 0.18).clamp(0.0, 1.0);
        let thrust = (stab * std::f32::consts::PI).sin() * 0.45;
        let (pos, pitch) = if aiming { (Vec3::new(0.22, -0.08, -0.35), 0.15) } else { (Vec3::new(0.3, -0.34, -0.5), 0.35) };
        if let Ok(mut tf) = tfs.get_mut(e) {
            let target = Transform::from_translation(pos + Vec3::new(0.0, 0.0, -thrust))
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2 + pitch) * Quat::from_rotation_z(-0.12));
            let k = (time.delta_secs() * 14.0).min(1.0);
            tf.translation = tf.translation.lerp(target.translation, k);
            tf.rotation = tf.rotation.slerp(target.rotation, k);
        }
    }
}

fn on_held_ready(trigger: On<bevy::world_serialization::WorldInstanceReady>, mut commands: Commands, children: Query<&Children>, meshes: Query<(), With<Mesh3d>>) {
    for e in children.iter_descendants(trigger.entity) {
        if meshes.contains(e) {
            commands.entity(e).insert((
                RenderLayers::layer(VIEW_LAYER),
                bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver,
                bevy::camera::visibility::NoFrustumCulling,
            ));
        }
    }
}
