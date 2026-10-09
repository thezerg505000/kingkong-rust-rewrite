//! Wall triangles for Jack's movement, read straight from the level glb the player supplies.
//!
//! The exported collision JSON only carries upward-facing ground triangles plus small prop boxes, so
//! ruins, rock faces and thick walls standing on ground could be walked into. Here every opaque mesh
//! of the level glb is read (positions + indices, node TRS baked) and its steep triangles become
//! walls. Foliage / water / mist (MASK and BLEND materials), occluders (`OCL_`) and the breakable ODE
//! pieces (`LD_*ODE*`, handled by `breakable.rs`) are left out. [G: OK3 collision blocks are not
//! decoded, so the render mesh stands in for the game's collision mesh]

use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

pub const CELL: f32 = 4.0;

/// Steep triangles of the level, glTF Y-up metres, bucketed on a 4 m XZ grid.
#[derive(Default)]
pub struct WallMesh {
    pub tris: Vec<[Vec3; 3]>,
    /// breakable group of each triangle (`world::BREAKABLES` index + 1, 0 = static)
    pub group: Vec<u8>,
    /// upward faces of the same meshes: walkable surfaces the exported ground set misses (stairs, steps,
    /// plinths) are merged into the ground triangles by `world.rs`
    pub floor: Vec<[Vec3; 3]>,
    pub grid: HashMap<(i32, i32), Vec<u32>>,
}

fn mat_of(n: &Value) -> Mat4 {
    if let Some(m) = n["matrix"].as_array() {
        let v: Vec<f32> = m.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
        if v.len() == 16 {
            return Mat4::from_cols_array(&v.try_into().unwrap());
        }
    }
    let f = |a: &Value, i: usize, d: f32| a.get(i).and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(d);
    let t = &n["translation"];
    let r = &n["rotation"];
    let s = &n["scale"];
    Mat4::from_scale_rotation_translation(
        Vec3::new(f(s, 0, 1.0), f(s, 1, 1.0), f(s, 2, 1.0)),
        Quat::from_xyzw(f(r, 0, 0.0), f(r, 1, 0.0), f(r, 2, 0.0), f(r, 3, 1.0)),
        Vec3::new(f(t, 0, 0.0), f(t, 1, 0.0), f(t, 2, 0.0)),
    )
}

struct Glb {
    json: Value,
    bin: Vec<u8>,
}

impl Glb {
    fn read(path: &std::path::Path) -> Option<Glb> {
        let d = std::fs::read(path).ok()?;
        if d.len() < 20 || &d[0..4] != b"glTF" {
            return None;
        }
        let jl = u32::from_le_bytes(d[12..16].try_into().ok()?) as usize;
        let json: Value = serde_json::from_slice(&d[20..20 + jl]).ok()?;
        let bo = 20 + jl;
        let bl = u32::from_le_bytes(d[bo..bo + 4].try_into().ok()?) as usize;
        let bin = d[bo + 8..bo + 8 + bl].to_vec();
        Some(Glb { json, bin })
    }

    fn view(&self, acc: usize) -> Option<(&[u8], usize, usize, u64, usize)> {
        let a = &self.json["accessors"][acc];
        let bv = &self.json["bufferViews"][a["bufferView"].as_u64()? as usize];
        let off = bv["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let count = a["count"].as_u64()? as usize;
        let comp = a["componentType"].as_u64()?;
        let ncomp = match a["type"].as_str()? {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            _ => return None,
        };
        let stride = bv["byteStride"].as_u64().unwrap_or(0) as usize;
        Some((&self.bin[off..], count, ncomp, comp, stride))
    }

    fn positions(&self, acc: usize) -> Option<Vec<Vec3>> {
        let (b, count, ncomp, comp, stride) = self.view(acc)?;
        if comp != 5126 || ncomp != 3 {
            return None;
        }
        let st = if stride == 0 { 12 } else { stride };
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let o = i * st;
            let f = |k: usize| f32::from_le_bytes(b[o + 4 * k..o + 4 * k + 4].try_into().unwrap());
            out.push(Vec3::new(f(0), f(1), f(2)));
        }
        Some(out)
    }

    fn indices(&self, acc: usize) -> Option<Vec<u32>> {
        let (b, count, _, comp, _) = self.view(acc)?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            out.push(match comp {
                5121 => b[i] as u32,
                5123 => u16::from_le_bytes(b[2 * i..2 * i + 2].try_into().unwrap()) as u32,
                5125 => u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap()),
                _ => return None,
            });
        }
        Some(out)
    }
}

/// Node names whose geometry never blocks Jack.
fn skip_node(name: &str) -> bool {
    name.contains("OCL_") || name.contains("Brume") || name.contains("Water") || name.contains("Eau") || name.contains("ODE") || name.contains("Herbe") || name.contains("Liane")
}

