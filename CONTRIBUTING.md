# Contributing

Thanks for your interest! Before anything else, read [NOTICE.md](NOTICE.md). **Do not commit game files or anything derived from them** (assets, decoded streams, screenshots, decompiled code).

## Ground rules

- **Static analysis only.** Recover behaviour by reading the game's data and code. Do not trace the running game.
- **Tag every number.** Use `[C]` (read from code or data, with the function address or record), `[L]` (inferred) or `[G]` (guessed or tuned). See `docs/PLAYBOOK.md`.
- **Logic goes in `kk_mechanics`.** It is pure Rust with unit tests and no engine or assets. Presentation goes in `kk_fps`.
- **Keep the ledger current.** When a mechanic changes state, run `python3 tools/ledger.py set <ID> status=...` and then `python3 tools/ledger.py render`.

## Workflow

1. Fork and create a branch.
2. `cargo fmt`, `cargo clippy`, `cargo test -p kk-mechanics`.
3. `python3 tools/check_release.py .` must report OK.
4. Open a pull request that describes the evidence (addresses, records) behind any recovered behaviour.

## Where to start

`docs/HANDOFF.md` lists the current state and the next tasks. `spec/STATUS.md` lists every mechanic and its status.
