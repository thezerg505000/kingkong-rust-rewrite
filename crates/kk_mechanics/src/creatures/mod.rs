//! Creature AI (ledger groups CREATURE_AI, ENEMY_ARCHETYPES): one module per species.
//!
//! * [`raptor`] - Venatosaurus (`PNJ_Raptor_*@0x83xxxx-0x87xxxx`, species byte 0xe). The same engine class also
//!   drives the Jack-level V-Rex (0x10) and the herd compies (0x16); only 0xe is ported here.
//!   Evidence: `spec/evidence/X02.md` (+ `A01..A15` raptor views).
//! * [`vrex_jack`] - the Jack-level V-Rex (species 0x10 of the same class): perception, target chain, bite/kill outcome.
//!   Evidence: `spec/evidence/X04.md` (+ `B01`, `G17`, `A12`, `A14`, `A17`).

pub mod raptor;

pub mod vrex_jack;
