# Playbook: how to recover one mechanic and port it to Rust

This is the loop every agent (human, orchestrator or subagent) runs. One mechanic ID from
`spec/mechanics.yaml` at a time. Do not skip steps; the ledger and evidence docs are how the
next session knows what is real.

## 0. Rules that never change
- **Never launch the original game. No live traces, no emulator runs.** Everything is static
  analysis of the user's own files.
- Never commit the ISO, BIOS, ELF, extracted assets, `research/pc/kb` or savestates.
- Tag every number and every rule with its confidence:
  `[C]` read from code/data, `[L]` logical inference, `[G]` guess/tuned. Ledger evidence
  levels: `CONFIRMED_FROM_GAMEPLAY`, `CONFIRMED_FROM_MANUAL`, `INFERRED_IMPLEMENTATION`,
  `REQUIRES_BINARY_VERIFICATION`, `MEASURED_PS2_PARITY`.
- The user wants game data over invention. A `[G]` is allowed only to keep something
  playable, and must be listed as a gap.
- Ask nothing; decide, record the decision, continue.

## 1. Pick
```
python3 tools/ledger.py next JACK_SPEARS      # what is open in a group
python3 tools/ledger.py show S05
```
Prefer mechanics in the order of `spec/GAMEPLAY_SPEC.md` § 20 (implementation order).

## 2. Locate the code (knowledge base)
```
python3 tools/kb.py find Javelin              # functions by name
python3 tools/kb.py fn Javelin_launch         # decompiled C
python3 tools/kb.py callers Javelin_launch
python3 tools/kb.py strings -i "javelin"      # strings + which functions use them
python3 tools/kb.py grep "0x4758"             # every function touching G+0x4758
```
`docs/ENGINE_MAP.md` says which prefix owns which mechanic. The decompiled code is MSVC `-O0`:
every local is a stack slot, every float goes through x87. Read it as the straightforward C it
was compiled from.

To resolve `*(float *)(this + 0xNNN)`: look the offset up in
`research/pc/code/ova/models.json` for the right model (Univers = `m334_*`, Jack = `m733_*`),
or grep `research/pc/code/ova/*.txt`. The value assigned to it is in
`research/pc/code/ova/uni_init_all.txt` (Univers init statements) or in the model's own init.

If the name table does not have what you need, it is engine code: start from the engine
functions called by the named AI function (`kb callees`) and from strings (`kb refs`).

## 3. Write the evidence doc
Create `spec/evidence/<ID>.md` with exactly these sections:
```
# <ID> <name>
## Behaviour (what the game does)        – 3–10 lines, plain English
## Code                                   – function@addr list, what each does, key branches
## Numbers                                – table: value | unit | where read | confidence
## Data                                   – ova/ofc statements, level data used
## Rust                                   – module/function that implements it; tests
## Gaps                                   – what is still [L]/[G], what the PS2 build may change
```
Quote decompiled lines sparingly (addresses are enough; the KB is the source).

## 4. Port to Rust
- Logic goes into `crates/kk_mechanics` (pure Rust, no Bevy, deterministic): one module per
  ledger group, one function/struct per original function where sensible, with the original
  function name and address in a doc comment.
- Presentation (rendering, audio, input devices) goes into `crates/kk_fps` (or a future Kong
  crate), which calls `kk_mechanics`.
- Keep the original numbers in named constants with their confidence tag. Keep the original
  semantics even when they look odd (e.g. the 3-state wound model, not an HP bar).
- Every ported function gets a unit test that pins at least one value recovered from the
  binary or data (`#[test] fn colt_damage_bands()`). That is what makes a mechanic
  `VERIFIED`.

## 5. Record
```
python3 tools/ledger.py set S05 status=IMPLEMENTED evidence=REQUIRES_BINARY_VERIFICATION \
   binary+=Javelin_launch@0x879190 rust+=kk_mechanics::spears::launch doc=spec/evidence/S05.md \
   notes="throw speed 18 m/s [C], gravity [L]"
python3 tools/ledger.py check
```
`ledger.py set` also regenerates `spec/STATUS.md`. Then add one line to `docs/research-log.md`
(date, ID, what was found, what is open). Finish by updating `docs/HANDOFF.md` if you changed
how anything is run.

## 6. Verify visually (only for things you can see)
The GPU batch harness (`run_kk_gpu_test.bat`, see `docs/HANDOFF.md`) renders scripted scenes
and checks numeric mechanics (49 checks today). Add a check to `crates/kk_fps/src/batch.rs`
when a mechanic has an observable number (timings, counts, distances). A reviewer subagent
grades frames against reference screenshots in `research/pc/reference/`.

## Running subagents (for the orchestrator)
Give each subagent **one** group or one mechanic, this file, `docs/ENGINE_MAP.md`, the KB path,
and the exact ledger IDs. Ask for: evidence docs, ledger `set` commands (or the YAML diff),
Rust in `kk_mechanics` with tests, and a 10-line summary. Run independent groups in parallel.
Never let a subagent touch `kk_fps` rendering code and `kk_mechanics` in the same task.
Review their evidence docs for unsupported `[C]` claims before accepting (open the KB
function they cite and confirm the number is in it).

## PS2 (parity target)
`research/target/SLUS_213.11` is the retail ELF. The AI2C name table exists there too
(the same name strings in `.rodata`). Pipeline to set up when needed:
1. Import into Ghidra with the EmotionEngine (R5900) language from `EmotionEngineReloaded`
   (build the extension with gradle, install into Ghidra), or use the existing PC project
   `GhidraFiles/KongPS2.gpr`.
2. Write a script that walks the AI2C table in the ELF (`{u32 key, fn, name*}` rows) and
   applies names, then run `KKExport`-style export into `research/ps2/kb`.
3. Compare a PC function with its PS2 twin for the constants that matter (frame timing is
   the usual difference: PS2 runs 60 Hz fields, timers may be in frames).
