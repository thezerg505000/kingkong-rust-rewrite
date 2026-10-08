//! Gameplay mechanics of Peter Jackson's King Kong (2005), ported from the game's own code.
//!
//! Pure Rust, deterministic, no engine dependency: the presentation layer (`kk_fps`, later a
//! Kong crate) calls into here. One module per ledger group (`spec/mechanics.yaml`); every
//! item cites the original function (`Name@0xaddr` in KingKong8.exe) and tags each number
//! `[C]` (read from code/data), `[L]` (inferred) or `[G]` (guess). See `docs/PLAYBOOK.md`.
//!
//! Module map (ledger group -> module):
//! * `JACK_GUNPLAY` -> [`weapons`]
//! * `SURVIVAL` -> [`wounds`]
//! * `JACK_MOVEMENT`, `JACK_CAMERA` -> [`jack`] (movement, look, eye, FOV ported)
//! * `JACK_SPEARS` -> [`spears`]
//! * `CREATURE_AI`, `ENEMY_ARCHETYPES` -> `creatures::<species>` (planned; raptor first)
//! * `NPC_COMPANIONS`, `ENVIRONMENT_INTERACTION` (doors, levers, crates), `INVENTORY` (lever, swaps) -> [`companions`]
//! * `PROGRESSION`, `SCORING`, `UNLOCKS`, `AMMO_REPORT`, `MUSIC` (P*, I11, G07, W07) -> [`progression`]
//! * `KONG_*` -> [`kong`] (V-Rex state machine `kong::vrex`, fury `kong::fury`; rest planned)

pub mod creatures;
pub mod environment;
pub mod companions;
pub mod jack;
pub mod kong;
pub mod progression;
pub mod spears;
pub mod weapons;
pub mod wounds;

/// Confidence tag for a recovered value. Kept as data so tools can list what is still a guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    /// read directly from code or game data
    C,
    /// logical inference from code structure
    L,
    /// guess / tuned so the slice stays playable
    G,
}
