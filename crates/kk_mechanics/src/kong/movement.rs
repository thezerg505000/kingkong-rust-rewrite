//! Kong's traversal: stick filtering, jump reach, hotspot windows, obstacle bands, release
//! reaches, input latches, interactive kits (ledger K01-K15).
//!
//! Sources (all `KingKong8.exe`): `GG_Exec_Joy@0x695030` (stick), `k_exec_joy@0x8ae5f0` (buttons),
//! `k_ETAT_jump@0x8c6690`, `k_exec_check_hotspot@0x8d6b60`, `k_exec_check_obstacle@0x8d42e0`,
//! `fn@0x008851a0` (obstacle response), `k_ETAT_walling@0x8a5a20`, `k_ETAT_SwingArch@0x8d7f60`,
//! `k_ETAT_SwingPillar@0x8d9a40`, `k_ETAT_colonne_exec@0x8eb890`, `KIGO_*`, `KIMO_*`.
//! Locomotion speed itself is animation driven (anim id `Kong+0x112c` picks the handler in
//! `k_ETAT_main@0x89a120`), so there is no walk-speed constant; see `spec/evidence/K01.md`.

/// Kong AI action ids stored at `Kong+0x5c` (`[C]` from each handler's own `K[0x5c] = id`).
pub mod state {
    pub const MAIN: u32 = 0xc8; // k_ETAT_main
    pub const GRAB_ANN: u32 = 0x15f; // k_ETAT_grab_ANN
    pub const ACCROCH_MUR: u32 = 0x385; // k_ETAT_accroch_mur
    pub const CLIMB_REMONTE_RAPIDE: u32 = 0x389;
    pub const SWING_ARCH: u32 = 0x3b6;
    pub const SWING_PILLAR: u32 = 0x3b7;
    pub const WALLING: u32 = 1000; // k_ETAT_walling
    pub const JUMP: u32 = 0x3e9; // k_ETAT_jump
}

/// Animation ids named in the handlers (`[C]`).
pub mod anim {
    pub const JUMP_STAND: u32 = 0x3c;
    pub const JUMP_RUN: u32 = 0x3d;
    pub const LAND_SOFT: u32 = 0x3a;
    pub const LAND_HARD: u32 = 0x41;
    pub const LAND_FALL: u32 = 0x3f;
    pub const FALL_LOOP: u32 = 0x40;
    pub const SWING_ARCH_GRAB: u32 = 0x8f;
    pub const SWING_ARCH_SWING: u32 = 0x90;
    pub const SWING_PILLAR_GRAB: u32 = 0x91;
    pub const SWING_PILLAR_SWING: u32 = 0x92;
    pub const CLIMB_UP_QUICK: u32 = 0x4d;
    pub const CLIMB_OVER: u32 = 0x104; // fn@0x00887160(.., 0x104), combat phase 0xb
    pub const COLUMN_IDLE: u32 = 0x105;
}

// ---- K01: stick ---------------------------------------------------------------------------
/// Radial dead zone of the filtered stick (`GG_Exec_Joy@0x695030`, `0.25 <= |s|`). `[C]`
pub const STICK_DEADZONE: f32 = 0.25;
/// Stick travel after the dead zone that maps to full speed (`local_10 = (|s|-0.25)/0.675`). `[C]`
pub const STICK_RANGE: f32 = 0.675;
/// Magnitudes above this snap to exactly 1.0 (`0.99 < K[0x68]`). `[C]`
pub const STICK_SNAP: f32 = 0.99;
/// A latched direction is released when the new camera-relative direction differs by dot < 0.9. `[C]`
pub const LATCH_RELEASE_DOT: f32 = 0.9;
/// ... or when this time has passed with the secondary button held (`fn@0x0043c690(.., 0.5)`). `[C]`
pub const LATCH_RELEASE_SECONDS: f32 = 0.5;
/// Camera turns by more than 45 deg (dot < cos 45) arm the latch. `[C]`
pub const LATCH_ARM_DOT: f32 = 0.707_106_77;
/// `Kong+0x1200` (horizontal speed) above this counts as "moving" (jump/hotspot decisions). `[C]`
pub const MOVING_SPEED: f32 = 0.06;

