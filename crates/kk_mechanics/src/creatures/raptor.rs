//! Venatosaurus (species 0xe of `PNJ_Raptor_*`), ledger X02 + CREATURE_AI A01-A15 (raptor view).
//!
//! Source functions (KingKong8.exe): `PNJ_Raptor_init@0x8315a0`, `ETAT_ATTENTE@0x839e40`, `exec_select_action@0x83a950`,
//! `exec_check_vision@0x83f130` (+`fn@0x006e58c0` cone test), `exec_check_shoot@0x85b0a0`, `exec_check_paf@0x849910`,
//! `exec_update_best_interest@0x863360`, `exec_test_bite@0x866ed0`, `exec_bite@0x84cae0`, `ETAT_MORD@0x84d860`,
//! `ETAT_GRAB@0x847af0`, `ETAT_PAF_*`, `ETAT_MORT@0x8535c0`, `ETAT_FADE@0x846020`, `exec_check_javelin@0x869370`.
//! Tags: [C] read from the decompiled code, [L] inferred, [G] guess. See `spec/evidence/X02.md`.

use crate::wounds::HitKind;

// ---------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------

/// hp_max of variants 0 and 1 [C init@0x8315a0: 0x42480000].
pub const HP_MAX_STANDARD: f32 = 50.0;
/// hp_max of variant 2 (alpha) [C init: 0x42c80000].
pub const HP_MAX_ALPHA: f32 = 100.0;
/// Scale of variant 2 [C init: 0x3f933333 = 1.15].
pub const ALPHA_SCALE: f32 = 1.15;
/// Fire/aux gauge `+0x2194` [C init: 0x41a00000].
pub const FIRE_GAUGE: f32 = 20.0;

/// Idle time in ATTENTE before the raptor starts to patrol [C ETAT_ATTENTE `+0x1404 <= 1.0`].
pub const IDLE_BEFORE_PATROL_S: f32 = 1.0;
/// Sighting age above which FIGHT drops to SEARCH [C ETAT_FIGHT / ETAT_ATTENTE `> 1.5`].
pub const LOST_TARGET_AGE_S: f32 = 1.5;
/// Clamp of the sighting age while searching [C ETAT_SEARCH `6.0`].
pub const SEARCH_AGE_CLAMP_S: f32 = 6.0;
/// Radius of the search ring around the last known position `+0x1924` [C init 0x40a00000].
pub const SEARCH_RING_RADIUS: f32 = 5.0;
/// Radius of the circling ring in shark/wait mode [C way_find_dest_pos case 3: normalise * 5.0].
pub const CIRCLE_RING_RADIUS: f32 = 5.0;
/// Orbit time after which circling stops [C ETAT_FIGHT `4.0 < +0x1aa8`].
pub const CIRCLE_ORBIT_MAX_S: f32 = 4.0;

/// Wait ("requin") after a bite, a flinch or a lost target [C ETAT_MORD / PAF_* / check_requin: 0x40400000].
pub const POST_BITE_WAIT_S: f32 = 3.0;

/// Vision: narrow cone cosine (30 degrees) [C check_vision `0x3f5db3d7`].
pub const VISION_COS_NARROW: f32 = 0.866_025_4;
/// Vision: narrow cone range `+0x210c` [C init 0x41200000]; the near radius is added.
pub const VISION_RANGE: f32 = 10.0;
/// Vision: always-seen radius around the head [C check_vision 1.0].
pub const VISION_NEAR_RADIUS: f32 = 1.0;
/// Vision: wide cone cosine for humans (130.8 degrees half angle) [C check_vision `0xbf275526`].
pub const VISION_COS_WIDE: f32 = -0.653_643;
/// Vision: wide cone range for humans [C check_vision 0x42c80000].
pub const VISION_RANGE_WIDE: f32 = 100.0;
/// Shot-near detection: closest approach (SQUARED, 25 -> 5 m) [C check_shoot].
pub const SHOT_NEAR_SQ: f32 = 25.0;

/// Bite start radius = this * scale (horizontal) [C test_bite `3.0`].
pub const BITE_START_RADIUS: f32 = 3.0;
/// Below this squared distance the bite starts without the cone test [C test_bite `2.0`].
pub const BITE_START_ALWAYS_SQ: f32 = 2.0;
/// Cone for the bite start (60 degrees) [C test_bite `0.5`].
pub const BITE_START_COS: f32 = 0.5;
/// Vertical window of the bite start [C test_bite (-2.0, 3.5)].
pub const BITE_DZ: (f32, f32) = (-2.0, 3.5);
/// Reach of the bite hit test [C exec_bite `2.5`].
pub const BITE_REACH: f32 = 2.5;
/// Cone of the bite hit test [C exec_bite `0x3f5db3d7`].
pub const BITE_HIT_COS: f32 = 0.866_025_4;
/// Frames of anim 0x46 during which the bite is armed (inclusive) [C ETAT_MORD `0xe..0x13`].
pub const BITE_ARMED_FRAMES: (u32, u32) = (14, 19);
/// Hold time before the grab sends the kill message [C ETAT_GRAB `4.0 < +0x1404`, meaning L].
pub const GRAB_KILL_AFTER_S: f32 = 4.0;

/// Death: MORT -> FADE after this [C ETAT_MORT `5.0 < +0x1404`].
pub const MORT_TO_FADE_S: f32 = 5.0;
/// Death: corpse released after this in FADE [C ETAT_FADE `+0x1404 <= 10.0`].
pub const FADE_DURATION_S: f32 = 10.0;
/// Wounded flag below this hp ratio [C reflex `< 0.3`].
pub const WOUNDED_RATIO: f32 = 0.3;
/// Corpse/food ignored if farther than sqrt(2500) from Jack [C update_best_interest `2500.0`].
pub const FOOD_MAX_JACK_DIST_SQ: f32 = 2500.0;
/// Time since last target change before "hurt me" re-targets [C update_best_interest `0x40800000`].
pub const RETARGET_AFTER_S: f32 = 4.0;
/// Time since the last bullet hit before the other-raptor rule applies [C update_best_interest `0x41200000`].
pub const OTHER_RAPTOR_AFTER_S: f32 = 10.0;
/// Knockback speed of PAF_SLIDE / PAF_FALL along the hit direction [C `* 10.0`]; PAF_FLY uses 12 with vz >= 6.
pub const KNOCKBACK_SPEED: f32 = 10.0;
pub const KNOCKBACK_FLY_SPEED: f32 = 12.0;
pub const KNOCKBACK_FLY_MIN_VZ: f32 = 6.0;

