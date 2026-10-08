//! Ann in the Kong levels (ledger AN01-AN11) plus the creature size/stat tables recovered
//! during the species survey (X01, X03, X05-X18).
//!
//! Source: `KAnn_*@0x5edf10-0x601dc0` (Ann), `k_ETAT_grab_ANN@0x8bfc70`, `k_TEST_Grab@0x8eaaa0`,
//! `k_reflex@0x8a4360` (Kong side), `PNJ_Scolo_exec_settings@0x826150`, `PNJ_Scorpion_Init@0x4f7d30`,
//! `PNJ_Raptor_init@0x8315a0`. Every number carries `[C]` (read from the decompiled C),
//! `[L]` (inferred) or `[G]` (guess); see `spec/evidence/AN01.md` and the X*.md survey docs.
//!
//! Ann's state machine runs in AI slot 2 of the `KAnn` model; the state id lives in
//! `Ann+0x248`, the previous id in `+0x24c`, the state timer in `+0x254`. Ids below are the
//! literal values written by each `KAnn_ETAT_*` entry block [C].

use crate::Confidence;

/// `Ann+0x248` values written by `KAnn_ETAT_*` [C].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnState {
    /// `KAnn_ETAT_grabbed_KK@0x5f0bd0` writes 2: held in Kong's hand.
    GrabbedKong = 2,
    /// `KAnn_ETAT_paf@0x5f7370` writes 5: hurt reaction (anim 0xad).
    Paf = 5,
    /// `KAnn_ETAT_mort@0x5fa820` writes 6: dead.
    Mort = 6,
    /// `KAnn_ETAT_grabbed_bat@0x5fa9a0` writes 7: carried off by a big bat.
    GrabbedBat = 7,
    /// `KAnn_ETAT_follow_network@0x5fb530` writes 9: walks the follower network (default state).
    FollowNetwork = 9,
    /// `KAnn_ETAT_action@0x5fd350` writes 0xb: scripted action (javelin, hide, ...).
    Action = 0xb,
    /// `KAnn_ETAT_grabbed_trex@0x5fd090` writes 0xd: in the V-Rex's jaws.
    GrabbedTrex = 0xd,
}

impl AnnState {
    pub fn from_id(id: i32) -> Option<AnnState> {
        Some(match id {
            2 => AnnState::GrabbedKong,
            5 => AnnState::Paf,
            6 => AnnState::Mort,
            7 => AnnState::GrabbedBat,
            9 => AnnState::FollowNetwork,
            0xb => AnnState::Action,
            0xd => AnnState::GrabbedTrex,
            _ => return None,
        })
    }
}

/// `Ann+0xfe0` action selector dispatched by `KAnn_ETAT_action@0x5fd350` [C].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnAction {
    /// default branch: stand/idle and face the target (`+0x1018`)
    Default = 0,
    /// `KAnn_exec_action_teleport@0x5fe640`
    Teleport = 1,
    /// `KAnn_exec_action_javelin_get@0x5ff7f0` (only when she holds no javelin: `+0x5c == 0`)
    JavelinGet = 2,
    /// `KAnn_exec_action_javelin_throw@0x5fee00`
    JavelinThrow = 3,
    /// `KAnn_exec_action_cache@0x5fe790`
    Cache = 4,
    /// `KAnn_exec_action_javelin_set_fire@0x5ffb50` (only while holding a javelin)
    JavelinSetFire = 5,
    /// fight gesture branch (`KAnn_exec_action_interaction_attack_GEST@0x601dc0`)
    AttackGesture = 6,
    /// `KAnn_exec_action_interaction_cache@0x5ffc00`
    InteractionCache = 7,
    /// `KAnn_exec_action_colonne@0x5f1870`
    Colonne = 8,
    /// `KAnn_exec_action_sacrifice@0x6004c0`
    Sacrifice = 9,
}

