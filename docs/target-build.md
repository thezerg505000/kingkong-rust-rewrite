# Frozen target build

## Retail game

| Field | Value |
|---|---|
| Title | Peter Jackson's King Kong: The Official Game of the Movie |
| Platform | PlayStation 2 |
| Region | NTSC-U |
| Serial | SLUS-21311 |
| Disc version | 1.02 |
| Boot executable | SLUS_213.11 |
| ISO SHA-256 | 0BFB9552BE77AAB159377DF15C8EB5070A0EFDC490A4C7C306ABACF6F8EA2B67 |
| Main ELF SHA-256 | 3302F871D3A117CB9733938A485C4B85C430BF724C45BB33B3B96581CB210DA4 |
| Main ELF size | 5,613,540 bytes |
| Main ELF ISO extent | LBA 345 |
| First reference state | `research/target/states/jack-pistol-crabs-first-level.p2s` (copy) |
| Reference state SHA-256 | 2C5947BB1C4AC573F1E9BC50D06A198BBA28CD3BA7DE4C0EF57E86D239538969 |

The ISO's SYSTEM.CNF reports BOOT2=cdrom0:\SLUS_213.11;1, VER=1.02, VMODE=NTSC. The extracted ELF is kept at `../research/target/SLUS_213.11`, outside this repository. The original PCSX2 save remains in its original location; the listed state is a separate copy. User's scene annotation: first level, Jack has the pistol, crab enemies ahead. The copied state was loaded in the isolated debugger; its embedded screenshot matched the displayed frame.

## Current deliverable: offline Rust vertical slice

- Active target: recover the V-Rex/T-Rex and King Kong assets, animation data, and combat/AI behavior from this frozen PS2 build, then host the result in a playable native Rust executable. The planned scene is a plain test floor with a Kong player placeholder and a dinosaur opponent. Search asset names using `V-Rex`, `VREX`, `T-Rex`, `TREX`, and `B_Rex_*`; a text hit alone does not establish a model/package link.
- `crates/kk_demo` builds `target/release/kk-demo.exe`. It currently runs a deterministic placeholder Kong-versus-T-Rex arena with a flat, obstacle-free floor. Its Tab inspector reads the 32 external B_Rex part envelopes and displays 27 RLI-key/part-name candidates. Kong movement and punch/heavy/guard/dodge controls are playable prototype behavior; the T-Rex arena behavior and figures remain prototypes. None claims retail parity.
- The Rust executable is the prototype target. For the active offline phase, no original game or PCSX2 behavior test is being performed. Earlier controlled traces are historical evidence only and are retained in the research log.
- Extracted disc content, decompressed FF00 streams, and candidate GAO records stay outside this Git repository under `../research/target/`. The raw GAO candidates and their index are at `../research/target/extracted-gao-candidates/`; extraction code is `../research/tools/extract_named_gao_candidates.py`.
- The original focused FF00 scan indexed 272 bounded T-Rex-named GAO envelopes (41 distinct name/hash pairs), including the 32 repeated `B_Rex_*` part records, wrappers, and the three 14B `F_PNJ_TREX_*` records. An alias-aware expansion now indexes 717 bounded candidate envelopes (420 distinct names) across 45 streams; it also captures broadly named level objects, so those extra name hits need package/type validation. A bounded parser validates 74 `A_NET_TRex*` candidates in `ff00beca`: each has a 16-f32 matrix field at payload `+0x16` and u32 matrix type at `+0x56`. Seventy-two pass conventional rigid-transform invariants (43/43 type 6 and 29/30 type 2); `A_NET_TRex02_16bis.gao` (type 10) and `A_NET_TRex02_33.gao` (type 2) are exceptions. Matrix values 12–14 are the serialized fourth-row XYZ values at `+0x46`; calling them route coordinates remains a candidate interpretation. Their 53 internal A_NET references form 21 non-branching directed chains of 1–15 records. Field meaning, coordinate space, owning level actor, matrix-type meanings, and runtime AI use remain unproven. Details are under `../research/target/trex_a_net_candidate/`. The 14B F_PNJ names form a serialized reference chain, but object-root semantics are not proven.
- `S_TRex_Def_Gauche.gao` contains a validated 27-pair RLI table. Its indices align with 27 positions in the 33-name `B_Rex_*` reference chain and skip three `Snap_*` helper records. All 27 selected records have proper rigid 4x4 transforms of matrix type 6. A separate byte scan finds 20 RLI keys in their ordinal-matched records, two more in helper records, and five absent from this 33-record corpus. This strengthens the mapping for 20 parts but does not identify the byte field's semantics or prove parent/child hierarchy, bind pose, or composition order. The matching right-side name currently has only a 23-byte raw reference payload, not a recovered GAO body.
- Static ELF audits now identify the `KT_*` actor state IDs, animation selector API, selector-to-table indexing, 20-byte action rows, 24-byte runtime channel descriptors, 8-byte keys, and 60 Hz tick use. The serialized table handle and its T-Rex data remain unlocated; no human clip names or real track values are available. A bounded schema check of `T-rex_fake.gao` and `PNJ_TRex02.gao` found weak 20-byte-stride windows but no valid action-row/channel/key chain. `kk_assets::jade::animation` and `kk_gameplay::dinosaur::trex_state` preserve only these recovered interfaces.
- Same-suffix FF40 banks are confirmed as package-associated candidates, but literal WOL/RLI keys do not occur in the 44 scanned banks and they do not match the known FF8 LZO framing. No FF40 geometry decoder or direct actor-to-mesh binding is established; the renderable T-Rex mesh remains the main blocker. Audit notes and manifests are under `../research/target/`.
- The `TrigExec_RexChaseRange` callback is not T-Rex evidence: the static references lead to `PNJ_Raptor_exec_select_action`. The Rust demo's arena behavior is a deterministic prototype and does not claim retail AI or animation parity.

