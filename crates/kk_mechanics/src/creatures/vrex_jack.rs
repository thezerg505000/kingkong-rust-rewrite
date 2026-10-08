//! Jack-level V-Rex (species byte 0x10 of the shared `PNJ_Raptor_*` class), ledger X04 (Jack side), B01, G17,
//! A12, A13, A14, A17. Evidence: `spec/evidence/X04.md` (section "Jack level"), `B01.md`, `G17.md`, `A12.md`,
//! `A14.md`, `A17.md`; shared machinery (states, perceived-actor flags, vision cones) is in `X02.md` / `raptor.rs`.
//!
//! This is NOT the Kong-level rex (`KT_*`, `kong::vrex`): that one is a separate state machine with a life gauge.
//! The Jack-level rex has 2000 hp that Jack's weapons never touch (`check_paf@0x849910` has no damage call for
//! species 0x10), it perceives through 20-slot "perceived actor" lists and picks targets with the chain in
//! `exec_update_best_interest@0x863360`.
//!
//! Tags: [C] read from decompiled code, [L] inferred, [G] guess. Every constant names its source function.
//! Pure Rust, no engine dependency; the caller supplies perception results and animation facts.

use super::raptor::flag;

// ---------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------

/// Species byte `this+0x20` [C init@0x8315a0].
pub const SPECIES: u8 = 0x10;
/// `hp_max` `this+0x10` [C init: 0x44fa0000].
pub const HP_MAX: f32 = 2000.0;
/// Acceleration limit while speeding up, m/s per s [C select_action@0x83a950 `local_5c = 1.5`; unit [L]].
pub const ACCEL: f32 = 1.5;
/// Exponential blend rate while slowing down (k = rate*dt, clamped to 0..1) [C select_action `dt * 4.0`].
pub const DECEL_BLEND_RATE: f32 = 4.0;
/// Speed while holding Jack = (1.3 / scale-ish) * 0.33 [C select_action `local_14 = 0.33`; the divisor is `+0x2188`
/// = 1.0 for the rex (init: 0x3f800000)].
pub const GRAB_CARRY_SPEED: f32 = 1.3 * 0.33;
/// Speed of the scene-wide minimum while in FIGHT with Jack on screen [C select_action `3.0`], [L meaning].
pub const FIGHT_MIN_SPEED: f32 = 3.0;

/// Gait root speeds, m/s. They are NOT constants of the exe: `fn@0x00481eb0` (called from init) measures each gait
/// clip's root travel / duration at start-up. Values from `trex_rootmotion.json` (clips walk/run_c/run_b), mapping of
/// AI gait index -> clip is [L].
pub const GAIT_WALK: f32 = 2.2039;
pub const GAIT_RUN: f32 = 14.0009;
pub const GAIT_RUN_FAST: f32 = 17.0577;

/// Wide vision cone (cosine of the half angle, ~130.8 deg) and range, humans only, needs line of sight
/// [C check_vision@0x83f130 `0xbf275526`, `0x42c80000`].
pub const VISION_COS_WIDE: f32 = -0.653_643;
pub const VISION_RANGE_WIDE: f32 = 100.0;
/// Narrow "noticed" cone: cos 30 deg, range `+0x210c` = 8.0 for the rex (raptor 10.0), plus a 1.0 m near radius
/// [C check_vision `0x3f5db3d7`; init `0x41000000`].
pub const VISION_COS_NARROW: f32 = 0.866_025_4;
pub const VISION_RANGE_NARROW: f32 = 8.0;
pub const VISION_NEAR_RADIUS: f32 = 1.0;
/// A shot (type-0x15 ray) whose closest approach to the rex is below sqrt(64) = 8 m flags the shooter [C
/// check_shoot@0x85b0a0: compared as `local_48 < 64.0` on a SQUARED distance; the old note "range 64" was wrong].
pub const SHOT_NEAR_RADIUS_SQ: f32 = 64.0;
/// Radius of the noise event every gunshot creates (`H_callback_tir` -> `fn@0x006e3d30`, radius `0x43160000`) [C].
pub const GUNSHOT_NOISE_RADIUS: f32 = 150.0;
/// Noise flag bits stored for the shooter: heard, and heard-gunshot when the event has bit 0x2000 [C check_sound].
pub const HEARD_FLAGS: u32 = 0x800;
pub const HEARD_GUNSHOT_FLAGS: u32 = 0x804;

/// Cumulative damage one attacker must have done before it is flagged "hurt me" [C check_paf `10.0 < +0x1cdc[i]`].
pub const ATTACKER_DAMAGE_THRESHOLD: f32 = 10.0;
/// A "hurt me" target is taken only this long after the last target change [C update_best_interest `0x40800000`].
pub const HURT_RETARGET_AFTER_S: f32 = 4.0;
/// Target weight is cleared (target becomes replaceable) this long after it was set [C update_best_interest
/// `fn@0x0043c690(+0x1458, 20.0)`, species 0x10 only].
pub const TARGET_COMMIT_S: f32 = 20.0;
/// FIGHT -> SEARCH when the target has not been refreshed for this long [C ETAT_FIGHT `> 1.5`].
pub const LOST_TARGET_AGE_S: f32 = 1.5;
/// Jack-hits-with-flag-0x40 bookkeeping: a hit slows the rex by this much when the single hit is < 10 / >= 10 [C check_paf].
pub const HIT_SLOW_SMALL: f32 = 0.5;
pub const HIT_SLOW_BIG: f32 = 5.0;
pub const HIT_BIG_THRESHOLD: f32 = 10.0;
/// A single hit of at least this much always requests a re-think of the target [C check_paf `< 20.0` else `local_98 = 1`].
pub const HIT_RETHINK_DAMAGE: f32 = 20.0;
/// The slow-down / re-think reaction only runs if Jack is within 20 m or reachable in <= 3.5 s [C check_paf +
/// `fn@0x00487910` returns 0 inside 20 m, else distance / closing speed].
pub const HIT_REACT_INTERCEPT_S: f32 = 3.5;
pub const HIT_REACT_NEAR_M: f32 = 20.0;
/// Jack's accumulated bullet damage (+0x1c40) under which a Jack that is flagged as a corpse is ignored by rule 3 [C].
pub const JACK_BULLET_ACCUM_LIMIT: f32 = 60.0;