impl AnnAction {
    pub fn from_id(id: i32) -> Option<AnnAction> {
        Some(match id {
            0 => AnnAction::Default,
            1 => AnnAction::Teleport,
            2 => AnnAction::JavelinGet,
            3 => AnnAction::JavelinThrow,
            4 => AnnAction::Cache,
            5 => AnnAction::JavelinSetFire,
            6 => AnnAction::AttackGesture,
            7 => AnnAction::InteractionCache,
            8 => AnnAction::Colonne,
            9 => AnnAction::Sacrifice,
            _ => return None,
        })
    }
}

// ---- Ann's animation ids (argument of ANIM_Play `fn@0x00423e30`) [C] ----
pub const ANIM_IDLE_NO_JAVELIN: i32 = 10; // 0xa, `action` hub
pub const ANIM_IDLE_WITH_JAVELIN: i32 = 0x4b;
pub const ANIM_PAF: i32 = 0xad; // KAnn_ETAT_paf@0x5f7370
pub const ANIM_DEAD: i32 = 0x3a; // KAnn_ETAT_mort@0x5fa820, grabbed_trex@0x5fd090
pub const ANIM_HELD_BY_KONG: i32 = 0x1d; // grabbed_KK@0x5f0bd0
pub const ANIM_SACRIFICE: i32 = 0xdc; // KAnn_exec_action_sacrifice@0x6004c0
pub const ANIM_JAVELIN_THROW: i32 = 0x5c; // KAnn_exec_action_javelin_throw@0x5fee00
pub const ANIM_JAVELIN_FIRE: i32 = 0x5f; // KAnn_exec_action_javelin_set_fire@0x5ffb50
pub const ANIM_JAVELIN_PICKUP: i32 = 0x32; // KAnn_exec_action_javelin_get@0x5ff7f0
/// the javelin catches fire at this clip frame of anim 0x5f [C, set_fire@0x5ffb50: `0x41 < frame`]
pub const SET_FIRE_FRAME: i32 = 0x41;

// ---- timings and distances ----
/// Delay before the first javelin throw once she has a target, seconds. [C] throw@0x5fee00
pub const FIRST_THROW_DELAY: f32 = 0.5;
/// Next throw interval is `Rand_Range(4.0, 7.0)` s. [C] throw@0x5fee00 (0x40800000, 0x40e00000)
pub const THROW_INTERVAL: (f32, f32) = (4.0, 7.0);
/// Distance (m) under which she adopts Kong's target as her own. [C] action@0x5fd350 (`< 30.0`)
pub const ADOPT_TARGET_RANGE: f32 = 30.0;
/// Distance gate for `KAnn_exec_propose_interaction`. [C] @0x5fb260 (`< 30.0`)
pub const PROPOSE_INTERACTION_RANGE: f32 = 30.0;
/// Horizontal range for being proposed as a Kong grab object: `d^2 < 49` -> 7 m. [C] @0x5f59a0
pub const KK_GRAB_RANGE: f32 = 7.0;
/// Vertical tolerance for the same test, `|dz| <= 5.0`. [C] @0x5f59a0
pub const KK_GRAB_MAX_DZ: f32 = 5.0;
/// Grab-proposal weights (`fn@0x007dd310`, lower distance^2 wins in `k_exec_selection_grab`) [C]
pub const PROPOSE_WEIGHT_KONG_GRAB: f32 = 0.001; // 0x3a83126f @0x5f59a0 / @0x5fe500
pub const PROPOSE_WEIGHT_TRANSPORT: f32 = 0.01; // 0x3c23d70a @0x5f7150
pub const PROPOSE_WEIGHT_INTERACTION: f32 = 1.0; // 0x3f800000 @0x5fb260
/// Rex-grab proposal: needs the `Ann+0xff0` timer >= 10 s and rex within sqrt(16) = 4 m. [C] @0x5fe500
pub const TREX_GRAB_MIN_TIMER: f32 = 10.0;
pub const TREX_GRAB_RANGE: f32 = 4.0;
/// Seconds-since-hit gate in `KAnn_exec_action_interaction_cache`. [C]
pub const CACHE_SPEECH_GAP: f32 = 0.5;
/// While in paf/mort the engine writes 30.0 into `Univers+0x4dd4` every frame. [C] @0x5f7370,@0x5fa820;
/// meaning (a timer that the game-over logic reads) [L].
pub const DEATH_TIMER_VALUE: f32 = 30.0;
/// Blend rates (per second, clamped to 1) pulling `Ann+0x210` toward 1.0 in the held states. [C]
pub const BLEND_RATE_HELD_KONG: f32 = 10.0;
pub const BLEND_RATE_HELD_BAT: f32 = 7.0;
pub const BLEND_RATE_HELD_TREX: f32 = 7.0;
/// Struggle speech while held by a bat repeats after `Rand_Range(8, 15)` s. [C] grabbed_bat@0x5fa9a0
pub const BAT_STRUGGLE_SPEECH: (f32, f32) = (8.0, 15.0);
/// Spider attack gesture: attack range window of the horizontal distance helper. [C] @0x601a40
pub const SPIDER_GESTURE_NEAR: f32 = 2.5;
pub const SPIDER_GESTURE_FAR: f32 = 13.0;
/// Between spider-jab clips she waits `Rand_Range(0.75, 1.25)` s. [C] @0x601a40
pub const SPIDER_JAB_GAP: (f32, f32) = (0.75, 1.25);
/// Spider jab clips cycle 0x5f, 0x63, 0x62 by `(n + 1) % 3`. [C] @0x601a40
pub const SPIDER_JAB_ANIMS: [i32; 3] = [0x5f, 0x63, 0x62];