/// Stick magnitude (0..~1.4 raw) to Kong's analogue input `Kong+0x68` in 0..1.
pub fn stick_magnitude(raw: f32) -> f32 {
    if raw < STICK_DEADZONE {
        return 0.0;
    }
    let m = ((raw - STICK_DEADZONE).min(STICK_RANGE)) / STICK_RANGE;
    if m > STICK_SNAP { 1.0 } else { m }
}

/// Camera-relative walking direction on the ground plane. `right`/`fwd` are the camera axes
/// projected on the ground; stick `(sx, sy)`: x = right, y = forward. Returns a unit vector
/// (x, y) or `None` inside the dead zone. (`[L]` sign convention: the binary negates the raw
/// pad vector and rotates it by the camera matrix, then zeroes the vertical component.)
pub fn camera_relative_dir(stick: (f32, f32), right: (f32, f32), fwd: (f32, f32)) -> Option<(f32, f32)> {
    let raw = (stick.0 * stick.0 + stick.1 * stick.1).sqrt();
    if raw < STICK_DEADZONE {
        return None;
    }
    let x = stick.0 * right.0 + stick.1 * fwd.0;
    let y = stick.0 * right.1 + stick.1 * fwd.1;
    let l = (x * x + y * y).sqrt();
    if l == 0.0 { None } else { Some((x / l, y / l)) }
}

/// Direction latch against fast camera cuts (`Kong+0x74`, `GG_Exec_Joy`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DirLatch {
    pub active: bool,
    pub dir: (f32, f32),
    pub age: f32,
}

impl DirLatch {
    /// Arm when the camera forward axis jumped (dot with previous < cos 45 deg).
    pub fn arm_if_camera_cut(&mut self, cam_dot: f32, current_dir: (f32, f32)) {
        if !self.active && cam_dot < LATCH_ARM_DOT {
            *self = DirLatch { active: true, dir: current_dir, age: 0.0 };
        }
    }
    /// Returns the direction to use this frame and updates the latch.
    pub fn resolve(&mut self, new_dir: (f32, f32), secondary_held: bool, dt: f32) -> (f32, f32) {
        if !self.active {
            return new_dir;
        }
        self.age += dt;
        let dot = new_dir.0 * self.dir.0 + new_dir.1 * self.dir.1;
        if dot < LATCH_RELEASE_DOT || (secondary_held && self.age >= LATCH_RELEASE_SECONDS) {
            self.active = false;
            new_dir
        } else {
            self.dir
        }
    }
}

// ---- input slots --------------------------------------------------------------------------
/// Logical pad ids passed to `Pad_ButtonPressed/Held` in `k_exec_joy@0x8ae5f0` and the Kong
/// flag each sets (`[C]`). The physical names of ids 0-4 are `[G]` (jump = id 2: it drives the
/// release/leap tests `K[0x8c]` in walling, swing and column states).
pub const PAD_JUMP: u8 = 2;
/// Kong offset set by each pressed pad id: (pad id, offset). Id 2 also sets `+0x70`; ids 3 and 0
/// also set `+0xa4`.
pub const PAD_PRESSED_FLAG: [(u8, u32); 4] = [(1, 0x7c), (2, 0x8c), (0, 0x90), (3, 0x78)];
/// Held: pad id 2 sets `Kong+0x8c8` ("run/long jump" hold) - selects 16 m reach.
pub const PAD_HELD_RUN_FLAG: u32 = 0x8c8;
/// Edge-latch slots written by `fn@0x00884810`: (Kong flag, edge id, latch offset). `[C]`
pub const EDGE_LATCH: [(u32, u8, u32); 4] = [(0x78, 2, 0x1ad4), (0x8c, 1, 0x1acc), (0x90, 3, 0x1ad0), (0x7c, 4, 0x1ad8)];

