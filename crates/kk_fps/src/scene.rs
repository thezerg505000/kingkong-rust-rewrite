//! Which level is loaded: 03E (default), the flat test area (`testarea.rs`) or a Kong swamp level:
//! * 05C "Kong vs first T-Rex" marsh (`KK_SCENE=swamp05c`, and the default of the `b10*` batches when the
//!   level was built): the level of the reference clip (`references/kingkong.mp4`);
//! * 07D "Kong Saves Ann" (`KK_SCENE=swamp07d`).
//! Both use the swamp presentation in `swamp.rs`.

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Swamp {
    L05C,
    L07D,
}

/// The swamp level in use, if any.
pub fn swamp_level() -> Option<Swamp> {
    static A: OnceLock<Option<Swamp>> = OnceLock::new();
    *A.get_or_init(|| {
        let scene = std::env::var("KK_SCENE").unwrap_or_default();
        let batch = std::env::var("KK_BATCH").unwrap_or_default();
        match scene.as_str() {
            "swamp07d" => return Some(Swamp::L07D),
            "swamp05c" => return Some(Swamp::L05C),
            _ => {}
        }
        if batch.starts_with("b10") {
            let has05 = crate::asset_dir().join("level05c/level05c.glb").exists();
            return Some(if has05 { Swamp::L05C } else { Swamp::L07D });
        }
        None
    })
}

/// A Kong swamp level is the active scene.
pub fn swamp() -> bool {
    swamp_level().is_some()
}

pub fn marsh05c() -> bool {
    swamp_level() == Some(Swamp::L05C)
}

pub fn level_glb() -> &'static str {
    match swamp_level() {
        Some(Swamp::L05C) => "level05c/level05c.glb",
        Some(Swamp::L07D) => "level07d/level07d.glb",
        None => crate::anim::LEVEL_GLB,
    }
}

pub fn level_collision() -> &'static str {
    match swamp_level() {
        Some(Swamp::L05C) => "level05c/level05c_collision.json",
        Some(Swamp::L07D) => "level07d/level07d_collision.json",
        None => crate::anim::LEVEL_COLLISION,
    }
}
