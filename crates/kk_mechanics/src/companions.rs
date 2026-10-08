//! Human companions and the interactive objects they use (ledger N01-N14, E08-E11, E14, I08-I10).
//!
//! Everything here is recovered from KingKong8.exe (see `spec/evidence/N01.md`, `N04.md`, `N10.md`,
//! `E08.md`, `E09.md`, `E11.md`, `I09.md`, `I10.md`). Companions are NOT a separate AI model: they
//! are further instances of Jack's `H_*` model (slot = `this+4`: 1 Jack, 2 Ann, 3 Hayes, 5 Denham,
//! 6 = Jimmy [L]); `this[0] == 0` marks an AI companion, `this[0] != 0` the human-controlled Jack
//! (`H_exec_read_joy@0x579f30`, `fn@0x0052f7b0@0x52f7b0`).
//!
//! Confidence tags: `[C]` read from code, `[L]` inferred, `[G]` guess.

/// Slot numbers of the `H_` instances (`this+4`, also the index of `G+0x1bb4+4*slot`).
pub mod slot {
    /// Jack, human-controlled [C]
    pub const JACK: u32 = 1;
    /// Ann: debug label "Ann : " in `H_TRACK_test_IK@0x5d46a0`, only slot 2 runs `H_ETAT_ann_heal` [C]
    pub const ANN: u32 = 2;
    /// Hayes: debug label "Hayes : "; `TrigCINE_ResetMunitionHayes@0x7665d0` clears slot 3's ammo [C]
    pub const HAYES: u32 = 3;
    /// Denham: debug label "Denham : " [C]
    pub const DENHAM: u32 = 5;
    /// Slot 6: companion slot (stats code lists 2,3,5,6); name Jimmy is [L]
    pub const SLOT6: u32 = 6;
}

/// State ids stored at `this+0xa98` by the `H_ETAT_*` functions [C].
pub mod state {
    pub const FOLLOW: u32 = 600; // H_ETAT_IA_suivi@0x57f1b0
    pub const HIDE: u32 = 0x25b; // H_ETAT_IA_hide@0x5ab150
    pub const PSSS: u32 = 0x25e; // H_ETAT_IA_psss@0x5d88a0 (weapon swap with Jack)
    pub const HELP: u32 = 0x25f; // H_ETAT_IA_help@0x5b1720
    pub const USE: u32 = 0x261; // H_ETAT_IA_use@0x5d2710 (pick up / crank / lever)
    pub const CARRY: u32 = 0x262; // H_ETAT_IA_carry@0x5b45b0
    pub const MULTI_COVER: u32 = 0x263; // H_ETAT_IA_MultiCover@0x5d5000
    pub const WALK_STUNNED: u32 = 0x264; // H_ETAT_IA_walkstunned@0x5dbc20
    pub const DOOR: u32 = 0x268; // H_ETAT_Ia_porte@0x5e1b60 (the human cranking a pillar)
    pub const CARRIED: u32 = 0x2c5; // H_ETAT_IA_carried@0x5c6700
    pub const GRAB_TRANSPORT: u32 = 0x2c6; // H_ETAT_IA_grabtransporte@0x5adc60
    pub const SHOOT: u32 = 0x2c9; // H_ETAT_IA_tir@0x5a1610
    pub const ANN_HEAL: u32 = 0x3ee; // H_ETAT_ann_heal@0x5d0380
    pub const ANN_CLIMB: u32 = 0x3ef; // H_ETAT_ann_climb@0x5dcc40
    pub const JIMMY_MUNITIONS: u32 = 0x3f2; // H_ETAT_jimmy_munitions@0x5dffb0
    pub const CINE_SPEECH: u32 = 0x7d2; // H_CINE_Speech@0x584300
}

/// Order codes read from the order queue (`this+0x1180`, `fn@0x006e2e50`) by
/// `H_exec_change_etat@0x5c7df0` [C]. The state each one jumps to is a GAO key resolved by `Humain@0x577a60`;
/// the mapping names are [L].
pub mod order {
    pub const ASSIST: u32 = 1; // -> help/shoot (not entered when already in 0x25f / 0x2c9)
    pub const HEAL: u32 = 2; // -> H_ETAT_ann_heal (0x3ee)
    pub const USE: u32 = 3; // -> H_ETAT_IA_use (0x261)
    pub const MUNITIONS: u32 = 4; // -> H_ETAT_jimmy_munitions (0x3f2)
    pub const CARRY: u32 = 6; // -> H_ETAT_IA_carry (0x262)
}

// ---------------------------------------------------------------------------------------------
// N01: following (H_ETAT_IA_suivi@0x57f1b0)
// ---------------------------------------------------------------------------------------------

/// A companion is considered to have an error big enough to sprint when it is further than this
/// from its desired distance (metres) and not wading (`local_48 > 8.0`, sets `this+0xbc0`) [C].
pub const FOLLOW_SPRINT_ERROR: f32 = 8.0;
/// Extra distance kept while the companion itself is moving (`this+0xc4c != 0`) [C].
pub const FOLLOW_MOVING_BONUS: f32 = 1.5;
/// Distance used when the global rush flag (`G2+0x10cc`) is set [C].
pub const FOLLOW_RUSH_DISTANCE: f32 = 15.0;
/// Distance used (= "do not hold formation, run to the leader") inside a no-follow region
/// (`fn@0x0068a930`) or when `this+0x3cc8` is set [C].
pub const FOLLOW_FAR_DISTANCE: f32 = 100.0;
/// Squared distance to the player under which the "hurry" flag `this+0x19cc` is cleared (4 m) [C].
pub const FOLLOW_HURRY_CLEAR_SQ: f32 = 16.0;
/// Distance used when the hurry flag is set and no override is given [C].
pub const FOLLOW_HURRY_DISTANCE: f32 = 5.0;
/// Seconds spent in the follow state before the actor raises its "available" flag 0x800,
/// which `TrigCINE_WaitDisponibility@0x7664e0` waits for [C].
pub const FOLLOW_AVAILABLE_AFTER_S: f32 = 1.0;

/// Inputs of the desired-distance computation in `H_ETAT_IA_suivi`.
#[derive(Clone, Copy, Debug)]
pub struct FollowInputs {
    pub slot: u32,
    /// `this+0x1930` (integer metres set by script); -1 = automatic formation [C]
    pub override_dist: i32,
    /// `this+0x19cc`
    pub hurry: bool,
    /// squared distance companion -> player
    pub dist_sq_to_player: f32,
    /// formation variant: true when the "tight" branch is taken, i.e. NOT
    /// (`this+0x1004 <= 0.6 || this+0xaf8 <= 1.0 || this+0xbf4 != 0 || !elapsed(this+0x3d6c, 2.0)`) [C cond.]
    pub tight: bool,
    /// `G2+0x10cc`
    pub rush: bool,
    /// `fn@0x0068a930(pos)` (no-follow region) or `this+0x3cc8`
    pub far_override: bool,
    /// `this+0xc4c != 0`
    pub moving: bool,
}

