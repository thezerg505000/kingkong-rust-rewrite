//! Kong-level V-Rex (`KT_*`): state machine, life gauge, hit rules and the jaw-break finisher.
//! Ledger B02, X04 (Kong side), KC10, KC11, KC12, KC13. Evidence: `spec/evidence/B02.md`.
//!
//! The original is an AI2C state machine: every `KT_ETAT_*` function stores its id in
//! `Rex+0x3a8`, the previous id in `+0x3ac`, a state timer in `+0x3b4`, and changes state by
//! `GOTO(x)` = "if the active slot is 2 call `fn@0x0043f720(x)` and return 5, else
//! `fn@0x0043f2c0(2,x)` and return 0" - i.e. the first `GOTO` executed in a frame wins.
//! `step()` below applies the transitions in the same priority order as the original code.
//! Rex struct offsets are written `Rex+0xNNN`; names are by use [L] because the KT model is not
//! decoded in `ova/models.json`.

use super::fury::is_zero;

// ---- constants (all [C] unless marked) -----------------------------------------------------------

/// Damage dealt to the rex when a thrown/charging rex hits a wall or Kong's slam lands
/// (`fn@0x00770af0(life, 20.0)` in `KT_ETAT_grabbed`, `_charge`, `_derap`, `_projectile`).
pub const IMPACT_DAMAGE: f32 = 20.0;
/// `Rex+0x2e0` is set to 5.0 whenever life reaches 0 (`KT_TRACK_reflex`, `_charge`, `_grabbed`,
/// `_projectile`, `_derap`, `_JumpAttak`): the knocked-down time. `KT_ETAT_KO_au_sol` holds the
/// rex on the ground for `Rex+0x2e0 - 0.25` s.
pub const KO_SECONDS: f32 = 5.0;
pub const KO_HOLD_TRIM: f32 = 0.25;
/// If `Rex+0xa3c != 0` (already knocked down once) the hold is 2.0 s instead.
pub const KO_REPEAT_SECONDS: f32 = 2.0;
/// When the KO state is left (and the next state is not `grabbed`) a rex below 20.0 life is
/// set to exactly 20.0 (`fn@0x00556780` = `fn@0x00770aa0(life, 20.0)`).
pub const KO_EXIT_LIFE_FLOOR: f32 = 20.0;
/// Three light hits within 2.0 s (`Rex+0x28c`/`0x288`), or two heavy within 2.5 s
/// (`+0x290`/`+0x294`), trigger the rex's counter-attack out of `paf` (`KT_ETAT_paf`).
pub const LIGHT_HITS_FOR_COUNTER: f32 = 3.0;
pub const LIGHT_WINDOW: f32 = 2.0;
pub const HEAVY_HITS_FOR_COUNTER: f32 = 2.0;
pub const HEAVY_WINDOW: f32 = 2.5;
/// Hit-flag bits (`fn@0x0051f190` of the incoming paf): 0x10 = Ann's / non-damaging class,
/// 0x4a (0x2|0x8|0x40) = hits that still hurt a wounded rex, 0x2 = knock-down capable.
pub const FLAG_NO_DAMAGE_CLASS: u32 = 0x10;
pub const FLAGS_HURT_WOUNDED: u32 = 0x4a;
pub const FLAG_KNOCKDOWN: u32 = 0x2;
/// Rex attack ranges from `KT_ETAT_fight_KONG` (`Rex+0xa00` = horizontal distance to target).
pub const MELEE_RANGE: f32 = 8.0;
pub const BITE_RANGE: (f32, f32) = (5.5, 14.0);
pub const FACING_SIDE_COS: f32 = 0.707_106_77;
pub const CHARGE_FACING_COS: f32 = 0.866_025_4;
/// `fight_KONG` entry: first attack is held back by 0.3 s (`Rex+0xbf0 = 0x3e99999a`).
pub const FIGHT_ENTRY_DELAY: f32 = 0.3;
/// Horizontal range of the "fury" shout the rex reacts to (`KT_exec_check_fury`).
pub const FURY_REACT_RADIUS: f32 = 30.0;
/// Finisher proposal: Kong must be within 22.0 of the rex (`KT_exec_propose_grab`).
pub const FINISH_PROPOSE_RADIUS: f32 = 22.0;
/// Rex-side damage packets sent to Kong/Ann (`KT_exec_zdf_zdc`, `KT_ETAT_charge`): (flags, damage).
pub const ATTACK_JAW: (u32, i32) = (2, 10); // mode 1 (also starts a GrabKong bite)
pub const ATTACK_HEADBUTT: (u32, i32) = (1, 10); // mode 2
pub const ATTACK_CHARGE: (u32, i32) = (0x42, 10); // mode 3
pub const ATTACK_BODY_SLAM: (u32, i32) = (0x41, 20); // modes 4 and 8 (flung rex hitting Kong)
pub const ATTACK_SWEEP: (u32, i32) = (1, 10); // mode 5
pub const ATTACK_TAIL: (u32, i32) = (2, 10); // mode 7
/// Jaw-break finisher (`KT_ETAT_Finish`).
pub const FINISH_LOCK_SECONDS: f32 = 2.74;
pub const FINISH_MASH_WINDOW: f32 = 10.0;
pub const FINISH_MIN_FRAME: f32 = 102.0;
pub const DT_MIN: f32 = 0.016;
pub const DT_MAX: f32 = 0.048;

// ---- states -----------------------------------------------------------------------------------

/// `Rex+0x3a8` values; the function names are the AI2C names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum KtState {
    Attente = 0x00,
    Charge = 0x04,
    Paf = 0x0b,
    KoAuSol = 0x0c,
    Projectile = 0x14,
    Chute = 0x15,
    Mort = 0x16,
    JumpAttak = 0x17,
    Attaque = 0x18,
    Derap = 0x1a,
    Cri = 0x1b,
    FightKong = 0x1c,
    FightAnn = 0x1d,
    FightCibleHauteur = 0x1e,
    Grabbed = 0x65,
    Choppe = 0x6a,
    Finish = 0x6c,
    IFinish = 0x6d,
}