/// Perceived-slot flag bits (`this+0x1578[i]`). Bit meaning [C set-site] / [L meaning]; see X02.
pub mod flag {
    pub const JACK: u32 = 0x1;
    pub const SHOT_NEAR: u32 = 0x2;
    pub const LOUD: u32 = 0x4;
    pub const IN_CONE: u32 = 0x8;
    pub const OTHER_RAPTOR: u32 = 0x10;
    pub const HURT_ME: u32 = 0x20;
    pub const BULLET: u32 = 0x40;
    pub const OTHER_HAS_INTEREST: u32 = 0x80;
    pub const DOWNED: u32 = 0x100;
    pub const HIDING: u32 = 0x200;
    pub const CORPSE: u32 = 0x400;
    pub const HEARD: u32 = 0x800;
    pub const OCCUPIED: u32 = 0x1000;
    pub const UNREACHABLE: u32 = 0x2000;
    pub const CLAIMED: u32 = 0x4000;
    pub const CINEMATIC: u32 = 0x8000;
    pub const SEEN: u32 = 0x100_0000;
    pub const FAR_CORPSE: u32 = 0x200_0000;
    pub const EATEN: u32 = 0x400_0000;
    /// Slots with any of these bits are skipped by the main candidate loop [C update_best_interest `0x5580`].
    pub const MAIN_LOOP_EXCLUDE: u32 = 0x5580;
}

// ---------------------------------------------------------------------------------------------
// Variant, states
// ---------------------------------------------------------------------------------------------

/// `this+0x24` for species 0xe [C init].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Standard = 0,
    Light = 1,
    Alpha = 2,
}

/// Flinch thresholds of `check_paf` [C].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    pub light: f32,
    pub medium: f32,
    pub heavy: f32,
    pub accumulated: f32,
}

impl Variant {
    pub fn hp_max(self) -> f32 {
        match self {
            Variant::Alpha => HP_MAX_ALPHA,
            _ => HP_MAX_STANDARD,
        }
    }
    pub fn scale(self) -> f32 {
        match self {
            Variant::Alpha => ALPHA_SCALE,
            _ => 1.0,
        }
    }
    /// Damage carried by the bite message to Jack [C exec_bite: 6 / 6 / 10].
    pub fn bite_damage(self) -> f32 {
        match self {
            Variant::Alpha => 10.0,
            _ => 6.0,
        }
    }
    /// Jack wound class of the bite flags: 0x4004 (bit 4 = heavy), 0x4002 (bit 2 = medium) [C exec_bite].
    pub fn bite_hit_kind(self) -> HitKind {
        match self {
            Variant::Light => HitKind::Medium,
            _ => HitKind::Heavy,
        }
    }
    /// check_paf thresholds: 8/11/20/10 (variants 0,1) and 15/20/40/30 (variant 2) [C].
    pub fn thresholds(self) -> Thresholds {
        match self {
            Variant::Alpha => Thresholds { light: 15.0, medium: 20.0, heavy: 40.0, accumulated: 30.0 },
            _ => Thresholds { light: 8.0, medium: 11.0, heavy: 20.0, accumulated: 10.0 },
        }
    }
}

/// Value of `this+0x13f4` for each state [C, first lines of each ETAT function].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RaptorState {
    Attente = 0,
    Search = 1,
    Fight = 2,
    ATerre = 3,
    Vala = 4,
    Hesite = 5,
    Mort = 6,
    Mord = 7,
    Grab = 8,
    Lance = 9,
    Devore = 10,
    AttaqueCache = 11,
    Fall = 12,
    Rode = 13,
    PafSlide = 14,
    PafFall = 15,
    PafFly = 16,
    Fade = 17,
    Coodbool = 18,
    Burn = 19,
}

impl RaptorState {
    pub fn id(self) -> u8 {
        self as u8
    }
}

// ---------------------------------------------------------------------------------------------
// Perception
// ---------------------------------------------------------------------------------------------

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `fn@0x006e58c0` (`Perception_ConeTest`) as used by check_vision with flatten = 1, minus the cone-edge capsule
/// branch (taken only when the target is just outside the cone; not ported). Returns the distance (+1e-5) when seen.
/// `near` is the always-seen radius, `range` is added to it for the cone [C].
pub fn cone_test(origin: V3, axis: V3, cos_thr: f32, range: f32, target: V3, near: f32, flatten: bool) -> Option<f32> {
    let mut axis = axis;
    let mut t = target;
    if flatten {
        t[2] = origin[2];
        axis[2] = 0.0;
        let len = dot(axis, axis).sqrt();
        if len < 0.001 {
            return None;
        }
        axis = [axis[0] / len, axis[1] / len, 0.0];
    }
    let d = sub(t, origin);
    let d2 = dot(d, d);
    if d2 <= near * near {
        return Some(d2.sqrt() + 1e-5);
    }
    if d2 <= (range + near) * (range + near) {
        let dist = d2.sqrt();
        let c = dot(axis, [d[0] / dist, d[1] / dist, d[2] / dist]);
        if c >= cos_thr {
            return Some(dist + 1e-5);
        }
    }
    None
}

/// Narrow-cone sight (flag IN_CONE) for species 0xe [C].
pub fn sees_in_cone(head: V3, forward: V3, target: V3) -> bool {
    cone_test(head, forward, VISION_COS_NARROW, VISION_RANGE, target, VISION_NEAR_RADIUS, true).is_some()
}

/// Wide-cone sight for human targets (class 1..12); the caller must also pass the line-of-sight ray [C].
pub fn sees_human_wide(head: V3, forward: V3, target: V3, line_of_sight_clear: bool) -> bool {
    line_of_sight_clear
        && cone_test(head, forward, VISION_COS_WIDE, VISION_RANGE_WIDE, target, VISION_NEAR_RADIUS, true).is_some()
}

/// One slot of the 20-entry perceived-actor list (`fn@0x0047e180`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Perceived {
    pub actor: u32,
    /// `this+0x1620[i]`: 1 = Jack, 2..12 other humans, 0xe raptor, 0x21 prop/food [C `fn@0x007dd2d0` users].
    pub class: u8,
    pub flags: u32,
    pub dist: f32,
    pub life_ratio: f32,
    pub pos: V3,
    /// `+0x14d0[i] != 0`
    pub reachable: bool,
    /// `+0x147c[i] != 0` ("changed/new")
    pub changed: bool,
    /// `+0x1524[i] == +0x8f4`
    pub same_territory: bool,
    pub dist_to_jack_sq: f32,
}

