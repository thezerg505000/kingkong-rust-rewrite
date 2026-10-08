# The decompiled knowledge base (`research/pc/kb`)

Generated, never committed, rebuildable in about an hour. It is the thing that makes
"find the mechanic in the engine" a grep instead of a Ghidra session.

## Contents
```
research/pc/kb/
  functions/<Name>_<addr>.c     one file per function of KingKong8.exe (10 685), decompiled by
                                Ghidra 12.1.4, with a header: aliases, callers, callees, strings
  index/functions.jsonl         one JSON record per function: addr, name, aliases, size,
                                callers, callees, strings, globals (data addresses touched), file
  index/names.tsv               the 2 105 AI2C + 167 trigger + extra names applied (addr, primary, aliases)
  index/strings.tsv             every defined string: addr, text, referencing functions
  index/callgraph.tsv           caller \t callee
```

## Query it
`tools/kb.py` (Python 3, no dependencies). `KK_KB=/path` overrides the location.
```
kb stats
kb find "^PNJ_Raptor_"             # 72 Venatosaurus functions
kb fn PNJ_Raptor_exec_select_action
kb callers sub_53a370
kb strings "Venato|Raptor"
kb refs "RELOAD"                   # functions referencing a string
kb grep "0x4758"                   # offset / constant search across all decompiled code
kb globals 0xb99358                # functions touching a global
```
Plain `grep -rn` over `functions/` works too.

## Rebuild it
Cloud session (Ghidra + JDK 21 already present in earlier sessions):
```
GHIDRA=~/tools/ghidra_12.1.4_PUBLIC PROJ=~/ghidra_proj RESEARCH=~/kkpc tools/ghidra/run_export.sh
```
Windows: `tools/ghidra/run_export_pc.bat` (needs a JDK 21 and a Ghidra project `KK8` in
`GhidraFiles` containing `KingKong8.exe`; import once with the GUI or
`analyzeHeadless ... -import KingKong8.exe -processor x86:LE:32:default -cspec windows`).

`KK_EXPORT_RESUME=1` (the default in `run_export.sh`) keeps existing `functions/*.c` and only
re-generates headers and indexes, so re-runs after renaming functions are quick.

## Renaming / adding names
Names come from three JSON files in `research/pc/code/` and are applied on every export:
- `ai2c_functions.json`: the AI2C table in `.data` (0xAEC0D0.., 2 105 × `{u32 key, fn, name}`),
  scanned by `scan_ai2c.py`.
- `ai2c_triggers.json`: the trigger table (167 × `{file, index, fn, name}`).
- `extra_names.json`: `{"0xaddr": "Name"}`: hand-recovered engine function names. **Add to
  this file** when you identify an engine function (e.g. `"0x53a370": "Weapon_DamageAtDistance"`),
  then re-run the export so the name shows up everywhere.
Addresses shared by many AI2C entries are stubs and are named `AI2C_shared_stub_<addr>`.

## Why REA is not used for the PC exe
REA (`morluto/rea`, installed under `KongPS2/REA`) drives Hopper/Ghidra through MCP. Its Ghidra
provider on Windows admits **x86-64 PE only** (`REA/docs/windows-ghidra-p0.md`), and there is
no fork adding PE32; `KingKong8.exe` is a 32-bit PE. Ghidra itself handles PE32 fine, so this
project calls Ghidra headless directly (`tools/ghidra/`) and REA stays available for the PS2
ELF and for its evidence-bundle tooling. The capstone lifter `research/pc/code/lift.py` is the
older fallback; the Ghidra decompilation supersedes it.

## Reading tips for the decompiled code
- MSVC 2003, `-O0`, x87 floats: `fVar1 = (float)(double)...` casts are noise; the compare
  chains `if ((fVar1 < 0.0) == (fVar1 == 0.0))` are `>=`. Ghidra's `x87 pseudo-registers` etc. are x87
  register artifacts.
- `this` for AI2C functions is the AI instance struct. `*(int *)(this + 0x734)` is a model
  variable: look it up in `research/pc/code/ova/models.json`.