/// Bite reach from the head bone `+0x19a4`: 3.8 m, 2.5 m while the state is FIGHT [C exec_bite@0x84cae0].
pub const BITE_REACH: f32 = 3.8;
pub const BITE_REACH_IN_FIGHT: f32 = 2.5;
/// Bite hit cone cos 40 deg [C exec_bite `0x3f441b7d`].
pub const BITE_HIT_COS: f32 = 0.766_044_44;
/// Bite start (`test_bite@0x866ed0`): horizontal distance^2 < 100, dz window (-3, 11), probes cos 20 deg [C].
pub const BITE_START_DIST_SQ: f32 = 100.0;
pub const BITE_START_DZ: (f32, f32) = (-3.0, 11.0);
pub const BITE_PROBE_COS: f32 = 0.939_692_6;
/// Fallback bite-start distance^2 and cone (cos 70 deg) when no probe hits [C test_bite].
pub const BITE_FALLBACK_COS: f32 = 0.342_020_15;
pub const BITE_FALLBACK_SQ_JACK_STAND: f32 = 16.0;
pub const BITE_FALLBACK_SQ_JACK_RUN: f32 = 36.0;
/// Alternate set used when the target has an attach parent (`fn@0x00418fb0`): 64 (36 when `+0x143c` != 0); others 49 [C].
pub const BITE_ATTACHED_SQ_JACK: f32 = 64.0;
pub const BITE_ATTACHED_SQ_OTHER: f32 = 49.0;
/// Jack within this distance of the rex is grabbed at once by `exec_hard_grab@0x870c20` [C check_vision `2.5 <= +0x13f0`].
pub const HARD_GRAB_DIST: f32 = 2.5;
/// Paf the bite sends to a victim that is not grabbed: damage 1000, flags 0x4104 [C exec_bite `0x4104`, 1000].
pub const BITE_PAF_DAMAGE: f32 = 1000.0;
pub const BITE_PAF_FLAGS: u32 = 0x4104;
/// Paf the grab sends at the end of the hold: damage 1000, flags 0x4a10 (contains 0x200 = kill) [C ETAT_GRAB@0x847af0].
pub const GRAB_KILL_PAF_FLAGS: u32 = 0x4a10;
pub const GRAB_KILL_PAF_DAMAGE: f32 = 1000.0;
/// The bite does not grab a human whose life ratio is above this; it strikes [C exec_bite `0.15 < +0x171c[i]`].
pub const GRAB_LIFE_RATIO_LIMIT: f32 = 0.15;

/// Hearing/scoring: interest weights [C fn@0x0047e180 + Beacon_Create callers].
pub const INTEREST_JACK_DEFAULT: f32 = 100.0; // [L] H_TRACK_init `+0x141c = 100` is what `fn@0x0053ad10` publishes
pub const INTEREST_JACK_FORCED: f32 = 200.0; // [C] 0x43480000 when `+0x3d0 != 0` or Jack has an attach parent
pub const INTEREST_CREATURE: f32 = 150.0; // [C] 0x43160000 published by raptor/rex/compy/scolo/spider reflexes
pub const INTEREST_UNKNOWN_ACTOR: f32 = 30.0; // [C] 0x41f00000 when the actor has no beacon
pub const INTEREST_CARCASS: f32 = 10.0; // [C] update_best_interest main loop `local_80 = 10.0`

/// Eating (DEVORE@0x86a9f0): stand at 0.75*8..8 m, meat bite of 4.0 every 1.0 s [C].
pub const EAT_STAND_RANGE: f32 = 8.0;
pub const EAT_TICK_S: f32 = 1.0;
pub const EAT_BITE_DAMAGE: f32 = 4.0;
/// A corpse is only taken if Jack (when attached) is within 50 m of it, or the 50 m radius check is skipped [C 2500.0].
pub const FOOD_MAX_JACK_DIST_SQ: f32 = 2500.0;
/// When eating and Jack is attached and further than this, the rex drops the corpse [C DEVORE `50.0 < +0x13f0`].
pub const EAT_ABANDON_JACK_DIST: f32 = 50.0;

/// Roar: random delay after HESITE starts, then the roar clip (0x26 near / 0x10 far) [C HESITE `Rand_Range(0.5, 0.8)`].
pub const ROAR_DELAY_RANGE_S: (f32, f32) = (0.5, 0.8);
/// Camera shake `fn@0x007d48e0(0.01)` while the roar frame is in (10, 160) and Jack is within 25 m [C select_action].
pub const ROAR_SHAKE_RADIUS: f32 = 25.0;
pub const ROAR_SHAKE_FRAMES: (i32, i32) = (10, 160);
/// Search ring around the last known position `+0x1924` [C init 0x41a00000].
pub const SEARCH_RING_RADIUS: f32 = 20.0;
/// `+0x2194` pain/aux gauge (raptor 20) [C init 0x41200000].
pub const AUX_GAUGE: f32 = 10.0;

// ---------------------------------------------------------------------------------------------
// State ids
// ---------------------------------------------------------------------------------------------

/// State ids (`this+0x13f4`) [C: each `ETAT_*` stores its id; same table as the raptor].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RexState {
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
    AttaqueCache = 0xb,
    Fall = 0xc,
    Rode = 0xd,
    PafSlide = 0xe,
    PafFall = 0xf,
    PafFly = 0x10,
    Fade = 0x11,
    Coodbool = 0x12,
    Burn = 0x13,
}

impl RexState {
    pub fn id(self) -> u8 {
        self as u8
    }
}