/// Inputs of the target selection chain.
#[derive(Clone, Copy, Debug)]
pub struct SelectCtx {
    pub state: RaptorState,
    pub jack_slot: Option<usize>,
    pub current: Option<usize>,
    pub since_target_change: f32,
    pub since_bullet_hit: f32,
    pub wait_timer: f32,
}

fn nearest(slots: &[Perceived], pred: impl Fn(&Perceived) -> bool) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, s) in slots.iter().enumerate() {
        if !pred(s) {
            continue;
        }
        match best {
            Some(b) if slots[b].dist <= s.dist => {}
            _ => best = Some(i),
        }
    }
    best
}

/// `update_best_interest@0x863360` priority chain (rules 1,3,4,5,6,7; rules 2,8,9 lost assignments are not ported). First match wins [C order, L tie-breaks].
pub fn select_target(slots: &[Perceived], ctx: &SelectCtx) -> Option<usize> {
    use RaptorState::*;
    let busy = matches!(ctx.state, Mord | Grab | Coodbool);
    // 1: cinematic forced
    if let Some(i) = slots.iter().position(|s| s.flags & flag::CINEMATIC != 0) {
        return Some(i);
    }
    // 3: Jack
    if let Some(j) = ctx.jack_slot {
        if j < slots.len() && slots[j].flags & flag::JACK != 0 {
            return Some(j);
        }
    }
    // 4: whatever hurt it
    if !busy && ctx.since_target_change >= RETARGET_AFTER_S {
        if let Some(i) = slots.iter().position(|s| s.flags & flag::HURT_ME != 0) {
            return Some(i);
        }
    }
    // 5: another raptor, 10 s after the last bullet hit, when not already on a raptor
    let cur_is_raptor = ctx.current.map_or(false, |c| slots.get(c).map_or(false, |s| s.class == 0xe));
    if ctx.since_bullet_hit >= OTHER_RAPTOR_AFTER_S && !cur_is_raptor {
        if let Some(i) = nearest(slots, |s| {
            s.flags & flag::OTHER_RAPTOR != 0 && s.flags & (flag::HIDING | flag::CORPSE) == 0 && s.same_territory
        }) {
            return Some(i);
        }
    }
    // 6: corpses / food
    if ctx.wait_timer == 0.0 && !matches!(ctx.state, Vala | Mord | Grab | Coodbool) {
        if let Some(i) = nearest(slots, |s| {
            s.reachable
                && s.flags & flag::CORPSE != 0
                && s.flags & flag::FAR_CORPSE == 0
                && s.dist_to_jack_sq <= FOOD_MAX_JACK_DIST_SQ
        }) {
            return Some(i);
        }
    }
    // 7: nearest eligible slot in the narrow cone
    if !matches!(ctx.state, Mord | Grab | Devore | Coodbool) {
        if let Some(i) = nearest(slots, |s| {
            s.flags & flag::IN_CONE != 0 && s.flags & flag::MAIN_LOOP_EXCLUDE == 0 && s.reachable && s.changed
        }) {
            return Some(i);
        }
    }
    ctx.current
}

// ---------------------------------------------------------------------------------------------
// Damage reception (check_paf)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    None,
    /// PAF_SLIDE
    Light,
    /// PAF_FALL
    Medium,
    /// PAF_FLY
    Heavy,
}

