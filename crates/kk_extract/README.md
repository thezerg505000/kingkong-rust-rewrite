# kk_extract

Rebuilds the game's assets from the **user's own copy** of *King Kong* (2005) PC Gamer's Edition.
The released game ships no game data; on first run the exe calls `kk_extract::build_all` (or the user runs the
`kk-extract` CLI) and the asset tree is generated from `KKMaps.bf` and `KKTextures.bf`.
Pure Rust, no Python, no Ghidra, no network. Dependencies: `serde_json`, `png`, `walkdir`.

```
kk-extract --game "<install dir>" --out <assets dir> [--only <asset>] [--force]
kk-extract --list
```

```rust
let report = kk_extract::build_all(game_dir, out_dir, &mut |p| println!("[{}/{}] {}: {}", p.step, p.total, p.asset, p.message))?;
if kk_extract::is_complete(out_dir) { /* nothing to do on later runs */ }
```

`build_all` is idempotent: `<out>/.kk_extract.json` records, per asset, a hash of the recipe plus the size/mtime
of the two `.bf` files; up to date outputs whose files still exist are skipped (`--force` / `Options::force`
rebuilds). Outputs are written to a temp name and renamed, so an interrupted run never leaves a half-written glb.
One failing asset does not stop the others (`Report::failed`). The `.bf` files are searched
case-insensitively in the install dir and up to three levels below it.

## What it reads and writes

| input (game files) | output (asset dir) |
|---|---|
| `KKMaps.bf` `ROOT/Bin/ff0003eb.bin` (T-Rex chase world: Jack FPS arms, weapons, arms clips) | `jack_fps_arms.glb` (30 joints, 101 clips, WeaponSocket, textured), `jack_fps_luger.glb`, `jack_fps_tommygun.glb`, `jack_fps_shotgun.glb`, `jack_fps_sniperrifle.glb` |
| `KKMaps.bf` `ff00018c`, `ff001b17`, `ff000213`, `ff00f858`, `ff009d63`, `ff001793` (T-Rex mesh/rig and its 123 clips) | `trex_inplace.glb` (27 skinned joints, 123 clips with the root motion stripped, textured), `trex_rootmotion.json` (per clip travel and speed, used by `kk_fps`) |
| `KKTextures.bf` banks `ff8003eb`, `ff80018c` | PNGs embedded in the glbs (RGB diffuse, normal map with rebuilt Z) |
| `KKMaps.bf` `ff00018c` / `ff0003eb` / `ff001793` / `ff002001` + bank `ff80018c` (Kong, Ann, action table) | `kong/kong.glb`, `kong/ann.glb`, `kong/*_rootmotion.json`, `kong/kong_actions.json` (54 labelled actions, in-place clips) |
| `KKMaps.bf` creature streams (raptor, compy, raptor+Kong, brontosaurus, crab) | `creatures/<name>.glb` (+ `_actions.json`, `_rootmotion.json`), `creatures/manifest.json` |
| `Sound_Common.bf` (`.smd` definitions, MS-ADPCM `.wav` / `.waa` / `.wac`) | `sounds/*.wav` (16 bit PCM, **not** ogg) and `sound_defs.json` (`files` now end in `.wav`) |
| `KKTextures.bf` banks `ff8003eb`, `ff801b17`, `ff80018c` | `fx/*.png` (18 sprites), `sky/ciel_1f007698.png`, `rex_nrm_*.png`, `rex_mr_*.png` |

`src/manifest.json` is the recipe list (stream key, GEO offset, rig prefix, texture bank and chunk ordinals);
`data/clips.json` lists the clip tracks (stream key, offset) with their labels. Those are positions/names in the
retail files, not game data. They are valid for the retail PC Gamer's Edition files only.

## Modules and the Python tool each one ports