// ---------------------------------------------------------------------------------------------
// Perception
// ---------------------------------------------------------------------------------------------

/// What the rex's eyes report for one actor (`check_vision@0x83f130`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sight {
    /// not perceived (out of cone/range or blocked)
    Unseen,
    /// seen: flag 0x1000000 and "changed"
    Seen,
    /// seen and inside the narrow cone: flag 0x8 as well
    SeenInCone,
}

/// Actor classes (`fn@0x007dd2d0` of the actor's beacon) as far as the rex code distinguishes them [C].
pub fn is_human_class(class: i32) -> bool {
    (1..=12).contains(&class)
}

/// Sight test. `eating` = state DEVORE (the wide-cone test is skipped, only line of sight counts). Non-humans
/// (class outside 1..12) are always "seen" with no line of sight [C check_vision]. `hidden` = Jack in script mode 2 with
/// hide sub-type 1/2 (skipped unless the rex is biting/eating anim 0xf within 4 m) [C check_vision `local_f0 == 2`].
pub fn sees(class: i32, dist: f32, cos_to_facing: f32, los_clear: bool, eating: bool, hidden: bool) -> bool {
    if !is_human_class(class) {
        return true;
    }
    if hidden {
        return false;
    }
    let in_wide = eating || (dist <= VISION_RANGE_WIDE && cos_to_facing > VISION_COS_WIDE);
    in_wide && los_clear
}

/// Narrow-cone flag 0x8: within 8 m (+1 m) and 30 degrees of the facing direction [C check_vision second `fn@0x006e58c0`].
pub fn in_narrow_cone(dist: f32, cos_to_facing: f32) -> bool {
    dist <= VISION_RANGE_NARROW + VISION_NEAR_RADIUS && (cos_to_facing >= VISION_COS_NARROW || dist <= VISION_NEAR_RADIUS)
}

/// `check_shoot`: does a shot whose closest approach to the rex is `miss_dist` flag the shooter (flag 0x2)? [C]
pub fn shot_flags_shooter(miss_dist: f32) -> bool {
    miss_dist * miss_dist < SHOT_NEAR_RADIUS_SQ
}

/// `check_sound`: flags the noise source when it is inside the event's radius [C]; `gunshot` = event bit 0x2000.
pub fn noise_flags(dist: f32, event_radius: f32, gunshot: bool) -> u32 {
    if dist < event_radius {
        if gunshot {
            HEARD_GUNSHOT_FLAGS
        } else {
            HEARD_FLAGS
        }
    } else {
        0
    }
}

// ---------------------------------------------------------------------------------------------
// Incoming damage (G17)
// ---------------------------------------------------------------------------------------------

/// Result of one hit as seen by `check_paf` for the rex.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitEffect {
    /// new `hp` (only the exact-flag caps ever change it)
    pub hp: f32,
    /// flags to OR into the attacker's perceived slot
    pub slot_flags: u32,
    /// subtract this from the current speed (m/s)
    pub slow: f32,
    /// raise the "re-think target" request (`+0x1440 = 1`, leads to HESITE)
    pub rethink: bool,
}

/// Per-attacker accumulator of damage (`+0x1cdc[i]`) and Jack's bullet total (`+0x1c40`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitLedger {
    pub from_attacker: f32,
    pub jack_bullet_total: f32,
}

/// `check_paf@0x849910` for species 0x10.
/// `flags` is the paf flag word (`fn@0x0051f190`), `dmg` the damage (`fn@0x0051f110`), `attacker_is_jack` the sender.
/// `jack_near` = Jack inside 20 m or reachable in <= 3.5 s (`fn@0x00487910 <= 3.5`), `moving` = `+0x1c30 != 0`.
/// HP is only touched by the exact flag words 0x200 (cap 0), 0x100 (cap 15 %), 0x80 (cap 30 %): level scripts, not
/// Jack's guns [C]; bullets (flag word 0x44 = bit 0x40 + bit 0x4) never change hp.
pub fn on_hit(hp: f32, flags: u32, dmg: f32, attacker_is_jack: bool, jack_near: bool, moving: bool, ledger: &mut HitLedger) -> HitEffect {
    let mut out = HitEffect { hp, slot_flags: 0, slow: 0.0, rethink: false };
    match flags {
        0x200 => {
            out.hp = hp.min(0.0);
            return out;
        }
        0x100 => {
            out.hp = hp.min(0.15 * HP_MAX);
            return out;
        }
        0x80 => {
            out.hp = hp.min(0.3 * HP_MAX);
            return out;
        }
        _ => {}
    }
    if flags & 0x10 != 0 {
        return out;
    }
    ledger.from_attacker += dmg;
    if attacker_is_jack && flags & 0x40 != 0 {
        ledger.jack_bullet_total += dmg;
    }
    // slot flagging: zero damage, or 0x400 hits, or cumulative damage above the threshold
    let heavy = flags & 0x400 != 0;
    if dmg == 0.0 || heavy || ledger.from_attacker > ATTACKER_DAMAGE_THRESHOLD {
        out.slot_flags = flag::HURT_ME | if attacker_is_jack { flag::JACK } else { 0 } | if heavy { flag::BULLET } else { 0 };
    }
    if jack_near {
        if moving {
            out.slow = if dmg < HIT_BIG_THRESHOLD { HIT_SLOW_SMALL } else { HIT_SLOW_BIG };
        }
        out.rethink = dmg >= HIT_RETHINK_DAMAGE;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Target selection (A12, A13, A14, A17)
// ---------------------------------------------------------------------------------------------

/// One slot of the 20-entry perceived-actor list as the rex sees it (`fn@0x0047e180` fills it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// `+0x1620[i]`: 1 Jack, 2..12 other humans, 0xe raptor, 0x10 rex, 0x16 compy, 0x14 big bat, 0x1a scolo, 0x21 food plant [C users]
    pub class: i32,
    pub flags: u32,
    /// `+0x1674[i]` distance to the rex
    pub dist: f32,
    /// `+0x16c8[i]` interest weight
    pub interest: f32,
    /// `+0x147c[i] != 0`
    pub seen: bool,
    /// `+0x14d0[i] != 0` reachable on the nav grid
    pub reachable: bool,
    /// `fn@0x00470690(+0x1524[i])`: the slot's territory is in the rex's territory mask `+0x8f0`
    pub in_territory: bool,
    /// distance^2 between Jack and this slot's actor (corpse range test)
    pub dist_to_jack_sq: f32,
}

/// Inputs of the chain that are not slot data.
#[derive(Clone, Copy, Debug)]
pub struct SelectCtx {
    pub state: RexState,
    pub jack_slot: Option<usize>,
    pub current: Option<usize>,
    /// weight of the current target (`Life_GetMax(+0x1460)`)
    pub current_weight: f32,
    pub since_target_set: f32,
    /// `+0x1450`: set the first time a target is acquired (the rex then knows every slot, seen or not)
    pub alerted: bool,
    /// `+0x3d0`: attack Jack regardless of territory
    pub attack_jack_anywhere: bool,
    /// Jack has an attach parent (`fn@0x00418fb0(+0x13ec)`)
    pub jack_attached: bool,
    /// `+0x1aa4` wait timer (always 0 for the rex: `check_requin` is skipped for species 0x10)
    pub wait_timer: f32,
}

fn nearest(slots: &[Slot], pred: impl Fn(&Slot) -> bool) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, s) in slots.iter().enumerate() {
        if pred(s) && best.map_or(true, |b| s.dist < slots[b].dist) {
            best = Some(i);
        }
    }
    best
}

