//! Kong's fury ("rage"): ledger KF01-KF07. Evidence: `spec/evidence/KF01.md`.
//!
//! Everything here is read from Kong's AI functions in `KingKong8.exe`. Kong's struct offsets
//! are written `Kong+0xNNN`; the fury timer is `Kong+0x7c4` (a float, seconds): "fury is on"
//! is `!IsZero(Kong+0x7c4)` (`fn@0x006038a0`, query id 0x322 of `fn@0x006021d0`), where
//! `IsZero(x) = |x| < 1e-4` (`fn@0x0041d450`, epsilon 0x38d1b717).

/// `IsZero` epsilon, `fn@0x0041d450@0x41d450` (DAT_00af5500 = 0x38d1b717) [C].
pub const IS_ZERO_EPS: f32 = 1.0e-4;
/// Timer value that means "fury forever" (0x56b5e621 as a float); `k_exec_fury` never decays it
/// down to zero in practice and `k_exec_fury` compares against it [C k_exec_fury@0x8b1d00].
pub const FURY_INFINITE: f32 = 1.000_000_0e14;

/// Fury duration set when fury starts: 15.0 (`fn@0x00883430` stores 0x41700000 in both branches)
/// + 3.0 (`_DAT_00a88e74`) = 18.0 s [C fn@0x00883430@0x883430].
pub const FURY_START_SECONDS: f32 = 15.0 + 3.0;
/// Each landed hit while fury is on adds 1.5 s (`_DAT_00a88e70`) [C fn@0x00883700@0x883700] ...
pub const FURY_EXTEND_PER_HIT: f32 = 1.5;
/// ... and the timer is then clamped to 15.0 (`_DAT_00a88ee8`) [C fn@0x00883700].
pub const FURY_EXTEND_CAP: f32 = 15.0;
/// Charge meter (`Kong+0x7d4`) set to 1.0 when fury starts [C fn@0x00883430].
pub const METER_AT_START: f32 = 1.0;
/// Charge meter gain per qualifying hit while fury is off (`_DAT_00a88c48`) [C fn@0x00883700].
pub const METER_PER_HIT: f32 = 0.1;
/// If the meter exceeds 0.8 (`_DAT_00a88f08`) outside combat phases 4/7 it is reset to 0.7
/// (0x3f333333) [C fn@0x00883700]. No code reads the meter as a gate: it is HUD/FX state [L].
pub const METER_SOFT_CAP_TRIGGER: f32 = 0.8;
pub const METER_SOFT_CAP_VALUE: f32 = 0.7;
/// Kong's base melee damage is 10; with fury it is 10 + 10 = 20 (`fn@0x0088e0c0`:
/// `fild 10; fadd _DAT_00a88e24(10.0); _ftol`) [C]. Hit class also becomes 2 (heavy) instead of 1.
pub const BASE_HIT_DAMAGE: i32 = 10;
pub const FURY_DAMAGE_BONUS: f32 = 10.0;
/// Animation playback speed is multiplied by 1.25 (`_DAT_00a89194`) at the end of `k_ETAT_main`
/// while fury is on (`fn@0x004258b0(self,0,ftol(speed*1.25))`) [C k_ETAT_main@0x89a120 +0x8a4064].
pub const FURY_ANIM_SPEED_MUL: f32 = 1.25;
/// Mash-challenge resistance (`k_ETAT_grab_mashing`): the decay rate of Kong's input energy is
/// `Kong+0x77c`; with fury it is multiplied by 0.5 (`_DAT_00a878c8`) [C k_ETAT_grab_mashing@0x8cca60].
/// (Larger decay = harder, so 0.5 means the energy bar drains slower.)
pub const FURY_MASH_DECAY_MUL: f32 = 0.5;
/// Jaw-break pull force decay per second: 20.0 normally, 3.0 with fury (`KT_ETAT_Finish`,
/// `k_ETAT_grab` use the same pair) [C].
pub const PULL_DECAY_NORMAL: f32 = 20.0;
pub const PULL_DECAY_FURY: f32 = 3.0;
/// Being hit while fury > 5.0 s (`_DAT_00a88e20`) shortens it: 2.5 s if the hit flags have
/// neither bit 1 nor bit 2, else 5.0 s [C k_ETAT_paf@0x8c24a0].
pub const HIT_PENALTY_MIN_TIMER: f32 = 5.0;
pub const HIT_PENALTY_SOFT: f32 = 2.5;
pub const HIT_PENALTY_HARD: f32 = 5.0;
/// If Kong is wounded when hit during fury, the timer is clamped to [5.0, 15.0] [C k_ETAT_paf].
pub const WOUNDED_CLAMP: (f32, f32) = (5.0, 15.0);
/// With fury on and Kong's life at 0, life is reset to 10.0 instead of dying
/// (`k_reflex@0x8a4360`: `fn@0x00770aa0(Kong+0x1aa0, 10.0)`) [C].
pub const FURY_REVIVE_LIFE: f32 = 10.0;
/// Without fury, life 0 starts a 10.0 s "last stand" (`Kong+0x1fcc`); a hit while < 5.0 s remain
/// kills (state code 0x1f), a hit while >= 5.0 s remain cuts it to 5.0 [C k_reflex, k_exec_detect_paf].
pub const LAST_STAND_SECONDS: f32 = 10.0;
pub const LAST_STAND_LETHAL_BELOW: f32 = 5.0;