- The release executable is at `target/release/kk-demo.exe` under the Rust workspace. At the latest launch it remained open with window title “Kong and T-Rex Prototype Arena.” Its visible controls are documented in `crates/kk_demo/README.md`.
- A hash-checked offline probe extracted a 945-point, point-only Kong forearm-shell candidate from `S_Kong_Weta_Def_CoqueAvBrasD.gao`. A full scan of 45 decoded FF00 streams (37,615 bounded GAO occurrences) found no larger Kong-name body candidate; the next largest is a 4,099-byte camera record. Follow-up checks found the count-prefixed integer tables strictly increasing and unique, not direct triangle winding, and A/B indices did not coordinate-match the 455-point block; their remap/sparse-index interpretation is still unproven. Eight Weta RLI entries align by order with selected `B_Kong_*` names. Full field bounds and evidence are in `../research/target/kong_asset_mesh_probe_2026-10-06.md`; all candidate payloads and OBJ output remain outside the Rust repository.

## Reference emulator

| Field | Value |
|---|---|
| Executable | `../PCSX2 1.6.0/pcsx2-qt.exe` |
| Product version | 2.9.101.0 |
| Executable SHA-256 | F1E7B285AE3D96CEB72A999086C2675A70CAF06DA305967DE4D3970DAF7A4AFC |
| Status | User's game session was running at initial setup; left untouched |
| Settings/profile | Not yet captured |
| Selected BIOS identity/hash | Not yet captured |
| DebugServer/Pine in reference process | Ports 21512 and 28011 were not listening when checked |

The folder name "PCSX2 1.6.0" does not match the executable's product version. Preserve the existing reference session and profile. The debugger-enabled PCSX2 fork is a separate tool instance; record its identity before using it for traces.

## Analysis tooling and data

