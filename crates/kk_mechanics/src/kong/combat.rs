//! Kong's basic melee combat (ledger KC01-KC09, KC14-KC18): the combat-phase machine
//! (`Kong+0x1adc`), the blow table (damage + hit class per attack), dodge/invulnerability, the
//! hit receiver, the held-object swing chain and the thrown-creature impact numbers.
//!
//! Sources (all `KingKong8.exe`, PC Gamer's Edition):
//! * `k_ETAT_main@0x89a120` (the whole function is 0x89a120..0x8a4100; the shared KB export of it
//!   failed, so it was read from the disassembly), phase starters `fn@0x00884b00/d40/de0/e70/f00/f60`,
//!   `fn@0x00885010`, `fn@0x00886b00`, `fn@0x008851a0`;
//! * `k_exec_melee_hit@0x88e0c0`, `k_exec_zdf_zdc@0x8ca9d0`, `k_exec_hit_landed@0x883050`;
//! * `k_exec_detect_paf@0x8c3fc0`, `k_ETAT_paf@0x8c24a0`, `fn@0x00604370` (invulnerability);
//! * `k_ETAT_main_souche@0x8e2250` (holding a weapon), `KIGO_*` (grab objects), `KR_exec_check_paf@0x65a8b0`.
//!
//! Tags: `[C]` read from code, `[L]` inferred, `[G]` guess. Evidence: `spec/evidence/KC01.md` etc.
//! Fury numbers live in [`super::fury`]; the V-Rex hit rules in [`super::vrex`]; this module
//! only adds what Kong *sends*.

use super::fury::Fury;

// ---------------------------------------------------------------------------------------------
// Hit classes: the flag word `Msg_BuildHit` carries (k_exec_hit_landed param_2) and every
// receiver reads with `fn@0x0051f190(hit)`.
// ---------------------------------------------------------------------------------------------

/// Message id Kong's blows are sent with (`k_exec_hit_landed` arg 4). [C] k_exec_melee_hit@0x88e0c0.
pub const HIT_MSG_ID: u32 = 0x38d1_b717;
/// Light blow. [C]
pub const HIT_LIGHT: u32 = 0x01;
/// Heavy blow (knock-down capable on the V-Rex, see `vrex::knockdown_capable`). [C]
pub const HIT_HEAVY: u32 = 0x02;
/// Repel / push-back flag; with [`HIT_NO_DAMAGE`] it is the "upward repel" (class 0x14). [C]
pub const HIT_REPEL: u32 = 0x04;
/// Downward-strike flag (class 9 = 1|8, class 10 = 2|8 in fury). Hurts a wounded V-Rex and
/// kills a Kong-level raptor outright (`KR_exec_check_paf`). [C]
pub const HIT_DOWN: u32 = 0x08;
/// "No life damage" class (V-Rex `hit_damage` ignores it). [C]
pub const HIT_NO_DAMAGE: u32 = 0x10;
/// Class 0x20: Kong's own `k_exec_detect_paf` ignores it; `KR_exec_check_paf` sets the
/// raptor's life to 0 on it. Sent by the phase-10 interaction strike. [C]
pub const HIT_SET_LIFE_ZERO: u32 = 0x20;
/// Class 0x40: sent to the object that *proposed* itself for the grab (`param_6 != 0`). [C]
pub const HIT_PROPOSER: u32 = 0x40;

// ---------------------------------------------------------------------------------------------
// Phases (`Kong+0x1adc`, read by `fn@0x008849b0`, written by `k_exec_set_combat_phase@0x884a10`).
// ---------------------------------------------------------------------------------------------

/// Combat phase. The numeric values are the ones stored in `Kong+0x1adc`. [C]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 0: no combat action (cleared by `fn@0x008849e0`).
    None = 0,
    /// 1: first strike (`fn@0x00884b00`), anims 0x17 / 0x19 / 0x1b.
    Punch1 = 1,
    /// 2: second strike (`fn@0x00884d40`), anim 0x1d.
    Punch2 = 2,
    /// 3: upward repel (`fn@0x00884e70`), anim 0xac.
    Repel = 3,
    /// 4: chest pound (`k_exec_start_chest_pound@0x884f00`), anim 0xab; see `fury::PoundWindow`.
    ChestPound = 4,
    /// 5: downward strike (`fn@0x00884de0`), anim 0x1f.
    Downward = 5,
    /// 6: counter lunge after a dodge (`fn@0x00884f60`), anim 0x16.
    CounterLunge = 6,
    /// 7: roll (`fn@0x008851a0` default), anim 0xe.
    DodgeRoll = 7,
    /// 8: side-step around a target (`fn@0x008851a0` with a target), anims 6 / 7.
    DodgeSide = 8,
    /// 9: recovery / ready (`fn@0x00885010`): any action may start from here.
    Recover = 9,
    /// 10: contextual strike on a proposed target (`fn@0x00886b00`), anim 0xce.
    Interact = 10,
    /// 11: climb-over / column (`fn@0x00887160`, anim param 0x104): movement agent's domain.
    ClimbOver = 11,
}

impl Phase {
    pub fn id(self) -> u32 {
        self as u32
    }

    pub fn from_id(id: u32) -> Option<Phase> {
        Some(match id {
            0 => Phase::None,
            1 => Phase::Punch1,
            2 => Phase::Punch2,
            3 => Phase::Repel,
            4 => Phase::ChestPound,
            5 => Phase::Downward,
            6 => Phase::CounterLunge,
            7 => Phase::DodgeRoll,
            8 => Phase::DodgeSide,
            9 => Phase::Recover,
            10 => Phase::Interact,
            11 => Phase::ClimbOver,
            _ => return None,
        })
    }

