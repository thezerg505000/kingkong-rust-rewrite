# Peter Jackson's King Kong (2005): complete gameplay mechanics specification

Reference build: PlayStation 2 (SLUS-21311) · Developer: Ubisoft Montpellier · Genre: cinematic
survival action-adventure. Code is read from the PC Gamer's Edition (`KingKong8.exe`), which
compiles the same AI scripts (see `docs/ENGINE_MAP.md`).

This document is the user-supplied requirements catalogue. **The per-mechanic tables with IDs
live in `spec/mechanics.yaml`** (240 mechanics in 16 groups) and the generated
`spec/STATUS.md`; this file keeps the narrative rules, tables and priorities. Each mechanic ID
(`J01`, `G11`, `KC11`, …) is the key used by the ledger, the evidence docs
(`spec/evidence/<ID>.md`), Rust doc comments and tests.

Accuracy convention:
- **Confirmed**: supported by the original manual, contemporary reviews or walkthroughs.
- **Behavioural description**: how to implement an observed mechanic, without claiming to
  know the original code.
- **Requires verification**: exact numbers, collision rules, animation timings, internal
  algorithms. These are what the binary work recovers.

Every mechanical claim carries one of: `CONFIRMED_FROM_GAMEPLAY`, `CONFIRMED_FROM_MANUAL`,
`INFERRED_IMPLEMENTATION`, `REQUIRES_BINARY_VERIFICATION`, `MEASURED_PS2_PARITY`.

## 1. Core architecture
| System | Mechanics |
|---|---|
| Playable characters | Jack Driscoll and King Kong |
| Jack | First-person survival shooter |
| Kong | Third-person action combat |
| Progression | Linear, story-driven chapters |
| Health | Regenerating, shown through audiovisual feedback (no bar) |
| Inventory | Restricted, context-sensitive |
| Combat | Firearms, spears, environmental weapons, melee |
| Enemy AI | Pursuit, attacking, feeding, distraction, scripted behaviour |
| Companions | AI allies, combat assistance, cooperative progression |
| Environment | Fire propagation, destructible obstacles, puzzles |
| Bosses | Large predators, V-Rexes |
| Saving | Checkpoints and chapter progression |
| Interface | Minimal HUD, optional crosshair and inventory display |

Jack is vulnerable: conserve ammunition, protect companions, use hazards, distract predators,
escape. Kong is powerful: physical combat, climbing, jumping, destruction, grappling.

## 2. Jack: controls (PS2)
| Input | Action |
|---|---|
| Left stick | Move |
| Right stick | Look |
| L3 | Crouch |
| L2 (hold) | Aim / ready weapon |
| R2 | Shoot while aiming; otherwise interact or repel |
| R1 | Reload |
| R3 | Zoom |
| Triangle | Drop spear |
| Circle | Check reserve ammunition |
| X | Call / interact with companion |
| Start | Pause |

**No manual jump.** Traversal is walking, crouching, terrain and scripted transitions. Do not
invent an FPS jump. Groups: `JACK_MOVEMENT` (J01–J15), `JACK_CAMERA` (C01–C12).

## 3. Jack: inventory (I01–I12)
One firearm + one spear/bone weapon (+ a puzzle lever). Holding a spear blocks the gun until
dropped. Picking up a gun replaces the current one. Ammo from crates, Englehorn's plane drops
and Hayes. Equipment persists across consecutive chapters unless scripted otherwise.

## 4. Jack: firearms (G01–G18)
| Weapon | Magazine | Behaviour |
|---|---|---|
| Pistol / Colt | 8 | moderate rate, low damage, repels small enemies |
| Shotgun | 5 | high close damage, spread, weak at distance, slow |
| Sniper rifle | 5 (manual; a walkthrough says 7: build-specific) | long range, powerful, slow |
| Thompson | 50 | automatic, low per-bullet damage |

Recovered so far (PC exe, `[C]`): ids 1 Colt, 2 TommyGun, 3 ShootGun, 4 SniperRiffle,
5 Grenade, 6 Javelin; hitscan with range Colt 50 / Tommy 80 / Shotgun 25 / Sniper 250 m;
damage bands near/mid/far by distance radii R1/R2: Colt 8/4/2 (5/25 m), Tommy 2/1/1 (10/10),
Shotgun 20/10/5 (5/10), Sniper 15/10/10 (5/50); cooldowns 0.2 / 0.1 / 0.5 / 0.4 s; shotgun 25
pellets in a ±10° grid with ±0.05 jitter; reload moves min(clip−mag, reserve). Source:
`research/pc/code/gameplay_spec.md`, `crates/kk_fps/src/spec.rs`.

Rules: ammunition is scarce; firearms repel without necessarily killing; **human firearms
cannot kill an adult V-Rex**; shooting small animals makes bait; some objects can be shot
down; damage/accuracy must be verified per enemy and hit region.

## 5. Jack: spears (S01–S16)
Developed spear (strong) and bone spear (weak, renewable from bone piles). Stab, aim, throw,
embed, recover, degrade/break, ignite at fire sources, burn enemies and vegetation, skewer
bait creatures and throw them to distract predators, drop to return to the gun. A spear is a
state machine: `standard → flaming | bait-carrier`, `held → thrown → embedded → recovered |
broken`.

