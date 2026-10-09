//! Test arena: a jungle clearing with rock pillars for cover.
//! The arena is a stand-in (no level geometry is loaded yet); collision and ray tests are
//! analytic so gameplay does not depend on a physics engine.

use crate::anim::GameState;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::RenderLayers;
use rand::{Rng, SeedableRng};

/// Render layer used by the arms/weapon viewmodel camera.
pub const VIEW_LAYER: usize = 1;
pub const ARENA_RADIUS: f32 = 75.0;

#[derive(Clone, Copy)]
pub struct Pillar {
    pub center: Vec2,
    pub radius: f32,
    pub height: f32,
}

/// Collision for the original level 03E (from research/pc level03e_collision.json).
pub struct LevelCollision {
    /// upward-facing ground triangles, glTF Y-up metres
    pub tris: Vec<[Vec3; 3]>,
    /// 4 m grid buckets over tris (XZ)
    grid: std::collections::HashMap<(i32, i32), Vec<u32>>,
    /// solid obstacle boxes (min, max), small props only
    pub boxes: Vec<(Vec3, Vec3)>,
    /// every solid object box (rocks, ruins, props, trunks) up to 40 m wide: arena clearance and camera rays
    pub obstacles: Vec<(Vec3, Vec3)>,
    /// names of `boxes` (the breakable ODE pieces are switched off by name when broken)
    pub box_names: Vec<String>,
    /// steep faces of the level's opaque meshes (meshcol.rs): Jack's walls
    pub walls: Option<crate::meshcol::WallMesh>,
}

const CELL: f32 = 4.0;

/// A breakable ODE structure of the level: its physics pieces (`LD_03E_ODE_*` GAOs, hidden while
/// intact) and the box the intact stones occupy in the façade mesh (which draws them while intact).
pub struct BreakDef {
    pub key: &'static str,
    /// the façade mesh also draws the intact stones (cut them out when broken)
    pub cut_facade: bool,
    pub prefixes: &'static [&'static str],
    pub lo: Vec3,
    pub hi: Vec3,
}

/// 03E breakables [C names / boxes from the level's GAOs and the exported collision boxes]:
/// the courtyard gate (5 blocks, `LD_03E_Activate_ODE_Porte` trigger in front of it), the corridor wall
/// before CHK03 (Base / Top / Mid / Hat rows) and the entrance lintel the rex smashes.
pub const BREAKABLES: [BreakDef; 3] = [
    BreakDef { key: "porte", cut_facade: false, prefixes: &["LD_03E_ODE_block"], lo: Vec3::new(44.7, 3.5, -102.3), hi: Vec3::new(51.4, 13.4, -98.8) },
    BreakDef { key: "couloir", cut_facade: false, prefixes: &["LD_03E_ODE_Base_", "LD_03E_ODE_Top_", "LD_03E_ODE_Mid_", "LD_03E_ODE_Hat_"], lo: Vec3::new(31.4, 4.3, -137.2), hi: Vec3::new(39.6, 9.8, -135.7) },
    BreakDef { key: "entree", cut_facade: false, prefixes: &["LD_03E_ODE_Entree"], lo: Vec3::new(30.0, 8.7, -66.9), hi: Vec3::new(40.2, 16.0, -64.8) },
];

pub fn breakable_of_name(name: &str) -> Option<usize> {
    BREAKABLES.iter().position(|b| b.prefixes.iter().any(|p| name.contains(p)))
}

pub fn breakable_of_point(p: Vec3) -> Option<usize> {
    BREAKABLES.iter().position(|b| p.cmpge(b.lo).all() && p.cmple(b.hi).all())
}

