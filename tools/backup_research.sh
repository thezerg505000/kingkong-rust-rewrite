#!/bin/sh
# Zips the important research (not decompiled output, not game data) into KongPS2/backups/research_<date>.zip
# Run from Git Bash/WSL on the PC, or adapt ROOT.  Windows users: backup_research.bat.
ROOT="${KONGPS2:-/z/ClaudeCode/KongPS2}"
D=$(date +%Y-%m-%d)
mkdir -p "$ROOT/backups"
OUT="$ROOT/backups/research_$D.zip"
cd "$ROOT" || exit 1
zip -r "$OUT" research/pc/*.md research/pc/code/*.json research/pc/code/*.py research/pc/code/ova \
  research/pc/atmos research/pc/fx/*.md research/pc/reviews research/pc/tools research/pc/keys/tools \
  kingkong-ps2-rs/docs kingkong-ps2-rs/spec kingkong-ps2-rs/tools kingkong-ps2-rs/crates \
  -x "research/pc/kb/*" "*.dec" "*.raw" "*/textures/*" "*/meshes/*" "*/game_assets/*" "*/batches/*" "*/target/*" "*/vendor/*" "*/__pycache__/*"
echo "wrote $OUT"
