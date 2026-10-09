//! `KongBrain`: a deterministic AI that plays Kong against the KT rex through the same pad
//! abstraction a player uses ([`KongInput`]), so every move goes through the recovered rules in
//! [`super::fight`] (phase graph with its latches and windows, hit classes, fury by the pound press
//! pattern, side-step invulnerability, grab/throw, KO and the jaw-break mash).
//!
//! The brain itself is **ours (`[G]`)**: the real game has no Kong combat AI (Kong is always
//! player controlled, only the KT rex has an AI). It follows a playbook meant to showcase all
//! of Kong's moves in a natural order, and reacts to the rex's state:
//!
//! 1. approach (walk, so the rex's opening charge can start), dodge the charge,
//! 2. punch chain (2 punches), repel, downward strike (rex 80 -> 50 = wounded, B02 instance data),
//! 3. shoulder strike (= counter lunge, after a dodge of an incoming rex attack) (-> 40),
//! 4. grab (offered while the rex recovers from an attack / skids / roars), grab strike, throw (-> 10),
//! 5. fury while the thrown rex skids (special, special, hold special: the chest-pound press pattern),
//! 6. pressure with heavier, faster blows until the rex is knocked down (KO),
//! 7. jaw-break finisher (only when the game offers it: KO + wounded + first KO + within 22), mash,
//! 8. victory pound and roar (played by [`super::fight::Fight`]).
//!
//! Reactions (any stage): a rex attack wind-up or charge heading at Kong is answered with the
//! side-step (stick toward the rex keeps the dodge gate `dot >= -0.342` open) after a seeded human
//! reaction delay of 0.10-0.28 s; 8 % of threats are misjudged. When the rex hesitates (cool-down)
//! Kong closes in.

use super::combat::Phase;
use super::fight::*;
use super::vrex::KtState;

/// Playbook stages, in the order the brain tries to show them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    Approach,
    PunchChain,
    Repel,
    Downward,
    Shoulder,
    GrabThrow,
    Fury,
    Pressure,
    Finisher,
    Done,
}

/// A note the brain leaves for debugging / the presentation (time, text).
#[derive(Clone, Debug, PartialEq)]
pub struct BrainNote {
    pub time: f32,
    pub stage: Stage,
    pub text: &'static str,
}

/// Rex life budget [G] (rex 80/80/50, spec/evidence/B02.md): the brain keeps the rex above this life until the fury
/// has been shown, so the knock-down is made by a fury blow (class 2, 20, the only plain blow besides the
/// downward strike and the shoulder that still hurts a wounded rex) and not by an earlier move. Every
/// non-fury blow costs at most 10 (grab strike, downward, shoulder; the throw impact 20 is taken first).
const RESERVE_LIFE: f32 = 10.0;
/// One impact (20, B02) plus one blow above the knock-down: below this the brain shows the fury next [G].
const LOW_LIFE: f32 = 30.0;
/// Grab strike damage (KC12 / combat::GRAB_STRIKE: class 9, 10) [C].
const GRAB_STRIKE_DAMAGE: f32 = 10.0;
/// Standing distance (centre to centre) the brain fights at [G]; a hesitating rex lets it step closer.
const ENGAGE_DIST: f32 = 8.2;
const ENGAGE_DIST_CLOSE: f32 = 6.8;
/// Time limits after which a stage is skipped [G].
/// Pressure pacing [G]: blows per burst and the rest after it. Retuned (was 3 blows, 1.6-2.8 s) when the recovered animation
/// lengths (KC01: punch 51 f, recover 22 f, ...) made every blow about twice as slow as the old placeholder table.
const BURST_LEN: u32 = 4;
/// A blow takes about 1.2 s (punch 0.85 s + recovery 0.37 s) and cannot be cancelled before its end, so the brain does not start one
/// when the rex's attack cool-down has less than this left [G].
const ATTACK_SOON_CD: f32 = 1.9;
/// Longest the brain waits for a safe moment to start the 2.4 s chest pound (seconds of ticks) [G].
const FURY_WAIT_MAX: f32 = 8.0;
/// Distance at which the brain feels safe to start the chest pound [G].
const FURY_SAFE_DIST: f32 = 17.0;
const REST_BASE: f32 = 0.5;
const REST_JITTER: f32 = 0.8;
const STAGE_TIMEOUT: [(Stage, f32); 4] =
    [(Stage::Shoulder, 14.0), (Stage::GrabThrow, 25.0), (Stage::Fury, 14.0), (Stage::Pressure, 40.0)];

#[derive(Clone, Debug)]
pub struct KongBrain {
    rng: Rng,
    pub stage: Stage,
    stage_t: f32,
    pub notes: Vec<BrainNote>,
    last_press: [f32; 4],
    last_chain_key: [i64; 4],
    // threat reaction
    threat_prev: bool,
    react_left: Option<f32>,
    ignore_threat: bool,
    threat_handled: bool,
    // chain bookkeeping
    punch_seen: bool,
    grab_step: u8,
    fury_done: bool,
    grab_done: bool,
    /// the second grab of the showcase ends in the slam (0xa5) instead of the throw [G]
    slam_done: bool,
    downward_tries: u32,
    shoulder_done: bool,
    fury_tries: u32,
    pressure_toggle: bool,
    mash_timer: f32,
    mash_toggle: bool,
    /// time the brain has been waiting for a safe moment to start the fury pattern
    fury_wait: f32,
    /// pressure pacing: blows thrown in the current burst, and the rest period after it
    burst_start: u32,
    rest_left: f32,
    lead_time: f32,
    pub now: f32,
}

impl KongBrain {
    pub fn new(seed: u32) -> Self {
        KongBrain {
            rng: Rng::new(seed ^ 0x9e37_79b9),
            stage: Stage::Approach,
            stage_t: 0.0,
            notes: Vec::new(),
            last_press: [-10.0; 4],
            last_chain_key: [-1; 4],
            threat_prev: false,
            react_left: None,
            ignore_threat: false,
            threat_handled: false,
            punch_seen: false,
            grab_step: 0,
            fury_done: false,
            grab_done: false,
            slam_done: false,
            downward_tries: 0,
            shoulder_done: false,
            fury_tries: 0,
            pressure_toggle: false,
            mash_timer: 0.0,
            mash_toggle: false,
            fury_wait: 0.0,
            burst_start: 0,
            rest_left: 0.0,
            lead_time: 0.0,
            now: 0.0,
        }
    }

    fn note(&mut self, text: &'static str) {
        self.notes.push(BrainNote { time: self.now, stage: self.stage, text });
    }

    fn goto(&mut self, s: Stage, why: &'static str) {
        if self.stage != s {
            self.stage = s;
            self.stage_t = 0.0;
            self.note(why);
        }
    }

