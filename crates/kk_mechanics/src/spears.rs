//! Jack's spears and thrown objects (ledger JACK_SPEARS S01-S16, I02/I03/I05/I06).
//! Evidence: `spec/evidence/S05.md` (main), `S06.md`, `S08.md`, `S09.md`, `S12.md`, `S16.md`, `I02.md`.
//!
//! Sources in KingKong8.exe (all `[C]` unless tagged):
//! * `H_ETAT_IA_lance@0x5ca1e0`: the throw state. Launch speed = 2.5 x the per-item range
//!   (20 for weapons 6/7/8, 15 grenade, 7.5 for 9/10), direction = (aim point - spawn point), aim point
//!   = camera position + camera forward x range; message 2000 carries velocity, gravity, scale.
//! * `Javelin_init@0x8783a0`: durability 6 (type 0) / 4 (type 1); `Javelin_launch@0x879190`: flight
//!   tick (ray sweep, material classes, stick = wear += 2, damage by squared distance travelled).
//! * `Javelin_waittaken@0x87d160`, `H_TRACK_Reflex@0x58de70`: recovery and slot rules.
//! * `Jack_DropHeldThrowable@0x68c870`, `Jack_ConsumeThrownWeapon@0x68db70`,
//!   `Jack_SpearWearStage@0x68a3c0`: drop, post-throw slot cleanup, wear stage.
//!
//! Engine integration of the flight itself is done by the engine's dynamics (`Dyn_SetSpeed`,
//! `Dyn_SetAcceleration`); the explicit Euler step in [`Spear::integrate`] is `[L]`.

use crate::weapons::{damage_at_distance_sq, def, WeaponId};

// ---------------------------------------------------------------- constants

/// Launch speed factor: `fn@0x0041c8e0(&vel, range * 2.5)` in `H_ETAT_IA_lance@0x5ca1e0` [C].
pub const LAUNCH_SPEED_FACTOR: f32 = 2.5;
/// Aim distance (also the "range" fed to the speed factor) for spears and the small throwable [C].
pub const AIM_RANGE_SPEAR: f32 = 20.0;
/// Weapon 5 (grenade) aim distance [C].
pub const AIM_RANGE_GRENADE: f32 = 15.0;
/// Weapons 9 and 10 aim distance [C]. (9 is the best bait candidate, 10 is the lever item.)
pub const AIM_RANGE_CARRIED: f32 = 7.5;
/// Gravity (z, m/s^2) written into the javelin by Jack's throw when the weapon is 6/7/8 [C]
/// (`fn@0x00440620(0,0,0xc1200000)` in `H_ETAT_IA_lance`).
pub const GRAVITY_JACK_SPEAR: f32 = -10.0;
/// Gravity for a grenade thrown by Jack (`0xc0a00000`) [C].
pub const GRAVITY_JACK_GRENADE: f32 = -5.0;
/// Default gravity: NPC throws, weapons 9/10, drop message 0x7d3, `Javelin_init` [C] (`0xc1a00000`).
pub const GRAVITY_DEFAULT: f32 = -20.0;
/// Durability budget (`Javelin_init@0x8783a0`: `this[0x8c] = 6` type 0, `4` type 1) [C].
pub const MAX_WEAR_DEVELOPED: i32 = 6;
pub const MAX_WEAR_BONE: i32 = 4;
/// Wear added by every embed (`Javelin_launch`: `this[0x86] += 2` for types 0 and 1) [C].
pub const WEAR_PER_EMBED: i32 = 2;
/// Fire lasts this long after ignition (`Javelin_launch`: `this[0x7a] = 10.0`, ticked in
/// `Javelin_GFX_fire1@0x87fa70`) [C].
pub const BURN_SECONDS: f32 = 10.0;
/// Damage added to the hit while the spear burns (`Javelin_launch`: `local_fc + 10`) [C].
pub const BURN_DAMAGE_BONUS: i32 = 10;
/// A stuck spear is placed `hit_point - axis * 0.6` (`Javelin_launch`) [C].
pub const EMBED_DEPTH: f32 = 0.6;
/// Rebound after a non-sticking collision: velocity x 0.1, reflected about the surface normal [C].
pub const REBOUND_SPEED_FACTOR: f32 = 0.1;
/// Falls this far (|z - z at launch|) from the launch height and the spear is destroyed [C]
/// (`Javelin_launch`/`Javelin_endofmove`: `fabs(dz) > 50.0`).
pub const FALL_KILL_DISTANCE: f32 = 50.0;
/// Resting spears farther than sqrt(225) = 15 m from Jack are destroyed after 3.0 s [C]
/// (`Javelin_waittaken`: `Obj_DistSq > 225.0`, timer `this[0x85] > 3.0`), except the newest one.
pub const DESPAWN_DISTANCE: f32 = 15.0;
pub const DESPAWN_DELAY: f32 = 3.0;
/// Number of spears alive in the world; the 33rd evicts the farthest (`Javelin_exec_addinworld`) [C].
pub const WORLD_CAP: usize = 32;
/// In water the spear loses speed: frame counter +2 per frame in water, -1 out; speed scale
/// `1 - (n-10)/10` for n in 10..20, stops at n >= 20 (`Javelin_endofmove`) [C].
pub const WATER_STOP_COUNT: i32 = 20;
/// Pickup radius around the spear (squared compared; `Javelin_waittaken`: `local_2c = 3.0`, +1.0 when
/// Jack faces the spear (dot > 0.9) and moves (|v| > 1)) [C for 3.0/1.0, L for the exact vector].
pub const PICKUP_RADIUS: f32 = 3.0;
pub const PICKUP_RADIUS_APPROACH_BONUS: f32 = 1.0;
/// Melee stab (`H_exec_test_ZDE_FIGHT@0x5915b0`): hit message damage 1 [C], cooldown 0.4 s
/// (`fn@0x0043c690(Jack+0x3038, 0x3ecccccd)`) [C], reach 5.0 (`fn@0x0042d400(..., 0x40a00000, ...)`) [C].
pub const MELEE_DAMAGE: i32 = 1;
pub const MELEE_COOLDOWN: f32 = 0.4;
pub const MELEE_REACH: f32 = 5.0;
/// Hit flag in the melee message: 0x10 normal, 0x40 when the held item burns [C].
pub const MELEE_FLAG_NORMAL: u32 = 0x10;
pub const MELEE_FLAG_BURNING: u32 = 0x40;
/// Item-slot ids [C]: 10 is the lever/crank carried item (`G+0x4588`), not a weapon.
pub const ITEM_LEVER: u8 = 10;

