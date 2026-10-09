//! 03E's breakable ODE structures: the courtyard gate, the corridor wall, the entrance lintel.
//!
//! In the game each structure is drawn intact by the façade mesh, and a set of `LD_03E_ODE_*` physics
//! pieces (hidden) waits to take over when it is smashed (ODE = the Open Dynamics Engine rigid bodies;
//! `LD_03E_Activate_ODE_Porte` / `_Arche` / `_pont` are the level's triggers). Here:
//!
//! * a Kong blow whose hit window opens with the structure in reach (`FightEvent::KongSwing`), a thrown
//!   or charging rex slamming into it, or `KK_BREAK=<key>` breaks it;
//! * the façade loses the triangles inside the structure's box, the pieces appear with an impulse along
//!   the blow and fall under gravity until they rest on the ground [G motion: the ODE parameters are
//!   not decoded], a dust burst and a camera shake go off;
//! * its collision boxes and wall faces stop blocking (`Arena::broken`), so Jack walks through;
//! * F8 (`RespawnAll`) puts everything back.

use crate::fx::{rand_unit, spawn_particle, CameraShake, FxAssets, Particle, ShakeParams};
use crate::kong::KongCtl;
use crate::world::{Arena, BREAKABLES};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, VertexAttributeValues};
use kk_mechanics::kong::fight::FightEvent;
use rand::Rng;

pub struct BreakablePlugin;

/// Smash a breakable by key (batches, debugging) exactly as a Kong blow would.
#[derive(Event, Clone, Copy, Debug)]
pub struct BreakRequest(pub &'static str);

impl Plugin for BreakablePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Breakables>().add_event::<BreakRequest>().add_systems(
            Update,
            (find_pieces, triggers, debris).chain().after(crate::kong::KongSet).run_if(in_state(crate::anim::GameState::Playing)),
        );
    }
}

#[derive(Clone)]
struct Piece {
    entity: Entity,
    group: usize,
    /// local transform at rest (intact) and the parent's global matrix
    home: Transform,
    parent: Mat4,
    /// world-space state while flying
    pos: Vec3,
    rot: Quat,
    vel: Vec3,
    spin: Vec3,
    half: f32,
    resting: bool,
}

#[derive(Clone)]
struct FacadeEdit {
    entity: Entity,
    original: Handle<Mesh>,
    group: usize,
}

