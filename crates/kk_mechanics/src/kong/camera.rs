//! Kong-level camera mode selection and the "normal" follow mode (ledger K02).
//! Sources: `CM_Kong_Modes@0x761c70` (selector), `CM_Kong_Mode_Normal@0x757030`,
//! `CM_Kong_AutoMode`, `CM_Kong_FightSelector`, `CM_Kong_Mode_{Cut,Walling,Swing_Horiz,...}`.

/// Debug-overlay mode ids (`Kong cam+0x57d`, from the `switch` in `CM_Kong_Modes`). `[C]`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CamMode {
    KamModifier = 0,
    Cut = 1,
    Finish = 3,
    SwingPillar = 4,
    Walling = 5,
    Normal = 6,
    Cheat = 7,
    Test = 8,
    Cine = 9,
}

/// Inputs to the selector, in the order the binary tests them.
#[derive(Clone, Copy, Debug, Default)]
pub struct SelectorInputs {
    pub auto_mode: Option<CamMode>,
    pub fight_selector: Option<CamMode>,
    pub cine: bool,       // cam+0x5d2 != 0
    pub cheat: bool,      // Kong+0x1a9c != 0 (cheat free-cam)
    pub test: bool,       // cam[0] != 0
    pub finish: bool,     // finisher active (cam+0x645 set and object flag 4 clear)
    pub cchamp: bool,     // cam+0x5a0 != 0 (timed override)
    pub cut: bool,        // cam+0x6d != 0 || cam+0x51 > 0
}

/// `CM_Kong_Modes`: auto mode, fight selector, then Cine, Cheat, Test, Finish, CChamp, Cut, Normal.
/// `[C]` order from the nested `if` chain; CChamp has no id in the overlay switch (returned as Normal).
pub fn select(i: &SelectorInputs) -> CamMode {
    if let Some(m) = i.auto_mode {
        return m;
    }
    if let Some(m) = i.fight_selector {
        return m;
    }
    if i.cine {
        CamMode::Cine
    } else if i.cheat {
        CamMode::Cheat
    } else if i.test {
        CamMode::Test
    } else if i.finish {
        CamMode::Finish
    } else if i.cchamp {
        CamMode::Normal
    } else if i.cut {
        CamMode::Cut
    } else {
        CamMode::Normal
    }
}

/// `fn@0x00882200(15.0, 3.0, 7.0, 3.0)` arguments of the normal mode (meaning `[G]`:
/// distance, height, look-ahead, height 2). `[C]` values.
pub const NORMAL_PARAMS: [f32; 4] = [15.0, 3.0, 7.0, 3.0];
/// Look-at height blend target when not in state 0xb. `[C]`
pub const NORMAL_LOOK_HEIGHT: f32 = 3.8;
/// Smoothing rate per second of `cam+0x1264` and friends: `lerp(cur, tgt, clamp(dt*1.0))`. `[C]`
pub const NORMAL_SMOOTH_RATE: f32 = 1.0;
/// Default field written to `cam+0x110`. `[C]`
pub const NORMAL_DEFAULT_110: f32 = 0.2;

/// Clamped exponential-ish blend used all over `CM_Kong_Mode_Normal`:
/// `(1-t)*cur + t*target` with `t = clamp(dt*rate, 0, 1)`.
pub fn blend(cur: f32, target: f32, dt: f32, rate: f32) -> f32 {
    let t = (dt * rate).clamp(0.0, 1.0);
    (1.0 - t) * cur + t * target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_order() {
        let mut i = SelectorInputs::default();
        assert_eq!(select(&i), CamMode::Normal);
        i.cut = true;
        assert_eq!(select(&i), CamMode::Cut);
        i.finish = true;
        assert_eq!(select(&i), CamMode::Finish);
        i.cine = true;
        assert_eq!(select(&i), CamMode::Cine);
        i.fight_selector = Some(CamMode::SwingPillar);
        assert_eq!(select(&i), CamMode::SwingPillar);
        i.auto_mode = Some(CamMode::Walling);
        assert_eq!(select(&i), CamMode::Walling);
        assert_eq!(CamMode::Normal as u8, 6);
    }

    #[test]
    fn blend_clamps() {
        assert_eq!(blend(0.0, 10.0, 0.5, 1.0), 5.0);
        assert_eq!(blend(0.0, 10.0, 5.0, 1.0), 10.0);
        assert_eq!(NORMAL_PARAMS[0], 15.0);
    }
}
