//! Gameplay events shared by the sound, effects and test systems.

use bevy::prelude::*;

#[derive(Event, Clone, Debug)]
pub enum GunEvent {
    /// A shot left weapon `w` (index into spec::WEAPONS). `muzzle` is world space.
    Fired { w: usize, muzzle: Vec3, dir: Vec3 },
    /// One pellet/bullet hit something.
    Impact { pos: Vec3, normal: Vec3, rex: bool, damage: f32, dist: f32 },
    /// Trigger pulled with nothing left to fire.
    Empty { w: usize },
    /// Reload cycle started (shotgun/sniper: once per cycle).
    ReloadStart { w: usize, first: bool },
    /// Rounds were transferred from reserve to the magazine.
    ReloadCommit { w: usize, rounds: u32 },
    /// Reload finished.
    ReloadEnd { w: usize },
    /// Weapon swap started, switching to `to`.
    Swap { to: usize },
}

#[derive(Event, Clone, Debug)]
pub enum RexEvent {
    Roar { alert: bool, pos: Vec3 },
    Footstep { pos: Vec3, strong: bool },
    BiteStart { pos: Vec3 },
    BiteHit { pos: Vec3 },
    Flinch { pos: Vec3 },
    Eat { pos: Vec3 },
    Breath { pos: Vec3 },
    Died { pos: Vec3 },
}

#[derive(Event, Clone, Debug)]
pub enum JackEvent {
    Footstep,
    Wounded,
    Died,
}

pub struct EventsPlugin;

impl Plugin for EventsPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<GunEvent>().add_event::<RexEvent>().add_event::<JackEvent>();
    }
}