/// Desired distance to the leader (`local_70`), metres. Negative values keep the sign the
/// original uses (they select the "stay within |d|" test of the node search, meaning [L]).
/// Slots without a table entry keep -1.0 exactly like the compiled switch [C].
pub fn follow_distance(i: &FollowInputs) -> f32 {
    let mut hurry = i.hurry;
    if i.dist_sq_to_player < FOLLOW_HURRY_CLEAR_SQ {
        hurry = false;
    }
    let mut d = i.override_dist as f32;
    if hurry && d == 0.0 {
        d = FOLLOW_HURRY_DISTANCE;
    }
    if d == -1.0 {
        if !i.tight {
            d = match i.slot {
                slot::ANN => 4.0,
                slot::HAYES => 7.0,
                slot::DENHAM => 6.0,
                slot::SLOT6 => 5.0,
                _ => -1.0,
            };
        } else {
            d = match i.slot {
                slot::ANN => -5.5,
                slot::HAYES => 3.0,
                slot::DENHAM => -4.5,
                slot::SLOT6 => -4.5,
                _ => -1.0,
            };
        }
    } else {
        d += match i.slot {
            slot::HAYES => 4.0,
            slot::DENHAM => 2.0,
            slot::SLOT6 => 1.0,
            _ => 0.0,
        };
    }
    if i.rush {
        d = FOLLOW_RUSH_DISTANCE;
    }
    if i.far_override {
        d = FOLLOW_FAR_DISTANCE;
    }
    if d != 0.0 && i.moving {
        d += FOLLOW_MOVING_BONUS;
    }
    d
}

/// Sprint request: `8.0 < |desired - actual|` and not wading [C].
pub fn follow_wants_sprint(desired: f32, actual: f32, wading: bool) -> bool {
    !wading && (desired - actual).abs() > FOLLOW_SPRINT_ERROR
}

// ---------------------------------------------------------------------------------------------
// NET_Follower: generic network walker (0x7809a0..0x781fe0)
// ---------------------------------------------------------------------------------------------

/// `NET_Follower_ETAT_wait@0x781ea0`: becomes active when `trigger_flag` is set on `trigger_obj`
/// or the player is within `radius`; then waits `delay` seconds before the run state.
#[derive(Clone, Copy, Debug)]
pub struct NetFollowerWait {
    pub radius: f32,
    pub delay: f32,
    pub armed: bool,
}

impl NetFollowerWait {
    /// One frame. Returns true when the run state must start. `trigger_set` = the flag test on the
    /// configured object, `dist_sq` = player distance squared. `this[0xe]` is the `armed` latch [C].
    pub fn step(&mut self, dt: f32, trigger_set: bool, dist_sq: f32) -> bool {
        if !self.armed {
            if trigger_set || dist_sq < self.radius * self.radius {
                self.armed = true;
            }
        }
        if self.armed {
            if self.delay == 0.0 {
                return true;
            }
            let step = if self.delay < dt { self.delay } else { dt };
            self.delay -= step;
        }
        false
    }
}

/// Speed blend of `NET_Follower_exec_compute_position@0x780a00`: `speed` moves to `target` with
/// `t = clamp(dt * blend)`; blend == -1 snaps [C]. Distance along the edge then grows by `dt * speed`.
pub fn net_follower_speed(speed: f32, target: f32, blend: f32, dt: f32) -> f32 {
    if blend == -1.0 {
        return target;
    }
    let t = (dt * blend).clamp(0.0, 1.0);
    (1.0 - t) * speed + t * target
}

/// Control-handle length used for the smoothed corner curve: `0.33 * edge length` [C].
pub const NET_FOLLOWER_HANDLE: f32 = 0.33;

// ---------------------------------------------------------------------------------------------
// N10/N11: companion wound model (H_exec_ch_Stimulus_Paf@0x5a3060, branch this[0] == 0)
// ---------------------------------------------------------------------------------------------

/// Values of `G+0x1bb4+4*slot` [C]. Note: `wounds::WoundState` labels the same field 1/2/3 as
/// healthy/wounded/dead; the compiled code starts at 0 (`H_TRACK_init`), a hit on 0/2 sets 1 and a
/// hit on 1 sets 3 (see `spec/evidence/N10.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Healthy = 0,
    Wounded = 1,
    Recovering = 2,
    Dead = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitClass {
    /// paf flag 4
    Heavy,
    /// paf flag 2
    Medium,
    Light,
}

/// Hurt duration stored at `this+0xb8f` by the first hit on an AI companion (no difficulty
/// dependence in this branch) [C].
pub fn companion_hurt_duration(c: HitClass) -> f32 {
    match c {
        HitClass::Heavy => 15.5,
        HitClass::Medium => 10.5,
        HitClass::Light => 5.5,
    }
}

/// A wounded companion that is hit again at least this long (s) after being wounded dies [C];
/// earlier hits only refresh the 4.0 s hurt timer (when the paf lacks flag 0x100).
pub const COMPANION_DEATH_AFTER_WOUND_S: f32 = 9.0;
/// Hurt timer refresh on a hit while wounded [C].
pub const COMPANION_WOUNDED_HURT_S: f32 = 4.0;
/// A non-Ann companion left stunned this long (s) goes from Wounded to Recovering [C].
pub const STUNNED_TO_RECOVERING_S: f32 = 8.0;
/// Ann recovers on her own after being stunned this long (s) [C] (`H_ETAT_IA_stunned`, slot 2).
pub const ANN_SELF_RECOVER_S: f32 = 5.0;
/// Recovering -> Healthy after this long (s) (`H_TRACK_Reflex`, `fn@0x0043c690(this+0xf35, 30.0)`) [C].
pub const RECOVERING_TO_HEALTHY_S: f32 = 30.0;
/// `H_ETAT_ann_heal` heal time (s); 2.0 when the patient is Jack and Jack is not dead [C].
pub const HEAL_TIME_S: f32 = 5.0;
pub const HEAL_TIME_JACK_S: f32 = 2.0;
/// Seconds in deep water/hazard flag 0x40 after which an AI companion dies (human: 1.0) [C].
pub const COMPANION_WATER_DEATH_S: f32 = 2.0;
pub const HUMAN_WATER_DEATH_S: f32 = 1.0;

#[derive(Clone, Copy, Debug)]
pub struct CompanionWounds {
    pub slot: u32,
    pub status: Status,
    /// `this+0xb8f`
    pub hurt_timer: f32,
    /// seconds since the wound (`now - this+0xb93`)
    pub since_wound: f32,
    /// time spent stunned (`this+0x2be`)
    pub stunned_for: f32,
    /// time since entering Recovering
    pub recovering_for: f32,
}

impl CompanionWounds {
    pub fn new(slot: u32) -> Self {
        CompanionWounds { slot, status: Status::Healthy, hurt_timer: 0.0, since_wound: 0.0, stunned_for: 0.0, recovering_for: 0.0 }
    }