/// Interest weight of a slot as `fn@0x0047e180` stores it: Jack 200 when `+0x3d0` or attached else the beacon weight.
pub fn slot_interest(class: i32, beacon_weight: f32, attack_jack_anywhere: bool, jack_attached: bool) -> f32 {
    if class == 1 && (attack_jack_anywhere || jack_attached) {
        INTEREST_JACK_FORCED
    } else {
        beacon_weight
    }
}

/// The rex's target chain (`update_best_interest@0x863360`), rules that are readable are ported in code order:
/// 1 cinematic-forced (0x8000); 3 Jack when flagged 0x1 (not while GRAB); 4 "hurt me" (0x20) after 4 s; 6 nearest
/// reachable corpse; 7 nearest slot in the narrow cone (Jack, or non-humans after 4 s); then the weighted loop.
/// Lost / not ported: rule 2 (current target flagged hiding), 5 (other raptor, needs `+0x10` territory flag),
/// 8-10 (link, wound-ratio, pack). First match wins; `None` = keep the current target.
pub fn select_target(slots: &[Slot], ctx: &SelectCtx) -> Option<usize> {
    use RexState::*;
    // 1
    if let Some(i) = slots.iter().position(|s| s.flags & flag::CINEMATIC != 0) {
        return Some(i);
    }
    // 3: Jack flagged "shot me" (not while holding someone)
    if let Some(j) = ctx.jack_slot {
        if j < slots.len() && slots[j].flags & flag::JACK != 0 && ctx.state != Grab {
            return Some(j);
        }
    }
    // 4: whoever hurt it, after 4 s on the current target
    if !matches!(ctx.state, Mord | Grab | Coodbool) && ctx.since_target_set >= HURT_RETARGET_AFTER_S {
        if let Some(i) = slots.iter().position(|s| s.flags & flag::HURT_ME != 0) {
            return Some(i);
        }
    }
    // 6: corpses / food
    if !matches!(ctx.state, Vala | Mord | Grab | Coodbool) && ctx.wait_timer == 0.0 {
        if let Some(i) = nearest(slots, |s| {
            s.reachable
                && s.flags & flag::CORPSE != 0
                && s.flags & flag::FAR_CORPSE == 0
                && (!ctx.jack_attached || s.dist_to_jack_sq <= FOOD_MAX_JACK_DIST_SQ)
        }) {
            return Some(i);
        }
    }
    // 7: nearest slot in the narrow cone; Jack always, non-humans after 4 s, other humans never
    if !matches!(ctx.state, Mord | Grab | Devore | Coodbool) {
        if let Some(i) = nearest(slots, |s| {
            s.flags & flag::IN_CONE != 0
                && s.flags & flag::MAIN_LOOP_EXCLUDE == 0
                && s.reachable
                && s.seen
                && (s.class == 1 || (!is_human_class(s.class) && ctx.since_target_set >= HURT_RETARGET_AFTER_S))
        }) {
            return Some(i);
        }
    }
    // weighted loop: highest interest wins; a new target must beat the current weight strictly
    let mut best = ctx.current.map(|_| ctx.current_weight);
    let mut pick: Option<usize> = None;
    for (i, s) in slots.iter().enumerate() {
        if s.flags & flag::EATEN != 0 {
            continue;
        }
        if !(ctx.alerted || s.seen || s.flags & 0x100_0c00 != 0) {
            continue;
        }
        let mut score = s.interest;
        if s.flags & flag::CORPSE != 0 {
            if !s.reachable {
                continue;
            }
            score = INTEREST_CARCASS;
        } else if s.flags & 0x7100 != 0 {
            score = 0.0;
        } else if !(ctx.attack_jack_anywhere && s.class == 1) && s.flags & flag::HIDING == 0 && !s.in_territory {
            score = 0.0;
        }
        let better = match best {
            None => true, // no current target: `-1.0 <= score`
            Some(b) => {
                if ctx.current.is_none() {
                    b <= score
                } else {
                    b < score
                }
            }
        };
        if better {
            best = Some(score);
            pick = Some(i);
        }
    }
    pick.or(ctx.current)
}

