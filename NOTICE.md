# Notice: no game content in this repository

This repository contains **only original source code, documentation and tooling** written for this project.

It does **not** contain, and must never contain:

- the game executable (`KingKong8.exe` or any other), a PS2 ELF, BIOS or ISO image;
- the game's archives (`*.bf`), raw or decoded level streams (`ff00xxxx.bin` / `.dec`), texture banks;
- models, textures, animations, audio, video, screenshots or any other asset taken from or rendered with the game;
- decompiled game code (the static-analysis knowledge base stays on the developer's machine).

What it does contain: Rust and Python code that **reads the user's own installed copy** of the game at run time and rebuilds what the game needs locally, plus written descriptions of how the original gameplay works (numbers, rules, file-format notes, with references to function addresses).

Users must supply their own legally obtained copy of *Peter Jackson's King Kong: The Official Game of the Movie* (PC Gamer's Edition).

*King Kong* and all related names, characters and assets are trademarks and/or copyrights of their respective owners (Universal Pictures, Ubisoft and others). This is an unofficial, non-commercial fan project with no affiliation to, or endorsement by, those owners.

## For contributors

`tools/check_release.py` scans the tree and fails on asset file types, oversized files and decompiler output markers. It runs in CI on every push and pull request. Never commit anything produced from the game's data: extracted assets go in `assets/`, `assets_dev/` or `game_assets/`, which `.gitignore` excludes.