    /// Animation the phase starter plays (`ANIM_Play`). Phase 1 depends on the stick, see
    /// [`punch_variant`]; returns its default (0x17). Phase 9 plays a recovery anim chosen from
    /// the previous phase, see [`recovery_anim`].
    pub fn start_anim(self) -> Option<u32> {
        Some(match self {
            Phase::Punch1 => ANIM_PUNCH_A,
            Phase::Punch2 => ANIM_PUNCH2,
            Phase::Repel => ANIM_REPEL,
            Phase::ChestPound => ANIM_POUND,
            Phase::Downward => ANIM_DOWNWARD,
            Phase::CounterLunge => ANIM_COUNTER,
            Phase::DodgeRoll => ANIM_ROLL,
            Phase::DodgeSide => ANIM_SIDESTEP_L,
            Phase::Interact => ANIM_INTERACT,
            _ => return None,
        })
    }
}

pub const ANIM_IDLE: u32 = 0x00;
pub const ANIM_PUNCH_A: u32 = 0x17; // [C] fn@0x00884b00
pub const ANIM_PUNCH_B: u32 = 0x19; // [C]
pub const ANIM_PUNCH_C: u32 = 0x1b; // [C]
pub const ANIM_PUNCH2: u32 = 0x1d; // [C] fn@0x00884d40
pub const ANIM_DOWNWARD: u32 = 0x1f; // [C] fn@0x00884de0
pub const ANIM_REPEL: u32 = 0xac; // [C] fn@0x00884e70
pub const ANIM_POUND: u32 = 0xab; // [C] k_exec_start_chest_pound
pub const ANIM_ROAR: u32 = 0xad; // [C] fury start / end of phase 4
pub const ANIM_COUNTER: u32 = 0x16; // [C] fn@0x00884f60
pub const ANIM_ROLL: u32 = 0x0e; // [C] fn@0x008851a0 (0x60000000-biased id)
pub const ANIM_SIDESTEP_L: u32 = 0x06; // [C] fn@0x008851a0
pub const ANIM_SIDESTEP_R: u32 = 0x07; // [C]
pub const ANIM_INTERACT: u32 = 0xce; // [C] fn@0x00886b00
pub const ANIM_STUN_PUNCH: u32 = 0x80; // [C] melee_hit: damage 0, class 0x10

/// `fn@0x00885010`: animation played when a phase ends (`fn@0x008849b0` = the phase that ended).
/// Phase 1: 0x17->0x18, 0x19->0x1a, 0x1b->0x1c (by the current anim); phase 2: 0x1e;
/// phase 3: 0 (idle); phase 4: 0xad (roar). Other phases play nothing. [C]
pub fn recovery_anim(ended: Phase, current_anim: u32) -> Option<u32> {
    match ended {
        Phase::Punch1 => match current_anim {
            0x17 => Some(0x18),
            0x19 => Some(0x1a),
            0x1b => Some(0x1c),
            _ => None,
        },
        Phase::Punch2 => Some(0x1e),
        Phase::Repel => Some(ANIM_IDLE),
        Phase::ChestPound => Some(ANIM_ROAR),
        _ => None,
    }
}

/// Parameters `fn@0x00884ab0(a, b, c)` stores at every phase start: `a` -> `Kong+0x1ae8` (attack
/// step speed; `fn@0x00887450` = "is non-zero" turns the forward lunge on), `b` -> `Kong+0x18a4`,
/// `c` -> `Kong+0x1af0` (`b`/`c` are blend / side parameters `[L]`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackParams {
    pub step_speed: f32,
    pub b: f32,
    pub c: f32,
}

/// Phase 1 variant choice (`fn@0x00884b00`). `side_dot` = dot(stick, (-V.y, V.x, 0)) and
/// `back_dot` = dot(stick, -V) with `V = Kong+0x11ac..0x11b4` (the reference direction, `[L]`
/// Kong's facing). `carrying` = `Kong+4 != 0`. Returns (anim, params). [C]
pub fn punch_variant(side_dot: f32, back_dot: f32, carrying: bool) -> (u32, AttackParams) {
    if side_dot <= 0.5 {
        if back_dot >= 0.0 {
            (ANIM_PUNCH_B, AttackParams { step_speed: 5.0, b: 0.0, c: -1.0 })
        } else if !carrying {
            (ANIM_PUNCH_A, AttackParams { step_speed: 5.0, b: 1.0, c: 1.0 })
        } else {
            (ANIM_PUNCH_C, AttackParams { step_speed: 5.0, b: 1.0, c: 0.0 })
        }
    } else {
        (ANIM_PUNCH_C, AttackParams { step_speed: 3.0, b: 1.0, c: 0.0 })
    }
}

/// `fn@0x00884ab0` arguments of the other phase starters. [C]
pub fn phase_params(p: Phase) -> AttackParams {
    match p {
        Phase::Punch2 | Phase::Downward => AttackParams { step_speed: 2.0, b: 0.0, c: 0.0 },
        Phase::Repel => AttackParams { step_speed: 2.0, b: 0.5, c: 0.0 },
        Phase::ChestPound | Phase::CounterLunge | Phase::Recover | Phase::Interact | Phase::DodgeRoll | Phase::DodgeSide => {
            AttackParams { step_speed: 0.0, b: 0.5, c: 0.0 }
        }
        Phase::Punch1 => AttackParams { step_speed: 5.0, b: 1.0, c: 1.0 },
        Phase::None | Phase::ClimbOver => AttackParams { step_speed: 0.0, b: 0.5, c: 0.0 },
    }
}

