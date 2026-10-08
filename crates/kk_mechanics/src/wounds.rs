//! Jack's wound model (ledger H01-H04, H09, H10): `H_exec_ch_Stimulus_Paf@0x5a3060` (hit receiver),
//! `H_TRACK_joueur@0x5816f0` (wound timer -> recovering, death camera), `H_TRACK_Reflex@0x58de70`
//! (cooldown tick, hazard death), `H_ETAT_ann_heal@0x5d0380` (cure), `H_ETAT_IA_mort@0x5a3e80`
//! (death state), `Stats_OnPlayerDeath@0x52f240` (death counter `G+0x4dc8`).
//!
//! CORRECTION (wave 3): the status values stored at `G+0x1bb4+4*slot` (slot 1 = Jack) are
//! **0 healthy, 1 wounded, 2 recovering, 3 dead** [C] (`H_TRACK_init@0x5786d0` writes 0 at spawn;
//! `H_TRACK_PublishBeacon@0x53ad10` maps 0/2/3/else(1) to life ratio 1.0/0.3/0.0/0.15). The older
//! labels 1/2/3 = Healthy/Wounded/Dead were shifted by one. Also `G+0x4dc8` is NOT a difficulty
//! setting: it is the number of deaths since the last map change (`Stats_OnPlayerDeath` ++,
//! `ES_CheckPoint`/`ES_ExitMap`/`uni_go` reset to 0). The wound timers shrink as you die more often.
//!
//! Jack never heals by himself: the wound timer only moves 1 -> 2; 2 -> 0 needs Ann
//! (`H_ETAT_ann_heal`, 5.0 s, 2.0 s for Jack) [C]. A hit on status 0 or 2 wounds; a hit on status 1
//! kills once the post-hit cooldown (`this[0x328]`) is over.

/// Paf flag bits tested by `H_exec_ch_Stimulus_Paf` on `this[0x321]` (= paf flags) [C].
pub mod flag {
    /// bit 2: medium hit
    pub const MEDIUM: u32 = 0x2;
    /// bit 4: heavy hit (rex bite 0x4104 has it)
    pub const HEAVY: u32 = 0x4;
    /// bit 0x40: knock-back direction is recorded
    pub const KNOCKBACK: u32 = 0x40;
    /// bit 0x100: force wounded even inside the cooldown (never kills by itself)
    pub const FORCE_WOUND: u32 = 0x100;
    /// bit 0x200: instant kill (rex grab 0x4a10)
    pub const KILL: u32 = 0x200;
    /// bit 0x1000: selects camera reaction 0x8000000b instead of 0x80000000
    pub const CAM_ALT: u32 = 0x1000;
}

/// Values of `G+0x1bb4+4*slot` [C].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WoundState {
    Healthy = 0,
    Wounded = 1,
    /// wound timer elapsed; hit logic identical to Healthy, life ratio 0.3 until Ann heals
    Recovering = 2,
    Dead = 3,
}

impl WoundState {
    /// Life ratio published to the creatures by `H_TRACK_PublishBeacon@0x53ad10` [C].
    pub fn life_ratio(self) -> f32 {
        match self {
            WoundState::Healthy => 1.0,
            WoundState::Recovering => 0.3,
            WoundState::Dead => 0.0,
            WoundState::Wounded => 0.15,
        }
    }
}

/// Hit classes (flags 4 / 2 / neither) [C].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    /// paf flag 4
    Heavy,
    /// paf flag 2
    Medium,
    /// anything else
    Light,
}

impl HitKind {
    /// Classification order in the receiver: bit 4 first, then bit 2 [C].
    pub fn from_flags(flags: u32) -> HitKind {
        if flags & flag::HEAVY != 0 {
            HitKind::Heavy
        } else if flags & flag::MEDIUM != 0 {
            HitKind::Medium
        } else {
            HitKind::Light
        }
    }
    pub fn flags(self) -> u32 {
        match self {
            HitKind::Heavy => flag::HEAVY,
            HitKind::Medium => flag::MEDIUM,
            HitKind::Light => 0,
        }
    }
}

/// Tier derived from `G+0x4dc8` (deaths since the last map change): 0, 1, 2 and more [C].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathTier {
    NoDeaths = 0,
    OneDeath = 1,
    TwoOrMore = 2,
}

impl DeathTier {
    pub fn from_deaths(deaths: u32) -> DeathTier {
        match deaths {
            0 => DeathTier::NoDeaths,
            1 => DeathTier::OneDeath,
            _ => DeathTier::TwoOrMore,
        }
    }
}