impl KtState {
    pub const ALL: [KtState; 18] = [
        KtState::Attente, KtState::Charge, KtState::Paf, KtState::KoAuSol, KtState::Projectile,
        KtState::Chute, KtState::Mort, KtState::JumpAttak, KtState::Attaque, KtState::Derap,
        KtState::Cri, KtState::FightKong, KtState::FightAnn, KtState::FightCibleHauteur,
        KtState::Grabbed, KtState::Choppe, KtState::Finish, KtState::IFinish,
    ];

    pub fn id(self) -> u16 {
        self as u16
    }

    pub fn from_id(id: u16) -> Option<KtState> {
        KtState::ALL.iter().copied().find(|s| s.id() == id)
    }

    pub fn ai2c_name(self) -> &'static str {
        match self {
            KtState::Attente => "KT_ETAT_attente",
            KtState::Charge => "KT_ETAT_charge",
            KtState::Paf => "KT_ETAT_paf",
            KtState::KoAuSol => "KT_ETAT_KO_au_sol",
            KtState::Projectile => "KT_ETAT_projectile",
            KtState::Chute => "KT_ETAT_chute",
            KtState::Mort => "KT_ETAT_mort",
            KtState::JumpAttak => "KT_ETAT_JumpAttak",
            KtState::Attaque => "KT_ETAT_attaque",
            KtState::Derap => "KT_ETAT_derap",
            KtState::Cri => "KT_ETAT_cri",
            KtState::FightKong => "KT_ETAT_fight_KONG",
            KtState::FightAnn => "KT_ETAT_fight_ANN",
            KtState::FightCibleHauteur => "KT_ETAT_fight_cible_hauteur",
            KtState::Grabbed => "KT_ETAT_grabbed",
            KtState::Choppe => "KT_ETAT_choppe",
            KtState::Finish => "KT_ETAT_Finish",
            KtState::IFinish => "KT_ETAT_I_Finish",
        }
    }

    /// `KT_ETAT_fight_KONG`, `_fight_ANN`, `_fight_cible_hauteur` (`fn@0x00556d40`).
    pub fn is_fight_state(self) -> bool {
        matches!(self, KtState::FightKong | KtState::FightAnn | KtState::FightCibleHauteur)
    }
}

// ---- life gauge -------------------------------------------------------------------------------

/// The rex's "Life" variable (AI type 0x1e at `Rex+0x880`): slot 1 = (max, current, wound ratio).
/// `KT_TRACK_init` builds it with `fn@0x00770900`; max/current/ratio come from the level instance
/// data (`Rex+0xf4/0xf8/0xfc/0x100`) and are not constants of the exe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KtLife {
    pub max: f32,
    pub cur: f32,
    pub wound_ratio: f32,
}

impl KtLife {
    pub fn new(max: f32, cur: f32, wound_ratio: f32) -> Self {
        KtLife { max, cur, wound_ratio }
    }

    /// `fn@0x00770af0(life, d)`: `cur = max(cur - d, 0)`.
    pub fn apply_damage(&mut self, d: f32) {
        let v = self.cur - d;
        self.cur = if v <= 0.0 { 0.0 } else { v };
    }

    /// `fn@0x00770aa0(life, v)`: set current life.
    pub fn set(&mut self, v: f32) {
        self.cur = v;
    }

    /// `fn@0x00770b70`: "Blesse" (wounded) when `cur / max <= ratio`.
    pub fn is_wounded(&self) -> bool {
        self.cur / self.max <= self.wound_ratio
    }

    pub fn is_dead(&self) -> bool {
        self.cur <= 0.0
    }

    /// Pace factor used for the roar/hit sound: `max(cur/max, 0.1)` (`KT_TRACK_reflex`).
    pub fn ratio_floor(&self) -> f32 {
        (self.cur / self.max).max(0.1)
    }
}

/// `KT_exec_check_paf` damage rule for one incoming hit: damage applies when the hit is not in
/// the 0x10 class and (the rex is not wounded, or the accumulated flags this frame contain one of
/// 0x4a). Returns the damage to subtract (`fn@0x0051f110(hit)`) or 0.
pub fn hit_damage(flags: u32, accumulated_flags: u32, wounded: bool, raw_damage: f32) -> f32 {
    if flags & FLAG_NO_DAMAGE_CLASS == 0 && (!wounded || accumulated_flags & FLAGS_HURT_WOUNDED != 0) {
        raw_damage
    } else {
        0.0
    }
}

/// `fn@0x00559f20(flags)`: can this hit from Kong knock the rex down (it then calls
/// `fn@0x00606dd0(kong, 0)` = Kong's "hit connected" feedback)? Life must be non-zero, flag 0x10
/// clear, and either Kong is in fury or (the rex is not in `JumpAttak` and flag 0x2 is set).
pub fn knockdown_capable(flags: u32, life_cur: f32, kong_fury: bool, state: KtState) -> bool {
    if life_cur == 0.0 || flags & FLAG_NO_DAMAGE_CLASS != 0 {
        return false;
    }
    if kong_fury {
        return true;
    }
    state != KtState::JumpAttak && flags & FLAG_KNOCKDOWN != 0
}

/// Rolling hit counters that decide the counter-attack (`KT_TRACK_reflex` ages them,
/// `KT_exec_check_paf` increments them, `KT_ETAT_paf` consumes them).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitCounters {
    pub light: f32,
    pub light_age: f32,
    pub heavy: f32,
    pub heavy_age: f32,
}

impl HitCounters {
    /// A Kong hit was registered: flag 0x10 -> heavy counter, else light counter; the age resets.
    pub fn register(&mut self, flags: u32) {
        if flags & FLAG_NO_DAMAGE_CLASS == 0 {
            self.light_age = 0.0;
            self.light += 1.0;
        } else {
            self.heavy_age = 0.0;
            self.heavy += 1.0;
        }
    }