// ---- K04: jump ----------------------------------------------------------------------------
/// Jump reach in metres (`Kong+0x980`, set at `k_ETAT_jump` entry). `[C]`
pub const JUMP_REACH_STAND: f32 = 6.0;
pub const JUMP_REACH_STICK: f32 = 10.0;
pub const JUMP_REACH_HOLD: f32 = 16.0;
/// Reach override (`Kong+0x9bc`, non-zero) is used once then cleared. Values written by the
/// release states: swing arch 8, swing pillar 10 / 13 (13 with `Kong+0xa00 = 1.5`), wall leap 18
/// (10 when the dot test fails), column 2 / 6, wall-hang 1. `[C]`
pub const REACH_SWING_ARCH: f32 = 8.0;
pub const REACH_SWING_PILLAR: f32 = 10.0;
pub const REACH_SWING_PILLAR_FAR: f32 = 13.0;
pub const REACH_WALL_LEAP: f32 = 18.0;
pub const REACH_WALL_LEAP_SHORT: f32 = 10.0;
pub const REACH_COLUMN_NEAR: f32 = 2.0;
pub const REACH_COLUMN_FAR: f32 = 6.0;

/// Reach at jump start. `override_reach` is `Kong+0x9bc` (0 = none).
pub fn jump_reach(override_reach: f32, run_held: bool, stick_active: bool) -> f32 {
    if override_reach.abs() > 1e-6 {
        override_reach
    } else if !run_held {
        if stick_active { JUMP_REACH_STICK } else { JUMP_REACH_STAND }
    } else {
        JUMP_REACH_HOLD
    }
}

// ---- K04/K06: hotspots --------------------------------------------------------------------
/// Hotspot type flag bits tested in `k_exec_check_hotspot@0x8d6b60` (`[C]`; meanings `[L]`).
pub const HS_JUMP_TARGET: u32 = 0x4;
pub const HS_CUSTOM_RADIUS: u32 = 0x2;
pub const HS_FACING_ONLY: u32 = 0x8;
pub const HS_LEDGE: u32 = 0x800;
pub const HS_ONE_SIDED: u32 = 0x2080;

/// Base search radius before squaring (`local_14`). `[C]`
pub fn hotspot_base_radius(kong_has_current: bool, flags: u32) -> f32 {
    if !kong_has_current {
        25.0
    } else if flags == 4 || flags == 0x80 {
        30.0
    } else {
        20.0
    }
}

/// Final reach: base + `max(0, -dz * 0.25)` (higher targets reach further), or the hotspot's own
/// radius when `HS_CUSTOM_RADIUS` is set and non-zero. Returned already squared.
pub fn hotspot_reach_sq(base: f32, dz: f32, flags: u32, custom: f32) -> f32 {
    let mut r = base + (-dz * 0.25).max(0.0);
    if flags & HS_CUSTOM_RADIUS != 0 && custom != 0.0 {
        r = custom;
    }
    r * r
}

/// Window test (`dz` = height of target relative to Kong as the binary signs it, `d2` = squared
/// horizontal distance). `jump_ok`/`grab_ok` are `Kong+0x9d8` / `Kong+0x9d4`.
pub fn hotspot_in_window(flags: u32, dz: f32, d2: f32, reach_sq: f32, jump_ok: bool, grab_ok: bool) -> bool {
    (flags & HS_JUMP_TARGET != 0 && jump_ok && dz < -2.0 && dz > -30.0 && d2 > 9.0 && d2 < reach_sq)
        || (grab_ok && flags & HS_LEDGE != 0 && dz < 10.0 && dz > -17.0 && d2 > 36.0 && d2 < reach_sq)
        || (grab_ok && flags & 0x804 == 0 && dz < 15.0 && dz > -17.0 && d2 > 36.0 && d2 < reach_sq)
}
/// Minimum cosine between Kong's heading and the hotspot direction. `[C]`
pub const HOTSPOT_MIN_FACING: f32 = 0.5;
/// Cone radius handed to the best-candidate picker `fn@0x004321c0`. `[C]`
pub const HOTSPOT_PICK_RADIUS: f32 = 20.0;