// ---------------------------------------------------------------------------------------------
// Aim assist (fn@0x008848e0 -> fn@0x00882990) and opponent selection (k_exec_select_opponent).
// ---------------------------------------------------------------------------------------------

/// Cone of the aim assist applied when a punch phase starts: cos 45 deg. [C] fn@0x008848e0 (0x3f3504f3)
pub const AIM_ASSIST_COS: f32 = 0.707_106_77;
/// Reach of that aim assist (`0x41000000`). [C]
pub const AIM_ASSIST_REACH: f32 = 8.0;
/// Search limit passed to `fn@0x00882990` (`0x42c80000`). [C]
pub const AIM_ASSIST_SEARCH: f32 = 100.0;
/// `k_exec_select_opponent`: candidates are the type-8 messages within sqrt(10000) = 100. [C]
pub const OPPONENT_SEARCH_DIST_SQ: f32 = 10_000.0;
/// The previously selected target is kept when it is still listed and closer than 12 (144 squared). [C]
pub const OPPONENT_STICKY_DIST_SQ: f32 = 144.0;
/// Max opponents remembered per frame in `Kong+0xfec[]`... the rider table has 5 slots. [C]
pub const RIDER_SLOTS: usize = 5;

/// Which listed opponent `k_exec_select_opponent` picks: the one whose direction has the largest
/// dot with the stick (`dots` is that dot per candidate, in list order; the original keeps the best
/// score in a stack slot initialised to -1e14). Returns the index. [C, labels of locals swapped in
/// the decompiler; checked in the disassembly]
pub fn select_opponent(dots: &[f32]) -> Option<usize> {
    let mut best = -1.0e14_f32;
    let mut idx = None;
    for (i, &d) in dots.iter().enumerate() {
        if best < d {
            best = d;
            idx = Some(i);
        }
    }
    idx
}

// ---------------------------------------------------------------------------------------------
// The combo graph: which latched button leads where (all phase handlers of k_ETAT_main).
// ---------------------------------------------------------------------------------------------

/// Latched buttons (`k_exec_button_latched@0x884600`). Slot n = pad index n-1 of
/// `k_exec_joy@0x8ae5f0`: slot 3 = pad 0 (`Kong+0x90`, the attack button), slot 2 = pad 3
/// (`Kong+0x78`, the fury / special button; "Triangle" `[L]`), slot 1 = pad 2 (`Kong+0x8c`,
/// jump/roll, handled by `k_ObstacleResponse`), slot 4 = pad 1 (`Kong+0x7c`, mash/cancel).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Latched {
    pub attack: bool,
    pub special: bool,
    pub jump_roll: bool,
    pub cancel: bool,
}

impl Latched {
    pub fn any(&self) -> bool {
        self.attack || self.special || self.jump_roll || self.cancel
    }
}

/// Kong's object flag word (`obj+0x18 -> +0x28`, 16 bit) as the animation event tracks drive it (`Obj_ModifyFlags@0x415950` via AI function 0x1fc5;
/// see KC01). Bits 0x20/0x40/0x80 are the input windows tested by `k_exec_button_edge@0x884700` (edge slot 1 -> 0x20, slot 2 -> 0x40,
/// slots 3 and 4 -> 0x80): a press only latches while the matching bit is set. `k_tag_on@0x894c10` clears 0x20/0x40/0x80/0x100.
pub const FLAG_HIT_WINDOW: u16 = 0x1;
pub const FLAG_WINDOW_JUMP_ROLL: u16 = 0x20;
pub const FLAG_WINDOW_SPECIAL: u16 = 0x40;
pub const FLAG_WINDOW_ATTACK: u16 = 0x80;
/// The cancel window: `fn@0x00884780` = `Obj_TestFlags(Kong, 0x100)`. Phases 1/2/... accept the next latched press when the animation ended
/// or this bit is set. `k_callback_after_blend@0x896bf0` clears `FLAG_HIT_WINDOW` whenever the animation changes.
pub const FLAG_CANCEL_WINDOW: u16 = 0x100;

/// Which window bit a latch slot (`k_LatchButtonEdges@0x884810`: pad `Kong+0x8c` -> slot 1, `+0x78` -> 2, `+0x90` -> 3, `+0x7c` -> 4) needs. [C]
pub fn window_for_slot(slot: usize) -> u16 {
    match slot {
        1 => FLAG_WINDOW_JUMP_ROLL,
        2 => FLAG_WINDOW_SPECIAL,
        _ => FLAG_WINDOW_ATTACK,
    }
}

/// `k_exec_button_latched(1)` is forced to 0 while two or more riders sit on Kong. [C]
pub fn jump_roll_latched(raw: bool, riders: u32) -> bool {
    raw && riders < 2
}

/// State of the proposed target for the special button (`Kong+0xc != -1`, `Kong+4 == 0`, poll of
/// the proposal message succeeded). The special button then sends Kong to phase 10 when the
/// target's life is non-zero and to phase 3 when it is 0. [C]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialTarget {
    None,
    Dead,
    Alive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComboCtx {
    /// A proposal with message bit 8 ("grabbable") was returned by the poll `fn@0x007dd5b0(0xc, ..)`.
    pub grab_offered: bool,
    /// `Kong+4 != 0` (carrying; `k_reflex` sets it with the Ann carry link, `[L]`).
    pub carrying: bool,
    pub special_target: SpecialTarget,
    /// `k_TEST_Grab@0x8eaaa0` returned a state to go to (the cancel slot).
    pub test_grab_state: bool,
}