| module | ports | notes |
|---|---|---|
| `bf` | `tools/bf.py` | BIG v36 index (chained FAT blocks, dirs, names), read by key / path. Unit tests on synthetic archives (several FAT blocks, shared keys, unnamed files, directory cycles, bad headers) |
| `lzo` | `tools/lzo1x.c` | LZO1X decompressor, bounds checked (errors, never panics). Tested on a random *valid stream generator* that emits every token form, and differentially against the compiled C tool |
| `stream` | `tools/extract_bin.py`, framing loop of `lzo1x.c`, `tex_decode.unstream/chunks` | blocks `{ulen, clen, data}` + 4 zero bytes, raw when `clen >= ulen` |
| `records` | `tools/jadegao.py`, GRO notes of `mesh_findings.md` | named GAO records (regex-free scan, same results), GRO chain walker, GEO magic scan |
| `geo` | `tools/jadegeo.py`, `meshutil.flatten` | GEO incl. skin block (bf16 weights) and the OK3 search heuristic (positions located by "n positions followed by n unit normals"); render-vertex flattening |
| `skel` | `tools/jade_skel.py`, `build_anim_glb.load_rig` | hierarchy from the GAO payload before each bone name (anim_findings §1), parent = key - base, helper bones skipped, root fixed against the skin inverse-bind |
| `anim` | `tools/anim_parse.py`, `find_trls.py`, `clipfeat.py` | TRL track lists -> events/keys (anim_findings §2), full-stream TRL scan, clip features |
| `texture` | `tools/tex_decode.py`, PNG parts of `texture_bind.py` | Xenos 2D untiling, DXT1/3/5, DXN (ATI2), A8R8G8B8, L8, bank chunk ordinals (`idxNNN`), PNG encoders for diffuse / normal |
| `glb` | `tools/glbwriter.py`, writers in `build_anim_glb.py`, `make_game_assets.py` | glTF 2.0 .glb: skinned meshes, skins, animations, embedded PNGs |
| `build` | `export_meshes.py`, `build_anim_glb.py`, `texture_bind.py`, `make_game_assets.py` (graft, labels, WeaponSocket), root-motion strip | recipes -> files; `Source` trait abstracts where streams/banks come from |
| `source` | - | opens the game's `.bf` files (maps, textures, `Sound_Common.bf`), LRU cache of decoded streams |
| `creature` | `export_creature.py`, `creature_rig.py`, `kk_trl.py` | Kong / Ann: bone-chain hierarchy (`hier2`), skin hypotheses validated by skin matrices, lenient TRL parser, Kong action table (kit key -> record -> TRL), in-place root removal, glb + `_actions` / `_rootmotion` JSON |
| `kkc` | `kkc_lib.py`, `kkc_geo.py`, `kkc_anim.py`, `kkc_export.py` | raptor / compy / raptor_kong / brontosaurus / crab: rig with 3 skin hypotheses, pelvis in-place, labels, `creatures/manifest.json` |
| `sound` | extraction of the PS2-era `kksnd` tree | `.smd` parser, MS-ADPCM decoder (ffmpeg compatible), PCM wav writer, `sound_defs.json` |
| `images` | `tex_decode.py`, `texmap2.pal_img`, rex map scripts | standalone PNGs: Xenos copies, paletted FX sprites, V-Rex normal / roughness maps |

Root-motion strip (how `trex_inplace.glb` / `trex_rootmotion.json` were made): the pelvis translation track also
carries the `JadeActor` travel, so `pelvis(t) -= actor(t)` (linear interpolation of the actor keys), the actor
track is zeroed, and per clip the actor displacement (`last - first`, Jade axes and glTF axes `(x, z, -y)`),
duration and speed are written to `trex_rootmotion.json`. Verified against the existing files on 45 clips.

## Tests

Without game data the unit tests run (`cargo test -p kk-extract`): BF layout, LZO (random streams), framing,
record scanning, TRL parsing, texture primitives, glb writer, clip tables, root-motion maths, CLI flags.

With `KK_TEST_DATA=<dir>` the parity tests also run (they `skip` when it is unset, so release CI needs nothing).
Prepare the directory with `tests/tools/make_test_data.sh <research/pc dir> <dir with reference glbs> <out dir>`,
which runs the original Python tools (`make_reference.py`, `make_tex_reference.py`) on the decoded streams
`ff0003eb.dec`, `ff00018c.dec`, `ff001793.dec`, `ff001b17.dec` and links the existing glbs as references.
Use `--release` (the debug build needs ~3 minutes for the track-list scan):

