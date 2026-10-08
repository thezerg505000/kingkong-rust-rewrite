#!/bin/bash
# Rebuild the PC knowledge base (research/pc/kb) from the Ghidra project.
# Linux / cloud usage:  GHIDRA=~/tools/ghidra_12.1.4_PUBLIC PROJ=~/ghidra_proj tools/ghidra/run_export.sh
# First import (only once):
#   $GHIDRA/support/analyzeHeadless $PROJ KK8 -import KingKong8.exe -processor x86:LE:32:default -cspec windows
set -e
GHIDRA=${GHIDRA:-$HOME/tools/ghidra_12.1.4_PUBLIC}
PROJ=${PROJ:-$HOME/ghidra_proj}
HERE=$(cd "$(dirname "$0")" && pwd)
RESEARCH=${RESEARCH:-$HERE/../../../research/pc}
mkdir -p "$RESEARCH/kb"
KK_EXPORT_RESUME=${KK_EXPORT_RESUME:-1} "$GHIDRA/support/analyzeHeadless" "$PROJ" KK8 -process KingKong8.exe -noanalysis \
  -scriptPath "$HERE" -postScript KKExport.java "$RESEARCH/code" "$RESEARCH/kb"

# drop stale FUN_<addr> files for functions that have since been named
python3 - "$RESEARCH/kb/functions" <<'PY'
import os,re,sys,collections
d=sys.argv[1]; by=collections.defaultdict(list)
for f in os.listdir(d):
    m=re.match(r'(.*)_([0-9a-f]{8})\.c$',f)
    if m: by[m.group(2)].append(f)
for a,fs in by.items():
    if len(fs)>1:
        for f in fs:
            if f.startswith('FUN_'): os.remove(os.path.join(d,f))
PY
