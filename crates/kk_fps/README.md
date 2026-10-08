# kk-fps — Jack vs. V-Rex first-person test slice (Rust / Bevy 0.16)

A playable first-person slice built from *Peter Jackson's King Kong* (2005) data:

- **Meshes, textures, skeletons, animation clips:** Jack's FPS arms (101 clips), Colt, Tommy gun, shotgun and sniper rifle, and the V-Rex (`B_Rex_*` rig, 123 clips). They come from the PC Gamer's Edition BF archives via the tools in `research/pc/tools`, exported to glTF.
- **Gameplay numbers:** movement speeds, eye heights, look rates, FOV, weapon magazines, fire rates, ranges and damage bands, the shotgun pellet grid, reload commit frames, the Rex's HP, bite reach and cone, and Jack's wound model. They come from static analysis of `KingKong8.exe` (`research/pc/code/gameplay_spec.md`). The recovered logic now lives in `kk_mechanics` (see "Uses kk_mechanics"); every number there is tagged `[C]` (read from code), `[L]` (likely) or `[G]` (filled in), and `src/spec.rs` keeps only presentation tables and slice-only debug values.
- The original game is never run.

## What's original vs. recreated

| Area | Source |
|---|---|
| **Sounds** | 95 original sounds from `Sound_Common.bf`, driven by the game's own `.smd` sound-definition table (`sound_defs.json`: which wavs each event picks from, and its volume). Covers all four guns (Tommy gun loop A + end tail, pump/bolt rearm, per-shell/per-round reloads, empty trigger, take), ricochets (stone/flesh), every V-Rex event (3 roars, near footsteps, bite, attack, growl, breath, eat), Jack (footsteps, injured heartbeat, body fall), 03E rain ambience and thunder. |
| **Level** | Original level 03E "Chased by the T-Rex" (`level03e/level03e_v2.glb`): all 201 visual instances from bank `ff001b17` with their **real Jade materials**. They're resolved through the recovered load-order key map (GAO → GRM sub-material → texture key → `ff801b17` bank chunk). Paletted textures are decoded as BGRA. Soft-alpha surfaces (water, mist) blend, and foliage cards are alpha cut-outs. Collision is 51k upward-facing triangles from opaque ground and rock meshes. The level's own sky dome (`ENV_Ciel`) and distant tree cards (`DEC_E_fake02_*`) are included. Its mist cards (`*Brume*`) and second sky layer are exported but hidden, because their edge fade lives in per-vertex alpha that the exporter doesn't carry yet. The destructible `LD_03E_ODE_*` doubles and occluder volumes are hidden. |
| **Camera shake** | An exact port of the game's `CM_Sfx`. V-Rex footstep shake (0.025/0.05·k, 50 rad/s, decay 0.1 ×0.95 per frame, k = 1 − max(d−10,0)/190) and bite shake come from code. The roar shake uses the only shake record in 03E, `TrigExec_ShakeCamAndRumble` (0.075, 30, 0.15, 20, 0.15, 1.02), with no rumble [values C]. KingKong8.exe itself has no roar shake: the only raptor shakes are footstep 0x486890 and hide-attack 0x482b80, which only fires when k > 0.5. Applying the record to the roar is [G]. |
| **Rumble** | Gun (50,2)/(100,4) for the sniper, and V-Rex footsteps (⌊100·k⌋, 7), all from code. |
| **Effects** | Parameters come from code. Sprites come from the game's global FX material `0x62000f8c`: muzzle-flash frames (subs 0x12/0x13/0x11, cycled as in the code, additive), GFX smoke (0x08), spark streak (0x21), splatter dots (0x25, tinted red for blood) and rock chips (0x28). Muzzle light is only on the Tommy gun and shotgun. Sizes, flash tint, blood motion, roar breath and footstep dust are [G]. |
| **Weather** | Rain is in the level data (`SFX_*_RainSnowStatic` emitters plus the rain ambience). Drop count, speed and the thunder timing are approximations. |
| **V-Rex colour** | The Rex uses its own materials, multi-material `0x4300621b`: body `0x7e0001d3` (diffuse, normal and spec maps) and head `0x83000071`. Their colour constants are [C]: body ambient/diffuse 0x96, head ambient 0x19 / diffuse 0x7f, specular 0xe5. The head's low ambient is reproduced with a constant occlusion texture (Bevy's occlusion scales only indirect light); this removed the old pale face. Roughness comes from the spec maps (`rex_mr_*.png`) [L]. The synthetic tint is gone. |
| **Atmosphere** | `atmos.rs`. Fog is Jade linear fog with near 1 m and far 105 m [C]. Its colour is tuned to the reference shots [G], because the recovered greys (93/119, ChangeFog 37,47,47) are brightened by the PC after-effects pass. Moonlight is RGB(151,180,186) [C] on a directional key plus the arena's own `Xe_Light_Spot_Moon_*` spots (positions C, aimed down local −Y [L], cones from the spot records). The **light shafts** port the original `pslightshaft` idea (slices in the light frustum × scrolling noise × shadow map, added on top) onto Bevy volumetric fog: shadowed volumetric moon lights in a mist volume with a scrolling 3D noise density. Density and intensities are [G]. Off with `KK_NO_SHAFTS=1`, and automatically under software GL, which cannot compile the pass. |