// ---- fury FX (KF06) ------------------------------------------------------------------------
/// Music/ambience param 0xca target: `min(timer / 4.0, 0.8)` (`_DAT_00a88ecc`=4, `_DAT_00a88f08`=0.8);
/// no fury -> 0 at rate 0.2/s; fury -> rate 1.0/s; floor 0.5 at rate 10 once below 0.5 [C k_exec_fury].
pub const FX_AUDIO_DIVISOR: f32 = 4.0;
pub const FX_AUDIO_MAX: f32 = 0.8;
/// Post-process blend coefficient: `timer <= 5 ? timer/5 : 1` smoothed at rate 3/s (<=5 s) or 1/s,
/// then each afx channel moves toward `configured_value * coef` at 5/s [C k_exec_fury].
pub const FX_COEF_RAMP_SECONDS: f32 = 5.0;

/// `IsZero` (`fn@0x0041d450`).
pub fn is_zero(x: f32) -> bool {
    x.abs() < IS_ZERO_EPS
}

/// Kong actions during which the fury timer does **not** run down (`k_exec_fury`): the grab-mash
/// states `0x160`, `0x352`, `0x353` and `0x15e` unless `fn@0x00606b30(kong,0)` says otherwise.
pub fn timer_paused_in_action(action_id: i32, fn_606b30_nonzero: bool) -> bool {
    match action_id {
        0x160 | 0x352 | 0x353 => true,
        0x15e => !fn_606b30_nonzero, // local_20 = 0 (paused) unless fn@0x00606b30 != 0 (then decays)
        _ => false,
    }
}

/// Kong's fury state (`Kong+0x7c4` timer, `Kong+0x7d4` meter, `Kong+0x7d8` flag).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fury {
    pub timer: f32,
    pub meter: f32,
    /// `Kong+0x7e0`: set once per attack action by `fn@0x00883700` so one swing counts once.
    pub counted_this_action: bool,
}

impl Default for Fury {
    fn default() -> Self {
        Fury { timer: 0.0, meter: 0.0, counted_this_action: false }
    }
}

impl Fury {
    pub fn is_active(&self) -> bool {
        !is_zero(self.timer)
    }

    /// `fn@0x00883430`: fury starts (called when the chest-pound window resolves with no other
    /// button, `k_ETAT_main` +0x89bb6b with anim 0xad).
    pub fn start(&mut self) {
        self.meter = METER_AT_START;
        self.timer = FURY_START_SECONDS;
    }

