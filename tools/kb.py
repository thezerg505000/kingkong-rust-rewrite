#!/usr/bin/env python3
"""Query the decompiled-code knowledge base (research/pc/kb, built by tools/ghidra/KKExport.java).

  kb fn <name|0xaddr>          print the decompiled C of a function (prefix match on name)
  kb find <regex>              list functions whose NAME matches
  kb grep <regex> [-n]         grep the decompiled C of every function (prints file:line)
  kb callers <name>            who calls it
  kb callees <name>            what it calls
  kb strings <regex>           strings in the exe matching, with the functions that reference them
  kb refs <regex>              functions that reference a string matching regex
  kb globals <0xaddr>          functions that touch a global address (e.g. 0xb99358)
  kb stats                     counts

See also tools/kb_annotate.py: prints/writes decompiled C with model-variable offsets named
(`+ 0xacc /*i_etat_courant*/`); `kb_annotate.py --all` fills kb/annotated/.

Set KK_KB to override the knowledge-base directory (default research/pc/kb next to the repo).
Naming prefixes are explained in docs/ENGINE_MAP.md.
"""
import sys, os, re, json
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KB = os.environ.get("KK_KB") or os.path.join(os.path.dirname(ROOT), "research", "pc", "kb")

def load_index():
    recs = []
    with open(os.path.join(KB, "index", "functions.jsonl"), encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                try:
                    recs.append(json.loads(line))
                except json.JSONDecodeError:
                    pass  # export still running: last line may be partial
    # functions exported but not yet indexed (export in progress): minimal records from file names
    seen = {r["file"] for r in recs}
    import glob
    for p in sorted(glob.glob(os.path.join(KB, "functions", "*.c"))):
        rel = "functions/" + os.path.basename(p)
        if rel in seen:
            continue
        m = re.match(r"(.*)_([0-9a-f]{8})\.c$", os.path.basename(p))
        if m:
            recs.append({"addr": m.group(2), "name": m.group(1), "aliases": [], "size": "?", "callers": [], "callees": [], "strings": [], "globals": [], "file": rel})
    return recs

def match_fn(recs, key):
    key_l = key.lower()
    if key_l.startswith("0x"):
        addr = int(key, 16)
        return [r for r in recs if int(r["addr"], 16) == addr]
    exact = [r for r in recs if r["name"].lower() == key_l or key_l in [a.lower() for a in r.get("aliases", [])]]
    if exact:
        return exact
    return [r for r in recs if key_l in r["name"].lower()]

def main():
    if len(sys.argv) < 2:
        print(__doc__); return
    cmd = sys.argv[1]; arg = sys.argv[2] if len(sys.argv) > 2 else ""
    if not os.path.isdir(KB):
        sys.exit("knowledge base not found at %s (run tools/ghidra/run_export.sh or set KK_KB)" % KB)
    recs = load_index()
    if cmd == "stats":
        named = sum(1 for r in recs if not r["name"].startswith("FUN_"))
        print("functions", len(recs), "named", named, "kb", KB)
    elif cmd == "fn":
        for r in match_fn(recs, arg)[:5]:
            print(open(os.path.join(KB, r["file"]), encoding="utf-8", errors="replace").read())
            print("// ---- %s %s" % (r["name"], r["addr"]))
    elif cmd == "find":
        rx = re.compile(arg, re.I)
        for r in recs:
            if rx.search(r["name"]) or any(rx.search(a) for a in r.get("aliases", [])):
                print("%s %-40s size=%-6s callers=%d callees=%d" % (r["addr"], r["name"], r["size"], len(r["callers"]), len(r["callees"])))
    elif cmd == "grep":
        rx = re.compile(arg, re.I)
        for r in recs:
            p = os.path.join(KB, r["file"])
            try:
                for i, line in enumerate(open(p, encoding="utf-8", errors="replace"), 1):
                    if rx.search(line):
                        print("%s:%d: %s" % (r["file"], i, line.rstrip()[:200]))
            except FileNotFoundError:
                pass
    elif cmd in ("callers", "callees"):
        for r in match_fn(recs, arg)[:3]:
            print("%s %s:" % (r["name"], r["addr"]))
            for c in sorted(set(r[cmd])):
                print("   ", c)
    elif cmd == "strings":
        rx = re.compile(arg, re.I)
        for line in open(os.path.join(KB, "index", "strings.tsv"), encoding="utf-8", errors="replace"):
            parts = line.rstrip("\n").split("\t")
            if len(parts) >= 2 and rx.search(parts[1]):
                print(line.rstrip()[:300])
    elif cmd == "refs":
        rx = re.compile(arg, re.I)
        for r in recs:
            hits = [s for s in r.get("strings", []) if rx.search(s)]
            if hits:
                print("%s %-40s %s" % (r["addr"], r["name"], hits[:4]))
    elif cmd == "globals":
        a = int(arg, 16)
        for r in recs:
            if any(int(g, 16) == a for g in r.get("globals", [])):
                print("%s %s" % (r["addr"], r["name"]))
    else:
        print(__doc__)

if __name__ == "__main__":
    main()