// ---- Kong-side hand-over (k_ETAT_grab_ANN@0x8bfc70) ----
/// Put-down probe: 4 raycast steps `c = 0..3` of length 5.0 behind Kong; the first clear one is
/// where Ann is placed, step 3 is accepted without a hit test. [C]
pub const PUTDOWN_PROBES: u32 = 4;
pub const PUTDOWN_RAY_LENGTH: f32 = 5.0;
/// Clip timer windows (`Kong+0x68`) in the carry/put-down blend, seconds. [C] @0x8bfc70
pub const PICKUP_BLEND_END: f32 = 0.55;
pub const HAND_OPEN_AT: f32 = 1.15;
pub const SHOULDER_RELEASE_AT: f32 = 1.95;
pub const PUTDOWN_MIN_TIME: f32 = 0.3;

/// Inputs of `KAnn_exec_PROPOSE_KKGrab_Object@0x5f59a0`.
#[derive(Clone, Copy, Debug)]
pub struct KkGrabInput {
    /// Ann's position minus Kong's position, metres (x, y, z with z up)
    pub delta: [f32; 3],
    /// `Obj_TestFlags(Ann, 8)`: Ann already has "not grabbable" flag 8
    pub ann_flag8: bool,
    /// `Kong_IsDoing(Kong+0x190, 0xb)` is true
    pub kong_busy: bool,
    /// Ann's `+0x248 == 0xb` (she is in the action state)
    pub ann_in_action: bool,
}

/// `KAnn_exec_PROPOSE_KKGrab_Object@0x5f59a0`: the proposal weight, or `None` when she is not
/// offered to Kong. [C] conditions; the weight is the value passed to `fn@0x007dd310`.
pub fn propose_kk_grab(i: KkGrabInput) -> Option<f32> {
    if i.ann_flag8 {
        return None;
    }
    if i.delta[2].abs() > KK_GRAB_MAX_DZ {
        return None;
    }
    let d2 = i.delta[0] * i.delta[0] + i.delta[1] * i.delta[1];
    if d2 >= KK_GRAB_RANGE * KK_GRAB_RANGE {
        return None;
    }
    if i.kong_busy && !i.ann_in_action {
        return None;
    }
    Some(PROPOSE_WEIGHT_KONG_GRAB)
}