/// Where the machine goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Stay in the phase (`fn@0x008873f0` marks `Kong+0x1ae0 = 1`, the animation keeps playing).
    Stay,
    /// `AI_GotoState(k_ETAT_grab)`.
    GrabState,
    /// `AI_GotoState` of the state `k_TEST_Grab` returned.
    TestGrabState,
    /// Start another phase.
    Phase(Phase),
    /// `fn@0x008851a0`: jump / roll / climb-over logic (movement agent: `k_ObstacleResponse`).
    ObstacleResponse,
    /// `fn@0x00885010`: end the action, phase 9.
    End,
}

/// Is the phase allowed to take its next input? `anim_ended || (anim flag 0x100 && any latched)`.
/// (`fn@0x00884780` tests object flag 0x100; the animation data sets it at the cancel frame.) [C]
pub fn ready_for_input(anim_ended: bool, cancel_window_flag: bool, latched: &Latched) -> bool {
    anim_ended || (cancel_window_flag && latched.any())
}

fn special_branch(c: &ComboCtx) -> Next {
    match c.special_target {
        SpecialTarget::Alive => Next::Phase(Phase::Interact),
        _ => Next::Phase(Phase::Repel),
    }
}

/// The input graph of `k_ETAT_main` once a phase is `ready_for_input`. `cur` is the phase that is
/// finishing (`Phase::None`/`Recover` = idle or recovery, where the same inputs start an action).
/// Order of tests (first that is latched wins): attack, special, cancel, jump_roll. [C]
///
/// * attack: a grab offer starts `k_ETAT_grab`; otherwise Punch1 -> Punch2, Punch2 -> End,
///   Repel -> Downward (or Punch1 again while carrying), Downward -> Punch1, DodgeRoll/Side ->
///   CounterLunge, idle/recovery -> Punch1.
/// * special: Repel (phase 3) or Interact (phase 10); from phase 3 (not carrying) -> ChestPound.
/// * cancel: `k_TEST_Grab` target or End.
/// * jump_roll: `fn@0x008851a0`.
/// * nothing latched: End (phase 9).
pub fn next_phase(cur: Phase, l: &Latched, c: &ComboCtx) -> Next {
    if l.attack {
        if c.grab_offered {
            return Next::GrabState;
        }
        return match cur {
            Phase::Punch1 => Next::Phase(Phase::Punch2),
            Phase::Punch2 => Next::End,
            Phase::Repel => {
                if c.carrying {
                    Next::Phase(Phase::Punch1)
                } else {
                    Next::Phase(Phase::Downward)
                }
            }
            Phase::Downward => Next::Phase(Phase::Punch1),
            Phase::DodgeRoll | Phase::DodgeSide => Next::Phase(Phase::CounterLunge),
            Phase::None | Phase::Recover => Next::Phase(Phase::Punch1),
            _ => Next::End,
        };
    }
    if l.special {
        if cur == Phase::Repel {
            // k_ETAT_main case 3: Kong+4 == 0 -> chest pound; otherwise only ends with the anim.
            return if c.carrying { Next::Stay } else { Next::Phase(Phase::ChestPound) };
        }
        return special_branch(c);
    }
    if l.cancel {
        return if c.test_grab_state { Next::TestGrabState } else { Next::End };
    }
    if l.jump_roll {
        return Next::ObstacleResponse;
    }
    Next::End
}

/// Chained dodge: in phase 8 a second press is accepted after anim frame 0x37 (55) while fewer
/// than two riders sit on Kong. [C] k_ETAT_main@0x89dad0
pub const DODGE_CHAIN_FRAME: i32 = 0x37;

// ---------------------------------------------------------------------------------------------
// Invulnerability and dodge
// ---------------------------------------------------------------------------------------------

/// `fn@0x00604370` (named here `Kong_IsInvulnerable`): Kong takes no damage from the normal hit path
/// while the global flag `Univers+0x16944`, `Kong+0x474`, or the current animation is 6, 7 (side-step)
/// or 0x16 (counter lunge). The roll (0xe) is NOT in the list. [C]
pub fn is_invulnerable(anim: u32, kong_flag_474: bool, global_flag: bool) -> bool {
    global_flag || kong_flag_474 || matches!(anim, 6 | 7 | 0x16)
}

/// Side-step distance gate in `fn@0x008851a0`: a target is used when its squared distance is
/// at most 196 (14 units) and the stick has dot >= -0.342 (cos 110 deg) with the direction to it. [C]
pub const SIDESTEP_MAX_DIST_SQ: f32 = 196.0;
pub const SIDESTEP_MIN_DOT: f32 = -0.342_020_15;
/// After a side-step ends: `Kong+0x1164 = 5.0`, `Kong+0x1168 = 1.0` (cool-down values, `[L]`). [C]
pub const DODGE_END_1164: f32 = 5.0;
pub const DODGE_END_1168: f32 = 1.0;

// ---------------------------------------------------------------------------------------------
// The blow table (k_exec_melee_hit)
// ---------------------------------------------------------------------------------------------

/// Damage and class of one blow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blow {
    pub damage: i32,
    pub class: u32,
}

/// Kong's action id (`Kong+0x5c`) values that matter for the blow table.
pub mod action {
    pub const MAIN: i32 = 0xfa; // k_ETAT_main / main_souche
    pub const GRAB: i32 = 0x15e;
    pub const MASHING: i32 = 0x160;
    pub const PAF: i32 = 600; // k_ETAT_paf
    pub const JUMP: i32 = 0x3e9; // k_ETAT_jump
    pub const SWING_ARCH: i32 = 0x3b6; // k_ETAT_SwingArch
}