- Ghidra 12.1.4 PUBLIC project: `../GhidraFiles/KongPS2`.
- The exact ELF is imported as `SLUS_213.11` with language `r5900:LE:32:default`. A distinct EE RAM program, `jack-pistol-crabs-eeMemory.bin`, is loaded at base address 0 under `/KingKong_PS2_NTSC_U_first_level_state_EE_RAM`.
- Emotion Engine: Reloaded v2.1.38 is installed for this Ghidra version.
- PS2Recomp source: `../PS2Recomp`. The older C++ experiment is preserved under `../research/ghidra/output`. A fresh export from the analyzed `SLUS_213.11` Ghidra program and generation run are isolated under `../research/ghidra/fresh-20261006`; generation reported zero unhandled instructions but 2,772 unresolved indirect `JR/JALR` sites and has not been linked or run. This is comparative C++ output, not the Rust implementation.
- PCSX2-MCP source: `../PCSX2-MCP`; its Win64 release is installed separately under `../PCSX2-MCP-win64-v1.0.0`. The configured Codex MCP currently connects to the isolated portable instance's DebugServer and Pine endpoints.
- First state copy and extracted raw memories are under `../research/target/states/`. The state uses Zstandard-compressed entries; raw EE RAM extraction succeeded via the PCSX2-bundled Zstandard DLL. The Ghidra save-state importer did not support this compression method.
- The Rust workspace contains source scaffolding plus an evidence-bounded `kk_gameplay::input` model for normalized stick/trigger samples and trace stimuli; it does not implement game responses. No original ELF, ISO, BIOS, save state, or extracted game assets are in the Rust repository. The validated BF extraction remains outside this repository under `../research/target/data-bf-extract`.

The saved RAM contains one unconfirmed camera-transform candidate at 0x00e97364 with a transform pointer at 0x00e97390 and candidate position (320.661, 292.984, 22.903). This is not Jack's located actor transform. See `research-log.md` for the pointer chain and caveats.
## Isolated debugger instance

| Field | Value |
|---|---|
| Executable | `../PCSX2-MCP-win64-v1.0.0/PCSX2-MCP-v1.0.0-win64/pcsx2-qt.exe` |
| Window/build title | PCSX2 d75a0ad |
| Windows product-version metadata | 0.0.0.0 |
| Selected BIOS | scph39001.bin, USA v1.60 (07/02/2002) |
| BIOS SHA-256 | F4C948E61A291D4B3F92A141E550CF8357204287A31FF784CACCBEDAEF910C9D |
| MCP connection | DebugServer and Pine IPC connected |
| State slot 1 | `SLUS-21311 (2B1CC3FF).01.p2s`, exact copy of preserved user state |
| Game info | SLUS-21311, version 1.02, Running |
| Current recovered session | Running and unpaused; DebugServer and Pine connected; no breakpoints or watchpoints |

This portable instance has its own folders and settings under the release directory. The frozen user PCSX2 profile was not changed. The debug instance loaded the state and its screen matched the state screenshot. Its selected BIOS is recorded separately; the reference instance's BIOS remains unknown. Unattended input and RAM trace helpers are in `../research/tools`; with the physical Xbox unplugged, the virtual Xbox is the sole SDL gamepad at player index 0, matching this profile's SDL-0 mappings. The trace helper verifies this route before each pulse and leaves the emulator paused with the virtual pad neutral after each trace. After recovery from the conditional-breakpoint crash documented in `research-log.md`, the instance was relaunched at the preserved first-level state and left running at the user's request.

After an APPCRASH during a first persistent-breakpoint timing prototype, the isolated debugger PCSX2 was relaunched on the owned ISO and slot 1 restored. The current helper installs one-shot breakpoints before each timed stage, saves any hit's backtrace/register/context data, and resumes the EE until its requested active interval completes. The latest staged input trace is `../research/target/traces/20261006T004015Z-aim_then_fire` (1.25 s aim, 1.0 s aim+fire, 0.5 s neutral). During R2, `[s4+0x0ba8]` was set to 1 at `0x002f00ec`, cleared at `0x002f0174`, and remained 0 through the action-7 checks; its conditional restore was skipped because `[s4+0x0a98]` was 0 rather than `0x2c2`. The 0.5 s neutral tail captured a later `H_callback_tir`/`H_exec_GFX_Tir` callback pair and another `H_exec_read_joy` pass. The fire-request stores and handler remained unreachable, and neither projectile entry hit. Defaults now match this measured recipe; callback dispatch and timing interpretation are documented in `research-log.md`. The trace ended paused with neutral input and no breakpoints/watchpoints. The earlier `20261006T000458Z-idle` remains the latest standalone idle trace.

