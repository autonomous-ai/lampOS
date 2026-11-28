#!/bin/zsh
# el-say.sh: a `say` stand-in for STOP_LINES_SAY (stop-lines.sh calls SAY -v VOICE -r RATE -o FILE TEXT). A line
# rendered in PLAYED_WAV_DIR (default: wav/ beside this file) as <sha12 of "VOICE|TEXT">.wav is converted to FILE.
# A line with no rendering fails (exit 3) instead of going to mac `say`, so a rendered set never mixes in a mac voice
# (the Reviewer-lead's P2-b on #8673). A call that renders nothing (no -v or no -o) still goes to /usr/bin/say.
args=("$@"); voice=""; out=""
while (( $# > 1 )); do case $1 in -v) voice=$2; shift 2;; -o) out=$2; shift 2;; -r) shift 2;; *) shift;; esac; done
dir=${PLAYED_WAV_DIR:-${0:A:h}/wav}
wav=$dir/$(printf '%s' "$voice|$1" | shasum -a 256 | cut -c1-12).wav
if [[ -n $voice && -n $out ]]; then
  [[ -s $wav ]] || { echo "el-say.sh: no rendered WAV for a $voice line in $dir; render it first" >&2; exit 3; }
  exec /usr/bin/afconvert -f AIFF -d BEI16 "$wav" "$out"
fi
exec /usr/bin/say "${args[@]}"