    /// A hit that reaches `H_exec_ch_Stimulus_Paf` (friendly fire from Jack is dropped by the caller
    /// unless `this[0x2a9]` is set [C]). `flag_100` = paf flag 0x100.
    pub fn hit(&mut self, class: HitClass, flag_100: bool) {
        match self.status {
            Status::Dead => {}
            Status::Wounded => {
                if !flag_100 {
                    self.hurt_timer = COMPANION_WOUNDED_HURT_S;
                }
                if self.since_wound >= COMPANION_DEATH_AFTER_WOUND_S {
                    self.status = Status::Dead;
                }
            }
            Status::Healthy | Status::Recovering => {
                if self.hurt_timer <= 0.5 {
                    self.hurt_timer = companion_hurt_duration(class);
                } else {
                    self.status = Status::Wounded;
                    self.since_wound = 0.0;
                    self.stunned_for = 0.0;
                }
            }
        }
    }

    /// Per-frame bookkeeping (`H_ETAT_IA_stunned`, `H_TRACK_Reflex`); timers decay [L].
    pub fn tick(&mut self, dt: f32) {
        if self.hurt_timer > 0.0 {
            self.hurt_timer = (self.hurt_timer - dt).max(0.0);
        }
        match self.status {
            Status::Wounded => {
                self.since_wound += dt;
                self.stunned_for += dt;
                if self.slot == slot::ANN {
                    if self.stunned_for > ANN_SELF_RECOVER_S {
                        self.status = Status::Healthy;
                    }
                } else if self.stunned_for > STUNNED_TO_RECOVERING_S {
                    self.status = Status::Recovering;
                    self.recovering_for = 0.0;
                }
            }
            Status::Recovering => {
                self.recovering_for += dt;
                if self.recovering_for >= RECOVERING_TO_HEALTHY_S {
                    self.status = Status::Healthy;
                }
            }
            _ => {}
        }
    }

    /// Completed `H_ETAT_ann_heal` on this actor: status 0 (`G+0x1bb4+4*slot = 0`) [C].
    pub fn healed(&mut self) {
        if self.status != Status::Dead {
            self.status = Status::Healthy;
            self.hurt_timer = 0.0;
        }
    }
}

/// Heal duration for a patient (`H_ETAT_ann_heal`, `local_60`) [C].
pub fn heal_time(patient_slot: u32, jack_dead: bool) -> f32 {
    if patient_slot == slot::JACK && !jack_dead {
        HEAL_TIME_JACK_S
    } else {
        HEAL_TIME_S
    }
}

/// Jack's death handling that depends on a healer (`H_ETAT_IA_mort@0x5a3e80`, Jack's `this+0x1a18`
/// is the healer actor): game over when the death timer exceeds 4.0 s and either no healer is
/// assigned or the healer's own timer (`this[0xeca]`) is above 0.5; and when no healer has begun the
/// heal (`healer.flag 0x2e48 == 0`) at 8.0 s [C].
pub fn jack_death_is_final(death_timer: f32, healer_assigned: bool, healer_timer: f32, healer_started: bool) -> bool {
    if death_timer > 4.0 && (!healer_assigned || healer_timer > 0.5) {
        return true;
    }
    if (!healer_assigned || !healer_started) && death_timer > 8.0 {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------------------------
// N04: weapon assistance (H_ETAT_IA_tir@0x5a1610, H_exec_validate_tir@0x5ad900, H_callback_tir)
// ---------------------------------------------------------------------------------------------

/// Aim alignment (cos) an AI companion needs before `H_exec_validate_tir` starts shooting [C]:
/// `dot(forward, dir to target) >= 0.5`.
pub const SHOOT_MIN_ALIGNMENT: f32 = 0.5;
/// Cooldown loaded into `this+0x1124` when the target is closer than the enemy's own distance [C].
pub const SHOOT_RETRY_COOLDOWN_S: f32 = 3.0;
/// Idle time without a target after which the shoot state is left (s) [C].
pub const SHOOT_NO_TARGET_EXIT_S: f32 = 2.0;
/// `H_callback_tir` gate for AI: aim-ready value `this[0x60c]` must be >= this [C].
pub const SHOOT_AIM_READY: f32 = 0.95;
/// Enemy kinds the companion never shoots at (`fn@0x006e2e50(this+0xc7c)`) [C].
pub fn companion_ignores_enemy_kind(kind: u32) -> bool {
    kind == 0x1d || kind == 0x1e
}
/// Maximum engagement distance: half the weapon ray length (shotgun: 8 m) [C] (`H_ETAT_IA_help`).
pub fn companion_engage_distance(weapon: u32, weapon_range: f32) -> f32 {
    if weapon == 3 {
        8.0
    } else {
        weapon_range * 0.5
    }
}
/// Seconds after which the help/assist order is abandoned (`this+0x3d48 > 30.0`) [C].
pub const ASSIST_GIVE_UP_S: f32 = 30.0;
/// Distance at which an assisting companion stops approaching its target (m); 2.0 for the
/// special target `G2+0x10d8` [C].
pub const ASSIST_STOP_DISTANCE: f32 = 5.0;
pub const ASSIST_STOP_DISTANCE_SPECIAL: f32 = 2.0;

/// One row of the Univers per-enemy-kind table the shoot/retreat code reads (`G+0x2fec+4k` safe
/// distance, `G+0x2db4+12k` first engage distance, `G+0x310c+4k` flag); values from the `uni_init.ofc`
/// statements 838-881 (`code/ova/uni_init_all.txt`) [C]. Kind ids are the `PNJ_Raptor` type field
/// (0x10 = 16 T-Rex, 0xe = 14 Venatosaurus, 0x16 = 22 small) [C]; other kinds [L].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemyKindRow {
    pub kind: u32,
    pub safe_dist: f32,
    pub engage_dist: f32,
    pub flag: f32,
}

/// Default row (statements 838/840): safe distance 4, flag 1 [C].
pub const ENEMY_KIND_DEFAULT: EnemyKindRow = EnemyKindRow { kind: 0, safe_dist: 4.0, engage_dist: 0.0, flag: 1.0 };

pub const ENEMY_KIND_ROWS: [EnemyKindRow; 8] = [
    EnemyKindRow { kind: 13, safe_dist: 4.0, engage_dist: 10.0, flag: 1.0 },
    EnemyKindRow { kind: 14, safe_dist: 6.0, engage_dist: 10.0, flag: 5.0 }, // statement 863 overrides 854's 8
    EnemyKindRow { kind: 16, safe_dist: 10.0, engage_dist: 15.0, flag: 10.0 },
    EnemyKindRow { kind: 22, safe_dist: 4.0, engage_dist: 6.0, flag: 5.0 },
    EnemyKindRow { kind: 23, safe_dist: 3.0, engage_dist: 4.0, flag: 1.0 },
    EnemyKindRow { kind: 27, safe_dist: 3.0, engage_dist: 3.0, flag: 1.0 },
    EnemyKindRow { kind: 28, safe_dist: 8.0, engage_dist: 10.0, flag: 5.0 },
    EnemyKindRow { kind: 20, safe_dist: 4.0, engage_dist: 0.0, flag: 3.0 },
];

pub fn enemy_kind_row(kind: u32) -> EnemyKindRow {
    for r in ENEMY_KIND_ROWS.iter() {
        if r.kind == kind {
            return *r;
        }
    }
    EnemyKindRow { kind, ..ENEMY_KIND_DEFAULT }
}

/// Squared radius inside which a shooting companion raises its alert flags (`0xba0`/`0xc4c`):
/// `safe^2 + 2` when the kind's flag is exactly 1.0, else `engage^2 + 4` [C] (`H_ETAT_IA_tir`).
pub fn companion_alert_radius_sq(kind: u32) -> f32 {
    let r = enemy_kind_row(kind);
    if r.flag == 1.0 {
        r.safe_dist * r.safe_dist + 2.0
    } else {
        r.engage_dist * r.engage_dist + 4.0
    }
}

/// `H_ETAT_IA_tir` exit test after 2.0 s in the state: too far (`dist > weapon range / 2`, kind 0x1b uses
/// 6.0) or, unless the previous state was "assist" and the kind is the small 0x16, closer than the
/// kind's safe distance (then the 3.0 s retry cooldown is armed) [C].
pub fn shoot_should_exit(kind: u32, dist: f32, weapon_range: f32, came_from_assist: bool, melee_held: bool) -> (bool, bool) {
    let range = if kind == 0x1b { 6.0 } else { weapon_range };
    let mut exit = range * 0.5 < dist;
    let mut cooldown = false;
    if !came_from_assist && kind != 0x16 && dist < enemy_kind_row(kind).safe_dist && !melee_held {
        exit = true;
        cooldown = true;
    }
    (exit, cooldown)
}

// ---------------------------------------------------------------------------------------------
// N05/I09: weapon swap with a companion (H_ETAT_IA_psss, fn@0x0053a8e0) and Hayes ammo reset
// ---------------------------------------------------------------------------------------------

/// Per-slot weapon data: (clip, reserve) for weapons 1..=4 (index 0 unused).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Arsenal {
    pub clip: [i32; 11],
    pub reserve: [i32; 11],
}

