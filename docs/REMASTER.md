# Remaster features

The rebuild renders the recovered 2005 scenes (03E courtyard, 05C marsh, 07D swamp) with Bevy 0.19. On top of
the reference-matched look ("Original" preset) it can switch on modern rendering, audio and mods. Every feature
is a setting in the in-game menu (**F10**), saved to `kk_settings.json` next to the executable
(`KK_SETTINGS=<file>` overrides). Nothing here changes the game data or mechanics.

## Graphics (F10 → GRAPHICS)

| Setting | Values | Notes |
|---|---|---|
| Preset | Original / Remaster / Custom | Original = the look matched to the user's reference frames; Remaster = everything below at sensible values |
| Upscaling | Native, AMD FSR 1.0 (Ultra Quality 77 %, Quality 67 %, Balanced 59 %, Performance 50 %), NVIDIA DLSS (Auto, DLAA, Quality, Balanced, Performance, Ultra Performance) | FSR is a WGSL port of FidelityFX FSR 1.0 (EASU + RCAS, MIT), runs on any GPU; DLSS needs a `dlss` build, an RTX GPU and the Vulkan backend |
| Sharpening | 0–100 % | RCAS with FSR, CAS with TAA / DLSS |
| Anti-aliasing | Off / FXAA / SMAA / TAA | TAA is replaced by FSR / DLSS when those are on |
| Ambient occlusion | Off / Low / Medium / High / Ultra | Bevy SSAO (GTAO) |
| Global illumination | Off (original ambient) / Sky probe | image-based diffuse + specular light from a sky probe built from the scene's fog colour and sun |
| Ray tracing | On / Off | Bevy Solari (see below); applies after a restart |
| Contact shadows | On / Off | screen-space contact shadows for the sun |
| Shadow quality | Low / Medium / High / Ultra | sun shadow map 1024–8192 |
| Tonemapping | Original (TonyMcMapface) / AgX / ACES / Blender Filmic / Khronos PBR Neutral | |
| Bloom, Depth of field, Motion blur, Vignette, Chromatic aberration | On / Off | |
| Volumetric fog + sun shafts | On / Off | 03E: the original distance fog is replaced by a ray-marched fog volume over the level, the key light is re-aimed at the cloudy sun the god ray beams from and casts shafts through the fog (needs its shadow map). Ray tracing also uses that sun as Solari's light |

Ray tracing leaves out of the Solari scene everything that follows the camera or is not opaque world geometry
(guns, held spear, cloud sky, sun disc, sprites, particles): those flickered the ray-traced light.

### Why not RTX Remix

RTX Remix replaces the renderer of DirectX 8 / 9 **fixed-function** games by intercepting their D3D9 calls.
This rebuild draws through wgpu (Vulkan / DX12), so Remix has nothing to hook; the MCP server for the Remix
Toolkit only helps develop the Toolkit itself. The rebuild therefore uses Bevy's own real-time ray tracer.

### Ray tracing (Bevy Solari)

* Build with `--features raytracing` (`build_dev.bat` does). Needs a GPU with hardware ray queries (NVIDIA RTX,
  AMD RDNA2 or newer, Intel Arc) and Vulkan or DX12; on other GPUs the menu entry stays without effect and the
  log says which GPU features are missing.
* ReSTIR direct + indirect light, ray-traced shadows and specular; shadow maps are switched off while it runs.
* The scene renders deferred while ray tracing is on, so the switch applies on the next start.
* Experimental, like Solari: only static level meshes enter the ray-tracing scene (Solari has no skinned
  meshes yet), so Kong, the rex and Jack's arms receive ray-traced light but do not cast ray-traced shadows.
* Not testable in the cloud build environment (no GPU): verified to compile, not to look right. First runs on a
  real RTX card may need tuning (sun intensity, emissive values).

### DLSS

* Build with `--features dlss` (`build_dlss.bat`). Build-time requirements, all from NVIDIA / LunarG, none
  redistributed by this project:
  1. the NVIDIA DLSS SDK v310.5.3 (`git clone --branch v310.5.3 https://github.com/NVIDIA/DLSS`), environment
     variable `DLSS_SDK` pointing at it;
  2. the Vulkan SDK (`VULKAN_SDK` set by its installer);
  3. LLVM / clang (bindgen).
* Run time: copy `%DLSS_SDK%\lib\Windows_x86_64\rel\nvngx_dlss.dll` (and `nvngx_dlssd.dll` for ray
  reconstruction) next to `kk-fps.exe`, and keep the DLSS licence text with it (DLSS SDK licence §9.5).
* A `dlss` build forces the Vulkan backend. Without an RTX GPU the menu shows "GPU/driver not supported".

### FSR 1.0 details

The main pass renders at the chosen scale into the corner of the full-size target (`MainPassResolutionOverride`,
the mechanism Bevy's DLSS uses); EASU reconstructs the full frame straight after the transparent pass, RCAS
sharpens it, and bloom, the god ray and tonemapping then run at full resolution. HDR input is wrapped in AMD's
invertible `c / (1 + max(c))` tonemap around both passes. Depth-based effects that read the depth buffer after
the main pass (depth of field, motion blur) see the scaled depth: switch them off with FSR.

## Audio (F10 → AUDIO)

The game's own sound definitions (decoded from `Sound_Common.bf`, `sfx.rs`) now play through the Firewheel
audio engine (`bevy_seedling`).

| Setting | Effect |
|---|---|
| Audio engine: Original | one voice per sound definition, definition volume, simple distance roll-off (as before) |
| Audio engine: Remaster | positional sounds become 3D voices with distance attenuation |
| 3D audio (HRTF) | binaural (IRCAM HRTF) for headphones; off = speaker panning |
| Environmental reverb | Freeverb send bus sized to the scene (stone courtyard 03E, open swamp 05C / 07D); Jack's own gunshots ring in it too |
| Sound occlusion | a ray through the level collision muffles voices hidden behind walls (low-pass 900 Hz) |
| Master volume | |

## Mods (F10 → MODS)

See `docs/MODDING.md`. Mods live in `mods/` next to the executable; each can replace any asset by relative path
and override presentation tunables. Switching a mod applies on the next start.

## Testing

The software-GL test runs (`KK_SOFTWARE_GL=1`) always use the Original preset, so the scripted batches keep
checking the reference look. FSR can be checked there with `KK_FORCE_FSR=<scale>` (see `fsr.rs`).