    /// Per-frame ageing in `KT_TRACK_reflex`: counters clear after 2.0 s / 2.5 s.
    pub fn tick(&mut self, dt: f32) {
        self.light_age += dt;
        if self.light > 0.0 && self.light_age > LIGHT_WINDOW {
            self.light = 0.0;
        }
        self.heavy_age += dt;
        if self.heavy > 0.0 && self.heavy_age > HEAVY_WINDOW {
            self.heavy = 0.0;
        }
    }

    /// `KT_ETAT_paf` while the hit animation plays past frame 19: counter-attack due? Consumes.
    pub fn take_counter(&mut self) -> bool {
        let mut go = false;
        if self.light >= LIGHT_HITS_FOR_COUNTER {
            self.light = 0.0;
            go = true;
        }
        if self.heavy >= HEAVY_HITS_FOR_COUNTER {
            self.heavy = 0.0;
            go = true;
        }
        go
    }
}

// ---- attack selection (fight_KONG) --------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackKind {
    /// mode 5, anims 0xc/0xd (target in front)
    Sweep,
    /// mode 7, anims 0xe/0xf (target beside/behind)
    Tail,
    /// mode 1 bite/lunge, anim from `fn@0x00556540` (4, 0xa or 0xb)
    Bite,
}

/// Distance-band part of `KT_ETAT_fight_KONG`: with a clear line, dist <= 8.0 -> melee swing
/// (`facing_cos <= 0.7071` -> tail else sweep); 5.5 <= dist <= 14.0 -> bite. Both need the
/// attack cooldown (`Rex+0x210 == 0 || Rex+0xbf0 == 0`) to be ready.
pub fn choose_attack(dist: f32, facing_cos: f32, cooldown_ready: bool) -> Option<AttackKind> {
    if !cooldown_ready {
        return None;
    }
    if dist <= MELEE_RANGE {
        return Some(if facing_cos <= FACING_SIDE_COS { AttackKind::Tail } else { AttackKind::Sweep });
    }
    if dist >= BITE_RANGE.0 && dist <= BITE_RANGE.1 {
        return Some(AttackKind::Bite);
    }
    None
}

/// `KT_ETAT_fight_KONG` charge test: the minimum distance depends on whether Kong faces the rex
/// (`dot(kong forward, rex->kong) <= 0`): 20.0 if he does, 10.0 if he faces away, 0.0 while Kong is
/// in the grab-mash action 0x160 (`Rex+0xbb0`). The rex must also face Kong within 30 degrees
/// (`cos > 0.866`) and `fn@0x00559db0` (cool-down vs `Rex+0x3a4`) must allow it [C values, L meaning].
pub fn charge_min_distance(kong_faces_rex: bool, kong_mashing: bool) -> f32 {
    if kong_mashing {
        0.0
    } else if kong_faces_rex {
        20.0
    } else {
        10.0
    }
}

// ---- knockdown / finishers ----------------------------------------------------------------------

/// Knocked-down hold time: `Rex+0x2e0 - 0.25` normally, 2.0 if knocked down before (`KT_ETAT_KO_au_sol`).
pub fn ko_hold_seconds(ko_time: f32, knocked_down_before: bool) -> f32 {
    if knocked_down_before { KO_REPEAT_SECONDS } else { ko_time - KO_HOLD_TRIM }
}

/// Life after leaving the KO state (`fn@0x00556780` if `life < 20`), unless the next state is `grabbed`.
pub fn ko_exit_life(cur: f32, next_is_grabbed: bool) -> f32 {
    if !next_is_grabbed && cur < KO_EXIT_LIFE_FLOOR { KO_EXIT_LIFE_FLOOR } else { cur }
}

/// `KT_exec_propose_grab`: does the rex accept a *finish* proposal (message mask bit 8) from Kong?
/// Needs: state KO (0xc), height difference <= 5.0, wounded (`Rex+0x87c`), the KO window flag
/// (`Rex+0x4e8`), and Kong within 22.0 (`fn@0x00445d70`).
pub fn finish_accepts(state: KtState, dz: f32, wounded: bool, ko_window: bool, dist: f32) -> bool {
    state == KtState::KoAuSol && dz.abs() <= 5.0 && wounded && ko_window && dist < FINISH_PROPOSE_RADIUS
}

/// Outcome of one `KT_ETAT_grabbed` grab move, from the rex side (KC12): anim ids from the code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrabEnd {
    /// move finished and the rex still has life: back to `fight_KONG` (or KO if life == 0)
    Released,
    /// thrown (anim 0x7a): becomes `projectile`; impact does 20.0 damage; KO if life hits 0
    Thrown,
    /// ground slam (anims 0x8d/0xa1): life 0 -> `mort`
    Slammed,
}

/// End of a grab move: `anim` is the rex animation id at the end of `KT_ETAT_grabbed`.
pub fn grab_end_state(anim: u16, life: f32) -> KtState {
    match anim {
        0x7a => {
            if life == 0.0 { KtState::KoAuSol } else { KtState::FightKong }
        }
        0x8d | 0xa1 => {
            if life == 0.0 { KtState::Mort } else { KtState::FightKong }
        }
        _ => KtState::Projectile,
    }
}

// ---- jaw-break finisher mash (KC11 / KC13) ------------------------------------------------------

/// State of the tug-of-war in `KT_ETAT_Finish` while Kong's finish message type is 0x15:
/// `progress` = `Rex+0x858` (animation frame cursor), `pull` = `+0x860` (Kong's force),
/// `resist` = `+0x864` (rex's resistance). All in animation frames per frame [C values, L meaning].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FinishMash {
    pub progress: f32,
    pub pull: f32,
    pub resist: f32,
    pub elapsed: f32,
    /// `Rex+0x85c`: the rex has broken free (replies 0x18 to Kong)
    pub escaped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FinishOutcome {
    Running,
    /// progress passed the end of the animation: rex replies 0x16, Kong sends 0x17, -> `mort`
    Won,
    /// at 2.74 s the cursor was < 102 frames: rex replies 0x18 and plays the shake-off anim 0x39
    Escaped,
}

