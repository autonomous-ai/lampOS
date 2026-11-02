#!/bin/zsh
# render_one.sh ID VOICE TEXT: one `say` render into work/sayy.
O=/Users/autonomous/lamp-name-round/work/sayy
say -v "$2" -r $(( 160 + RANDOM % 60 )) -o $O/$1.wav --file-format=WAVE --data-format=LEI16@16000 "$3" 2>/dev/null && print -r -- "$1	$2	$3" >> $O/index.tsv
