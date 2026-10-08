//! Jack's firearms (ledger G02–G06, G09–G13, I01, I07, I12). Verified against the KB 2026-10-07,
//! see `spec/evidence/G02.md, G03.md, G10.md, G11.md, I07.md`.
//!
//! Sources in KingKong8.exe:
//! * `GG_Exec_AppendWeaponName@0x696240`: weapon ids [C]
//! * `H_callback_tir@0x5bbd10`: fire. `switch(G+0x3344+4p)` weapon 1..4; Colt/Tommy/Sniper use the
//!   hitscan `fn@0x00540fd0@0x540fd0` (ray length = `G+0x465c+4w`, cast by `fn@0x0042d400` mode 0xb);
//!   the shotgun casts 25 rays itself (angles in radians, see [`shotgun_pattern`]) [C]
//! * `fn@0x0053a370@0x53a370` (`Weapon_DamageAtDistance`): damage band by SQUARED distance, 1000
//!   with cheat flag `G+0xbf0` bit 1, float table value converted to int by `_ftol` [C]
//! * `H_exec_loading_weapon@0x5ccc70`: reload moves min(clip − mag, min(reserve, clip)); the
//!   shotgun offers only 1 round per reload cycle [C]
//! * `Munition_init@0x6c45a0`: the box holds `count` pickups, count clamped to 10 [C];
//!   `Munition_wait@0x6c4000` consumes one per pickup; `H_TRACK_Reflex@0x58de70` adds to reserve [C]
//! * numbers: Univers init `uni_init.ofc` statements 769–832 (`code/ova/uni_init_all.txt`; ova
//!   offsets + 4 = exe offsets): `G+0x4758+4w` clip, `G+0x465c+4w` range, `G+0x46b0+4w` R1,
//!   `G+0x4704+4w` R2, `G+0x4804+12w+4b` damage [C]

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeaponId {
    Colt = 1,
    TommyGun = 2,
    Shotgun = 3,
    SniperRifle = 4,
    Grenade = 5,
    Javelin = 6,
    /// Univers weapon row 7 (bone spear, javelin type 1) [C: Javelin_launch@0x879190 `local_48`]
    BoneSpear = 7,
    /// Univers weapon row 8 (small throwable stack, javelin type 2) [C]
    SmallThrowable = 8,
}

impl WeaponId {
    /// `TrigTest_Arme@0x469980`: ids 1–4 are firearms, 5–8 throwables [C].
    pub fn is_firearm(self) -> bool {
        (self as u32) <= 4
    }
}

/// One row of the Univers weapon table [C].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponDef {
    pub id: WeaponId,
    pub name: &'static str,
    /// rounds per magazine
    pub clip: u32,
    /// hitscan ray length, metres
    pub range: f32,
    /// damage band radii, metres
    pub r1: f32,
    pub r2: f32,
    /// damage for d < R1 / d < R2 / beyond
    pub damage: [f32; 3],
    /// Value `H_callback_tir` stores at Jack `+0x3bc8` and `+0x3bcc` after every shot
    /// (`local_46c[0xef2]=[0xef3]`), seconds. Value is [C]; that it is the fire cooldown is [L]
    /// (no reader of those slots exists in the decompiled code). 0 = none stored (throwables).
    pub shot_timer: f32,
    /// Auto-fire interval: Tommy fires shot n+1 when `n * 0.1 <= elapsed` (`local_46c[0x408]*0.1`
    /// vs `[0x409]`) [C]. 0 = not an auto-fire weapon.
    pub auto_interval: f32,
    /// pellets per shot
    pub pellets: u32,
    /// "pointblank" column of the table (meaning [L]: melee/point-blank variant index)
    pub pointblank: u32,
}

