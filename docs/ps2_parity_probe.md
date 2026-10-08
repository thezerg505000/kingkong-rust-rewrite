# PS2 parity probe (float constants)

Method (pure Python, no Ghidra): `tools/ps2/ps2_parity_probe.py` does a linear sweep of a PS2 function
(start = PS2 address from `tools/ps2/pc_ps2_map.tsv`, end = next AI2C-table function address), tracks
`lui/ori/addiu` register constants (gp from `.reginfo` = 0x662370), and records a float whenever a constant
reaches `mtc1`, `lwc1`, or a `lui+ori` pair. Then the float literals in the PC decompiled C
(`kb/functions/<name>_<addr>.c`, 0.0 excluded) are compared with that set. Constants the sweep could not resolve exactly (0.9, 0.33) were confirmed only by a matching `lui hi16`
(weaker: the low half was not checked; flagged in the table). Branch-free linear tracking is an approximation; it can miss, not invent.

| Function (PC -> PS2) | PC float literals | found on PS2 |
|---|---|---|
| H_exec_select_action 0x587bd0 -> 0x2e4d40 (13280 B) | 22 | 22 (21 exact via mtc1/lui+ori, 0.9 by `lui 0x3f66` at 0x2e80c8) |
| PNJ_Raptor_exec_select_action 0x83a950 -> 0x538320 (6128 B) | 13 | 13 (0.33 by `lui 0x3ea8` at 0x539770) |
| H_callback_tir 0x5bbd10 -> 0x2c33b0 (9456 B) | 19 | 19 |
| PNJ_Scorpion_exec_select_action 0x4fb090 -> 0x314b80 (1104 B) | 3 | 3 |

Requested constants in H_exec_select_action: 4.5 (mtc1 at 0x2e61dc, 0x2e61e8), 2.5 (0x2e5d54, 0x2e7d18),
1.5 (0x2e5ce8, 0x2e5df8, 0x2e62cc) are all present, plus 2.0 and 0.1 (the `speed = x*2.0/0.1 + 2.5` expression
at PC line 1124). So the PC speed multipliers 4.5/2.5/1.5 are the same on PS2.

Weapon_DamageAtDistance (PC 0x53a370): the PC function has no float literals (it returns 1000 when
`flags(+0xbf0) & 2`, else `ftol(...)` from per-slot distance tables at +0x46b0 and +0x4704, squared compare).
Search of the whole PS2 image for a function containing `lw ..,0xbf0`, `addiu ..,0x3e8` (1000), `lwc1 ..,0x46b0(..)`
and `lwc1 ..,0x4704(..)` finds exactly one: PS2 0x467450 (1004 B, `lw` at 0x467798, `li 1000` at 0x4677a4,
`lwc1 0x46b0` at 0x4677b0, `lwc1 0x4704` at 0x4677c4), called once from 0x463bc0 (inside `H_LocalLib2`..`SIG_` region, no AI2C
name). It is NOT called from PS2 `H_callback_tir` directly (PC's only caller), so the call structure differs (inlined or
reached through the H_ lib callback). The struct offsets 0x46b0/0x4704/0xbf0 are identical on both builds,
which is a strong layout-parity indicator. Applied as an extra name in `kkps2/extra_names_ps2.json` (candidate, not proven).