/// One incoming paf message (weapon hit etc).
#[derive(Clone, Copy, Debug)]
pub struct HitIn {
    /// message damage `fn@0x0051f110` (the weapon's distance-band damage, `weapons::damage_at_distance`)
    pub damage: f32,
    /// hit flags `fn@0x0051f190`
    pub flags: u32,
    /// bone id is 0 or 0xf5 (head/jaw) [C `fn@0x006e4170`]
    pub head: bool,
    /// z of the hit direction `+0x1c5c`
    pub dir_z: f32,
    pub from_jack: bool,
    /// the raptor is currently biting Jack's leg latch `+0x1d54 != 0`
    pub bite_leg_latch: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitOutcome {
    /// hp removed (before the min(hp, .) clamp)
    pub damage: f32,
    /// `CapLife` fraction of hp_max (flags 0x80 -> 0.3, 0x100 -> 0.15, 0x200 -> 0)
    pub cap_fraction: Option<f32>,
    pub reaction: Reaction,
}

/// Hit flag bits [C check_paf].
pub mod hit {
    pub const CAP_30: u32 = 0x80;
    pub const CAP_15: u32 = 0x100;
    pub const KILL: u32 = 0x200;
    pub const BULLET_BITS: u32 = 0x440;
    pub const HEAVY_IGNORE_DIR: u32 = 0x400;
    pub const LIGHT_ONLY: u32 = 0x20_0000;
}

/// `check_paf@0x849910` for species 0xe: damage multiplier, life caps and flinch class. `acc` is the damage
/// accumulator `+0x1c3c` (updated in place); `leaping` = `this+0x290c != 0`; `in_long_anim` = current anim in
/// {0x33,0x35,0x38..0x3b}.
pub fn classify_hit(v: Variant, h: &HitIn, acc: &mut f32, leaping: bool, in_long_anim: bool) -> HitOutcome {
    let t = v.thresholds();
    let mut d = if h.damage == 10000.0 { 10.0 } else { h.damage };
    *acc += d;
    let mut mult = 1.0;
    if h.flags & hit::BULLET_BITS != 0 {
        match v {
            Variant::Alpha => {
                if !h.head {
                    d = (d - 5.0).max(0.0);
                }
            }
            _ => {
                if h.head {
                    mult = 2.0;
                }
            }
        }
    }
    let cap_fraction = if h.flags & hit::KILL != 0 {
        Some(0.0)
    } else if h.flags & hit::CAP_15 != 0 {
        Some(0.15)
    } else if h.flags & hit::CAP_30 != 0 {
        Some(0.3)
    } else {
        None
    };
    let reaction = if h.flags & hit::LIGHT_ONLY != 0 {
        *acc = 0.0;
        Reaction::Light
    } else if h.from_jack && h.bite_leg_latch {
        *acc = 0.0;
        Reaction::Medium
    } else if d < t.heavy || h.dir_z <= -0.5 || h.flags & hit::HEAVY_IGNORE_DIR != 0 {
        if d < t.medium {
            if t.light <= d || (!leaping && t.accumulated <= *acc) {
                *acc = 0.0;
                Reaction::Light
            } else {
                Reaction::None
            }
        } else {
            *acc = 0.0;
            Reaction::Medium
        }
    } else {
        *acc = 0.0;
        Reaction::Heavy
    };
    let reaction = if in_long_anim && reaction != Reaction::None { Reaction::Heavy } else { reaction };
    HitOutcome { damage: d * mult, cap_fraction, reaction }
}

/// `exec_check_javelin@0x869370`: hp lost this frame to `spears` embedded spears (max 3 count), floor 2.0 [C].
pub fn spear_bleed(hp: f32, hp_max: f32, spears: u32, dt: f32) -> f32 {
    let mut hp = hp;
    for _ in 0..spears.min(3) {
        if hp > 2.0 {
            hp = (hp - dt / 60.0 * hp_max).max(2.0);
        }
    }
    hp
}

// ---------------------------------------------------------------------------------------------
// Bite
// ---------------------------------------------------------------------------------------------

/// `exec_test_bite@0x866ed0` geometry for species 0xe: `dx,dy,dz` = target - raptor, `forward` = raptor facing [C].
pub fn bite_start_ok(scale: f32, delta: V3, forward: V3) -> bool {
    if !(dot(delta, forward) > 0.0 && delta[2] > BITE_DZ.0 && delta[2] < BITE_DZ.1) {
        return false;
    }
    let d2 = delta[0] * delta[0] + delta[1] * delta[1];
    if d2 < BITE_START_ALWAYS_SQ {
        return true;
    }
    let r = BITE_START_RADIUS * scale;
    if d2 >= r * r {
        return false;
    }
    let d = d2.sqrt();
    let f2 = (forward[0] * forward[0] + forward[1] * forward[1]).sqrt();
    if f2 == 0.0 {
        return false;
    }
    (delta[0] * forward[0] + delta[1] * forward[1]) / (d * f2) > BITE_START_COS
}

/// `exec_bite` hit test: cone from the head bone, reach 2.5 m, cos 0.866, `radius` = target capsule radius [C].
pub fn bite_hits(head: V3, forward: V3, target: V3, radius: f32) -> bool {
    cone_test(head, forward, BITE_HIT_COS, BITE_REACH, target, radius, false).is_some()
}

/// Is the bite armed in frame `frame` of anim 0x46 [C ETAT_MORD].
pub fn bite_armed(anim_0x46_frame: u32) -> bool {
    anim_0x46_frame >= BITE_ARMED_FRAMES.0 && anim_0x46_frame <= BITE_ARMED_FRAMES.1
}

/// Grab counter `this+0x1d64` (`exec_bite@0x84cae0`): Jack is grabbed only while it is 0; a grab sets it to
/// `(n + 1) % 3`; nothing else in the raptor code writes it [C].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GrabCounter(pub u8);

impl GrabCounter {
    pub fn can_grab_jack(&self) -> bool {
        self.0 == 0
    }
    pub fn on_grab_jack(&mut self) {
        self.0 = (self.0 + 1) % 3;
    }
}

// ---------------------------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct RaptorInput {
    pub perceived: Vec<Perceived>,
    pub jack_slot: Option<usize>,
    /// raptor head and facing
    pub head: V3,
    pub forward: V3,
    pub scale_override: Option<f32>,
    pub hits: Vec<HitIn>,
    pub spears: u32,
    /// the current animation (bite, flinch, ground) has ended
    pub anim_done: bool,
    /// frame of anim 0x46 while MORD
    pub bite_frame: u32,
    /// `bite_hits` result computed by the caller for this frame
    pub bite_connects: bool,
    /// the bite target is Jack
    pub bite_target_is_jack: bool,
    /// is at its patrol waypoint
    pub at_waypoint: bool,
    pub idle_expired_override: bool,
}