impl LevelCollision {
    fn load(path: &std::path::Path) -> Option<Self> {
        let txt = std::fs::read_to_string(path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&txt).ok()?;
        let p3 = |a: &serde_json::Value| {
            Vec3::new(a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32)
        };
        let tris: Vec<[Vec3; 3]> = v["ground_triangles"]
            .as_array()?
            .iter()
            .map(|t| [p3(&t[0]), p3(&t[1]), p3(&t[2])])
            .collect();
        let mut grid: std::collections::HashMap<(i32, i32), Vec<u32>> = Default::default();
        for (i, t) in tris.iter().enumerate() {
            let mn = t[0].min(t[1]).min(t[2]);
            let mx = t[0].max(t[1]).max(t[2]);
            for gx in (mn.x / CELL).floor() as i32..=(mx.x / CELL).floor() as i32 {
                for gz in (mn.z / CELL).floor() as i32..=(mx.z / CELL).floor() as i32 {
                    grid.entry((gx, gz)).or_default().push(i as u32);
                }
            }
        }
        let named: Vec<((Vec3, Vec3), String)> = v["boxes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|b| {
                        let (mn, mx) = (p3(&b["min"]), p3(&b["max"]));
                        let s = mx - mn;
                        // props and rocks small enough to be solid pieces; big shells (whole
                        // passages, cliffs) are left to the ground-coverage boundary
                        // the swamp scenes hide the ODE breakables (swamp.rs), so they must not collide either
                        let hidden_ode = crate::scene::swamp() && b["klass"] == "ode";
                        let name = b["name"].as_str().unwrap_or("").to_string();
                        let ode = b["klass"] == "ode" || name.contains("_ODE_");
                        (!hidden_ode && (b["role"] == "obj" || ode) && !name.contains("OCL_") && ((s.x < 6.0 && s.z < 6.0 && s.y > 0.8) || (ode && s.x < 8.0 && s.z < 8.0))).then_some(((mn, mx), name))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let boxes: Vec<(Vec3, Vec3)> = named.iter().map(|x| x.0).collect();
        let box_names: Vec<String> = named.into_iter().map(|x| x.1).collect();
        let obstacles = v["boxes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|b| {
                        let (mn, mx) = (p3(&b["min"]), p3(&b["max"]));
                        let s = mx - mn;
                        let name = b["name"].as_str().unwrap_or("");
                        let klass = b["klass"].as_str().unwrap_or("");
                        let solid = b["role"] == "obj" || (klass == "tree" && s.x < 8.0 && s.z < 8.0);
                        let hidden_ode = crate::scene::swamp() && klass == "ode";
                        (!hidden_ode && solid && !name.contains("OCL_") && !name.contains("Yucca") && s.x < 40.0 && s.z < 40.0 && s.y > 0.5).then_some((mn, mx))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self { tris, grid, boxes, obstacles, box_names, walls: None })
    }

    /// Merge more walkable triangles (the level mesh's upward faces) into the ground set and its grid.
    pub fn add_ground(&mut self, extra: Vec<[Vec3; 3]>) {
        let n0 = self.tris.len();
        let added = extra.len();
        self.tris.extend(extra);
        for i in n0..self.tris.len() {
            let t = self.tris[i];
            let mn = t[0].min(t[1]).min(t[2]);
            let mx = t[0].max(t[1]).max(t[2]);
            for gx in (mn.x / CELL).floor() as i32..=(mx.x / CELL).floor() as i32 {
                for gz in (mn.z / CELL).floor() as i32..=(mx.z / CELL).floor() as i32 {
                    self.grid.entry((gx, gz)).or_default().push(i as u32);
                }
            }
        }
        info!("ground: +{added} upward faces from the level mesh ({} total)", self.tris.len());
    }

    /// First solid along the ray from `o` along `d` (unit), up to `max` metres: below the ground, inside a solid
    /// box, or inside a rock / wall column (ground rising more than 1.6 m above the floor and 6 m above the point).
    /// Ground triangles are upward-facing only, so walls are detected as columns [G].
    pub fn ray_hit(&self, o: Vec3, d: Vec3, max: f32) -> Option<f32> {
        let step = 0.4;
        let n = (max / step).ceil() as usize;
        for k in 1..=n {
            let t = (k as f32 * step).min(max);
            let p = o + d * t;
            for (mn, mx) in &self.obstacles {
                if p.x > mn.x + 0.1 && p.x < mx.x - 0.1 && p.y > mn.y && p.y < mx.y && p.z > mn.z + 0.1 && p.z < mx.z - 0.1 {
                    return Some(t);
                }
            }
            if let Some(f) = self.ground(p.x, p.z, p.y, 0.0) {
                if let Some(top) = self.ground(p.x, p.z, p.y + 6.0, 0.0) {
                    if top > p.y + 0.3 && top - f > 1.6 {
                        return Some(t);
                    }
                }
            } else if self.ground(p.x, p.z, p.y + 6.0, 0.0).is_some_and(|top| top > p.y + 0.3) {
                // under the ground surface with nothing below it
                return Some(t);
            }
        }
        None
    }

    pub fn segment_clear(&self, a: Vec3, b: Vec3) -> bool {
        let d = b - a;
        let l = d.length();
        l < 1e-3 || self.ray_hit(a, d / l, l).is_none()
    }

    /// Ground height under (x,z): the highest triangle not more than `step` above `y_ref`.
    pub fn ground(&self, x: f32, z: f32, y_ref: f32, step: f32) -> Option<f32> {
        let key = ((x / CELL).floor() as i32, (z / CELL).floor() as i32);
        let mut best: Option<f32> = None;
        for &i in self.grid.get(&key)? {
            let [a, b, c] = self.tris[i as usize];
            let v0 = Vec2::new(c.x - a.x, c.z - a.z);
            let v1 = Vec2::new(b.x - a.x, b.z - a.z);
            let v2 = Vec2::new(x - a.x, z - a.z);
            let d00 = v0.dot(v0);
            let d01 = v0.dot(v1);
            let d11 = v1.dot(v1);
            let d20 = v2.dot(v0);
            let d21 = v2.dot(v1);
            let den = d00 * d11 - d01 * d01;
            if den.abs() < 1e-9 {
                continue;
            }
            let u = (d11 * d20 - d01 * d21) / den;
            let v = (d00 * d21 - d01 * d20) / den;
            if u < -0.01 || v < -0.01 || u + v > 1.02 {
                continue;
            }
            let y = a.y + u * (c.y - a.y) + v * (b.y - a.y);
            if y <= y_ref + step && best.map_or(true, |bb| y > bb) {
                best = Some(y);
            }
        }
        best
    }
}

#[derive(Resource)]
pub struct Arena {
    pub pillars: Vec<Pillar>,
    pub level: Option<LevelCollision>,
    pub player_spawn: Vec3,
    pub player_yaw: f32,
    pub rex_spawn: Vec3,
    pub rex_yaw: f32,
    /// per `BREAKABLES` entry: broken (its boxes and wall faces stop blocking)
    pub broken: Vec<bool>,
}

impl Arena {
    fn generate() -> Self {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1933);
        let mut pillars = Vec::new();
        while pillars.len() < 22 {
            let a = rng.gen_range(0.0..std::f32::consts::TAU);
            let r = rng.gen_range(12.0..ARENA_RADIUS - 6.0);
            let c = Vec2::new(a.cos() * r, a.sin() * r);
            let radius = rng.gen_range(0.8..2.6);
            if c.distance(Vec2::new(0.0, 30.0)) < 8.0 || c.distance(Vec2::new(0.0, -35.0)) < 12.0 {
                continue;
            }
            if pillars.iter().any(|p: &Pillar| p.center.distance(c) < p.radius + radius + 4.0) {
                continue;
            }
            pillars.push(Pillar { center: c, radius, height: rng.gen_range(3.0..11.0) });
        }
        Self {
            pillars,
            level: None,
            player_spawn: Vec3::new(0.0, 0.0, 30.0),
            player_yaw: 0.0,
            rex_spawn: Vec3::new(0.0, 0.0, -40.0),
            rex_yaw: 0.0,
            broken: vec![false; BREAKABLES.len()],
        }
    }

