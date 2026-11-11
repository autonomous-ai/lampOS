#!/bin/zsh
# Renders every line of mac/lines.tsv to clips/<clip>.aiff with macOS `say -o` (writes files, plays
# nothing). Voice Karen (en_AU), rate 185: one voice for every line, unlike talk.aiff's two
# (Samantha, Daniel), so the busy-room line is a third voice over the room. Already-made clips are
# kept unless FORCE=1. Then clips/SHA256SUMS.
set -eu
K=${0:A:h:h}; VOICE=${VOICE:-Karen}; RATE=${RATE:-185}
cd $K/clips
grep -v '^#' $K/mac/lines.tsv | while IFS=$'\t' read -r row kind how clip line; do
  [[ -n $clip ]] || continue
  if [[ ! -s $clip.aiff || ${FORCE:-0} = 1 ]]; then
    /usr/bin/say -v $VOICE -r $RATE -o $clip.aiff -- "$line"
  fi
  printf '%s\t%s\t%.1f s\t%s\n' $row $clip.aiff "$(afinfo $clip.aiff | awk '/estimated duration/{print $3}')" "$line"
done
shasum -a 256 *.aiff > SHA256SUMS