/// Seconds Jack stays wounded (`this[0xb8f]`, copied to `this[0xcfa]`; `H_TRACK_joueur` compares the
/// time since the hit against it) [C]: heavy 15.5/13.5/10.5, medium 10.5/7.5/7.5, light 5.5/5.5/4.5.
pub fn wound_duration(kind: HitKind, tier: DeathTier) -> f32 {
    let d = tier as usize;
    match kind {
        HitKind::Heavy => [15.5, 13.5, 10.5][d],
        HitKind::Medium => [10.5, 7.5, 7.5][d],
        HitKind::Light => [5.5, 5.5, 4.5][d],
    }
}

/// Post-hit cooldown (`this[0x328]`, decremented per frame in `H_TRACK_Reflex`) [C]:
/// heavy 3/3/5, medium 3/3/4, light 3/4/3. A hit inside it cannot kill (except flag 0x200).
pub fn hit_cooldown(kind: HitKind, tier: DeathTier) -> f32 {
    let d = tier as usize;
    match kind {
        HitKind::Heavy => [3.0, 3.0, 5.0][d],
        HitKind::Medium => [3.0, 3.0, 4.0][d],
        HitKind::Light => [3.0, 4.0, 3.0][d],
    }
}

/// Kept for callers that only need a nominal value (the normal-tier cooldown of a light hit).
pub const HIT_COOLDOWN_S: f32 = 3.0;
/// Death camera phase in `H_TRACK_joueur`: ragdoll/zoom runs, after `+0x2f24 > 3.0` it goes to the
/// death state (`> 0.5` starts the death camera mode, `> 1.5` stops the body) [C].
pub const DEATH_SEQUENCE_S: f32 = 3.0;
/// `H_ETAT_IA_mort` (state 0x2bd), Jack branch: restart requested once `this[0x2be] > 4.0` and the
/// fade object reports done / distance flag <= 0.5 [C]; hard timeout 8.0 s [C].
pub const MORT_RESTART_S: f32 = 4.0;
pub const MORT_TIMEOUT_S: f32 = 8.0;
/// Frames between `fn@0x00405130` (restart request) and the actual scene reload `fn@0x00405160`
/// (`DAT_00b0da28 = 30`, decremented per frame in `fn@0x008ff020`) [C].
pub const RESTART_DELAY_FRAMES: u32 = 30;
/// Deadly-hazard dwell (`fn@0x0042b770(obj,0x40)` true and `fn@0x00689f80(pos)` true): Jack dies after
/// 1.0 s, a companion after 2.0 s (`H_TRACK_Reflex`) [C].
pub const HAZARD_DEATH_JACK_S: f32 = 1.0;
pub const HAZARD_DEATH_AI_S: f32 = 2.0;
/// `G+0x4dd4` menu-lock timer set on Jack's death (read by `IntMIG_wait`) [C].
pub const DEATH_MENU_LOCK_S: f32 = 5.0;
/// Heartbeat period while Jack is wounded (`H_TRACK_joueur`): 0.5 s, 0.91 s when `+0x3d38` is set [C];
/// each beat fires a rumble pulse `fn@0x00a69150(100, 1)`.
pub const HEARTBEAT_S: f32 = 0.5;
/// Ann's cure time for Jack (`H_ETAT_ann_heal`): 2.0 s when Jack is alive, 5.0 s otherwise [C].
pub const ANN_HEAL_JACK_S: f32 = 2.0;
pub const ANN_HEAL_DEAD_S: f32 = 5.0;

/// What a hit did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitOutcome {
    /// gated out (dead, invulnerable, cooldown)
    Ignored,
    Wounded,
    Killed,
}

#[derive(Clone, Copy, Debug)]
pub struct Wounds {
    pub state: WoundState,
    /// `G+0x4dc8`: deaths since the last map change
    pub deaths: u32,
    /// time since the wound started (`Time_Now - this[0xb93]`)
    pub since_wound: f32,
    /// length of the current wound (`this[0xcfa]`)
    pub wound_len: f32,
    /// `this[0xb8f]`: hurt timer (counts down per frame, drives the screen effect)
    pub hurt_timer: f32,
    /// `this[0x328]`: post-hit cooldown
    pub cooldown: f32,
    /// `this[0xf4e]`: set by medium/heavy wounds (low-health rumble + sfx variant)
    pub strong_wound: bool,
    /// cheat bit 0 of `G+0xbf0`: all stimuli ignored
    pub invulnerable: bool,
    /// time in the death state (`this[0x2be]`)
    pub dead_time: f32,
}