impl Default for RaptorInput {
    fn default() -> Self {
        RaptorInput {
            perceived: Vec::new(),
            jack_slot: None,
            head: [0.0; 3],
            forward: [1.0, 0.0, 0.0],
            scale_override: None,
            hits: Vec::new(),
            spears: 0,
            anim_done: false,
            bite_frame: 0,
            bite_connects: false,
            bite_target_is_jack: false,
            at_waypoint: false,
            idle_expired_override: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RaptorEvent {
    StateChanged { from: RaptorState, to: RaptorState },
    TargetAcquired(usize),
    Damaged(f32),
    Flinch(Reaction),
    BiteStarted,
    /// plain damage bite on Jack: damage and wound class
    BiteHit { damage: f32, kind: HitKind },
    GrabStarted,
    /// 4.0 s into the hold: kill message 0x204
    GrabKill,
    Died,
    /// corpse released
    Faded,
}

#[derive(Clone, Debug)]
pub struct Raptor {
    pub variant: Variant,
    pub state: RaptorState,
    pub state_time: f32,
    pub hp: f32,
    pub wait: f32,
    pub acc: f32,
    pub grab: GrabCounter,
    pub target: Option<usize>,
    /// seconds since the target was last confirmed visible
    pub sighting_age: f32,
    pub since_target_change: f32,
    pub since_bullet_hit: f32,
    pub hesitation_clock: f32,
    pub hesitation_len: f32,
    pub bite_anim_toggle: u8,
    bite_landed: bool,
    pending: Reaction,
}

impl Raptor {
    pub fn new(variant: Variant) -> Self {
        Raptor {
            variant,
            state: RaptorState::Attente,
            state_time: 0.0,
            hp: variant.hp_max(),
            wait: 0.0,
            acc: 0.0,
            grab: GrabCounter::default(),
            target: None,
            sighting_age: 0.0,
            since_target_change: 1.0e9,
            since_bullet_hit: 1.0e9,
            hesitation_clock: 0.0,
            hesitation_len: 0.0,
            bite_anim_toggle: 0,
            bite_landed: false,
            pending: Reaction::None,
        }
    }

    pub fn wounded(&self) -> bool {
        self.hp / self.variant.hp_max() < WOUNDED_RATIO
    }

    fn goto(&mut self, to: RaptorState, ev: &mut Vec<RaptorEvent>) {
        if to != self.state {
            ev.push(RaptorEvent::StateChanged { from: self.state, to });
            self.state = to;
            self.state_time = 0.0;
        }
    }

    /// Apply `Raptor_ApplyDamage` / `Raptor_CapLife`.
    fn take(&mut self, o: &HitOutcome, ev: &mut Vec<RaptorEvent>) {
        let was_alive = self.hp > 0.0;
        let d = o.damage.min(self.hp);
        self.hp -= d;
        if d > 0.0 {
            ev.push(RaptorEvent::Damaged(d));
        }
        if let Some(f) = o.cap_fraction {
            self.hp = self.hp.min(f * self.variant.hp_max());
        }
        if self.hp < 0.01 {
            self.hp = 0.0;
        }
        if was_alive && self.hp == 0.0 {
            ev.push(RaptorEvent::Died);
        }
    }

    /// One frame (`dt` seconds). Pure: all world queries come in through `inp`.
    pub fn step(&mut self, inp: &RaptorInput, dt: f32) -> Vec<RaptorEvent> {
        use RaptorState::*;
        let mut ev = Vec::new();
        self.state_time += dt;
        self.wait = (self.wait - dt.min(self.wait)).max(0.0);
        self.since_target_change += dt;
        self.since_bullet_hit += dt;
        self.sighting_age += dt;

        // death states are terminal for the AI
        if matches!(self.state, Mort | Fade) {
            return self.step_death(dt, ev);
        }

        // spear bleed (never kills: floor 2.0)
        if inp.spears > 0 {
            self.hp = spear_bleed(self.hp, self.variant.hp_max(), inp.spears, dt);
        }

        // check_paf
        let mut reaction = Reaction::None;
        for h in &inp.hits {
            if h.flags & hit::BULLET_BITS & 0x40 != 0 {
                self.since_bullet_hit = 0.0;
            }
            let o = classify_hit(self.variant, h, &mut self.acc, false, false);
            self.take(&o, &mut ev);
            if o.reaction as u8 > reaction as u8 {
                reaction = o.reaction;
            }
        }
        if reaction != Reaction::None && self.hp > 0.0 {
            ev.push(RaptorEvent::Flinch(reaction));
        }
        if self.hp == 0.0 {
            // reaction states lead to the ground state, then death [C ETAT_A_TERRE: hp == 0 -> MORT]
            if self.state != ATerre {
                self.goto(ATerre, &mut ev);
                return ev;
            }
            return self.step_death(dt, ev);
        }
        match reaction {
            Reaction::Heavy => self.pending = Reaction::Heavy,
            Reaction::Medium if self.pending != Reaction::Heavy => self.pending = Reaction::Medium,
            Reaction::Light if self.pending == Reaction::None => self.pending = Reaction::Light,
            _ => {}
        }
        if self.pending != Reaction::None {
            let to = match self.pending {
                Reaction::Heavy => PafFly,
                Reaction::Medium => PafFall,
                _ => PafSlide,
            };
            self.pending = Reaction::None;
            self.wait = POST_BITE_WAIT_S;
            self.goto(to, &mut ev);
            return ev;
        }
        if matches!(self.state, PafSlide | PafFall | PafFly | Fall) {
            if inp.anim_done {
                self.wait = POST_BITE_WAIT_S;
                self.goto(Attente, &mut ev);
            }
            return ev;
        }

        // update_best_interest
        let ctx = SelectCtx {
            state: self.state,
            jack_slot: inp.jack_slot,
            current: self.target,
            since_target_change: self.since_target_change,
            since_bullet_hit: self.since_bullet_hit,
            wait_timer: self.wait,
        };
        let sel = select_target(&inp.perceived, &ctx);
        if sel != self.target {
            self.target = sel;
            self.since_target_change = 0.0;
            self.hesitation_clock = 0.0;
            // hesitation length = rand(0.5,1.0) * +0x3d8 (data); the midpoint is used [G]
            self.hesitation_len = 0.75;
            if let Some(i) = sel {
                ev.push(RaptorEvent::TargetAcquired(i));
                if !matches!(self.state, Mord | Grab) {
                    self.sighting_age = 0.0;
                    self.goto(Hesite, &mut ev);
                    return ev;
                }
            }
        }
        if let Some(t) = self.target.and_then(|i| inp.perceived.get(i)) {
            if t.flags & (flag::IN_CONE | flag::SEEN) != 0 {
                self.sighting_age = 0.0;
            }
        }

        let scale = inp.scale_override.unwrap_or_else(|| self.variant.scale());
        let tgt = self.target.and_then(|i| inp.perceived.get(i)).copied();
        match self.state {
            Attente => match tgt {
                None => {
                    if self.state_time > IDLE_BEFORE_PATROL_S {
                        self.goto(Vala, &mut ev);
                    }
                }
                Some(t) => {
                    let next = if self.wait != 0.0 {
                        Fight
                    } else if t.flags & flag::HIDING != 0 {
                        AttaqueCache
                    } else if t.flags & flag::UNREACHABLE != 0 {
                        if self.hesitation_clock < self.hesitation_len { Hesite } else { Vala }
                    } else if t.flags & flag::CORPSE != 0 && t.reachable {
                        Devore
                    } else if self.sighting_age > LOST_TARGET_AGE_S {
                        Search
                    } else {
                        Fight
                    };
                    self.goto(next, &mut ev);
                }
            },
            Vala => {
                if let Some(t) = tgt {
                    self.maybe_bite(&t, inp, scale, &mut ev);
                } else if inp.at_waypoint {
                    self.goto(Attente, &mut ev);
                }
            }
            Hesite => {
                self.hesitation_clock += dt;
                if let Some(t) = tgt {
                    if t.flags & flag::CORPSE != 0 {
                        self.goto(Devore, &mut ev);
                    } else if !self.maybe_bite(&t, inp, scale, &mut ev) && self.hesitation_clock >= self.hesitation_len {
                        self.goto(Fight, &mut ev);
                    }
                } else {
                    self.goto(Attente, &mut ev);
                }
            }
            Search => {
                self.sighting_age = self.sighting_age.min(SEARCH_AGE_CLAMP_S);
                if tgt.is_none() {
                    self.goto(Attente, &mut ev);
                } else if self.sighting_age <= LOST_TARGET_AGE_S || self.wait != 0.0 {
                    self.goto(Fight, &mut ev);
                }
            }
            Fight => match tgt {
                None => self.goto(Attente, &mut ev),
                Some(t) => {
                    if !self.maybe_bite(&t, inp, scale, &mut ev) && self.sighting_age > LOST_TARGET_AGE_S {
                        self.goto(Search, &mut ev);
                    }
                }
            },
            Mord => {
                if self.state_time <= dt {
                    self.bite_anim_toggle = (self.bite_anim_toggle + 1) % 2;
                    self.bite_landed = false;
                }
                if !self.bite_landed && bite_armed(inp.bite_frame) && inp.bite_connects {
                    self.bite_landed = true;
                    if inp.bite_target_is_jack && self.grab.can_grab_jack() {
                        self.grab.on_grab_jack();
                        ev.push(RaptorEvent::GrabStarted);
                        self.goto(Grab, &mut ev);
                        return ev;
                    }
                    ev.push(RaptorEvent::BiteHit { damage: self.variant.bite_damage(), kind: self.variant.bite_hit_kind() });
                }
                if inp.anim_done {
                    self.wait = POST_BITE_WAIT_S;
                    self.goto(Attente, &mut ev);
                }
            }
            Grab => {
                if self.state_time > GRAB_KILL_AFTER_S {
                    ev.push(RaptorEvent::GrabKill);
                    self.goto(Lance, &mut ev);
                } else if inp.anim_done && self.state_time > 2.0 {
                    self.goto(Lance, &mut ev);
                }
            }
            Lance => {
                if self.state_time > 1.2 {
                    self.wait = POST_BITE_WAIT_S;
                    self.goto(Vala, &mut ev);
                }
            }
            Devore => {
                if !tgt.map_or(false, |t| t.flags & flag::CORPSE != 0) {
                    self.goto(Attente, &mut ev);
                }
            }
            // unported states fall back to the hub
            AttaqueCache | Rode | Coodbool | Burn | Fall | ATerre | Mort | Fade | PafSlide | PafFall | PafFly => {
                self.goto(Attente, &mut ev);
            }
        }
        ev
    }

    fn maybe_bite(&mut self, t: &Perceived, inp: &RaptorInput, scale: f32, ev: &mut Vec<RaptorEvent>) -> bool {
        if self.wait != 0.0 {
            return false;
        }
        let delta = sub(t.pos, inp.head);
        if bite_start_ok(scale, delta, inp.forward) {
            ev.push(RaptorEvent::BiteStarted);
            self.goto(RaptorState::Mord, ev);
            true
        } else {
            false
        }
    }

    fn step_death(&mut self, _dt: f32, mut ev: Vec<RaptorEvent>) -> Vec<RaptorEvent> {
        use RaptorState::*;
        match self.state {
            ATerre => {
                // hp == 0 -> MORT [C ETAT_A_TERRE]
                self.goto(Mort, &mut ev);
            }
            Mort => {
                if self.state_time > MORT_TO_FADE_S {
                    self.goto(Fade, &mut ev);
                }
            }
            Fade => {
                if self.state_time > FADE_DURATION_S {
                    ev.push(RaptorEvent::Faded);
                }
            }
            _ => {}
        }
        ev
    }
}

// ---------------------------------------------------------------------------------------------
// Tests: pin recovered numbers
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weapons::{damage_at_distance, def, WeaponId};

    fn hit_of(w: WeaponId, dist: f32, head: bool) -> HitIn {
        HitIn {
            damage: damage_at_distance(def(w), dist, false),
            flags: 0x40,
            head,
            dir_z: 0.0,
            from_jack: true,
            bite_leg_latch: false,
        }
    }

    fn slot(class: u8, flags: u32, dist: f32) -> Perceived {
        Perceived {
            actor: 1,
            class,
            flags,
            dist,
            life_ratio: 1.0,
            pos: [dist, 0.0, 0.0],
            reachable: true,
            changed: true,
            same_territory: true,
            dist_to_jack_sq: 0.0,
        }
    }

    #[test]
    fn hp_and_variant_numbers() {
        assert_eq!(Variant::Standard.hp_max(), 50.0);
        assert_eq!(Variant::Light.hp_max(), 50.0);
        assert_eq!(Variant::Alpha.hp_max(), 100.0);
        assert_eq!(Variant::Standard.bite_damage(), 6.0);
        assert_eq!(Variant::Light.bite_damage(), 6.0);
        assert_eq!(Variant::Alpha.bite_damage(), 10.0);
        assert_eq!(Variant::Light.bite_hit_kind(), HitKind::Medium);
        assert_eq!(Variant::Alpha.bite_hit_kind(), HitKind::Heavy);
        assert_eq!(Variant::Standard.thresholds(), Thresholds { light: 8.0, medium: 11.0, heavy: 20.0, accumulated: 10.0 });
        assert_eq!(Variant::Alpha.thresholds(), Thresholds { light: 15.0, medium: 20.0, heavy: 40.0, accumulated: 30.0 });
        assert_eq!(Variant::Alpha.scale(), f32::from_bits(0x3f933333));
        assert_eq!(HP_MAX_STANDARD, f32::from_bits(0x42480000));
        assert_eq!(HP_MAX_ALPHA, f32::from_bits(0x42c80000));
    }

    #[test]
    fn state_ids_match_the_etat_functions() {
        assert_eq!(RaptorState::Attente.id(), 0);
        assert_eq!(RaptorState::Vala.id(), 4);
        assert_eq!(RaptorState::Mord.id(), 7);
        assert_eq!(RaptorState::Grab.id(), 8);
        assert_eq!(RaptorState::Devore.id(), 10);
        assert_eq!(RaptorState::PafFly.id(), 16);
        assert_eq!(RaptorState::Burn.id(), 19);
    }

    #[test]
    fn constants_equal_the_binary_immediates() {
        assert_eq!(VISION_COS_NARROW, f32::from_bits(0x3f5db3d7));
        assert_eq!(BITE_HIT_COS, f32::from_bits(0x3f5db3d7));
        assert_eq!(VISION_COS_WIDE, f32::from_bits(0xbf275526));
        assert_eq!(VISION_RANGE, f32::from_bits(0x41200000));
        assert_eq!(VISION_RANGE_WIDE, f32::from_bits(0x42c80000));
        assert_eq!(POST_BITE_WAIT_S, f32::from_bits(0x40400000));
        assert_eq!(SEARCH_RING_RADIUS, f32::from_bits(0x40a00000));
    }

    #[test]
    fn colt_flinch_bands() {
        let mut acc = 0.0;
        // Colt < 5 m = 8 damage: light flinch every hit
        let o = classify_hit(Variant::Standard, &hit_of(WeaponId::Colt, 3.0, false), &mut acc, false, false);
        assert_eq!(o.reaction, Reaction::Light);
        assert_eq!(o.damage, 8.0);
        // Colt at 10 m = 4 damage: nothing until the accumulator reaches 10 (3rd hit)
        let mut acc = 0.0;
        let h = hit_of(WeaponId::Colt, 10.0, false);
        assert_eq!(classify_hit(Variant::Standard, &h, &mut acc, false, false).reaction, Reaction::None);
        assert_eq!(classify_hit(Variant::Standard, &h, &mut acc, false, false).reaction, Reaction::None);
        assert_eq!(classify_hit(Variant::Standard, &h, &mut acc, false, false).reaction, Reaction::Light);
        assert_eq!(acc, 0.0);
    }

    #[test]
    fn heavy_weapons_pick_bigger_reactions() {
        let mut acc = 0.0;
        let r = |w, d| classify_hit(Variant::Standard, &hit_of(w, d, false), &mut 0.0, false, false).reaction;
        assert_eq!(r(WeaponId::Shotgun, 2.0), Reaction::Heavy); // 20 per pellet
        assert_eq!(r(WeaponId::Grenade, 1.0), Reaction::Heavy); // 25
        assert_eq!(r(WeaponId::SniperRifle, 3.0), Reaction::Medium); // 15
        assert_eq!(r(WeaponId::Javelin, 3.0), Reaction::Medium); // 11
        // alpha: sniper 15 is only light (and -5 on a body hit when flagged bullet) -> thresholds 15/20/40
        let o = classify_hit(Variant::Alpha, &hit_of(WeaponId::SniperRifle, 3.0, false), &mut acc, false, false);
        assert_eq!(o.damage, 10.0);
        assert_eq!(o.reaction, Reaction::None);
    }

    #[test]
    fn head_hits_double_for_standard_and_alpha_takes_five_less_on_body() {
        let mut acc = 0.0;
        let o = classify_hit(Variant::Standard, &hit_of(WeaponId::Colt, 3.0, true), &mut acc, false, false);
        assert_eq!(o.damage, 16.0);
        let o = classify_hit(Variant::Alpha, &hit_of(WeaponId::Colt, 3.0, true), &mut 0.0, false, false);
        assert_eq!(o.damage, 8.0);
        let o = classify_hit(Variant::Alpha, &hit_of(WeaponId::Colt, 3.0, false), &mut 0.0, false, false);
        assert_eq!(o.damage, 3.0);
    }

    #[test]
    fn life_caps() {
        let mut acc = 0.0;
        let mut h = hit_of(WeaponId::Colt, 3.0, false);
        h.flags |= hit::CAP_30;
        assert_eq!(classify_hit(Variant::Standard, &h, &mut acc, false, false).cap_fraction, Some(0.3));
        h.flags = hit::KILL;
        assert_eq!(classify_hit(Variant::Standard, &h, &mut acc, false, false).cap_fraction, Some(0.0));
    }

    #[test]
    fn fifty_hp_dies_to_seven_close_colt_hits() {
        let mut r = Raptor::new(Variant::Standard);
        let inp = RaptorInput { hits: vec![hit_of(WeaponId::Colt, 3.0, false)], ..Default::default() };
        let mut died = false;
        let mut n = 0;
        while !died {
            let ev = r.step(&inp, 0.1);
            n += 1;
            died = ev.contains(&RaptorEvent::Died);
            assert!(n < 20);
        }
        assert_eq!(n, 7); // 7 * 8 = 56 >= 50
        assert_eq!(r.hp, 0.0);
    }

    #[test]
    fn death_timeline_is_5s_then_10s() {
        let mut r = Raptor::new(Variant::Standard);
        let mut h = hit_of(WeaponId::Colt, 3.0, false);
        h.flags = hit::KILL;
        let ev = r.step(&RaptorInput { hits: vec![h], ..Default::default() }, 0.1);
        assert!(ev.contains(&RaptorEvent::Died));
        assert_eq!(r.state, RaptorState::ATerre);
        let idle = RaptorInput::default();
        r.step(&idle, 0.1);
        assert_eq!(r.state, RaptorState::Mort);
        for _ in 0..49 {
            r.step(&idle, 0.1);
        }
        assert_eq!(r.state, RaptorState::Mort);
        for _ in 0..3 {
            r.step(&idle, 0.1);
        }
        assert_eq!(r.state, RaptorState::Fade);
        let mut faded = false;
        for _ in 0..105 {
            faded |= r.step(&idle, 0.1).contains(&RaptorEvent::Faded);
        }
        assert!(faded);
    }

    #[test]
    fn spear_bleed_is_hp_max_over_60_per_second_with_floor() {
        let hp = spear_bleed(50.0, 50.0, 1, 1.0);
        assert!((hp - (50.0 - 50.0 / 60.0)).abs() < 1e-5);
        assert!((spear_bleed(50.0, 50.0, 3, 1.0) - (50.0 - 3.0 * 50.0 / 60.0)).abs() < 1e-4);
        assert_eq!(spear_bleed(2.1, 50.0, 3, 10.0), 2.0);
    }

    #[test]
    fn vision_cone_and_near_radius() {
        let o = [0.0, 0.0, 0.0];
        let f = [1.0, 0.0, 0.0];
        assert!(sees_in_cone(o, f, [9.0, 0.0, 0.0]));
        assert!(sees_in_cone(o, f, [11.0, 0.0, 0.0])); // range 10 + near radius 1
        assert!(!sees_in_cone(o, f, [11.5, 0.0, 0.0]));
        assert!(!sees_in_cone(o, f, [5.0, 5.0, 0.0])); // 45 degrees > 30
        assert!(sees_in_cone(o, f, [5.0, 2.5, 0.0])); // 26.6 degrees
        assert!(sees_in_cone(o, f, [-0.9, 0.0, 0.0])); // inside the 1 m near radius, behind
        assert!(!sees_in_cone(o, f, [-3.0, 0.0, 0.0]));
        // height is ignored (flattened)
        assert!(sees_in_cone(o, f, [5.0, 0.0, 30.0]));
        // wide cone: humans behind-left at 40 m with clear LOS are seen, without LOS not
        assert!(sees_human_wide(o, f, [-10.0, 20.0, 0.0], true)); // cos = -0.447 >= -0.653
        assert!(!sees_human_wide(o, f, [-10.0, 20.0, 0.0], false));
        assert!(!sees_human_wide(o, f, [-30.0, 5.0, 0.0], true)); // cos = -0.986
        assert!(!sees_human_wide(o, f, [101.5, 0.0, 0.0], true));
    }

    #[test]
    fn bite_start_geometry() {
        let f = [1.0, 0.0, 0.0];
        assert!(bite_start_ok(1.0, [2.9, 0.0, 0.0], f));
        assert!(!bite_start_ok(1.0, [3.1, 0.0, 0.0], f));
        assert!(bite_start_ok(1.15, [3.4, 0.0, 0.0], f)); // alpha: 3.45 m
        assert!(bite_start_ok(1.0, [2.0, 2.0, 0.0], f)); // 45 degrees, 2.83 m
        assert!(!bite_start_ok(1.0, [0.5, 2.9, 0.0], f)); // ~80 degrees, outside the 60 degree cone
        assert!(bite_start_ok(1.0, [0.5, 1.0, 0.0], f)); // d^2 = 1.25 < 2: no cone test
        assert!(!bite_start_ok(1.0, [-1.0, 0.0, 0.0], f)); // behind
        assert!(!bite_start_ok(1.0, [2.0, 0.0, 3.6], f)); // above the dz window
    }

    #[test]
    fn selection_priority_chain() {
        let ctx = SelectCtx {
            state: RaptorState::Attente,
            jack_slot: Some(0),
            current: None,
            since_target_change: 10.0,
            since_bullet_hit: 0.0,
            wait_timer: 0.0,
        };
        let jack = slot(1, flag::JACK | flag::IN_CONE, 8.0);
        let mut corpse = slot(0x21, flag::CORPSE, 3.0);
        corpse.dist_to_jack_sq = 100.0;
        let near_human = slot(2, flag::IN_CONE, 2.0);
        // Jack (rule 3) beats a nearer companion and a corpse
        assert_eq!(select_target(&[jack, corpse, near_human], &ctx), Some(0));
        // without the Jack flag the corpse (rule 6) is chosen before the cone candidate (rule 7)
        let jack2 = slot(1, flag::IN_CONE, 8.0);
        assert_eq!(select_target(&[jack2, corpse, near_human], &ctx), Some(1));
        // food beyond 50 m of Jack is ignored
        let mut far = corpse;
        far.dist_to_jack_sq = 2501.0;
        assert_eq!(select_target(&[far, near_human], &ctx), Some(1));
        // slots claimed by another raptor / unreachable are skipped by the main loop
        let mut claimed = slot(2, flag::IN_CONE | flag::CLAIMED, 1.0);
        claimed.changed = true;
        assert_eq!(select_target(&[claimed], &ctx), None);
        // a cinematic force beats everything
        let forced = slot(2, flag::CINEMATIC, 40.0);
        assert_eq!(select_target(&[jack, forced], &ctx), Some(1));
        // hurt-me needs 4.0 s since the last change
        let hurt = slot(2, flag::HURT_ME, 30.0);
        let mut c2 = ctx;
        c2.since_target_change = 3.9;
        assert_eq!(select_target(&[hurt], &c2), None);
        c2.since_target_change = 4.0;
        assert_eq!(select_target(&[hurt], &c2), Some(0));
    }

    #[test]
    fn idle_then_patrol_after_one_second() {
        let mut r = Raptor::new(Variant::Standard);
        let inp = RaptorInput::default();
        for _ in 0..9 {
            r.step(&inp, 0.1);
        }
        assert_eq!(r.state, RaptorState::Attente);
        for _ in 0..3 {
            r.step(&inp, 0.1);
        }
        assert_eq!(r.state, RaptorState::Vala);
    }

    #[test]
    fn chase_bite_grab_once_then_plain_bites_and_three_second_wait() {
        let mut r = Raptor::new(Variant::Standard);
        let mut jack = slot(1, flag::JACK | flag::IN_CONE, 2.0);
        jack.pos = [2.0, 0.0, 0.0];
        let inp = RaptorInput { perceived: vec![jack], jack_slot: Some(0), ..Default::default() };
        // frame 1 acquires the target and hesitates
        let ev = r.step(&inp, 0.1);
        assert!(ev.contains(&RaptorEvent::TargetAcquired(0)));
        assert_eq!(r.state, RaptorState::Hesite);
        // hesitation (0.75 s [G]) ends in a bite because Jack is inside 3 m
        let mut started = false;
        for _ in 0..3 {
            started |= r.step(&inp, 0.1).contains(&RaptorEvent::BiteStarted);
        }
        assert!(started);
        assert_eq!(r.state, RaptorState::Mord);
        // landed bite in the armed window, target Jack, counter 0 -> grab
        let hitf = RaptorInput { bite_frame: 15, bite_connects: true, bite_target_is_jack: true, ..inp.clone() };
        let ev = r.step(&hitf, 0.05);
        assert!(ev.contains(&RaptorEvent::GrabStarted), "{:?}", ev);
        assert_eq!(r.state, RaptorState::Grab);
        assert_eq!(r.grab, GrabCounter(1));
        // hold 4.0 s -> kill message
        let mut killed = false;
        for _ in 0..45 {
            killed |= r.step(&inp, 0.1).contains(&RaptorEvent::GrabKill);
        }
        assert!(killed);
        // the second landed bite on Jack from this raptor is a plain 6 damage heavy bite
        let mut r2 = Raptor::new(Variant::Standard);
        r2.grab = GrabCounter(1);
        r2.state = RaptorState::Mord;
        let ev = r2.step(&hitf, 0.05);
        assert!(ev.contains(&RaptorEvent::BiteHit { damage: 6.0, kind: HitKind::Heavy }), "{:?}", ev);
        // bite end sets the 3 s wait
        let done = RaptorInput { anim_done: true, ..inp.clone() };
        r2.step(&done, 0.05);
        assert_eq!(r2.state, RaptorState::Attente);
        assert!((r2.wait - POST_BITE_WAIT_S).abs() < 1e-6);
    }

    #[test]
    fn alpha_bite_is_ten() {
        let mut r = Raptor::new(Variant::Alpha);
        r.grab = GrabCounter(1);
        r.state = RaptorState::Mord;
        let hitf = RaptorInput { bite_frame: 14, bite_connects: true, bite_target_is_jack: true, ..Default::default() };
        let ev = r.step(&hitf, 0.05);
        assert!(ev.contains(&RaptorEvent::BiteHit { damage: 10.0, kind: HitKind::Heavy }));
        assert!(!bite_armed(13) && bite_armed(14) && bite_armed(19) && !bite_armed(20));
    }

    #[test]
    fn flinch_sends_the_raptor_into_the_matching_paf_state() {
        let mut r = Raptor::new(Variant::Standard);
        r.step(&RaptorInput { hits: vec![hit_of(WeaponId::Shotgun, 2.0, false)], ..Default::default() }, 0.1);
        assert_eq!(r.state, RaptorState::PafFly);
        assert_eq!(r.wait, POST_BITE_WAIT_S);
    }
}