    /// Press `slot` unless it was pressed in the same phase instance / less than 0.3 s ago.
    fn press(&mut self, inp: &mut KongInput, f: &Fight, slot: usize) -> bool {
        // inside a phase a slot is pressed once per phase instance; in idle only the 0.3 s gate applies
        let in_phase = !matches!(f.kong.phase, Phase::None | Phase::Recover);
        let key = if in_phase { f.kong.attacks_started as i64 } else { -1 };
        if self.now - self.last_press[slot] < 0.3 || (in_phase && self.last_chain_key[slot] == key) {
            return false;
        }
        self.last_press[slot] = self.now;
        self.last_chain_key[slot] = key;
        inp.press(slot);
        true
    }

    fn stick_toward(f: &Fight, mag: f32) -> V2 {
        let d = f.to_rex();
        (d.0 * mag, d.1 * mag)
    }

    /// May the brain hurt the rex now? Keeps the knock-down for after the grab and fury moments.
    /// Before fury: a second grab that ends in the slam, if the rex can take the slam's damage and stay
    /// above the reserve (so the knock-down is still made by a fury blow) [G].
    fn wants_slam(&self, f: &Fight) -> bool {
        self.grab_done
            && !self.slam_done
            && !f.kong.fury.is_active()
            && matches!(self.stage, Stage::Fury | Stage::Shoulder)
            && f.rex.life() - SLAM_DAMAGE >= RESERVE_LIFE
    }

    fn may_damage(&self, f: &Fight) -> bool {
        // the grab + throw still to show: keep enough life for the strike (10) and the impact (20) [G]
        if !self.grab_done && f.rex.life() <= super::vrex::IMPACT_DAMAGE + GRAB_STRIKE_DAMAGE + 10.0 && self.stage >= Stage::GrabThrow {
            return false;
        }
        if self.grab_done && self.fury_done {
            return true;
        }
        // plain class-1 blows do nothing to a wounded rex; the rest costs at most 10
        f.rex.life() > RESERVE_LIFE
    }

    /// Is a rex attack about to start or land within the time a blow now commits Kong to (blows cannot be cancelled before their
    /// end, KC01: punch 0.85 s + recovery)? [G]
    fn attack_soon(f: &Fight) -> bool {
        if f.rex.attack_eta().is_some_and(|eta| eta < 1.1) {
            return true;
        }
        let r = &f.rex;
        r.attack_cd < ATTACK_SOON_CD && r.recover <= 0.0 && matches!(r.state(), KtState::FightKong | KtState::Attente | KtState::Paf)
    }