/// `KAnn_exec_propose_grab_trex@0x5fe500`: offered to the rex only after `Ann+0xff0 >= 10.0`
/// and, when the rex message carries category bit `0x10`, within 4 m horizontally. [C]
pub fn propose_trex_grab(ann_timer_ff0: f32, rex_msg_has_0x10: bool, horiz_dist_sq: f32) -> Option<f32> {
    if ann_timer_ff0 < TREX_GRAB_MIN_TIMER {
        return None;
    }
    if !rex_msg_has_0x10 {
        return None;
    }
    if horiz_dist_sq >= TREX_GRAB_RANGE * TREX_GRAB_RANGE {
        return None;
    }
    Some(PROPOSE_WEIGHT_KONG_GRAB)
}

/// `KAnn_exec_CHECK_paf@0x5f71b0` acceptance test for one incoming hit.
/// `invulnerable` = `Ann+0xfe8` (read by `fn@0x008810c0`), `global_8904_zero` = `MATH_IsZero(Univers+0x8904)`.
/// A hit word with bit `0x80` is always accepted (scripted kill). [C]
pub fn ann_hit_accepted(hit_flags: u32, invulnerable: bool, global_8904_zero: bool) -> bool {
    (hit_flags & 0x80) != 0 || (!invulnerable && global_8904_zero)
}

/// Where a hit sends her: `KAnn_ETAT_paf` ends in `mort` (`DAT_b991d4`) as soon as the anim
/// ends (`fn@0x00424940` true) [C]; the V-Rex jaw word `0x20000` goes straight to `mort` from
/// `grabbed_trex` [C, `(Ann+0x230 & 0x20000)`].
pub fn next_state_after_hit(current: AnnState, accumulated_hit_flags: u32, paf_anim_ended: bool) -> Option<AnnState> {
    match current {
        AnnState::Paf if paf_anim_ended => Some(AnnState::Mort),
        AnnState::GrabbedTrex if accumulated_hit_flags & 0x20000 != 0 => Some(AnnState::Mort),
        AnnState::GrabbedKong | AnnState::GrabbedBat | AnnState::FollowNetwork | AnnState::Action
            if accumulated_hit_flags != 0 =>
        {
            Some(AnnState::Paf)
        }
        _ => None,
    }
}

/// Javelin throw scheduler of `KAnn_exec_action_javelin_throw@0x5fee00`; `rand01` is a uniform
/// sample in [0,1). Returns the interval until the following throw. [C]
pub fn next_throw_interval(rand01: f32) -> f32 {
    THROW_INTERVAL.0 + (THROW_INTERVAL.1 - THROW_INTERVAL.0) * rand01
}

/// Javelin stock stored in the low nibble of the pick-up object's `+0x4c`: one is taken per
/// pick-up and per throw; the value 0xf means unlimited. [C] `get@0x5ff7f0`, `throw@0x5fee00`
pub fn decrement_javelin_stock(nibble: u32) -> u32 {
    let n = nibble & 0xf;
    if n != 0 && n != 0xf {
        n - 1
    } else {
        n
    }
}

/// Spider-jab clip chosen by the running counter (`Int_Mod(counter + 1, 3)`). [C] @0x601a40
pub fn spider_jab_anim(counter_after_increment: i32) -> i32 {
    SPIDER_JAB_ANIMS[counter_after_increment.rem_euclid(3) as usize]
}

/// Which put-down probe (0..=3) is used: the first whose ray does not hit; probe 3 is
/// accepted unconditionally. [C] `k_ETAT_grab_ANN@0x8bfc70` (`if (local_c == 3) goto LAB_008c0dfb`)
pub fn putdown_probe(hits: [bool; 3]) -> u32 {
    for (i, h) in hits.iter().enumerate() {
        if !*h {
            return i as u32;
        }
    }
    3
}