// ---------------------------------------------------------------------------------------------
// Bite and kill (X04)
// ---------------------------------------------------------------------------------------------

/// What a landed bite does to its victim.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BiteOutcome {
    /// no grab: a paf of `damage` with `flags` is sent to the victim
    Strike { damage: f32, flags: u32 },
    /// grab hand-shake -> state GRAB; the kill paf follows at the end of the hold
    Grab,
}

/// `exec_bite@0x84cae0`, species 0x10 branch. `life_ratio` = the victim's published life fraction (`+0x171c[i]`):
/// Jack publishes 1.0 healthy, 0.3 after the 2nd wound value, 0.15 when wounded, 0 dead (`fn@0x0053ad10`).
pub fn bite_outcome(class: i32, slot_flags: u32, has_link: bool, life_ratio: f32, victim_attached: bool) -> BiteOutcome {
    if !is_human_class(class) {
        return BiteOutcome::Grab; // raptors etc. are always grabbed
    }
    if slot_flags & flag::CINEMATIC != 0 || has_link {
        return BiteOutcome::Grab;
    }
    if life_ratio > GRAB_LIFE_RATIO_LIMIT && !victim_attached {
        BiteOutcome::Strike { damage: BITE_PAF_DAMAGE, flags: BITE_PAF_FLAGS }
    } else {
        BiteOutcome::Grab
    }
}

/// Jack's published life fraction from his wound value `G+0x1bb4+4p` [C fn@0x0053ad10]: 0 -> 1.0, 2 -> 0.3, 3 -> 0,
/// anything else (1.0) -> 0.15.
pub fn jack_life_ratio(wound_value: f32) -> f32 {
    if wound_value == 0.0 {
        1.0
    } else if wound_value == 2.0 {
        0.3
    } else if wound_value == 3.0 {
        0.0
    } else {
        0.15
    }
}

/// Bite reach by state [C exec_bite].
pub fn bite_reach(state: RexState) -> f32 {
    if state == RexState::Fight {
        BITE_REACH_IN_FIGHT
    } else {
        BITE_REACH
    }
}

/// Bite hit test on the target's capsule centre: within reach and inside the 40 degree cone [C exec_bite].
pub fn bite_hits(state: RexState, dist: f32, cos_to_head_axis: f32) -> bool {
    dist <= bite_reach(state) && cos_to_head_axis > BITE_HIT_COS
}

/// Fallback bite start (no probe hit): squared distance limit and 70 degree cone [C test_bite].
pub fn bite_fallback_ok(target_is_jack: bool, attached: bool, running: bool, dist_sq: f32, cos_to_facing: f32) -> bool {
    let limit = if attached {
        if target_is_jack {
            if running { 36.0 } else { BITE_ATTACHED_SQ_JACK }
        } else {
            BITE_ATTACHED_SQ_OTHER
        }
    } else if target_is_jack && running {
        BITE_FALLBACK_SQ_JACK_RUN
    } else {
        BITE_FALLBACK_SQ_JACK_STAND
    };
    dist_sq < limit && cos_to_facing > BITE_FALLBACK_COS
}

/// `exec_hard_grab` trigger: Jack inside 2.5 m (and reachable, not already held) [C check_vision].
pub fn hard_grab(jack_dist: f32, holding: bool, jack_reachable: bool) -> bool {
    jack_dist < HARD_GRAB_DIST && !holding && jack_reachable
}

// ---------------------------------------------------------------------------------------------
// Movement
// ---------------------------------------------------------------------------------------------

/// One step of the speed smoothing in `select_action@0x83a950` [C]: speeding up adds at most `accel*dt`; slowing down
/// blends exponentially with k = 4*dt (clamped 0..1).
pub fn speed_step(cur: f32, target: f32, dt: f32, accel: f32) -> f32 {
    if cur <= target {
        cur + (accel * dt).min(target - cur)
    } else {
        let k = (DECEL_BLEND_RATE * dt).clamp(0.0, 1.0);
        (1.0 - k) * cur + k * target
    }
}

/// Target speed of the chase: walk gait unless the run flag `+0x1a58` is set; the run flag is set for Jack and for
/// attached targets, never for other prey [C ETAT_FIGHT `1a58 = 1 if target is Jack or has a parent`].
pub fn chase_target_speed(run_flag: bool, jack_attached: bool) -> f32 {
    if !run_flag {
        GAIT_WALK
    } else if jack_attached {
        GAIT_RUN_FAST
    } else {
        GAIT_RUN
    }
}

/// `TrigExec_RexChaseRange@0x78b900`: writes `(flag=1, accel, unused)` to the rex instance. The accel replaces 1.5 for ONE
/// speed update (select_action clears the flag when it consumes it) and only while a target exists [C].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChaseOverride {
    pub armed: bool,
    pub accel: f32,
}

impl ChaseOverride {
    pub fn set(&mut self, accel: f32) {
        self.armed = true;
        self.accel = accel;
    }
    /// accel to use this frame; consumes the override when a target exists.
    pub fn take(&mut self, has_target: bool) -> f32 {
        if self.armed && has_target {
            self.armed = false;
            self.accel
        } else {
            ACCEL
        }
    }
}

// ---------------------------------------------------------------------------------------------
// State transitions of the free (non-scripted) loop
// ---------------------------------------------------------------------------------------------

/// How the current target looks to the state hub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    None,
    Prey,
    Corpse,
    /// flag 0x200: attacker hiding (ambush state)
    Hiding,
    /// flag 0x2000: cannot be reached on the nav grid
    Unreachable,
}

#[derive(Clone, Copy, Debug)]
pub struct HubInput {
    pub hp: f32,
    pub script_goto_vala: bool, // +0x900 == 0 or +0x1fa0 != 0 [C]
    pub retarget_request: bool, // +0x1440
    pub target: TargetKind,
    /// `now - last_seen(target)` (`Life_GetWoundRatio(+0x1460)`)
    pub since_seen: f32,
    pub bite_started: bool,  // test_bite returned true
    pub hard_grab: bool,
    pub eat_done: bool,
    pub roar_finished: bool, // HESITE: 1fc8 == 0 && 2914 == 0 && timer != 0
}