// ---------------------------------------------------------------- kinds

/// Javelin model type (`Javelin.this[1]`) and the Univers weapon row it uses (6/7/8) [C].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpearKind {
    /// type 0 = weapon 6 "Javelin": the developed spear (ledger S01) [L on the naming].
    Developed = 0,
    /// type 1 = weapon 7: the bone spear (S02) [L on the naming].
    Bone = 1,
    /// type 2 = weapon 8: small throwable stack (damage 3/2/1, no durability budget).
    Small = 2,
}

impl SpearKind {
    pub fn weapon_id(self) -> WeaponId {
        match self {
            SpearKind::Developed => WeaponId::Javelin,
            SpearKind::Bone => WeaponId::BoneSpear,
            SpearKind::Small => WeaponId::SmallThrowable,
        }
    }
    pub fn from_weapon_id(id: u8) -> Option<SpearKind> {
        match id {
            6 => Some(SpearKind::Developed),
            7 => Some(SpearKind::Bone),
            8 => Some(SpearKind::Small),
            _ => None,
        }
    }
    /// `Javelin_init`: 6 / 4; type 2 never sets the field (0, `[L]`: model default) so it never embeds.
    pub fn max_wear(self) -> i32 {
        match self {
            SpearKind::Developed => MAX_WEAR_DEVELOPED,
            SpearKind::Bone => MAX_WEAR_BONE,
            SpearKind::Small => 0,
        }
    }
    /// Number of embeds before the spear is spent.
    pub fn embeds_available(self) -> i32 {
        (self.max_wear() + WEAR_PER_EMBED - 1) / WEAR_PER_EMBED
    }
}

// ---------------------------------------------------------------- throw

