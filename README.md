# Peter Jackson's King Kong — Rust Rewrite

A clean-room style, static re-implementation of **Peter Jackson's King Kong: The Official Game of the Movie (2005)** in Rust, built on [Bevy](https://bevyengine.org/).

> **No game files are included in this repository, and none ever will be.**
> To run it you need **your own legally obtained copy** of the PC *Gamer's Edition*. The launcher rebuilds every model, texture, animation and sound it needs from *your* installed game files, on *your* machine.

The original game ran on Ubisoft's Jade engine. This project recovers its gameplay rules (weapons, damage, AI states, Kong's combat, fury, the V-Rex fight, creature behaviour, and so on) by **static analysis only**: reading the game's data and executable code. The original game is never launched or traced. Every recovered number in the code and docs is tagged `[C]` (read from code/data), `[L]` (inferred) or `[G]` (a tuned guess). The PS2 release (SLUS-21311) is the reference for behaviour parity.

## Status

Early but playable.

| Area | State |
|---|---|
| **Jack slice** (level 03E) | First-person Jack vs the V-Rex: the original weapon table, damage bands, shotgun pattern, reloads, wound model, the level geometry and lighting |
| **Kong slice** (level 05C marsh, "Kong vs first T-Rex") | Switch Jack ⇄ Kong with **Tab**. Kong AI fights the rex using his full move set: punch chain, repel, downward strike, counter lunge, grab / strike / throw, chest pound and fury, KO and the jaw-break finisher. Includes the shell fur ported from the engine's fur modifier, rain, fog and water splashes |
| **Creatures** | Raptors, compies, brontosaurus, crab, bats, scolopendras, spider, swamp crawler; a flat test area for batches |
| **Game logic** (`kk_mechanics`) | About 200 unit tests; a 240-entry mechanics ledger (`spec/`) |
| **Asset rebuild** (`kk_extract`) | Rebuilds the 03E level, characters, creatures, sounds and FX from the user's game files. *Levels 05C/07D are not yet rebuilt by the extractor* (see `docs/HANDOFF.md`) |

## Requirements

- Your own copy of **Peter Jackson's King Kong – Gamer's Edition (PC)**. The launcher needs `KKMaps.bf`, `KKTextures.bf` and `Sound_Common.bf` from the install folder.
- [Rust](https://rustup.rs/) (stable, 2021 edition) and a GPU with Vulkan, DirectX 12 or Metal.
- Windows 10/11 is the main target. Linux builds too; you need `libasound2-dev` and `libudev-dev` for audio and gamepads.

## Build and run

```sh
git clone https://github.com/thezerg505000/kingkong-rust-rewrite
cd kingkong-rust-rewrite
cargo build --release
```

Windows, all in one go:

```bat
scripts\windows\build_release.bat
```

This creates `dist\KingKongRecompiled.exe` (the launcher) and `dist\bin\kk-fps.exe` (the game). Then run:

```bat
dist\KingKongRecompiled.exe --game "C:\Program Files (x86)\Ubisoft\Peter Jackson's King Kong - Gamers Edition"
```

On the first start the launcher checks your game folder, then rebuilds the assets into `dist\assets\` (this takes a few minutes, once). After that it starts the game. The folder is remembered in `game_path.txt`.

| Launcher option | Meaning |
|---|---|
| `--game <dir>` | Your King Kong install folder |
| `--scene testarea` / `--scene swamp05c` | Choose a scene (default: the 03E Jack slice) |
| `--batch <name> --out <dir>` | Run a scripted test batch with screenshots and a JSON report |
| `--rebuild-assets`, `--extract-only` | Asset rebuild control |

The `.bat` shortcuts in `scripts/windows/` (`Play_Jack_03E.bat`, `Play_Kong_Marsh_05C.bat`, `Play_TestArea.bat`, `Watch_Kong_Fight_05C.bat`) are copied next to the exe by `build_release.bat`.

## Controls

| Action | Keyboard / mouse | Gamepad |
|---|---|---|
| Switch Jack ⇄ Kong | Tab | Select |
| Move | W A S D | Left stick |
| Jack: fire / aim | Left / right mouse | RT / LT |
| Kong: attack | Left mouse | X / West |
| Kong: jump / roll | Space | A / South |
| Kong: special (repel) | Q | Y / North |
| Kong: cancel | E | B / East |
| Jack: pick up a spear / bone (racks, bone pile, spears on the ground) | E | A / South |
| Jack: throw the spear (aim + fire) / stab (fire) | Right + left mouse / left mouse | LT + RT / RT |
| Jack: drop the spear | G | Y / North |
| Respawn everything (nothing respawns by itself) | F8 | D-pad up |
| Jack: get up after death (checkpoint) | Enter | Start |

## Repository layout

```
crates/
  kk_mechanics/   pure game logic ported from the original (no engine, no assets); unit tested
  kk_fps/         the playable Bevy slice (Jack and Kong scenes, rendering, FX, camera, test batches)
  kk_extract/     rebuilds assets from the user's own game files (BF archives, LZO, Jade GEO/TRL/textures)
  kk_launcher/    KingKongRecompiled launcher: finds the install, runs the extractor, starts the game
spec/             gameplay spec, mechanics ledger (mechanics.yaml -> STATUS.md), evidence docs per mechanic
docs/             handoff and playbook, engine map, knowledge-base guide, audit, research log
tools/            ledger and knowledge-base helpers, Ghidra export scripts, PS2 name tools, release checker
scripts/windows/  build and play shortcuts
```

Start with [`docs/HANDOFF.md`](docs/HANDOFF.md) to see where the project stands and what comes next. [`AGENTS.md`](AGENTS.md) has the working rules for contributors, human or AI.

## Testing

```sh
cargo test -p kk-mechanics          # game logic, seconds, no assets needed
cargo test -p kk-launcher
cargo test -p kk-extract            # asset tests skip unless KK_TEST_DATA points at extracted test data
```

## Legal

This is an unofficial, non-commercial fan project. It is not affiliated with or endorsed by Ubisoft, Universal Pictures, Peter Jackson or Wingnut Films. *King Kong* and all related names, characters and assets are the property of their respective owners. The repository contains **only original source code and documentation**: no executables, archives, models, textures, audio, video or decompiled code from the game. See [NOTICE.md](NOTICE.md).

## License

The source code is dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. This license covers this project's own code and documentation only, never the original game or its assets.