// ---- K06: obstacles -----------------------------------------------------------------------
/// Obstacle heights (`Kong+0x179c`): <= 1.9 is ignored, (1.9, 11] is vaulted/climbed over,
/// above 11 needs a hotspot (climb-up anim 0x104). `[C]` (`k_exec_check_obstacle`, `fn@0x008851a0`)
pub const OBSTACLE_MIN: f32 = 1.9;
pub const OBSTACLE_VAULT_MAX: f32 = 11.0;
/// Wall must be steeper than 45 deg: |normal.z| < cos 45. `[C]`
pub const OBSTACLE_MAX_NORMAL_Z: f32 = 0.707_106_77;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Obstacle {
    None,
    Vault,
    NeedsHotspot,
}

pub fn classify_obstacle(height: f32) -> Obstacle {
    if height <= OBSTACLE_MIN {
        Obstacle::None
    } else if height <= OBSTACLE_VAULT_MAX {
        Obstacle::Vault
    } else {
        Obstacle::NeedsHotspot
    }
}

// ---- K09/K10: swing -----------------------------------------------------------------------
/// Swing entry vertical velocity vector `Vec3(0, -0.5, 2.4)` and speed `2.8` scale `[C]`
/// (`k_ETAT_jump`, `k_ETAT_accroch_mur`: `Vec3_Make(0,-0.5,0x4019999a)`).
pub const SWING_ENTRY_VEL: (f32, f32, f32) = (0.0, -0.5, 2.4);
/// Pillar swing entry speed clamp passed to `fn@0x0041c8e0(.., 15.0)`. `[C]`
pub const SWING_PILLAR_SPEED: f32 = 15.0;

// ---- K13/K14: interactive kits ------------------------------------------------------------
/// `KIMO_InitFromKit@0x4c9af0`: per kit-id the five trailing floats given to `fn@0x006aa540`
/// (meaning `[G]`: reach, secondary reach, min, mid, max timings). `[C]` values.
pub const KIMO_KITS: [[f32; 5]; 7] = [
    [4.0, 2.0, 0.1, 0.25, 0.35],
    [20.0, 0.0, 0.1, 0.5, 1.0],
    [8.0, 4.0, 0.1, 0.5, 1.0],
    [8.0, 4.0, 0.1, 0.5, 1.0],
    [5.0, 3.0, 0.0, 0.1, 1.5],
    [5.0, 3.0, 0.0, 0.1, 1.5],
    [8.0, 5.0, 0.0, 0.1, 1.0],
];
/// `KIGO_init@0x671970`: `local_8[0x48] = 81.0` (grab distance squared = 9 m `[G]`).
pub const KIGO_GRAB_PARAM: f32 = 81.0;

