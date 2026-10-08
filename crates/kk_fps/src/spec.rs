//! Slice-side constants and presentation tables for the playable Jack-vs-V-Rex slice.
//!
//! The game LOGIC lives in `kk_mechanics` (the product of the static analysis; every number there
//! carries `[C]`/`[L]`/`[G]` and a function@address). This file keeps only:
//!   * re-exports of the recovered Jack constants the slice reads (`kk_mechanics::jack`,
//!     `kk_mechanics::wounds`, `kk_mechanics::creatures::vrex_jack`),
//!   * the weapon PRESENTATION table (glb, arm clips, kick, spread, reload frame) plus accessors
//!     into `kk_mechanics::weapons` (clip, range, damage bands, pellets, shot timers),
//!   * slice-only debug numbers and Rex animation clip labels (`[G]`, no recovered data).
//!
//! Where each moved item went (evidence docs in `spec/evidence/`):
//!   * weapon table / damage band / reload / shotgun pattern / fire timing -> `kk_mechanics::weapons` (G02, G03, G10, G11)
//!   * wound model, hit flags, restart timers                              -> `kk_mechanics::wounds` (H01..H04)
//!   * walk/run/crouch speed, look, eye height, FOV                        -> `kk_mechanics::jack` (J*, C*)
//!   * Jack-level V-Rex perception, gaits, bite/grab                       -> `kk_mechanics::creatures::vrex_jack` (X04)

pub use kk_mechanics::jack::{EYE_STAND, FOV_DEFAULT, FOV_SNIPER};
pub use kk_mechanics::wounds::DEATH_SEQUENCE_S;

/// Jade animation tick. [G] - taken from the PS2 audit (60 Hz tick base), not confirmed on PC.
pub const ANIM_HZ: f32 = 60.0;

/// Scope FOV (name kept for the slice's HUD / batch code).
pub const FOV_SNIPER_AIM: f32 = FOV_SNIPER;

/// Convert the game's (assumed 4:3 horizontal) FOV into Bevy's vertical FOV.
pub fn vertical_fov(fov_h_4x3: f32) -> f32 {
    2.0 * ((fov_h_4x3 * 0.5).tan() * 0.75).atan()
}

// ---------------------------------------------------------------------------
// Weapons: presentation table. Numbers come from kk_mechanics::weapons.
// ---------------------------------------------------------------------------
use kk_mechanics::weapons as mech;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponId {
    Colt = 1,
    TommyGun = 2,
    Shotgun = 3,
    SniperRifle = 4,
}

pub struct WeaponDef {
    pub id: WeaponId,
    pub name: &'static str,
    pub glb: &'static str,
    /// starting reserve ammo [G] (reserve max not recovered)
    pub reserve: u32,
    pub spread_deg: f32,
    /// reload: rounds are transferred when the reload action reaches this frame [C Colt 25, Tommy 90; others G]
    pub reload_commit_frame: f32,
    /// Arm clip labels (prefix before "__arms_NNN" in the glb). [L]
    pub clip_idle: &'static str,
    pub clip_aim: &'static str,
    /// fire clip played while aiming ("" = none; hip fire uses procedural recoil)
    pub clip_fire: &'static str,
    pub clip_reload: &'static str,
    /// view kick per shot, radians [G] (no recoil code recovered yet)
    pub kick: f32,
}

impl WeaponDef {
    /// The recovered Univers weapon row (clip, range, R1/R2, damage, pellets, timers) [C].
    pub fn mech(&self) -> &'static mech::WeaponDef {
        mech::def(match self.id {
            WeaponId::Colt => mech::WeaponId::Colt,
            WeaponId::TommyGun => mech::WeaponId::TommyGun,
            WeaponId::Shotgun => mech::WeaponId::Shotgun,
            WeaponId::SniperRifle => mech::WeaponId::SniperRifle,
        })
    }
    pub fn clip(&self) -> u32 {
        self.mech().clip
    }
    pub fn range(&self) -> f32 {
        self.mech().range
    }
    pub fn pellets(&self) -> u32 {
        self.mech().pellets
    }
    /// Value stored in Jack's shot timer after each shot (G02: 0.4 / 0.2 / 0.5 / 0.4) [C value, L meaning].
    pub fn shot_timer(&self) -> f32 {
        self.mech().shot_timer
    }
    /// Auto-fire interval (Tommy 0.1; 0 = single shot per trigger pull) [C].
    pub fn auto_interval(&self) -> f32 {
        self.mech().auto_interval
    }
    #[allow(dead_code)]
    pub fn automatic(&self) -> bool {
        self.auto_interval() > 0.0
    }
    /// Reload loads one round per cycle (shotgun only, H_exec_loading_weapon) [C].
    pub fn one_at_a_time(&self) -> bool {
        self.id == WeaponId::Shotgun
    }
}