    /// Level 03E courtyard: Jack at the south end, the V-Rex coming down the north passage
    /// (positions from the level's own LD_Pos/CHK markers, see level03e/README.md).
    /// Flat test area (`testarea.rs`): no pillars, no boundary, Jack at the origin looking down -Z.
    fn testarea() -> Self {
        Self { pillars: Vec::new(), ..Self::generate_empty() }
    }

    fn generate_empty() -> Self {
        Self {
            pillars: Vec::new(),
            level: None,
            player_spawn: Vec3::ZERO,
            player_yaw: 0.0,
            rex_spawn: Vec3::new(0.0, 0.0, -4000.0),
            rex_yaw: 0.0,
            broken: vec![false; BREAKABLES.len()],
        }
    }

    fn level03e(level: LevelCollision) -> Self {
        let mut a = Self { pillars: Vec::new(), level: Some(level), ..Self::generate() };
        a.pillars.clear();
        a.player_spawn = Vec3::new(34.5, 5.3, -93.0);
        a.player_yaw = std::f32::consts::PI; // face +Z (north, up the passage)
        a.rex_spawn = Vec3::new(35.0, 6.5, -62.0);
        a.rex_yaw = std::f32::consts::PI; // Rex forward is +Z of its frame -> face -Z (south)
        if let Some(l) = &a.level {
            if let Some(y) = l.ground(a.player_spawn.x, a.player_spawn.z, 50.0, 0.0) { a.player_spawn.y = y; }
            if let Some(y) = l.ground(a.rex_spawn.x, a.rex_spawn.z, 50.0, 0.0) { a.rex_spawn.y = y; }
        }
        a
    }