    /// New attack action begins (`fn@0x00884b00`/`d40`/`de0`/`e70`: `Kong+0x7e0 = 0`).
    pub fn begin_action(&mut self) {
        self.counted_this_action = false;
    }

    /// `fn@0x00883700`, called from the hit-landed function `fn@0x00883050` when the victim has a
    /// positive life value. `target_wounded`: victim's life gauge is at/below its wound ratio;
    /// `heavy_extra`: Kong+0x1fc4 flag (doubles the gain); `msg_bits`: type of Kong's current
    /// hit message (`& 5 != 0` adds one more step); `in_phase_4_or_7`: `Kong+0xcfc` is 4 or 7.
    pub fn on_hit_landed(&mut self, heavy_extra: bool, target_wounded: bool, msg_bits: u32, in_phase_4_or_7: bool) {
        if self.counted_this_action {
            return;
        }
        self.counted_this_action = true;
        if !self.is_active() {
            self.meter += METER_PER_HIT;
            if heavy_extra {
                self.meter += METER_PER_HIT;
            }
            if target_wounded {
                self.meter += METER_PER_HIT;
            }
            if msg_bits & 5 != 0 {
                self.meter += METER_PER_HIT;
            }
            if !in_phase_4_or_7 && self.meter > METER_SOFT_CAP_TRIGGER {
                self.meter = METER_SOFT_CAP_VALUE;
            }
        } else {
            self.timer += FURY_EXTEND_PER_HIT;
            if self.timer > FURY_EXTEND_CAP {
                self.timer = FURY_EXTEND_CAP;
            }
        }
    }

    /// `k_ETAT_paf` (Kong takes a hit). `hit_flags`: Kong+0xc74; `kong_wounded`: Kong's own life
    /// gauge (+0x1aa0) is at/below its wound ratio.
    pub fn on_kong_hit(&mut self, hit_flags: u32, kong_wounded: bool) {
        if self.timer > HIT_PENALTY_MIN_TIMER {
            let pen = if hit_flags & 3 == 0 { HIT_PENALTY_SOFT } else { HIT_PENALTY_HARD };
            self.timer -= pen;
            if kong_wounded {
                self.timer = if self.timer <= WOUNDED_CLAMP.1 {
                    if self.timer < WOUNDED_CLAMP.0 { WOUNDED_CLAMP.0 } else { self.timer }
                } else {
                    WOUNDED_CLAMP.1
                };
            }
        }
    }

    /// `k_exec_fury` countdown: `timer -= min(timer, dt)` unless the timer is the "infinite"
    /// sentinel or Kong is in a paused action; at zero the meter and flag are cleared too.
    pub fn tick(&mut self, dt: f32, paused: bool) {
        if self.timer != 0.0 && (self.timer - FURY_INFINITE).abs() > 1.0 && self.is_active() && !paused {
            self.timer -= self.timer.min(dt);
        }
        if self.timer == 0.0 {
            self.meter = 0.0;
        }
    }

    /// `k_reflex` / `k_ETAT_finished`: dying or entering a cinematic clears fury.
    pub fn clear(&mut self) {
        *self = Fury::default();
    }

    /// Damage of Kong's basic melee blow (`fn@0x0088e0c0`): 10, or 20 with fury.
    pub fn melee_damage(&self) -> i32 {
        if self.is_active() { BASE_HIT_DAMAGE + FURY_DAMAGE_BONUS as i32 } else { BASE_HIT_DAMAGE }
    }

    /// Hit class sent with the blow: 2 (heavy, can knock down) with fury, else 1 (light).
    pub fn melee_hit_class(&self) -> u32 {
        if self.is_active() { 2 } else { 1 }
    }

    pub fn anim_speed_mul(&self) -> f32 {
        if self.is_active() { FURY_ANIM_SPEED_MUL } else { 1.0 }
    }

    pub fn pull_decay_rate(&self) -> f32 {
        if self.is_active() { PULL_DECAY_FURY } else { PULL_DECAY_NORMAL }
    }

