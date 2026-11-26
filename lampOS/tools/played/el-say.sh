#!/bin/zsh
# el-say.sh: a `say` stand-in for STOP_LINES_SAY (stop-lines.sh calls SAY -v VOICE -r RATE -o FILE TEXT). A line
# rendered in PLAYED_WAV_DIR (default: wav/ beside this file) as <sha12 of "VOICE|TEXT">.wav is converted to FILE;
# any other line goes to /usr/bin/say unchanged.
args=("$@"); voice=""; out=""
while (( $# > 1 )); do case $1 in -v) voice=$2; shift 2;; -o) out=$2; shift 2;; -r) shift 2;; *) shift;; esac; done
dir=${PLAYED_WAV_DIR:-${0:A:h}/wav}
wav=$dir/$(printf '%s' "$voice|$1" | shasum -a 256 | cut -c1-12).wav
if [[ -n $voice && -n $out && -s $wav ]]; then
  exec /usr/bin/afconvert -f AIFF -d BEI16 "$wav" "$out"
fi
exec /usr/bin/say "${args[@]}"