/// Inputs of the blow table.
#[derive(Clone, Copy, Debug, Default)]
pub struct BlowCtx {
    pub action: i32,
    pub anim: u32,
    pub fury: bool,
    /// `Kong+0x378 != 0`: a held object / creature is the weapon.
    pub holding: bool,
    /// `Kong+0x112c == 0xc6`: the held-object overhead smash.
    pub held_smash: bool,
    /// `Kong+0x9a8 != 0 || Kong+0x9ac != 0`: the jump-attack special flags.
    pub jump_special: bool,
    /// `Kong+0xc80`: the reaction kind stored by `k_ETAT_paf` (used as class while knocked back).
    pub paf_kind: u32,
}

fn dmg(fury: bool) -> i32 {
    if fury {
        super::fury::BASE_HIT_DAMAGE + super::fury::FURY_DAMAGE_BONUS as i32
    } else {
        super::fury::BASE_HIT_DAMAGE
    }
}

/// `k_exec_melee_hit@0x88e0c0`: damage and class sent for a blow landing now. [C]
pub fn blow(c: &BlowCtx) -> Blow {
    if c.holding {
        // weapon swing: damage 10 (20 in fury), class 2, or 10 (= 8|2) for the overhead smash 0xc6
        return Blow { damage: dmg(c.fury), class: if c.held_smash { 10 } else { 2 } };
    }
    if c.anim == 0x16 {
        return Blow { damage: dmg(c.fury), class: 2 };
    }
    if c.anim == ANIM_STUN_PUNCH {
        return Blow { damage: 0, class: HIT_NO_DAMAGE };
    }
    match c.action {
        action::JUMP => {
            if c.jump_special {
                Blow { damage: 30, class: 9 }
            } else {
                Blow { damage: 10, class: 1 }
            }
        }
        action::PAF => Blow { damage: 10, class: c.paf_kind },
        action::SWING_ARCH => Blow { damage: 10, class: 1 },
        _ => {
            let mut class = if c.fury { 2 } else { 1 };
            if c.anim == ANIM_DOWNWARD {
                class |= HIT_DOWN;
            } else if c.anim == ANIM_REPEL {
                class = HIT_NO_DAMAGE | HIT_REPEL;
            }
            Blow { damage: dmg(c.fury), class }
        }
    }
}

/// Convenience used by the fury module's tests: the plain punch blow.
pub fn punch_blow(fury: &Fury) -> Blow {
    Blow { damage: fury.melee_damage(), class: fury.melee_hit_class() }
}

/// Second pass of `k_exec_melee_hit` (`param_7 != 0`, the shove): damage 0, class 0x14, vector * 5. [C]
pub const SHOVE: Blow = Blow { damage: 0, class: HIT_NO_DAMAGE | HIT_REPEL };
/// Hit sent to the object that proposed itself (`param_6`): class 0x40, damage 2. [C]
pub const PROPOSER_HIT: Blow = Blow { damage: 2, class: HIT_PROPOSER };
/// Phase-10 strike (anim 0xce) after frame 0x3c: class 0x20, damage 2, to the proposed target. [C]
pub const INTERACT_HIT: Blow = Blow { damage: 2, class: HIT_SET_LIFE_ZERO };
pub const INTERACT_HIT_FRAME: i32 = 0x3c;
/// Counter-lunge direct hit (phase 6, window flag 1): damage 10, class 2. [C]
pub const COUNTER_DIRECT_HIT: Blow = Blow { damage: 10, class: HIT_HEAVY };
/// Grab state: anim 0xa2 frame > 0x12 hits the held creature once: damage 10, class 9. [C]
pub const GRAB_STRIKE: Blow = Blow { damage: 10, class: 9 };
pub const GRAB_STRIKE_ANIM: u32 = 0xa2;
pub const GRAB_STRIKE_FRAME: i32 = 0x12;
/// Hit sweep reach: the direction vector is multiplied by 3.0 (`param * 3.0`). [C]
pub const HIT_SWEEP_REACH: f32 = 3.0;
/// During anim 0x16 the hand volume is scaled to 4.0 for the sweep, then back to 3.0. [C] zdf_zdc
pub const COUNTER_VOLUME_SCALE: (f32, f32) = (4.0, 3.0);
/// Camera shake sent by `k_exec_hit_landed` on a landed blow: (amplitude, ...) by class bit
/// 2 -> 0.075, 8 -> 0.05, 1 -> 0.025 (`fn@0x007d49a0(amp, 35.0, 0, 0, 0.15, 1.1)`). [C]
pub fn hit_shake_amplitude(class: u32) -> Option<f32> {
    if class & 2 != 0 {
        Some(0.075)
    } else if class & 8 != 0 {
        Some(0.05)
    } else if class & 1 != 0 {
        Some(0.025)
    } else {
        None
    }
}
/// The downward strike shakes the camera when anim frame 0 > 0x20 (once per action, `Kong+0xdf8`):
/// `fn@0x007d49a0(0.05, 35.0, 0, 0, 0.15, 0.99)`. [C]
pub const DOWNWARD_SHAKE_FRAME: i32 = 0x20;

// ---------------------------------------------------------------------------------------------
// Kong as receiver (k_exec_detect_paf, k_ETAT_paf)
// ---------------------------------------------------------------------------------------------