impl FinishMash {
    pub fn new() -> Self {
        FinishMash { progress: 0.0, pull: 0.0, resist: 0.0, elapsed: 0.0, escaped: false }
    }

    /// One frame. `kong_pressing` = `fn@0x006039d0(kong)` (Kong's mash buttons active),
    /// `kong_value` = `fn@0x00602e40(kong)` in 0..1 (pull animation phase), `kong_fury` = fury on,
    /// `anim_frame` = current frame of the rex animation (used until the lock phase ends),
    /// `anim_len` = `fn@0x00425e60` frames.
    pub fn step(&mut self, dt: f32, kong_pressing: bool, kong_value: f32, kong_fury: bool, anim_frame: f32, anim_len: f32) -> FinishOutcome {
        self.elapsed += dt;
        if self.elapsed < FINISH_LOCK_SECONDS {
            self.progress = anim_frame;
            return FinishOutcome::Running;
        }
        if !self.escaped && self.progress < FINISH_MIN_FRAME {
            self.escaped = true;
            return FinishOutcome::Escaped;
        }
        let window = if self.pull - self.resist < -2.5 { 0.0 } else { FINISH_MASH_WINDOW };
        let d = dt.clamp(DT_MIN, DT_MAX);
        if self.pull < 2.0 {
            self.resist = (self.resist + d * 2.0).min(10.0);
        }
        let decay = if kong_fury { 3.0 } else { 20.0 };
        self.pull = (self.pull - decay * d).max(0.0);
        if self.elapsed < window + FINISH_LOCK_SECONDS && kong_pressing {
            self.pull = (0.5 - kong_value * 0.5) + 3.0;
            self.resist = (self.resist - 0.5).max(0.0);
        }
        self.progress += self.pull;
        self.progress -= self.resist;
        if self.progress > anim_len { FinishOutcome::Won } else { FinishOutcome::Running }
    }
}

impl Default for FinishMash {
    fn default() -> Self {
        FinishMash::new()
    }
}

// ---- state machine ----------------------------------------------------------------------------

/// Which creature the rex is fighting (`Rex+0x204`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    None,
    Kong,
    Ann,
}

/// Kong -> rex grab/finish message kinds (`fn@0x006e2e50` of the polled 0xC message, `Rex+0x494`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrabMsg {
    /// mask bit 1: Kong grabs the rex
    Grab,
    /// mask bit 8: Kong starts the jaw-break finisher (only accepted while KO)
    Finish,
}

/// Everything one `step()` reads. Field comments give the original variable.
#[derive(Clone, Copy, Debug)]
pub struct KtInput {
    pub dt: f32,
    /// `fn@0x00554470()`: no floor under the rex
    pub no_floor: bool,
    /// `Rex+0x2a4 != 0` after `KT_exec_check_paf`: a hit landed this frame
    pub hit: bool,
    /// the polled 0xC message (`Rex+0x48c`)
    pub grab_msg: Option<GrabMsg>,
    /// result of `KT_exec_fight_actor_select`
    pub target: Target,
    /// `Rex+0xa00`
    pub dist: f32,
    /// dot(rex forward, direction to target)
    pub facing_cos: f32,
    /// `fn@0x00424940`: current animation has ended
    pub anim_done: bool,
    /// `Rex+0x21c != 0` (fury shout heard) - reflex
    pub fury_shout: bool,
    /// KT_exec_check_msg type 4 (jump attack request) / type 1 (choppe) / type 9 (charge target)
    pub msg_jump_attack: bool,
    pub msg_choppe: bool,
    pub charge_target: bool,
    /// `Rex+0xbf0 == 0 || Rex+0x210 == 0`
    pub attack_ready: bool,
    /// path/line to target is clear (`Rex+0x9c8 >= 0.5`)
    pub line_clear: bool,
    /// `fn@0x00559db0`: charge cool-down elapsed
    pub charge_ready: bool,
    /// Kong is fury-roaring / target is in a finish (`fn@0x00603f00(kong,0x10)`)
    pub target_in_finish: bool,
    /// Kong in fury (`fn@0x006038a0`)
    pub kong_fury: bool,
    /// Kong is in the grab-mash action 0x160 (`Rex+0xbb0`)
    pub kong_mashing: bool,
    /// `dot(kong forward, rex->kong) <= 0`: Kong is facing the rex
    pub kong_faces_rex: bool,
    /// counter-attack due while in `paf` and past animation frame 19 (see [`HitCounters::take_counter`])
    pub counter_due: bool,
    /// the attacker of the current hit is Ann
    pub hit_by_ann: bool,
    /// the hit carried flag 0x20000 (kill hit) - used in KO
    pub kill_hit: bool,
    /// a wall/obstacle was struck while charging
    pub charge_blocked: bool,
    /// `Rex+0x390 == 1`: charge has been stopped
    pub charge_stopped: bool,
    /// at the end of a grab move: animation id (see [`grab_end_state`])
    pub grab_anim: u16,
    /// Kong finished the finisher (message 0x17)
    pub finish_done: bool,
    /// the rex escaped the jaw-break (`FinishOutcome::Escaped`)
    pub finish_escaped: bool,
    /// ground reached after a fall
    pub grounded: bool,
}

impl Default for KtInput {
    fn default() -> Self {
        KtInput {
            dt: 1.0 / 60.0,
            no_floor: false,
            hit: false,
            grab_msg: None,
            target: Target::None,
            dist: 0.0,
            facing_cos: 1.0,
            anim_done: false,
            fury_shout: false,
            msg_jump_attack: false,
            msg_choppe: false,
            charge_target: false,
            attack_ready: true,
            line_clear: true,
            charge_ready: false,
            target_in_finish: false,
            kong_fury: false,
            kong_mashing: false,
            kong_faces_rex: false,
            counter_due: false,
            hit_by_ann: false,
            kill_hit: false,
            charge_blocked: false,
            charge_stopped: false,
            grab_anim: 0,
            finish_done: false,
            finish_escaped: false,
            grounded: false,
        }
    }
}

