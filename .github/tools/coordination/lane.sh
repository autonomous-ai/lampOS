#!/bin/zsh
# lane.sh PR... (4 Nov, #5212 part 3): which train lane each PR goes to, as train-bot.sh decides it with two lanes:
# "no-crates" when none of its files is a build input (lib.sh, coord_lane), else "crate" with the first input. Read-only
# but for a local fetch of origin's lamp-voice and each PR's head (refs/remotes/pr/N), which the bot does too.
# REPO_DIR from the environment (README.md), default: the repo holding this folder.
source ${0:A:h}/lib.sh
cd "${REPO_DIR:-$(git -C ${0:A:h} rev-parse --show-toplevel)}" || exit 1
git fetch -q origin lamp-voice 2>/dev/null
coord_inputs_init origin/lamp-voice
for p in "$@"; do
  git fetch -q origin "+refs/pull/$p/head:refs/remotes/pr/$p" 2>/dev/null || { print -r -- "#$p: crate lane (its head can't be fetched)"; continue; }
  l=$(coord_lane refs/remotes/pr/$p)
  if [[ $l == nocrates ]]; then print -r -- "#$p: no-crates lane"; else print -r -- "#$p: crate lane (${l#crates: })"; fi
done