#[derive(Resource, Default)]
pub struct Breakables {
    found: bool,
    tries: u32,
    pieces: Vec<Piece>,
    edits: Vec<FacadeEdit>,
    /// (time, key) of every break, for the batch report
    pub log: Vec<(f32, &'static str)>,
    pub broken: Vec<bool>,
    t: f32,
    /// Jack's walk probe through the gate (start in front, 12 m south): (furthest z reached, got through)
    /// with the gate intact and after it broke
    pub probe_intact: Option<(f32, bool)>,
    pub probe_broken: Option<(f32, bool)>,
}

/// Walk a Jack-sized probe from the gate's front through it with the arena's own `move_to`.
fn gate_probe(arena: &Arena) -> Option<(f32, bool)> {
    let (front, centre) = approach_point("porte")?;
    let start_y = arena.ground_at(front + Vec3::Y * 1.0)?;
    let mut p = Vec3::new(centre.x, start_y, front.z);
    let goal_z = centre.z - 6.0;
    for _ in 0..400 {
        let want = p + Vec3::new(0.0, 0.0, -0.08);
        p = arena.move_to(p, want, 0.35);
        if p.z <= goal_z {
            return Some((p.z, true));
        }
    }
    Some((p.z, false))
}

impl Breakables {
    pub fn is_broken(&self, key: &str) -> bool {
        BREAKABLES.iter().position(|b| b.key == key).is_some_and(|g| self.broken.get(g).copied().unwrap_or(false))
    }
}

/// Front of a structure on Jack's side (centre of the box face toward the courtyard / the start).
pub fn approach_point(key: &str) -> Option<(Vec3, Vec3)> {
    let b = BREAKABLES.iter().find(|b| b.key == key)?;
    let c = (b.lo + b.hi) * 0.5;
    // the gate and the corridor wall are crossed going south (-Z), the lintel going north
    let n = if key == "entree" { Vec3::new(0.0, 0.0, -1.0) } else { Vec3::new(0.0, 0.0, 1.0) };
    let half = (b.hi - b.lo) * 0.5;
    Some((Vec3::new(c.x, b.lo.y, c.z) + n * (half.z + 3.2), c))
}

fn find_pieces(
    mut br: ResMut<Breakables>,
    arena: Res<Arena>,
    names: Query<(Entity, &Name, &Transform, Option<&ChildOf>), Without<Mesh3d>>,
    gts: Query<&GlobalTransform>,
) {
    if br.found || arena.level.is_none() || crate::scene::swamp() {
        return;
    }
    br.tries += 1;
    if br.broken.is_empty() {
        br.broken = vec![false; BREAKABLES.len()];
    }
    let mut pieces = Vec::new();
    for (e, n, tf, parent) in &names {
        let Some(g) = crate::world::breakable_of_name(n.as_str()) else { continue };
        // only the glTF node (has a parent and children meshes), not our own entities
        let Some(par) = parent else { continue };
        let Ok(pg) = gts.get(par.parent()) else { continue };
        let Ok(me) = gts.get(e) else { continue };
        let pos = me.translation();
        // half size from the collision box of the same name
        let half = arena
            .level
            .as_ref()
            .and_then(|l| l.box_names.iter().position(|b| b == n.as_str()).map(|k| (l.boxes[k].1 - l.boxes[k].0) * 0.5))
            .map_or(0.6, |h| h.min_element().max(0.3));
        pieces.push(Piece { entity: e, group: g, home: *tf, parent: pg.compute_matrix(), pos, rot: me.rotation(), vel: Vec3::ZERO, spin: Vec3::ZERO, half, resting: true });
    }
    if !pieces.is_empty() && pieces.iter().all(|p| p.parent != Mat4::IDENTITY || p.pos != Vec3::ZERO) {
        info!(
            "breakables: {} ODE pieces ({})",
            pieces.len(),
            BREAKABLES.iter().enumerate().map(|(g, b)| format!("{} {}", b.key, pieces.iter().filter(|p| p.group == g).count())).collect::<Vec<_>>().join(", ")
        );
        br.pieces = pieces;
        br.found = true;
    } else if br.tries > 600 {
        br.found = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn triggers(
    time: Res<Time>,
    mut br: ResMut<Breakables>,
    mut arena: ResMut<Arena>,
    ctl: Option<Res<KongCtl>>,
    (mut respawn, mut requests): (EventReader<crate::hud::RespawnAll>, EventReader<BreakRequest>),
    mut commands: Commands,
    facades: Query<(Entity, &Mesh3d, &GlobalTransform, Option<&Name>)>,
    (children, parents, names, names_q): (Query<&Children>, Query<&ChildOf>, Query<&Name>, Query<(Entity, &Name)>),
    mut meshes: ResMut<Assets<Mesh>>,
    fx: Option<Res<FxAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut shake: ResMut<CameraShake>,
    mut sfx: EventWriter<crate::sfx::PlaySfx>,
    mut tfs: Query<(&mut Transform, &mut Visibility)>,
) {
    br.t += time.delta_secs();
    if br.broken.len() != BREAKABLES.len() {
        br.broken = vec![false; BREAKABLES.len()];
    }
    if br.probe_intact.is_none() && br.found && arena.level.is_some() && !crate::scene::swamp() && !br.broken[0] {
        br.probe_intact = gate_probe(&arena);
        info!("gate probe (intact): {:?}", br.probe_intact);
    }
    if br.probe_broken.is_none() && br.broken.first().copied().unwrap_or(false) {
        br.probe_broken = gate_probe(&arena);
        info!("gate probe (broken): {:?}", br.probe_broken);
    }
    // F8: everything back
    if respawn.read().count() > 0 {
        for p in br.pieces.iter_mut() {
            p.resting = true;
            p.vel = Vec3::ZERO;
            let m = p.parent * p.home.compute_matrix();
            let (_, r, t) = m.to_scale_rotation_translation();
            p.pos = t;
            p.rot = r;
            if let Ok((mut tf, mut vis)) = tfs.get_mut(p.entity) {
                *tf = p.home;
                *vis = Visibility::Inherited;
            }
        }
        for e in br.edits.drain(..) {
            commands.entity(e.entity).insert(Mesh3d(e.original));
        }
        for b in br.broken.iter_mut() {
            *b = false;
        }
        for b in arena.broken.iter_mut() {
            *b = false;
        }
    }
    // what breaks what
    let mut hits: Vec<(usize, Vec3, Vec3)> = Vec::new();
    if let Ok(k) = std::env::var("KK_BREAK") {
        if br.t > 1.5 {
            for (g, b) in BREAKABLES.iter().enumerate() {
                if k.split(',').any(|x| x == b.key) && !br.broken[g] && br.log.iter().all(|l| l.1 != b.key) {
                    hits.push((g, (b.lo + b.hi) * 0.5, Vec3::new(0.0, 0.0, -1.0)));
                }
            }
        }
    }
    for r in requests.read() {
        if let Some(g) = BREAKABLES.iter().position(|b| b.key == r.0) {
            let b = &BREAKABLES[g];
            hits.push((g, Vec3::new((b.lo.x + b.hi.x) * 0.5, b.lo.y + 1.0, b.hi.z), Vec3::new(0.0, 0.0, -1.0)));
        }
    }
    if let Some(c) = ctl.as_ref() {
        for e in &c.frame_events {
            match e {
                FightEvent::KongSwing { pos, facing, .. } => {
                    let k = c.world(*pos, c.kong_y);
                    let f2 = KongCtl::dir_xz(*facing);
                    let fwd = Vec3::new(f2.x, 0.0, f2.y);
                    // Kong's hand: 3.0 m sweep reach + 1.5 m hand [C/G as the fight's blow reach]
                    for (g, b) in BREAKABLES.iter().enumerate() {
                        if br.broken[g] {
                            continue;
                        }
                        let q = (k + fwd * 3.0).clamp(b.lo, b.hi);
                        let d = Vec2::new(q.x - k.x, q.z - k.z);
                        let ahead = d.normalize_or_zero().dot(Vec2::new(fwd.x, fwd.z));
                        if d.length() <= 3.0 + 1.5 + 1.0 && ahead > 0.3 && k.y + 6.0 > b.lo.y {
                            hits.push((g, q, fwd));
                        }
                    }
                }
                FightEvent::ThrowImpact { .. } => {
                    let r = c.rex_world();
                    for (g, b) in BREAKABLES.iter().enumerate() {
                        let q = r.clamp(b.lo, b.hi);
                        if !br.broken[g] && Vec2::new(q.x - r.x, q.z - r.z).length() < 6.0 {
                            hits.push((g, q, (q - r).with_y(0.0).normalize_or(Vec3::NEG_Z)));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for (g, at, dir) in hits {
        if br.broken[g] {
            continue;
        }
        br.broken[g] = true;
        if let Some(a) = arena.broken.get_mut(g) {
            *a = true;
        }
        let key = BREAKABLES[g].key;
        let t = br.t;
        br.log.push((t, key));
        info!("breakable '{key}' smashed at ({:.1}, {:.1}, {:.1})", at.x, at.y, at.z);
        let def = &BREAKABLES[g];
        // the façade stops drawing the intact stones (level meshes only: Kong, the rex or Jack standing in
        // the box must not be touched)
        let level_root = names_q.iter().find(|(_, n)| n.as_str() == "Level03E").map(|(e, _)| e);
        let level_meshes: std::collections::HashSet<Entity> = level_root.map(|r| children.iter_descendants(r).collect()).unwrap_or_default();
        for (e, m3, gt, n) in &facades {
            if !level_meshes.contains(&e) || !def.cut_facade {
                continue;
            }
            // pieces are drawn by their own (hidden) node meshes: skip their children
            let parent_name = parents.get(e).ok().and_then(|p| names.get(p.parent()).ok()).map(|n| n.as_str().to_string()).unwrap_or_default();
            if crate::world::breakable_of_name(&parent_name).is_some() || n.is_some_and(|n| crate::world::breakable_of_name(n.as_str()).is_some()) {
                continue;
            }
            if br.edits.iter().any(|x| x.entity == e) {
                continue;
            }
            let Some(mesh) = meshes.get(&m3.0) else { continue };
            if let Some(cut) = cut_box(mesh, gt.compute_matrix(), def.lo, def.hi) {
                let h = meshes.add(cut);
                br.edits.push(FacadeEdit { entity: e, original: m3.0.clone(), group: g });
                commands.entity(e).insert(Mesh3d(h));
            }
        }
        let mut rng = rand::thread_rng();
        for p in br.pieces.iter_mut().filter(|p| p.group == g) {
            let away = (p.pos - at).with_y(0.0).normalize_or(dir);
            p.vel = (dir * 0.7 + away * 0.6).normalize_or(dir) * rng.gen_range(4.0..9.0) + Vec3::Y * rng.gen_range(1.5..5.0);
            p.spin = rand_unit(&mut rng) * rng.gen_range(1.0..4.0);
            p.resting = false;
            if let Ok((_, mut vis)) = tfs.get_mut(p.entity) {
                *vis = Visibility::Visible;
            }
            // the level loader hid the node's primitives too
            for c in children.iter_descendants(p.entity) {
                commands.entity(c).insert(Visibility::Inherited);
            }
        }
        // dust and chips, camera shake, the crash
        if let Some(fx) = fx.as_ref() {
            let c = (def.lo + def.hi) * 0.5;
            let half = (def.hi - def.lo) * 0.5;
            for _ in 0..26 {
                let p = c + Vec3::new(rng.gen_range(-half.x..half.x), rng.gen_range(-half.y..half.y), rng.gen_range(-half.z..half.z));
                spawn_particle(&mut commands, &mut mats, fx, &fx.dust, false, p, Particle {
                    vel: (dir + rand_unit(&mut rng) * 0.8) * rng.gen_range(0.6..2.4) + Vec3::Y * 0.4,
                    gravity: 0.2,
                    drag: 1.2,
                    age: 0.0,
                    life: rng.gen_range(1.6..2.8),
                    size: (1.4, 4.2),
                    color: [Color::srgba(0.55, 0.53, 0.48, 0.55).to_linear(), Color::srgba(0.5, 0.48, 0.44, 0.3).to_linear(), LinearRgba::NONE],
                    spin: rng.gen_range(0.0..6.28),
                    view_layer: false,
                });
            }
            for _ in 0..18 {
                let p = c + Vec3::new(rng.gen_range(-half.x..half.x), rng.gen_range(-half.y..half.y), 0.0);
                spawn_particle(&mut commands, &mut mats, fx, &fx.chips, false, p, Particle {
                    vel: (dir + rand_unit(&mut rng) * 0.9) * rng.gen_range(3.0..8.0) + Vec3::Y * 2.0,
                    gravity: 9.8,
                    drag: 0.3,
                    age: 0.0,
                    life: rng.gen_range(0.8..1.4),
                    size: (0.35, 0.5),
                    color: [Color::srgba(0.62, 0.6, 0.55, 1.0).to_linear(), Color::srgba(0.6, 0.58, 0.53, 0.9).to_linear(), LinearRgba::NONE],
                    spin: rng.gen_range(0.0..6.28),
                    view_layer: false,
                });
            }
        }
        shake.send(ShakeParams { amp_v: 0.08, freq_v: 30.0, amp_h: 0.05, freq_h: 21.0, decay: 0.15, decay_mult: 1.02 });
        // no stone-crash definition in the decoded 03E sound table: the grenade blast stands in [G]
        sfx.write(crate::sfx::PlaySfx { def: "Jack Grenade explode", pos: Some(at), gain: 0.7 });
    }
}

/// The mesh without the triangles whose centroid lies inside the world box (None when none do).
fn cut_box(mesh: &Mesh, to_world: Mat4, lo: Vec3, hi: Vec3) -> Option<Mesh> {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let idx: Vec<u32> = match mesh.indices()? {
        Indices::U16(v) => v.iter().map(|&i| i as u32).collect(),
        Indices::U32(v) => v.clone(),
    };
    let w: Vec<Vec3> = pos.iter().map(|p| to_world.transform_point3(Vec3::from(*p))).collect();
    let mut keep = Vec::with_capacity(idx.len());
    let mut cut = 0;
    for t in idx.chunks_exact(3) {
        let c = (w[t[0] as usize] + w[t[1] as usize] + w[t[2] as usize]) / 3.0;
        if c.cmpge(lo).all() && c.cmple(hi).all() {
            cut += 1;
        } else {
            keep.extend_from_slice(t);
        }
    }
    if cut == 0 {
        return None;
    }
    let mut m = mesh.clone();
    m.insert_indices(Indices::U32(keep));
    Some(m)
}

fn debris(time: Res<Time>, arena: Res<Arena>, mut br: ResMut<Breakables>, mut tfs: Query<&mut Transform>) {
    let dt = time.delta_secs().min(0.05);
    for p in br.pieces.iter_mut() {
        if p.resting {
            continue;
        }
        p.vel.y -= 9.8 * dt;
        p.pos += p.vel * dt;
        let sl = p.spin.length();
        if sl > 1e-4 {
            p.rot = Quat::from_axis_angle(p.spin / sl, sl * dt) * p.rot;
        }
        let floor = arena.ground_at(p.pos + Vec3::Y * (p.half + 0.5)).unwrap_or(p.pos.y - 50.0);
        if p.pos.y - p.half < floor {
            p.pos.y = floor + p.half;
            if p.vel.y < 0.0 {
                p.vel.y = -p.vel.y * 0.25;
            }
            p.vel.x *= 0.6;
            p.vel.z *= 0.6;
            p.spin *= 0.6;
            if p.vel.length() < 0.6 {
                p.resting = true;
                p.vel = Vec3::ZERO;
            }
        }
        if p.pos.y < -60.0 {
            p.resting = true;
        }
        if let Ok(mut tf) = tfs.get_mut(p.entity) {
            let local = p.parent.inverse() * Mat4::from_rotation_translation(p.rot, p.pos);
            let (s, r, t) = local.to_scale_rotation_translation();
            let _ = s;
            tf.translation = t;
            tf.rotation = r;
        }
    }
}