    /// Level 07D swamp: spawn points are placeholders; `kong.rs` stands Jack at the fight arena's vantage.
    fn level07d(level: LevelCollision) -> Self {
        let mut a = Self { pillars: Vec::new(), level: Some(level), ..Self::generate() };
        a.pillars.clear();
        let wy = crate::swamp::water_y();
        if crate::scene::marsh05c() {
            // 05C marsh: rex instances [c101f503] (118.5,-38.4,-181.5), [c101f50a] (140.5,-37.2,-187.3) [C]
            a.player_spawn = Vec3::new(125.0, wy, -205.0);
            a.rex_spawn = Vec3::new(118.5, wy, -181.5);
        } else {
            a.player_spawn = Vec3::new(100.0, wy, -280.0);
            a.rex_spawn = Vec3::new(120.0, wy, -280.0);
        }
        a.player_yaw = 0.0;
        a.rex_yaw = 0.0;
        if let Some(l) = &a.level {
            if let Some(y) = l.ground(a.player_spawn.x, a.player_spawn.z, wy + 3.6, 0.0) { a.player_spawn.y = y; }
            if let Some(y) = l.ground(a.rex_spawn.x, a.rex_spawn.z, wy + 3.6, 0.0) { a.rex_spawn.y = y; }
        }
        a
    }

    pub fn ground_at(&self, p: Vec3) -> Option<f32> {
        match &self.level {
            Some(l) => l.ground(p.x, p.z, p.y, 1.2),
            None => Some(0.0),
        }
    }

    /// Move from `old` to `new` with radius `r`: pushes out of obstacles, snaps to the ground,
    /// and refuses to step where the level has no ground (the playable boundary).
    pub fn move_to(&self, old: Vec3, mut p: Vec3, r: f32) -> Vec3 {
        let Some(level) = &self.level else {
            let mut q = self.collide(p, r);
            q.y = 0.0;
            return q;
        };
        // Jack's body: feet + a 0.55 m step [G], 1.75 m tall
        const STEP: f32 = 0.55;
        const HEIGHT: f32 = 1.75;
        p.y = old.y;
        let mut stand: Option<f32> = None;
        for (k, (mn, mx)) in level.boxes.iter().enumerate() {
            let off = level.box_names.get(k).and_then(|n| breakable_of_name(n)).is_some_and(|g| self.broken.get(g).copied().unwrap_or(false));
            if off || p.y + HEIGHT < mn.y || p.y > mx.y {
                continue;
            }
            let cx = p.x.clamp(mn.x, mx.x);
            let cz = p.z.clamp(mn.z, mx.z);
            let d = Vec2::new(p.x - cx, p.z - cz);
            let l = d.length();
            // a box whose top is within a step is stood on (steps, plinths, low blocks)
            if mx.y <= old.y + STEP {
                if l < 1e-4 {
                    stand = Some(stand.map_or(mx.y, |s: f32| s.max(mx.y)));
                }
                continue;
            }
            if l < r {
                let n = if l > 1e-4 { d / l } else { Vec2::X };
                p.x = cx + n.x * r;
                p.z = cz + n.y * r;
            }
        }
        if let Some(w) = &level.walls {
            p = w.push_out(p, r, STEP, HEIGHT, &self.broken);
        }
        let ground = match (level.ground(p.x, p.z, old.y, STEP + 0.05), stand) {
            (Some(g), Some(s)) => Some(g.max(s)),
            (g, s) => g.or(s),
        };
        match ground {
            Some(y) => {
                p.y = y;
                p
            }
            None => {
                // slide along the boundary: try each axis on its own
                let px = Vec3::new(p.x, old.y, old.z);
                if let Some(y) = level.ground(px.x, px.z, old.y, STEP + 0.05) {
                    return Vec3::new(px.x, y, px.z);
                }
                let pz = Vec3::new(old.x, old.y, p.z);
                if let Some(y) = level.ground(pz.x, pz.z, old.y, STEP + 0.05) {
                    return Vec3::new(pz.x, y, pz.z);
                }
                old
            }
        }
    }