/// `Kong+0xc74` flag bits tested by `k_exec_detect_paf`. [C]
pub mod recv {
    pub const IGNORED: u32 = 0x20;
    pub const FIRE_LIKE: u32 = 0x400; // plays anim 0x42, shake; no interrupt
    pub const REPEATED: u32 = 0x80; // counted, 30 damage after the 4th within 4 s
    pub const GRAB_HIT: u32 = 0x1_0000;
    pub const NO_INTERRUPT: u32 = 0x4;
}

/// A fourth `0x80` hit inside the 4.0 s window (counter `Kong+0xc94 > 3`, window `fn@0x0043c690(.., 4.0)`),
/// or any such hit during mashing (action 0x160), deals this fixed damage. [C]
pub const REPEATED_HIT_DAMAGE: f32 = 30.0;
pub const REPEATED_HIT_WINDOW: f32 = 4.0;
pub const REPEATED_HIT_COUNT: u32 = 4;

/// Hit-stun reaction chosen by `k_ETAT_paf`: (animation id, strength `Kong+0xc90`).
/// `dot` is the dot of the hit direction with the reference vector, `side_dot` the one with the
/// side vector (which of the two side anims is `[L]`). Anim ids are the `0xc00000xx` ids with the
/// flag bits removed. [C]
pub fn hit_reaction(flags: u32, dot: f32, side_dot: f32) -> (u32, u32) {
    const C: f32 = 0.707_106_77;
    if flags & 2 != 0 {
        // heavy: Kong+0xc80 = 1
        if dot <= C {
            if -C <= dot {
                if side_dot <= 0.0 { (0x2e, 0x14) } else { (0x2f, 0x1e) }
            } else {
                (0x2c, 0x1e)
            }
        } else {
            (0x2d, 0x2a)
        }
    } else if flags & 0x81 != 0 {
        // light: Kong+0xc80 = 4
        if dot <= C {
            if -C <= dot {
                if side_dot <= 0.0 { (0x2a, 4) } else { (0x2b, 4) }
            } else {
                (0x28, 4)
            }
        } else {
            (0x29, 0x14)
        }
    } else if flags & 4 != 0 {
        (0x27, 999)
    } else {
        (0x27, 0)
    }
}

/// While in the light hit-stun anims 0x28/0x2b, an attack press after anim frame 0xf restarts the
/// punch chain with blend 10 (`fn@0x00884b00(10)`). [C]
pub const HIT_STUN_PUNCH_CANCEL_FRAME: i32 = 0x0f;
/// Knocked-back Kong collides with things between 0.25 s and 0.7 s (0.4 s for anim 0x29) of the
/// state and sends them damage 10 (`fn@0x006e3df0`). [C]
pub const KNOCKBACK_HIT_WINDOW: (f32, f32) = (0.25, 0.7);
pub const KNOCKBACK_HIT_WINDOW_ANIM_29_END: f32 = 0.4;
pub const KNOCKBACK_HIT_DAMAGE: i32 = 10;

// ---------------------------------------------------------------------------------------------
// Held objects (k_ETAT_main_souche), thrown objects and creatures
// ---------------------------------------------------------------------------------------------

pub const SWING_RIGHT: u32 = 0xc2;
pub const SWING_LEFT: u32 = 0xc4;
pub const SWING_SMASH: u32 = 0xc6;
/// A swing may be followed once the animation passes this frame (or at the cancel flag). [C]
pub const SWING_CHAIN_FRAME: i32 = 0x23;

/// Next swing request (`fn@0x008915e0(anim)`; -1 = none) after the current swing is chainable.
/// attack: 0xc2 after 0xc4 / 0xc4 after 0xc2 (from idle the side stored in `Kong+0x18a8`);
/// special: 0xc6; neither: none. [C]
pub fn next_swing(current: u32, side_flag: bool, attack: bool, special: bool) -> Option<u32> {
    if attack {
        return Some(match current {
            SWING_RIGHT => SWING_LEFT,
            SWING_LEFT => SWING_RIGHT,
            _ => {
                if side_flag { SWING_LEFT } else { SWING_RIGHT }
            }
        });
    }
    if special {
        return Some(SWING_SMASH);
    }
    None
}

/// Kong-level raptor thrown into a wall or the ground: life -= 5.0 (twice in the code path: collision
/// flags 0x80 with anim 0x2e, and 0x40), unless `KR+0x520 == 1`. [C] KR_ETAT_projectile@0x652e70
pub const THROWN_RAPTOR_IMPACT_DAMAGE: f32 = 5.0;
/// Megapede (Scolo) grabbed by Kong takes 5.0 when released (`PNJ_Scolo_ETAT_Grabbed_By_Kong`). [C]
pub const THROWN_SCOLO_DAMAGE: f32 = 5.0;
/// `KIGO_init`: a grab object is proposed to Kong when its squared distance is below 81 (radius 9). [C]
pub const KIGO_PROPOSE_DIST_SQ: f32 = 81.0;
/// KIGO throw impact class: 2, or 0x22 when `KIGO+8 != 0`; the damage is `KIGO+0x18` (instance data). [C]
pub fn kigo_hit_class(flag_plus_8: bool) -> u32 {
    if flag_plus_8 { 0x22 } else { 2 }
}

