#!/usr/bin/env python3
"""Annotate model-variable offsets in the decompiled C of the knowledge base.

  kb_annotate.py <name|0xaddr> [--model univers|jack|m331|...] [--no-arrays]
  kb_annotate.py --all [--model M]      write kb/annotated/<same file name> for every function
                                        that resolves at least one offset
  kb_annotate.py --aliases              print the global-pointer aliases found in the KB

Ghidra shows AI-model variables as raw offsets: `*(float *)(param_1 + 0x734)`. The variable
names per offset come from code/ova/models.json (+ code/ova/mNNN_*.txt for type/count, which
gives array extents). The tool rewrites `(expr + 0xNNN` into `(expr + 0xNNN /*name*/`, and
`self[0x2d4]` (int*/float* self) into `self[0x2d4 /*name*/]`.

Paths (override with env): KK_KB (default /home/claude/kkpc/kb or kb.py's default),
KK_OVA (default <KB>/../code/ova).

Which model applies (by function-name prefix; --model overrides)
  H_            jack     m733_i_etat_courant   (`param_1` / the object at *(DAT_00b420dc+0x40))
  GG_ GST_ Trig* IW_      univers  m334_v_joy_camera_normalized
  IntMIG_       m331_i_DBG_display_interaction  (deviation from the brief: 393 of 557 offsets used
                by IntMIG_ code are exact m331 names; m334 fits far worse)
  anything else none, unless --model is given.
`self` = `param_1`, plus every local that is only ever assigned from
`*(T *)(DAT_00b420dc + 0x40)` (the current object's AI instance) or from `param_1`; the inline
expression `*(int *)(DAT_00b420dc + 0x40) + 0xNNN` is handled too. Model names are valid only
when the function really runs on that model; the prefix table is a heuristic.

Global aliases (what `g_b99358 / g_b98ca4 / g_b990d8 / g_b99920` are) -- worked out, not assumed
  Each global `DAT_00b9xxxx` is a pointer cache filled at start-up by
      iVar1 = FUN_00402080(<object key>); DAT_00b9xxxx = *(undefined4 *)(iVar1 + 0x40);
  (FUN_00402080 = find object by key; +0x40 = its AI instance). The tool scans the KB for that
  pattern (cached in <KB>/index/aliases.json) and groups globals by key. Two keys matter:
    0x72006b76  69 globals (DAT_00b99308, DAT_00b98ce8, ...)  the Univers (GST_Global family is
                key 0x72006b7x). 156 of the 230 offsets used through these globals are exact
                m334 names (0x10d0 i_GST_Climb_NearestSightReach, 0x10d4 ao_GST_Climb_...,
                0x88e4 i_kong_camera_status), so m334 offsets apply UNshifted. UNIVERS.
    0x3d0098b3  90 globals incl. DAT_00b99358, DAT_00b98ca4, DAT_00b990d8, DAT_00b99920,
                DAT_00b99608 (Humain(), H_LocalLib, TrigTest_Simple, Munition set them). This is
                the object with the weapon tables: ammo/slot arrays at +0x3344, +0x342c, +0x39d4,
                +0x465c, +0x4758, +0x4804, cheat flags +0xbf0. It is NOT m334 and not m733: its
                offsets collide with unrelated Jack/Univers names (G+0x3390 is Jack
                i_stunned_begin but here weapon slot 1), so using those tables would label it
                wrongly. Its model is not among the 14 dumped, so only a small curated table
                (gameplay_spec.md [C], H_exec_read_joy 0x579f30, H_exec_loading_weapon 0x5ccc70,
                sub_53a370, sub_540fd0) is applied, tagged `G.`.
  GG_Exec_AppendWeaponName (0x696240) does not use these globals: it reads
  `*(DAT_00b420dc+0x40)+0x1e28` (weapon id) and `+0x1e24` (text buffer) of its OWN object (the GG
  object, key family 0x2600xxxx), so m334 names at those offsets (ao_Lances, i_CB_PosValidSrc)
  are probably wrong for it; treat GG_ annotations as unverified.
  H_exec_read_joy reaches G+0x3344 as `DAT_00b99358 + 0x3344 + self[1]*4` where self[1] is the
  player index (self = *(DAT_00b420dc+0x40) = Jack instance).
  Absolute forms `*(float *)0x00b9xxxx` (the global itself) are tagged `/*UniversPtr*/` or
  `/*GPtr*/` if they occur (none in the current KB).

Array ranges: an offset inside [off, off + cnt*elem) of an array entry (elem = 12 for type 25
vectors, else 4) is shown as `name[+0xDELTA]` (narrowest enclosing entry). Some dumped counts
overlap (e.g. m334 ao_precal_wp cnt 2000), so array hits are weaker than exact hits.
"""
import sys, os, re, json, glob, argparse

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
KB = os.environ.get("KK_KB")
if not KB:
    KB = "/home/claude/kkpc/kb" if os.path.isdir("/home/claude/kkpc/kb") else None
if KB:
    os.environ["KK_KB"] = KB
