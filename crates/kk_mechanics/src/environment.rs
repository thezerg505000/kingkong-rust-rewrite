//! Fire, water and environment rules (ledger group `ENVIRONMENT_INTERACTION`, fire/water part:
//! E01-E07, E12, E13, E15-E18, W02, W04, W09, W10). Evidence: `spec/evidence/E01.md` (main) and
//! the short docs that point to it.
//!
//! Tags: `[C]` read from the decompiled exe, `[L]` inferred, `[G]` guess.
//!
//! The world has a **ground grid** (two byte layers, `DAT_00bdf5f8+0x22c/+0x230`, selected by
//! `fn@0x00427160(layer)`; read by `fn@0x004274e0@0x4274e0` -> `fn@0x0094a360`). Layer 1 byte:
//! low nibble = ground material, high nibble = fire state. Fire is simulated on that grid by
//! the `SGF_*` objects ("spreading grid fire"), creatures read the byte to know they stand in
//! flames, and torches/lamps are lit by *fire zones* (`Fire_PointInFireZone@0x6e3210`).

use crate::Confidence;

// ---------------------------------------------------------------- ground grid byte
/// low nibble mask of a ground-grid byte: material id. [C] every `& 0xf` in `SGF_Propage@0x6827c0`,
/// `Javelin_waittaken@0x87d160`, `fn@0x006c6750@0x6c6750`.
pub const GRID_MATERIAL_MASK: u8 = 0x0f;
/// material 8 = flammable ground (grass/brush). [C] `SGF_Propage`, `SGF_CP@0x683070`,
/// `TrigExec_Burn@0x78af30` all test `(v & 0xf) == 8`.
pub const MATERIAL_FLAMMABLE: u8 = 8;
/// material 3 = deep water cell (swamp crawler's target). [C] `SC_compute_NearestCaseOfWater@0x551210`.
pub const MATERIAL_WATER: u8 = 3;
/// material 7 = cell where Kong-level bats refuse to go. [C] `fn@0x006c6750@0x6c6750`.
pub const MATERIAL_BAT_FORBIDDEN: u8 = 7;
/// fire state "burning" (high nibble). [C] `SGF_Propage` sets via `fn@0x00427350(...,0x40)`; read as `& 0x40`
/// by raptor/scorpion/scolo/bat reflexes, `it_loop_off`, `TrigTest_PositionGrilleEnflam@0x46a360`.
pub const FIRE_BURNING: u8 = 0x40;
/// fire state "burnt out" (cannot re-ignite): written as 0xc0 then 0x80 on burn-out. [C] `SGF_Propage`.
pub const FIRE_BURNT: u8 = 0x80;
/// the nav layer (layer 0) gets +0x20 while a cell burns (AI avoidance) and -0x20 at burn-out. [C] `SGF_Propage`
/// (`fn@0x00427410(..,0x20)` / `0xffffffe0`); meaning "AI avoidance" is [L].
pub const NAV_FIRE_MARK: u8 = 0x20;
/// ring buffer capacity of burning cells per fire object. [C] `fn@0x0041ea90(x,200)` in `SGF_Propage`.
pub const SGF_RING: usize = 200;
/// paf damage a creature standing in a burning cell receives. [C] `PNJ_Raptor_reflex@0x835ea0`,
/// `PNJ_Scorpion_reflex@0x4f8610`, `PNJ_Scolo_Reflex@0x81a710` (`Msg_SendPaf(...,0x200200,...,1000,...)`).
pub const BURN_PAF_DAMAGE: f32 = 1000.0;
/// paf flag word used for fire damage to creatures (0x200000 = fire, 0x200 = generic hit). [C] same sites.
pub const PAF_FLAGS_FIRE_CREATURE: u32 = 0x0020_0200;
/// paf flag bit "fire" that ignites ODE objects. [C] `ode_check_trigger@0x6a1dd0` (`& 0x200000`).
pub const PAF_FIRE_BIT: u32 = 0x0020_0000;

/// Ground byte helpers (one grid cell).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroundByte(pub u8);

