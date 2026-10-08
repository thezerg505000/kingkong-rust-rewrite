#!/usr/bin/env bash
# Prepare the KK_TEST_DATA directory used by the parity / end-to-end tests (needs the research tree, not shipped).
# usage: make_test_data.sh <kkpc research dir> <assets dir with the reference glbs> <out dir>
set -euo pipefail
K=$(realpath "$1"); A=$(realpath "$2"); O=$3
HERE=$(dirname "$(realpath "$0")")
mkdir -p "$O/ref_assets"; O=$(realpath "$O")
python3 "$HERE/make_reference.py" "$K" "$O"        # ref_*.json + links to the decoded streams
python3 "$HERE/make_tex_reference.py" "$K" "$O"    # synthetic Xenos chunks + Python RGBA output
for f in jack_fps_arms jack_fps_luger jack_fps_tommygun jack_fps_shotgun jack_fps_sniperrifle trex_inplace; do ln -sf "$A/$f.glb" "$O/ref_assets/$f.glb"; done
ln -sf "$A/trex_rootmotion.json" "$O/ref_assets/trex_rootmotion.json"
ln -sf "$K/textures/ff8003eb/idx063.png" "$O/ref_assets/idx063.png"
ln -sf "$K/textures/ff8003eb/idx064.png" "$O/ref_assets/idx064.png"
gcc -O2 -o "$O/lzo1x" "$K/tools/lzo1x.c"          # the original C decoder, for the differential LZO test
# Kong / creature / sound / FX parity (needs the research tree next to the assets: kkpc/decoded streams, kksnd, kkassets)
R=$(dirname "$A")
for st in ff002001 ff00f858 ff001e3b ff006177; do [ -e "$K/decoded/$st.dec" ] && ln -sf "$K/decoded/$st.dec" "$O/$st.dec"; done
ln -sf "$K/w3e/decoded/ff801b17.dec" "$O/ff801b17.dec"      # level bank with the paletted FX sprites
for f in kong/kong.glb kong/kong_actions.json kong/kong_rootmotion.json kong/ann.glb kong/ann_rootmotion.json; do ln -sf "$A/$f" "$O/ref_assets/$(basename $f)"; done
for f in raptor compy raptor_kong brontosaurus crab; do for x in .glb _actions.json _rootmotion.json; do [ -e "$A/creatures/$f$x" ] && ln -sf "$A/creatures/$f$x" "$O/ref_assets/$f$x"; done; done
ln -sf "$A/creatures/manifest.json" "$O/ref_assets/creatures_manifest.json"
mkdir -p "$O/snd" "$O/ref_assets/fx"
for x in raw wav pcm amb smd_refs.json; do ln -sfn "$R/kksnd/$x" "$O/snd/$x"; done
ln -sf "$A/sound_defs.json" "$O/ref_assets/sound_defs.json"
for f in flare glow_small smoke_fx flash_f11 flash_f12 flash_f13 spark_streak splat_dots; do ln -sf "$A/fx/$f.png" "$O/ref_assets/fx/$f.png"; done
for f in rex_nrm_body rex_nrm_head rex_mr_body rex_mr_head; do ln -sf "$A/$f.png" "$O/ref_assets/$f.png"; done
for i in 189 190 191 192; do ln -sf "$K/textures/ff80018c/idx$i.png" "$O/ref_assets/idx$i.png"; done
echo "KK_TEST_DATA=$O"
