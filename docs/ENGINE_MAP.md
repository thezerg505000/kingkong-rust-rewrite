# Engine map: where King Kong's mechanics live

King Kong (2005) runs on Ubisoft Montpellier's **Jade** engine (the Beyond Good & Evil / Prince
of Persia engine). Everything gameplay-related is one of three things, and you need to know
which one you are looking for before you open Ghidra:

| Layer | What it is | Where to look | How we recover it |
|---|---|---|---|
| **AI2C code** (the game logic) | Jade's AI scripts (`.ova`/`.ofc`/`.omd` "models") were compiled to C and linked into the executable. Every state (`*_ETAT_*`), function (`*_exec_*`), callback and reflex of every character is a real x86 function with its **original name** in the AI2C table. | `research/pc/kb/functions/*.c` (decompiled, named). `tools/kb.py` to query. | Read the decompiled C, port to Rust in `crates/kk_mechanics`. |
| **AI model data** (the numbers) | Each AI model's variables live in a per-instance struct. Tunables (health, speeds, ranges, timers) are set by the model's `init` statements in the `.ofc` files inside the BF archives, or per level in the level's object data, not as constants in the exe. | `research/pc/code/ova/` (decoded models and the `uni_init_all.txt` statements), `research/pc/code/ova/models.json` (variable name per struct offset, per model). | `ofc_dump.py` / `ovaparse.py`; map `struct+0xNNN` in decompiled code to a variable name with `models.json`. |
| **Engine runtime** (Jade C++) | Collision, animation blending, physics, rendering, sound, the trigger system, loaders. Not scripted; shared across Jade games. Function names here are `unnamed-fn-address` unless we named them. | `research/pc/kb/functions/unnamed_fn_*.c`, strings table, callers of the AI2C functions. | Trace from a named AI2C function into the engine calls it makes. |

Level-specific behaviour (what spawns where, trigger volumes, fog, lights, which V-Rex
variant) is **data in the level banks** (`KKMaps.bf` → `ffXXXXXX` streams), decoded by
`research/pc/tools/*.py` and `research/pc/keys/tools/*.py`.

## Naming prefixes in the AI2C table (2 105 functions + 167 triggers, all named)

The names are the original French/English script names. `ETAT` = state, `exec` = helper
function, `cb`/`callback` = engine callback, `REFLEX` = per-frame reflex, `TRACK` = a
behaviour track, `Init`/`init` = instance setup, `paf` = hit/impact, `attente` = idle/wait,
`attaque` = attack, `mort` = death, `charge`, `cri` = roar, `suivre` = follow, `lance` = throw.