impl WallMesh {
    pub fn load(path: &std::path::Path) -> Option<WallMesh> {
        let g = Glb::read(path)?;
        let nodes = g.json["nodes"].as_array()?;
        let mut parent: HashMap<usize, usize> = HashMap::new();
        for (i, n) in nodes.iter().enumerate() {
            for c in n["children"].as_array().into_iter().flatten() {
                if let Some(c) = c.as_u64() {
                    parent.insert(c as usize, i);
                }
            }
        }
        let world = |mut i: usize| {
            let mut m = mat_of(&nodes[i]);
            while let Some(&p) = parent.get(&i) {
                m = mat_of(&nodes[p]) * m;
                i = p;
            }
            m
        };
        let opaque = |mi: Option<u64>| match mi {
            Some(mi) => g.json["materials"][mi as usize]["alphaMode"].as_str().unwrap_or("OPAQUE") == "OPAQUE",
            None => true,
        };
        let mut wm = WallMesh::default();
        for (i, n) in nodes.iter().enumerate() {
            let Some(mesh) = n["mesh"].as_u64() else { continue };
            let name = n["name"].as_str().unwrap_or("");
            if skip_node(name) {
                continue;
            }
            let m = world(i);
            for pr in g.json["meshes"][mesh as usize]["primitives"].as_array().into_iter().flatten() {
                if !opaque(pr["material"].as_u64()) {
                    continue;
                }
                if pr["mode"].as_u64().unwrap_or(4) != 4 {
                    continue;
                }
                let Some(pos) = pr["attributes"]["POSITION"].as_u64().and_then(|a| g.positions(a as usize)) else { continue };
                let idx = match pr["indices"].as_u64() {
                    Some(a) => match g.indices(a as usize) {
                        Some(v) => v,
                        None => continue,
                    },
                    None => (0..pos.len() as u32).collect(),
                };
                let wp: Vec<Vec3> = pos.iter().map(|p| m.transform_point3(*p)).collect();
                for t in idx.chunks_exact(3) {
                    let (a, b, c) = (wp[t[0] as usize], wp[t[1] as usize], wp[t[2] as usize]);
                    let cr = (b - a).cross(c - a);
                    let area2 = cr.length();
                    if area2 < 0.02 {
                        continue;
                    }
                    let ny = cr.y / area2;
                    // walkable faces go to the floor set; downward faces are ceilings
                    if ny > 0.55 {
                        wm.floor.push([a, b, c]);
                        continue;
                    }
                    if ny < -0.55 {
                        continue;
                    }
                    let g = crate::world::breakable_of_point((a + b + c) / 3.0).map_or(0, |g| g as u8 + 1);
                    wm.tris.push([a, b, c]);
                    wm.group.push(g);
                }
            }
        }
        for (i, t) in wm.tris.iter().enumerate() {
            let mn = t[0].min(t[1]).min(t[2]);
            let mx = t[0].max(t[1]).max(t[2]);
            for gx in (mn.x / CELL).floor() as i32..=(mx.x / CELL).floor() as i32 {
                for gz in (mn.z / CELL).floor() as i32..=(mx.z / CELL).floor() as i32 {
                    wm.grid.entry((gx, gz)).or_default().push(i as u32);
                }
            }
        }
        Some(wm)
    }

    /// Push a vertical capsule (feet at `p.y`, radius `r`) out of the walls in the band
    /// `[p.y + step, p.y + height]`. Returns the corrected position (y unchanged).
    pub fn push_out(&self, mut p: Vec3, r: f32, step: f32, height: f32, broken: &[bool]) -> Vec3 {
        let samples = [p.y + step.max(0.05), p.y + (step + height) * 0.5, p.y + height];
        for _ in 0..3 {
            let mut moved = false;
            let (cx0, cx1) = (((p.x - r) / CELL).floor() as i32, ((p.x + r) / CELL).floor() as i32);
            let (cz0, cz1) = (((p.z - r) / CELL).floor() as i32, ((p.z + r) / CELL).floor() as i32);
            let mut seen: Vec<u32> = Vec::new();
            for gx in cx0..=cx1 {
                for gz in cz0..=cz1 {
                    let Some(list) = self.grid.get(&(gx, gz)) else { continue };
                    for &ti in list {
                        if seen.contains(&ti) {
                            continue;
                        }
                        seen.push(ti);
                        let g = self.group.get(ti as usize).copied().unwrap_or(0);
                        if g > 0 && broken.get(g as usize - 1).copied().unwrap_or(false) {
                            continue;
                        }
                        let t = &self.tris[ti as usize];
                        let ymin = t[0].y.min(t[1].y).min(t[2].y);
                        let ymax = t[0].y.max(t[1].y).max(t[2].y);
                        for &sy in &samples {
                            if sy < ymin - r || sy > ymax + r {
                                continue;
                            }
                            let c = Vec3::new(p.x, sy, p.z);
                            let q = closest_on_tri(c, t);
                            // contacts below the step height are risers / kerbs Jack steps over
                            if q.y < p.y + step - 0.02 || (q.y - sy).abs() > r {
                                continue;
                            }
                            let d = Vec2::new(c.x - q.x, c.z - q.z);
                            let l = d.length();
                            if l < r {
                                let n = if l > 1e-4 {
                                    d / l
                                } else {
                                    let nn = (t[1] - t[0]).cross(t[2] - t[0]);
                                    Vec2::new(nn.x, nn.z).normalize_or(Vec2::X)
                                };
                                p.x = q.x + n.x * r;
                                p.z = q.z + n.y * r;
                                moved = true;
                            }
                        }
                    }
                }
            }
            if !moved {
                break;
            }
        }
        p
    }
}