pub type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn dist_sq(a: V3, b: V3) -> f32 {
    let d = sub(a, b);
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// Aim point of a player throw: `camera_pos + camera_forward * range` (`H_ETAT_IA_lance`, branch
/// `*local_40 != 0`, `fn@0x00416310(camera)*range + fn@0x00414720(camera)`) [C].
pub fn aim_point(cam_pos: V3, cam_forward: V3, range: f32) -> V3 {
    [cam_pos[0] + cam_forward[0] * range, cam_pos[1] + cam_forward[1] * range, cam_pos[2] + cam_forward[2] * range]
}

/// Launch velocity: `normalize(aim - spawn) * range * 2.5` (`fn@0x0041c8e0(&vel, range*2.5)`) [C].
/// The vector is NOT lofted: gravity then bends the path (no angle compensation for the player) [C].
pub fn launch_velocity(spawn: V3, aim: V3, range: f32) -> V3 {
    let d = sub(aim, spawn);
    let l = len(d);
    let s = if l > 0.0 { range * LAUNCH_SPEED_FACTOR / l } else { 0.0 };
    [d[0] * s, d[1] * s, d[2] * s]
}

/// Gravity z used for a throw by Jack (`H_ETAT_IA_lance`) [C].
pub fn jack_throw_gravity(item: u8) -> f32 {
    match item {
        6 | 7 | 8 => GRAVITY_JACK_SPEAR,
        5 => GRAVITY_JACK_GRENADE,
        _ => GRAVITY_DEFAULT,
    }
}

/// Aim range for an item id (`H_ETAT_IA_lance` switch: 5 -> 15, 6/7/8 -> 20, 9/10 -> 7.5) [C].
pub fn aim_range(item: u8) -> f32 {
    match item {
        5 => AIM_RANGE_GRENADE,
        6 | 7 | 8 => AIM_RANGE_SPEAR,
        _ => AIM_RANGE_CARRIED,
    }
}

// ---------------------------------------------------------------- collision classes

/// Result of the flight-tick surface test (`Javelin_launch` switch on `Col_LastHitMaterial`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitClass {
    /// materials 0,1,2,6: world surface the spear can stick into (`local_30 = 1`)
    Stick,
    /// materials 3,7: GFX impact only, no stick
    Glance,
    /// material 4: pass-through, extinguishes fire (`this[0x3e] = 0`); the ray is re-cast (max 2 passes) [L: water]
    Water,
    /// material 5, or material 0/>7 with a collision bone index: a creature body part (stick + damage)
    Creature,
}

/// `Javelin_launch@0x879190`: `local_4c = Col_LastHitMaterial()`; `if (local_4c == 0 || 7 < local_4c)`
/// then it becomes 1, or 5 when the hit has a bone index (`fn@0x00431990 != -1`) [C].
pub fn classify_hit(material: u32, has_bone: bool) -> HitClass {
    let mut m = material;
    if m == 0 || m > 7 {
        m = if has_bone { 5 } else { 1 };
    }
    match m {
        0 | 1 | 2 | 6 => HitClass::Stick,
        3 | 7 => HitClass::Glance,
        4 => HitClass::Water,
        _ => HitClass::Creature,
    }
}

// ---------------------------------------------------------------- state machine

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpearState {
    /// in Jack's hand (slot A)
    Held,
    /// in flight (`Javelin.state == 1`)
    Thrown,
    /// stuck in the world (`target = None`) or in a creature (`target = Some(id)`), `Javelin_Plug` follows it
    Embedded { target: Option<u32> },
    /// lying free (dropped, bounced or settled), pickable
    Dropped,
    /// durability spent and destroyed (`Javelin_waitdestroy` spawns the broken pieces)
    Broken,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spear {
    pub kind: SpearKind,
    pub state: SpearState,
    /// `Javelin.this[0x86]` / `G+0x1d78` while carried
    pub wear: i32,
    /// `Javelin.this[0x3e]`
    pub flaming: bool,
    /// seconds of fire left (`this[0x7a]`)
    pub burn_left: f32,
    /// bait on the spear (S14/S15). No code found: [G] flag only, never set by recovered logic.
    pub bait: bool,
    pub pos: V3,
    pub vel: V3,
    pub gravity_z: f32,
    /// `Javelin.this[0x72..]`: position at creation, distances for damage are measured from it
    pub origin: V3,
}

/// What a flight-tick collision did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImpactResult {
    /// embedded; damage applied to the target (if a creature)
    Embedded { damage: i32 },
    /// could not embed (spent or glancing): half damage message, spear rebounds
    Glanced { damage: i32 },
    /// passed through water; fire put out
    PassedWater,
}