import kb as kbmod  # noqa: E402
KB = kbmod.KB
OVA = os.environ.get("KK_OVA") or os.path.join(os.path.dirname(KB), "code", "ova")

MODELS = {"univers": "m334_v_joy_camera_normalized", "jack": "m733_i_etat_courant"}
PREFIX_MODEL = [("H_", "jack"), ("GG_", "univers"), ("GST_", "univers"), ("Trig", "univers"),
                ("IW_", "univers"), ("IntMIG_", "m331_i_DBG_display_interaction")]

# Curated names for the weapon/ammo object (key 0x3d0098b3). [C] = gameplay_spec.md citing these
# functions; exact offsets only (per-character index 4p and per-weapon strides are added in code).
G_NAMES = {
    0xbec: "G.flags_bec", 0xbf0: "G.cheat_flags(bit1=one-hit,sub_53a370)",
    0x1c98: "G.actor_gao[p]", 0x3344: "G.weapon_slot0[p]", 0x3390: "G.weapon_slot1[p]",
    0x33dc: "G.weapon_slot2[p]", 0x342c: "G.mag_rounds[w*0x48+4p]",
    0x39d4: "G.reserve_ammo[w*0x48+4p]", 0x3f80: "G.reserve_max[w*0x48+4p]",
    0x465c: "G.weapon_range[w]", 0x46b0: "G.dmg_R1[w]", 0x4704: "G.dmg_R2[w]",
    0x4758: "G.mag_size[w]", 0x4804: "G.dmg_bands[w*12+{0,4,8}]",
}


def alias_cache():
    p = os.path.join(KB, "index", "aliases.json")
    if os.path.exists(p):
        try:
            return json.load(open(p))
        except Exception:
            pass
    pat = re.compile(r'(\w+) = FUN_00402080\((0x[0-9a-f]+)\);\s*(DAT_[0-9a-f]+) = \*\(undefined4 \*\)\(\1 \+ 0x40\);')
    out = {}
    for f in glob.glob(os.path.join(KB, "functions", "*.c")):
        for m in pat.finditer(open(f, encoding="utf-8", errors="replace").read()):
            out.setdefault(m.group(2), set()).add(m.group(3))
    out = {k: sorted(v) for k, v in out.items()}
    try:
        json.dump(out, open(p, "w"))
    except OSError:
        pass
    return out


class Model:
    def __init__(self, mk):
        self.key = mk
        self.names = {int(k, 16): v for k, v in json.load(open(os.path.join(OVA, "models.json")))[mk]["vars"].items()}
        self.arrays = []  # (off, size, name)
        fn = [p for p in glob.glob(os.path.join(OVA, "m*.txt")) if os.path.basename(p).startswith(mk.split("_")[0] + "_")]
        if fn:
            for line in open(fn[0]):
                m = re.match(r'(\S+)\s+type (\S+) cnt (\d+)(\[arr\])? off (0x[0-9a-f]+)', line)
                if not m:
                    continue
                n, t, c, a, o = m.groups()
                size = int(c) * (12 if t == "25" else 4)
                if size > 4:
                    self.arrays.append((int(o, 16), size, n))
        self.arrays.sort(key=lambda e: e[1])  # narrowest first

    def look(self, o, arrays=True):
        if o in self.names:
            return self.names[o]
        if arrays:
            for off, size, n in self.arrays:
                if off <= o < off + size:
                    return "%s[+0x%x]" % (n, o - off)
        return None


_models = {}


def get_model(spec):
    mk = MODELS.get(spec, spec)
    if mk not in _models:
        _models[mk] = Model(mk)
    return _models[mk]


def model_for(fname, override):
    if override:
        return override
    for pre, m in PREFIX_MODEL:
        if fname.startswith(pre):
            return m
    return None


def balanced_end(s, i):
    """s[i] == '(' -> index just past the matching ')'."""
    d = 0
    for j in range(i, len(s)):
        if s[j] == "(":
            d += 1
        elif s[j] == ")":
            d -= 1
            if d == 0:
                return j + 1
    return -1


SELF_INLINE = re.compile(r'\*\((?:int|uint|float|undefined4) \*+\)\(DAT_00b420dc \+ 0x40\)')


def find_self_vars(text):
    """Locals assigned only from the current object's instance or from param_1."""
    cands = {}
    for m in re.finditer(r'^\s*(\w+) = ([^;]*);', text, re.M):
        v, rhs = m.group(1), m.group(2).strip()
        ok = bool(SELF_INLINE.fullmatch(rhs)) or rhs == "param_1" or bool(re.fullmatch(r'\(\w+ \*+\)\(?param_1\)?', rhs))
        cands.setdefault(v, []).append(ok)
    return {v for v, oks in cands.items() if all(oks)}