    /// Run `p` through Jack's collision until it stops moving (spawn / vantage points that start inside
    /// a wall face are pushed out once, instead of drifting while Jack stands still).
    pub fn settle(&self, mut p: Vec3, r: f32) -> Vec3 {
        for _ in 0..240 {
            let q = self.move_to(p, p, r);
            if q.distance(p) < 1e-3 {
                return q;
            }
            p = q;
        }
        p
    }

    /// Push a circle of radius `r` (on the ground plane) out of pillars and the arena wall.
    pub fn collide(&self, mut p: Vec3, r: f32) -> Vec3 {
        for pl in &self.pillars {
            let d = Vec2::new(p.x, p.z) - pl.center;
            let min = pl.radius + r;
            let len = d.length();
            if len < min && len > 1e-4 {
                let n = d / len * min + pl.center;
                p.x = n.x;
                p.z = n.y;
            }
        }
        if self.level.is_none() && !crate::testarea::active() {
            let h = Vec2::new(p.x, p.z);
            if h.length() > ARENA_RADIUS - r {
                let n = h.normalize() * (ARENA_RADIUS - r);
                p.x = n.x;
                p.z = n.y;
            }
        }
        p
    }

    /// Nearest hit (distance, surface normal) of a ray against the world.
    pub fn raycast(&self, o: Vec3, d: Vec3, max: f32) -> Option<(f32, Vec3)> {
        let mut best: Option<(f32, Vec3)> = None;
        let mut take = |t: f32, n: Vec3| {
            if t > 0.0 && t <= max && best.map_or(true, |b| t < b.0) {
                best = Some((t, n));
            }
        };
        if let Some(level) = &self.level {
            for tri in &level.tris {
                if let Some(t) = ray_tri(o, d, tri) {
                    let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
                    take(t, if n.dot(d) > 0.0 { -n } else { n });
                }
            }
            if let Some(w) = &level.walls {
                if let Some((t, n)) = w.ray(o, d, max, &self.broken) {
                    take(t, n);
                }
            }
            for (k, (mn, mx)) in level.boxes.iter().enumerate() {
                let off = level.box_names.get(k).and_then(|n| breakable_of_name(n)).is_some_and(|g| self.broken.get(g).copied().unwrap_or(false));
                if off {
                    continue;
                }
                if let Some((t, n)) = ray_aabb(o, d, *mn, *mx) {
                    take(t, n);
                }
            }
            return best;
        }
        if d.y < -1e-5 {
            take(-o.y / d.y, Vec3::Y);
        }
        for pl in &self.pillars {
            let oc = Vec2::new(o.x, o.z) - pl.center;
            let dd = Vec2::new(d.x, d.z);
            let a = dd.dot(dd);
            if a < 1e-8 {
                continue;
            }
            let b = 2.0 * oc.dot(dd);
            let c = oc.dot(oc) - pl.radius * pl.radius;
            let disc = b * b - 4.0 * a * c;
            if disc < 0.0 {
                continue;
            }
            let t = (-b - disc.sqrt()) / (2.0 * a);
            let hp = o + d * t;
            if hp.y >= 0.0 && hp.y <= pl.height {
                let n = Vec3::new(hp.x - pl.center.x, 0.0, hp.z - pl.center.y).normalize_or_zero();
                take(t, n);
            }
        }
        best
    }
}