impl GroundByte {
    pub fn material(self) -> u8 {
        self.0 & GRID_MATERIAL_MASK
    }
    pub fn is_flammable(self) -> bool {
        self.material() == MATERIAL_FLAMMABLE
    }
    pub fn is_burning(self) -> bool {
        self.0 & FIRE_BURNING != 0
    }
    pub fn is_burnt(self) -> bool {
        self.0 & FIRE_BURNT != 0
    }
    /// A cell can be ignited when it is flammable and neither burning nor burnt. [C] `SGF_Propage`,
    /// `SGF_CP`, `TrigExec_Burn`.
    pub fn can_ignite(self) -> bool {
        self.is_flammable() && !self.is_burning() && !self.is_burnt()
    }
}

/// `H_IsWadeMode@0x689ff0`: Jack wades in materials 3, 9, 10, 11, 12. [C]
pub fn is_wade_material(m: u8) -> bool {
    matches!(m, 3 | 9 | 10 | 11 | 12)
}

/// `Javelin_waittaken@0x87d160`: a thrown spear does not stick in materials 3, 9, 0xb, 0xa, 0xc, 1, 5. [C]
/// (the 0x400000 flag of the hit object overrides the water cases, [C]).
pub fn javelin_refuses_material(m: u8) -> bool {
    matches!(m, 1 | 3 | 5 | 9 | 10 | 11 | 12)
}

/// `fn@0x0068a930@0x68a930`: ground check used by companion/Jack helper code; true on material 1, 12
/// or a burning cell. [C]
pub fn companion_unsafe_ground(v: u8) -> bool {
    let b = GroundByte(v);
    v == 1 || v == 0xc || b.is_burning()
}

/// Two-layer ground grid (cell units). Only what the fire simulation needs.
#[derive(Clone, Debug)]
pub struct GroundGrid {
    pub w: i32,
    pub h: i32,
    /// layer 1: material | fire state
    pub ground: Vec<u8>,
    /// layer 0: nav counter (adds `NAV_FIRE_MARK` per burning cell)
    pub nav: Vec<u8>,
}

impl GroundGrid {
    pub fn new(w: i32, h: i32, fill: u8) -> Self {
        GroundGrid { w, h, ground: vec![fill; (w * h) as usize], nav: vec![0; (w * h) as usize] }
    }
    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            None
        } else {
            Some((y * self.w + x) as usize)
        }
    }
    /// `fn@0x004274e0`: out-of-grid reads give 0 (`fn@0x0094a360` returns 0 when the cell is invalid). [C]
    pub fn get(&self, x: i32, y: i32) -> u8 {
        self.idx(x, y).map_or(0, |i| self.ground[i])
    }
    pub fn set_material(&mut self, x: i32, y: i32, m: u8) {
        if let Some(i) = self.idx(x, y) {
            self.ground[i] = (self.ground[i] & 0xf0) | (m & 0x0f);
        }
    }
    /// `fn@0x00427350@0x427350` / `fn@0x0094a150`: keep low nibble, overwrite the high nibble. [C]
    pub fn set_state(&mut self, x: i32, y: i32, v: u8) {
        if let Some(i) = self.idx(x, y) {
            self.ground[i] = (self.ground[i] & 0x0f) | (v & 0xf0);
        }
    }
    pub fn nav_add(&mut self, x: i32, y: i32, d: u8) {
        if let Some(i) = self.idx(x, y) {
            self.nav[i] = self.nav[i].wrapping_add(d);
        }
    }
    pub fn nav_sub(&mut self, x: i32, y: i32, d: u8) {
        if let Some(i) = self.idx(x, y) {
            self.nav[i] = self.nav[i].wrapping_sub(d);
        }
    }
}

// ---------------------------------------------------------------- SGF grid fire (E01-E03)
/// Events produced by [`GridFire::tick`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FireEvent {
    Ignited(i32, i32),
    BurntOut(i32, i32),
    /// every cell has burnt out: the object goes to `SGF_Fin` (state `DAT_00b997ec`)
    Finished,
}