Conditional breakpoint installs are currently disabled. The PCSX2-MCP call returned an empty condition table for the earlier matrix-copy probe; a subsequent condition-install validation coincided with a Windows APPCRASH (`0xc0000374`). The helper no longer arms `0x0010b7e0` or records its hit as a transform writer. It verifies all plain one-shot breakpoints against the DebugServer table before resuming. Following recovery, matched movement traces cover idle and all four 0.359–1.001-second cardinal pulses: `../research/target/traces/20261006T011457Z-idle`, `../research/target/traces/20261006T011513Z-move_up`, `../research/target/traces/20261006T011642Z-move_up`, `../research/target/traces/20261006T012054Z-move_down`, `../research/target/traces/20261006T012132Z-move_right`, and `../research/target/traces/20261006T012138Z-move_left`. See `research-log.md` for the controlled candidate-transform deltas. After the final pulse, slot 1 was reloaded and the isolated game resumed without breakpoints or watchpoints.

### Decoded level resources

The validated MiniLZO probe and indexed texture output are under `../research/target/jade_decode_probe`. It decoded all 56 physical FF8 resources (454 blocks; 107,934,794 output bytes), plus matching 01B and 03E FF00 streams. The 01B and 03E streams produced 167 and 161 indexed PNGs, respectively, with 106 identical images. Their object-name strings include `_PJ_Jack.gao`, `S_JackFPS_Jambe02.gao`, and `PNJ_Tyranosaure_Jack.gao`.

Update for the active T-Rex slice: the MiniLZO probe has now decoded all 45 physical FF00 streams (309,049,748 output bytes total). The initial focused scanner extracted 267 bounded T-Rex-related GAO record occurrences (36 distinct names and hashes); the later alias-aware index covers 717 occurrences (420 names), including broader level-name candidates. The index is under `../research/target/extracted-gao-candidates/`. These include 32 `B_Rex_*` records repeated byte-for-byte across eight streams, with local transform-shaped matrices and intra-record references to other `B_Rex_*.gao` names. This establishes a reusable part-record set, not yet a decoded render mesh or proven animation rig. The physical FF40 resources remain opaque and are the current geometry extraction blocker; detailed evidence and V-Rex string hits are in `research-log.md`.


## 2026-10-06 — REA project and Claude Desktop installation

- Added REA source checkout at `../REA`, commit `50787b2faf1a4bea799d041960ca2b91199ef403`, and pinned published runtime `rea-agents@4.1.0` at `../tool-cache/rea-runtime`. Project CLI launcher: `../rea.ps1`; setup notes and evidence: `../research/rea-setup/README.md`.
- Upstream additive setup registered `rea` in Codex and the actual Microsoft Store Claude Desktop config under `Claude_pzs8sxrjxfjjc/LocalCache/Roaming/Claude`. Both original configs have `.rea.backup` files; unrelated settings are preserved. Existing Ghidra 12.1.4 and Microsoft JDK 21 are configured. No original game runtime experiment was performed.
- Production stdio MCP doctor passed: REA 4.1.0, 125/125 tools, 6/6 prompts, matching catalog identity and successful target-free `binary_session` request. This establishes startup/tool availability, not analysis of a target or Claude Desktop UI connection; clients must restart to load the registration.
- Windows Ghidra provider supports native x86-64 PE only, not this PS2 ELF/r5900 target. Existing PS2 Ghidra MCP remains the target analysis route. V-Rex mesh, animation-resource linkage, and runtime event descriptors remain unresolved.