pub(crate) fn ray_tri(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f32> {
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-7 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let tt = e2.dot(q) * inv;
    (tt > 1e-4).then_some(tt)
}

fn ray_aabb(o: Vec3, d: Vec3, mn: Vec3, mx: Vec3) -> Option<(f32, Vec3)> {
    let inv = Vec3::ONE / d;
    let t0 = (mn - o) * inv;
    let t1 = (mx - o) * inv;
    let tmin = t0.min(t1);
    let tmax = t0.max(t1);
    let tn = tmin.max_element();
    let tf = tmax.min_element();
    if tn > tf || tf < 0.0 || tn <= 0.0 {
        return None;
    }
    let n = if tn == tmin.x {
        Vec3::new(-d.x.signum(), 0.0, 0.0)
    } else if tn == tmin.y {
        Vec3::new(0.0, -d.y.signum(), 0.0)
    } else {
        Vec3::new(0.0, 0.0, -d.z.signum())
    };
    Some((tn, n))
}

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        let dir = crate::asset_dir();
        let arena = if crate::testarea::active() {
            info!("arena: test area (KK_SCENE=testarea)");
            Arena::testarea()
        } else { match LevelCollision::load(&dir.join(crate::scene::level_collision())) {
            Some(mut l) if dir.join(crate::scene::level_glb()).exists() && std::env::var("KK_STAND_IN").is_err() => {
                // the 03E slice only: the swamp fights keep their tuned arenas (KK_WALLS=1 forces it on)
                if std::env::var("KK_NO_WALLS").is_err() && (!crate::scene::swamp() || std::env::var("KK_WALLS").is_ok()) {
                    let t0 = std::time::Instant::now();
                    l.walls = crate::meshcol::WallMesh::load(&dir.join(crate::scene::level_glb()));
                    if let Some(w) = l.walls.as_mut() {
                        let extra = std::mem::take(&mut w.floor);
                        l.add_ground(extra);
                    }
                    info!("level walls: {} steep triangles from the level mesh ({:.2} s)", l.walls.as_ref().map_or(0, |w| w.tris.len()), t0.elapsed().as_secs_f32());
                }
                info!("arena: original level {} ({} ground tris, {} solid boxes, {} obstacles)", match crate::scene::swamp_level() { Some(crate::scene::Swamp::L05C) => "05C marsh", Some(_) => "07D swamp", None => "03E" }, l.tris.len(), l.boxes.len(), l.obstacles.len());
                if crate::scene::swamp() { Arena::level07d(l) } else { Arena::level03e(l) }
            }
            _ => {
                info!("arena: stand-in clearing (level03e not found or KK_STAND_IN set)");
                Arena::generate()
            }
        } };
        app.insert_resource(arena)
            .insert_resource(ClearColor(Color::srgb(0.52, 0.58, 0.55)))
            .insert_resource(AmbientLight {
                color: Color::srgb(0.75, 0.82, 0.78),
                brightness: 450.0,
                ..default()
            })
            .add_systems(OnEnter(GameState::Playing), spawn_arena)
            .add_systems(Startup, spawn_loading_camera)
            .add_systems(OnExit(GameState::Loading), despawn_loading_camera);
    }
}

#[derive(Component)]
struct LoadingCam;

fn spawn_loading_camera(mut commands: Commands) {
    commands.spawn((LoadingCam, Camera2d));
    commands.spawn((
        LoadingCam,
        Text::new("Loading King Kong assets…"),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(24.0),
            bottom: Val::Px(24.0),
            ..default()
        },
    ));
}