## 6. Jack: survival (H01–H12)
Hidden health; time between injuries matters. Recovered `[C/L]`: 3-state wound model
(healthy → wounded → dead, `G+0x1bb4+4p`), per-hit countdown by hit flags and difficulty
(15.5/13.5/10.5 s, 10.5/7.5/7.5 s, 5.5/5.5/4.5 s), hit cooldown 3–5 s, death timer 3 s. A V-Rex
bite is lethal at any health.

## 7. Environment and puzzles (E01–E18)
Burnable pathway: fire → ignite spear → vegetation → wait → pass. Missing gate handle: lever →
gate → insert → crank (with companions) → open. Predator-controlled passage: bait → lure →
pass. Kong obstruction: approach boulder → interaction → forceful move → pass.

## 8. Creature AI and the food chain (A01–A18, X01–X18)
Creatures target Jack, companions, prey and each other. Suggested predator priority (not a
claim about the original architecture; the real order must be read from `PNJ_*_exec_select_action`):
```
dead → death behaviour
scripted override active → scripted behaviour
valid prey distraction preferred → pursue food, feed in reach
target visible and reachable → chase, attack in range
recently lost target → investigate last known position
else → idle / patrol
```
Shooting bats down diverts a V-Rex. Enemy types must **not** share one combat model
(movement, attack ranges, navigable surfaces, perception, fire reaction, food preference
differ): see `ENEMY_ARCHETYPES`.

## 9. Kong (K01–K15, KC01–KC18, KF01–KF07, AN01–AN11)
Third-person; jump, dodge, climb designated surfaces, swing from branches, carry Ann, move
heavy obstacles, smash structures. Combat: punch chains, shoulder strike, upward repel +
downward knockout, grab/throw, bite, dodge, knockdown, jaw-break and throw finishers (some
button-mashed), environmental and creature weapons, environmental kills. Fury (Triangle
mash): chest pound, faster/stronger attacks, more damage, more resilience, warm colour
grading, timed end. State model:
```
IDLE, APPROACHING_TARGET, PUNCHING, REPELLING, DODGING, GRABBING, HOLDING_ENEMY,
THROWING_ENEMY, USING_ENVIRONMENTAL_WEAPON, ENTERING_FURY, FURY_ACTIVE, STAGGERED,
EXECUTING_FINISHER, RECOVERING, DEAD
```
Ann: pick up, carry, place, protect; she can navigate, use fire and devices, be abducted.

## 10. Companions (N01–N14), set pieces (B01–B11), world (W01–W10), progression (P01–P15)
See the ledger. Alternate ending unlocks after completing the game with 250 000 points from
chapter replay (playable seaplane sequence).

## 11. V-Rex: dedicated specification
Large-scale locomotion with turning constraints; detection and pursuit; lethal close bites;
destruction of structures; reactions to gunfire and spears; food attraction and corpse
feeding; target switching Jack/companions/prey; threatens other creatures; a separate combat
behaviour versus Kong with stagger, knockdown, grapple and finisher states. Code: Jack-level
`PNJ_Tyranosaure_Jack` + `TrigExec_RexChaseRange`/`TrigExec_KTREX_Charge` + level data;
Kong-level `KT_*`.

## 12. What must be verified for 1:1 parity
| Priority | Subsystem | Exact data |
|---|---|---|
| Critical | Jack movement | acceleration, speeds, crouch height, pitch limits, collision size |
| Critical | Gunplay | damage, intervals, recoil, aim assist, reload timing, accuracy |
| Critical | Spears | throw velocity, gravity, collision, damage, durability, pickup range, fire lifetime |
| Critical | V-Rex AI | detection, targeting, chase speed, turning, attack windows, distraction |
| High | Other creature AI | navigation, patrol, attacks, reactions, packs, food priority |
| High | Health | damage values, regen delay/rate, death |
| High | Animation | transitions, blends, root motion, hit frames, finishers |
| High | Kong combat | hitboxes, grab compatibility, dodge windows, fury, finisher rules |
| Medium | Companions | pathing, interactions, targeting, damage, mission dependencies |
| Medium | Fire | ignition, propagation timing, burn damage |
| Medium | Presentation | camera motion, weapon offsets, audio triggers, effect timing |
| Medium | Checkpoints | stored state, respawn, inventory persistence |

## 13. Implementation order (milestones)
1. Jack locomotion and camera.
2. Jack firearms.
3. Jack spears.
4. One dinosaur: Venatosaurus (`PNJ_Raptor_*`): movement, detection, pursuit, attack, damage, death.
5. Food chain: prey detection, bait, corpses, target switching.
6. V-Rex encounter (distract and evade).
7. One companion: follow, react, gate objective.
8. Kong: movement, traversal, combat, fury, finishers.
9. Mission scripting: encounters, puzzles, checkpoints, chapters.

A game can have every mechanic and still not feel like King Kong. Parity means matching input
response, animation, enemy decisions and timings, which is why every mechanic goes through the
binary (`docs/PLAYBOOK.md`).
