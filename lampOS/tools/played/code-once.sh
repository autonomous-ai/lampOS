#!/bin/zsh
# #2839's lamp check: a spoken sign-in's code is told once, and once more only when asked for again.
# With a stand-in Harness (lamp-tasks-harness-fixture's `code_flow`, its code held by `code_hold_until` until this
# script lets the question come), so nothing signs in for real. The stand-in is put in as ask-again.sh's is, through
# stand-in.sh's /run drop-ins, put back when this ends (and by a reboot). Counts lamp-tasks' journal lines
# `code handed to a start` and `code claimed` (#4112): one telling each, never the digits.
# PLAYS SOUND IN THE ROOM. About 2 minutes. Needs `sudo -n` on the lamp and the lamp's voice quiet for 45 s.
# argv: LABEL FIXTURE, FIXTURE being an ARM64 lamp-tasks-harness-fixture from the same build as the lamp's parts.
# TASKS=PATH (optional), as for ask-again.sh.
# Prints PASS or FAIL for: the code told, told once (still one 20 s later), told again when asked (two),
# still two 20 s later, and the question after the release.
cd "${0:A:h}" || exit 9
. ./settings.sh
L=${1:?LABEL}
FIXTURE=${2:?FIXTURE: an ARM64 lamp-tasks-harness-fixture}
[[ -f $FIXTURE ]] || { echo "no file at $FIXTURE" >&2; exit 2; }
[[ -z ${TASKS:-} || -f $TASKS ]] || { echo "no file at TASKS=$TASKS" >&2; exit 2; }
RATE=${LAMP_SAY_RATE:-185}
OUT="$PLAYED_OUT/scenarios/code-once-$L.txt"
REMOTE=/usr/local/lib/played-stand-in   # /run may be noexec; this folder is removed when the run ends
TOLD='tasks: sign-in [0-9]+: code (handed to a start|claimed)'
./lamp_ssh 'sudo -n true' </dev/null || { echo "the lamp's user needs sudo -n" >&2; exit 2; }
./quiet_gate.sh 45 || { echo "the lamp is not quiet: nothing played" >&2; exit 3; }

line() {  # VOICE TEXT: say it from this Mac's speaker, made once and kept
  local f="$PLAYED_OUT/prompts/generated/code-once-$(print -rn -- "$1|$2" | md5 -q).aiff"
  [[ -s $f ]] || say -v "$1" -r "$RATE" -o "$f" "$2"
  print "$(date +%H:%M:%S) PLAY $1: $2" | tee -a "$OUT"
  afplay "$f"
}
journal() {  # the voice's and lamp-tasks' lines since this run began
  ./lamp_ssh "sudo -n journalctl -u lamp-voice -u lamp-tasks --since @$START --no-pager -o short-iso" </dev/null
}
told() {  # how many times the voice has been handed the code since this run began
  journal | grep -cE -- "$TOLD"
}
wait_told() {  # COUNT SECONDS: wait until the code has been handed COUNT times; print the count
  local until=$(( $(date +%s) + $2 )) n=0
  while (( $(date +%s) < until )); do
    n=$(told)
    (( n >= $1 )) && break
    sleep 1
  done
  print -r -- "$n"
}

print "code-once $L  $(date -u +%Y-%m-%dT%H:%M:%SZ)  fixture $(shasum -a 256 "$FIXTURE" | cut -c1-16)" | tee "$OUT"
./lamp_ssh "sudo -n mkdir -p $REMOTE && sudo -n tee $REMOTE/harness-fixture >/dev/null && sudo -n chmod 0755 $REMOTE/harness-fixture" < "$FIXTURE" || exit 9
# The plan beside the fixture, which stand-in.sh takes before its default: the code, held until the release file.
print -r -- "{\"login\":{\"code_flow\":true,\"code_hold_until\":\"$REMOTE/release\"}}" |
  ./lamp_ssh "sudo -n tee $REMOTE/plan.json >/dev/null" || exit 9
SETUP_ENV="PLAYED_REMOTE=$REMOTE"
if [[ -n ${TASKS:-} ]]; then
  print "lamp-tasks for this run: $(shasum -a 256 "$TASKS" | cut -c1-12)" | tee -a "$OUT"
  ./lamp_ssh "sudo -n tee $REMOTE/lamp-tasks >/dev/null && sudo -n chmod 0755 $REMOTE/lamp-tasks" < "$TASKS" || exit 9
  SETUP_ENV="$SETUP_ENV PLAYED_TASKS=$REMOTE/lamp-tasks"
fi

put_back() {
  ./lamp_ssh "sudo -n env PLAYED_REMOTE=$REMOTE sh -s -- out" < stand-in.sh
}
trap 'put_back 2>&1 | tee -a "$OUT"' EXIT
if ! ./lamp_ssh "sudo -n env $SETUP_ENV sh -s -- in" < stand-in.sh >> "$OUT" 2>&1; then
  tail -2 "$OUT" >&2
  echo "code-once $L: the stand-in's setup failed; nothing was played (see $OUT)" | tee -a "$OUT" >&2
  exit 9
fi
tail -1 "$OUT"
sleep 20
START=$(./lamp_ssh 'date +%s' </dev/null)

typeset -A result
check() {  # NAME PASSED(0|1) DETAIL
  if (( $2 )); then result[$1]=PASS; else result[$1]=FAIL; fi
  print -r -- "${result[$1]} $1: $3" | tee -a "$OUT"
}
line Samantha "Hey Lamp, please sign the lamp in to my computer."
n=$(wait_told 1 60)
check "the code told" $(( n >= 1 )) "$n telling(s)"
# The news and the start's answer share one claim: a second telling would come within these 20 s.
sleep 20
n=$(told)
check "told once" $(( n == 1 )) "$n telling(s)"
line Samantha "Hey Lamp, could you say the code again, please?"
n=$(wait_told 2 45)
check "told again when asked" $(( n == 2 )) "$n telling(s)"
sleep 20
n=$(told)
check "still twice" $(( n == 2 )) "$n telling(s)"
# Let the stand-in's question come, so its sign-in ends as a real one would; answer no.
./lamp_ssh "sudo -n touch $REMOTE/release" </dev/null
q=$(journal | grep -E 'telling news .*\(sign-in-[0-9]+-asking\)' | tail -1)
for _ in {1..45}; do
  [[ -n $q ]] && break
  sleep 1
  q=$(journal | grep -E 'telling news .*\(sign-in-[0-9]+-asking\)' | tail -1)
done
check "the question after the release" $(( ${#q} > 0 )) "${q:0:200}"
[[ -n $q ]] && line Samantha "No, thank you."
journal | grep -E 'sign-in|telling news' | cut -c1-220 >> "$OUT"
fails=${#${(M)${(v)result}:#FAIL}}
print "code-once $L: ${#result} checks, $fails failed. Lines and the journal's sign-in lines: $OUT" | tee -a "$OUT"
exit $(( fails > 0 ))