impl Spear {
    pub fn held(kind: SpearKind, wear: i32, flaming: bool) -> Spear {
        Spear { kind, state: SpearState::Held, wear, flaming, burn_left: if flaming { BURN_SECONDS } else { 0.0 }, bait: false, pos: [0.0; 3], vel: [0.0; 3], gravity_z: GRAVITY_JACK_SPEAR, origin: [0.0; 3] }
    }

    /// Throw from `spawn` toward the aim point of the camera (`H_ETAT_IA_lance` + `Javelin_wait` msg 2000).
    /// `G+0x1d78` (wear) and the fire flag travel with the object; Jack's copy is cleared by the caller.
    pub fn launch(&mut self, spawn: V3, cam_pos: V3, cam_forward: V3) {
        let id = self.kind.weapon_id() as u8;
        let range = aim_range(id);
        let aim = aim_point(cam_pos, cam_forward, range);
        self.vel = launch_velocity(spawn, aim, range);
        self.gravity_z = jack_throw_gravity(id);
        self.pos = spawn;
        self.origin = spawn;
        self.state = SpearState::Thrown;
    }

    /// One flight step, semi-implicit Euler (`v += g dt; x += v dt`) [L]: the engine dynamics own the real
    /// integration; recovered facts are the initial velocity, the gravity vector and drag scale (1,1,1).
    pub fn integrate(&mut self, dt: f32) {
        if let SpearState::Thrown = self.state {
            self.vel[2] += self.gravity_z * dt;
            for i in 0..3 {
                self.pos[i] += self.vel[i] * dt;
            }
            if (self.pos[2] - self.origin[2]).abs() > FALL_KILL_DISTANCE {
                self.state = SpearState::Broken; // destroyed (fn@0x00412440)
            }
            if self.flaming {
                self.burn_left -= dt;
                if self.burn_left <= 0.0 {
                    self.flaming = false;
                    self.burn_left = 0.0;
                }
            }
        }
    }

    /// `Javelin_GFX_fire1`: ignite (message 0x7d2). Resets the timer to 10 s [C].
    pub fn ignite(&mut self) {
        self.flaming = true;
        self.burn_left = BURN_SECONDS;
    }

    /// Damage of a hit at the current position: Univers row 6/7/8 band by SQUARED distance from the
    /// launch point (the same inline test as `Weapon_DamageAtDistance`), +10 while burning [C]; the
    /// band value is read through `_ftol` [L: the decompiler drops the table operand].
    pub fn hit_damage(&self) -> i32 {
        let row = def(self.kind.weapon_id());
        let mut d = damage_at_distance_sq(row, dist_sq(self.pos, self.origin), false);
        if self.flaming {
            d += BURN_DAMAGE_BONUS;
        }
        d
    }

    /// Can the spear still embed? `wear < max` (`Javelin_launch`) [C].
    pub fn can_embed(&self) -> bool {
        self.wear < self.kind.max_wear()
    }

    /// Resolve a collision (`Javelin_launch@0x879190`). `target` is the creature id if any.
    /// `sub_object_excluded` mirrors the `DAT_00b9a088` model test (`local_2c == 0`).
    pub fn impact(&mut self, class: HitClass, target: Option<u32>, sub_object_excluded: bool) -> ImpactResult {
        match class {
            HitClass::Water => {
                self.flaming = false;
                self.burn_left = 0.0;
                ImpactResult::PassedWater
            }
            HitClass::Glance => {
                let d = self.hit_damage();
                self.rebound();
                ImpactResult::Glanced { damage: d }
            }
            HitClass::Stick | HitClass::Creature => {
                if self.can_embed() && !sub_object_excluded {
                    if matches!(self.kind, SpearKind::Developed | SpearKind::Bone) {
                        self.wear += WEAR_PER_EMBED;
                    }
                    let d = self.hit_damage();
                    self.state = SpearState::Embedded { target };
                    self.vel = [0.0; 3];
                    self.gravity_z = 0.0;
                    ImpactResult::Embedded { damage: d }
                } else {
                    // `local_fc = local_fc / 2` then the message is sent, the spear bounces
                    let d = self.hit_damage() / 2;
                    self.rebound();
                    ImpactResult::Glanced { damage: d }
                }
            }
        }
    }