pub const WEAPONS: [WeaponDef; 8] = [
    WeaponDef { id: WeaponId::Colt, name: "Colt", clip: 8, range: 50.0, r1: 5.0, r2: 25.0, damage: [8.0, 4.0, 2.0], shot_timer: 0.4, auto_interval: 0.0, pellets: 1, pointblank: 1 },
    WeaponDef { id: WeaponId::TommyGun, name: "TommyGun", clip: 50, range: 80.0, r1: 10.0, r2: 10.0, damage: [2.0, 1.0, 1.0], shot_timer: 0.2, auto_interval: 0.1, pellets: 1, pointblank: 2 },
    WeaponDef { id: WeaponId::Shotgun, name: "ShootGun", clip: 5, range: 25.0, r1: 5.0, r2: 10.0, damage: [20.0, 10.0, 5.0], shot_timer: 0.5, auto_interval: 0.0, pellets: 25, pointblank: 1 },
    WeaponDef { id: WeaponId::SniperRifle, name: "SniperRiffle", clip: 5, range: 250.0, r1: 5.0, r2: 50.0, damage: [15.0, 10.0, 10.0], shot_timer: 0.4, auto_interval: 0.0, pellets: 1, pointblank: 0 },
    WeaponDef { id: WeaponId::Grenade, name: "Grenade", clip: 1, range: 15.0, r1: 2.5, r2: 5.0, damage: [25.0, 2.0, 2.0], shot_timer: 0.0, auto_interval: 0.0, pellets: 1, pointblank: 0 },
    WeaponDef { id: WeaponId::Javelin, name: "Javelin", clip: 1, range: 20.0, r1: 5.0, r2: 10.0, damage: [11.0, 11.0, 5.0], shot_timer: 0.0, auto_interval: 0.0, pellets: 1, pointblank: 0 },
    WeaponDef { id: WeaponId::BoneSpear, name: "BoneSpear", clip: 1, range: 20.0, r1: 5.0, r2: 10.0, damage: [7.0, 7.0, 3.0], shot_timer: 0.0, auto_interval: 0.0, pellets: 1, pointblank: 0 },
    WeaponDef { id: WeaponId::SmallThrowable, name: "SmallThrowable", clip: 1, range: 20.0, r1: 5.0, r2: 10.0, damage: [3.0, 2.0, 1.0], shot_timer: 0.0, auto_interval: 0.0, pellets: 1, pointblank: 0 },
];

pub fn def(id: WeaponId) -> &'static WeaponDef {
    &WEAPONS[id as usize - 1]
}

/// `fn@0x0053a370(weapon, dist_sq)`: damage for a hit whose SQUARED distance is `dist_sq` [C].
/// The caller passes `fn@0x004150e0` = squared distance between the shooter's and the target
/// object's positions (not the ray length). Near iff `dist_sq < R1²`, mid iff `dist_sq < R2²`,
/// else far (asm: `fcomp; test ah,0x41; jnz` = jump to the next band when `R² <= dist_sq`).
/// The squares are formed in x87 extended precision, so f64 is used here. With the cheat flag
/// (`G+0xbf0` bit 1) it returns 1000. The table float is converted by `_ftol` (truncation).
pub fn damage_at_distance_sq(w: &WeaponDef, dist_sq: f32, one_hit_cheat: bool) -> i32 {
    if one_hit_cheat {
        return 1000;
    }
    let d2 = dist_sq as f64;
    let (r1, r2) = (w.r1 as f64, w.r2 as f64);
    let v = if d2 < r1 * r1 {
        w.damage[0]
    } else if d2 < r2 * r2 {
        w.damage[1]
    } else {
        w.damage[2]
    };
    v as i32
}

/// Convenience wrapper taking a distance in metres (squares it, as the callers' `fn@0x004150e0` does).
pub fn damage_at_distance(w: &WeaponDef, dist: f32, one_hit_cheat: bool) -> f32 {
    damage_at_distance_sq(w, dist * dist, one_hit_cheat) as f32
}