/// Mutable machine state: current/previous state, state timer, life, KO bookkeeping.
#[derive(Clone, Debug)]
pub struct KtMachine {
    pub state: KtState,
    pub prev: KtState,
    /// `Rex+0x3b4`
    pub timer: f32,
    pub life: KtLife,
    /// `Rex+0x3b8`: state to return to when a reaction state ends (default `FightKong`)
    pub return_state: KtState,
    /// `Rex+0x2e0`
    pub ko_time: f32,
    /// `Rex+0xa3c`
    pub knocked_down_before: bool,
    /// `Rex+0x2d8`: time left lying down
    pub ko_left: f32,
    /// `Rex+0xbf0`
    pub entry_delay: f32,
    /// the attack chosen on the last `FightKong -> Attaque` transition
    pub last_attack: Option<AttackKind>,
}

impl KtMachine {
    pub fn new(life: KtLife) -> Self {
        KtMachine {
            state: KtState::Attente,
            prev: KtState::Attente,
            timer: 0.0,
            life,
            return_state: KtState::FightKong,
            ko_time: 0.0,
            knocked_down_before: false,
            ko_left: 0.0,
            entry_delay: 0.0,
            last_attack: None,
        }
    }

    fn enter(&mut self, s: KtState) -> KtState {
        if s == self.state {
            return s;
        }
        // leaving KO: revive floor (fn@0x00556780) unless going to grabbed
        if self.state == KtState::KoAuSol {
            self.life.cur = ko_exit_life(self.life.cur, s == KtState::Grabbed);
            self.knocked_down_before = true;
        }
        self.prev = self.state;
        self.state = s;
        self.timer = 0.0;
        match s {
            KtState::KoAuSol => {
                if self.ko_time == 0.0 && self.prev != KtState::Chute {
                    self.ko_time = KO_SECONDS;
                }
                self.ko_left = ko_hold_seconds(self.ko_time, self.knocked_down_before);
            }
            KtState::FightKong => self.entry_delay = FIGHT_ENTRY_DELAY,
            KtState::Cri => {
                if self.return_state == KtState::Cri {
                    self.return_state = KtState::FightKong;
                }
            }
            _ => {}
        }
        s
    }

    /// Force the KO with the standard 5.0 s (`Rex+0x2e0 = 5.0`).
    pub fn start_ko(&mut self) -> KtState {
        self.ko_time = KO_SECONDS;
        self.enter(KtState::KoAuSol)
    }

