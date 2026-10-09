//! Where Kong and the rex fight: the 2D arena found in the level's collision data.
//!
//! The fight simulation (`kk_mechanics::kong::fight`) runs on an empty 32 m disc. The level is not empty:
//! 03E's courtyard has ruin blocks and rocks, the swamp has banks, stumps and drops. This module builds a
//! 1 m clearance grid over a search region from the collision data (ground triangles + solid boxes):
//!
//! * a cell is **blocked** when there is no floor within the wanted height window, when something taller than
//!   1.6 m stands on the floor there (rock, pillar, wall: the highest ground triangle above the floor), or when
//!   it lies in a solid box that reaches into Kong's height band;
//! * the clearance of a free cell is the distance to the nearest blocked cell;
//! * the fight centre is the free cell with the largest clearance (ties: nearest to the preferred point);
//! * the fight plane is rotated about the centre so that the fight's x axis follows the longest free chord;
//! * Kong and the rex are moved with collide-and-slide against the free cells (`slide`), so neither walks
//!   into a pillar, and the camera / Jack's vantage use the same grid.
//!
//! * the fighters themselves walk on a second grid flood-filled from that floor over slopes and banks (up
//!   `STEP_UP` per metre, down `STEP_DOWN`), so Kong climbs ramps and banks instead of stopping at the edge of
//!   the flat floor; the flat grid still picks the centre, the axis and Jack's vantage.
//!
//! All thresholds are `[G]`; the data they read is `[C]` render-mesh derived collision.

use crate::world::LevelCollision;
use bevy::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct ArenaSpec {
    /// search region (x0, z0, x1, z1)
    pub region: (f32, f32, f32, f32),
    /// reference floor height and the window below / above it that counts as the fight floor
    pub yref: f32,
    pub below: f32,
    pub above: f32,
    /// preferred centre (used to break ties)
    pub prefer: (f32, f32),
    /// the fight never leaves this radius around the centre
    pub max_radius: f32,
}

pub struct FightArena {
    pub center: Vec3,
    /// fight plane rotation: plane +x points along world (cos rot, -sin rot) (see `kong::KongCtl::world`)
    pub rot: f32,
    /// Jack's vantage point and yaw (looking at the centre)
    pub jack: Vec3,
    pub jack_yaw: f32,
    /// start of Kong / the rex on the plane x axis
    pub kong_x: f32,
    pub rex_x: f32,
    pub best_clearance: f32,
    pub max_radius: f32,
    x0: f32,
    z0: f32,
    w: usize,
    h: usize,
    /// where the fighters may walk: every cell reachable from the flat fight floor over steps of at most
    /// `STEP_UP` up / `STEP_DOWN` down per metre (slopes, banks, ramps) [G]
    clear: Vec<f32>,
    floor: Vec<f32>,
    /// the flat fight floor (height window around `yref`): picks the centre, axis and Jack's vantage
    clear_flat: Vec<f32>,
    floor_flat: Vec<f32>,
}

const RES: f32 = 1.0;
/// Kong (and the rex) climb up to 0.95 m per metre (about 43 degrees) and step down 1.2 m per metre [G]
/// (2.2 let them walk down the cliffs at the map's edge and off the playable ground).
pub const STEP_UP: f32 = 0.95;
pub const STEP_DOWN: f32 = 1.2;
/// a walkable cell needs this much free height above its floor (Kong's body band) [G]
const HEADROOM: f32 = 4.0;

/// Clearance grid: distance (m) from every free cell to the nearest blocked cell, brute force within 26 cells.
fn clearance_grid(blocked: &[bool], w: usize, h: usize) -> Vec<f32> {
    let reach = 26i32;
    let mut clear = vec![0.0f32; w * h];
    for j in 0..h as i32 {
        for i in 0..w as i32 {
            if blocked[j as usize * w + i as usize] {
                continue;
            }
            let mut best = (reach * reach) as f32;
            for dj in -reach..=reach {
                let jj = j + dj;
                for di in -reach..=reach {
                    let ii = i + di;
                    let d2 = (di * di + dj * dj) as f32;
                    if d2 >= best {
                        continue;
                    }
                    let b = ii < 0 || jj < 0 || ii >= w as i32 || jj >= h as i32 || blocked[jj as usize * w + ii as usize];
                    if b {
                        best = d2;
                    }
                }
            }
            clear[j as usize * w + i as usize] = best.sqrt() * RES;
        }
    }
    clear
}