## Run (Windows)

```powershell
cd Z:\ClaudeCode\KongPS2\kingkong-ps2-rs
cargo run --release -p kk-fps
```

The game reads its assets from `..\research\pc\game_assets`, which must hold:

- `jack_fps_arms.glb`, `jack_fps_luger.glb`, `jack_fps_tommygun.glb`, `jack_fps_shotgun.glb`, `jack_fps_sniperrifle.glb`
- `trex_inplace.glb`, `rex_mr_body.png`, `rex_mr_head.png`, `trex_rootmotion.json`
- `sound_defs.json` and `sounds/*.ogg`
- `fx/*.png` (including `flash_f11/12/13`, `spark_streak`, `splat_dots`, `rock_chips`, `smoke_fx`)
- `level03e/level03e_v2.glb` and `level03e/level03e_v2_collision.json` (set `KK_STAND_IN=1` to use the old stand-in clearing instead)

Set `KK_ASSETS=<folder>` to use another location. Assets are never committed to this repository.

## Controls

| Action | Keyboard / mouse | Gamepad |
|---|---|---|
| Move / look | WASD / mouse | left / right stick |
| Run, crouch | Shift, C or Ctrl | L3, B |
| Aim (ADS) | right mouse | LT |
| Fire | left mouse | RT |
| Reload | R | X |
| Weapons | 1–4, mouse wheel | Y / bumpers |
| Respawn after death | Enter | Start |
| Debug: cycle every arm clip | F1 | |
| Debug: mortal V-Rex (250 HP instead of 2000) | F2 | |
| Debug: show the Rex hit spheres | F3 | |
| Hide HUD (the original shows none) | F5 | |

In the original, Jack cannot kill the V-Rex: it has 2000 HP and guns do not change it (a hit only slows it, and a single hit of 20 or more makes it re-think and roar). F2 makes it killable so the encounter can be finished.

## How the pieces fit

- `anim.rs` loads the glTFs and builds an `AnimationGraph` from every named clip. Clips are looked up by their label, for example `idle_c14` resolves to `idle_c14__arms_026`.
- `player.rs` runs the FPS controller (`kk_mechanics::jack` speeds, look, eye, FOV) and the wound model (`kk_mechanics::wounds`). It also renders the viewmodel on its own camera and render layer, and pins the rig's `B_Jaf_Camera` bone to the eye every frame.
- `weapons.rs` handles hitscan fire against analytic world geometry and the Rex's per-bone hit spheres, distance damage bands, magazines and reloads, and weapon switching. Each weapon mesh is mounted on `WeaponSocket` (`B_Jaf_Anex01`).
- `rex.rs` drives the Rex with the `vrex_jack` rules: idle → hesite (turn, 0.5-0.8 s) → roar → chase (walk 2.2 / run 14 m/s, +1.5 m/s per second; the gait clip is the one whose root speed, from `trex_rootmotion.json`, is nearest) → bite (reach 3.8 m from the head bone, 40° cone; a healthy Jack is wounded, a wounded Jack is grabbed and killed). There is no flinch, bite cooldown or contact damage. `trex_inplace.glb` is `trex.glb` with the doubled root motion (JadeActor plus pelvis) removed.
- `world.rs` loads level 03E and its ground and prop collision (or the stand-in arena with `KK_STAND_IN`). `KK_HIDE=a,b` hides level instances by name, for debugging.
- `atmos.rs` sets up fog, moonlight, volumetric light shafts and the mist volume.