    /// One frame of `KT_TRACK_reflex` followed by the active `KT_ETAT_*`. Returns the state for
    /// the next frame. The first transition found wins, in the original order.
    pub fn step(&mut self, i: &KtInput) -> KtState {
        self.timer += i.dt;
        if self.entry_delay > 0.0 {
            self.entry_delay = (self.entry_delay - i.dt).max(0.0);
        }
        // ---- KT_TRACK_reflex (runs before the state) ----
        let s = self.state;
        if s == KtState::Mort {
            return s;
        }
        if i.fury_shout && (s.is_fight_state() || s == KtState::Attente || s == KtState::Attaque) {
            return self.enter(KtState::Cri);
        }
        if i.msg_jump_attack && s != KtState::JumpAttak {
            return self.enter(KtState::JumpAttak);
        }
        if i.msg_choppe {
            return self.enter(KtState::Choppe);
        }
        if i.charge_target && s != KtState::Charge {
            return self.enter(KtState::Charge);
        }
        if self.life.is_dead() && (s.is_fight_state() || s == KtState::Attente) {
            return self.start_ko();
        }
        // ---- the state itself ----
        match s {
            KtState::Attente => {
                if i.no_floor {
                    return self.enter(KtState::Chute);
                }
                if i.hit {
                    return self.enter(KtState::Paf);
                }
                if i.grab_msg.is_some() {
                    return self.enter(KtState::Grabbed);
                }
                match i.target {
                    Target::Ann => self.enter(KtState::FightAnn),
                    Target::Kong => self.enter(KtState::FightKong),
                    Target::None => s,
                }
            }
            KtState::FightKong | KtState::FightAnn | KtState::FightCibleHauteur => {
                if i.grab_msg.is_some() {
                    return self.enter(KtState::Grabbed);
                }
                if i.hit {
                    return self.enter(KtState::Paf);
                }
                if i.no_floor {
                    return self.enter(KtState::Chute);
                }
                match i.target {
                    Target::None => return self.enter(KtState::Attente),
                    Target::Ann if s == KtState::FightKong => return self.enter(KtState::FightAnn),
                    Target::Kong if s == KtState::FightAnn => return self.enter(KtState::FightKong),
                    _ => {}
                }
                if s == KtState::FightKong && i.line_clear && !i.kong_mashing {
                    if i.dist <= MELEE_RANGE && i.target_in_finish {
                        return self.enter(KtState::Cri);
                    }
                    if self.entry_delay == 0.0 || i.attack_ready {
                        if let Some(a) = choose_attack(i.dist, i.facing_cos, i.attack_ready) {
                            self.last_attack = Some(a);
                            return self.enter(KtState::Attaque);
                        }
                    }
                    let min_d = charge_min_distance(i.kong_faces_rex, false);
                    if i.dist >= min_d && i.facing_cos > CHARGE_FACING_COS && i.charge_ready {
                        if i.target_in_finish || i.kong_fury {
                            return self.enter(KtState::Cri);
                        }
                        return self.enter(KtState::Charge);
                    }
                }
                s
            }
            KtState::Attaque => {
                if i.grab_msg.is_some() {
                    return self.enter(KtState::Grabbed);
                }
                if i.hit {
                    return self.enter(KtState::Paf);
                }
                if i.anim_done {
                    let r = self.return_state;
                    return self.enter(r);
                }
                s
            }
            KtState::Cri => {
                if i.grab_msg.is_some() {
                    return self.enter(KtState::Grabbed);
                }
                if i.hit {
                    return self.enter(KtState::Paf);
                }
                if i.no_floor {
                    return self.enter(KtState::Chute);
                }
                if i.anim_done || self.timer > 3.0 {
                    let r = self.return_state;
                    return self.enter(r);
                }
                s
            }
            KtState::Charge => {
                if i.grab_msg.is_some() {
                    return self.enter(KtState::Grabbed);
                }
                if i.no_floor {
                    return self.enter(KtState::Chute);
                }
                if i.hit {
                    return self.enter(KtState::Paf);
                }
                if i.charge_blocked {
                    self.life.apply_damage(IMPACT_DAMAGE);
                    if self.life.is_dead() {
                        return self.start_ko();
                    }
                    return self.enter(KtState::Paf);
                }
                if i.charge_stopped {
                    return self.enter(if i.target == Target::Ann { KtState::FightAnn } else { KtState::Derap });
                }
                s
            }
            KtState::Derap => {
                if i.grab_msg.is_some() {
                    return self.enter(KtState::Grabbed);
                }
                if i.hit {
                    return self.enter(KtState::Paf);
                }
                if i.charge_blocked {
                    self.life.apply_damage(IMPACT_DAMAGE);
                    if self.life.is_dead() {
                        return self.start_ko();
                    }
                    return self.enter(KtState::Paf);
                }
                if i.anim_done {
                    return self.enter(KtState::FightKong);
                }
                s
            }
            KtState::Paf => {
                if i.no_floor {
                    return self.enter(KtState::Chute);
                }
                if self.life.is_dead() {
                    return self.start_ko();
                }
                // the counter is tested while the hit animation is still playing, past frame 19
                if !i.anim_done && i.counter_due {
                    self.last_attack = Some(AttackKind::Sweep);
                    return self.enter(KtState::Attaque);
                }
                if i.anim_done {
                    return self.enter(if i.hit_by_ann { KtState::FightAnn } else { KtState::FightKong });
                }
                s
            }
            KtState::KoAuSol => {
                if i.no_floor {
                    return self.enter(KtState::Chute);
                }
                match i.grab_msg {
                    Some(GrabMsg::Finish) => return self.enter(KtState::Finish),
                    Some(GrabMsg::Grab) => return self.enter(KtState::Grabbed),
                    None => {}
                }
                if i.hit {
                    if i.kill_hit {
                        return self.enter(KtState::Mort);
                    }
                    return self.enter(KtState::Paf);
                }
                self.ko_left -= i.dt;
                if self.ko_left <= 0.0 && i.anim_done {
                    let r = self.return_state;
                    return self.enter(r);
                }
                s
            }
            KtState::Chute => {
                if i.grounded {
                    if self.prev == KtState::Mort {
                        return self.enter(KtState::Mort);
                    }
                    self.ko_time = 0.0;
                    return self.enter(KtState::KoAuSol);
                }
                s
            }
            KtState::Projectile => {
                if i.hit {
                    return self.enter(KtState::Paf);
                }
                if i.charge_blocked {
                    self.life.apply_damage(IMPACT_DAMAGE);
                    if self.life.is_dead() {
                        return self.start_ko();
                    }
                    return self.enter(KtState::Derap);
                }
                if i.anim_done {
                    return self.enter(KtState::FightKong);
                }
                s
            }
            KtState::Grabbed => {
                if i.hit {
                    return self.enter(KtState::Paf);
                }
                if i.no_floor {
                    return self.enter(KtState::Chute);
                }
                if i.anim_done || i.grab_anim != 0 {
                    let n = grab_end_state(i.grab_anim, self.life.cur);
                    if n == KtState::KoAuSol {
                        return self.start_ko();
                    }
                    return self.enter(n);
                }
                s
            }
            KtState::Finish => {
                if i.finish_done {
                    return self.enter(KtState::Mort);
                }
                if i.finish_escaped {
                    return self.enter(KtState::FightKong);
                }
                s
            }
            KtState::JumpAttak => {
                if i.hit {
                    return self.enter(KtState::Paf);
                }
                if i.grab_msg.is_some() {
                    return self.enter(KtState::Grabbed);
                }
                if self.timer > 3.0 || i.anim_done {
                    if self.life.is_dead() {
                        self.ko_time = 0.0;
                    } else {
                        self.ko_time = KO_SECONDS;
                    }
                    return self.enter(KtState::KoAuSol);
                }
                s
            }
            KtState::Choppe => {
                if i.grab_msg.is_some() {
                    return self.enter(KtState::Grabbed);
                }
                if i.hit {
                    return self.enter(KtState::Paf);
                }
                if i.no_floor {
                    return self.enter(KtState::Chute);
                }
                if i.anim_done || self.timer > 2.0 {
                    return self.enter(KtState::Attente);
                }
                s
            }
            KtState::IFinish => {
                if i.anim_done {
                    return self.enter(KtState::FightKong);
                }
                s
            }
            KtState::Mort => s,
        }
    }
}