impl FightArena {
    pub fn build(level: &LevelCollision, spec: &ArenaSpec) -> Self {
        let (x0, z0, x1, z1) = spec.region;
        let w = ((x1 - x0) / RES) as usize;
        let h = ((z1 - z0) / RES) as usize;
        let mut blocked = vec![true; w * h];
        let mut floor = vec![f32::NAN; w * h];
        for j in 0..h {
            for i in 0..w {
                let (x, z) = (x0 + (i as f32 + 0.5) * RES, z0 + (j as f32 + 0.5) * RES);
                let Some(f) = level.ground_base(x, z, spec.yref + spec.above, 0.0) else { continue };
                if f < spec.yref - spec.below {
                    continue;
                }
                floor[j * w + i] = f;
                let top = level.ground_base(x, z, spec.yref + spec.above + 8.0, 0.0).unwrap_or(f);
                if top - f > 1.6 {
                    continue;
                }
                blocked[j * w + i] = false;
            }
        }
        // solid boxes that reach into Kong's height band
        for (mn, mx) in &level.obstacles {
            if mx.y < spec.yref + 0.3 || mn.y > spec.yref + spec.above + 4.0 {
                continue;
            }
            let i0 = (((mn.x - 0.6 - x0) / RES).floor().max(0.0)) as usize;
            let i1 = (((mx.x + 0.6 - x0) / RES).ceil().max(0.0) as usize).min(w);
            let j0 = (((mn.z - 0.6 - z0) / RES).floor().max(0.0)) as usize;
            let j1 = (((mx.z + 0.6 - z0) / RES).ceil().max(0.0) as usize).min(h);
            for j in j0..j1 {
                for i in i0..i1 {
                    blocked[j * w + i] = true;
                }
            }
        }
        let clear_flat = clearance_grid(&blocked, w, h);
        let (walk_blocked, walk_floor) = Self::walk_grid(level, spec, &blocked, &floor, (x0, z0, w, h));
        let clear = clearance_grid(&walk_blocked, w, h);
        let mut a = Self {
            center: Vec3::ZERO,
            rot: 0.0,
            jack: Vec3::ZERO,
            jack_yaw: 0.0,
            kong_x: -10.0,
            rex_x: 8.0,
            best_clearance: 0.0,
            max_radius: spec.max_radius,
            x0,
            z0,
            w,
            h,
            clear,
            floor: walk_floor,
            clear_flat,
            floor_flat: floor,
        };
        a.pick_center(spec);
        a.pick_axis();
        a.pick_vantage(level);
        a
    }

