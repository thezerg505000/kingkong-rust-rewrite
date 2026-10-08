# Subagent brief (read fully before starting)

You are working on KongPS2Recompiled: a static re-implementation of King Kong (2005) in Rust.
The orchestrator assigns you one mechanic group. Read, in this order:
1. `docs/PLAYBOOK.md` (the loop, the rules, the evidence-doc format)
2. `docs/ENGINE_MAP.md` (which function prefixes own what)
3. `docs/KNOWLEDGE_BASE.md` (how to query the decompiled code)
4. `spec/mechanics.yaml` entries for your IDs (`python3 tools/ledger.py show <ID>`)

Paths in the cloud session:
- framework repo additions: `/home/claude/kkframe` (tools/, docs/, spec/)
- Rust workspace: `/home/claude/kkbuild` (crates/kk_mechanics = where ported logic goes;
  crates/kk_fps = the Bevy slice, do not edit unless told)
- knowledge base: `/home/claude/kkpc/kb` (use `KK_KB=/home/claude/kkpc/kb python3 /home/claude/kkframe/tools/kb.py ...`)
- model variables / data: `/home/claude/kkpc/code/ova/` (`models.json`, `*.txt`, `uni_init_all.txt`),
  older findings: `/home/claude/kkpc/code/gameplay_spec.md`
- the exe for byte-level checks: `/home/claude/kkexe/KingKong8.exe`

Hard rules:
- Static analysis only. Never run the game or an emulator.
- Every number you write down gets `[C]`, `[L]` or `[G]` and the function@address it came from.
  A `[C]` must be visible in the decompiled C you cite. Do not upgrade an inference to `[C]`.
- Write `spec/evidence/<ID>.md` for each mechanic you research (format in PLAYBOOK § 3). One
  file per ID; if several IDs share one mechanism, write the main one fully and make the
  others short with a pointer.
- Rust goes in `/home/claude/kkbuild/crates/kk_mechanics/src/<module>.rs`, pure Rust, no
  dependencies, with unit tests pinning recovered numbers. Build with
  `cd /home/claude/kkbuild && cargo test --offline -p kk-mechanics`.
  Register new modules in `src/lib.rs`.
- Update the ledger with `python3 /home/claude/kkframe/tools/ledger.py set <ID> ...`
  (it validates and regenerates STATUS.md). Statuses: RESEARCHED (doc only), PARTIAL,
  IMPLEMENTED (Rust exists), VERIFIED (test pins a recovered value).
- If you identify what an unnamed `unnamed-fn-address` engine function does, add it to
  `/home/claude/kkpc/code/extra_names.json` as `"0xaddr": "Name"` (do not rename files in the KB).
- Do not ask questions. Decide, write the decision in the Gaps section, continue.
- Finish with a summary of at most 15 lines: IDs touched, new statuses, the 3 most important
  numbers found, and the open gaps.