// ====================================================================================
// Species survey tables (X01, X03, X05-X18). Only numbers read from code appear here.
// ====================================================================================

/// One size class of `PNJ_Scolo` (`Scolo+0x0` = type). `PNJ_Scolo_exec_settings@0x826150` [C].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoloClass {
    pub kind: i32,
    /// `Scolo+0x0c`, actor scale
    pub scale: f32,
    /// `Scolo+0x1890`, life maximum (`Life_Create` max in `PNJ_Scolo_Init@0x8074d0`)
    pub life: f32,
    /// `Scolo+0x528`, movement speed factor used by Attente/Attaque/Sol (`* 1.5 * dt` etc) [L: speed]
    pub speed: f32,
    /// `Scolo+0x520`, set only when `Scolo+0x51c != 0` (value for the `+0x148` slot) [L]
    pub extra: Option<f32>,
}

/// Types 0..=4, outside Kong-ride mode (`Scolo+0x1a98 == 0`). Type 4 has life 1e14 (invulnerable).
pub const SCOLO_CLASSES: [ScoloClass; 5] = [
    ScoloClass { kind: 0, scale: 1.3, life: 16.0, speed: 2.0, extra: Some(3.0) },
    ScoloClass { kind: 1, scale: 1.0, life: 12.0, speed: 4.0, extra: Some(1.0) },
    ScoloClass { kind: 2, scale: 0.7, life: 8.0, speed: 7.0, extra: Some(1.5) },
    ScoloClass { kind: 3, scale: 3.0, life: 50.0, speed: 2.0, extra: Some(3.0) },
    ScoloClass { kind: 4, scale: 3.0, life: 1.0e14, speed: 2.0, extra: None },
];

/// `PNJ_Scorpion_Init@0x4f7d30` per-type stats: (scale range, life, +0x2a8, +0x2ac). [C]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScorpionClass {
    pub kind: i32,
    pub scale: (f32, f32),
    pub life: f32,
    pub field_2a8: i32,
    pub field_2ac: i32,
    /// `Scorpion+0x1504` before the final `* 0.5` halving that every type receives [C]
    pub field_1504: f32,
}
pub const SCORPION_CLASSES: [ScorpionClass; 3] = [
    ScorpionClass { kind: 1, scale: (0.9, 1.2), life: 30.0, field_2a8: 10, field_2ac: 4, field_1504: 0.6 },
    ScorpionClass { kind: 2, scale: (0.4, 0.6), life: 10.0, field_2a8: 4, field_2ac: 0, field_1504: 1.0 },
    ScorpionClass { kind: 3, scale: (1.8, 1.8), life: 50.0, field_2a8: 15, field_2ac: 5, field_1504: 0.4 },
];

/// Compy (species 0x16 of `PNJ_Raptor`): hp default 3.0 (override `+0x34`), vision/aggro field
/// `+0x2194` 5.0 (override `+0x38`), scale `Rand_Range(0.2, 0.35)` unless `+0x2c` is set,
/// bite = paf flags 0x1000 damage 1. [C] `PNJ_Raptor_init@0x8315a0`, `exec_bite@0x84cae0`
pub const COMPY_HP: f32 = 3.0;
pub const COMPY_FIELD_2194: f32 = 5.0;
pub const COMPY_SCALE: (f32, f32) = (0.2, 0.35);
pub const COMPY_BITE_FLAGS: u32 = 0x1000;
pub const COMPY_BITE_DAMAGE: f32 = 1.0;
/// Jack-level V-Rex species 0x10: hp 2000 (0x44fa0000), scale 1.5. [C] @0x8315a0
pub const JACK_REX_HP: f32 = 2000.0;
pub const JACK_REX_SCALE: f32 = 1.5;
/// Kong-level rex (`KT_TRACK_init@0x498640`) actor scale is set to 1.0 [C]: the "small V-Rex" of
/// the survey is this scale relative to the 1.5 of the Jack-level rex [L].
pub const KONG_REX_SCALE: f32 = 1.0;