| Prefix | Owner | Examples |
|---|---|---|
| `H_` | **Jack (Hero)**: input, movement, weapons, wounds, grabs, IK | `H_exec_read_joy`, `H_exec_select_action`, `H_callback_tir` (fire), `H_exec_loading_weapon`, `H_exec_ch_Stimulus_Paf`, `H_ETAT_IA_mort` |
| `CM_`, `CAM_`, `camcont` | Cameras (Jack first-person `CM_Cam`, Kong modes `CM_Kong_Mode_*`) | `CM_Cam`, `CM_Pilote`, `CAM_controle` |
| `GG_`, `GST_`, `IW_`, `IntMIG_` | Game globals, game-state machine, cheats/console, menus | `GG_Exec_Joy`, `GG_Exec_AppendWeaponName`, `GST_Global_go` |
| `PNJ_<Species>_` | NPC creatures (PNJ = personnage non joueur). Species: `Raptor` (Venatosaurus), `Scolo` (Megapede/centipede), `Scorpion`, `Worm` (grub/larva), `Spider`, `KSpider` (Kong-level spider), `Corbeau` (bats/Terapusmordax), `Cricket`, `EatMe` (bait creature), `Car`, `Tank`, `Plane` (New York), `TplS`, `SwampCrawler`, `Brontosaure`/`pnjbronto`, `PNJInd` (natives, "indigènes"), `Tyranosaure_Jack` (Jack-level V-Rex) | `PNJ_Raptor_exec_select_action`, `PNJ_Scolo_ETAT_Attente` |
| `KT_` | **Kong-vs-T-Rex (V-Rex in Kong levels)**: 18 states with ids in `Rex+0x3a8` (see `spec/evidence/B02.md`) | `KT_ETAT_charge`, `KT_ETAT_attaque`, `KT_TRACK_init` |
| `KR_` | **Kong-level Raptor ("KRaptor", Venatosaurus)**, NOT rage/fury. Evidence: `KR_exec_bite`, `KR_ETAT_ride` (rides Kong), `KR_ETAT_intimide`, strings "JE SUIS UN RAPTOR SPAWNE", "KRAPTOR FINISH SUR KONG", "paf faible (10 dmg) en mordant kong" | `KR_ETAT_fight_KONG`, `KR_ETAT_Finish_on_KONG`, `KR_exec_check_fury` (reacts to Kong's fury shout, radius 35) |
| `k_`, `KIGO_`, `KIMO_`, `KIODE_`, `KInteractive_`, `KM_`, `KK_` | **Kong himself and his props**: `k_` = Kong (main state `k_ETAT_main` 0x89a120 id 200, grab `k_ETAT_grab`, mashing `k_ETAT_grab_mashing`, hit `k_ETAT_paf`, `k_reflex`); **fury is `k_exec_fury` 0x8b1d00 plus unnamed helpers at 0x883430/0x883700/0x884f00 and the timer `Kong+0x7c4`** (see `spec/evidence/KF01.md`); grab-object (`KIGO_`), mashing objects (`KIMO_` = KInteractive_Mashing_Object), physics objects (`KIODE_`), Kong camera (`KM_`), `KK_` = Kong-skin helper (init/tagon/tagoff) | `k_exec_each_frame`, `k_exec_fury`, `KIMO_ETAT_Mashing` |
| `KBC_`, `BC_`, `KBM_` | **Kong-level big bats** (`KBM` = bats manager: `KBM_ETAT_BigBat_Attack`, `PNJ_KBatsManager`, take-off/landing/perch/eat states `KBC_ETAT_Move_Decolle/Atterrit/Wait_Perchee/Sol_Mange`); NOT "boss combat" (`[L]`, from state names and the `KBigBat` triggers). `KBC_exec_check_fury` = bats react to Kong's fury shout within 50 | `KBC_ETAT_Fight_Network`, `KBC_exec_check_paf` |
| `KAnn_`, `KNa_`, `Na_`, `KNG_`, `KWC_`, `KPluieDeLances_`, `PluieDeLances_` | Kong-level Ann, natives (`Na`), native groups, "rain of spears" | `KAnn_ETAT_grabbed_trex` |
| `CR_`, `C_` | Crabs | `CR_ETAT_GRAB`, `CR_ETAT_APPARITION` |
| `SC_`, `MAF_`, `GOG_`, `SA_`, `ig_`, `ci_`, `PFK_`, `PLKJ_` | Swamp crawler (`SC`), flies (`MAF` = mouches), misc creatures and set pieces | |
| `Javelin_`, `Projectile_`, `Munition`, `Spawner_` | Spears (javelin), thrown projectiles, ammo boxes, spawners | `Javelin_launch`, `Projectile_Bullet`, `Munition` |
| `Interactive*`, `InteractiveDoor_`, `ode`, `btode` | Interactive objects: doors, levers, breakables, ODE physics bodies | `InteractiveDoor_waitactivation` |
| `TrigExec_`, `TrigTest_`, `TrigCINE_`, `Trig_`, `TrigSound` | **Trigger system**: 167 trigger actions/tests used by level scripts (the glue between level data and AI) | `TrigExec_RexChaseRange`, `TrigTest_KongLandingInBV`, `TrigCINE_Speech` |
| `SFX_`, `sfxg_`, `GFX_`, `Xenon_`, `SND_`, `GS_`, `SD_`, `MM_` | Effects, graphics helpers, sound, music manager, water (`MM_Exec_Water_Height`) | `SFX_RumbleCam_Capa` |
| `NET_Follower`, `PRG_`, `SCS_`, `Chaloupe_` | Follower network (companion pathing), program/script dispatch, raft (chaloupe) | |

Full list: `research/pc/kb/index/names.tsv` (address, primary name, aliases).
Note: the shared KB export (`research/pc/kb`) currently stops at address 0x6f1860 (4 806 of 10 685 functions), so
all `k_*`, `KBC_*`, `BC_*`, `KIGO_*`... functions at 0x7a0000-0x8f0000 are missing from it. Re-run
`tools/ghidra/run_export.sh` (resumable) or decompile on demand with a one-off headless script.


## The AI instance struct ("G" and per-character structs)

Decompiled AI2C functions access their model's variables as `*(type *)(this + 0xNNN)`.
There are two kinds of instance:

- **Univers / global game state** (`G`): one instance, aliased by many exe globals
  (`g_b99358`, `g_b98ca4`, `g_b990d8`, `g_b99920`). Weapon tables, ammo per player slot
  (`p=1` is Jack), difficulty, wound state. Variable names: `models.json` →
  `m334_v_joy_camera_normalized` (stream `ff00018c`).
- **Character structs** (Jack = model `m733_i_etat_courant` in `ff0003eb`, each PNJ its own
  model): per-actor timers, flags and tunables.

`research/pc/code/gameplay_spec.md` has the offsets already decoded (weapon table at
`G+0x342c..`, wound states `G+0x1bb4+4p`, etc.). `annot.py` shows how to annotate
`[reg+0xNNN]` with variable names; a KB-wide annotation pass is on the roadmap.

## Known entry points per mechanic group (start here)

| Group | Functions |
|---|---|
| Jack input/movement | `H_exec_read_joy` 0x579f30, `H_exec_select_action` 0x587bd0, `H_TRACK_joueur` 0x5816f0, `bhv_Move` 0x5ec020, `_main_pad` 0x5eae10, stick deadzone `sub_43cb00` |
| Jack camera | `CM_Cam` 0x45b1b0, `CM_CamInit` 0x45ad50, `CM_Pilote` 0x45af90, `CAM_controle` 0x611370, `camcont_loop` 0x6113d0 |
| Guns | `H_callback_tir` 0x5bbd10 (fire), `sub_540fd0` (hitscan), `sub_53a370` (damage bands), `H_exec_loading_weapon` 0x5ccc70 (reload), `H_exec_GFX_Tir`, `Munition` 0x6c3fc0 (ammo box), `GG_Exec_AppendWeaponName` 0x696240 (ids) |
| Spears | `Javelin_*`, `Projectile_Javelin`, `Projectile_launch`, `TrigTest_Arme` 0x469980 |
| Jack wounds/death | `H_exec_ch_Stimulus_Paf` 0x5a3060, `H_ETAT_IA_stunned` 0x5c6e20, `H_ETAT_IA_mort` 0x5a3e80 |
| Venatosaurus | `PNJ_Raptor_*` (init 0x8315a0, select_action 0x83a950, check_paf 0x849910, bite 0x84cae0, check_shoot 0x85b0a0) |
| Jack-level V-Rex | `PNJ_Tyranosaure_Jack`, `TrigExec_RexChaseRange` 0x78b900, `TrigExec_KTREX_Charge` 0x78bd60, level data ff001b17 |
| Kong-level V-Rex | `PNJ_KTREX` 0x4983b0, `KT_TRACK_init` 0x498640, `KT_ETAT_*` |
| Kong | `k_*` (Kong), fury = `k_exec_fury` + 0x883430/0x883700/0x884f00 + `Kong+0x7c4`, `KIGO_*`/`KIMO_*` (grab/mash objects), `CM_Kong_Mode_*`; creatures that fight him: `KT_*` rex, `KR_*` raptor, `KBC_*` bats, `KSpider`, `KNa_*` natives |
| Companions | `NET_Follower_*`, `PLKJ_*`, `InteractiveDoor_*`, `TrigCINE_Speech` |
| Fire | search strings `fire`/`feu`/`brule`, globals `ao_fire_gao`, `f_fire_propag` (Univers vars) |
| Triggers / level scripting | `TrigExec_*`, `TrigTest_*`; table of 167 in `research/pc/code/ai2c_triggers.json` |

## PS2 vs PC

Both builds compile the same AI2C scripts. The PC exe (MSVC `-O0`, x87) decompiles cleanly and
carries the names, so **the PC exe is where logic is read**. The PS2 ELF (`SLUS_213.11`,
MIPS R5900, 79k auto-functions, unnamed) is the parity target: use it to confirm a constant
or a branch when a PC value is suspected to differ (frame timing, controller mapping, memory
layout). The AI2C name table also exists in the ELF (same strings), so the same names can be
applied there; see `docs/PLAYBOOK.md` § PS2.