    /// Incoming rex threat: seconds until it connects.
    fn threat(f: &Fight) -> Option<f32> {
        let r = &f.rex;
        let d = f.dist();
        match r.state() {
            KtState::Attaque => {
                let eta = r.attack_eta()?;
                let reach = if r.attack.map(|a| a.kind) == Some(RexMove::Bite) { 15.0 } else { 10.5 };
                if d <= reach {
                    Some(eta)
                } else {
                    None
                }
            }
            // the side-step gate is 14 m [C]: do not dodge a charge before it is inside it
            KtState::Charge if !r.charge_hit_done && d <= 13.0 => {
                let to_kong = f.kong.pos;
                let toward = (to_kong.0 - r.pos.0) * r.charge_dir.0 + (to_kong.1 - r.pos.1) * r.charge_dir.1;
                if toward > 0.0 {
                    Some(((d - 5.5) / r.charge_speed.max(1.0)).max(0.0))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Produce Kong's pad for this step (`dt` seconds since the last call).
    pub fn think(&mut self, f: &Fight, dt: f32) -> KongInput {
        self.now = f.time;
        self.stage_t += dt;
        let mut inp = KongInput::default();
        // stage time-outs keep the playbook moving
        for (s, lim) in STAGE_TIMEOUT {
            if self.stage == s && self.stage_t > lim {
                let next = match s {
                    Stage::Shoulder => Stage::GrabThrow,
                    Stage::GrabThrow => Stage::Fury,
                    Stage::Fury => Stage::Pressure,
                    _ => Stage::Pressure,
                };
                match s {
                    Stage::Shoulder => self.shoulder_done = true,
                    Stage::GrabThrow => self.grab_done = true,
                    Stage::Fury => self.fury_done = true,
                    _ => {}
                }
                self.goto(next, "stage timed out");
            }
        }
        match f.kong.mode {
            KongMode::Dead | KongMode::Victory => return inp,
            KongMode::Paf => return inp,
            KongMode::Grab => {
                self.grab_mode(f, &mut inp);
                return inp;
            }
            KongMode::Finisher => {
                self.mash(f, dt, &mut inp);
                return inp;
            }
            KongMode::Main => {}
        }
        // ---- opportunism: the rex is down --------------------------------------------------
        let st = f.rex.state();
        // the jaw-break is only offered on the rex's FIRST knock-down while wounded (B02); a player never lets
        // that pass, so take it whatever the playbook stage (a wall-charge KO can come before fury)
        let first_ko_finish = st == KtState::KoAuSol && f.rex.wounded() && !f.rex.machine.knocked_down_before;
        if st == KtState::KoAuSol && self.stage < Stage::Finisher && ((self.grab_done && self.fury_done) || first_ko_finish) {
            self.goto(Stage::Finisher, "rex knocked down");
        }
        if f.kong.fury.is_active() {
            self.fury_done = true;
        }
        if self.stage == Stage::Fury && f.kong.fury.is_active() {
            self.goto(Stage::Pressure, "fury on");
        }
        // life-aware [G]: a wall charge or the throw impact costs 20 (B02), so once the rex is one impact away
        // from the knock-down, show the fury before anything else can end the fight
        // (the shoulder strike is still shown down to 20: one more 10 leaves a wall impact short of the KO) [G]
        let low = if self.stage <= Stage::Shoulder && !self.shoulder_done { LOW_LIFE - 10.0 } else { LOW_LIFE };
        if self.stage < Stage::Fury && !self.fury_done && f.rex.life() <= low && f.rex.state() != KtState::KoAuSol {
            self.fury_wait = 0.0;
            self.goto(Stage::Fury, "rex low, fury first");
        }
        // ---- reactions ---------------------------------------------------------------------
        if self.react(f, dt, &mut inp) {
            return inp;
        }
        // ---- follow-up while dodging -------------------------------------------------------
        if matches!(f.kong.phase, Phase::DodgeSide | Phase::DodgeRoll) {
            self.dodge_followup(f, &mut inp);
            return inp;
        }
        self.playbook(f, &mut inp);
        inp
    }

    // -------------------------------------------------------------------------------------
    fn react(&mut self, f: &Fight, dt: f32, inp: &mut KongInput) -> bool {
        let th = Self::threat(f);
        let have = th.is_some();
        if have && !self.threat_prev {
            // a new threat: human reaction delay, small chance to misjudge it
            let charge = f.rex.state() == KtState::Charge;
            self.react_left = Some(if charge { self.rng.range(0.05, 0.18) } else { self.rng.range(0.10, 0.28) });
            self.ignore_threat = self.rng.unit() < 0.08;
            self.threat_handled = false;
        }
        if !have {
            self.react_left = None;
        }
        self.threat_prev = have;
        if let (Some(eta), Some(left)) = (th, self.react_left) {
            if self.threat_handled || self.ignore_threat {
                return false;
            }
            let left = left - dt;
            self.react_left = Some(left);
            if left <= 0.0 {
                // too late to matter? still dodge if the invulnerable window can cover the blow
                if eta > 0.02 && matches!(f.kong.mode, KongMode::Main) {
                    inp.stick = Self::stick_toward(f, 1.0);
                    if self.press(inp, f, SLOT_JUMP_ROLL) {
                        self.threat_handled = true;
                        self.note("dodge");
                        return true;
                    }
                } else {
                    self.threat_handled = true;
                }
            }
        }
        false
    }

    fn dodge_followup(&mut self, f: &Fight, inp: &mut KongInput) {
        inp.stick = Self::stick_toward(f, 0.5);
        let st = f.rex.state();
        if st == KtState::KoAuSol || st == KtState::Mort || st == KtState::Finish {
            return;
        }
        match f.proposal() {
            Some(Proposal::Grab) if self.stage == Stage::GrabThrow || self.stage == Stage::Pressure || self.wants_slam(f) => {
                if self.stage == Stage::GrabThrow || !self.grab_done || self.wants_slam(f) {
                    if self.press(inp, f, SLOT_ATTACK) {
                        self.note("grab after dodge");
                    }
                    return;
                }
            }
            _ => {}
        }
        // shoulder strike: the attack after a dodge is the counter lunge (heavy class, invulnerable)
        let counter_ok = match self.stage {
            Stage::Shoulder | Stage::Pressure => self.may_damage(f),
            Stage::Approach | Stage::PunchChain | Stage::Repel | Stage::Downward => false,
            _ => false,
        };
        if counter_ok && f.surface_dist() <= 9.0 && f.proposal().is_none() && f.kong.frame() > 6.0 {
            if self.press(inp, f, SLOT_ATTACK) && self.stage == Stage::Shoulder {
                self.note("shoulder strike (counter lunge)");
            }
        }
    }

    // -------------------------------------------------------------------------------------
    fn grab_mode(&mut self, f: &Fight, inp: &mut KongInput) {
        if let GrabStage::Holding(_) = f.kong.grab {
            if f.rex.life() <= 0.0 {
                // a knocked-down rex that cannot be jaw-broken: ground slam ends it
                if self.press(inp, f, SLOT_JUMP_ROLL) {
                    self.note("ground slam");
                }
                return;
            }
            if self.grab_done && !self.slam_done {
                // second grab of the showcase: slam the rex down (the JUMP_ROLL slot in a hold, KC12)
                if self.press(inp, f, SLOT_JUMP_ROLL) {
                    self.slam_done = true;
                    self.note("slam");
                }
                return;
            }
            if self.grab_step == 0 && f.rex.life() <= GRAB_STRIKE_DAMAGE {
                // the strike would take the last life (no knock-down, no jaw-break): throw right away,
                // the impact makes the knock-down instead [G]
                if self.press(inp, f, SLOT_SPECIAL) {
                    self.grab_step = 2;
                    self.grab_done = true;
                    self.note("throw (rex too low for the strike)");
                }
                return;
            }
            if self.grab_step == 0 {
                if self.press(inp, f, SLOT_ATTACK) {
                    self.grab_step = 1;
                    self.note("grab strike");
                }
            } else if self.press(inp, f, SLOT_SPECIAL) {
                self.grab_step = 2;
                self.grab_done = true;
                self.note("throw");
            }
        }
        if matches!(f.kong.grab, GrabStage::Throw { .. }) && self.stage < Stage::Fury {
            self.grab_done = true;
            // the thrown rex skids and gets up: the safe window for the chest-pound press pattern
            self.fury_wait = 0.0;
            self.goto(Stage::Fury, "thrown, chest pound while it skids");
        }
    }

    fn mash(&mut self, f: &Fight, dt: f32, inp: &mut KongInput) {
        // 8-12 presses per second, alternating buttons [G]
        self.mash_timer -= dt;
        if self.mash_timer <= 0.0 {
            self.mash_timer = self.rng.range(0.085, 0.125);
            self.mash_toggle = !self.mash_toggle;
            inp.press(if self.mash_toggle { SLOT_ATTACK } else { SLOT_SPECIAL });
        }
        let _ = f;
    }

    // -------------------------------------------------------------------------------------
    fn playbook(&mut self, f: &Fight, inp: &mut KongInput) {
        let ph = f.kong.phase;
        let idle = matches!(ph, Phase::None | Phase::Recover);
        let st = f.rex.state();
        let dist = f.dist();
        let engage = if f.rex.is_hesitating() || matches!(st, KtState::Paf | KtState::Cri | KtState::Derap | KtState::KoAuSol) {
            ENGAGE_DIST_CLOSE
        } else {
            ENGAGE_DIST
        };
        // arena control [G]: when the fight has drifted to the wall, lead the rex back toward the middle
        // (it follows at walking pace) so throws have room to fly and the camera has a background
        let rex_r = (f.rex.pos.0 * f.rex.pos.0 + f.rex.pos.1 * f.rex.pos.1).sqrt();
        let kong_r = (f.kong.pos.0 * f.kong.pos.0 + f.kong.pos.1 * f.kong.pos.1).sqrt();
        if idle
            && self.stage > Stage::Approach
            && self.stage < Stage::Finisher
            && rex_r > 16.0
            && kong_r > 5.0
            && f.rex.state() == KtState::FightKong
            && f.proposal().is_none()
            && self.lead_time < 8.0
        {
            let l = kong_r.max(1e-3);
            inp.stick = (-f.kong.pos.0 / l * 0.8, -f.kong.pos.1 / l * 0.8);
            self.lead_time += TICK;
            return;
        }
        if rex_r <= 16.0 {
            self.lead_time = 0.0;
        }
        match self.stage {
            Stage::Approach => {
                // walk, not run, so the rex's opening charge can start and be dodged [G]
                if dist > engage {
                    inp.stick = Self::stick_toward(f, 0.6);
                } else {
                    self.goto(Stage::PunchChain, "in range");
                }
            }
            Stage::PunchChain => {
                self.close_in(f, inp, engage, idle);
                // a chain that whiffed because the rex moved away starts over
                if !idle && ph == Phase::Punch2 && f.landed == 0 && f.surface_dist() > 6.0 {
                    self.punch_seen = false;
                }
                match ph {
                    Phase::None | Phase::Recover if !self.punch_seen => {
                        if f.surface_dist() <= 5.0 && self.safe_to_strike(f) {
                            self.press(inp, f, SLOT_ATTACK);
                        }
                    }
                    Phase::Punch1 => {
                        self.punch_seen = true;
                        self.press(inp, f, SLOT_ATTACK);
                    }
                    Phase::Punch2 => self.goto(Stage::Repel, "punch chain done"),
                    _ => {}
                }
                if ph == Phase::None && self.punch_seen && self.stage_t > 3.0 {
                    // chain ended early (hit-stun): start again
                    self.punch_seen = false;
                }
            }
            Stage::Repel => {
                match ph {
                    Phase::Punch2 | Phase::Punch1 | Phase::Recover | Phase::None => {
                        if !idle || dist <= engage + 2.0 {
                            self.press(inp, f, SLOT_SPECIAL);
                        }
                    }
                    Phase::Repel => self.goto(Stage::Downward, "repel"),
                    _ => {}
                }
                if idle {
                    self.close_in(f, inp, engage, idle);
                }
            }
            Stage::Downward => match ph {
                Phase::Repel => {
                    self.press(inp, f, SLOT_ATTACK);
                }
                Phase::Downward => {
                    // move on once the strike has connected (a dodge can cut it short)
                    if f.last_landed_anim == super::combat::ANIM_DOWNWARD {
                        self.goto(Stage::Shoulder, "downward strike");
                    }
                }
                _ => {
                    if idle {
                        self.close_in(f, inp, engage, idle);
                        // the downward strike only follows a repel (phase graph): if it whiffed, repel again
                        // (at most 3 tries, then go on) [G]
                        if self.stage_t > 0.6 && self.downward_tries < 3 {
                            self.downward_tries += 1;
                            self.goto(Stage::Repel, "downward missed, repel again");
                        } else if self.stage_t > 0.6 {
                            self.goto(Stage::Shoulder, "downward skipped");
                        }
                    }
                }
            },
            Stage::Shoulder => {
                if f.kong.attacks_started > 0 && ph == Phase::CounterLunge {
                    self.shoulder_done = true;
                    self.goto(Stage::GrabThrow, "shoulder strike done");
                }
                if idle {
                    self.close_in(f, inp, engage, idle);
                }
            }
            Stage::GrabThrow => {
                if idle {
                    self.close_in(f, inp, engage, idle);
                    if f.proposal() == Some(Proposal::Grab) {
                        self.press(inp, f, SLOT_ATTACK);
                    }
                }
            }
            Stage::Fury => self.fury_pattern(f, inp),
            Stage::Pressure => self.pressure(f, inp, engage),
            Stage::Finisher => self.finisher(f, inp),
            Stage::Done => {}
        }
        // reaching the next stage by a state change
        if self.stage == Stage::Shoulder && ph == Phase::CounterLunge {
            self.shoulder_done = true;
            self.goto(Stage::GrabThrow, "shoulder strike");
        }
    }

    fn safe_to_strike(&self, f: &Fight) -> bool {
        // do not walk a punch into a rex about to bite unless there is no choice
        if let Some(eta) = f.rex.attack_eta() {
            return eta > 1.1 && self.may_damage(f);
        }
        // a blow only makes sense against a rex that stands (or flinches) in reach, not one that runs, skids or flies
        self.may_damage(f)
            && f.proposal().is_none()
            && matches!(f.rex.state(), KtState::FightKong | KtState::Paf | KtState::Attente | KtState::Cri | KtState::Attaque)
    }

    fn close_in(&mut self, f: &Fight, inp: &mut KongInput, engage: f32, idle: bool) {
        if idle && f.dist() > engage {
            inp.stick = Self::stick_toward(f, 1.0);
        }
    }

    fn fury_pattern(&mut self, f: &Fight, inp: &mut KongInput) {
        let ph = f.kong.phase;
        let idle = matches!(ph, Phase::None | Phase::Recover);
        if f.kong.fury.is_active() {
            return;
        }
        if idle && self.wants_slam(f) && f.proposal() == Some(Proposal::Grab) {
            if self.press(inp, f, SLOT_ATTACK) {
                self.note("second grab (slam)");
            }
            return;
        }
        // wait for a safe moment (rex far away / recovering) but never longer than 6 s [G]
        let st = f.rex.state();
        // the press pattern (special, repel, special, hold the 144-frame pound to frame 130) needs ~3.4 s
        // without a rex blow landing; a player gets it by opening distance (Kong runs 8 m/s, the rex walks
        // 4 m/s) or while the rex is down / skidding / roaring [G: the brain's choice]
        let safe = f.dist() >= FURY_SAFE_DIST
            || f.rex.recover > 1.0
            || f.rex.attack_cd > 3.6
            || matches!(st, KtState::Projectile | KtState::Derap | KtState::Cri | KtState::Grabbed);
        if idle && !safe {
            self.fury_wait += TICK;
            if self.fury_wait < FURY_WAIT_MAX {
                // back off to open distance while waiting (run away from the rex)
                let d = f.to_rex();
                inp.stick = (-d.0, -d.1);
                return;
            }
        }
        match ph {
            Phase::None | Phase::Recover => {
                if self.press(inp, f, SLOT_SPECIAL) {
                    self.fury_tries += 1;
                    self.note("fury: special (repel)");
                }
            }
            Phase::Repel => {
                if self.press(inp, f, SLOT_SPECIAL) {
                    self.note("fury: special again (chest pound)");
                }
            }
            Phase::ChestPound => {
                inp.hold(SLOT_SPECIAL);
            }
            _ => {}
        }
    }

    fn pressure(&mut self, f: &Fight, inp: &mut KongInput, engage: f32) {
        let ph = f.kong.phase;
        let idle = matches!(ph, Phase::None | Phase::Recover);
        let st = f.rex.state();
        if matches!(st, KtState::KoAuSol) {
            self.goto(Stage::Finisher, "rex knocked down");
            return;
        }
        // fury over and the rex is wounded: plain blows no longer hurt it, roar again [G]
        if !f.kong.fury.is_active() && f.rex.wounded() && self.fury_tries < 5 && self.stage_t > 0.5 {
            self.fury_done = false;
            self.fury_wait = 0.0;
            self.goto(Stage::Fury, "fury again");
            return;
        }
        if matches!(st, KtState::Mort | KtState::Finish | KtState::Grabbed) {
            return;
        }
        if idle {
            self.close_in(f, inp, engage, idle);
        }
        if f.dist() > engage + 2.0 && idle {
            return;
        }
        // pacing [G]: after a burst of 3 phases step back and let the rex come (then dodge + counter)
        if self.rest_left > 0.0 {
            self.rest_left -= TICK;
            if idle && f.dist() < 12.0 {
                let d = f.to_rex();
                inp.stick = (-d.0 * 0.7, -d.1 * 0.7);
            }
            return;
        }
        if f.kong.attacks_started >= self.burst_start + BURST_LEN && idle {
            self.burst_start = f.kong.attacks_started;
            self.rest_left = REST_BASE + self.rng.range(0.0, REST_JITTER);
            return;
        }
        // one more blow would knock the rex down: a single blow, no chain (a follow-up would land on the
        // downed rex, whose `hit` sends it to paf and ends the knock-down before the finisher)
        let kill_zone = f.rex.life() <= if f.kong.fury.is_active() { 20.0 } else { 10.0 };
        if kill_zone && !idle {
            return;
        }
        // blows now take 0.85-1.0 s and can only be cancelled at their end (KC01): do not start one into a rex attack that lands sooner [G]
        if Self::attack_soon(f) && (idle || ph == Phase::Punch1) {
            return;
        }
        match ph {
            Phase::None | Phase::Recover => {
                if f.proposal() == Some(Proposal::Grab) && !self.grab_done {
                    // the grab + throw still to show: the throw impact makes the knock-down
                    self.press(inp, f, SLOT_ATTACK);
                } else if f.proposal().is_none() && (self.grab_done || f.rex.life() > super::vrex::IMPACT_DAMAGE + GRAB_STRIKE_DAMAGE + 10.0) {
                    self.press(inp, f, SLOT_ATTACK);
                }
            }
            Phase::Punch1 => {
                self.press(inp, f, SLOT_ATTACK);
            }
            Phase::Punch2 => {
                if self.pressure_toggle {
                    self.press(inp, f, SLOT_SPECIAL);
                }
                self.pressure_toggle = !self.pressure_toggle;
            }
            Phase::Repel | Phase::Downward => {
                self.press(inp, f, SLOT_ATTACK);
            }
            _ => {}
        }
    }

    fn finisher(&mut self, f: &Fight, inp: &mut KongInput) {
        let ph = f.kong.phase;
        let idle = matches!(ph, Phase::None | Phase::Recover);
        let st = f.rex.state();
        match st {
            KtState::KoAuSol => {
                if !idle {
                    return;
                }
                match f.proposal() {
                    Some(Proposal::Finish) => {
                        if f.dist() > 14.0 {
                            inp.stick = Self::stick_toward(f, 1.0);
                        }
                        if self.press(inp, f, SLOT_ATTACK) {
                            self.note("jaw-break finisher");
                        }
                    }
                    Some(Proposal::Grab) => {
                        // second knock-down: no jaw-break any more, grab and slam
                        if self.press(inp, f, SLOT_ATTACK) {
                            self.note("grab the downed rex");
                        }
                    }
                    None => {
                        if f.dist() > ENGAGE_DIST_CLOSE {
                            inp.stick = Self::stick_toward(f, 1.0);
                        }
                    }
                }
            }
            KtState::Mort | KtState::Finish => {}
            _ => {
                // the rex got up (finisher missed or escaped): back to pressure
                self.goto(Stage::Pressure, "rex up again");
            }
        }
    }
}

/// Run a whole fight with the brain; returns the fight and the timestamped events.
pub fn simulate(seed: u32, max_seconds: f32) -> (Fight, KongBrain, Vec<(f32, FightEvent)>) {
    simulate_with(seed, max_seconds, RexProfile::KT_DEFAULT)
}

/// `simulate` with an explicit rex life profile (e.g. `RexProfile::ARENA_07D`).
pub fn simulate_with(seed: u32, max_seconds: f32, profile: RexProfile) -> (Fight, KongBrain, Vec<(f32, FightEvent)>) {
    let mut fight = Fight::with_profile(seed, profile);
    let mut brain = KongBrain::new(seed);
    let mut log = Vec::new();
    let dt = TICK;
    let mut t = 0.0;
    while t < max_seconds && !fight.is_finished() {
        let inp = brain.think(&fight, dt);
        let before = fight.rex.life();
        for e in fight.step(dt, &inp) {
            log.push((fight.time, e));
        }
        if std::env::var("KK_LIFE_TRACE").is_ok() && fight.rex.life() != before {
            eprintln!("LIFE {:6.2} {} -> {} rex {:?}", fight.time, before, fight.rex.life(), fight.rex.state());
        }
        t += dt;
    }
    (fight, brain, log)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kong::combat::{self, BlowCtx};
    use crate::kong::fury;
    use crate::kong::vrex;

    /// One recorded event with the world state just before the step that produced it.
    struct Rec {
        t: f32,
        e: FightEvent,
        rex_wounded: bool,
        rex_state: KtState,
        rex_ko_before: bool,
        dist: f32,
        fury: bool,
    }

    /// Run a brain fight tick by tick, recording each event with the pre-step world state, and
    /// checking the per-tick fury invariants (KF01) on the way.
    fn run_checked(seed: u32, max: f32) -> (Fight, KongBrain, Vec<Rec>) {
        let mut f = Fight::new(seed);
        let mut b = KongBrain::new(seed);
        let mut recs = Vec::new();
        let mut prev_fury = 0.0_f32;
        let mut extend_seen = false;
        while f.time < max && !f.is_finished() {
            let inp = b.think(&f, TICK);
            let pre = (f.rex.wounded(), f.rex.state(), f.rex.machine.knocked_down_before, f.dist(), f.kong.fury.is_active());
            let evs = f.step(TICK, &inp);
            let t = fury_timer_check(&f, prev_fury, &evs, &mut extend_seen);
            prev_fury = t;
            for e in evs {
                recs.push(Rec { t: f.time, e, rex_wounded: pre.0, rex_state: pre.1, rex_ko_before: pre.2, dist: pre.3, fury: pre.4 });
            }
        }
        (f, b, recs)
    }

    /// KF01: 18.0 at the start, +1.5 per landed blow capped at 15.0, -penalties when hit, otherwise
    /// it only runs down by dt per tick; it never grows by itself and never exceeds 18.
    fn fury_timer_check(f: &Fight, prev: f32, evs: &[FightEvent], extend_seen: &mut bool) -> f32 {
        let now = f.kong.fury.timer;
        let started = evs.iter().any(|e| matches!(e, FightEvent::FuryStart { .. }));
        let extended = evs.iter().any(|e| matches!(e, FightEvent::FuryExtend { .. }));
        let hurt = evs.iter().any(|e| matches!(e, FightEvent::Hit { victim: Actor::Kong, .. }));
        let landed = evs.iter().any(|e| matches!(e, FightEvent::Hit { attacker: Actor::Kong, .. }));
        if started {
            assert!((now - 18.0).abs() < 0.05, "fury starts at 18.0, got {now}");
            *extend_seen = false;
        }
        if extended {
            assert!(now <= fury::FURY_EXTEND_CAP + 1e-3, "extension is capped at 15.0: {now}");
            *extend_seen = true;
        }
        assert!(now <= 18.0 + 1e-3);
        if !started && !extended && !hurt && !landed && prev > 0.0 {
            assert!(now <= prev + 1e-4, "fury timer grew by itself: {prev} -> {now}");
            assert!(prev - now <= TICK + 1e-4, "fury timer drops by at most one tick: {prev} -> {now}");
        }
        now
    }

    fn first(recs: &[Rec], p: impl Fn(&FightEvent) -> bool) -> Option<f32> {
        recs.iter().find(|r| p(&r.e)).map(|r| r.t)
    }

    #[test]
    fn full_fight_shows_every_move_in_order_and_the_rex_dies() {
        let (f, b, recs) = run_checked(1, 120.0);
        // the fight ends with the rex dead, inside 120 s, Kong alive
        assert_eq!(f.rex.state(), KtState::Mort);
        assert!(f.time < 120.0, "fight took {}", f.time);
        // with the recovered life profile (rex 80/80/50, B02 instance data) a real fight is short: 30-70 s
        assert!(f.time > 25.0, "fight is a showcase, took only {}", f.time);
        assert_ne!(f.kong.mode, KongMode::Dead);
        assert_eq!(f.over, Some(Actor::Kong));

        let phase = |p: Phase| first(&recs, |e| matches!(e, FightEvent::KongPhase { phase } if *phase == p));
        let t_approach = first(&recs, |e| matches!(e, FightEvent::Anim { actor: Actor::Kong, id, .. } if *id == ANIM_WALK || *id == ANIM_RUN));
        let t_charge_dodge = first(&recs, |e| matches!(e, FightEvent::HitAvoided { attack: RexMove::Charge, .. }));
        let t_p1 = phase(Phase::Punch1).expect("punch 1");
        let t_p2 = phase(Phase::Punch2).expect("punch 2");
        let t_repel = phase(Phase::Repel).expect("repel");
        let t_down = phase(Phase::Downward).expect("downward strike");
        let t_side = phase(Phase::DodgeSide).expect("side-step");
        let t_shoulder = phase(Phase::CounterLunge).expect("shoulder strike");
        let t_avoid = first(&recs, |e| matches!(e, FightEvent::HitAvoided { .. })).expect("an avoided rex attack");
        let t_grab = first(&recs, |e| matches!(e, FightEvent::GrabStart)).expect("grab");
        let t_gstrike = first(&recs, |e| matches!(e, FightEvent::GrabStrike)).expect("grab strike (bite)");
        let t_throw = first(&recs, |e| matches!(e, FightEvent::Throw { .. })).expect("throw");
        let t_impact = first(&recs, |e| matches!(e, FightEvent::ThrowImpact { .. })).expect("throw impact");
        let t_pound = first(&recs, |e| matches!(e, FightEvent::PoundStart)).expect("chest pound");
        let t_fury = first(&recs, |e| matches!(e, FightEvent::FuryStart { .. })).expect("fury");
        let t_ko = first(&recs, |e| matches!(e, FightEvent::KoStart { .. })).expect("rex knocked down");
        let t_fin = first(&recs, |e| matches!(e, FightEvent::FinisherStart)).expect("finisher start");
        let t_mash = first(&recs, |e| matches!(e, FightEvent::FinisherMash { .. })).expect("finisher mash");
        let t_win = first(&recs, |e| matches!(e, FightEvent::FinisherSuccess)).expect("finisher success");
        let t_dead = first(&recs, |e| matches!(e, FightEvent::RexDied)).expect("rex died");
        let t_vpound = first(&recs, |e| matches!(e, FightEvent::VictoryPound)).expect("victory pound");
        let t_vroar = first(&recs, |e| matches!(e, FightEvent::VictoryRoar)).expect("victory roar");
        let t_over = first(&recs, |e| matches!(e, FightEvent::FightOver { winner: Actor::Kong })).expect("fight over");
        assert!(t_approach.is_some() && t_charge_dodge.is_some(), "approach and a dodged charge");
        // natural order of the showcase
        let order = [
            t_p1, t_p2, t_repel, t_down, t_shoulder, t_grab, t_gstrike, t_throw, t_impact, t_fury, t_ko, t_fin, t_mash, t_win, t_dead, t_vpound, t_vroar, t_over,
        ];
        for w in order.windows(2) {
            assert!(w[0] <= w[1], "moves out of order: {order:?}");
        }
        assert!(t_pound < t_fury && t_side <= t_shoulder && t_avoid <= t_shoulder + 10.0);
        // the rex was also seen doing its own repertoire and the stages were announced
        assert!(b.notes.len() > 15);
        // dodge of an incoming rex attack (not only the charge)
        assert!(recs.iter().any(|r| matches!(r.e, FightEvent::HitAvoided { attack, .. } if attack != RexMove::Charge)));
    }

    #[test]
    fn hits_respect_the_recovered_damage_classes_and_the_wounded_rule() {
        let (_f, _b, recs) = run_checked(1, 120.0);
        let mut kong_hits = 0;
        let mut zero_on_wounded = 0;
        for r in &recs {
            if let FightEvent::Hit { attacker: Actor::Kong, victim: Actor::Rex, class, damage, applied, anim, .. } = r.e {
                kong_hits += 1;
                let (ec, ed) = if anim == combat::GRAB_STRIKE_ANIM as u32 {
                    (combat::GRAB_STRIKE.class, combat::GRAB_STRIKE.damage)
                } else {
                    let b = combat::blow(&BlowCtx { action: combat::action::MAIN, anim, fury: r.fury, ..Default::default() });
                    (b.class, b.damage)
                };
                assert_eq!((class, damage), (ec, ed), "blow table mismatch for anim {anim:#x} fury={}", r.fury);
                let expect = vrex::hit_damage(class, class, r.rex_wounded, damage as f32);
                assert_eq!(applied, expect, "applied damage for class {class:#x}, wounded={}", r.rex_wounded);
                if r.rex_wounded && class == 1 {
                    assert_eq!(applied, 0.0);
                    zero_on_wounded += 1;
                }
                // fury doubles the plain blows
                if r.fury && anim == combat::ANIM_PUNCH_A as u32 {
                    assert_eq!((class, damage), (2, 20));
                }
                if !r.fury && anim == combat::ANIM_PUNCH_A as u32 {
                    assert_eq!((class, damage), (1, 10));
                }
                if anim == combat::ANIM_REPEL as u32 {
                    assert_eq!(applied, 0.0, "repel deals no life damage (class 0x14)");
                }
            }
            if let FightEvent::Hit { attacker: Actor::Rex, victim: Actor::Kong, class, damage, .. } = r.e {
                // rex packets are the recovered table
                let ok = [vrex::ATTACK_JAW, vrex::ATTACK_HEADBUTT, vrex::ATTACK_CHARGE, vrex::ATTACK_SWEEP, vrex::ATTACK_TAIL]
                    .iter()
                    .any(|&(fl, d)| fl == class && d == damage);
                assert!(ok, "unknown rex packet {class:#x}/{damage}");
            }
        }
        assert!(kong_hits >= 6, "{kong_hits} kong hits (rex 80 life: the six distinct blows below are the whole budget)");
        let _ = zero_on_wounded;
        // every distinct blow of the showcase appeared with its class
        for (anim, class) in [(0x17u32, 1u32), (0x1d, 1), (0xac, 0x14), (0x1f, 9), (0x16, 2), (0xa2, 9)] {
            assert!(
                recs.iter().any(|r| matches!(r.e, FightEvent::Hit { attacker: Actor::Kong, anim: a, class: c, .. } if a == anim && (c == class || (r.fury && c == (class | 2) )))),
                "no hit with anim {anim:#x}"
            );
        }
    }

    #[test]
    fn fury_follows_kf01_and_slows_the_rex_down_nothing() {
        let (f, _b, recs) = run_checked(1, 120.0);
        let starts: Vec<f32> = recs.iter().filter(|r| matches!(r.e, FightEvent::FuryStart { .. })).map(|r| r.t).collect();
        assert!(!starts.is_empty());
        // fury was entered through the pound pattern: PoundStart precedes every FuryStart, and the
        // fury roar anim 0xad is requested at the start
        for &s in &starts {
            assert!(recs.iter().any(|r| r.t <= s && matches!(r.e, FightEvent::PoundStart)));
            assert!(recs.iter().any(|r| (r.t - s).abs() < 0.02 && matches!(r.e, FightEvent::Anim { actor: Actor::Kong, id, .. } if id == fury::ANIM_FURY_ROAR)));
        }
        // while in fury, animations play at 1.25
        assert!(recs.iter().any(|r| r.fury && matches!(r.e, FightEvent::Anim { actor: Actor::Kong, speed, .. } if (speed - 1.25).abs() < 1e-6)));
        // heavy blows in fury (class 2 or 10) can knock the rex down: the feedback event shows up
        assert!(recs.iter().any(|r| r.fury && matches!(r.e, FightEvent::KnockdownFeedback { class } if class & 2 != 0)));
        // fury is still sane at the end
        assert!(f.kong.fury.timer <= 18.0);
        // the grab happened before fury (creatures cannot be grabbed ... Kong holds nothing in fury)
        let t_grab = first(&recs, |e| matches!(e, FightEvent::GrabStart)).unwrap();
        assert!(t_grab < starts[0]);
    }

    #[test]
    fn finisher_only_starts_in_the_recovered_conditions() {
        for seed in 1..=6 {
            let (_f, _b, recs) = run_checked(seed, 120.0);
            let mut n = 0;
            for r in &recs {
                if matches!(r.e, FightEvent::FinisherStart) {
                    n += 1;
                    assert_eq!(r.rex_state, KtState::KoAuSol, "rex must be knocked out");
                    assert!(r.rex_wounded, "rex must be wounded");
                    assert!(!r.rex_ko_before, "the finisher window is only the first knock-down (Rex+0x4e8)");
                    assert!(r.dist < vrex::FINISH_PROPOSE_RADIUS, "Kong within 22, was {}", r.dist);
                }
            }
            assert!(n >= 1, "seed {seed}: no finisher");
        }
    }

    #[test]
    fn many_seeds_all_end_with_the_rex_dead_inside_120_s() {
        for seed in 1..=64 {
            let (f, _b, recs) = run_checked(seed, 120.0);
            assert_eq!(f.rex.state(), KtState::Mort, "seed {seed}: rex not dead at {}", f.time);
            assert!(f.time < 120.0, "seed {seed}: {}", f.time);
            assert!(recs.iter().any(|r| matches!(r.e, FightEvent::FightOver { winner: Actor::Kong })), "seed {seed}");
            // every seed still shows the headline moves
            for (name, p) in [
                ("throw", &(|e: &FightEvent| matches!(e, FightEvent::Throw { .. })) as &dyn Fn(&FightEvent) -> bool),
                ("fury", &|e| matches!(e, FightEvent::FuryStart { .. })),
                ("dodge", &|e| matches!(e, FightEvent::HitAvoided { .. })),
                ("shoulder", &|e| matches!(e, FightEvent::KongPhase { phase: Phase::CounterLunge })),
                ("ko", &|e| matches!(e, FightEvent::KoStart { .. })),
                ("finisher", &|e| matches!(e, FightEvent::FinisherSuccess)),
            ] {
                assert!(recs.iter().any(|r| p(&r.e)), "seed {seed}: missing {name}");
            }
        }
    }

    #[test]
    fn the_07d_arena_rex_also_ends_by_ko_and_finisher_with_every_move() {
        // 07D swamp rex J_PNJ_KTREX_2: life 50/50/25 (instance data, tunables.json)
        // the 50-life rex leaves little room: always KO + finisher, the full showcase on almost every seed
        let mut full = 0;
        for seed in 1..=64 {
            let (f, _b, log) = simulate_with(seed, 120.0, RexProfile::ARENA_07D);
            assert_eq!(f.rex.state(), KtState::Mort, "seed {seed}");
            assert!(log.iter().any(|(_, e)| matches!(e, FightEvent::FinisherSuccess)), "07D seed {seed}: no finisher");
            let has = |p: &dyn Fn(&FightEvent) -> bool| log.iter().any(|(_, e)| p(e));
            if has(&|e| matches!(e, FightEvent::Throw { .. })) && has(&|e| matches!(e, FightEvent::FuryStart { .. })) && has(&|e| matches!(e, FightEvent::HitAvoided { .. })) {
                full += 1;
            }
        }
        assert!(full >= 58, "only {full}/64 07D fights showed throw + fury + dodge");
    }

    #[test]
    fn the_05c_marsh_rex_ends_by_ko_and_finisher_with_every_move() {
        // 05C marsh rexes PNJ_KTREX: life 55/55/40 (instance data, 05c_instances.json): wounded after 15 damage
        let mut full = 0;
        for seed in 1..=64 {
            let (f, _b, log) = simulate_with(seed, 120.0, RexProfile::MARSH_05C);
            assert_eq!(f.rex.state(), KtState::Mort, "seed {seed}");
            assert!(log.iter().any(|(_, e)| matches!(e, FightEvent::FinisherSuccess)), "05C seed {seed}: no finisher");
            let has = |p: &dyn Fn(&FightEvent) -> bool| log.iter().any(|(_, e)| p(e));
            let shoulder = log.iter().any(|(_, e)| matches!(e, FightEvent::Hit { attacker: Actor::Kong, anim: 0x16, .. }));
            if shoulder && has(&|e| matches!(e, FightEvent::Throw { .. })) && has(&|e| matches!(e, FightEvent::FuryStart { .. })) && has(&|e| matches!(e, FightEvent::HitAvoided { .. })) {
                full += 1;
            }
        }
        // the recovered knock-back (8/3 m per blow) shortens the low-life marsh fights, so a few seeds
        // end before the demo brain has shown every move
        assert!(full >= 52, "only {full}/64 05C fights showed throw + fury + dodge");
    }

    #[test]
    fn the_fight_is_deterministic_per_seed() {
        let (_, _, a) = simulate(7, 120.0);
        let (_, _, b) = simulate(7, 120.0);
        let (_, _, c) = simulate(8, 120.0);
        assert_eq!(a, b);
        assert_ne!(a, c, "different seeds should differ in timing");
        // splitting the time step does not change the outcome either
        let mut f = Fight::new(7);
        let mut br = KongBrain::new(7);
        let mut log = Vec::new();
        while f.time < 120.0 && !f.is_finished() {
            let inp = br.think(&f, TICK);
            // feed the same tick as two half steps: edges are applied once
            let mut first = inp;
            let mut second = inp;
            for s in 0..4 {
                second.buttons[s].pressed = false;
            }
            first.stick = inp.stick;
            for e in f.step(TICK / 2.0, &first) {
                log.push((f.time, e));
            }
            for e in f.step(TICK / 2.0, &second) {
                log.push((f.time, e));
            }
        }
        let ev_a: Vec<&FightEvent> = a.iter().map(|(_, e)| e).collect();
        let ev_l: Vec<&FightEvent> = log.iter().map(|(_, e)| e).collect();
        assert_eq!(ev_a, ev_l);
    }

    #[test]
    fn brain_dodges_a_charge_and_the_rex_takes_20_when_it_hits_the_wall() {
        // rex far away facing Kong: the opening charge is answered by a side-step, not a hit
        let (_f, _b, recs) = run_checked(3, 20.0);
        let charge = first(&recs, |e| matches!(e, FightEvent::RexAttack { kind: RexMove::Charge })).expect("opening charge");
        let avoid = first(&recs, |e| matches!(e, FightEvent::HitAvoided { attack: RexMove::Charge, .. })).expect("charge avoided");
        assert!(avoid > charge && avoid - charge < 2.5);
        assert!(!recs.iter().any(|r| r.t < avoid + 0.5 && matches!(r.e, FightEvent::Hit { attacker: Actor::Rex, class: 0x42, .. })));
    }

    #[test]
    fn a_player_can_drive_the_same_state_machine_with_a_pad() {
        // no brain: scripted pad presses produce the recovered combo graph
        let mut f = Fight::new(1);
        f.kong.pos = (0.0, 0.0);
        f.rex.pos = (7.0, 0.0);
        f.rex.attack_cd = 1e9;
        f.rex.charge_cd = 1e9;
        let mut seen = Vec::new();
        let press = |f: &mut Fight, slot: usize, seen: &mut Vec<Phase>| {
            let mut i = KongInput::default();
            i.press(slot);
            for e in f.step(TICK, &i) {
                if let FightEvent::KongPhase { phase } = e {
                    seen.push(phase);
                }
            }
            for _ in 0..8 {
                for e in f.step(TICK, &KongInput::default()) {
                    if let FightEvent::KongPhase { phase } = e {
                        seen.push(phase);
                    }
                }
            }
        };
        press(&mut f, SLOT_ATTACK, &mut seen); // punch 1
        press(&mut f, SLOT_ATTACK, &mut seen); // punch 2 (latched, runs at the cancel window)
        for _ in 0..60 {
            for e in f.step(TICK, &KongInput::default()) {
                if let FightEvent::KongPhase { phase } = e {
                    seen.push(phase);
                }
            }
        }
        assert!(seen.starts_with(&[Phase::Punch1, Phase::Punch2]), "{seen:?}");
    }
}
#[cfg(test)]
mod timeline {
    use super::*;

    /// `cargo test -p kk-mechanics print_timeline -- --ignored --nocapture` prints the move timeline of seed 1.
    #[test]
    #[ignore]
    fn print_timeline() {
        let seed = std::env::var("KK_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        let prof = if std::env::var("KK_07D").is_ok() { RexProfile::ARENA_07D } else if std::env::var("KK_05C").is_ok() { RexProfile::MARSH_05C } else { RexProfile::KT_DEFAULT };
        let (f, b, log) = simulate_with(seed, 150.0, prof);
        for (t, e) in &log {
            if !matches!(e, FightEvent::Anim { .. } | FightEvent::CameraShake { .. } | FightEvent::FinisherMash { .. }) {
                println!("{:6.2} {:?}", t, e);
            }
        }
        for n in &b.notes {
            println!("NOTE {:6.2} {:?} {}", n.time, n.stage, n.text);
        }
        println!("end t={:.1} rex={:?} kong life {:.0}", f.time, f.rex.state(), f.kong.life);
    }

    /// `KK_07D=1 cargo test -p kk-mechanics print_seed_coverage -- --ignored --nocapture`: which seeds show every
    /// move the scene batches check (shoulder strike, Kong stunned, throw, fury, dodge, finisher)
    #[test]
    #[ignore]
    fn print_seed_coverage() {
        let prof = if std::env::var("KK_07D").is_ok() { RexProfile::ARENA_07D } else if std::env::var("KK_05C").is_ok() { RexProfile::MARSH_05C } else { RexProfile::KT_DEFAULT };
        for seed in 1..=40 {
            let (f, _b, log) = simulate_with(seed, 150.0, prof);
            let has = |p: &dyn Fn(&FightEvent) -> bool| log.iter().any(|(_, e)| p(e));
            let sh = has(&|e| matches!(e, FightEvent::Hit { attacker: Actor::Kong, anim: 0x16, .. }));
            let st = has(&|e| matches!(e, FightEvent::Hit { attacker: Actor::Rex, .. })) || has(&|e| matches!(e, FightEvent::KongStunned { .. }));
            let th = has(&|e| matches!(e, FightEvent::Throw { .. }));
            let fu = has(&|e| matches!(e, FightEvent::FuryStart { .. }));
            let fi = has(&|e| matches!(e, FightEvent::FinisherSuccess));
            println!("seed {seed:2}: {:.1} s shoulder {sh} stunned {st} throw {th} fury {fu} finisher {fi}", f.time);
        }
    }

    #[test]
    #[ignore]
    fn print_seed_times() {
        for seed in 1..=64 {
            let (f, _b, _log) = simulate(seed, 150.0);
            println!("seed {seed:2}: {:.1} s, rex {:?}, Kong life {:.0}", f.time, f.rex.state(), f.kong.life);
        }
    }
}
#[cfg(test)]
mod dbg {
    use super::*;
    #[test]
    #[ignore]
    fn dbg_state() {
        let mut f = Fight::new(1);
        let mut b = KongBrain::new(1);
        while f.time < 14.0 {
            let inp = b.think(&f, TICK);
            f.step(TICK, &inp);
            if (f.time * 60.0) as i32 % 30 == 0 && f.time > 3.0 {
                println!("{:.2} stage {:?} phase {:?} mode {:?} dist {:.1} sd {:.1} rex {:?} cd {:.2} rec {:.2} life {} prop {:?} soon {}", f.time, b.stage, f.kong.phase, f.kong.mode, f.dist(), f.surface_dist(), f.rex.state(), f.rex.attack_cd, f.rex.recover, f.rex.life(), f.proposal(), KongBrain::attack_soon(&f));
            }
        }
    }
}