/// `KR_exec_check_paf` rule: what a hit of `flags` does to a Kong-level raptor with raw damage `d`.
/// Class 8 (downward) and class 0x20 set the life to 0; otherwise damage `d` unless the 0x10 bit is set. [C]
pub fn raptor_hit(flags: u32, raw_damage: f32, life: f32) -> f32 {
    if flags & (HIT_DOWN | HIT_SET_LIFE_ZERO) != 0 {
        0.0
    } else if flags & HIT_NO_DAMAGE == 0 {
        (life - raw_damage).max(0.0)
    } else {
        life
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kong::vrex::{hit_damage, knockdown_capable, KtState};

    fn ctx() -> ComboCtx {
        ComboCtx { grab_offered: false, carrying: false, special_target: SpecialTarget::None, test_grab_state: false }
    }
    fn press(a: bool, s: bool, j: bool, c: bool) -> Latched {
        Latched { attack: a, special: s, jump_roll: j, cancel: c }
    }

    #[test]
    fn phase_ids_and_anims_match_the_starters() {
        assert_eq!(Phase::Punch1.id(), 1);
        assert_eq!(Phase::ChestPound.id(), 4);
        assert_eq!(Phase::Recover.id(), 9);
        assert_eq!(Phase::from_id(10), Some(Phase::Interact));
        assert_eq!(Phase::Punch2.start_anim(), Some(0x1d));
        assert_eq!(Phase::Repel.start_anim(), Some(0xac));
        assert_eq!(Phase::Downward.start_anim(), Some(0x1f));
        assert_eq!(Phase::CounterLunge.start_anim(), Some(0x16));
        assert_eq!(Phase::Interact.start_anim(), Some(0xce));
    }

    #[test]
    fn punch_variants_and_step_speeds() {
        assert_eq!(punch_variant(0.9, 0.0, false), (0x1b, AttackParams { step_speed: 3.0, b: 1.0, c: 0.0 }));
        assert_eq!(punch_variant(0.0, 0.5, false).0, 0x19);
        assert_eq!(punch_variant(0.0, -0.5, false).0, 0x17);
        assert_eq!(punch_variant(0.0, -0.5, true).0, 0x1b);
        assert_eq!(phase_params(Phase::Punch2).step_speed, 2.0);
        assert_eq!(phase_params(Phase::ChestPound).step_speed, 0.0);
        assert_eq!(recovery_anim(Phase::Punch1, 0x19), Some(0x1a));
        assert_eq!(recovery_anim(Phase::ChestPound, 0), Some(0xad));
    }

    #[test]
    fn combo_graph() {
        let c = ctx();
        // idle / recovery: attack starts the punch
        assert_eq!(next_phase(Phase::Recover, &press(true, false, false, false), &c), Next::Phase(Phase::Punch1));
        // punch 1 -> punch 2 -> end
        assert_eq!(next_phase(Phase::Punch1, &press(true, false, false, false), &c), Next::Phase(Phase::Punch2));
        assert_eq!(next_phase(Phase::Punch2, &press(true, false, false, false), &c), Next::End);
        // special after a punch = repel; after the repel, attack = downward strike
        assert_eq!(next_phase(Phase::Punch2, &press(false, true, false, false), &c), Next::Phase(Phase::Repel));
        assert_eq!(next_phase(Phase::Repel, &press(true, false, false, false), &c), Next::Phase(Phase::Downward));
        // carrying (Kong+4) turns the downward strike into a new punch and blocks the chest pound
        let carry = ComboCtx { carrying: true, ..c };
        assert_eq!(next_phase(Phase::Repel, &press(true, false, false, false), &carry), Next::Phase(Phase::Punch1));
        assert_eq!(next_phase(Phase::Repel, &press(false, true, false, false), &carry), Next::Stay);
        // special twice (repel then special) = chest pound
        assert_eq!(next_phase(Phase::Repel, &press(false, true, false, false), &c), Next::Phase(Phase::ChestPound));
        // a grab offer wins over the punch
        let offer = ComboCtx { grab_offered: true, ..c };
        assert_eq!(next_phase(Phase::Punch1, &press(true, false, false, false), &offer), Next::GrabState);
        // a living proposed target turns the special into phase 10
        let alive = ComboCtx { special_target: SpecialTarget::Alive, ..c };
        assert_eq!(next_phase(Phase::Punch1, &press(false, true, false, false), &alive), Next::Phase(Phase::Interact));
        // dodge then attack = counter lunge
        assert_eq!(next_phase(Phase::DodgeRoll, &press(true, false, false, false), &c), Next::Phase(Phase::CounterLunge));
        // nothing latched: end
        assert_eq!(next_phase(Phase::Punch1, &Latched::default(), &c), Next::End);
        assert!(ready_for_input(false, true, &press(true, false, false, false)));
        assert!(!ready_for_input(false, true, &Latched::default()));
    }

    #[test]
    fn dodge_is_blocked_by_two_riders_and_has_iframes_only_in_side_step_and_counter() {
        assert!(jump_roll_latched(true, 1));
        assert!(!jump_roll_latched(true, 2));
        assert!(is_invulnerable(6, false, false));
        assert!(is_invulnerable(7, false, false));
        assert!(is_invulnerable(0x16, false, false));
        assert!(!is_invulnerable(0x0e, false, false));
        assert!(is_invulnerable(0x0e, true, false));
        assert_eq!(DODGE_CHAIN_FRAME, 55);
    }

    #[test]
    fn blow_table_matches_melee_hit() {
        let plain = BlowCtx { action: action::MAIN, anim: 0x17, ..Default::default() };
        assert_eq!(blow(&plain), Blow { damage: 10, class: 1 });
        let fury = BlowCtx { fury: true, ..plain };
        assert_eq!(blow(&fury), Blow { damage: 20, class: 2 });
        // consistent with the fury module
        let mut f = Fury::default();
        assert_eq!(punch_blow(&f), blow(&plain));
        f.start();
        assert_eq!(punch_blow(&f), blow(&fury));
        // upward repel: class 0x14 whatever the fury
        let rep = BlowCtx { anim: ANIM_REPEL, ..plain };
        assert_eq!(blow(&rep).class, 0x14);
        assert_eq!(blow(&BlowCtx { fury: true, ..rep }).class, 0x14);
        // downward strike: 1|8 = 9, in fury 2|8 = 10
        let down = BlowCtx { anim: ANIM_DOWNWARD, ..plain };
        assert_eq!(blow(&down), Blow { damage: 10, class: 9 });
        assert_eq!(blow(&BlowCtx { fury: true, ..down }), Blow { damage: 20, class: 10 });
        // counter lunge is always heavy
        assert_eq!(blow(&BlowCtx { anim: 0x16, ..plain }), Blow { damage: 10, class: 2 });
        // jump attack and its special variant
        let jump = BlowCtx { action: action::JUMP, ..plain };
        assert_eq!(blow(&jump), Blow { damage: 10, class: 1 });
        assert_eq!(blow(&BlowCtx { jump_special: true, ..jump }), Blow { damage: 30, class: 9 });
        // held weapon: class 2, smash class 10
        let held = BlowCtx { holding: true, ..plain };
        assert_eq!(blow(&held), Blow { damage: 10, class: 2 });
        assert_eq!(blow(&BlowCtx { held_smash: true, fury: true, ..held }), Blow { damage: 20, class: 10 });
        assert_eq!(blow(&BlowCtx { anim: ANIM_STUN_PUNCH, ..plain }), Blow { damage: 0, class: 0x10 });
    }

    #[test]
    fn blows_against_the_vrex_rules() {
        // the repel does no life damage; the downward strike hurts a wounded rex (flag 8 is in 0x4a)
        let rep = blow(&BlowCtx { action: action::MAIN, anim: ANIM_REPEL, ..Default::default() });
        assert_eq!(hit_damage(rep.class, rep.class, false, rep.damage as f32), 0.0);
        let down = blow(&BlowCtx { action: action::MAIN, anim: ANIM_DOWNWARD, ..Default::default() });
        assert_eq!(hit_damage(down.class, down.class, true, down.damage as f32), 10.0);
        // a plain light punch does not hurt a wounded rex, a fury punch (class 2) does
        let p = blow(&BlowCtx { action: action::MAIN, anim: 0x17, ..Default::default() });
        assert_eq!(hit_damage(p.class, p.class, true, p.damage as f32), 0.0);
        let pf = blow(&BlowCtx { action: action::MAIN, anim: 0x17, fury: true, ..Default::default() });
        assert_eq!(hit_damage(pf.class, pf.class, true, pf.damage as f32), 20.0);
        // knock-down capability: class 2 only (or fury)
        assert!(!knockdown_capable(p.class, 50.0, false, KtState::FightKong));
        assert!(knockdown_capable(pf.class, 50.0, true, KtState::FightKong));
        assert!(knockdown_capable(HIT_HEAVY, 50.0, false, KtState::FightKong));
        assert!(!knockdown_capable(rep.class, 50.0, false, KtState::FightKong));
    }

    #[test]
    fn raptor_dies_to_the_downward_strike() {
        assert_eq!(raptor_hit(9, 10.0, 50.0), 0.0);
        assert_eq!(raptor_hit(1, 10.0, 50.0), 40.0);
        assert_eq!(raptor_hit(0x14, 10.0, 50.0), 50.0);
        assert_eq!(raptor_hit(HIT_SET_LIFE_ZERO, 2.0, 50.0), 0.0);
        assert_eq!(THROWN_RAPTOR_IMPACT_DAMAGE, 5.0);
    }

    #[test]
    fn hit_reactions_and_receiver_constants() {
        assert_eq!(hit_reaction(2, 0.9, 0.0), (0x2d, 0x2a));
        assert_eq!(hit_reaction(2, -0.9, 0.0), (0x2c, 0x1e));
        assert_eq!(hit_reaction(1, 0.9, 0.0), (0x29, 0x14));
        assert_eq!(hit_reaction(1, 0.0, -1.0), (0x2a, 4));
        assert_eq!(hit_reaction(4, 0.0, 0.0), (0x27, 999));
        assert_eq!(REPEATED_HIT_DAMAGE, 30.0);
        assert_eq!(hit_shake_amplitude(2), Some(0.075));
        assert_eq!(hit_shake_amplitude(9), Some(0.05));
    }

    #[test]
    fn weapon_swing_chain_and_opponent_selection() {
        assert_eq!(next_swing(SWING_RIGHT, false, true, false), Some(SWING_LEFT));
        assert_eq!(next_swing(SWING_LEFT, false, true, false), Some(SWING_RIGHT));
        assert_eq!(next_swing(SWING_LEFT, false, false, true), Some(SWING_SMASH));
        assert_eq!(next_swing(SWING_LEFT, false, false, false), None);
        assert_eq!(kigo_hit_class(false), 2);
        assert_eq!(kigo_hit_class(true), 0x22);
        assert_eq!(select_opponent(&[0.1, 0.9, 0.4]), Some(1));
        assert_eq!(select_opponent(&[]), None);
        assert_eq!(AIM_ASSIST_REACH, 8.0);
    }

    #[test]
    fn input_window_bits_match_the_button_edge_switch() {
        assert_eq!(window_for_slot(1), 0x20);
        assert_eq!(window_for_slot(2), 0x40);
        assert_eq!(window_for_slot(3), 0x80);
        assert_eq!(window_for_slot(4), 0x80);
        assert_eq!(FLAG_CANCEL_WINDOW, 0x100);
    }
}