impl WallMesh {
    /// Nearest wall face along the ray (distance, normal facing the ray), skipping broken breakables.
    pub fn ray(&self, o: Vec3, d: Vec3, max: f32, broken: &[bool]) -> Option<(f32, Vec3)> {
        let mut best: Option<(f32, Vec3)> = None;
        let mut seen: std::collections::HashSet<u32> = Default::default();
        let mut done_cells: std::collections::HashSet<(i32, i32)> = Default::default();
        let mut cells: Vec<(i32, i32)> = Vec::new();
        let step = CELL * 0.4;
        let n = (max / step).ceil() as usize + 1;
        for k in 0..=n {
            let t = (k as f32 * step).min(max);
            let p = o + d * t;
            let c = ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let cc = (c.0 + dx, c.1 + dz);
                    if done_cells.insert(cc) {
                        cells.push(cc);
                    }
                }
            }
            // stop once a hit nearer than this sample is known
            if best.is_some_and(|b| b.0 < t - CELL) {
                break;
            }
            for cc in cells.drain(..) {
                let Some(list) = self.grid.get(&cc) else { continue };
                for &ti in list {
                    if !seen.insert(ti) {
                        continue;
                    }
                    let g = self.group.get(ti as usize).copied().unwrap_or(0);
                    if g > 0 && broken.get(g as usize - 1).copied().unwrap_or(false) {
                        continue;
                    }
                    let tri = &self.tris[ti as usize];
                    if let Some(tt) = crate::world::ray_tri(o, d, tri) {
                        if tt > 0.0 && tt <= max && best.map_or(true, |b| tt < b.0) {
                            let nn = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
                            best = Some((tt, if nn.dot(d) > 0.0 { -nn } else { nn }));
                        }
                    }
                }
            }
            if t >= max {
                break;
            }
        }
        best
    }
}

/// Closest point on triangle `t` to `p` (Ericson, Real-Time Collision Detection 5.1.5).
pub fn closest_on_tri(p: Vec3, t: &[Vec3; 3]) -> Vec3 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    a + ab * v + ac * w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capsule_is_pushed_off_a_wall_but_steps_over_a_low_riser() {
        let mut wm = WallMesh::default();
        // wall plane x = 1, from y 0 to 3, z -5..5 (two tris)
        wm.tris.push([Vec3::new(1.0, 0.0, -5.0), Vec3::new(1.0, 3.0, -5.0), Vec3::new(1.0, 0.0, 5.0)]);
        wm.tris.push([Vec3::new(1.0, 3.0, -5.0), Vec3::new(1.0, 3.0, 5.0), Vec3::new(1.0, 0.0, 5.0)]);
        // riser x = -1, 0.3 m tall
        wm.tris.push([Vec3::new(-1.0, 0.0, -5.0), Vec3::new(-1.0, 0.3, -5.0), Vec3::new(-1.0, 0.0, 5.0)]);
        for (i, t) in wm.tris.iter().enumerate() {
            let mn = t[0].min(t[1]).min(t[2]);
            let mx = t[0].max(t[1]).max(t[2]);
            for gx in (mn.x / CELL).floor() as i32..=(mx.x / CELL).floor() as i32 {
                for gz in (mn.z / CELL).floor() as i32..=(mx.z / CELL).floor() as i32 {
                    wm.grid.entry((gx, gz)).or_default().push(i as u32);
                }
            }
        }
        let p = wm.push_out(Vec3::new(0.8, 0.0, 0.0), 0.35, 0.55, 1.8, &[]);
        assert!((p.x - 0.65).abs() < 1e-3, "{p:?}");
        let q = wm.push_out(Vec3::new(-1.1, 0.0, 0.0), 0.35, 0.55, 1.8, &[]);
        assert!((q.x + 1.1).abs() < 1e-4, "{q:?}");
    }
}