/// 10° in radians as stored in `H_callback_tir` (`-0.17453292`) [C].
pub const SHOTGUN_HALF_SPREAD: f32 = 0.17453292;
/// 5° step (`0.08726646`) [C].
pub const SHOTGUN_STEP: f32 = 0.08726646;
/// Jitter half-range in RADIANS: `Rand_Range(0.05, -0.05)` (`fn@0x0041c450`, constants 0x3d4ccccd /
/// 0xbd4ccccd) is ADDED TO THE ANGLE, not to a direction-vector component [C].
pub const SHOTGUN_JITTER: f32 = 0.05;
/// Pellet index that gets no jitter (`local_1f4 != 0xc`, the centre of the grid) [C].
pub const SHOTGUN_CENTRE_PELLET: usize = 12;

/// Shotgun pellet angles from `H_callback_tir` case 3 (`0x5bdXXX..0x5bec..`) [C]: 25 iterations;
/// `a` (inner, `local_200`) runs -10°..+10° in 5° steps with f32 accumulation, wrapping when
/// `0.17453292 < a`, then `b` (outer, `local_1e8`) steps by 5°. Each of `a`, `b` gets
/// `rand(-0.05, 0.05)` radians except pellet 12. Which of the two is yaw vs pitch is [L] (they
/// rotate the aim vector about two perpendicular axes via `fn@0x0041d730`). `rand(lo, hi)` must
/// return a value in `[lo, hi]`.
pub fn shotgun_pattern(mut rand: impl FnMut(f32, f32) -> f32) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(25);
    let mut a = -SHOTGUN_HALF_SPREAD;
    let mut b = -SHOTGUN_HALF_SPREAD;
    for i in 0..25 {
        let (mut ja, mut jb) = (a, b);
        if i != SHOTGUN_CENTRE_PELLET {
            ja = rand(SHOTGUN_JITTER, -SHOTGUN_JITTER) + a;
            jb = rand(SHOTGUN_JITTER, -SHOTGUN_JITTER) + b;
        }
        out.push((ja, jb));
        a += SHOTGUN_STEP;
        if SHOTGUN_HALF_SPREAD < a {
            a = -SHOTGUN_HALF_SPREAD;
            b += SHOTGUN_STEP;
        }
    }
    out
}

/// Ammunition state of one weapon slot (`G+0x342c+w*0x48+4p` mag, `G+0x39d4+..` reserve,
/// `G+0x3f80+..` reserve max) [C layout].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ammo {
    pub mag: u32,
    pub reserve: u32,
    pub reserve_max: u32,
}

impl Ammo {
    /// Fire one round if the magazine has one (G04, G05).
    pub fn fire(&mut self) -> bool {
        if self.mag == 0 {
            return false;
        }
        self.mag -= 1;
        true
    }
    /// `H_exec_loading_weapon` (end of the reload animation) [C]:
    /// `avail = reserve` (shotgun: 1), `avail = min(avail, clip)`, `need = clip − mag`,
    /// `n = min(need, avail)`; `mag += n; reserve -= n`. Returns the rounds loaded.
    /// (A reload is refused earlier when `mag == clip`.)
    pub fn reload(&mut self, w: &WeaponDef) -> u32 {
        let mut avail = self.reserve;
        if w.id == WeaponId::Shotgun {
            avail = 1;
        }
        avail = avail.min(w.clip);
        let n = w.clip.saturating_sub(self.mag).min(avail);
        self.mag += n;
        self.reserve = self.reserve.saturating_sub(n);
        n
    }
    /// Reserve update when a box pickup is applied (`H_TRACK_Reflex@0x58de70`, asm at 0x58f1e8..) [C]:
    /// `t = reserve + clip; if reserve_max < t { reserve = reserve_max } else { reserve = reserve + t }`.
    /// As compiled the else branch ADDS `t` to the old reserve (2·reserve + clip); this is the
    /// literal behaviour (looks like a shipped bug), reserve may then exceed `reserve_max`.
    pub fn apply_box_pickup(&mut self, w: &WeaponDef) {
        let t = self.reserve + w.clip;
        if self.reserve_max < t {
            self.reserve = self.reserve_max;
        } else {
            self.reserve += t;
        }
    }
}