/// `is_zero` re-export for callers that only import this module.
pub fn life_is_zero(x: f32) -> bool {
    is_zero(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rex() -> KtMachine {
        KtMachine::new(KtLife::new(100.0, 100.0, 0.5))
    }

    #[test]
    fn state_ids_match_the_code() {
        assert_eq!(KtState::Attente.id(), 0x00);
        assert_eq!(KtState::Charge.id(), 0x04);
        assert_eq!(KtState::Paf.id(), 0x0b);
        assert_eq!(KtState::KoAuSol.id(), 0x0c);
        assert_eq!(KtState::Projectile.id(), 0x14);
        assert_eq!(KtState::Chute.id(), 0x15);
        assert_eq!(KtState::Mort.id(), 0x16);
        assert_eq!(KtState::JumpAttak.id(), 0x17);
        assert_eq!(KtState::Attaque.id(), 0x18);
        assert_eq!(KtState::Derap.id(), 0x1a);
        assert_eq!(KtState::Cri.id(), 0x1b);
        assert_eq!(KtState::FightKong.id(), 0x1c);
        assert_eq!(KtState::FightAnn.id(), 0x1d);
        assert_eq!(KtState::FightCibleHauteur.id(), 0x1e);
        assert_eq!(KtState::Grabbed.id(), 0x65);
        assert_eq!(KtState::Choppe.id(), 0x6a);
        assert_eq!(KtState::Finish.id(), 0x6c);
        assert_eq!(KtState::IFinish.id(), 0x6d);
        for s in KtState::ALL {
            assert_eq!(KtState::from_id(s.id()), Some(s));
        }
    }

    #[test]
    fn life_gauge_matches_fun_00770af0() {
        let mut l = KtLife::new(100.0, 30.0, 0.5);
        assert!(l.is_wounded());
        l.apply_damage(20.0);
        assert_eq!(l.cur, 10.0);
        l.apply_damage(IMPACT_DAMAGE);
        assert_eq!(l.cur, 0.0);
        assert!(l.is_dead());
        assert!(!KtLife::new(100.0, 51.0, 0.5).is_wounded());
    }

    #[test]
    fn damage_rule_wounded_rex_only_hurt_by_0x4a_flags() {
        assert_eq!(hit_damage(0x1, 0x1, false, 10.0), 10.0);
        assert_eq!(hit_damage(0x1, 0x1, true, 10.0), 0.0);
        assert_eq!(hit_damage(0x2, 0x2, true, 10.0), 10.0);
        assert_eq!(hit_damage(0x10, 0x10, false, 10.0), 0.0);
    }

    #[test]
    fn knockdown_needs_fury_or_flag_2() {
        assert!(!knockdown_capable(0x1, 50.0, false, KtState::FightKong));
        assert!(knockdown_capable(0x2, 50.0, false, KtState::FightKong));
        assert!(!knockdown_capable(0x2, 50.0, false, KtState::JumpAttak));
        assert!(knockdown_capable(0x1, 50.0, true, KtState::FightKong));
        assert!(!knockdown_capable(0x2, 0.0, true, KtState::FightKong));
        assert!(!knockdown_capable(0x12, 50.0, true, KtState::FightKong));
    }

    #[test]
    fn counter_attack_after_three_light_or_two_heavy_hits() {
        let mut c = HitCounters::default();
        c.register(1);
        c.register(1);
        assert!(!c.take_counter());
        c.register(1);
        assert!(c.take_counter());
        assert_eq!(c.light, 0.0);
        let mut h = HitCounters::default();
        h.register(0x10);
        h.register(0x10);
        assert!(h.take_counter());
        // ageing clears the counters after 2.0 / 2.5 s
        let mut a = HitCounters::default();
        a.register(1);
        a.tick(2.1);
        assert_eq!(a.light, 0.0);
        a.register(0x10);
        a.tick(2.1);
        assert_eq!(a.heavy, 1.0);
        a.tick(0.5);
        assert_eq!(a.heavy, 0.0);
    }

    #[test]
    fn attack_bands() {
        assert_eq!(choose_attack(7.9, 1.0, true), Some(AttackKind::Sweep));
        assert_eq!(choose_attack(7.9, 0.5, true), Some(AttackKind::Tail));
        assert_eq!(choose_attack(8.5, 1.0, true), Some(AttackKind::Bite));
        assert_eq!(choose_attack(3.0, 1.0, true), Some(AttackKind::Sweep));
        assert_eq!(choose_attack(5.5, 1.0, true), Some(AttackKind::Sweep)); // melee wins below 8.0
        assert_eq!(choose_attack(9.0, 1.0, true), Some(AttackKind::Bite));
        assert_eq!(choose_attack(14.0, 1.0, true), Some(AttackKind::Bite));
        assert_eq!(choose_attack(14.1, 1.0, true), None);
        assert_eq!(choose_attack(6.0, 1.0, false), None);
    }

    #[test]
    fn charge_distances() {
        assert_eq!(charge_min_distance(true, false), 20.0);
        assert_eq!(charge_min_distance(false, false), 10.0);
        assert_eq!(charge_min_distance(true, true), 0.0);
    }

    #[test]
    fn ko_timing_and_revive_floor() {
        assert_eq!(ko_hold_seconds(5.0, false), 4.75);
        assert_eq!(ko_hold_seconds(5.0, true), 2.0);
        assert_eq!(ko_exit_life(0.0, false), 20.0);
        assert_eq!(ko_exit_life(35.0, false), 35.0);
        assert_eq!(ko_exit_life(0.0, true), 0.0);
    }

    #[test]
    fn life_zero_in_fight_state_knocks_down_for_5s() {
        let mut m = rex();
        m.state = KtState::FightKong;
        m.life.cur = 0.0;
        let s = m.step(&KtInput::default());
        assert_eq!(s, KtState::KoAuSol);
        assert_eq!(m.ko_time, 5.0);
        assert_eq!(m.ko_left, 4.75);
    }

    #[test]
    fn ko_to_finish_needs_finish_message_then_mort() {
        let mut m = rex();
        m.state = KtState::KoAuSol;
        m.ko_time = 5.0;
        m.ko_left = 4.75;
        let mut i = KtInput::default();
        i.grab_msg = Some(GrabMsg::Finish);
        assert_eq!(m.step(&i), KtState::Finish);
        let mut j = KtInput::default();
        j.finish_done = true;
        assert_eq!(m.step(&j), KtState::Mort);
        // terminal
        assert_eq!(m.step(&KtInput::default()), KtState::Mort);
    }

    #[test]
    fn finish_accepts_only_when_down_wounded_and_close() {
        assert!(finish_accepts(KtState::KoAuSol, 1.0, true, true, 21.9));
        assert!(!finish_accepts(KtState::KoAuSol, 1.0, true, true, 22.0));
        assert!(!finish_accepts(KtState::KoAuSol, 6.0, true, true, 5.0));
        assert!(!finish_accepts(KtState::FightKong, 1.0, true, true, 5.0));
        assert!(!finish_accepts(KtState::KoAuSol, 1.0, false, true, 5.0));
    }

    #[test]
    fn grab_end_states() {
        assert_eq!(grab_end_state(0x7a, 10.0), KtState::FightKong);
        assert_eq!(grab_end_state(0x7a, 0.0), KtState::KoAuSol);
        assert_eq!(grab_end_state(0x8d, 0.0), KtState::Mort);
        assert_eq!(grab_end_state(0xa1, 5.0), KtState::FightKong);
        assert_eq!(grab_end_state(0x33, 5.0), KtState::Projectile);
    }

    #[test]
    fn leaving_ko_revives_to_20() {
        let mut m = rex();
        m.state = KtState::KoAuSol;
        m.life.cur = 0.0;
        m.ko_left = 0.0;
        m.ko_time = 5.0;
        let mut i = KtInput::default();
        i.anim_done = true;
        let s = m.step(&i);
        assert_eq!(s, KtState::FightKong);
        assert_eq!(m.life.cur, 20.0);
        assert!(m.knocked_down_before);
    }

    #[test]
    fn charge_into_wall_costs_20_life() {
        let mut m = rex();
        m.state = KtState::Charge;
        let mut i = KtInput::default();
        i.charge_blocked = true;
        assert_eq!(m.step(&i), KtState::Paf);
        assert_eq!(m.life.cur, 80.0);
    }

    #[test]
    fn fight_kong_picks_attack_by_distance() {
        let mut m = rex();
        m.state = KtState::FightKong;
        let mut i = KtInput::default();
        i.target = Target::Kong;
        i.dist = 6.0;
        assert_eq!(m.step(&i), KtState::Attaque);
        assert_eq!(m.last_attack, Some(AttackKind::Sweep));
        let mut m2 = rex();
        m2.state = KtState::FightKong;
        i.dist = 12.0;
        assert_eq!(m2.step(&i), KtState::Attaque);
        assert_eq!(m2.last_attack, Some(AttackKind::Bite));
    }

    #[test]
    fn fury_shout_makes_the_rex_roar_and_stops_charging_no() {
        let mut m = rex();
        m.state = KtState::FightKong;
        let mut i = KtInput::default();
        i.fury_shout = true;
        assert_eq!(m.step(&i), KtState::Cri);
        // a charging rex is not interrupted by the reflex roar (only fight/idle/attack states)
        let mut c = rex();
        c.state = KtState::Charge;
        assert_eq!(c.step(&i), KtState::Charge);
    }

    #[test]
    fn paf_counter_attack_path() {
        let mut m = rex();
        m.state = KtState::Paf;
        let mut i = KtInput::default();
        i.counter_due = true; // mid-animation (frame > 19)
        assert_eq!(m.step(&i), KtState::Attaque);
        let mut m2 = rex();
        m2.state = KtState::Paf;
        i.counter_due = false;
        i.anim_done = true;
        assert_eq!(m2.step(&i), KtState::FightKong);
    }

    #[test]
    fn finish_mash_idle_kong_loses_progress_and_rex_escapes() {
        // frame 110 >= 102 at the end of the 2.74 s lock phase: no immediate escape, but with
        // no pulling the resistance drags the cursor back under 102 and the rex breaks free
        let mut f = FinishMash::new();
        let mut out = FinishOutcome::Running;
        let mut frames = 0;
        for _ in 0..600 {
            frames += 1;
            out = f.step(1.0 / 60.0, false, 0.0, false, 110.0, 200.0);
            if out != FinishOutcome::Running {
                break;
            }
        }
        assert_eq!(out, FinishOutcome::Escaped);
        assert!(frames > 165); // not during the 2.74 s (164.4 frames) lock phase
        // a cursor already below 102 when the lock phase ends escapes at once
        let mut g = FinishMash::new();
        let mut res = FinishOutcome::Running;
        for _ in 0..300 {
            res = g.step(1.0 / 60.0, false, 0.0, false, 80.0, 200.0);
            if res != FinishOutcome::Running {
                break;
            }
        }
        assert_eq!(res, FinishOutcome::Escaped);
    }

    #[test]
    fn finish_mash_pressing_every_frame_wins() {
        let mut f = FinishMash::new();
        let mut out = FinishOutcome::Running;
        for _ in 0..2000 {
            out = f.step(1.0 / 60.0, true, 0.0, false, 110.0, 200.0);
            if out != FinishOutcome::Running {
                break;
            }
        }
        assert_eq!(out, FinishOutcome::Won);
    }

    #[test]
    fn finish_mash_pull_values() {
        let mut f = FinishMash::new();
        f.elapsed = 3.0;
        f.progress = 150.0;
        f.step(1.0 / 60.0, true, 1.0, false, 0.0, 400.0);
        assert_eq!(f.pull, 3.0); // (0.5 - 1.0*0.5) + 3.0
        f.step(1.0 / 60.0, true, 0.0, false, 0.0, 400.0);
        assert_eq!(f.pull, 3.5);
        // release: pull decays 20/s (3/s with fury)
        let mut a = FinishMash::new();
        a.elapsed = 3.0;
        a.progress = 150.0;
        a.pull = 3.5;
        a.step(0.016, false, 0.0, false, 0.0, 400.0);
        assert!((a.pull - (3.5 - 20.0 * 0.016)).abs() < 1e-5);
        let mut b = a;
        b.pull = 3.5;
        b.step(0.016, false, 0.0, true, 0.0, 400.0);
        assert!((b.pull - (3.5 - 3.0 * 0.016)).abs() < 1e-5);
    }

    #[test]
    fn finish_mash_completes() {
        let mut f = FinishMash::new();
        f.elapsed = 3.0;
        f.progress = 399.0;
        let o = f.step(1.0 / 60.0, true, 0.0, false, 0.0, 400.0);
        assert_eq!(o, FinishOutcome::Won);
    }
}