impl Wounds {
    pub fn new(deaths: u32) -> Self {
        Wounds {
            state: WoundState::Healthy,
            deaths,
            since_wound: 0.0,
            wound_len: 10.0, // `this[0xcfa]` reset to 10.0 while status 0 (H_TRACK_joueur) [C]
            hurt_timer: 0.0,
            cooldown: 0.0,
            strong_wound: false,
            invulnerable: false,
            dead_time: 0.0,
        }
    }

    pub fn tier(&self) -> DeathTier {
        DeathTier::from_deaths(self.deaths)
    }

    /// `H_exec_ch_Stimulus_Paf` (Jack branch, `*this != 0`) for one paf with `flags`.
    pub fn hit(&mut self, flags: u32) -> HitOutcome {
        if self.invulnerable || self.state == WoundState::Dead {
            return HitOutcome::Ignored;
        }
        let mut outcome = HitOutcome::Ignored;
        if self.cooldown == 0.0 {
            if self.state == WoundState::Wounded {
                self.state = WoundState::Dead;
                outcome = HitOutcome::Killed;
            } else {
                let kind = HitKind::from_flags(flags);
                let tier = self.tier();
                self.state = WoundState::Wounded;
                self.since_wound = 0.0;
                self.strong_wound = kind != HitKind::Light;
                self.cooldown = hit_cooldown(kind, tier);
                self.hurt_timer = wound_duration(kind, tier);
                self.wound_len = self.hurt_timer;
                outcome = HitOutcome::Wounded;
            }
        }
        if flags & flag::KILL != 0 {
            self.state = WoundState::Dead;
            outcome = HitOutcome::Killed;
        } else if flags & flag::FORCE_WOUND != 0 && self.state != WoundState::Dead {
            if self.state != WoundState::Wounded {
                self.since_wound = 0.0;
                outcome = HitOutcome::Wounded;
            }
            self.state = WoundState::Wounded;
        }
        outcome
    }

    /// Per-frame update: cooldown and hurt timer count down; an elapsed wound becomes Recovering
    /// (`H_TRACK_joueur`); in the dead state the death timer runs.
    pub fn tick(&mut self, dt: f32) {
        self.cooldown = (self.cooldown - dt).max(0.0);
        self.hurt_timer = (self.hurt_timer - dt).max(0.0);
        match self.state {
            WoundState::Wounded => {
                self.since_wound += dt;
                if self.since_wound >= self.wound_len {
                    self.state = WoundState::Recovering;
                }
            }
            WoundState::Dead => self.dead_time += dt,
            _ => {}
        }
    }

    /// Ann's cure (`H_ETAT_ann_heal`): status -> 0 and the hurt timer is cleared. Also revives a
    /// dead Jack while the death state has not yet requested the restart (`H_ETAT_IA_mort` sees 0).
    pub fn heal(&mut self) {
        self.state = WoundState::Healthy;
        self.hurt_timer = 0.0;
        self.dead_time = 0.0;
    }

    /// Hazard dwell (water/deadly volume): kills after `HAZARD_DEATH_JACK_S` seconds of contact.
    pub fn hazard_dwell(&mut self, seconds_in_hazard: f32) {
        if seconds_in_hazard > HAZARD_DEATH_JACK_S && !self.invulnerable {
            self.state = WoundState::Dead;
        }
    }

    /// True when the death state asks for the restart (`H_ETAT_IA_mort`): dead for more than 4.0 s
    /// (or 8.0 s timeout).
    pub fn restart_due(&self) -> bool {
        self.state == WoundState::Dead && self.dead_time > MORT_RESTART_S
    }

    /// `Stats_OnPlayerDeath` + `fn@0x00405130`: count the death and request the checkpoint restart.
    /// Returns the frame delay before the reload.
    pub fn commit_death(&mut self) -> u32 {
        self.deaths += 1;
        RESTART_DELAY_FRAMES
    }

    /// Respawn at the checkpoint (`H_TRACK_init` writes 0); the death counter survives, the map-change
    /// reset (`ES_CheckPoint`, `ES_ExitMap`) is [`Wounds::map_changed`].
    pub fn respawn(&mut self) {
        let deaths = self.deaths;
        *self = Wounds::new(deaths);
    }

