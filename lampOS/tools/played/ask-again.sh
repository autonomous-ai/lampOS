#!/bin/zsh
# #2702's lamp check: a sign-in's question cut short is asked again, then heard whole, and the yes after it is taken.
# With a stand-in Harness (lamp-tasks-harness-fixture's `code_flow`: a code, the question at once, the answer read,
# success), so nothing signs in for real: lamp-tasks gets --sign-in-by-code and the stand-in as its Harness, and the
# voice LAMP_TASKS_SIGN_IN=on, all through /run drop-ins put back when this ends (and by a reboot). The lamp's own
# Harness, its HOME and its sign-in are not touched; hand-overs fail while it runs.
# PLAYS SOUND IN THE ROOM. About 3 minutes. Needs `sudo -n` on the lamp and the lamp's voice quiet for 45 s.
# argv: LABEL FIXTURE, FIXTURE being an ARM64 lamp-tasks-harness-fixture (from a lampOS ARM64 run's artifact).
# TASKS=PATH (optional): an ARM64 lamp-tasks from the same build, used for this run only, for a lamp whose own
# lamp-tasks is older than #2702 (no --sign-in-by-code: the setup says so and nothing is played).
# Prints each step and PASS or FAIL: the question told, talk over its first second cutting it short, "offered again"
# about 30 s after it was taken (ASK_AGAIN_AFTER), the question told again and heard whole ("question told in
# request N"), and the yes then taken ("answered yes", "signed in").
cd "${0:A:h}" || exit 9
. ./settings.sh
L=${1:?LABEL}
FIXTURE=${2:?FIXTURE: an ARM64 lamp-tasks-harness-fixture}
[[ -f $FIXTURE ]] || { echo "no file at $FIXTURE" >&2; exit 2; }
[[ -z ${TASKS:-} || -f $TASKS ]] || { echo "no file at TASKS=$TASKS" >&2; exit 2; }
RATE=${LAMP_SAY_RATE:-185}
OUT="$PLAYED_OUT/scenarios/ask-again-$L.txt"
REMOTE=/usr/local/lib/played-ask-again   # /run may be noexec; this folder is removed when the run ends
./lamp_ssh 'sudo -n true' </dev/null || { echo "the lamp's user needs sudo -n" >&2; exit 2; }
./quiet_gate.sh 45 || { echo "the lamp is not quiet: nothing played" >&2; exit 3; }

line() {  # VOICE TEXT: say it from this Mac's speaker, made once and kept
  local f="$PLAYED_OUT/prompts/generated/ask-again-$(print -rn -- "$1|$2" | md5 -q).aiff"
  [[ -s $f ]] || say -v "$1" -r "$RATE" -o "$f" "$2"
  print "$(date +%H:%M:%S) PLAY $1: $2" | tee -a "$OUT"
  afplay "$f"
}
journal() {  # the voice's and lamp-tasks' lines since this run began
  ./lamp_ssh "sudo -n journalctl -u lamp-voice -u lamp-tasks --since @$START --no-pager -o short-iso" </dev/null
}
wait_for() {  # PATTERN SECONDS: wait until the journal has a line matching PATTERN; print it
  local until=$(( $(date +%s) + $2 )) hit
  while (( $(date +%s) < until )); do
    hit=$(journal | grep -E -- "$1" | tail -1)
    [[ -n $hit ]] && { print -r -- "$hit"; return 0; }
    sleep 1
  done
  return 1
}

print "ask-again $L  $(date -u +%Y-%m-%dT%H:%M:%SZ)  fixture $(shasum -a 256 "$FIXTURE" | cut -c1-16)" | tee "$OUT"
./lamp_ssh "sudo -n mkdir -p $REMOTE && sudo -n tee $REMOTE/harness-fixture >/dev/null && sudo -n chmod 0755 $REMOTE/harness-fixture" < "$FIXTURE" || exit 9
SETUP_ENV="PLAYED_REMOTE=$REMOTE"
if [[ -n ${TASKS:-} ]]; then
  print "lamp-tasks for this run: $(shasum -a 256 "$TASKS" | cut -c1-12)" | tee -a "$OUT"
  ./lamp_ssh "sudo -n tee $REMOTE/lamp-tasks >/dev/null && sudo -n chmod 0755 $REMOTE/lamp-tasks" < "$TASKS" || exit 9
  SETUP_ENV="$SETUP_ENV PLAYED_TASKS=$REMOTE/lamp-tasks"
fi

# The stand-in in (ask-again-setup.sh, on the lamp as root), and put back whatever happens. A setup that fails says
# which step and stops here: nothing is played (#2715's first run, lamp-4ace, 22 Oct 02:5x).
put_back() {
  ./lamp_ssh "sudo -n env PLAYED_REMOTE=$REMOTE sh -s -- out" < ask-again-setup.sh
}
trap 'put_back 2>&1 | tee -a "$OUT"' EXIT
if ! ./lamp_ssh "sudo -n env $SETUP_ENV sh -s -- in" < ask-again-setup.sh >> "$OUT" 2>&1; then
  tail -2 "$OUT" >&2
  echo "ask-again $L: the stand-in's setup failed; nothing was played (see $OUT)" | tee -a "$OUT" >&2
  exit 9
fi
tail -1 "$OUT"
sleep 20
START=$(./lamp_ssh 'date +%s' </dev/null)

typeset -A result
check() {  # NAME LINE-OR-EMPTY; kept here, not in a pipe's subshell
  if [[ -n $2 ]]; then result[$1]=PASS; else result[$1]=FAIL; fi
  print -r -- "${result[$1]} $1${2:+: ${2:0:200}}" | tee -a "$OUT"
}
line Samantha "Hey Lamp, please sign the lamp in to my computer."
first=$(wait_for 'telling news .*\(sign-in-[0-9]+-asking\)' 60)
check "the question told" "$first"
# Talk over its first second, as a person in the room did on lamp-75c9 (21 Oct 22:32:59).
[[ -n $first ]] && line Karen "Oh, sorry, I was just saying the meeting moved to three this afternoon."
check "cut short" "$(wait_for 'news \(sign-in-[0-9]+-asking\) was cut short' 20)"
check "offered again" "$(wait_for 'question \(sign-in-[0-9]+-asking\) was not heard whole: offered again' 75)"
check "heard whole" "$(wait_for 'tasks: sign-in [0-9]+: question told in request' 75)"
[[ ${result[heard whole]} == PASS ]] && line Samantha "Yes, please."
check "the yes taken" "$(wait_for 'tasks: sign-in [0-9]+: answered yes' 30)"
check "signed in (the stand-in)" "$(wait_for 'tasks: sign-in [0-9]+: signed in' 20)"
journal | grep -E 'sign-in|telling news|cut short|offered again' | cut -c1-220 >> "$OUT"
fails=${#${(M)${(v)result}:#FAIL}}
print "ask-again $L: ${#result} checks, $fails failed. Lines and the journal's sign-in lines: $OUT" | tee -a "$OUT"
exit $(( fails > 0 ))