/// Beacon classes (A17.md) per survey species, class id, weight. [C] `Msg beacon creators`
pub const BEACON_CLASS_SCOLO: u32 = 0x1a;
pub const BEACON_CLASS_SPIDER: u32 = 0x20;
pub const BEACON_CLASS_SCORPION: u32 = 0x19;
pub const BEACON_CLASS_WORM: u32 = 0x19;
pub const BEACON_CLASS_SWAMPCRAWLER: u32 = 0xf;
pub const BEACON_CLASS_SEA_MONSTER: u32 = 0x1f;
pub const BEACON_CLASS_NATIVE: u32 = 0xd;
pub const BEACON_CLASS_BIG_BAT: u32 = 0x14;

/// Damage packet `Msg_SendPaf(flags, source, 1e-4, target, damage, dir)` of each species'
/// melee, as (flags, damage). [C] cited per entry in the X*.md docs.
pub const PAF_SCOLO_BITE: (u32, f32) = (0x1001, 5.0); // also (0x1001, 10) and (3, 10) variants
pub const PAF_SCORPION_BITE: (u32, f32) = (3, 10.0);
pub const PAF_SCORPION_EAT: (u32, f32) = (0x800, 1.0);
pub const PAF_SPIDER_AFTER_ETAT: (u32, f32) = (0x203, 10.0);
pub const PAF_KSPIDER_ON_KONG: (u32, f32) = (0x20, 1.0);
pub const PAF_WORM_GRAB: (u32, f32) = (1, 10.0);
pub const PAF_SC_HIT: (u32, f32) = (0x10, 1.0);
pub const PAF_SCS_ATTACK: (u32, f32) = (0x800, 1.0);
pub const PAF_NATIVE_CHARGE: (u32, f32) = (2, 1.0);

/// Brontosaurus foot stomp: when a foot is within sqrt(9) = 3 m of the target a paf with flags
/// 0x10 is sent. [C] `pnjbronto_cb_afterblend@0x7f22f0` (`dist^2 < 9.0`)
pub const BRONTO_STOMP_RADIUS: f32 = 3.0;
pub const BRONTO_STOMP_FLAGS: u32 = 0x10;

