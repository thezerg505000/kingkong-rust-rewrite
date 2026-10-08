//! Kong-level mechanics (ledger groups KONG_*): the V-Rex fight (B02, X04, KC10-KC13) and
//! Kong's fury/rage (KF01-KF07).
//!
//! * [`vrex`]  - `KT_*` (Kong-level T-Rex) state machine, life gauge, hit rules, the jaw-break
//!   finisher mash. Source: `KT_ETAT_*@0x49xxxx-0x4cxxxx`, `KT_TRACK_reflex@0x49a430`.
//! * [`fury`]  - Kong's fury timer, activation window, damage/speed/resilience effects.
//!   Source: `k_ETAT_main@0x89a120`, `fn@0x00883430` (start), `fn@0x00883700` (charge/extend),
//!   `k_exec_fury@0x8b1d00`, `k_ETAT_paf@0x8c24a0`, `k_reflex@0x8a4360`.
//!
//! Naming note: `KR_*` is the Kong-level **Raptor** (Venatosaurus), *not* rage; `KBC_*`/`BC_*`
//! is the Kong-level big bat; fury is `k_exec_fury` plus a timer in Kong's own struct
//! (`Kong+0x7c4`). See `spec/evidence/B02.md` and `docs/ENGINE_MAP.md`.

pub mod ann;
pub mod fury;
pub mod vrex;
pub mod camera;
pub mod movement;
pub mod combat;
pub mod fight;
pub mod anims;
pub mod ai;