    pub fn map_changed(&mut self) {
        self.deaths = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const LIGHT: u32 = 0;
    const REX_BITE: u32 = 0x4104;
    const REX_GRAB: u32 = 0x4a10;

    #[test]
    fn status_numbering_and_life_ratio() {
        assert_eq!(WoundState::Healthy as u8, 0);
        assert_eq!(WoundState::Wounded as u8, 1);
        assert_eq!(WoundState::Recovering as u8, 2);
        assert_eq!(WoundState::Dead as u8, 3);
        // fn@0x0053ad10: 0 -> 1.0, 2 -> 0.3, 3 -> 0, 1 -> 0.15
        assert_eq!(WoundState::Healthy.life_ratio(), 1.0);
        assert!((WoundState::Recovering.life_ratio() - 0.3).abs() < 1e-6);
        assert_eq!(WoundState::Dead.life_ratio(), 0.0);
        assert!((WoundState::Wounded.life_ratio() - 0.15).abs() < 1e-6);
    }

    #[test]
    fn durations_and_cooldowns_by_death_tier() {
        use DeathTier::*;
        assert_eq!(wound_duration(HitKind::Heavy, NoDeaths), 15.5);
        assert_eq!(wound_duration(HitKind::Heavy, TwoOrMore), 10.5);
        assert_eq!(wound_duration(HitKind::Medium, OneDeath), 7.5);
        assert_eq!(wound_duration(HitKind::Light, TwoOrMore), 4.5);
        assert_eq!(hit_cooldown(HitKind::Heavy, TwoOrMore), 5.0);
        assert_eq!(hit_cooldown(HitKind::Medium, TwoOrMore), 4.0);
        assert_eq!(hit_cooldown(HitKind::Light, OneDeath), 4.0);
        assert_eq!(hit_cooldown(HitKind::Light, NoDeaths), 3.0);
        assert_eq!(DeathTier::from_deaths(7), TwoOrMore);
    }

    #[test]
    fn rex_bite_wounds_then_kills_after_cooldown() {
        let mut w = Wounds::new(0);
        assert_eq!(w.hit(REX_BITE), HitOutcome::Wounded);
        assert_eq!(w.state, WoundState::Wounded);
        assert_eq!(w.hurt_timer, 15.5);
        assert!(w.strong_wound);
        // inside the 3.0 s cooldown: ignored
        w.tick(1.0);
        assert_eq!(w.hit(REX_BITE), HitOutcome::Ignored);
        assert_eq!(w.state, WoundState::Wounded);
        w.tick(2.5);
        assert_eq!(w.hit(LIGHT), HitOutcome::Killed);
        assert_eq!(w.state, WoundState::Dead);
    }

    #[test]
    fn grab_flag_kills_from_any_state_even_in_cooldown() {
        let mut w = Wounds::new(0);
        assert_eq!(w.hit(REX_GRAB), HitOutcome::Killed);
        assert_eq!(w.state, WoundState::Dead);
        let mut w = Wounds::new(0);
        w.hit(LIGHT);
        assert_eq!(w.hit(REX_GRAB), HitOutcome::Killed);
    }

    #[test]
    fn wound_expires_into_recovering_not_healthy() {
        let mut w = Wounds::new(2);
        w.hit(REX_BITE); // heavy, 2+ deaths: 10.5 s
        assert_eq!(w.wound_len, 10.5);
        for _ in 0..(11 * 60) {
            w.tick(1.0 / 60.0);
        }
        assert_eq!(w.state, WoundState::Recovering);
        // a Recovering Jack is wounded again (not killed) by the next hit
        assert_eq!(w.hit(LIGHT), HitOutcome::Wounded);
        assert_eq!(w.state, WoundState::Wounded);
        // only Ann cures
        w.heal();
        assert_eq!(w.state, WoundState::Healthy);
    }

    #[test]
    fn death_counter_and_restart_flow() {
        let mut w = Wounds::new(0);
        w.hit(REX_GRAB);
        w.tick(3.9);
        assert!(!w.restart_due());
        w.tick(0.2);
        assert!(w.restart_due());
        assert_eq!(w.commit_death(), 30);
        assert_eq!(w.deaths, 1);
        w.respawn();
        assert_eq!(w.state, WoundState::Healthy);
        assert_eq!(w.deaths, 1);
        // the next heavy wound is shorter (13.5 s)
        w.hit(REX_BITE);
        assert_eq!(w.hurt_timer, 13.5);
        w.map_changed();
        assert_eq!(w.deaths, 0);
    }

    #[test]
    fn hazard_and_cheat() {
        let mut w = Wounds::new(0);
        w.hazard_dwell(0.9);
        assert_eq!(w.state, WoundState::Healthy);
        w.hazard_dwell(1.1);
        assert_eq!(w.state, WoundState::Dead);
        let mut c = Wounds::new(0);
        c.invulnerable = true;
        assert_eq!(c.hit(REX_GRAB), HitOutcome::Ignored);
    }
}