    /// Visual blend coefficient target (`k_exec_fury`): `timer<=5 ? timer/5 : 1`.
    pub fn fx_coef_target(&self) -> f32 {
        if !self.is_active() {
            0.0
        } else if self.timer <= FX_COEF_RAMP_SECONDS {
            (self.timer / FX_COEF_RAMP_SECONDS).max(0.0)
        } else {
            1.0
        }
    }

    /// Audio parameter target: `min(timer/4, 0.8)` (0 without fury).
    pub fn fx_audio_target(&self) -> f32 {
        if !self.is_active() { 0.0 } else { (self.timer / FX_AUDIO_DIVISOR).min(FX_AUDIO_MAX) }
    }
}

/// What Kong's life reaching 0 does (`k_reflex@0x8a4360`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LifeZero {
    /// fury on: life := 10.0, last-stand reset
    Revive(f32),
    /// first frame at 0 without fury: last-stand timer := 10.0
    StartLastStand(f32),
    /// last-stand running: timer -= dt
    LastStandTick(f32),
    /// timer ran out: life := wound_ratio * max (see `k_reflex`)
    Restore,
}

/// `last_stand_timer` is `Kong+0x1fcc`, `last_stand_flag` is `Kong+0x1fc8`.
pub fn life_zero_response(fury_active: bool, last_stand_timer: f32, last_stand_flag: bool, dt: f32) -> LifeZero {
    if !is_zero(last_stand_timer) && !fury_active {
        LifeZero::LastStandTick((last_stand_timer - last_stand_timer.min(dt)).max(0.0))
    } else if !last_stand_flag && !fury_active {
        LifeZero::StartLastStand(LAST_STAND_SECONDS)
    } else if fury_active {
        LifeZero::Revive(FURY_REVIVE_LIFE)
    } else {
        LifeZero::Restore
    }
}

/// A hit while the last-stand flag is set (`k_exec_detect_paf` +0x8c4082): returns `true` if it kills.
pub fn last_stand_hit_kills(last_stand_timer: f32) -> bool {
    last_stand_timer < LAST_STAND_LETHAL_BELOW
}