/// The fire object of `SGF_*` (`SGF_Init@0x6824d0`, `SGF_Propage@0x6827c0`, `SGF_Fin@0x682f20`).
/// Fields: `burn_time` = `this+0xb4` (cell lifetime, 0 = never burns out), `spread_interval` =
/// `this+0xb8` (one new cell per interval). Both can be overridden by the level through
/// `TrigExec_BurnParams@0x78aee0` (Univers+0x16620 / +0x16624) when non-zero. [C]
#[derive(Clone, Debug)]
pub struct GridFire {
    pub burn_time: f32,
    pub spread_interval: f32,
    cells: [(i32, i32); SGF_RING],
    ignite_time: [f32; SGF_RING],
    /// `this+0x109c`: cell that is being expanded
    front: usize,
    /// `this+0x10a0`: oldest burning cell (next to burn out)
    oldest: usize,
    /// `this+0x10a4`: next free slot
    tail: usize,
    /// `this+0x10ac`: time of the last spread tick
    last_spread: f32,
}

impl GridFire {
    /// `SGF_Init`: seed cell = the object's own position, `tail = 1`, start time recorded. Level
    /// overrides (`univ_burn_time`, `univ_interval`) replace the instance values when != 0. [C]
    pub fn new(seed: (i32, i32), now: f32, burn_time: f32, spread_interval: f32, univ_burn_time: f32, univ_interval: f32) -> Self {
        let mut f = GridFire {
            burn_time: if univ_burn_time != 0.0 { univ_burn_time } else { burn_time },
            spread_interval: if univ_interval != 0.0 { univ_interval } else { spread_interval },
            cells: [(0, 0); SGF_RING],
            ignite_time: [0.0; SGF_RING],
            front: 0,
            oldest: 0,
            tail: 1,
            last_spread: 0.0,
        };
        f.cells[0] = seed;
        f.ignite_time[0] = now;
        f
    }

    pub fn burning_cells(&self) -> usize {
        (self.tail + SGF_RING - self.oldest) % SGF_RING
    }

    /// `TrigExec_Burn@0x78af30` start rule: the target position's own cell if it can ignite, else the
    /// first of its 8 neighbours (dx -1..1 outer, dy -1..1 inner) that can. [C]
    pub fn find_start_cell(grid: &GroundGrid, x: i32, y: i32) -> Option<(i32, i32)> {
        if GroundByte(grid.get(x, y)).material() == MATERIAL_FLAMMABLE
            && grid.get(x, y) & 0xc0 == 0
        {
            return Some((x, y));
        }
        for dx in -1..=1 {
            for dy in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                if GroundByte(grid.get(x + dx, y + dy)).can_ignite() {
                    return Some((x + dx, y + dy));
                }
            }
        }
        None
    }

    fn ignite(&mut self, grid: &mut GroundGrid, now: f32, c: (i32, i32), slot: usize) {
        self.cells[slot] = c;
        self.ignite_time[slot] = now;
        grid.set_state(c.0, c.1, FIRE_BURNING);
        grid.nav_add(c.0, c.1, NAV_FIRE_MARK);
    }

    /// One frame of `SGF_Propage` (spread part and burn-out part). `now` in seconds. [C]
    pub fn tick(&mut self, grid: &mut GroundGrid, now: f32) -> Vec<FireEvent> {
        let mut ev = Vec::new();
        // spread: one cell per `spread_interval` while there is a front cell.
        if now - self.last_spread >= self.spread_interval && self.front < self.tail_abs() {
            while self.front != self.tail {
                self.last_spread = now;
                let c = self.cells[self.front];
                if grid.get(c.0, c.1) & 0xc0 == 0 {
                    // seed cell not yet burning: ignite it (only reached for the very first cell)
                    let slot = self.front;
                    self.ignite(grid, now, c, slot);
                    ev.push(FireEvent::Ignited(c.0, c.1));
                    break;
                }
                let mut found = None;
                'n: for dx in -1..=1 {
                    for dy in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        if GroundByte(grid.get(c.0 + dx, c.1 + dy)).can_ignite() {
                            found = Some((c.0 + dx, c.1 + dy));
                            break 'n;
                        }
                    }
                }
                if let Some(n) = found {
                    let slot = self.tail;
                    self.ignite(grid, now, n, slot);
                    self.tail = (self.tail + 1) % SGF_RING;
                    ev.push(FireEvent::Ignited(n.0, n.1));
                    break;
                }
                // no free neighbour: this cell is exhausted, move the front on
                self.front = (self.front + 1) % SGF_RING;
            }
        }
        // burn-out: oldest cell, one per frame, after `burn_time` (0 = never).
        if self.burn_time != 0.0 && now - self.ignite_time[self.oldest] >= self.burn_time && self.oldest != self.tail {
            let c = self.cells[self.oldest];
            grid.nav_sub(c.0, c.1, NAV_FIRE_MARK);
            grid.set_state(c.0, c.1, 0xc0);
            grid.set_state(c.0, c.1, FIRE_BURNT);
            ev.push(FireEvent::BurntOut(c.0, c.1));
            self.oldest = (self.oldest + 1) % SGF_RING;
            if self.oldest == self.tail {
                ev.push(FireEvent::Finished);
            }
        }
        ev
    }

    fn tail_abs(&self) -> usize {
        // the original loops `while front != tail`; this guard only prevents a pointless entry
        // when front == tail.
        if self.front == self.tail { self.front } else { self.front + 1 }
    }
}