```
KK_TEST_DATA=... cargo test --release -p kk-extract
```

| test | what is compared with the Python tools | result |
|---|---|---|
| `parity::named_records_match_python` | all named GAO records of ff0003eb (787) and ff00018c (1376): name, offsets, size, pre-header | identical |
| `parity::geo_matches_python` | every GEO header (292 + 406): parse success, counts, position/normal/UV sums, element and triangle sums, skin lists and matrices, flattening | 281/292 and 389/406 parse, the same ones Python parses (113 + 150 skinned, 164 + 227 with OK3) |
| `parity::skeleton_matches_python` | arms (35 bones) and T-Rex (32 bones): names, parents, skin ids, parent keys, local matrices | identical; chain product vs inverse skin matrix 4.4e-7 / 2.2e-6 (as in anim_findings) |
| `parity::trl_scan_matches_python` | full-stream track-list scan, 4 streams | 278 / 674 / 591 / 542 lists identical |
| `parity::clips_match_python_and_catalog` | 101 arms clips and the 45 rex clips whose streams are available: tracks, event counts, key sums, lengths, features | identical; frame counts equal the catalog |
| `parity::texture_decoders_match_python` | synthetic Xenos chunks, 6 formats, odd sizes, vs `tex_decode.decode_xe` | bit identical (12 images, plus the one case Python rejects) |
| `build_e2e::jack_arms_matches_python_glb` | rebuilt `jack_fps_arms.glb` vs the current asset: nodes, hierarchy, skin, ibm, every vertex attribute, indices, 101 clips (names, targets, times, values, extras) | match |
| `build_e2e::weapons_match_python_glb` | 4 weapon glbs | match |
| `build_e2e::trex_inplace_matches_python_glb` | rig, mesh, 45 in-place clips, 45 rootmotion entries | match |
| `build_e2e::png_binding_matches_reference_images` | diffuse/normal PNG binders applied to the Python-decoded arms textures vs the images embedded in the current glb | match |
| `lzo_vs_c` | Rust vs the compiled `lzo1x.c` on 160 random LZO blocks | identical |
| `game_dir` | synthetic install (BF files built in the test): discovery, BF read, decode, textures embedded, idempotent rerun, `--force`, `--only`, failure isolation, CLI | pass |
| `creature_e2e` (9 tests) | Kong, Ann and the five creatures vs `kkassets/kong` and `kkassets/creatures`: rig, mesh, skin, every clip (names, times, values, extras), `_rootmotion` / `_actions` JSON, material texture names | identical (normal maps within 1 level: Python truncates, Rust rounds) |
| `sound_e2e` | synthetic `Sound_Common.bf` index built from the `kksnd` tree: 106 definitions vs `sound_defs.json` (`.ogg` -> `.wav`), 91 waves vs the ffmpeg decode | definitions identical, PCM sample-for-sample identical |
| `images_e2e` | the 8 paletted FX sprites rebuilt from bank `ff801b17` vs `fx/*.png`; the four Rex maps vs `rex_*.png` | pixel identical |

## Coverage of the assets the game loads