// ---- activation: the chest-pound window (KF01/KF02) --------------------------------------------
/// Initial time budget of combat phase 4 (`fn@0x00884a10`: `Kong+0x7c0 = 0.5`, `Kong+0x7bc = 0`) [C].
pub const POUND_BUDGET_START: f32 = 0.5;
/// Budget added per call while the fury button (`Kong+0x78`, button slot 2) is held
/// (`_DAT_00a88e1c`) [C k_ETAT_main case 4].
pub const POUND_BUDGET_PER_PRESS_FRAME: f32 = 0.2;
/// Fury resolves once the pound animation 0xab is past this frame (0x82) with the button held [C].
pub const POUND_RESOLVE_FRAME: i32 = 0x82;
/// Anim ids: pound (phase 4) 0xab, fury roar 0xad [C fn@0x00884f00, k_ETAT_main +0x89bb66].
pub const ANIM_POUND: u32 = 0xab;
pub const ANIM_FURY_ROAR: u32 = 0xad;
/// Shout stimulus radius grows 10/s (`_DAT_00a88e24 * dt`) up to 30.0 (`_DAT_00a88e14`) while pounding [C].
pub const SHOUT_RADIUS_RATE: f32 = 10.0;
pub const SHOUT_RADIUS_MAX: f32 = 30.0;
/// Creature-side reaction radius to the fury shout (stimulus kind 2): KT 30.0, KR 35.0, KBC 50.0
/// (`KT_exec_check_fury`, `KR_exec_check_fury`, `KBC_exec_check_fury`) [C].
pub const REACT_RADIUS_KT: f32 = 30.0;
pub const REACT_RADIUS_KR: f32 = 35.0;
pub const REACT_RADIUS_KBC: f32 = 50.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PoundResult {
    Continue,
    /// budget used up or animation ended: back to idle, no fury
    Exit,
    /// another button resolved the window (grab=3, cancel=4, other=1)
    OtherButton(u8),
    /// resolved with the fury button and no other press: fury starts (`fn@0x00883430(0xad)`)
    FireFury,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoundWindow {
    pub budget: f32,
    pub elapsed: f32,
    pub shout_radius: f32,
}

impl Default for PoundWindow {
    fn default() -> Self {
        PoundWindow { budget: POUND_BUDGET_START, elapsed: 0.0, shout_radius: 0.0 }
    }
}

impl PoundWindow {
    /// One call of `k_ETAT_main` case 4. `held`: `Kong+0x78 != 0`; `frame`: anim frame of the pound
    /// anim; `window_flag`: `fn@0x00884780` (anim flag 0x100) - together with a latched button-3
    /// press it also resolves; `latched`: latched presses of buttons 1,3,4 (`fn@0x00884600`).
    pub fn step(&mut self, dt: f32, held: bool, frame: i32, window_flag: bool, anim_ended: bool, latched: [bool; 3]) -> PoundResult {
        let (b1, b3, b4) = (latched[0], latched[1], latched[2]);
        if held {
            self.budget += POUND_BUDGET_PER_PRESS_FRAME;
        }
        let resolve = (held && frame > POUND_RESOLVE_FRAME) || (window_flag && b3);
        if resolve {
            return if b3 {
                PoundResult::OtherButton(3)
            } else if b4 {
                PoundResult::OtherButton(4)
            } else if b1 {
                PoundResult::OtherButton(1)
            } else {
                PoundResult::FireFury
            };
        }
        if self.budget < self.elapsed || anim_ended {
            return PoundResult::Exit;
        }
        self.elapsed += dt;
        self.shout_radius = (self.shout_radius + SHOUT_RADIUS_RATE * dt).min(SHOUT_RADIUS_MAX);
        PoundResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_sets_18s_and_full_meter() {
        let mut f = Fury::default();
        assert!(!f.is_active());
        f.start();
        assert_eq!(f.timer, 18.0);
        assert_eq!(f.meter, 1.0);
        assert!(f.is_active());
    }

    #[test]
    fn hits_extend_1_5s_capped_15() {
        let mut f = Fury::default();
        f.start();
        f.on_hit_landed(false, false, 0, false);
        assert_eq!(f.timer, 15.0); // 18 + 1.5 clamped to 15
        f.tick(1.0, false);
        assert_eq!(f.timer, 14.0);
        f.begin_action();
        f.on_hit_landed(false, false, 0, false);
        assert_eq!(f.timer, 15.0);
        // one count per action
        f.on_hit_landed(false, false, 0, false);
        assert_eq!(f.timer, 15.0);
    }

    #[test]
    fn meter_gain_and_soft_cap() {
        let mut f = Fury::default();
        for _ in 0..4 {
            f.begin_action();
            f.on_hit_landed(false, true, 5, false); // +0.1 +0.1 +0.1 = 0.3 per action
        }
        // 0.3, 0.6, 0.9 -> soft cap 0.7, 1.0 -> 0.7
        assert!((f.meter - 0.7).abs() < 1e-6);
        let mut g = Fury::default();
        g.begin_action();
        g.on_hit_landed(true, true, 1, true); // 0.4, no cap in phase 4/7
        assert!((g.meter - 0.4).abs() < 1e-6);
    }

    #[test]
    fn damage_doubles_and_becomes_heavy() {
        let mut f = Fury::default();
        assert_eq!(f.melee_damage(), 10);
        assert_eq!(f.melee_hit_class(), 1);
        f.start();
        assert_eq!(f.melee_damage(), 20);
        assert_eq!(f.melee_hit_class(), 2);
        assert_eq!(f.anim_speed_mul(), 1.25);
    }

    #[test]
    fn kong_hit_shortens_fury() {
        let mut f = Fury { timer: 12.0, ..Default::default() };
        f.on_kong_hit(0x400, false); // no bit 1/2 -> 2.5 s
        assert_eq!(f.timer, 9.5);
        f.on_kong_hit(0x2, false); // bit 2 -> 5.0 s
        assert_eq!(f.timer, 4.5);
        f.on_kong_hit(0x2, false); // timer <= 5.0: no penalty
        assert_eq!(f.timer, 4.5);
        let mut w = Fury { timer: 6.0, ..Default::default() };
        w.on_kong_hit(0x2, true); // 1.0 -> wounded clamp to 5.0
        assert_eq!(w.timer, 5.0);
    }

    #[test]
    fn timer_runs_down_except_in_mash_actions() {
        let mut f = Fury { timer: 3.0, ..Default::default() };
        f.tick(0.5, true);
        assert_eq!(f.timer, 3.0);
        assert!(timer_paused_in_action(0x160, false));
        assert!(!timer_paused_in_action(0x200, false));
        f.tick(0.5, false);
        assert_eq!(f.timer, 2.5);
        f.tick(10.0, false);
        assert_eq!(f.timer, 0.0);
        assert_eq!(f.meter, 0.0);
        assert!(!f.is_active());
    }

    #[test]
    fn fx_targets() {
        let f = Fury { timer: 2.5, ..Default::default() };
        assert_eq!(f.fx_coef_target(), 0.5);
        assert_eq!(f.fx_audio_target(), 0.625);
        let g = Fury { timer: 15.0, ..Default::default() };
        assert_eq!(g.fx_coef_target(), 1.0);
        assert_eq!(g.fx_audio_target(), 0.8);
    }

    #[test]
    fn life_zero_rules() {
        assert_eq!(life_zero_response(true, 0.0, false, 0.016), LifeZero::Revive(10.0));
        assert_eq!(life_zero_response(false, 0.0, false, 0.016), LifeZero::StartLastStand(10.0));
        assert!(matches!(life_zero_response(false, 10.0, true, 0.5), LifeZero::LastStandTick(t) if (t - 9.5).abs() < 1e-6));
        assert!(last_stand_hit_kills(4.9));
        assert!(!last_stand_hit_kills(7.0));
    }

    #[test]
    fn pound_window_fires_fury_when_held_past_frame_130() {
        let mut w = PoundWindow::default();
        // hold the button for the whole animation
        let mut res = PoundResult::Continue;
        for frame in 0..200 {
            res = w.step(1.0 / 60.0, true, frame, false, false, [false, false, false]);
            if res != PoundResult::Continue {
                assert_eq!(frame, 131);
                break;
            }
        }
        assert_eq!(res, PoundResult::FireFury);
    }

    #[test]
    fn pound_window_times_out_without_input() {
        let mut w = PoundWindow::default();
        let mut n = 0;
        loop {
            n += 1;
            match w.step(0.125, false, 10, false, false, [false; 3]) {
                PoundResult::Continue => {}
                PoundResult::Exit => break,
                other => panic!("{other:?}"),
            }
            assert!(n < 100);
        }
        // 0.5 s budget at 0.125 s/step: elapsed 0.5 after 4 calls, 0.625 > 0.5 on the 6th call
        assert_eq!(n, 6);
    }

    #[test]
    fn pound_window_other_button_blocks_fury() {
        let mut w = PoundWindow::default();
        assert_eq!(w.step(0.016, true, 140, false, false, [false, true, false]), PoundResult::OtherButton(3));
        let mut w = PoundWindow::default();
        assert_eq!(w.step(0.016, true, 140, false, false, [true, false, false]), PoundResult::OtherButton(1));
    }

    #[test]
    fn mash_and_pull_constants() {
        let f = Fury { timer: 5.0, ..Default::default() };
        assert_eq!(f.pull_decay_rate(), 3.0);
        assert_eq!(Fury::default().pull_decay_rate(), 20.0);
        assert_eq!(FURY_MASH_DECAY_MUL, 0.5);
    }
}