/// `TrigCINE_ResetMunitionHayes@0x7665d0`: zeroes slot 3's clip and reserve of weapons 1..=4 [C].
pub fn reset_munition_hayes(hayes: &mut Arsenal) {
    for w in 1..=4 {
        hayes.clip[w] = 0;
        hayes.reserve[w] = 0;
    }
}

/// Firearm exchange of `Jack_ExchangeWeaponSlots@0x53a8e0` for firearm ids 1..=4: the companion gives
/// weapon `theirs` (with its clip/reserve) and receives weapon `mine`; the ammo travels with the weapon,
/// both old entries are zeroed first [C].
pub fn swap_firearms(jack: &mut Arsenal, companion: &mut Arsenal, mine: usize, theirs: usize) {
    let (c_clip, c_res) = (companion.clip[theirs], companion.reserve[theirs]);
    let (j_clip, j_res) = (jack.clip[mine], jack.reserve[mine]);
    companion.clip[theirs] = 0;
    companion.reserve[theirs] = 0;
    jack.clip[mine] = 0;
    jack.reserve[mine] = 0;
    jack.clip[theirs] = c_clip;
    jack.reserve[theirs] = c_res;
    companion.clip[mine] = j_clip;
    companion.reserve[mine] = j_res;
}

// ---------------------------------------------------------------------------------------------
// I10: carried lever (weapon-slot item 10), E10 missing lever
// ---------------------------------------------------------------------------------------------

/// Item id of the lever in the weapon-slot tables (`G+0x3344/0x3390/0x33dc`) [C].
pub const LEVER_ITEM: u32 = 10;

/// Jack's three weapon cells for one slot plus the lever flag `G+0x4588`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeldItems {
    /// `G+0x3344+4p`: item in hand
    pub current: u32,
    /// `G+0x3390+4p`: stashed item
    pub stash: u32,
    /// `G+0x33dc+4p`: holstered while cranking
    pub holster: u32,
    /// `G+0x4588`
    pub has_lever: bool,
}

impl HeldItems {
    /// Pickup of item 10 (`H_TRACK_Reflex@0x58de70` ~line 555): refused when the lever is already held
    /// (stash or hand), otherwise the hand item goes to the stash if that is empty [C].
    pub fn pick_up_lever(&mut self) -> bool {
        if self.stash == LEVER_ITEM || self.current == LEVER_ITEM {
            return false;
        }
        self.has_lever = true;
        if self.stash == 0 {
            self.stash = self.current;
        }
        self.current = LEVER_ITEM;
        true
    }

    /// Drop (`fn@0x0068c870` case 10, called from nearly every state change): clears the flag, the
    /// stash moves back to the hand, and a holstered lever returns to the stash [C].
    pub fn drop_lever(&mut self) {
        if self.current != LEVER_ITEM {
            return;
        }
        self.has_lever = false;
        self.current = self.stash;
        self.stash = 0;
        if self.holster == LEVER_ITEM {
            self.stash = self.holster;
            self.holster = 0;
        }
    }

    /// Entering the crank state (`H_ETAT_Ia_porte`): a lever in hand is holstered [C].
    pub fn enter_crank(&mut self) {
        if self.current == LEVER_ITEM {
            self.holster = LEVER_ITEM;
            self.current = 0;
        }
    }

    /// Leaving the crank state [C].
    pub fn exit_crank(&mut self) {
        if self.holster == LEVER_ITEM && self.current == 0 {
            self.current = LEVER_ITEM;
            self.holster = 0;
        }
    }

    /// Inserting the lever into a pillar (`InteractiveDoor_loop`): flag cleared; any slot cell that
    /// still holds item 10 is released [C].
    pub fn insert_lever(&mut self) {
        self.has_lever = false;
        if self.current == LEVER_ITEM {
            self.current = self.stash;
            self.stash = 0;
        }
        if self.stash == LEVER_ITEM {
            self.stash = 0;
        }
        if self.holster == LEVER_ITEM {
            self.holster = 0;
        }
    }
}

/// `TrigTest_Levier@0x469940`: level script test on the lever flag, optionally inverted [C].
pub fn trig_test_lever(has_lever: bool, invert: bool) -> bool {
    if invert {
        !has_lever
    } else {
        has_lever
    }
}

// ---------------------------------------------------------------------------------------------
// E09/E10/E11: pillar doors (InteractiveDoor_*, 0x874c30..0x878180)
// ---------------------------------------------------------------------------------------------

