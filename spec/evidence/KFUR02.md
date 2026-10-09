# KFUR02 — Kong's fur length mask (RLI alpha) and the body GEO normals

Follow-up to the shell-fur port (KFUR, `kk_fps/src/kong_fur.rs`). Two bugs the user reported: Kong's face was
covered in fur, and his body had none while the arms did.

## Per-vertex fur length [C]

* PC fur vertex shader `vsfur.hlsl` (the shader table entry is referenced by `FUN_00a054b0`):
  `Input.RLI.a = 1.0 - Input.RLI.a; fScale = g_fFurNormalOffset * Input.RLI.a; Position += Normal * fScale;
  TexCoord0 += g_vFurTextureOffset`. The shell offset is scaled per vertex by **1 − alpha of the vertex's RLI
  colour**.
* `g_fFurNormalOffset` per shell comes from `FUN_00a1c990` (len/100/layers, as already ported); the RLI is not in
  the GEO (Kong's GEOs carry no vertex colours) but in the GAO's visual: key field `rli` of `S_Kong_Weta_Def.gao`,
  `S_XE_KongBrasG/D.gao`, `S_Kong_Weta_Def_TeteHDef01.gao` (07D loader emulation, `recs07.pkl`).
* Record format (RLI_FINDINGS, loader cb `0x95dd50`): `u32 size; "RLI\x80"; u32 count; u32 colour[count]`, one
  D3DCOLOR per GEO vertex; the record follows each GEO in the stream. Counts match the GEOs: body 1301, arms 318 / 376,
  head 2025 (ff00018c offsets 0xa8efe7, 0xad24c9, 0xaea7f5, 0xb22607; same alphas in 07D ff00f858).
* Alpha histograms (32-wide bins): body 266/286/49/143/27/86/5/439 → about half the body has long fur, the rest
  (chest, palms, soles) short or none; head 1622 of 2025 vertices ≥ 224 → only scalp, cheeks and jaw line are
  furred (12 % of the head's render vertices more than half length). RGB is 0 (no baked light on Kong).
* Implementation: `kk_extract` recipe output `kong/kong_fur_rli.bin` (one byte per kong.glb render vertex, the
  alpha through the same `flatten` order; test `kong_fur_rli_matches_python`), `kong_fur.rs` copies each part
  mesh with `1 - a/255` in the vertex colour alpha, `kong_fur.wgsl` scales the shell offset by it and discards
  shells where it is ~0.

## Body normals [C measurement, L fix]

* `S_Kong_Weta_Def` stores normals that disagree with its own triangles on 543 of 1301 vertices (median dot with
  the area-weighted geometric normal 0.3; mostly the upper torso), while the arm GEOs agree exactly (1.0) and the
  head nearly (0.96). The shells, pushed along those normals, went inside the torso: no body fur, and the torso
  lit as if facing away.
* Geometric normals point outward as often as the arms' do (78 %), so `kong_fur::weld_smooth_normals` rebuilds the
  body's normals from its triangles (welded across UV seams) at load [L]. How the engine itself treats that normal
  array (a different encoding, a skinning-space quirk) is **open**.

## Not done

* The dynamic part of the fur modifier (k14..k24 strand stiffness / velocity buffers).
* The `Coque*` simple-fur shells (textures not in the 05C bank).