fn despawn_loading_camera(mut commands: Commands, q: Query<Entity, With<LoadingCam>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// Procedural ground texture (mud / moss noise). Replace with a recovered ff80 texture later.
fn ground_image() -> Image {
    const N: u32 = 256;
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let lattice: Vec<f32> = (0..(17 * 17)).map(|_| rng.gen()).collect();
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let fx = x as f32 / N as f32 * 16.0;
            let fy = y as f32 / N as f32 * 16.0;
            let (ix, iy) = (fx as usize, fy as usize);
            let (tx, ty) = (fx.fract(), fy.fract());
            let l = |i: usize, j: usize| lattice[(j % 16) * 17 + (i % 16)];
            let a = l(ix, iy) * (1.0 - tx) + l(ix + 1, iy) * tx;
            let b = l(ix, iy + 1) * (1.0 - tx) + l(ix + 1, iy + 1) * tx;
            let n = a * (1.0 - ty) + b * ty;
            let grain: f32 = rng.gen_range(-0.06..0.06);
            let v = (n * 0.7 + 0.3 + grain).clamp(0.0, 1.0);
            let moss = (n - 0.45).max(0.0) * 1.6;
            let r = 0.28 * v + 0.05 - moss * 0.08;
            let g = 0.25 * v + 0.07 + moss * 0.08;
            let bl = 0.17 * v + 0.04 - moss * 0.04;
            for c in [r, g, bl] {
                data.push((c.clamp(0.0, 1.0) * 255.0) as u8);
            }
            data.push(255);
        }
    }
    let mut img = Image::new(
        Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

/// Atmosphere of the 03E level: teal jungle fog (matches the original's look) [G values].
pub const FOG_COLOR: Color = Color::srgb(0.21, 0.31, 0.30);

fn spawn_arena(
    mut commands: Commands,
    arena: Res<Arena>,
    rigs: Res<crate::anim::Rigs>,
    gltfs: Res<Assets<Gltf>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    _clear: ResMut<ClearColor>,
    _ambient: ResMut<AmbientLight>,
) {
    if crate::testarea::active() {
        spawn_viewmodel_lights(&mut commands);
        return;
    }
    if arena.level.is_some() {
        if let Some(scene) = rigs.level.as_ref().and_then(|h| gltfs.get(h)).map(|g| g.scenes[0].clone()) {
            if crate::scene::swamp() {
                commands.spawn((Name::new("Level07D"), SceneRoot(scene))).observe(crate::swamp::on_level_ready);
            } else {
                commands.spawn((Name::new("Level03E"), SceneRoot(scene))).observe(on_level_ready);
            }
        }
        if crate::scene::swamp() {
            spawn_viewmodel_lights(&mut commands);
            return;
        }
        // dark mud floor under the level so holes in the partial export read as ground [G]
        let ground_tex = images.add(ground_image());
        commands.spawn((
            Name::new("UnderFloor"),
            Mesh3d(meshes.add(Plane3d::default().mesh().size(400.0, 400.0))),
            MeshMaterial3d(mats.add(StandardMaterial {
                base_color: Color::srgb(0.35, 0.38, 0.32),
                base_color_texture: Some(ground_tex),
                perceptual_roughness: 1.0,
                uv_transform: Affine2::from_scale(Vec2::splat(60.0)),
                ..default()
            })),
            Transform::from_xyz(35.0, 2.0, -80.0),
        ));
        // fog, moonlight, light shafts and ambient: atmos.rs (recovered level values)
        spawn_viewmodel_lights(&mut commands);
        return;
    }
    spawn_viewmodel_lights(&mut commands);
    let ground_tex = images.add(ground_image());
    commands.spawn((
        Name::new("Ground"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(ARENA_RADIUS * 2.6, ARENA_RADIUS * 2.6))),
        MeshMaterial3d(mats.add(StandardMaterial {
            base_color_texture: Some(ground_tex),
            perceptual_roughness: 0.95,
            uv_transform: Affine2::from_scale(Vec2::splat(24.0)),
            ..default()
        })),
    ));
    let rock = mats.add(StandardMaterial {
        base_color: Color::srgb(0.36, 0.35, 0.31),
        perceptual_roughness: 0.9,
        ..default()
    });
    for p in &arena.pillars {
        commands.spawn((
            Name::new("Pillar"),
            Mesh3d(meshes.add(Cylinder::new(p.radius, p.height).mesh().resolution(10))),
            MeshMaterial3d(rock.clone()),
            Transform::from_xyz(p.center.x, p.height * 0.5, p.center.y),
        ));
    }
    // ring of tall "trees" marking the arena edge
    let bark = mats.add(StandardMaterial {
        base_color: Color::srgb(0.22, 0.17, 0.12),
        perceptual_roughness: 1.0,
        ..default()
    });
    let leaves = mats.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.26, 0.11),
        perceptual_roughness: 1.0,
        ..default()
    });
    let trunk = meshes.add(Cylinder::new(1.1, 30.0).mesh().resolution(8));
    let crown = meshes.add(Sphere::new(7.0).mesh().ico(1).unwrap());
    for i in 0..56 {
        let a = i as f32 / 56.0 * std::f32::consts::TAU;
        let r = ARENA_RADIUS + 3.0 + (i % 3) as f32 * 2.5;
        let (x, z) = (a.cos() * r, a.sin() * r);
        commands.spawn((Mesh3d(trunk.clone()), MeshMaterial3d(bark.clone()), Transform::from_xyz(x, 15.0, z)));
        commands.spawn((Mesh3d(crown.clone()), MeshMaterial3d(leaves.clone()), Transform::from_xyz(x, 30.0, z)));
    }
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            illuminance: 9000.0,
            shadows_enabled: true,
            color: Color::srgb(1.0, 0.95, 0.85),
            ..default()
        },
        Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        // lights both the world and the viewmodel layer
        RenderLayers::from_layers(&[0, VIEW_LAYER]),
        bevy::pbr::CascadeShadowConfigBuilder {
            maximum_distance: 120.0,
            ..default()
        }
        .build(),
    ));
}