/// Confidence of each table above, for tools that list guesses.
pub const TABLE_CONFIDENCE: Confidence = Confidence::C;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_ids_round_trip() {
        for id in [2, 5, 6, 7, 9, 0xb, 0xd] {
            assert_eq!(AnnState::from_id(id).unwrap() as i32, id);
        }
        assert!(AnnState::from_id(3).is_none());
        assert_eq!(AnnAction::from_id(3), Some(AnnAction::JavelinThrow));
        assert_eq!(AnnAction::from_id(9), Some(AnnAction::Sacrifice));
        assert!(AnnAction::from_id(10).is_none());
    }

    #[test]
    fn kong_grab_proposal_is_7m_and_weight_0_001() {
        let near = KkGrabInput { delta: [6.9, 0.0, 4.9], ann_flag8: false, kong_busy: false, ann_in_action: false };
        assert_eq!(propose_kk_grab(near), Some(0.001));
        let far = KkGrabInput { delta: [5.0, 5.0, 0.0], ..near }; // d^2 = 50 >= 49
        assert_eq!(propose_kk_grab(far), None);
        let high = KkGrabInput { delta: [1.0, 0.0, 5.1], ..near };
        assert_eq!(propose_kk_grab(high), None);
        let busy = KkGrabInput { kong_busy: true, ..near };
        assert_eq!(propose_kk_grab(busy), None);
        assert_eq!(propose_kk_grab(KkGrabInput { kong_busy: true, ann_in_action: true, ..near }), Some(0.001));
        assert_eq!(propose_kk_grab(KkGrabInput { ann_flag8: true, ..near }), None);
    }

    #[test]
    fn trex_grab_proposal_needs_10_s_and_4_m() {
        assert_eq!(propose_trex_grab(9.9, true, 1.0), None);
        assert_eq!(propose_trex_grab(10.0, true, 15.9), Some(0.001));
        assert_eq!(propose_trex_grab(10.0, true, 16.0), None);
        assert_eq!(propose_trex_grab(10.0, false, 1.0), None);
    }

    #[test]
    fn any_accepted_hit_kills_her_via_paf_then_mort() {
        assert!(ann_hit_accepted(0, false, true));
        assert!(!ann_hit_accepted(0, true, true));
        assert!(!ann_hit_accepted(0, false, false));
        assert!(ann_hit_accepted(0x80, true, false));
        assert_eq!(next_state_after_hit(AnnState::FollowNetwork, 1, false), Some(AnnState::Paf));
        assert_eq!(next_state_after_hit(AnnState::Paf, 1, false), None);
        assert_eq!(next_state_after_hit(AnnState::Paf, 1, true), Some(AnnState::Mort));
        assert_eq!(next_state_after_hit(AnnState::GrabbedTrex, 0x20000, false), Some(AnnState::Mort));
        assert_eq!(next_state_after_hit(AnnState::GrabbedTrex, 0x1, false), None);
    }

    #[test]
    fn javelin_numbers() {
        assert_eq!(next_throw_interval(0.0), 4.0);
        assert_eq!(next_throw_interval(1.0), 7.0);
        assert_eq!(FIRST_THROW_DELAY, 0.5);
        assert_eq!(decrement_javelin_stock(3), 2);
        assert_eq!(decrement_javelin_stock(0), 0);
        assert_eq!(decrement_javelin_stock(0xf), 0xf);
        assert_eq!(SET_FIRE_FRAME, 0x41);
    }

    #[test]
    fn spider_jab_cycle_and_probe() {
        assert_eq!(spider_jab_anim(3), 0x5f);
        assert_eq!(spider_jab_anim(4), 0x63);
        assert_eq!(spider_jab_anim(5), 0x62);
        assert_eq!(putdown_probe([false, true, true]), 0);
        assert_eq!(putdown_probe([true, false, true]), 1);
        assert_eq!(putdown_probe([true, true, true]), 3);
    }

    #[test]
    fn scolo_size_classes() {
        // megapede = types 0 and 3, smaller faster millipedes = types 1 and 2
        assert_eq!(SCOLO_CLASSES[0].life, 16.0);
        assert_eq!(SCOLO_CLASSES[3].life, 50.0);
        assert_eq!(SCOLO_CLASSES[1].speed, 4.0);
        assert_eq!(SCOLO_CLASSES[2].speed, 7.0);
        assert!(SCOLO_CLASSES[2].scale < SCOLO_CLASSES[1].scale);
        assert!(SCOLO_CLASSES[4].life > 1.0e13);
    }

    #[test]
    fn scorpion_and_compy_tables() {
        assert_eq!(SCORPION_CLASSES[0].life, 30.0);
        assert_eq!(SCORPION_CLASSES[2].scale.0, 1.8);
        assert_eq!(COMPY_HP, 3.0);
        assert_eq!(COMPY_BITE_FLAGS, 0x1000);
        assert!(KONG_REX_SCALE < JACK_REX_SCALE);
        assert_eq!(JACK_REX_HP, 2000.0);
        assert_eq!(BRONTO_STOMP_RADIUS * BRONTO_STOMP_RADIUS, 9.0);
        assert_eq!(PAF_NATIVE_CHARGE, (2, 1.0));
        assert_eq!(PAF_SCORPION_BITE, (3, 10.0));
    }
}
