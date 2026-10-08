#!/usr/bin/env python3
"""Fail if the release tree contains game data or decompiled code. Usage: check_release.py [root]"""
import os, sys, re
root = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), ".."))
BAD_EXT = {".bf", ".iso", ".elf", ".glb", ".gltf", ".ogg", ".wav", ".mp3", ".png", ".jpg", ".jpeg", ".dds", ".tga", ".mp4", ".avi",
           ".bin", ".dec", ".raw", ".wem", ".pkl", ".dll", ".gpr"}
# tools that parse decompiler output legitimately name its patterns
MARK_OK = {"tools/check_release.py", "tools/kb.py", "tools/kb_annotate.py"}
ALLOW = {"icon.png", "icon.ico", "logo.png"}          # our own artwork (none today)
SKIP_DIRS = {".git", "target", "vendor", "assets", "assets_dev", "bin", "__pycache__"}
MAXSZ = 2 * 1024 * 1024
MARK = re.compile(r"undefined\d|FUN_00|in_ST0|/\* WARNING: Subroutine")
TEXT = {".md", ".txt", ".py", ".sh", ".bat", ".rs", ".toml", ".yaml", ".yml", ".json", ".tsv", ".lock", ".wgsl", ".c", ".h", ".java", ".gitignore"}
errs = []
for d, dirs, files in os.walk(root):
    # assets/, assets_dev/, bin/ are git-ignored local output; they must not exist in a release tree either
    for s in list(dirs):
        if s in ("assets", "assets_dev", "bin", "target", "vendor"):
            errs.append(f"forbidden folder: {os.path.relpath(os.path.join(d, s), root)}")
        if s in SKIP_DIRS:
            dirs.remove(s)
    for f in files:
        p = os.path.join(d, f); rel = os.path.relpath(p, root)
        ext = os.path.splitext(f)[1].lower()
        if ext in BAD_EXT and f.lower() not in ALLOW:
            errs.append(f"forbidden extension: {rel}")
        if ext == ".exe" and rel != "KingKongRecompiled.exe":
            errs.append(f"unexpected exe: {rel}")
        if os.path.getsize(p) > MAXSZ:
            errs.append(f"larger than 2 MB: {rel}")
        if ext in TEXT or f.startswith("."):
            if rel.replace(os.sep, "/") in MARK_OK:
                continue
            try:
                for i, l in enumerate(open(p, errors="ignore"), 1):
                    if MARK.search(l):
                        errs.append(f"decompiler marker: {rel}:{i}"); break
            except OSError as e:
                errs.append(f"unreadable {rel}: {e}")
if errs:
    print("RELEASE CHECK FAILED"); [print(" -", e) for e in errs]; sys.exit(1)
print("release check OK:", root)