| asset | status | notes |
|---|---|---|
| `jack_fps_*.glb`, `trex_inplace.glb`, `trex_rootmotion.json` | done | parity with the Python glbs |
| `kong/kong.glb`, `kong_actions.json`, `kong_rootmotion.json`, `ann.glb`, `ann_rootmotion.json` | done | parity; Ann keeps the flat placeholder textures of the current asset |
| `creatures/*.glb` (5), `*_actions.json`, `*_rootmotion.json`, `manifest.json` | done | parity; the action label tables are embedded analysis data (`data/creature_meta.json`, `data/kong_labels.json`) |
| `sounds/*.wav`, `sound_defs.json` | done, format changed | the old `.ogg` came from an ad-hoc ffmpeg run; the same 95 waves + 3 ambience / thunder definitions are produced as PCM `.wav`. Bevy needs its `wav` feature (the `hound` crate, not vendored) or a custom decoder using `kk_extract::sound::decode_wav` |
| `fx/*.png` (10 from bank 03E, 8 paletted from `ff801b17`) | done | pixel identical; bank-03E copies rely on the texture parity test only (no decoded bank in the test data) |
| `sky/ciel_1f007698.png` | done | ordinal 75 of `ff8003eb` (not separately tested) |
| `rex_nrm_*.png` | done | pixel identical (Y flip, XY x 1.569, renormalised) |
| `rex_mr_*.png` | done, fitted | body roughness `round(255 - 0.8675 max(spec - 101.5, 0))`, head `floor(255 clamp(1 - 5/6 (mean(rgb) - 0.03), 0.4, 0.9))`: pixel identical, but the formulas are fitted to the existing maps, not read from code |
| `level03e/level03e_v2.glb` + collision | **missing** | see below |
| `level07d/level07d.glb` + collision | **missing** | see below |

Limits of the verification: the real `KKMaps.bf` / `KKTextures.bf` were not available in the cloud. The BF reader is
tested on synthetic archives built from the documented layout; the texture decoders are tested bit-exactly against the
Python decoder on synthetic chunks, and the texture *binding* against the images already embedded in the current
glb. The first run on a real install should be checked once with the expected sizes (arms glb ~3.9 MB, T-Rex ~7.9 MB).

## Not ported yet

* **Level worlds 03E and 07D** (`level03e/level03e_v2.glb` 27 MB + `_collision.json`, `level07d/level07d.glb` 56 MB + `_collision.json`;
  also `keys/*_bindings.json`, `spawns.json`, `atmos_*.json`). These need, in Rust:
  1. the **world-load emulation** (`keys/tools/sim.py`, `loop.py`, `eve.py`, `mdf.py`, `runall.py`; `lvl/*` for 07D, ~1100 lines): it replays the
     engine's resource-list callbacks over the whole 38-64 MB stream to recover the record table (GAO 0x95de20 with world matrices, GRO 0x9b2ac0,
     GRM material trees, RLI 0x95dd50 vertex lighting). Stream offsets in the keys are not stored in the stream, so this replay is the only source;
  2. the **texture key map** (`texmap2.py`, `mkkeymap_lvl.py`): load order of the keys anchored on ~229 cross-world identical images plus a DP
     (heuristic, `[L]`), and the paletted-texture decoder (done: `images::decode_pal`);
  3. `jadegeo2` / `meshutil.flatten` for world GEOs (the Rust `geo` module covers the character GEO subset; world GEOs carry the OK3 collision block and
     per-element material ids), the name -> class tables of `build_level.py` (`klass`, `DROP_RE`), ground-triangle / box collision export and the
     RLI -> `COLOR_0` / `_RLI_ALPHA` attributes.
  The Rust side already has the stream reader, record scan, GEO flattening, Xenos / palette decoders and the glb writer; the missing piece is item 1.
  Until then these two glbs have to come from the old assets.
* **Clip labels** are an embedded table (`data/clips.json`, taken from the current assets), not re-derived:
  `label_clips.py` needs forward kinematics of the arms (`arm_fk.py`, `arms_poses.pkl`) and heuristics. Kong / creature labels likewise
  (`data/kong_labels.json`, `data/creature_meta.json`).
  The clip offsets are also fixed in the table; `anim::find_trls` (the scan) is ported and tested but not needed at build time.
* `trex_src.glb` (with root motion), `dup_*`, `jack_fps_leg`, `jack_fps_tnt` exports; the `follow_materials` extras of the
  mesh glbs; `*.json` sidecars of `export_meshes.py`; `trex_tinted_synthetic.glb` / colour previews.
* Ann's textures are placeholders (same as the current asset); `atmos/*.png` copies are the same images as `fx/` and are not written.
* The OK3 collision block inside GEOs is still skipped by search (layout undocumented, see `mesh_findings.md`).