/// `SGF_CP@0x683070` checkpoint restore: when the level already recorded the fire as burnt, flood
/// the 3x3-connected flammable unburnt cells from the seed and mark them burnt (0x80). [C]
pub fn restore_burnt(grid: &mut GroundGrid, seed: (i32, i32)) -> usize {
    let mut queue = vec![seed];
    let mut head = 0;
    let mut marked = 0;
    while head < queue.len() && queue.len() < SGF_RING {
        let c = queue[head];
        head += 1;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let n = (c.0 + dx, c.1 + dy);
                if GroundByte(grid.get(n.0, n.1)).can_ignite() {
                    grid.set_state(n.0, n.1, FIRE_BURNT);
                    marked += 1;
                    if queue.len() < SGF_RING {
                        queue.push(n);
                    }
                }
            }
        }
    }
    marked
}

// ---------------------------------------------------------------- creature burn reaction (E04)
/// Which creature is reading the cell it stands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BurnVictim {
    Raptor,
    Scorpion,
    Scolo,
}

/// Cadence of the 1000-damage paf while the creature stands on a burning cell. Raptor: every frame
/// (`PNJ_Raptor_reflex@0x835ea0:302-325`, no timer). Scorpion: timer reset to Rand(0.5,1.5)
/// (`PNJ_Scorpion_reflex@0x4f8610`). Scolo: timer reset to Rand(0.3,0.5) per body part
/// (`PNJ_Scolo_Reflex@0x81a710`). [C]
pub fn burn_paf_interval(v: BurnVictim, rand01: f32) -> f32 {
    match v {
        BurnVictim::Raptor => 0.0,
        BurnVictim::Scorpion => 0.5 + rand01 * (1.5 - 0.5),
        BurnVictim::Scolo => 0.3 + rand01 * (0.5 - 0.3),
    }
}

/// Raptor `ETAT_BURN` (`PNJ_Raptor_ETAT_BURN@0x872a10`): the panic state is left when the burning flag
/// (`this+0x38e0`) is cleared or after 8.0 s in the state. [C]
pub const RAPTOR_BURN_STATE_MAX: f32 = 8.0;
/// Raptor burning flag is set to 1.0 when standing on a burning cell. [C] `PNJ_Raptor_reflex`.
pub fn raptor_burn_state_over(burning_flag: f32, time_in_state: f32) -> bool {
    burning_flag == 0.0 || time_in_state > RAPTOR_BURN_STATE_MAX
}