/// Ammo box (`Munition_init@0x6c45a0`, `Munition_wait@0x6c4000`). `count` = number of pickups
/// stored in the box, clamped to 10 at init [C]. One pickup per use; refused (message
/// 0xd0005ae9) when `reserve == reserve_max` [C].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmmoBox {
    pub count: u32,
}

pub const AMMO_BOX_MAX: u32 = 10;

impl AmmoBox {
    pub fn new(count: u32) -> Self {
        AmmoBox { count: count.min(AMMO_BOX_MAX) }
    }
    /// Returns true if a pickup was consumed.
    pub fn take(&mut self, ammo: &mut Ammo, w: &WeaponDef) -> bool {
        if self.count == 0 || ammo.reserve_max.saturating_sub(ammo.reserve) == 0 {
            return false;
        }
        self.count -= 1;
        ammo.apply_box_pickup(w);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_uni_init() {
        // uni_init.ofc statements 769-832 [C]
        let exp = [
            (WeaponId::Colt, 8, 50.0, 5.0, 25.0, [8.0, 4.0, 2.0]),
            (WeaponId::TommyGun, 50, 80.0, 10.0, 10.0, [2.0, 1.0, 1.0]),
            (WeaponId::Shotgun, 5, 25.0, 5.0, 10.0, [20.0, 10.0, 5.0]),
            (WeaponId::SniperRifle, 5, 250.0, 5.0, 50.0, [15.0, 10.0, 10.0]),
            (WeaponId::Grenade, 1, 15.0, 2.5, 5.0, [25.0, 2.0, 2.0]),
            (WeaponId::Javelin, 1, 20.0, 5.0, 10.0, [11.0, 11.0, 5.0]),
            (WeaponId::BoneSpear, 1, 20.0, 5.0, 10.0, [7.0, 7.0, 3.0]),
            (WeaponId::SmallThrowable, 1, 20.0, 5.0, 10.0, [3.0, 2.0, 1.0]),
        ];
        for (id, clip, range, r1, r2, dmg) in exp {
            let d = def(id);
            assert_eq!((d.clip, d.range, d.r1, d.r2, d.damage), (clip, range, r1, r2, dmg));
        }
    }

    #[test]
    fn colt_damage_bands() {
        let c = def(WeaponId::Colt);
        assert_eq!(damage_at_distance(c, 1.0, false), 8.0);
        assert_eq!(damage_at_distance(c, 4.99, false), 8.0);
        assert_eq!(damage_at_distance(c, 5.0, false), 4.0);
        assert_eq!(damage_at_distance(c, 24.9, false), 4.0);
        assert_eq!(damage_at_distance(c, 25.0, false), 2.0);
        assert_eq!(damage_at_distance(c, 1.0, true), 1000.0);
    }

    #[test]
    fn bands_are_compared_on_squared_distance() {
        // fn@0x0053a370 takes the squared distance directly: 24.99 < R1^2 = 25 -> near.
        let c = def(WeaponId::Colt);
        assert_eq!(damage_at_distance_sq(c, 24.99, false), 8);
        assert_eq!(damage_at_distance_sq(c, 25.0, false), 4);
        assert_eq!(damage_at_distance_sq(c, 624.99, false), 4);
        assert_eq!(damage_at_distance_sq(c, 625.0, false), 2);
        assert_eq!(damage_at_distance_sq(c, 0.0, true), 1000);
        // Tommy: R1 == R2, so the mid band is empty.
        let t = def(WeaponId::TommyGun);
        assert_eq!(damage_at_distance_sq(t, 99.0, false), 2);
        assert_eq!(damage_at_distance_sq(t, 100.0, false), 1);
    }

    #[test]
    fn shotgun_grid_is_25_pellets_in_radians() {
        let p = shotgun_pattern(|_, _| 0.0);
        assert_eq!(p.len(), 25);
        let max = p.iter().map(|(x, y)| x.abs().max(y.abs())).fold(0.0, f32::max);
        assert!((max - 10.0f32.to_radians()).abs() < 1e-5);
        // 5 distinct angles per axis, 5° apart, f32-accumulated like the original
        let mut xs: Vec<f32> = p.iter().map(|q| q.0).collect();
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        assert!(xs.len() >= 5);
        assert!((p[1].0 - p[0].0 - 5.0f32.to_radians()).abs() < 1e-5);
        // centre pellet (index 12) is exactly (0,0) up to f32 accumulation
        assert!(p[12].0.abs() < 1e-5 && p[12].1.abs() < 1e-5);
    }

    #[test]
    fn shotgun_jitter_is_plus_minus_0_05_radians_and_skips_centre() {
        let mut calls = 0;
        let p = shotgun_pattern(|hi, lo| {
            calls += 1;
            assert_eq!((hi, lo), (0.05, -0.05));
            hi
        });
        assert_eq!(calls, 24 * 2); // pellet 12 is not jittered
        let q = shotgun_pattern(|_, _| 0.0);
        for i in 0..25 {
            let d = if i == 12 { 0.0 } else { 0.05 };
            assert!((p[i].0 - q[i].0 - d).abs() < 1e-6 && (p[i].1 - q[i].1 - d).abs() < 1e-6);
        }
    }

    #[test]
    fn shot_timers_and_auto_interval() {
        // H_callback_tir stores 0.4 / 0.2 / 0.5 / 0.4 at +0x3bc8/+0x3bcc (0x3ecccccd, 0x3e4ccccd, 0x3f000000, 0x3ecccccd)
        assert_eq!(def(WeaponId::Colt).shot_timer, f32::from_bits(0x3ecccccd));
        assert_eq!(def(WeaponId::TommyGun).shot_timer, f32::from_bits(0x3e4ccccd));
        assert_eq!(def(WeaponId::Shotgun).shot_timer, 0.5);
        assert_eq!(def(WeaponId::SniperRifle).shot_timer, f32::from_bits(0x3ecccccd));
        assert_eq!(def(WeaponId::TommyGun).auto_interval, 0.1);
    }

    #[test]
    fn reload_moves_min_of_missing_and_reserve() {
        let w = def(WeaponId::Colt);
        let mut a = Ammo { mag: 2, reserve: 3, reserve_max: 32 };
        assert_eq!(a.reload(w), 3);
        assert_eq!(a, Ammo { mag: 5, reserve: 0, reserve_max: 32 });
        let mut b = Ammo { mag: 2, reserve: 30, reserve_max: 32 };
        assert_eq!(b.reload(w), 6);
        assert_eq!(b.mag, 8);
        assert_eq!(b.reserve, 24);
    }

    #[test]
    fn shotgun_reloads_one_shell_per_cycle() {
        let w = def(WeaponId::Shotgun);
        let mut a = Ammo { mag: 1, reserve: 20, reserve_max: 30 };
        assert_eq!(a.reload(w), 1);
        assert_eq!((a.mag, a.reserve), (2, 19));
    }

    #[test]
    fn ammo_box_count_capped_at_ten() {
        assert_eq!(AmmoBox::new(50).count, 10);
        assert_eq!(AmmoBox::new(3).count, 3);
    }

    #[test]
    fn ammo_box_pickup_is_refused_when_full_and_follows_the_compiled_formula() {
        let w = def(WeaponId::Colt);
        let mut b = AmmoBox::new(2);
        let mut full = Ammo { mag: 0, reserve: 32, reserve_max: 32 };
        assert!(!b.take(&mut full, w));
        assert_eq!(b.count, 2);
        let mut a = Ammo { mag: 0, reserve: 0, reserve_max: 32 };
        assert!(b.take(&mut a, w));
        assert_eq!((a.reserve, b.count), (8, 1)); // 0 + (0 + clip 8)
        assert!(b.take(&mut a, w));
        assert_eq!(a.reserve, 24); // 8 + (8 + 8): literal compiled formula
        let mut c = Ammo { mag: 0, reserve: 30, reserve_max: 32 };
        c.apply_box_pickup(w);
        assert_eq!(c.reserve, 32); // reserve + clip > max -> max
    }
}