    fn rebound(&mut self) {
        // reflect about an unknown normal is the engine's job; keep 10 % speed and default gravity [C]
        for i in 0..3 {
            self.vel[i] *= REBOUND_SPEED_FACTOR;
        }
        self.gravity_z = GRAVITY_DEFAULT;
        self.state = SpearState::Dropped;
    }

    /// `Javelin_waitdestroy@0x87f600`: a spent spear (wear >= max) of type 0/1 leaves broken pieces.
    pub fn is_spent(&self) -> bool {
        self.wear >= self.kind.max_wear() && matches!(self.kind, SpearKind::Developed | SpearKind::Bone)
    }

    /// `Jack_SpearWearStage@0x68a3c0` (used to pick the dropped model): 0 fresh, 1 spent, 2 beyond base+3.
    pub fn wear_stage(kind: SpearKind, wear: i32) -> u8 {
        let base = match kind {
            SpearKind::Developed => 6,
            SpearKind::Bone => 4,
            SpearKind::Small => 0, // `local_c` unset in the original (garbage); [G]
        };
        if wear < base + 3 {
            if wear < base {
                0
            } else {
                1
            }
        } else {
            2
        }
    }

    /// Pickup test (`Javelin_waittaken`): within `PICKUP_RADIUS`(+1 when approaching), not in water,
    /// not the "bone spear while holding the developed spear" case, and not a type Jack already holds [C].
    pub fn can_be_picked_up(&self, dist: f32, approaching: bool, in_water: bool, held_weapon: u8) -> bool {
        if matches!(self.state, SpearState::Thrown | SpearState::Broken | SpearState::Held) || in_water {
            return false;
        }
        let id = self.kind.weapon_id() as u8;
        if id == 7 && held_weapon == 6 {
            return false;
        }
        if held_weapon == id {
            return false;
        }
        let r = PICKUP_RADIUS + if approaching { PICKUP_RADIUS_APPROACH_BONUS } else { 0.0 };
        dist <= r
    }
}

/// Fire-source test result for a resting spear: ignites when its tip is inside a fire zone, tested
/// once per second (`Javelin_waittaken`: timer `this[0x84]`, reset 1.0, `Fire_PointInFireZone@0x6e3210`) [C].
pub const FIRE_TEST_INTERVAL: f32 = 1.0;

/// Melee: damage message `1` every 0.4 s; flag 0x40 when burning (`H_exec_test_ZDE_FIGHT`) [C].
pub fn melee_flag(burning: bool) -> u32 {
    if burning {
        MELEE_FLAG_BURNING
    } else {
        MELEE_FLAG_NORMAL
    }
}

// ---------------------------------------------------------------- Jack's item slots

/// Jack's three weapon slots (`G+0x3344/0x3390/0x33dc` + 4p, p = Jack) and the throwable bookkeeping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Slots {
    /// slot 0 = held weapon (0 = none)
    pub held: u8,
    /// slot 1 = stored weapon
    pub stored: u8,
    /// slot 2 = only used to park the lever item (10)
    pub third: u8,
    /// `G+0x1d78`
    pub wear: i32,
    /// `G+0x4520+4p`
    pub burning: bool,
    /// `G+0x366c+4p` (weapon 8 stack count)
    pub stack: i32,
}

impl Slots {
    /// `Jack_IsThrowableHeld@0x68b8b0`: held in 6..=8 [C]
    pub fn throwable_held(&self) -> bool {
        (6..=8).contains(&self.held)
    }
    /// `Jack_IsNonFirearmHeld@0x68b930`: 5..=10 [C]. While true the gun controls are off (I03).
    pub fn non_firearm_held(&self) -> bool {
        self.throwable_held() || matches!(self.held, 5 | 9 | 10)
    }
    /// A firearm can be fired only when the held id is 1..=4 [C] (`TrigTest_Arme` class 200).
    pub fn can_fire_gun(&self) -> bool {
        (1..=4).contains(&self.held)
    }
    /// `TrigTest_Arme@0x469980` case 100: any slot holding 5..=8
    pub fn carries_throwable(&self) -> bool {
        [self.held, self.stored, self.third].iter().any(|&w| (5..=8).contains(&w))
    }