pub const WEAPONS: [WeaponDef; 4] = [
    WeaponDef {
        id: WeaponId::Colt,
        name: "Colt (Luger model)",
        glb: "jack_fps_luger.glb",
        reserve: 32,
        spread_deg: 0.0,
        reload_commit_frame: 25.0,
        // One-handed low-right pistol carry, matching the original's hip view of the Luger
        // (reference screenshot "Jack aiming the Luger at a V-Rex"); aimed pose for ADS. [L]
        clip_idle: "idle_c14",
        clip_aim: "idle_short_c23",
        clip_fire: "fire_c23",
        // short left-hand reload keeps the pistol in frame; its 35 frames contain the
        // recovered commit at frame 25 [L]
        clip_reload: "reload_l_short_c36",
        kick: 0.012,
    },
    WeaponDef {
        id: WeaponId::TommyGun,
        name: "Tommy Gun",
        glb: "jack_fps_tommygun.glb",
        reserve: 150,
        spread_deg: 1.5,
        reload_commit_frame: 90.0,
        clip_idle: "idle_c14",
        clip_aim: "idle_short_c25",
        clip_fire: "",
        clip_reload: "reload_l_c43",
        kick: 0.006,
    },
    WeaponDef {
        id: WeaponId::Shotgun,
        name: "Shotgun",
        glb: "jack_fps_shotgun.glb",
        reserve: 20,
        spread_deg: 10.0,
        reload_commit_frame: 20.0,
        clip_idle: "idle_c16",
        clip_aim: "idle_short_c24",
        clip_fire: "fire_c24",
        clip_reload: "reload_l_short_c36",
        kick: 0.03,
    },
    WeaponDef {
        id: WeaponId::SniperRifle,
        name: "Sniper Rifle",
        glb: "jack_fps_sniperrifle.glb",
        reserve: 20,
        spread_deg: 0.0,
        reload_commit_frame: 20.0,
        clip_idle: "idle_c17",
        clip_aim: "idle_short_c25",
        clip_fire: "",
        // The sniper transfers min(clip - mag, reserve) in ONE cycle (G03: only the shotgun is
        // limited to one round per cycle); the short left-hand clip shows the rifle diagonal
        // with the hand at the bolt [L]
        clip_reload: "reload_l_short_c40",
        kick: 0.02,
    },
];

/// Damage band by distance: `kk_mechanics::weapons::damage_at_distance_sq` (G02), truncated to int.
pub fn damage_at(w: &WeaponDef, dist: f32) -> f32 {
    mech::damage_at_distance_sq(w.mech(), dist * dist, false) as f32
}

// ---------------------------------------------------------------------------
// V-Rex on Jack levels: logic constants live in kk_mechanics::creatures::vrex_jack (X04).
// ---------------------------------------------------------------------------
pub use kk_mechanics::creatures::vrex_jack as vrex;
pub const REX_HP: f32 = vrex::HP_MAX; // [C]
/// HP used when "mortal mode" (F2) is on so the encounter can be finished. Debug cheat [G]:
/// the real rex cannot be hurt by guns (X04).
pub const REX_HP_MORTAL: f32 = 250.0;
/// Chase turn rate, rad/s [G]. Only the HESITE turn (k = 4*dt) is recovered.
pub const REX_TURN_RATE: f32 = 1.4;

/// Rex clip labels (prefix before "__rex_NNN"). The gait speeds are recovered (walk 2.2 / run_c 14.0 /
/// run_b 17.06 m/s, X04); the intermediate trot clips are used only while accelerating at 1.5 m/s^2 [G].
pub const REX_IDLE: &str = "idle";
pub const REX_WALK: &str = "walk_b";
pub const REX_TROT: &str = "trot";
pub const REX_TROT_FAST: &str = "trot_b";
pub const REX_RUN: &str = "run_c";
pub const REX_ROAR: &str = "roar";
pub const REX_BITE: &str = "bite_b";
pub const REX_DEATH: &str = "other__rex_056";