- `unnamed 0x004xxxxx` low addresses are engine/runtime; `0x57..0x5d` is Jack (`H_`); `0x83..0x86`
  Raptor; `0x49` KT (Kong-level V-Rex); `0x76..0x80` triggers.
- Strings in the header line list what the function logs or looks up: a fast way to tell
  what an unnamed engine function does.

## Annotated C: `tools/kb_annotate.py`
Resolves raw offsets to model-variable names so you can read `(param_1 + 0xacc /*i_etat_courant*/)`
instead of hex.
```
python3 tools/kb_annotate.py H_callback_tir          # one function, annotated C on stdout
python3 tools/kb_annotate.py 0x5ccc70 --model jack   # force a model (univers|jack|m331_...)
python3 tools/kb_annotate.py --all                   # writes kb/annotated/<same file name> (405 functions today)
python3 tools/kb_annotate.py --aliases               # global-pointer aliases found in the KB
```
Model by prefix: `H_` Jack (m733), `GG_/GST_/Trig*/IW_` Univers (m334), `IntMIG_` m331. Others: none
unless `--model`. Names come from `code/ova/models.json`, array extents from `code/ova/mNNN_*.txt`.
Key findings (details in the tool's docstring):
- The `DAT_00b9xxxx` globals are pointer caches filled by `fn@0x402080(<key>)+0x40`. Key
  `0x72006b76` (69 globals, e.g. `DAT_00b99308`) is the real Univers instance: m334 offsets fit
  unshifted. Key `0x3d0098b3` (90 globals incl. `DAT_00b99358`, `b98ca4`, `b990d8`, `b99920`) is the
  weapon/ammo/slot object (`+0x3344` slot0, `+0x342c` mags, `+0x4758` clip size...). Its model is not
  among the 14 dumped, so m334/m733 names must NOT be applied to it; only a small curated `G.` table is.
- `GG_` functions run on their own object (keys 0x2600xxxx); their m334 labels are unverified.
- Array hits show as `name[+0xDELTA]` and are weaker than exact hits (some dumped counts overlap).

## PS2 names
`tools/ps2/ai2c_names_ps2.py` recovers the AI2C name tables from `SLUS_213.11` (single PT_LOAD, vaddr = file offset + 0xFFF00;
code below 0x640000, string pool above). Layouts (little endian):
- function table, 12-byte rows `{u32 fn, u32 name_ptr, u32 key}` at 0x62db84..0x632774, **1621 rows**
  (PC order is `{key, fn, name_ptr}`). Keys differ between builds (0/1621 equal): match by name only.
- trigger table, 16-byte rows `{u32 file, u32 index, u32 fn, u32 name_ptr}` at 0x635400..0x635df0, **160 rows**
  (same as PC; `file` keys are equal across builds). Seven more trigger names (Trig_CINE, TrigExec_Simple, TrigTest_Simple,
  TrigExec_Spawner, TrigExec_FX, TrigExec_InteractiveObject, TrigSound) sit in the function table.
- Match vs PC: 1621 of 2105 function names and 167 of 167 trigger names = 1781 unique names; 484 PC-only (script libs/
  characters not shipped on PS2, e.g. Museum, Worm; R_init, KK, KNative exist only as plain strings), 0 PS2-only.
- Outputs: `/home/claude/kkps2/ai2c_functions_ps2.json` (name,key,addr,entry), `ai2c_triggers_ps2.json`,
  `ai2c_pc_only_names.txt`, and `tools/ps2/pc_ps2_map.tsv` (name, pc_addr, ps2_addr).
- Build the PS2 KB: import `SLUS_213.11` into Ghidra (language `MIPS:LE:32:R5900` / EmotionEngine plugin if installed, else
  `MIPS:LE:32:default`; image base 0x100000, do not rebase), run auto-analysis, then
  `analyzeHeadless <proj> KKPS2 -process SLUS_213.11 -noanalysis -scriptPath tools/ps2 -postScript ApplyPS2Names.java /home/claude/kkps2 /home/claude/kkps2/applied.tsv`,
  then a KKExport variant (copy `tools/ghidra/KKExport.java`, read the `_ps2.json` files). Not run yet.
- Parity of constants/offsets: see `docs/ps2_parity_probe.md` (PC float literals of 4 AI functions all present on PS2).