/// `fn@0x0068a9b0@0x68a9b0` (from `H_TRACK_Reflex`): the "fire near me" flag (`this+0x3cc8`) is true when any cell of the
/// 5x5 block (+-2) around the actor is burning (and the level has fire). AUDIT 2026-10-07: the original only
/// runs this for AI companions (`*this == 0`), not for Jack; the name is kept for API stability. [C]
pub fn jack_near_fire(grid: &GroundGrid, x: i32, y: i32) -> bool {
    for dx in -2..=2 {
        for dy in -2..=2 {
            if grid.get(x + dx, y + dy) & FIRE_BURNING != 0 {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------- fire zones (E02, E06, E07)
/// Fire zone object (message-list type 0x17, `fn@0x0043b780(0x17,..)`). [C] `Fire_PointInFireZone@0x6e3210`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FireZone {
    /// kind 0: sphere
    Sphere { c: [f32; 3], r: f32 },
    /// kind 1: capsule-like segment `base .. base+axis`, radius r; only points whose projection
    /// parameter t is strictly inside (0,1) count (no end caps). [C]
    Segment { base: [f32; 3], axis: [f32; 3], r: f32 },
}

/// Radius of the fire zone the fire network creates at the held spear's tip. [C] `fn@0x004d5b60@0x4d5b60`
/// (`fn@0x006e3140(...,0.5)`), only while the held item id is 6, 7 or 8.
pub const NET_FIRE_TIP_ZONE_RADIUS: f32 = 0.5;

pub fn point_in_fire_zone(zones: &[FireZone], p: [f32; 3]) -> bool {
    for z in zones {
        match *z {
            FireZone::Sphere { c, r } => {
                let d = [c[0] - p[0], c[1] - p[1], c[2] - p[2]];
                if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < r * r {
                    return true;
                }
            }
            FireZone::Segment { base, axis, r } => {
                let d = [p[0] - base[0], p[1] - base[1], p[2] - base[2]];
                let t = (d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2])
                    / (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]);
                if 0.0 < t && t < 1.0 {
                    let q = [d[0] - axis[0] * t, d[1] - axis[1] * t, d[2] - axis[2] * t];
                    if q[0] * q[0] + q[1] * q[1] + q[2] * q[2] < r * r {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Torch / brazier (`it_*`: `it_loop_off@0x4ce740`) ignition test: lit when forced by a level flag, or
/// a fire zone contains its point, or a random cell within one step of it is burning. [C]
pub fn torch_lights(forced: bool, in_zone: bool, nearby_cell_burning: bool) -> bool {
    forced || in_zone || nearby_cell_burning
}
/// Default torch parameters (`it_init@0x4cd5c0`): flame radius 0.2 when unset, second radius = first, scale 1.0. [C]
pub const TORCH_DEFAULT_RADIUS: f32 = 0.2;
pub const TORCH_DEFAULT_SCALE: f32 = 1.0;

// ---------------------------------------------------------------- ODE objects on fire (E01, E13, E17)
/// Burning physics object (`ode_check_trigger@0x6a1dd0` ignites, `ode_exec_gestion_feu@0x6a3740` burns).
pub const ODE_FLAME_START: f32 = 0.5;
/// intensity growth per frame (frame-based, not dt-based) until 2.0. [C]
pub const ODE_FLAME_STEP: f32 = 0.01;
pub const ODE_FLAME_MAX: f32 = 2.0;
/// intensity at which the object starts igniting neighbours. [C]
pub const ODE_SPREAD_INTENSITY: f32 = 1.5;
/// seconds after ignition when the fire dies. [C]
pub const ODE_BURN_SECONDS: f32 = 10.0;

#[derive(Clone, Copy, Debug)]
pub struct OdeBurn {
    pub intensity: f32,
    pub ignited_at: f32,
}

impl OdeBurn {
    pub fn ignite(now: f32) -> Self {
        OdeBurn { intensity: ODE_FLAME_START, ignited_at: now }
    }
    /// one frame of the first loop of `ode_exec_gestion_feu`. Returns true when the fire is out.
    pub fn frame(&mut self, now: f32) -> bool {
        if ODE_FLAME_MAX <= self.intensity {
            // saturated: nothing to add (the original refreshes the flame-FX parameters here)
        } else {
            self.intensity += ODE_FLAME_STEP;
        }
        now - self.ignited_at > ODE_BURN_SECONDS
    }
    pub fn spreads(&self) -> bool {
        ODE_SPREAD_INTENSITY <= self.intensity
    }
    /// frames from ignition to spreading: (1.5-0.5)/0.01 = 100, 101 with f32 rounding.
    pub fn frames_to_spread() -> u32 {
        let mut b = OdeBurn::ignite(0.0);
        let mut n = 0;
        while !b.spreads() {
            b.frame(0.0);
            n += 1;
        }
        n
    }
}

// ---------------------------------------------------------------- water (W02)
/// `MM_Exec_Water_Height@0x644070`: amplitude of the water effect as a function of the camera height
/// above the water plane: full strength within 50, linear fade to 0 at 200. [C]
pub fn water_effect_scale(cam_z: f32, water_z: f32, base: f32) -> f32 {
    let d = (cam_z - water_z).abs();
    if d <= 50.0 {
        return base;
    }
    let t = ((d - 50.0) / 150.0).min(1.0);
    let k = 1.0 - t;
    if k == 0.0 { 0.0 } else { k * base }
}

/// Quicksand pull (`Trap_QS_DETECT_c@0x6ee6a0`): captured clients are lerped towards the sink point at
/// `dt * 6.0` per frame. [C]
pub const QUICKSAND_PULL_RATE: f32 = 6.0;

/// Confidence of the main recovered numbers, for tooling.
pub const CONFIDENCE: &[(&str, Confidence)] = &[
    ("grid byte layout", Confidence::C),
    ("burn paf damage 1000", Confidence::C),
    ("spread rule (1 cell per interval, first free neighbour)", Confidence::C),
    ("cell size = 1 world unit", Confidence::L),
    ("per-level burn time / interval values", Confidence::G),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn meadow(n: i32) -> GroundGrid {
        GroundGrid::new(n, n, MATERIAL_FLAMMABLE)
    }

    #[test]
    fn ground_byte_flags() {
        assert!(GroundByte(0x08).can_ignite());
        assert!(!GroundByte(0x48).can_ignite());
        assert!(!GroundByte(0x88).can_ignite());
        assert!(!GroundByte(0x05).can_ignite());
        assert!(is_wade_material(3) && is_wade_material(12) && !is_wade_material(8));
        assert!(javelin_refuses_material(5) && !javelin_refuses_material(8));
    }

    #[test]
    fn grid_fire_spreads_one_cell_per_interval_and_burns_out() {
        let mut g = meadow(9);
        let mut f = GridFire::new((4, 4), 0.0, 3.0, 0.5, 0.0, 0.0);
        // first tick at t=0.5 ignites the seed
        let e = f.tick(&mut g, 0.5);
        assert_eq!(e, vec![FireEvent::Ignited(4, 4)]);
        assert!(GroundByte(g.get(4, 4)).is_burning());
        assert_eq!(g.nav[(4 * 9 + 4) as usize], NAV_FIRE_MARK);
        // next tick before the interval: nothing
        assert!(f.tick(&mut g, 0.7).is_empty());
        // at +0.5 s: first neighbour in scan order dx=-1,dy=-1
        let e = f.tick(&mut g, 1.0);
        assert_eq!(e, vec![FireEvent::Ignited(3, 3)]);
        assert_eq!(f.burning_cells(), 2);
        // seed lifetime 3.0 s from t=0.5 -> burns out at 3.5
        let e = f.tick(&mut g, 3.5);
        assert!(e.contains(&FireEvent::BurntOut(4, 4)));
        let b = GroundByte(g.get(4, 4));
        assert!(b.is_burnt() && !b.is_burning());
        assert_eq!(g.nav[(4 * 9 + 4) as usize], 0);
        // burnt cells cannot re-ignite
        assert!(!b.can_ignite());
    }

    #[test]
    fn grid_fire_stops_at_non_flammable_and_finishes() {
        let mut g = meadow(3);
        // make only the centre flammable
        for y in 0..3 {
            for x in 0..3 {
                if (x, y) != (1, 1) {
                    g.set_material(x, y, 1);
                }
            }
        }
        let mut f = GridFire::new((1, 1), 0.0, 1.0, 0.25, 0.0, 0.0);
        let mut t = 0.0;
        let mut finished = false;
        for _ in 0..40 {
            t += 0.25;
            if f.tick(&mut g, t).contains(&FireEvent::Finished) {
                finished = true;
                break;
            }
        }
        assert!(finished);
        assert!(GroundByte(g.get(1, 1)).is_burnt());
    }

    #[test]
    fn level_overrides_win_when_nonzero() {
        let f = GridFire::new((0, 0), 0.0, 5.0, 1.0, 12.0, 0.0);
        assert_eq!(f.burn_time, 12.0);
        assert_eq!(f.spread_interval, 1.0);
    }

    #[test]
    fn start_cell_search_prefers_own_cell_then_scan_order() {
        let mut g = meadow(5);
        assert_eq!(GridFire::find_start_cell(&g, 2, 2), Some((2, 2)));
        g.set_state(2, 2, FIRE_BURNT);
        assert_eq!(GridFire::find_start_cell(&g, 2, 2), Some((1, 1)));
    }

    #[test]
    fn restore_marks_connected_flammable_cells() {
        let mut g = meadow(4);
        let n = restore_burnt(&mut g, (0, 0));
        assert_eq!(n, 16);
        assert!(GroundByte(g.get(3, 3)).is_burnt());
    }

    #[test]
    fn creature_burn_numbers() {
        assert_eq!(BURN_PAF_DAMAGE, 1000.0);
        assert_eq!(PAF_FLAGS_FIRE_CREATURE, 0x200200);
        assert_eq!(burn_paf_interval(BurnVictim::Scorpion, 0.0), 0.5);
        assert_eq!(burn_paf_interval(BurnVictim::Scorpion, 1.0), 1.5);
        assert!((burn_paf_interval(BurnVictim::Scolo, 1.0) - 0.5).abs() < 1e-6);
        assert!(raptor_burn_state_over(1.0, 8.1));
        assert!(!raptor_burn_state_over(1.0, 8.0));
        assert!(raptor_burn_state_over(0.0, 0.1));
    }

    #[test]
    fn jack_fire_proximity_is_5x5() {
        let mut g = meadow(9);
        g.set_state(6, 4, FIRE_BURNING);
        assert!(jack_near_fire(&g, 4, 4));
        assert!(!jack_near_fire(&g, 3, 4));
    }

    #[test]
    fn fire_zone_shapes() {
        let s = [FireZone::Sphere { c: [0.0, 0.0, 0.0], r: NET_FIRE_TIP_ZONE_RADIUS }];
        assert!(point_in_fire_zone(&s, [0.4, 0.0, 0.0]));
        assert!(!point_in_fire_zone(&s, [0.5, 0.0, 0.0]));
        let seg = [FireZone::Segment { base: [0.0; 3], axis: [10.0, 0.0, 0.0], r: 1.0 }];
        assert!(point_in_fire_zone(&seg, [5.0, 0.9, 0.0]));
        assert!(!point_in_fire_zone(&seg, [-0.1, 0.0, 0.0])); // no end caps
        assert!(!point_in_fire_zone(&seg, [5.0, 1.1, 0.0]));
    }

    #[test]
    fn ode_fire_timeline() {
        let n = OdeBurn::frames_to_spread();
        assert!((100..=101).contains(&n), "{n}");
        let mut b = OdeBurn::ignite(0.0);
        assert!(!b.frame(10.0));
        assert!(b.frame(10.5));
        assert!(PAF_FIRE_BIT & 0x0020_0200 != 0);
    }

    #[test]
    fn water_scale_fades_between_50_and_200() {
        assert_eq!(water_effect_scale(30.0, 0.0, 2.0), 2.0);
        assert!((water_effect_scale(125.0, 0.0, 2.0) - 1.0).abs() < 1e-6);
        assert_eq!(water_effect_scale(200.0, 0.0, 2.0), 0.0);
        assert_eq!(water_effect_scale(-500.0, 0.0, 2.0), 0.0);
    }

    #[test]
    fn torch_light_rule() {
        assert!(torch_lights(false, true, false));
        assert!(!torch_lights(false, false, false));
        assert_eq!(TORCH_DEFAULT_RADIUS, 0.2);
    }
}
