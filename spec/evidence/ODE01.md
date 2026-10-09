# ODE01 Breakable ODE structures (03E gate, corridor wall, entrance lintel)
## Behaviour
Level 03E has three structures built from `LD_03E_ODE_*` rigid-body pieces that the V-Rex smashes in the game:
the courtyard gate (5 blocks), the corridor wall before CHK03 (Base / Top / Mid / Hat rows) and the entrance lintel.
Each structure has a controller GAO `LD_03E_ODE_IA_*` (AI model) and is triggered by `LD_03E_Activate_ODE_*`
(Porte / Arche / pont). Once moving, the pieces hit whatever they touch.
## Data [C]
03E stream `ff001b17` through the loader emulation (`research/pc/keys/tools/lvl/runall.py`, 6399 records):
* pieces: GAO + GRO visual (cb 0x9b2ac0) + RLI (0x95dd50) + packed body data (cb 0x9b9630) + 0x981150;
  gate `LD_03E_ODE_block01..05` (x 44.8-51.3, y 3.6-13.3, z -102.2..-98.9 glTF), corridor wall `Base/Top/Mid/Hat`
  (x 31.5-39.5, z -137..-135.9), lintel `EntreeBase/EntreeHat` (z -66.8..-64.9).
* controllers with sound banks (resource cb 0xa346e0): `LD_ODE_IA_Ponton`, `LD_03E_ODE_Occluder_IA`,
  `LD_03E_ODE_IA_Pont_Fin`; `LD_03E_ODE_IA_Porte` has an AI model and no bank.
* The pieces are what the level draws of the structures (rendering them hidden left an open doorway behind an
  invisible collision box).
## Code [C]
* `ode_exec_gestion_paf_externes@0x6a4250`: every moving piece with a damage value (`+0x53c`) sends a hit message
  (flags 0x100000, that damage, direction piece -> target) to objects it touches, once per target unless the damage
  grew (`+0x5c0`/`+0x604` lists).
* `ode_exec_gestion_son@0x6a4610`: rolling loop = bank slot 5 (parameter 0x1c = smoothed speed ratio); impact
  sounds = bank slot 10 + contact class (`fn@0x431ea0` -> 1..4) when a piece's velocity changes, capped by
  `+0xa30` per second.
## Rust
`kk_fps::breakable` (`world::BREAKABLES`), `FightEvent::KongSwing` from `kong::fight`. Not ported: the piece
damage messages and the impact / rolling sounds (bank slots of the controllers; the gate's controller has no bank).
## Gaps
ODE body parameters (mass, friction, restitution: the cb 0x9b9630 data is packed and not decoded), piece damage
values (instance data), the trigger logic of `LD_03E_Activate_ODE_*`. The debris motion is [G].