/// Transition of the hub states ATTENTE / HESITE / SEARCH / FIGHT / DEVORE as read from the five `ETAT_*` functions [C order;
/// semantics L]. Returns the next state (or `state` to stay).
pub fn next_state(state: RexState, i: &HubInput) -> RexState {
    use RexState::*;
    if i.hard_grab && !matches!(state, Grab | Mord | Mort) {
        return Grab;
    }
    if i.hp <= 0.0 && !matches!(state, Mort | Fade) {
        return Mort; // [L] reflex: hp 0 -> death states
    }
    if i.script_goto_vala {
        return Vala;
    }
    match state {
        Attente => {
            if i.retarget_request {
                Hesite
            } else {
                match i.target {
                    TargetKind::None => Attente,
                    TargetKind::Hiding => AttaqueCache,
                    TargetKind::Unreachable => Vala,
                    TargetKind::Corpse => Devore,
                    TargetKind::Prey => {
                        if i.since_seen <= LOST_TARGET_AGE_S {
                            Search
                        } else {
                            Fight
                        }
                    }
                }
            }
        }
        Hesite => {
            if matches!(i.target, TargetKind::Corpse) {
                Devore
            } else if i.roar_finished {
                match i.target {
                    TargetKind::Unreachable => Vala,
                    _ => Fight, // [L] returns to the state it came from (`+0x140c`); FIGHT is the usual one
                }
            } else {
                Hesite
            }
        }
        Fight => {
            if i.retarget_request {
                Hesite
            } else if i.bite_started {
                Mord
            } else {
                match i.target {
                    TargetKind::None => Attente,
                    TargetKind::Corpse => Devore,
                    TargetKind::Hiding => AttaqueCache,
                    TargetKind::Unreachable => Vala,
                    TargetKind::Prey => {
                        if i.since_seen <= LOST_TARGET_AGE_S {
                            Fight
                        } else {
                            Search
                        }
                    }
                }
            }
        }
        Search => {
            if i.retarget_request {
                Hesite
            } else if i.bite_started {
                Mord
            } else {
                match i.target {
                    TargetKind::None => Attente,
                    TargetKind::Corpse => Devore,
                    _ => {
                        if i.since_seen <= 0.0 {
                            Fight
                        } else {
                            Search
                        }
                    }
                }
            }
        }
        Devore => {
            if i.retarget_request || i.eat_done || !matches!(i.target, TargetKind::Corpse) {
                Attente
            } else {
                Devore
            }
        }
        Mord => Attente, // [L] bite clip ends -> ATTENTE (or GRAB when a link exists)
        other => other,
    }
}