    /// Flood fill from the flat fight floor: a neighbour cell is walkable when its floor (highest ground not above
    /// the current floor + `STEP_UP`) is no more than `STEP_DOWN` below, nothing tall stands on it and no solid box
    /// fills Kong's body band above it. Uses the full ground set (level mesh floors included where loaded).
    fn walk_grid(level: &LevelCollision, spec: &ArenaSpec, flat_blocked: &[bool], flat_floor: &[f32], g: (f32, f32, usize, usize)) -> (Vec<bool>, Vec<f32>) {
        let (x0, z0, w, h) = g;
        let centre = |i: usize, j: usize| (x0 + (i as f32 + 0.5) * RES, z0 + (j as f32 + 0.5) * RES);
        let mut boxes: Vec<Vec<u32>> = vec![Vec::new(); w * h];
        for (k, (mn, mx)) in level.obstacles.iter().enumerate() {
            let i0 = (((mn.x - 0.6 - x0) / RES).floor().max(0.0)) as usize;
            let i1 = (((mx.x + 0.6 - x0) / RES).ceil().max(0.0) as usize).min(w);
            let j0 = (((mn.z - 0.6 - z0) / RES).floor().max(0.0)) as usize;
            let j1 = (((mx.z + 0.6 - z0) / RES).ceil().max(0.0) as usize).min(h);
            for j in j0..j1 {
                for i in i0..i1 {
                    boxes[j * w + i].push(k as u32);
                }
            }
        }
        let boxed = |c: usize, f: f32| {
            boxes[c].iter().any(|&k| {
                let (mn, mx) = level.obstacles[k as usize];
                mn.y < f + HEADROOM && mx.y > f + 0.3
            })
        };
        let mut floor = vec![f32::NAN; w * h];
        let mut ok = vec![false; w * h];
        let mut queue = std::collections::VecDeque::new();
        for c in 0..w * h {
            if !flat_blocked[c] {
                ok[c] = true;
                floor[c] = flat_floor[c];
                queue.push_back(c);
            }
        }
        // at most 3.5 m below the fight floor: deeper is the outside of the map [G]
        let (ylo, yhi) = (spec.yref - 3.5, spec.yref + 14.0);
        while let Some(c) = queue.pop_front() {
            let (i, j) = ((c % w) as i32, (c / w) as i32);
            let f0 = floor[c];
            for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (ii, jj) = (i + di, j + dj);
                if ii < 0 || jj < 0 || ii >= w as i32 || jj >= h as i32 {
                    continue;
                }
                let n = jj as usize * w + ii as usize;
                if ok[n] {
                    continue;
                }
                let (x, z) = centre(ii as usize, jj as usize);
                let Some(f) = level.ground(x, z, f0 + STEP_UP, 0.0) else { continue };
                if f < f0 - STEP_DOWN || f < ylo || f > yhi {
                    continue;
                }
                // something taller than 1.6 m standing on the floor here (rock face, pillar, wall)
                let top = level.ground(x, z, f + HEADROOM, 0.0).unwrap_or(f);
                if top - f > 1.6 || boxed(n, f) {
                    continue;
                }
                ok[n] = true;
                floor[n] = f;
                queue.push_back(n);
            }
        }
        (ok.iter().map(|o| !o).collect(), floor)
    }

    fn flat_clearance(&self, x: f32, z: f32) -> f32 {
        self.idx(x, z).map_or(0.0, |i| self.clear_flat[i])
    }

    fn flat_floor_at(&self, x: f32, z: f32) -> Option<f32> {
        self.idx(x, z).map(|i| self.floor_flat[i]).filter(|f| f.is_finite())
    }

    fn idx(&self, x: f32, z: f32) -> Option<usize> {
        let i = ((x - self.x0) / RES).floor();
        let j = ((z - self.z0) / RES).floor();
        if i < 0.0 || j < 0.0 || i >= self.w as f32 || j >= self.h as f32 {
            return None;
        }
        Some(j as usize * self.w + i as usize)
    }

    /// Clearance (m) to the nearest blocked cell around the world point; 0 outside the grid / on blocked cells.
    pub fn clearance(&self, x: f32, z: f32) -> f32 {
        self.idx(x, z).map_or(0.0, |i| self.clear[i])
    }

    pub fn floor_at(&self, x: f32, z: f32) -> Option<f32> {
        self.idx(x, z).map(|i| self.floor[i]).filter(|f| f.is_finite())
    }

    fn pick_center(&mut self, spec: &ArenaSpec) {
        let mut best = (-1.0f32, 0usize);
        for j in 0..self.h {
            for i in 0..self.w {
                let c = self.clear_flat[j * self.w + i];
                if c <= 0.0 {
                    continue;
                }
                let (x, z) = (self.x0 + (i as f32 + 0.5) * RES, self.z0 + (j as f32 + 0.5) * RES);
                let d = ((x - spec.prefer.0).powi(2) + (z - spec.prefer.1).powi(2)).sqrt();
                // largest empty circle first; a distance penalty keeps the choice near the preferred point
                let s = c.min(14.0) - d * 0.02;
                if s > best.0 {
                    best = (s, j * self.w + i);
                }
            }
        }
        let (j, i) = (best.1 / self.w, best.1 % self.w);
        let (x, z) = (self.x0 + (i as f32 + 0.5) * RES, self.z0 + (j as f32 + 0.5) * RES);
        self.best_clearance = self.clear_flat[best.1];
        self.center = Vec3::new(x, self.flat_floor_at(x, z).unwrap_or(spec.yref), z);
    }

    /// Rotate the fight plane so its x axis follows the longest free chord through the centre.
    fn pick_axis(&mut self) {
        let c = self.center;
        let reach = |a: f32, dir: f32| -> f32 {
            // free length from the centre along the world direction (cos a, -sin a) * dir, margin 2.5 m
            let (dx, dz) = (a.cos() * dir, -a.sin() * dir);
            let mut s = 0.0;
            while s < self.max_radius {
                if self.flat_clearance(c.x + dx * s, c.z + dz * s) < 2.5 {
                    break;
                }
                s += 0.5;
            }
            s
        };
        let mut best = (-1.0f32, 0.0f32, 0.0f32, 0.0f32);
        for k in 0..24 {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            let (neg, pos) = (reach(a, -1.0), reach(a, 1.0));
            let s = neg.min(pos * 1.6) + pos.min(neg * 1.6);
            if s > best.0 {
                best = (s, a, neg, pos);
            }
        }
        self.rot = best.1;
        // Kong starts on the -x side, the rex on +x, as far apart as the chord allows (24 m in the sim)
        self.kong_x = -(best.2 - 1.5).clamp(5.0, 15.0);
        self.rex_x = (best.3 - 3.5).clamp(5.0, 9.0);
    }

    /// Jack's vantage: 17..28 m from the centre, on a free cell, with an unobstructed 3 m high sight line to the
    /// centre; prefers high clearance and a floor not far below the fight floor.
    fn pick_vantage(&mut self, level: &LevelCollision) {
        let c = self.center;
        let mut best = (-1.0e9f32, c + Vec3::new(0.0, 0.0, 20.0));
        for min_cl in [2.5f32, 1.5] {
        if best.0 > -1.0e8 {
            break;
        }
        for k in 0..36 {
            let a = k as f32 / 36.0 * std::f32::consts::TAU;
            for r in [17.0f32, 20.0, 23.0, 26.0] {
                let (x, z) = (c.x + a.cos() * r, c.z + a.sin() * r);
                let cl = self.flat_clearance(x, z);
                let Some(f) = self.flat_floor_at(x, z) else { continue };
                if cl < min_cl || (f - c.y).abs() > 2.5 {
                    continue;
                }
                if !self.sight(level, Vec3::new(x, f + 1.7, z), Vec3::new(c.x, c.y + 3.0, c.z)) {
                    continue;
                }
                let s = cl.min(6.0) - (r - 20.0).abs() * 0.15;
                if s > best.0 {
                    best = (s, Vec3::new(x, f, z));
                }
            }
        }
        }
        self.jack = best.1;
        let d = c - self.jack;
        self.jack_yaw = (-d.x).atan2(-d.z);
    }

    /// Unobstructed line of sight between two points against the solid boxes and rising ground.
    pub fn sight(&self, level: &LevelCollision, a: Vec3, b: Vec3) -> bool {
        level.segment_clear(a, b)
    }

    /// Move from `from` to `to` (world XZ) without entering cells with clearance below `margin`: collide-and-slide.
    pub fn slide(&self, from: Vec2, to: Vec2, margin: f32) -> Vec2 {
        let ok = |p: Vec2| {
            self.clearance(p.x, p.y) >= margin && (p - Vec2::new(self.center.x, self.center.z)).length() <= self.max_radius
        };
        if ok(to) {
            return to;
        }
        let d = to - from;
        let n = ((d.length() / 0.25).ceil() as usize).max(1);
        let mut p = from;
        if !ok(p) {
            // started inside a wall (teleport / start up): walk toward the centre until free
            let c = Vec2::new(self.center.x, self.center.z);
            for k in 1..=80 {
                let q = p.lerp(c, k as f32 / 80.0);
                if ok(q) {
                    return q;
                }
            }
            return c;
        }
        let step = d / n as f32;
        for _ in 0..n {
            let q = p + step;
            if ok(q) {
                p = q;
                continue;
            }
            // slide: keep the component that stays free
            let qx = Vec2::new(p.x + step.x, p.y);
            let qz = Vec2::new(p.x, p.y + step.y);
            if ok(qx) {
                p = qx;
            } else if ok(qz) {
                p = qz;
            } else {
                break;
            }
        }
        p
    }

    /// Highest walkable floor above the fight centre within the fight radius (m): how far up slopes and banks the
    /// fighters can go (0 = the old flat-only arena).
    pub fn climb(&self) -> f32 {
        let mut best = 0.0f32;
        for j in 0..self.h {
            for i in 0..self.w {
                let c = j * self.w + i;
                if self.clear[c] < 1.6 || !self.floor[c].is_finite() {
                    continue;
                }
                let (x, z) = (self.x0 + (i as f32 + 0.5) * RES, self.z0 + (j as f32 + 0.5) * RES);
                if Vec2::new(x - self.center.x, z - self.center.z).length() > self.max_radius {
                    continue;
                }
                best = best.max(self.floor[c] - self.center.y);
            }
        }
        best
    }

    /// PGM dump of the clearance grid (debug: `KK_ARENA_DUMP=<file>.pgm`).
    pub fn dump_pgm(&self, path: &str) {
        let mut s = format!("P2\n{} {}\n255\n", self.w, self.h);
        for j in 0..self.h {
            for i in 0..self.w {
                let c = self.clear[j * self.w + i];
                let v = if c <= 0.0 { 0 } else { (40.0 + c.min(12.0) / 12.0 * 215.0) as i32 };
                s.push_str(&format!("{v} "));
            }
            s.push('\n');
        }
        let _ = std::fs::write(path, s);
        info!("arena grid dumped to {path}: x0 {} z0 {} {}x{}", self.x0, self.z0, self.w, self.h);
    }
}