## Uses kk_mechanics

The slice is a consumer of the ported game logic in `crates/kk_mechanics` (evidence docs in `spec/evidence/`):

| kk_fps | kk_mechanics | Evidence |
|---|---|---|
| `weapons.rs`, `spec.rs` | `weapons::{WEAPONS, damage_at_distance_sq, shotgun_pattern, Ammo::reload}`: clip, range, R1/R2 bands (int-truncated, squared Jack-to-rex-origin distance), 25-ray shotgun grid, reload transfer (only the shotgun is 1 per cycle), shot timers 0.4/0.5/0.4 and the Tommy 0.1 s auto interval | G02, G03, G10, G11 |
| `player.rs` (Wounds) , `hud.rs` | `wounds::Wounds`: status 0 healthy / 1 wounded / 2 recovering / 3 dead, per-hit flags, death-count tiers, flag 0x200 kills, Jack never heals without Ann, restart after 4.0 s dead (cap 8.0 s); the HUD vignette follows the status | H01-H04, H10 |
| `player.rs` | `jack::{MoveState, look_step, eye_target, smooth_eye, fov_target, smooth_fov}`, stick thresholds 0.25 / 0.675 | J01-J07, C01-C06 |
| `rex.rs` | `creatures::vrex_jack`: perception (100 m cone + LOS, 150 m gunshots, 8 m shot-notice), hit rules (no hp loss, slow, re-think at >= 20), gaits and 1.5 m/s² acceleration, `bite_outcome` (bite 0x4104 wound / grab 0x4a10 kill), HESITE roar delay | X04, G17 |

Still slice-only (`[G]`): view kick and viewmodel bob, mouse sensitivity and the look sensitivity fed to `look_step` (0.05), the sniper/shotgun reload commit frames, Rex chase turn rate, bite-in-place and hold length (bite clip), hit spheres, the intermediate trot clips, wound vignette alphas, the debug cheats (F2 mortal rex), and the batch scaffolding that forces rex states.

## Arm clip mapping (how it was chosen)

The game's action table (`ff0003eb` @ 0x1e905a) isn't decoded yet, so no clip names exist. The mapping was chosen in two steps:

1. For each of the 101 clips, compute where `WeaponSocket` aims the weapon's muzzle (−Y). Only about 19 clips keep it pointing forward.
2. Render every weapon in each of those clips with `KK_GALLERY=<dir>` and check the grip visually.

| Weapon | Hip | Aim | Fire (aimed) | Reload |
|---|---|---|---|---|
| Colt | idle_c01 | idle_short_c23 | fire_c23 | reload_l_c09 |
| Tommy gun | idle_c14 | idle_short_c25 | procedural | reload_l_c43 |
| Shotgun | idle_c16 | idle_short_c24 | fire_c24 | reload_l_short_c36 (one shell per cycle) |
| Sniper rifle | idle_c17 | idle_short_c25 (scope) | procedural | reload_c37 |

## Real-GPU test run (Windows)