    /// Pickup of an item with id `id` (6/7/8 spear family) — `H_TRACK_Reflex@0x58de70` lines 541-640.
    /// Returns false when nothing changed (same id already held: only the stack count is refreshed for 8).
    /// `carried_wear` is the wear counter of the picked spear (`G+0x4578`), `fire` the burn flag (`G+0x4574`).
    pub fn pickup(&mut self, id: u8, carried_wear: i32, fire: bool, count: i32) -> bool {
        if id == self.held {
            if id == 8 {
                self.stack = count;
            }
            return false;
        }
        if self.held == ITEM_LEVER {
            self.third = ITEM_LEVER;
            self.held = 0;
        }
        if self.stored == ITEM_LEVER {
            self.third = ITEM_LEVER;
            self.stored = 0;
        }
        self.drop_held();
        if self.stored == ITEM_LEVER {
            self.third = ITEM_LEVER;
            self.stored = 0;
        }
        if self.stored == 0 {
            self.stored = self.held;
        }
        self.held = id;
        if (6..=8).contains(&id) {
            self.burning = fire;
        }
        self.stack = count;
        self.wear = carried_wear;
        true
    }

    /// `Jack_DropHeldThrowable@0x68c870`: slot0 := slot1; slot1 := 0; the lever moves from slot 2 to slot 1.
    /// Returns the dropped item (id, wear, burning, count) if a throwable was held.
    pub fn drop_held(&mut self) -> Option<(u8, i32, bool, i32)> {
        if !self.throwable_held() {
            return None;
        }
        let out = (self.held, self.wear, self.burning, if self.held == 8 { self.stack } else { 1 });
        self.held = self.stored;
        self.stored = 0;
        if self.third == ITEM_LEVER {
            self.stored = ITEM_LEVER;
            self.third = 0;
        }
        self.burning = false;
        Some(out)
    }

    /// `Jack_ConsumeThrownWeapon@0x68db70`, run after the throw: weapon 8 loses one from its stack and
    /// stays in hand while the stack is non-empty (fire cleared); otherwise the slot is vacated and the
    /// stored weapon (or the lever parked in slot 2) comes up. `G+0x1d78` and the fire flag are cleared
    /// by `H_ETAT_IA_lance` itself [C].
    pub fn consume_after_throw(&mut self) {
        let mut vacate = true;
        if self.held == 8 {
            vacate = false;
            self.stack -= 1;
            if self.stack == 0 {
                vacate = true;
            } else {
                self.burning = false;
            }
        }
        if vacate {
            if self.third == ITEM_LEVER {
                self.held = ITEM_LEVER;
                self.third = 0;
            } else if self.stored == 0 {
                self.held = 0;
            } else {
                self.held = self.stored;
                self.stored = 0;
            }
        }
        self.wear = 0;
        self.burning = false;
    }
}