/// Crank rotation parameter handed to the operators (`this+0x42c`, rad/s [L]) [C].
pub const CRANK_RATE: f32 = 0.576;
/// Default opening/closing time (s) used when the configured value is negative [C].
pub const DOOR_DEFAULT_TIME_S: f32 = 0.1;
/// The configured closing time is divided by this (`fn@0x0048d940`) [C].
pub const DOOR_CLOSE_DIVISOR: f32 = 3.0;
/// Closing times at/above this never close [C].
pub const DOOR_NEVER_CLOSES: f32 = 10000.0;
/// Interaction prompt radius (squared, 2 m) for lever/pillar prompts [C].
pub const PILLAR_PROMPT_RADIUS_SQ: f32 = 4.0;
/// Prompt id shown on a lever socket when Jack carries a lever / on a free pillar [C].
pub const PROMPT_INSERT_LEVER: u32 = 0xe;
pub const PROMPT_TURN_CRANK: u32 = 9;
/// Door speech: cooldown per cue (s), speaking range (squared, 10 m) and cue ids [C].
pub const DOOR_SPEECH_COOLDOWN_S: f32 = 30.0;
pub const DOOR_SPEECH_RANGE_SQ: f32 = 100.0;
/// Operator waiting this long (s) for its partner makes non-Ann companions complain (speech 0x4c) [C].
pub const CRANK_WAIT_COMPLAIN_S: f32 = 5.0;
/// Offsets used to place an operator at a pillar (m) [C]: back from the pillar and sideways.
pub const CRANK_STAND_BACK: f32 = 0.71;
pub const CRANK_STAND_SIDE: f32 = 0.5;
/// Operator is "in position" within this distance of the stand point (m) [C].
pub const CRANK_IN_POSITION_M: f32 = 0.5;
/// Pillar count allowed per door [C] (`InteractiveDoor_init`: "pilar nb = 1 or 2").
pub const MAX_PILLARS: usize = 2;

/// Opening/closing times after `fn@0x0048d940`: (open_time, close_time, crank_rate) [C].
pub fn door_timing(open_cfg: f32, close_cfg: f32) -> (f32, f32, f32) {
    let open = if open_cfg < 0.0 { DOOR_DEFAULT_TIME_S } else { open_cfg };
    let close = if close_cfg < 0.0 { DOOR_DEFAULT_TIME_S } else { close_cfg };
    (open, close / DOOR_CLOSE_DIVISOR, CRANK_RATE)
}

/// Who is on a pillar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    None,
    /// pillar flag 0x80: Jack is in the crank state
    Player,
    /// pillar flag 0x100: a companion has reached its stand point
    Companion,
}

#[derive(Clone, Copy, Debug)]
pub struct PillarState {
    /// lever flag 1: a lever is mounted on this pillar (E10: otherwise the socket needs one)
    pub lever_mounted: bool,
    pub operator: Operator,
    /// pillar flag 0x1000: operator's turning animation is ready
    pub ready: bool,
}

/// What the door loop does with the door flags (0x200 = opening wanted, 0x400 = held).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// flags cleared: nobody on any pillar
    Idle,
    /// flags 0x600: some but not all pillars operated, or an operator not ready yet: the door neither
    /// opens nor closes
    Hold,
    /// 0x200 set, 0x400 cleared and pillars get 0x4000 (turn): every pillar operated and ready
    Release,
}