/// Feeding: meat points taken from the corpse (`+0x38` of `KCadavre`) per elapsed time, in whole 1 s ticks [C DEVORE].
pub fn eat_ticks(elapsed: f32) -> f32 {
    (elapsed / EAT_TICK_S).floor() * EAT_BITE_DAMAGE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(class: i32, flags: u32, dist: f32) -> Slot {
        Slot {
            class,
            flags,
            dist,
            interest: INTEREST_CREATURE,
            seen: true,
            reachable: true,
            in_territory: true,
            dist_to_jack_sq: 0.0,
        }
    }

    fn ctx() -> SelectCtx {
        SelectCtx {
            state: RexState::Attente,
            jack_slot: Some(0),
            current: None,
            current_weight: 0.0,
            since_target_set: 100.0,
            alerted: false,
            attack_jack_anywhere: false,
            jack_attached: false,
            wait_timer: 0.0,
        }
    }

    #[test]
    fn species_and_hp_match_init() {
        assert_eq!(SPECIES, 0x10);
        assert_eq!(HP_MAX, 2000.0);
        assert_eq!(f32::from_bits(0x44fa0000), HP_MAX);
        assert_eq!(f32::from_bits(0x3fc00000), ACCEL);
        assert_eq!(f32::from_bits(0x41000000), VISION_RANGE_NARROW);
        assert_eq!(f32::from_bits(0x41a00000), SEARCH_RING_RADIUS);
        assert_eq!(f32::from_bits(0x41200000), AUX_GAUGE);
    }

    #[test]
    fn hexfloat_constants() {
        assert_eq!(f32::from_bits(0xbf275526), VISION_COS_WIDE);
        assert_eq!(f32::from_bits(0x42c80000), VISION_RANGE_WIDE);
        assert_eq!(f32::from_bits(0x3f5db3d7), VISION_COS_NARROW);
        assert_eq!(f32::from_bits(0x3f441b7d), BITE_HIT_COS);
        assert!((BITE_PROBE_COS - 20.0f32.to_radians().cos()).abs() < 1e-6); // 0.9396926 = cos 20 deg
        assert!((BITE_FALLBACK_COS - 70.0f32.to_radians().cos()).abs() < 1e-6);
        assert_eq!(f32::from_bits(0x43160000), GUNSHOT_NOISE_RADIUS);
        assert_eq!(f32::from_bits(0x43480000), INTEREST_JACK_FORCED);
        assert_eq!(f32::from_bits(0x41f00000), INTEREST_UNKNOWN_ACTOR);
    }

    #[test]
    fn shot_radius_is_squared() {
        // the old note read 64 as metres; it is compared against a squared distance
        assert!(shot_flags_shooter(7.9));
        assert!(!shot_flags_shooter(8.1));
    }

    #[test]
    fn gunshot_is_heard_within_150_m() {
        assert_eq!(noise_flags(149.0, GUNSHOT_NOISE_RADIUS, true), 0x804);
        assert_eq!(noise_flags(151.0, GUNSHOT_NOISE_RADIUS, true), 0);
        assert_eq!(noise_flags(10.0, 20.0, false), 0x800);
    }

    #[test]
    fn humans_need_los_and_the_wide_cone_nonhumans_do_not() {
        assert!(!sees(1, 50.0, 0.0, false, false, false)); // no line of sight
        assert!(sees(1, 99.0, 0.0, true, false, false));
        assert!(!sees(1, 101.0, 0.0, true, false, false)); // beyond 100 m
        assert!(!sees(1, 10.0, -0.7, true, false, false)); // behind (cos < -0.6536)
        assert!(sees(1, 10.0, -0.7, true, true, false)); // eating: cone skipped
        assert!(sees(0xe, 500.0, -1.0, false, false, false)); // raptors: always
        assert!(!sees(1, 10.0, 1.0, true, false, true)); // hidden Jack
    }

    #[test]
    fn bullets_never_hurt_the_rex_but_scripts_can() {
        let mut l = HitLedger::default();
        let e = on_hit(2000.0, 0x44, 8.0, true, true, true, &mut l);
        assert_eq!(e.hp, 2000.0);
        let e = on_hit(2000.0, 0x200, 0.0, false, false, false, &mut l);
        assert_eq!(e.hp, 0.0);
        let e = on_hit(2000.0, 0x100, 0.0, false, false, false, &mut l);
        assert_eq!(e.hp, 300.0);
        let e = on_hit(2000.0, 0x80, 0.0, false, false, false, &mut l);
        assert_eq!(e.hp, 600.0);
    }

    #[test]
    fn jack_is_flagged_after_more_than_10_damage() {
        let mut l = HitLedger::default();
        let e = on_hit(2000.0, 0x44, 6.0, true, false, false, &mut l);
        assert_eq!(e.slot_flags, 0);
        let e = on_hit(2000.0, 0x44, 6.0, true, false, false, &mut l); // total 12 > 10
        assert_eq!(e.slot_flags & (flag::HURT_ME | flag::JACK), flag::HURT_ME | flag::JACK);
        assert_eq!(l.jack_bullet_total, 12.0);
    }

    #[test]
    fn hit_slow_and_rethink() {
        let mut l = HitLedger::default();
        let e = on_hit(2000.0, 0x44, 5.0, true, true, true, &mut l);
        assert_eq!(e.slow, 0.5);
        assert!(!e.rethink);
        let e = on_hit(2000.0, 0x44, 25.0, true, true, true, &mut l);
        assert_eq!(e.slow, 5.0);
        assert!(e.rethink);
        let e = on_hit(2000.0, 0x44, 25.0, true, false, true, &mut l); // Jack far: no reaction
        assert_eq!((e.slow, e.rethink), (0.0, false));
    }

    #[test]
    fn corpse_beats_a_calm_jack() {
        let slots = [slot(1, flag::IN_CONE | flag::SEEN, 20.0), slot(0x21, flag::CORPSE, 40.0)];
        assert_eq!(select_target(&slots, &ctx()), Some(1));
    }

    #[test]
    fn shooting_the_rex_overrides_bait() {
        let slots = [slot(1, flag::JACK | flag::HURT_ME, 60.0), slot(0x21, flag::CORPSE, 10.0)];
        assert_eq!(select_target(&slots, &ctx()), Some(0));
    }

    #[test]
    fn jack_is_not_retargeted_while_holding_someone() {
        let slots = [slot(1, flag::JACK, 3.0), slot(0x21, flag::CORPSE, 10.0)];
        let mut c = ctx();
        c.state = RexState::Grab;
        // rules 3 and 6 are off in GRAB: the weighted loop decides, the corpse scores 10 against the creature weight 150
        assert_eq!(select_target(&slots, &c), Some(0));
    }

    #[test]
    fn hurt_me_waits_for_four_seconds() {
        let slots = [slot(0xe, 0, 20.0), slot(2, flag::HURT_ME, 30.0)];
        let mut c = ctx();
        c.since_target_set = 3.9;
        c.current = Some(0);
        c.current_weight = 1000.0; // nothing can win the weighted loop
        assert_eq!(select_target(&slots, &c), Some(0)); // before 4 s the attacker is not forced
        c.since_target_set = 4.0;
        assert_eq!(select_target(&slots, &c), Some(1));
    }

    #[test]
    fn nearest_corpse_wins_and_far_corpses_are_skipped_when_jack_attached() {
        let mut a = slot(0x21, flag::CORPSE, 30.0);
        a.dist_to_jack_sq = 3000.0;
        let mut b = slot(0xe, flag::CORPSE, 60.0);
        b.dist_to_jack_sq = 100.0;
        let mut c = ctx();
        assert_eq!(select_target(&[a, b], &c), Some(0)); // Jack not attached: distance to Jack ignored
        c.jack_attached = true;
        assert_eq!(select_target(&[a, b], &c), Some(1));
    }

    #[test]
    fn rex_prefers_raptors_to_jack_by_interest() {
        // weighted loop only (no cone, no flags): raptor 150 > Jack 100; both in territory
        let mut jack = slot(1, 0, 40.0);
        jack.interest = INTEREST_JACK_DEFAULT;
        let raptor = slot(0xe, 0, 50.0);
        let mut c = ctx();
        c.alerted = true;
        assert_eq!(select_target(&[jack, raptor], &c), Some(1));
        c.attack_jack_anywhere = true; // Jack weight becomes 200 via slot_interest
        let mut jack2 = jack;
        jack2.interest = slot_interest(1, INTEREST_JACK_DEFAULT, true, false);
        assert_eq!(select_target(&[jack2, raptor], &c), Some(0));
    }

    #[test]
    fn out_of_territory_prey_scores_zero() {
        let mut far = slot(0xe, 0, 20.0);
        far.in_territory = false;
        let mut cur = slot(0xe, 0, 80.0);
        cur.interest = 0.0;
        let mut c = ctx();
        c.alerted = true;
        c.current = Some(1);
        c.current_weight = 5.0;
        // `far` scores 0 (outside the territory mask) and does not beat the current weight 5
        assert_eq!(select_target(&[far, cur], &c), Some(1));
        let mut inside = far;
        inside.in_territory = true; // same prey inside the territory scores 150 and takes over
        assert_eq!(select_target(&[inside, cur], &c), Some(0));
    }

    #[test]
    fn bite_is_a_strike_on_a_healthy_jack_and_a_grab_on_a_wounded_one() {
        assert_eq!(
            bite_outcome(1, 0, false, jack_life_ratio(0.0), false),
            BiteOutcome::Strike { damage: 1000.0, flags: 0x4104 }
        );
        assert_eq!(bite_outcome(1, 0, false, jack_life_ratio(1.0), false), BiteOutcome::Grab);
        assert_eq!(bite_outcome(1, 0, false, jack_life_ratio(2.0), false), BiteOutcome::Strike { damage: 1000.0, flags: 0x4104 });
        assert_eq!(bite_outcome(0xe, 0, false, 1.0, false), BiteOutcome::Grab);
        assert_eq!(GRAB_KILL_PAF_FLAGS & 0x200, 0x200); // the kill paf carries the 0x200 "dead" bit
    }

    #[test]
    fn jack_life_ratio_table() {
        assert_eq!(jack_life_ratio(0.0), 1.0);
        assert_eq!(jack_life_ratio(1.0), 0.15);
        assert_eq!(jack_life_ratio(2.0), 0.3);
        assert_eq!(jack_life_ratio(3.0), 0.0);
    }

    #[test]
    fn bite_geometry() {
        assert!(bite_hits(RexState::Mord, 3.7, 0.8));
        assert!(!bite_hits(RexState::Fight, 3.0, 0.8)); // reach is 2.5 in FIGHT
        assert!(bite_hits(RexState::Fight, 2.4, 0.8));
        assert!(!bite_hits(RexState::Mord, 3.0, 0.7)); // outside 40 degrees
        assert!(hard_grab(2.4, false, true));
        assert!(!hard_grab(2.6, false, true));
        assert!(!hard_grab(1.0, true, true));
        assert!(bite_fallback_ok(true, false, false, 15.0, 0.4));
        assert!(!bite_fallback_ok(true, false, false, 17.0, 0.4));
        assert!(bite_fallback_ok(true, false, true, 30.0, 0.4)); // running: 36
    }

    #[test]
    fn speed_smoothing() {
        // accelerates by 1.5 m/s per second, brakes exponentially
        let s = speed_step(0.0, GAIT_RUN, 1.0, ACCEL);
        assert_eq!(s, 1.5);
        let s = speed_step(14.0, 0.0, 0.1, ACCEL); // k = 0.4
        assert!((s - 8.4).abs() < 1e-4);
        assert_eq!(speed_step(2.0, 2.2, 1.0, ACCEL), 2.2);
        assert_eq!(chase_target_speed(false, false), GAIT_WALK);
        assert_eq!(chase_target_speed(true, false), GAIT_RUN);
    }

    #[test]
    fn rex_chase_range_override_is_one_shot() {
        let mut o = ChaseOverride::default();
        o.set(6.0);
        assert_eq!(o.take(false), ACCEL); // no target: not consumed
        assert_eq!(o.take(true), 6.0);
        assert_eq!(o.take(true), ACCEL);
    }

    #[test]
    fn state_hub() {
        let base = HubInput {
            hp: HP_MAX,
            script_goto_vala: false,
            retarget_request: false,
            target: TargetKind::None,
            since_seen: 0.0,
            bite_started: false,
            hard_grab: false,
            eat_done: false,
            roar_finished: false,
        };
        assert_eq!(next_state(RexState::Attente, &base), RexState::Attente);
        let notice = HubInput { retarget_request: true, target: TargetKind::Prey, ..base };
        assert_eq!(next_state(RexState::Attente, &notice), RexState::Hesite);
        let roared = HubInput { target: TargetKind::Prey, roar_finished: true, ..base };
        assert_eq!(next_state(RexState::Hesite, &roared), RexState::Fight);
        let lost = HubInput { target: TargetKind::Prey, since_seen: 1.6, ..base };
        assert_eq!(next_state(RexState::Fight, &lost), RexState::Search);
        let kept = HubInput { target: TargetKind::Prey, since_seen: 1.5, ..base };
        assert_eq!(next_state(RexState::Fight, &kept), RexState::Fight);
        let corpse = HubInput { target: TargetKind::Corpse, ..base };
        assert_eq!(next_state(RexState::Fight, &corpse), RexState::Devore);
        let grab = HubInput { hard_grab: true, target: TargetKind::Prey, ..base };
        assert_eq!(next_state(RexState::Fight, &grab), RexState::Grab);
        let bite = HubInput { bite_started: true, target: TargetKind::Prey, ..base };
        assert_eq!(next_state(RexState::Fight, &bite), RexState::Mord);
        let found = HubInput { target: TargetKind::Prey, since_seen: 0.0, ..base };
        assert_eq!(next_state(RexState::Search, &found), RexState::Fight);
        assert_eq!(RexState::Devore.id(), 10);
        assert_eq!(RexState::Grab.id(), 8);
        assert_eq!(RexState::Mord.id(), 7);
    }

    #[test]
    fn eating_rate() {
        assert_eq!(eat_ticks(0.9), 0.0);
        assert_eq!(eat_ticks(3.2), 12.0);
        assert_eq!(EAT_BITE_DAMAGE, 4.0);
    }

    #[test]
    fn narrow_cone() {
        assert!(in_narrow_cone(8.5, 0.9));
        assert!(!in_narrow_cone(8.5, 0.8));
        assert!(!in_narrow_cone(12.0, 1.0));
        assert!(in_narrow_cone(0.9, -1.0)); // inside the 1 m near radius
    }
}