/// Spear world list: at most 32; adding one more evicts the farthest from Jack among those not in
/// state 1 (flying) (`Javelin_exec_addinworld@0x87f110`) [C].
pub fn eviction_index(distances_sq: &[f32], flying: &[bool]) -> Option<usize> {
    if distances_sq.len() < WORLD_CAP {
        return None;
    }
    let mut best = 0usize;
    let mut best_d = 0.0f32;
    for i in 0..WORLD_CAP.min(distances_sq.len()) {
        if !flying[i] && distances_sq[i] > best_d {
            best = i;
            best_d = distances_sq[i];
        }
    }
    Some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_speed_is_two_and_a_half_times_range() {
        // H_ETAT_IA_lance: Vec3 set-length(range * 2.5), spear range 20 -> 50 m/s
        let v = launch_velocity([0.0; 3], [20.0, 0.0, 0.0], AIM_RANGE_SPEAR);
        assert!((v[0] - 50.0).abs() < 1e-4 && v[1] == 0.0 && v[2] == 0.0);
        let g = launch_velocity([0.0; 3], [0.0, 15.0, 0.0], AIM_RANGE_GRENADE);
        assert!((g[1] - 37.5).abs() < 1e-4);
        let b = launch_velocity([0.0; 3], [0.0, 0.0, 1.0], AIM_RANGE_CARRIED);
        assert!((b[2] - 18.75).abs() < 1e-4);
    }

    #[test]
    fn gravity_values_by_item() {
        assert_eq!(jack_throw_gravity(6), -10.0);
        assert_eq!(jack_throw_gravity(8), -10.0);
        assert_eq!(jack_throw_gravity(5), -5.0);
        assert_eq!(jack_throw_gravity(9), -20.0);
    }

    #[test]
    fn spear_rows_in_flight_use_weapon_table() {
        // dev spear: 11 under 10 m, 5 beyond; bone: 7 / 3; small: 3 / 2 / 1 (uni_init statements 809-832)
        let mut s = Spear::held(SpearKind::Developed, 0, false);
        s.launch([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]);
        s.pos = [4.9, 0.0, 0.0];
        assert_eq!(s.hit_damage(), 11);
        s.pos = [9.9, 0.0, 0.0];
        assert_eq!(s.hit_damage(), 11);
        s.pos = [10.0, 0.0, 0.0];
        assert_eq!(s.hit_damage(), 5);
        s.ignite();
        assert_eq!(s.hit_damage(), 15);
        let mut b = Spear::held(SpearKind::Bone, 0, false);
        b.launch([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]);
        b.pos = [3.0, 0.0, 0.0];
        assert_eq!(b.hit_damage(), 7);
        b.pos = [12.0, 0.0, 0.0];
        assert_eq!(b.hit_damage(), 3);
        let mut m = Spear::held(SpearKind::Small, 0, false);
        m.launch([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]);
        m.pos = [6.0, 0.0, 0.0];
        assert_eq!(m.hit_damage(), 2);
    }

    #[test]
    fn durability_is_three_embeds_for_developed_two_for_bone() {
        assert_eq!(SpearKind::Developed.embeds_available(), 3);
        assert_eq!(SpearKind::Bone.embeds_available(), 2);
        let mut s = Spear::held(SpearKind::Developed, 0, false);
        let mut embeds = 0;
        for _ in 0..5 {
            s.launch([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]);
            s.pos = [2.0, 0.0, 0.0];
            if let ImpactResult::Embedded { .. } = s.impact(HitClass::Creature, Some(1), false) {
                embeds += 1;
            }
        }
        assert_eq!(embeds, 3);
        assert_eq!(s.wear, 6);
        assert!(s.is_spent());
        // a spent spear only glances for half damage (11 / 2 = 5)
        s.launch([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]);
        s.pos = [2.0, 0.0, 0.0];
        assert_eq!(s.impact(HitClass::Creature, Some(1), false), ImpactResult::Glanced { damage: 5 });
        // type 2 never embeds
        let mut m = Spear::held(SpearKind::Small, 0, false);
        m.launch([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]);
        assert!(matches!(m.impact(HitClass::Stick, None, false), ImpactResult::Glanced { .. }));
    }

    #[test]
    fn material_classes() {
        assert_eq!(classify_hit(1, false), HitClass::Stick);
        assert_eq!(classify_hit(6, false), HitClass::Stick);
        assert_eq!(classify_hit(3, false), HitClass::Glance);
        assert_eq!(classify_hit(7, false), HitClass::Glance);
        assert_eq!(classify_hit(4, false), HitClass::Water);
        assert_eq!(classify_hit(5, false), HitClass::Creature);
        assert_eq!(classify_hit(0, true), HitClass::Creature);
        assert_eq!(classify_hit(9, false), HitClass::Stick);
    }

    #[test]
    fn water_puts_the_fire_out_and_burn_times_out() {
        let mut s = Spear::held(SpearKind::Developed, 0, true);
        s.launch([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]);
        assert_eq!(s.burn_left, BURN_SECONDS);
        s.impact(HitClass::Water, None, false);
        assert!(!s.flaming);
        let mut t = Spear::held(SpearKind::Bone, 0, true);
        t.launch([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]);
        t.gravity_z = 0.0;
        t.vel = [0.0; 3];
        for _ in 0..101 {
            t.integrate(0.1);
        }
        assert!(!t.flaming);
    }

    #[test]
    fn flight_follows_gravity_minus_ten() {
        let mut s = Spear::held(SpearKind::Developed, 0, false);
        s.launch([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]);
        assert_eq!(s.vel, [50.0, 0.0, 0.0]);
        s.integrate(1.0);
        assert_eq!(s.vel[2], -10.0);
        assert_eq!(s.pos[0], 50.0);
        // |dz| > 50 destroys it
        let mut f = Spear::held(SpearKind::Developed, 0, false);
        f.launch([0.0; 3], [0.0; 3], [0.0, 0.0, 1.0]);
        f.vel = [0.0, 0.0, -60.0];
        f.integrate(1.0);
        assert_eq!(f.state, SpearState::Broken);
    }

    #[test]
    fn wear_stage_matches_original_thresholds() {
        assert_eq!(Spear::wear_stage(SpearKind::Developed, 4), 0);
        assert_eq!(Spear::wear_stage(SpearKind::Developed, 6), 1);
        assert_eq!(Spear::wear_stage(SpearKind::Bone, 3), 0);
        assert_eq!(Spear::wear_stage(SpearKind::Bone, 4), 1);
        assert_eq!(Spear::wear_stage(SpearKind::Bone, 7), 2);
    }

    #[test]
    fn slot_rules_pickup_throw_drop() {
        // empty-handed with a Colt stored: pick the developed spear
        let mut s = Slots { held: 1, ..Default::default() };
        assert!(s.can_fire_gun());
        assert!(s.pickup(6, 0, false, 1));
        assert_eq!((s.held, s.stored), (6, 1));
        assert!(!s.can_fire_gun() && s.non_firearm_held() && s.throwable_held());
        // throw: gun comes back
        s.consume_after_throw();
        assert_eq!((s.held, s.stored), (1, 0));
        // drop while holding bone spear, with the lever in slot 2
        let mut d = Slots { held: 7, stored: 3, third: 10, wear: 2, ..Default::default() };
        let dropped = d.drop_held().unwrap();
        assert_eq!(dropped, (7, 2, false, 1));
        assert_eq!((d.held, d.stored, d.third), (3, 10, 0));
        // weapon 8 is a stack
        let mut st = Slots { held: 8, stack: 2, burning: true, ..Default::default() };
        st.consume_after_throw();
        assert_eq!((st.held, st.stack), (8, 1));
        st.consume_after_throw();
        assert_eq!(st.held, 0);
        // same id already held: refused
        let mut h = Slots { held: 6, ..Default::default() };
        assert!(!h.pickup(6, 0, false, 1));
    }

    #[test]
    fn pickup_conditions() {
        let mut s = Spear::held(SpearKind::Bone, 2, false);
        s.state = SpearState::Dropped;
        assert!(s.can_be_picked_up(3.0, false, false, 0));
        assert!(!s.can_be_picked_up(3.5, false, false, 0));
        assert!(s.can_be_picked_up(4.0, true, false, 0));
        assert!(!s.can_be_picked_up(1.0, false, true, 0)); // water
        assert!(!s.can_be_picked_up(1.0, false, false, 6)); // developed spear in hand
        assert!(!s.can_be_picked_up(1.0, false, false, 7)); // same type
    }

    #[test]
    fn world_cap_evicts_farthest_non_flying() {
        let d: Vec<f32> = (0..32).map(|i| i as f32).collect();
        let mut fl = vec![false; 32];
        assert_eq!(eviction_index(&d, &fl), Some(31));
        fl[31] = true;
        assert_eq!(eviction_index(&d, &fl), Some(30));
        assert_eq!(eviction_index(&d[..31], &fl[..31]), None);
    }

    #[test]
    fn melee_constants() {
        assert_eq!(melee_flag(true), 0x40);
        assert_eq!(melee_flag(false), 0x10);
        assert_eq!(MELEE_DAMAGE, 1);
    }
}
