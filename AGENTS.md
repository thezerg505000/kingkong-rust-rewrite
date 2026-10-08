# KongPS2Recompiled: instructions for any agent

Goal: rebuild Peter Jackson's King Kong (2005) gameplay in Rust, mechanic by mechanic, from the
user's own copies of the game (PS2 SLUS-21311 v1.02 is the parity target; the PC Gamer's
Edition `KingKong8.exe` is where the code is read because it carries the original function
names). The complete requirements catalogue is `spec/GAMEPLAY_SPEC.md`; the ledger of the 240
mechanics and their status is `spec/mechanics.yaml` (rendered: `spec/STATUS.md`).

## Read in this order
1. `docs/HANDOFF.md`: current state, how to run and test, what the last session left open.
2. `docs/PLAYBOOK.md`: the loop (pick → locate code → evidence doc → Rust → ledger).
3. `docs/ENGINE_MAP.md`: where each mechanic lives in the engine (function-name prefixes).
4. `docs/KNOWLEDGE_BASE.md`: the decompiled-code knowledge base and `tools/kb.py`.
5. `docs/SUBAGENT_BRIEF.md`: what to hand a subagent.

## Rules
- **Never launch the original game. No live traces, no emulator runs.** Static analysis only.
- Keep the ISO, BIOS, ELF, extracted assets, savestates and `research/` out of git.
- Every number and rule carries `[C]` (read from code/data), `[L]` (inferred) or `[G]` (guess),
  with the function@address or data record it came from. Game data beats invention.
- One mechanic ID at a time; every researched mechanic has `spec/evidence/<ID>.md`; every
  ported mechanic has a unit test in `crates/kk_mechanics` pinning a recovered value.
- Record: `tools/ledger.py set ...`, one line in `docs/research-log.md`, and `docs/HANDOFF.md`
  when the way of working changes. Do not ask the user questions; decide and write it down.

## Layout
```
AGENTS.md                this file
spec/                    GAMEPLAY_SPEC.md, mechanics.yaml (ledger), STATUS.md (generated), evidence/<ID>.md
docs/                    HANDOFF.md, PLAYBOOK.md, ENGINE_MAP.md, KNOWLEDGE_BASE.md, SUBAGENT_BRIEF.md,
                         research-log.md, target-build.md, parity-frontier.md (historic)
tools/                   ledger.py, kb.py, kb_annotate.py, ghidra/ (KKExport.java, run_export.sh|bat)
crates/kk_mechanics      ported game logic, pure Rust, tests   <- the product of this project
crates/kk_fps            the playable Bevy slice (Jack vs V-Rex, level 03E); consumes kk_mechanics
crates/*                 older exploratory crates (kk_gameplay has the PS2-side KT state helpers)
../research/pc/          NOT in git: kb/ (decompiled exe), code/ (AI2C tables, model vars, ova data),
                         game_assets/, atmos/, fx/, reviews/, reference/, batches/gpu/
```

## Build and test
```
cargo test -p kk-mechanics                 # logic tests (fast, no GPU)
cargo run --release -p kk-fps              # the playable slice (Windows, GPU)
run_kk_gpu_test.bat                        # scripted scenes + 49 numeric checks (see docs/HANDOFF.md)
python3 tools/ledger.py status             # where we are
```