Double-click `run_kk_gpu_test.bat` in the workspace root. It builds a release binary and runs all six batches on the GPU (Vulkan) with light shafts and bloom on, writing to `research\pc\batches\gpu\run_<n>\`; `latest.txt` names the newest run. Each run takes about 90 s including the build. The software-GL cloud runs can't compile Bevy's volumetric pass or bloom, so those are switched off there.

## Mechanic test batches (with independent visual review)

`KK_BATCH=<name> KK_BATCH_OUT=<dir> cargo run -p kk-fps` stages one scenario on a fixed 30 Hz clock, drives the inputs, checks the recovered mechanics numerically, saves clean (HUD-less) frames, writes `<dir>/<name>.json` and exits:

| Batch | Mechanics checked | Reference shot |
|---|---|---|
| `b1_colt_vrex_roar` | Colt semi-auto cadence (0.4 s shot timer, G02), rex hp untouched by guns, damage bands 8/4/2, reload transfer, roar sound/shake/breath, gun rumble, flash per shot, no Colt light | Luger vs V-Rex |
| `b2_tommy_vrex_charge` | 10 rounds/s, damage band, magazine, muzzle light, loop/end sounds, footsteps from the animated toes, footstep rumble/shake | Thompson vs V-Rex |
| `b3_shotgun_close` | 0.5 s shot timer, 25 pellets, 20/10/5 bands, shell-by-shell reload, pump sound, flesh ricochet | Trench gun |
| `b4_sniper_scope` | 0.4 s shot timer, scope FOV 0.3, damage band, rumble 100, shoot/rearm sounds | Sniper |
| `b5_reload` | Sniper reload: commit frame, min(clip-mag, reserve) in one cycle (only the shotgun is 1 per cycle), sound sequence | Reload |
| `b6_rex_hunt` | V-Rex AI: idle → hesite → roar → chase (1.5 m/s² ramp) → bite wounds (0x4104) → bite grabs and kills (0x4a10), plus the sounds | Luger vs V-Rex |

The frames go to an independent reviewer agent that compares them with original-game screenshots (`research/pc/reference/imfdb`). A batch passes at 9/10 or higher.

## Automated checks

- `KK_AUTOTEST=<dir>` plays a scripted 32 s session and writes screenshots plus `autotest.log`. The session covers firing, aiming, reloading and switching all four weapons, and the Rex's approach and bite.
- `KK_GALLERY=<dir>` renders the clip gallery.
- Add `KK_SOFTWARE_GL=1` on machines without a GPU (Mesa llvmpipe). It disables GPU culling and compiles pipelines synchronously.

## Creatures, test area and test-area batches (2026-10-08)

**`creatures.rs`** is a generic creature plugin: `spawn_creature(commands, assets, kind, pos, yaw, SpawnOpts)` spawns any kind of the table `KINDS` (raptor, compy, raptor_kong, brontosaurus, crab, kong) from `<assets>/creatures/*.glb` (+ `kong/kong.glb`), `CreatureAssets::resolve(kind, id)` turns an action id into a clip (a label `idle|walk|run|attack|hit|death`, a bank index `#31`, an AI animation id `0x46` through `*_actions.json`, or a clip name) and `play_clip` cross-fades to it. Feet are put on the ground automatically (lowest foot bone, `calibrate_ground`).

| Species | Logic | Evidence |
|---|---|---|
| raptor (0xe), compy (0x16) | `kk_mechanics::creatures::raptor::Raptor` state machine: wide-cone sight 100 m + line of sight, target chain, HESITE then FIGHT chase at the run clip's root speed (6.19 m/s), bite start 3 m / 60 deg, bite hit 2.5 m / 30 deg in the armed frames 14-19 of the bite clip, **first bite on Jack grabs** (grab counter) = paf 0x4, 4 s later 0x204 kill, later bites are plain wounds (0x4004; compy 0x1000, damage 1, no grab, 0.5 s wait, hp 3, scale 0.2-0.35), flinch classes from Jack's gunfire (hits from `GunEvent::Fired`, damage = weapon band at the Jack-creature origin distance, flags 0x44, head x2), A_TERRE -> MORT 5 s -> FADE 10 s -> corpse released; corpses are food for the others (rule 6) | X02, X01, A03, A05, A07, A08, A10 |
| brontosaurus | waypoint loop at the walk clip's root speed (3.26 m/s, 0.2 rad/s turn); each of the 4 leg bones within 3 m of Jack sends paf 0x10 (rising edge) | X14 |
| raptor_kong, crab, Kong | props: clips only | - |

Jack is only touched through the public `Player::paf(cause, flags)` (wound model H01-H04). Gait speed ramp, hesitation midpoint (0.75 s), turn rates, hit spheres, compy run speed (0.8 x raptor), the grab hold geometry and the creature clip picks are `[G]`.

**In the 03E slice** a pack of 3 raptors (south-west of Jack, facing away) and 4 compies (east and west of the courtyard) spawn once their glbs are loaded. `KK_NO_CREATURES=1` removes them; every `KK_BATCH` that is not a test-area batch (b1-b6) and `KK_AUTOTEST` behave as if it were set, so their numeric checks stay valid.

**Test area** (`testarea.rs`): `KK_SCENE=testarea` (set automatically by the `t*` batches) replaces level 03E by a flat 3 km grid ground (1 m cells, 8 m tile, mip-mapped), neutral sky and ambient light, a post every 10 m down Jack's view axis (-Z), +X, -X and +Z with labels, Jack at the origin looking down -Z, no Rex. Every creature kind is loaded; creatures are spawned by script (`tbatch.rs`) or in code. Labels above creatures: `ShowLabels` resource. For hand play set `KK_NO_KONG=1` as well, the Kong fight of `kong.rs` expects the 03E arena.

**Test-area batches** (`KK_BATCH=t1 ...`, same runner conventions as b1-b6: 30 Hz clock, clean frames, JSON; the clock waits until every creature glb is loaded). `KK_RES=800x450` shrinks the window (software GL is ~4x faster). The script language is `TAct` in `tbatch.rs` (Put creature, Play action id, Face, Jack, Frame, Aim, AimNearest, Auto fire, AutoOnGrab, AutoHeal, Measure, Sample, Shot, Mark, End); `AutoHeal` restores Jack 0.25 s after each wound so one run can test several bites.

| Batch | Checks (each cites its evidence doc) |
|---|---|
| `t1_creature_lineup` | all glbs loaded and rigged, clip counts = manifest (88/88/78/3/21, Kong 254), standing height = manifest height x runtime scale, feet on the ground, idle clip playing; front, side, bronto and close screenshots |
| `t2_raptor_hunt` | perception (wide cone, raptor faces away at first), HESITE -> FIGHT -> MORD -> GRAB order, hesitation 0.5-1.0 s, run speed 6.19 m/s, bite start <= 3 m, bite reach, grab wounds Jack, Colt bands 8/4/2, flinch classes replayed through `classify_hit`, hp 50 (head x2), A_TERRE/MORT 5 s/FADE 10 s timers, corpse stays |
| `t3_compy_swarm` | hp 3, scale 0.2-0.35, all chase, bite flags 0x1000 / damage 1 wound Jack, one Colt hit kills |
| `t4_bronto_walk` | walk speed = clip root speed, lane following, stomp paf 0x10 within 3 m on >= 2 feet, wounds Jack |
| `t5_creature_anims` | idle/walk/run/attack/hit/death on every creature: clip plays, pose changes, undefined actions match the manifest; one screenshot per action |

Run: `bash run_batch.sh t2 <outdir>` (needs `KK_ASSETS` with `creatures/` and `kong/`). Grid of the t5 frames: `montage t5_creature_anims_{idle,walk,run,attack,hit,death}.png -tile 3x2 -geometry 800x450+2+2 t5_grid.png`.

## Lighting, sky and god ray (2026-10-07)
Start with `research/pc/HANDOFF.md`, which is the resume guide. Full write-ups are in `research/pc/atmos/`.

- **`atmos.rs`:** fog, the real 03E light colours, the sky fill and the mist volume.
  - 03E sky directional: RGB(169,186,184), record 08001b3e [C].
  - Fog: zone 2, far 105 [C].
  - Bloom and volumetric fog are turned on only on a real GPU.
- **`godray.rs` / `godray.wgsl`:** a port of the game's XeGodRayEffect, done as a single pass with 128 taps.
  - Source: the 03E `LD_03E_GodRay` object.
  - Sky mask: depth beyond 400 m. Only light above the overcast level feeds the rays.
- **`sky.rs`:** the level's own cloud texture (tex_1f007698) on a sphere that follows the camera, plus a sun gap behind the Rex. The god ray streams from that gap. Set `KK_OLD_SKY=1` to use the original fogged ENV_Ciel dome instead.
- **`rex.rs`:** the decoded material (MATERIAL_FORMAT.md) and the level RLI vertex light. COLOR_0 in `level03e_v2.glb` is the RLI vertex light.
- **Both cameras use `Msaa::Off`.** They share one view target, and the god-ray pass needs MSAA off.

**Debug env vars:**
- `KK_NO_GODRAY`
- `KK_GODRAY_DEBUG=nodepth|pass`
- `KK_NO_SHAFTS`
- `KK_OLD_SKY`
- `KK_SOFTWARE_GL`
- `KK_HIDE=a,b`
- `KK_NO_RAIN`

## Kong (`kong.rs`, `kong_cam.rs`, `kong_fx.rs`)

King Kong is in the 03E arena fighting the V-Rex, driven by `kk_mechanics::kong::fight` (the same sim the mechanics tests cover).

- **Fight to world:** the fight plane (metres, x/y) maps to X = cx + x, Z = cz - y around (40, -83). `FightEvent::Anim` ids are resolved to `kong_actions.json` clips (254 clips in `kong/kong.glb`), speed-scaled to the fight's animation lengths. The rex entity follows the fight's `KtState`, so the existing rex events, sfx and fx fire.
- **As Jack:** `KongBrain` plays Kong's pad and shows every move. Jack can walk about and shoot. `jack_rex_ai` and `jack_active` run conditions stop Jack's own rex AI and controls when the player is Kong.
- **As Kong:** Tab or pad Select switches. Jack is hidden and frozen, the camera becomes the third-person Kong camera, and the keys map to `KongInput`.
  - WASD or left stick moves.
  - Space or A is jump/roll (dodge).
  - Q or Y is special (repel, fury).
  - Mouse 1 or X is attack.
  - E or B is cancel.
  - Tab again restores Jack and his camera. The fight clock does not restart.
- **Effects:** water and mud splashes at feet and impacts, camera shake and rumble, a hit flash, and a GPU-only `MotionBlur` on the Kong camera (off on software GL).
- **Env vars:** `KK_NO_KONG=1` restores the old slice. `KK_KONG_DEBUG=1` logs the fight twice a second. `KK_KONG_WATCH=1` makes Jack's camera follow the fight. `KK_KONG_CAM=1` uses the Kong camera while the AI plays.
- **Batches:** `b7_kong_fight` (AI fight, event-triggered screenshots), `b8_kong_player` (Tab, scripted pad, Tab back), `b9_kong_cinema` (AI fight through the Kong camera).
- **Known gaps:** Ann is not placed (the model is headless and untextured). MotionBlur cannot be checked in the cloud. Kong's fur is lifted with an emissive term to stay readable in the dark 03E light.

### Swamp scene (`KK_SCENE=swamp07d`, `swamp.rs`, `fightarena.rs`, `scene.rs`)

Level 07D "Kong Saves Ann" (`level07d.glb` + collision + `atmos_07d.json`): LGT_FOG (140,143,129) exp-squared fog, key/fill lights, screen-space
rain (no emitter record was recovered, so the streaks are a guess), drifting mist cards, a murky water material (ripple normal map scrolling) on
the water meshes (y = -1.65), ring ripples and swamp-scaled splashes at footsteps, hits and falls, and Ann (headless export + a head ball) at
the edge. The fight sim itself is unchanged; `fightarena.rs` picks the fight centre and axis from the collision data (largest empty circle on
a 1 m clearance grid, longest chord as the fight axis), rotates the fight plane to it and slides both fighters against free cells (so they never
enter pillars). Jack spawns at a vantage point facing the fight. Same placement code runs in 03E.

Batches: `b10_swamp_fight` (cinematic style camera: close-ups for chest pound, roar and finisher; screenshots every 3 s as `_tNNN` plus key
moments). `KK_BATCH_MAXT=<s>` shortens a run. `KK_SURVEY=...` / `KK_FREEZE=1` are debug poses for material calibration.

Kong camera (`kong_cam.rs`): Player and Cinema styles; the desired position is ray-tested from Kong's head against the level collision and
pulled in (kept 0.6 m off the surface), then eased and re-clipped; it frames both fighters and uses the recovered `kong::camera` parameters.
Kong material: diffuse 0.298 x tint, normal scale 1.21, strand detail layer baked into the fur atlas, contrast tuned against reference crops.

Swamp camera/FX update: `swamp.rs::occluder_hide` hides level meshes (not floors) that stand between the camera and the fighters or contain the camera (the 07D canyon is too cramped for a camera 12 m behind Kong); swamp splash particles fade out near the camera and their sizes are capped so the fighters stay readable; the chest-pound/roar close-up sits 11.5 m in front of Kong looking at head/chest. b3's "25 pellets" check now counts rays fired (`Arsenal::ray_log`: rays, impacts on any surface, sky misses) per G11.
