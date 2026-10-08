# UPDATE 2026-10-08 (late): Kong marsh 05C, shell fur, first release build

**The reference clip is level 05C "Kong vs first T-Rex"**, not 07D (map table `KKMaps.index.tsv`: `05C_Kong_vs_first_Trex.wol` = key 0xc101eabc -> stream `ff01eabc`, bank `ff81eabc`; ambience `Amb_05C_area_c_rain`). Extracted on the PC with `tools/extract_bin.py ff01eabc` + `tex_decode.py ff81eabc`, rebuilt with the level tools (`runall.py` loader emulation clean over all 10 worlds; new `mkkeymap_lvl2.py` (KK_WORLD, KK_RECS), 234/325 anchors, 227 agree; `build_level.py` with cfg `ode_ground:false`, marsh `_eau` meshes classed as water) -> `game_assets/level05c/` (glb 40 MB, collision, bindings).
- 05C layout: arena 1 (single rex `PNJ_KTREX` model default 80/80/50, river + waterfall, Ann) and the **marsh** (two rexes [c101f503]/[c101f50a] with `f_life_init/max 55, f_life_seuil_blesse 40` [C], Ann on branch `D_BV_ANN_Branche` (135.7,-36.1,-171.7), water `DEC_05C_D_marais_eau` y -38.7 over floor -39.6..-37). `kk_mechanics::kong::fight::RexProfile::MARSH_05C`; AI test `the_05c_marsh_rex_ends_by_ko_and_finisher_with_every_move` (shoulder strike kept before fury). 203 kk_mechanics tests pass.
- Atmosphere [C]: `05C_LGT_FOG` (104,112,106) near 7 far 65; marsh `LD_Changefog` zone 2 fog (39,45,43) 1..100, ambient (63,74,74); key light `Xe_light_Spot_s07_Main` (131,143,137). Presentation (fog density, grade, exposure) tuned to the clip [G]; mean frame colour now (69,81,72) vs clip (72,86,77).
- `KK_SCENE=swamp05c` (default of b10 when level05c exists; `swamp07d` still available). b10_swamp_fight: **48/48 checks** (software GL).
- Rendering fixes: level materials double-sided; occluder hiding is triangle-exact and runs in PostUpdate (no lag on camera cuts); hidden ODE breakables no longer collide; camera escape search when pulled into Kong's head; fighters wade (ground >= water - 0.9 m); splash particles in front of Kong / the rex fade inside their silhouettes; lighter fury splash ring; rain finer.
- **Kong fur, ported from the exe** (`kk_fps/src/kong_fur.rs` + `kong_fur.wgsl`): Jade MDF FUR modifier type 0x2a (loader `fn@0x0096d2e0`, apply `fn@0x0096a880`/`fn@0x00a1c990`, draw `fn@0x00a4c090` OGLShiftDraw.c): N shells pushed along normals by i*len/N, texcoord shift i*(a,b)/10000, alpha test GL_GEQUAL i/N against the material's fur layer. Per-part data [C]: body len 12 cm/10 shells, arms 42 cm/22 shells (shift b=-40), head 15 cm/12 shells. Fur layer UV matrices decoded from the multitexture layer words (`fn@0x009a5470`/`fn@0x009a53b0`: scale = float(w & 0xfffe0000), offset = float(w<<16), 16-step rotation in stolen bits): body (-11.375, 23.25, 135 deg), face (-2.03, -1.97, 90 deg). The old contrast/detail bakes are off; Kong reflectance lowered (Bevy Fresnel caused the grey sheen). Not ported: dynamic strand physics (k14..k24). Kong's `Coque*` meshes are small bone-attached simple-fur shells (type 0x1a) whose textures (ca00xxxx) are not in the 05C bank: open.
- Extractor: manifest recipe `kong_fur` (fur_detail.png = idx173, fur_head.png = idx177). **Gap:** level 05C/07D world builds are not in `kk_extract` yet (release needs `build_release.bat dev` for the marsh).
- Release folder `Z:\ClaudeCode\KongPS2\release`: source (sanitised comments), `build_dev.bat`, `Play_Kong_Marsh_05C.bat`, `Play_Jack_03E.bat`, `Play_TestArea.bat`, `Watch_Kong_Fight_05C.bat`. Release `kk_fps` enables `bevy/wav` (cloud builds can't: hound not vendored).
- Backups of overwritten PC files: `research\pc\scratch\backup_2026-10-08c\`.

# HANDOFF: pick up here (last session 2026-10-08, scope-up waves 1–3 + slice integration)

Start by reading `AGENTS.md` (one page). This file says what exists, how to run it, and
exactly what to do next. Everything referenced lives in `Z:\ClaudeCode\KongPS2\` on the user's
PC: the git repo is `kingkong-ps2-rs\`, the non-committed research tree is `research\pc\`.

## 1. State in one paragraph
The playable Jack-vs-V-Rex slice (`crates/kk_fps`, Bevy 0.16, level 03E, real assets, lighting
matched to the user's reference, god rays confirmed by the user) now **runs on the ported game
logic** (`crates/kk_mechanics`): weapon table, damage bands, shotgun pattern, reload, fire timing,
Jack's wound model (0 healthy / 1 wounded / 2 recovering / 3 dead, no self-heal without Ann),
movement/look/eye/FOV, and the recovered Jack-level V-Rex numbers. **53/53 scripted checks pass
on the GPU** (`research/pc/batches/gpu/run_31302`; was 49 before integration). Weapon-only batches
(b4, b5) use the test-only `RexForce::Hold` so the rex's recovered 100 m perception does not walk
it out of the line of fire. The engine-recreation framework: a complete
decompiled, named knowledge base of `KingKong8.exe` (10 685 functions, 2 598 named), a 240-mechanic
ledger (`spec/mechanics.yaml` → `spec/STATUS.md`), 202 evidence docs in `spec/evidence/`, and
`crates/kk_mechanics` with 175 tests (Jack, weapons, wounds, spears, raptor, Jack-level V-Rex,
companions, doors/levers, fire grid, progression/scoring/checkpoints, Kong movement/camera/combat/
fury/Ann, Kong-level V-Rex). Ledger: **50 VERIFIED, 54 IMPLEMENTED, 60 PARTIAL, 53 RESEARCHED,
23 NOT_STARTED**. An independent audit (`docs/AUDIT_2026-10-07.md`) re-checked 73 `[C]` numbers
from 23 docs: 70 confirmed, 0 numeric errors, 3 meaning errors fixed.

## 2. What to do next (in this order)
1. **Per-instance data decoding (the main remaining front).** Most open numbers (creature hp/gait
   speeds/perception per instance, trigger bodies, burn/spread times per level, barricade and ODE
   thresholds, mash-kit counts, Kong life, music phases) live in the level banks' instance data
   (`.oin`, trigger records), not in the exe. Start from `research/pc/code/ovaparse.py`,
   `ofc_dump.py`, `research/pc/keys/tools/*.py` (level 03E bank ff001b17 decoding) and the
   `TrigExec_*`/`TrigTest_*` argument readers in the KB. One subagent per data family.
2. **Animation data** (hit frames, cancel windows, root-motion gait speeds for Kong and creatures):
   the action/channel/key schema is in `docs/parity-frontier.md` (PS2 side) and
   `research/pc/anim_findings.md`.
3. **The 23 NOT_STARTED** (`python3 tools/ledger.py next`): mostly set pieces B03–B11 (rafting,
   New York, Empire State, alternate-ending flight), W01/W03/W05, J09–J11/J13, C12, G14/G16, A04/A16/A18.
4. **Second playable slice**: a Kong scene using `kk_mechanics::kong::*` (movement, combat, fury,
   Kong-level V-Rex fight and finisher) once the Kong model/animations are exported.
5. **PS2 parity**: build the PS2 KB (`tools/ps2/`, see `docs/KNOWLEDGE_BASE.md` § PS2 names) and
   diff constants for every VERIFIED mechanic.
6. Remaining slice gaps: rex SEARCH/lose-interest/corpse eating (needs a second creature in the
   scene), Ann heal (needs Ann), look-sensitivity default (unknown; slice uses 0.05).

## 3. How to run things
| What | Command / place |
|---|---|
| Logic tests | `cargo test -p kk-mechanics` (pure Rust, seconds) |
| Playable slice | `cargo run --release -p kk-fps` (Windows, GPU) |
| GPU scene + mechanic checks | double-click `kingkong-ps2-rs\run_kk_gpu_test.bat`; output in `research\pc\batches\gpu\run_<n>\`, newest in `latest.txt`; `status.txt` ends with `ALLDONE`; expect 49 × `"pass": true` |
| Colour check vs reference | `python3 research/pc/batches/gpu/measure.py <run dir>` |
| Ledger | `python3 tools/ledger.py status|next <GROUP>|show <ID>|set <ID> k=v|check|render` |
| Knowledge base | `python3 tools/kb.py find|fn|grep|callers|callees|strings|refs|globals` (set `KK_KB` if `research/pc/kb` is not next to the repo) |
| Annotated C (offsets → variable names) | `python3 tools/kb_annotate.py <fn> [--model univers|jack|...]`, `--all` |
| Rebuild the KB | `tools/ghidra/run_export.sh` (Linux/cloud) or `tools/ghidra/run_export_pc.bat`; needs Ghidra 12.1.4 + JDK 21 and a Ghidra project containing `KingKong8.exe` (cloud project was `~/ghidra_proj/KK8`; on the PC import it into `GhidraFiles` first). ~1 h cold, minutes with `KK_EXPORT_RESUME=1` |
| PS2 names | `tools/ps2/ai2c_names_ps2.py` (done: `research/ps2/ai2c_functions_ps2.json`, 1 781 names matched), `tools/ps2/ApplyPS2Names.java` (untested Ghidra script) |

Driving the PC from a cloud session: stage files to `/mnt/user-data/outputs/...`, then
`device_commit_files` with `force=true` (never in parallel with the copy); files over 9 MB
must be split and `cat`-joined on the device; the Windows exe can only be launched by
double-clicking the `.bat` in File Explorer through computer use (click tier; access expires
after 30 min idle). The device shell (`device_bash`) can read results and run Python but not
the exe. **Never run two Ghidra headless instances on one project** (it killed the first export).

## 4. Where the knowledge is
- `docs/ENGINE_MAP.md`: function-name prefixes → mechanic owner. Corrections this session:
  `KR_` = Kong-level Raptor (not rage); `KBC_`/`BC_` = Kong-level bats; fury is in `k_*`;
  `PLKJ_` is a stick-driven character package, not companions; **companions are extra
  instances of Jack's `H_*` model** (slot 2 Ann, 3 Hayes, 5 Denham, 6 Jimmy?; `this[0]==0` =
  AI-controlled); **the Jack-level V-Rex is `PNJ_Raptor` species 0x10** (raptor 0xe, compies
  0x16), there is no `PNJ_Tyranosaure_Jack`.
- `docs/KNOWLEDGE_BASE.md`: KB layout, the global-pointer discovery (`DAT_00b9xxxx` are caches
  of `fn@0x00402080(key)+0x40`; key 0x72006b76 = Univers, key 0x3d0098b3 = weapon/ammo object),
  PS2 table layout (`{fn, name_ptr, key}` 12-byte rows at 0x62db84, 1 621 rows; triggers 16-byte
  rows at 0x635400, 160 rows).
- `docs/ps2_parity_probe.md`: all float literals of 4 PC functions found in their PS2 twins.
- `spec/evidence/*.md`: the recovered mechanics. Read `B02.md`, `KF01.md`, `X02.md`, `X04.md`,
  `N01.md`, `S05.md`, `G10.md` to see the expected depth.
- `docs/research-log.md`: one line per wave.
- Lighting/sky/god ray/material/RLI work of the slice: `research/pc/atmos/*.md`,
  `research/pc/HANDOFF_slice_2026-10-07.md` (the previous handoff, kept verbatim).

## 5. Known gaps and warnings
- `wounds.rs` numbering (see § 2.1). `gameplay_spec.md` (older notes) has several numbers that
  the KB work corrected (Colt/Tommy cooldowns, "walk modulated", check_shoot 64 = 8 m squared);
  trust `spec/evidence/` over it.
- Per-instance data (creature hp/gait/thresholds, trigger bodies, level overrides) lives in the
  level banks and `.oin` instance files, not the exe: decoding those is the next research
  front after wave 3 (`research/pc/code/ovaparse.py`, `ofc_dump.py` are the starting tools).
- `models.json` names for creature structs (m481, m331, m724) are unreliable; evidence docs
  name offsets by use `[L]`.
- REA's Windows Ghidra provider is x86-64-only; there is no PE32 fork. Ghidra headless is used
  directly (works fine); REA remains for the PS2 ELF/evidence bundles.
- Bait skewering (S14/S15), renewable bones (S11), Ann's torch (N07), Englehorn plane timing
  are not in the exe code found so far (script/instance data).

## 6. Housekeeping note
During the 2026-10-07 copy, `README.md`, `AGENTS.md` and `docs/research-log.md` were overwritten (the repo has no commits). `AGENTS.md` now contains the original instructions again; the old research-log entries are lost unless a Windows Previous Versions copy exists. **Make a first git commit soon** (respecting `.gitignore`: no research/, assets, ELF) so this cannot happen again.
- The PC copy of `research/pc/kb/functions` holds ~70 stale `FUN_<addr>.c` files for functions named since (the folder cannot delete files). `kb.py` reads the index, so they only show up in raw `grep`; delete them when deletion is allowed (any `FUN_*` whose address also exists under a real name).