/// The multi-operator count of `InteractiveDoor_loop` (`this[0x105]` vs `this[0x99]`) [C].
pub fn pillar_gate(pillars: &[PillarState]) -> Gate {
    let n = pillars.len();
    let operated = pillars
        .iter()
        .filter(|p| p.lever_mounted && p.operator != Operator::None)
        .count();
    if operated == 0 {
        return Gate::Idle;
    }
    if operated == n && pillars.iter().all(|p| p.ready) {
        Gate::Release
    } else {
        Gate::Hold
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Door {
    /// opening ratio 0..1 (`this[0x10f]`)
    pub ratio: f32,
    pub open_time: f32,
    /// already divided by 3
    pub close_time: f32,
}

impl Door {
    pub fn new(open_cfg: f32, close_cfg: f32) -> Self {
        let (open_time, close_time, _) = door_timing(open_cfg, close_cfg);
        Door { ratio: 0.0, open_time, close_time }
    }

    /// One frame of the ratio update at the end of `InteractiveDoor_loop`. `open_test` is the
    /// `mt_OpeningTest` result (level script condition), `gate` the pillar gate [C].
    pub fn step(&mut self, dt: f32, open_test: bool, gate: Gate) {
        let want_open = open_test || gate != Gate::Idle;
        if !want_open {
            if self.close_time >= 0.0 && self.close_time < DOOR_NEVER_CLOSES {
                let d = (dt / self.close_time).min(self.ratio);
                self.ratio -= d;
            }
        } else if self.open_time >= 0.0 {
            if gate != Gate::Hold {
                let d = (dt / self.open_time).min(1.0 - self.ratio);
                self.ratio += d;
            }
        } else {
            self.ratio = 1.0;
        }
        self.ratio = self.ratio.clamp(0.0, 1.0);
    }
}

/// Rotation of a pillar this frame by an operator (`H_ETAT_IA_use` kind 5 and `H_ETAT_Ia_porte`):
/// rate = `this+0x3448` (0 -> 1.0), negated for a mirrored pillar (pillar flag 0x20), times dt [C].
pub fn crank_rotation(rate_param: f32, mirrored: bool, dt: f32) -> f32 {
    let mut r = if rate_param == 0.0 { 1.0 } else { rate_param };
    if mirrored {
        r = -r;
    }
    r * dt
}

/// `fn@0x0048db90`: order the candidate helpers (slot, squared distance to the pillar) by weighted
/// distance. When `weighted` is set slot 2 counts x4, slot 5 x5, slot 6 x2, others x1 (so far-away
/// weighted slots rank later) [C]. At most 10 entries are kept.
pub fn rank_helpers(cands: &[(u32, f32)], weighted: bool) -> Vec<(u32, f32)> {
    let mut v: Vec<(u32, f32)> = cands
        .iter()
        .take(10)
        .map(|&(s, d)| {
            let m = if !weighted {
                1.0
            } else {
                match s {
                    2 => 4.0,
                    5 => 5.0,
                    6 => 2.0,
                    _ => 1.0,
                }
            };
            (s, d * m)
        })
        .collect();
    // insertion sort, stable like the original
    for i in 1..v.len() {
        let mut j = i;
        while j > 0 && v[j - 1].1 > v[j].1 {
            v.swap(j - 1, j);
            j -= 1;
        }
    }
    v
}

/// `InteractiveDoor_exec_speech`: pick the speaker among active companion slots 2..=6, the nearest to
/// the door within `radius_sq`; None when nobody qualifies [C].
pub fn pick_door_speaker(cands: &[(u32, bool, f32)], radius_sq: f32) -> Option<u32> {
    let mut best: Option<(u32, f32)> = None;
    let mut limit = radius_sq;
    for &(slot, active, d) in cands {
        if (2..7).contains(&slot) && active && d < limit {
            limit = d;
            best = Some((slot, d));
        }
    }
    best.map(|b| b.0)
}

/// Speech cue ids of the door (`this+0x44c`) and the sound ids they play [C].
pub fn door_speech_sound(cue: u32, two_levers: bool) -> Option<i32> {
    match cue {
        0 => Some(0x37),
        1 => Some(if two_levers { 0x36 } else { 0x35 }),
        2 => Some(0x34),
        3 => Some(0x39),
        4 => Some(0x38),
        5 => Some(-0x2fff9f9b),
        6 => Some(0x13),
        7 => Some(0x38),
        8 => Some(0x4c),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// E08: breakable wooden obstacle (IOB_*, 0x7a3820..0x7a4020; helpers 0x649200..0x6499b0)
// ---------------------------------------------------------------------------------------------

/// Number of piece slots of an obstacle [C].
pub const OBSTACLE_MAX_PIECES: usize = 10;
/// Debris lifetime before removal (s) [C] (`this+0x200 = 6.0`).
pub const OBSTACLE_DEBRIS_S: f32 = 6.0;
/// Weak hits needed to count as one damage point (the 6th) [C].
pub const OBSTACLE_WEAK_HITS: u32 = 6;

#[derive(Clone, Debug)]
pub struct Obstacle {
    /// per piece: damage needed to release it (`this+0x1d4+4i`)
    pub thresholds: Vec<i32>,
    /// resistance (`this+8`): hits stronger than this destroy everything
    pub resistance: f32,
    /// accumulated damage (`this+0x228`)
    pub damage: i32,
    /// weak-hit counter (`this+0x8c*4`)
    pub weak_hits: u32,
    pub broken: Vec<bool>,
}

impl Obstacle {
    pub fn new(thresholds: Vec<i32>, resistance: f32) -> Self {
        let n = thresholds.len().min(OBSTACLE_MAX_PIECES);
        Obstacle { thresholds: thresholds[..n].to_vec(), resistance, damage: 0, weak_hits: 0, broken: vec![false; n] }
    }

    /// `fn@0x00649200`: damage points produced by one hit event. `flag_10` = paf flag 0x10 (1 point),
    /// `strength` > resistance = 100 points, otherwise weak hits accumulate and the 6th gives 1 [C].
    pub fn damage_of_hit(&mut self, flag_10: bool, strength: f32) -> i32 {
        if flag_10 {
            return 1;
        }
        if self.resistance < strength {
            return 100;
        }
        self.weak_hits += 1;
        if self.weak_hits >= OBSTACLE_WEAK_HITS {
            self.weak_hits = 0;
            return 1;
        }
        0
    }

    /// `IOB_Etat_Go`: apply `dmg` points; returns the indices of the pieces released this frame
    /// and whether the whole obstacle is now broken [C].
    pub fn apply(&mut self, dmg: i32) -> (Vec<usize>, bool) {
        let mut released = Vec::new();
        if dmg > 0 {
            for (i, &t) in self.thresholds.iter().enumerate() {
                if self.damage + dmg >= t && t > self.damage && !self.broken[i] {
                    self.broken[i] = true;
                    released.push(i);
                }
            }
            self.damage += dmg;
        }
        let all = self.broken.iter().all(|&b| b || false) && !self.broken.is_empty();
        (released, all)
    }
}

// ---------------------------------------------------------------------------------------------
// E14/I08: supply drop crate (SA_*, 0x56c5b0..0x56f530)
// ---------------------------------------------------------------------------------------------

/// Crate prompt radius (squared, 2 m) [C].
pub const CRATE_PROMPT_RADIUS_SQ: f32 = 4.0;
/// Prompt id of the crate [C] (`SA_ETAT_Loop` sets interaction type 9).
pub const CRATE_PROMPT_ID: u32 = 9;

/// `SA_init`: ammo the crate offers after subtracting what the player already carries of that weapon
/// (when `this[0xb8] == 0`); a crate whose remaining count is < 1 is removed [C].
pub fn crate_remaining_ammo(crate_ammo: i32, player_clip: i32, player_reserve: i32, subtract: bool) -> Option<i32> {
    let n = if subtract { crate_ammo - (player_clip + player_reserve) } else { crate_ammo };
    if n < 1 {
        None
    } else {
        Some(n)
    }
}

/// `SA_echange`: exchange of the crate content with the taker. Returns
/// `(taker_weapon_after, crate_weapon_after, crate_ammo_after, taker_clip, taker_reserve)`.
/// The taker's old weapon (if different from the crate's) is zeroed and its ammo goes into the crate;
/// the taker receives `min(crate_ammo, clip_size - clip)` in the clip and the rest in reserve,
/// then one round moves from clip to reserve (`-1`/`+1` tail of the function) [C].
pub fn crate_exchange(
    taker_weapon: u32,
    taker_clip: i32,
    taker_reserve: i32,
    crate_weapon: u32,
    crate_ammo: i32,
    clip_size: i32,
) -> (u32, u32, i32, i32, i32) {
    let mut ammo_back = 0;
    let mut old_weapon = 0;
    let (mut clip, mut reserve) = (taker_clip, taker_reserve);
    if taker_weapon != crate_weapon {
        old_weapon = taker_weapon;
        ammo_back = taker_clip + taker_reserve;
        clip = 0;
        reserve = 0;
    }
    let give = crate_ammo;
    let room = clip_size - clip;
    if give < room {
        clip += give;
    } else {
        clip = clip_size;
        reserve += give - room;
    }
    clip -= 1;
    reserve += 1;
    // `if (local_18 == 0) crate weapon = 0 else crate weapon = old weapon, ammo = old ammo` [C]
    let crate_weapon_after = if ammo_back == 0 { 0 } else { old_weapon };
    let crate_ammo_after = ammo_back;
    (crate_weapon, crate_weapon_after, crate_ammo_after, clip, reserve)
}

// ---------------------------------------------------------------------------------------------
// N02/N14: cinematic message queue of a human (H_exec_ch_msg@0x5b3d70)
// ---------------------------------------------------------------------------------------------

/// Cinematic message codes accepted by `H_exec_ch_msg` [C].
pub mod cine {
    pub const WAIT: u32 = 0x186a1;
    pub const VALA: u32 = 0x186a2;
    pub const SPEECH: u32 = 0x186a3;
    pub const CLEAR: u32 = 0x186a4;
    pub const WAIT_DEFAULT: u32 = 0x186a5;
    pub const RESET_WAIT_DEFAULT: u32 = 0x186a6;
    pub const FORCE_ETAT: u32 = 0x186a7;
    pub const CODE_8: u32 = 0x186a8;
    /// queue capacity; overflow logs "Trop de messages cines humains" [C]
    pub const QUEUE_CAPACITY: usize = 0x14;
    /// speech state gives up after this long (s) (`H_CINE_Speech`) [C]
    pub const SPEECH_MAX_S: f32 = 15.0;
    /// `TrigCINE_Speech` waits at most this long (s) for the line to start [C]
    pub const SPEECH_START_WAIT_S: f32 = 3.0;
}

/// Sub-code 5 of `FORCE_ETAT` (give item): value 10 hands Jack the lever, values > 100 are
/// `weapon*100 + flag`, others select the held weapon id directly [C].
pub fn force_etat_give_item(value: i32) -> (Option<u32>, bool, bool) {
    // (weapon id set, lever flag set, G+0x4520 flag set)
    if value == LEVER_ITEM as i32 {
        (None, true, false)
    } else if value > 100 {
        (Some((value / 100) as u32), false, true)
    } else {
        (Some(value as u32), false, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fi(slot: u32, tight: bool) -> FollowInputs {
        FollowInputs {
            slot,
            override_dist: -1,
            hurry: false,
            dist_sq_to_player: 100.0,
            tight,
            rush: false,
            far_override: false,
            moving: false,
        }
    }

    #[test]
    fn formation_distances_per_slot() {
        assert_eq!(follow_distance(&fi(slot::ANN, false)), 4.0);
        assert_eq!(follow_distance(&fi(slot::HAYES, false)), 7.0);
        assert_eq!(follow_distance(&fi(slot::DENHAM, false)), 6.0);
        assert_eq!(follow_distance(&fi(slot::SLOT6, false)), 5.0);
        assert_eq!(follow_distance(&fi(slot::ANN, true)), -5.5);
        assert_eq!(follow_distance(&fi(slot::HAYES, true)), 3.0);
        assert_eq!(follow_distance(&fi(slot::DENHAM, true)), -4.5);
        assert_eq!(follow_distance(&fi(slot::SLOT6, true)), -4.5);
    }

    #[test]
    fn override_rush_far_and_moving() {
        let mut i = fi(slot::HAYES, false);
        i.override_dist = 3;
        assert_eq!(follow_distance(&i), 7.0); // 3 + 4
        i.slot = slot::DENHAM;
        assert_eq!(follow_distance(&i), 5.0); // 3 + 2
        i.moving = true;
        assert_eq!(follow_distance(&i), 6.5); // + 1.5
        i.rush = true;
        assert_eq!(follow_distance(&i), 16.5); // 15 + 1.5
        i.far_override = true;
        assert_eq!(follow_distance(&i), 101.5);
        let mut h = fi(slot::ANN, false);
        h.override_dist = 0;
        h.hurry = true;
        assert_eq!(follow_distance(&h), 5.0);
        h.dist_sq_to_player = 15.9; // inside 4 m clears the hurry flag
        assert_eq!(follow_distance(&h), 0.0);
    }

    #[test]
    fn sprint_threshold_is_eight_metres() {
        assert!(!follow_wants_sprint(4.0, 12.0, false));
        assert!(follow_wants_sprint(4.0, 12.1, false));
        assert!(!follow_wants_sprint(4.0, 30.0, true));
    }

    #[test]
    fn net_follower_wait_arms_and_counts_down() {
        let mut w = NetFollowerWait { radius: 5.0, delay: 0.5, armed: false };
        assert!(!w.step(0.125, false, 100.0));
        assert!(!w.armed);
        assert!(!w.step(0.125, false, 24.0)); // inside 5 m -> armed, delay 0.375
        assert!(w.armed);
        for _ in 0..3 {
            assert!(!w.step(0.125, false, 1000.0));
        }
        assert_eq!(w.delay, 0.0);
        assert!(w.step(0.125, false, 1000.0)); // delay reached 0 -> run state
        assert_eq!(net_follower_speed(1.0, 3.0, -1.0, 0.016), 3.0);
        assert!((net_follower_speed(1.0, 3.0, 2.0, 0.25) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn companion_wound_model() {
        let mut c = CompanionWounds::new(slot::HAYES);
        c.hit(HitClass::Heavy, false);
        assert_eq!(c.status, Status::Healthy);
        assert_eq!(c.hurt_timer, 15.5);
        c.hit(HitClass::Light, false); // second hit while hurt -> wounded
        assert_eq!(c.status, Status::Wounded);
        c.tick(1.0);
        c.hit(HitClass::Light, false); // too early to kill
        assert_eq!(c.status, Status::Wounded);
        assert_eq!(c.hurt_timer, 4.0);
        c.since_wound = 9.0;
        c.hit(HitClass::Light, false);
        assert_eq!(c.status, Status::Dead);
        assert_eq!(companion_hurt_duration(HitClass::Medium), 10.5);
        assert_eq!(companion_hurt_duration(HitClass::Light), 5.5);
    }

    #[test]
    fn stun_recovery_and_ann_self_heal() {
        let mut h = CompanionWounds::new(slot::HAYES);
        h.status = Status::Wounded;
        for _ in 0..(81) {
            h.tick(0.1);
        }
        assert_eq!(h.status, Status::Recovering);
        for _ in 0..301 {
            h.tick(0.1);
        }
        assert_eq!(h.status, Status::Healthy);
        let mut a = CompanionWounds::new(slot::ANN);
        a.status = Status::Wounded;
        for _ in 0..51 {
            a.tick(0.1);
        }
        assert_eq!(a.status, Status::Healthy);
        assert_eq!(heal_time(slot::JACK, false), 2.0);
        assert_eq!(heal_time(slot::JACK, true), 5.0);
        assert_eq!(heal_time(slot::HAYES, false), 5.0);
    }

    #[test]
    fn jack_death_waits_for_a_healer() {
        assert!(jack_death_is_final(4.1, false, 0.0, false));
        assert!(!jack_death_is_final(4.1, true, 0.0, true));
        assert!(jack_death_is_final(4.1, true, 0.6, true));
        assert!(!jack_death_is_final(7.0, true, 0.0, false));
        assert!(jack_death_is_final(8.1, true, 0.0, false));
    }

    #[test]
    fn shooting_rules() {
        assert!(companion_ignores_enemy_kind(0x1d) && companion_ignores_enemy_kind(0x1e));
        assert!(!companion_ignores_enemy_kind(0x0e));
        assert_eq!(companion_engage_distance(3, 25.0), 8.0);
        assert_eq!(companion_engage_distance(1, 50.0), 25.0);
    }

    #[test]
    fn enemy_kind_table_and_retreat() {
        assert_eq!(enemy_kind_row(16).safe_dist, 10.0);
        assert_eq!(enemy_kind_row(14).safe_dist, 6.0);
        assert_eq!(enemy_kind_row(99).safe_dist, 4.0);
        assert_eq!(companion_alert_radius_sq(23), 3.0 * 3.0 + 2.0);
        assert_eq!(companion_alert_radius_sq(16), 15.0 * 15.0 + 4.0);
        // T-Rex at 8 m with a Colt (range 50): inside safe 10 -> retreat with cooldown
        assert_eq!(shoot_should_exit(16, 8.0, 50.0, false, false), (true, true));
        // same distance but came from assist: stay
        assert_eq!(shoot_should_exit(16, 8.0, 50.0, true, false), (false, false));
        // too far: leave without cooldown
        assert_eq!(shoot_should_exit(14, 30.0, 50.0, false, false), (true, false));
        // small creatures never trigger the retreat
        assert_eq!(shoot_should_exit(22, 1.0, 50.0, false, false), (false, false));
    }

    #[test]
    fn hayes_ammo_reset_and_weapon_swap() {
        let mut h = Arsenal::default();
        h.clip[2] = 50;
        h.reserve[3] = 20;
        reset_munition_hayes(&mut h);
        assert_eq!(h, Arsenal::default());
        let mut jack = Arsenal::default();
        let mut comp = Arsenal::default();
        jack.clip[1] = 6;
        jack.reserve[1] = 12;
        comp.clip[3] = 5;
        comp.reserve[3] = 15;
        swap_firearms(&mut jack, &mut comp, 1, 3);
        assert_eq!((jack.clip[3], jack.reserve[3]), (5, 15));
        assert_eq!((comp.clip[1], comp.reserve[1]), (6, 12));
        assert_eq!((jack.clip[1], comp.clip[3]), (0, 0));
    }

    #[test]
    fn lever_pickup_drop_and_crank() {
        let mut h = HeldItems { current: 1, stash: 0, holster: 0, has_lever: false };
        assert!(h.pick_up_lever());
        assert_eq!((h.current, h.stash, h.has_lever), (10, 1, true));
        assert!(!h.pick_up_lever()); // already carried
        h.enter_crank();
        assert_eq!((h.current, h.holster), (0, 10));
        h.exit_crank();
        assert_eq!((h.current, h.holster), (10, 0));
        h.drop_lever();
        assert_eq!((h.current, h.stash, h.has_lever), (1, 0, false));
        assert!(h.pick_up_lever());
        h.insert_lever();
        assert!(!h.has_lever);
        assert_eq!(h.current, 1);
        assert!(trig_test_lever(true, false) && !trig_test_lever(true, true));
    }

    #[test]
    fn door_timing_defaults_and_close_divisor() {
        assert_eq!(door_timing(-1.0, -1.0), (0.1, 0.1 / 3.0, 0.576));
        let (o, c, r) = door_timing(6.0, 9.0);
        assert_eq!((o, c, r), (6.0, 3.0, 0.576));
        assert_eq!(crank_rotation(0.0, false, 0.5), 0.5);
        assert_eq!(crank_rotation(CRANK_RATE, true, 1.0), -0.576);
    }

    #[test]
    fn two_pillar_door_needs_both_operators_ready() {
        let a = PillarState { lever_mounted: true, operator: Operator::Player, ready: true };
        let b_idle = PillarState { lever_mounted: true, operator: Operator::None, ready: false };
        let b_op = PillarState { lever_mounted: true, operator: Operator::Companion, ready: true };
        let b_slow = PillarState { lever_mounted: true, operator: Operator::Companion, ready: false };
        assert_eq!(pillar_gate(&[a, b_idle]), Gate::Hold);
        assert_eq!(pillar_gate(&[a, b_slow]), Gate::Hold);
        assert_eq!(pillar_gate(&[a, b_op]), Gate::Release);
        assert_eq!(pillar_gate(&[b_idle, b_idle]), Gate::Idle);
        let nolever = PillarState { lever_mounted: false, operator: Operator::Player, ready: true };
        assert_eq!(pillar_gate(&[nolever, b_op]), Gate::Hold);
        // single pillar: one operator is enough
        assert_eq!(pillar_gate(&[a]), Gate::Release);

        let mut d = Door::new(2.0, 3.0);
        d.step(1.0, false, Gate::Hold);
        assert_eq!(d.ratio, 0.0); // held: no movement
        d.step(1.0, false, Gate::Release);
        assert!((d.ratio - 0.5).abs() < 1e-6); // 1 s of 2 s
        d.step(1.0, false, Gate::Hold);
        assert!((d.ratio - 0.5).abs() < 1e-6);
        d.step(0.5, false, Gate::Idle); // closes at 3.0/3 = 1 s full travel
        assert!((d.ratio - 0.0).abs() < 1e-6);
        d.step(0.5, true, Gate::Idle); // script test opens it as well
        assert!((d.ratio - 0.25).abs() < 1e-6);
    }

    #[test]
    fn helpers_are_ranked_by_weighted_distance() {
        let c = [(2, 4.0), (3, 9.0), (5, 3.0)];
        let plain = rank_helpers(&c, false);
        assert_eq!(plain.iter().map(|x| x.0).collect::<Vec<_>>(), vec![5, 2, 3]);
        let w = rank_helpers(&c, true);
        // 2 -> 16, 3 -> 9, 5 -> 15
        assert_eq!(w.iter().map(|x| x.0).collect::<Vec<_>>(), vec![3, 5, 2]);
    }

    #[test]
    fn door_speaker_and_sounds() {
        let c = [(1, true, 1.0), (2, true, 50.0), (3, true, 30.0), (4, false, 5.0), (5, true, 120.0)];
        assert_eq!(pick_door_speaker(&c, DOOR_SPEECH_RANGE_SQ), Some(3));
        assert_eq!(pick_door_speaker(&c, 10.0), None);
        assert_eq!(door_speech_sound(1, true), Some(0x36));
        assert_eq!(door_speech_sound(1, false), Some(0x35));
        assert_eq!(DOOR_SPEECH_COOLDOWN_S, 30.0);
    }

    #[test]
    fn obstacle_breaks_in_stages() {
        let mut o = Obstacle::new(vec![1, 3, 100], 5.0);
        assert_eq!(o.damage_of_hit(true, 0.0), 1);
        let (rel, all) = o.apply(1);
        assert_eq!(rel, vec![0]);
        assert!(!all);
        // five weak hits give nothing, the sixth gives one point
        for _ in 0..5 {
            assert_eq!(o.damage_of_hit(false, 1.0), 0);
        }
        assert_eq!(o.damage_of_hit(false, 1.0), 1);
        assert_eq!(o.damage_of_hit(false, 9.0), 100);
        let (rel, all) = o.apply(100);
        assert_eq!(rel, vec![1, 2]);
        assert!(all);
    }

    #[test]
    fn supply_crate_rules() {
        assert_eq!(crate_remaining_ammo(30, 10, 10, true), Some(10));
        assert_eq!(crate_remaining_ammo(20, 10, 10, true), None);
        assert_eq!(crate_remaining_ammo(20, 10, 10, false), Some(20));
        // same weapon: crate adds to the clip first, then reserve; then one round moves clip -> reserve
        let (w, cw, ca, clip, res) = crate_exchange(2, 40, 10, 2, 30, 50);
        assert_eq!((w, cw, ca), (2, 0, 0));
        assert_eq!((clip, res), (50 - 1, 10 + 20 + 1));
        // different weapon: old weapon and ammo go into the crate
        let (w, cw, ca, clip, res) = crate_exchange(1, 6, 12, 3, 8, 5);
        assert_eq!((w, cw, ca), (3, 1, 18));
        assert_eq!((clip, res), (4, 3 + 1));
    }

    #[test]
    fn cine_codes_and_give_item() {
        assert_eq!(cine::WAIT, 100001);
        assert_eq!(cine::FORCE_ETAT, 100007);
        assert_eq!(cine::QUEUE_CAPACITY, 20);
        assert_eq!(force_etat_give_item(10), (None, true, false));
        assert_eq!(force_etat_give_item(203), (Some(2), false, true));
        assert_eq!(force_etat_give_item(3), (Some(3), false, false));
    }
}