/// Level meshes: set matte surfaces. `KK_HIDE=a,b` hides instances whose name contains any
/// of the substrings (debugging aid).
fn on_level_ready(
    trigger: Trigger<bevy::scene::SceneInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    names: Query<&Name>,
    q: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let hide: Vec<String> = std::env::var("KK_HIDE")
        .map(|v| v.split(',').filter(|s| !s.is_empty()).map(String::from).collect())
        .unwrap_or_default();
    let mut done = std::collections::HashSet::new();
    for e in children.iter_descendants(trigger.target()) {
        if let Ok(n) = names.get(e) {
            // OCL_* are occluder volumes (invisible in the game)
            if n.as_str().contains("OCL_") || hide.iter().any(|h| n.as_str().contains(h.as_str())) {
                commands.entity(e).insert(Visibility::Hidden);
                info!("KK_HIDE: hiding {}", n);
            }
        }
        let name = names.get(e).map(|n| n.as_str().to_string()).unwrap_or_default();
        // level's own sky domes (ENV_Ciel*) and mist cards (*Brume*): self-lit like the
        // original's sky/FX shaders; the sky ignores distance fog [L]
        let sky = name.contains("ENV_Ciel");
        let mist = name.contains("Brume") || name.contains("brume");
        // large mist planes fade at their edges through per-vertex alpha in Jade, which the
        // exporter does not carry yet: hidden until then (hard-edged otherwise)
        // (all mist cards for now; ENV_Ciel2 is a second sky layer whose blend mode is not decoded)
        // LD_03E_ODE_* are the destructible physics doubles of the gate/arena blocks (the DEC_*
        // meshes already show them intact)
        // with the cloud layer (sky.rs) on, ENV_Ciel is replaced by the same texture on an
        // unfogged camera-following sphere so the sun gap can feed the god ray
        let replaced_sky = sky && crate::sky::enabled();
        // LD_03E_ODE_*: the breakable pieces ARE what the level draws of the gate / walls (the façade has an
        // opening there); breakable.rs moves them when smashed. Only the swamp scenes hide theirs.
        if mist || replaced_sky || name.contains("ENV_Ciel2") {
            commands.entity(e).insert(Visibility::Hidden);
            continue;
        }
        if let Ok(h) = q.get(e) {
            if sky || mist {
                if let Some(src) = mats.get(&h.0).cloned() {
                    let mut m = src;
                    m.unlit = true;
                    m.cull_mode = None;
                    if sky {
                        // the sky reads bright grey-white in the reference frames: dome unfogged and
                        // brightened (the original's after-effects lift it) [G]
                        // fogged: with the 35 m fog zone the dome dissolves into a smooth pale overcast
                        m.fog_enabled = true;
                    } else {
                        m.alpha_mode = AlphaMode::Blend;
                        let c = m.base_color.to_srgba();
                        m.base_color = Color::srgba(c.red * 0.9, c.green * 0.92, c.blue * 0.92, 0.55);
                    }
                    let nh = mats.add(m);
                    commands.entity(e).insert(MeshMaterial3d(nh));
                    // the sky dome must not shadow the moon light
                    commands.entity(e).insert((bevy::pbr::NotShadowCaster, bevy::pbr::NotShadowReceiver));
                }
                continue;
            }
            if !done.insert(h.0.id()) {
                continue;
            }
            if let Some(m) = mats.get_mut(&h.0) {
                m.perceptual_roughness = 1.0;
                m.reflectance = 0.08;
                // RLI multiplier (1 + 10*RLI, baked in COLOR_0) uses an unrecovered global RLI
                // scale; balanced against the reference frames [G]
                let c = m.base_color.to_linear();
                m.base_color = Color::LinearRgba(LinearRgba::new(c.red * 0.32, c.green * 0.32, c.blue * 0.32, c.alpha));
            }
        }
    }
}

/// The arms/weapon layer gets its own soft key + cool fill so it reads shaded like the
/// original's viewmodel instead of flat-lit by the world sun.
fn spawn_viewmodel_lights(commands: &mut Commands) {
    commands.spawn((
        Name::new("ViewKey"),
        DirectionalLight { illuminance: 8500.0, shadows_enabled: false, color: Color::srgb(0.95, 0.97, 0.92), ..default() },
        Transform::default().looking_to(Vec3::new(0.6, -0.8, -0.4), Vec3::Y),
        RenderLayers::layer(VIEW_LAYER),
    ));
    commands.spawn((
        Name::new("ViewFill"),
        DirectionalLight { illuminance: 2200.0, shadows_enabled: false, color: Color::srgb(0.55, 0.75, 0.8), ..default() },
        Transform::default().looking_to(Vec3::new(-0.7, 0.2, 0.3), Vec3::Y),
        RenderLayers::layer(VIEW_LAYER),
    ));
}