/// `k_ETAT_main@0x89a120` per-frame order (K15), first match returns [C: line order of `etat_main.c`, with the guarding fields]:
/// every entry that "goes to a state" does `AI_GotoStateQueued` (or `AI_GotoStateNow` + return 5 when the AI slot is 2).
pub const MAIN_PRIORITY: [&str; 12] = [
    "fn@0x0043f170 rider-collision step when Kong+0x64 (previous frame flag) is set",
    "k_exec_check_obstacle (climb over / hotspot) - its result is returned after the Time_Now stamp (Kong+0x1df4)",
    "fn@0x007dd3f0 -> Kong+0x1b38 != 0: leave to the queued state (grab-held / forced entry)",
    "refresh the grab target handle Kong+0x358[Kong+0x368] (Handle_IsValid, Msg_FindGrabTarget)",
    "k_AutoJumpDecision when Kong+0x8c8 || +0x74 || +0x13e4 (jump / auto jump state)",
    "k_exec_detect_paf (damage reaction) unless gait state 0x42/0x43/0x44",
    "k_CheckForcedState when Kong+0xc6c, then Kong+0x1a78 != 0 -> forced state",
    "k_StandingHotspotJump",
    "k_LatchButtonEdges (presses only latch while the matching input-window flag is set: 0x20 jump/roll, 0x40 special, 0x80 attack/cancel)",
    "k_exec_get_combat_phase switch (Kong+0x1adc): per phase, end-of-animation or the 0x100 cancel window + a latched press picks the next move; holds the grab-target check (Msg_FindGrabTarget kind 0xc) before every k_exec_start_*",
    "obstacle / climb handling in the idle phase: k_exec_check_colonnes, k_StartClimbOver (Kong+0x1af4), k_CheckHotspotWrapper, k_ObstacleResponse",
    "gait switch on Kong+0x112c: locomotion anim ids (0 idle, 3 idle with a rider, 4 run, 5 walk) via ANIM_Play, speed = ANIM_GetBaseSpeed",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_curve() {
        assert_eq!(stick_magnitude(0.2), 0.0);
        assert!((stick_magnitude(0.25 + 0.3375) - 0.5).abs() < 1e-5);
        assert_eq!(stick_magnitude(0.925), 1.0);
        assert_eq!(stick_magnitude(1.4), 1.0);
    }

    #[test]
    fn camera_relative() {
        let d = camera_relative_dir((0.0, 1.0), (1.0, 0.0), (0.0, 1.0)).unwrap();
        assert_eq!(d, (0.0, 1.0));
        let d = camera_relative_dir((1.0, 0.0), (0.0, -1.0), (1.0, 0.0)).unwrap();
        assert!((d.0).abs() < 1e-6 && (d.1 + 1.0).abs() < 1e-6);
        assert!(camera_relative_dir((0.1, 0.1), (1.0, 0.0), (0.0, 1.0)).is_none());
    }

    #[test]
    fn latch_holds_until_stick_moves() {
        let mut l = DirLatch::default();
        l.arm_if_camera_cut(0.2, (0.0, 1.0));
        assert!(l.active);
        assert_eq!(l.resolve((0.0, 1.0), false, 0.1), (0.0, 1.0));
        // direction changed by > 25 deg -> released
        let n = (0.6, 0.8);
        assert_eq!(l.resolve(n, false, 0.1), n);
        assert!(!l.active);
        // timeout path
        let mut l = DirLatch { active: true, dir: (0.0, 1.0), age: 0.45 };
        assert_eq!(l.resolve((0.0, 1.0), true, 0.1), (0.0, 1.0));
    }

    #[test]
    fn jump_reach_values() {
        assert_eq!(jump_reach(0.0, false, false), 6.0);
        assert_eq!(jump_reach(0.0, false, true), 10.0);
        assert_eq!(jump_reach(0.0, true, true), 16.0);
        assert_eq!(jump_reach(REACH_WALL_LEAP, true, true), 18.0);
    }

    #[test]
    fn hotspot_windows() {
        let base = hotspot_base_radius(true, 4);
        assert_eq!(base, 30.0);
        let r2 = hotspot_reach_sq(base, -8.0, 4, 0.0); // 30 + 2 = 32
        assert!((r2 - 1024.0).abs() < 1e-3);
        assert!(hotspot_in_window(4, -5.0, 100.0, r2, true, false));
        assert!(!hotspot_in_window(4, -1.0, 100.0, r2, true, false)); // not far enough below
        assert!(!hotspot_in_window(4, -5.0, 8.0, r2, true, false)); // inside 3 m
        assert!(hotspot_in_window(0, 0.0, 49.0, 400.0, false, true));
        assert!(!hotspot_in_window(0, 16.0, 49.0, 400.0, false, true));
        assert_eq!(hotspot_base_radius(false, 4), 25.0);
        assert_eq!(hotspot_base_radius(true, 0x80), 30.0);
        assert_eq!(hotspot_base_radius(true, 1), 20.0);
        assert_eq!(hotspot_reach_sq(20.0, 0.0, HS_CUSTOM_RADIUS, 7.0), 49.0);
    }

    #[test]
    fn obstacle_bands() {
        assert_eq!(classify_obstacle(1.9), Obstacle::None);
        assert_eq!(classify_obstacle(2.0), Obstacle::Vault);
        assert_eq!(classify_obstacle(11.0), Obstacle::Vault);
        assert_eq!(classify_obstacle(11.1), Obstacle::NeedsHotspot);
    }

    #[test]
    fn kit_table() {
        assert_eq!(KIMO_KITS[1][0], 20.0);
        assert_eq!(KIMO_KITS[0][4], 0.35);
        assert_eq!(state::JUMP, 1001);
        assert_eq!(state::WALLING, 1000);
    }
}