def annotate(text, fname, override=None):
    """Return (annotated text, number of annotated offsets)."""
    mspec = model_for(fname, override)
    aliases = alias_cache()
    univ = set(aliases.get("0x72006b76", []))
    gobj = set(aliases.get("0x3d0098b3", []))
    selfm = get_model(mspec) if mspec else None
    selfvars = find_self_vars(text) if selfm else set()
    if selfm:
        selfvars.add("param_1")
    # locals that alias a global pointer (iVarN = DAT_00b99358;)
    lvars_univ, lvars_g = set(), set()
    assign = {}
    for m in re.finditer(r'^\s*(\w+) = (DAT_[0-9a-f]+);', text, re.M):
        assign.setdefault(m.group(1), []).append(m.group(2))
    for v, rs in assign.items():
        if all(r in univ for r in rs):
            lvars_univ.add(v)
        elif all(r in gobj for r in rs):
            lvars_g.add(v)
    count = 0

    def classify(base):
        if base in selfvars:
            return selfm and ("m", selfm)
        if base in univ or base in lvars_univ:
            return ("m", get_model("univers"))
        if base in gobj or base in lvars_g:
            return ("g", None)
        return None

    def name_for(kind, o, arrays=True):
        if kind[0] == "g":
            return G_NAMES.get(o)
        return kind[1].look(o, arrays)

    # 1) `(BASE + 0xN`, BASE a token
    def rep_tok(m):
        nonlocal count
        k = classify(m.group(1))
        if not k:
            return m.group(0)
        o = int(m.group(2), 16)
        if o < 0x80 and k[0] == "m":
            return m.group(0)
        n = name_for(k, o, ARRAYS)
        if not n:
            return m.group(0)
        count += 1
        return "%s + %s /*%s*/" % (m.group(1), m.group(2), n)
    text = re.sub(r'(?<![\w.])(\w+) \+ (0x[0-9a-f]+)(?=\)| \+ )', rep_tok, text) if (selfvars or univ or gobj) else text

    # 2) inline `*(int *)(DAT_00b420dc + 0x40) + 0xN`
    if selfm:
        def rep_inl(m):
            nonlocal count
            o = int(m.group(2), 16)
            n = selfm.look(o, ARRAYS) if o >= 0x80 else None
            if not n:
                return m.group(0)
            count += 1
            return "%s + %s /*%s*/" % (m.group(1), m.group(2), n)
        text = re.sub(r'(\*\((?:int|uint|float|undefined4) \*+\)\(DAT_00b420dc \+ 0x40\)) \+ (0x[0-9a-f]+)(?=\)| \+ )', rep_inl, text)

    # 3) self[0xIDX] with a 4-byte element type
    if selfm:
        for v in sorted(selfvars - {"param_1"}):
            if not re.search(r'\b(?:int|uint|float|undefined4) \*+%s;' % v, text):
                continue

            def rep_idx(m):
                nonlocal count
                o = int(m.group(1), 16) * 4
                n = selfm.look(o, ARRAYS) if o >= 0x80 else None
                if not n:
                    return m.group(0)
                count += 1
                return "%s[%s /*%s*/]" % (v, m.group(1), n)
            text = re.sub(r'\b%s\[(0x[0-9a-f]+)\]' % v, rep_idx, text)

    # 4) absolute global addresses: *(T *)0x00b9xxxx
    def rep_abs(m):
        a = "DAT_" + m.group(2)
        if a in univ:
            return m.group(0) + " /*UniversPtr*/"
        if a in gobj:
            return m.group(0) + " /*GPtr*/"
        return m.group(0)
    text = re.sub(r'\*\((\w+) \*\)0x(00b9[0-9a-f]{4})\b', rep_abs, text)
    return text, count


ARRAYS = True


def kb_dir_files():
    return sorted(glob.glob(os.path.join(KB, "functions", "*.c")))


def main():
    global ARRAYS
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("fn", nargs="?")
    ap.add_argument("--model")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--no-arrays", action="store_true")
    ap.add_argument("--aliases", action="store_true")
    a = ap.parse_args()
    ARRAYS = not a.no_arrays
    if a.aliases:
        for k, v in alias_cache().items():
            print(k, len(v), " ".join(v[:8]), "..." if len(v) > 8 else "")
        return
    if a.all:
        outd = os.path.join(KB, "annotated")
        os.makedirs(outd, exist_ok=True)
        n = tot = 0
        for p in kb_dir_files():
            base = os.path.basename(p)
            name = re.sub(r'_[0-9a-f]{8}\.c$', '', base)
            text = open(p, encoding="utf-8", errors="replace").read()
            out, c = annotate(text, name, a.model)
            if c:
                open(os.path.join(outd, base), "w", encoding="utf-8").write(out)
                n += 1
                tot += c
        print("annotated %d functions, %d offsets -> %s" % (n, tot, outd))
        return
    if not a.fn:
        ap.error("function name/address or --all required")
    recs = kbmod.match_fn(kbmod.load_index(), a.fn)
    if not recs:
        sys.exit("no function matches %r" % a.fn)
    r = recs[0]
    p = os.path.join(KB, r["file"])
    out, c = annotate(open(p, encoding="utf-8", errors="replace").read(), r["name"], a.model)
    sys.stdout.write(out)
    sys.stderr.write("[%s: %d offsets annotated, model=%s]\n" % (r["name"], c, model_for(r["name"], a.model)))


if __name__ == "__main__":
    main()
