# Modding

Mods are folders in `mods/` next to `kk-fps.exe` (set `KK_MODS=<folder>` to use another place).

```
mods/
  my_mod/
    mod.json          required
    assets/           optional: files that replace game assets
    tunables.json     optional: presentation values
```

## mod.json

```json
{ "name": "My mod", "version": "1.0", "author": "me", "description": "what it does", "priority": 10 }
```

`priority` decides which mod wins when two ship the same file (higher wins). Mods are enabled when first found;
switch them in the F10 menu (saved in `kk_settings.json` under `mods`). Changes apply on the next start.

## assets/

Any file whose relative path matches a game asset replaces it, for everything that loads assets: models
(`kong/kong.glb`, `level03e/level03e_v2.glb`), textures inside glbs, sounds (`sounds/...wav`), the sound table
(`sound_defs.json`), Kong's fur (`kong/fur_detail.png`, `kong/fur_head.png`, `kong/kong_fur_rli.bin`), level
collision (`level03e/level03e_v2_collision.json`), animation tables, creature manifests. The asset folder of the
game is built by `kk-extract` from the player's own copy; look there for the paths.

**Do not distribute files taken from the game.** Mods should hold the modder's own work (new textures, models,
sounds, data), or patches the player applies to files built from their own copy.

## tunables.json

```json
{ "fog_density_scale": 0.85, "sun_intensity_scale": 1.1, "ambient_scale": 1.0, "rain": 1, "time_scale": 1.0 }
```

| Key | Default | Effect |
|---|---|---|
| `fog_density_scale` | 1.0 | scales the scene fog density |
| `sun_intensity_scale` | 1.0 | scales the directional lights |
| `ambient_scale` | 1.0 | scales the ambient light |
| `rain` | 1 | 0 turns the rain and its ambience off |
| `time_scale` | 1.0 | slow motion / fast forward (0.05–4) |

An example lives in `docs/examples/mods/example_tweaks` (copy it into `mods/` to try it).
