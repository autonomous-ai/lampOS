#!/bin/zsh
# #751's build row "Email, calendar and other connectors, through hand-overs", end to end with a stand-in Harness:
# "what's on my calendar today?" is handed over to the computer's agent, the stand-in answers with two made-up events,
# and the lamp tells them back. Nothing reaches a real Harness, account or calendar: lamp-tasks gets
# lamp-tasks-harness-fixture as its Harness (stand-in.sh) and a targets file for the run, one alias, the voice's own,
# on the fixture's machine; the voice gets LAMP_TASKS_ACCOUNTS=on (the person's calendar is asked of their computer's
# agent, #1676). All through /run drop-ins put back when this ends, and by a reboot. Hand-overs fail while it runs.
# PLAYS SOUND IN THE ROOM. About 4 minutes. Needs `sudo -n` on the lamp and the lamp's voice quiet for 45 s.
# argv: LABEL FIXTURE, FIXTURE being an ARM64 lamp-tasks-harness-fixture (from a lampOS ARM64 run's artifact).
# TASKS=PATH (optional): an ARM64 lamp-tasks from the same build, for a lamp whose own is older (as ask-again.sh).
# Prints PASS or FAIL for each step: offered (start_agent_task), handed over (the stand-in's `new`), the answer read
# (its `show`), told (the news), and heard whole: not cut short, and both made-up events in the lamp's words.
cd "${0:A:h}" || exit 9
. ./settings.sh
L=${1:?LABEL}
FIXTURE=${2:?FIXTURE: an ARM64 lamp-tasks-harness-fixture}
[[ -f $FIXTURE ]] || { echo "no file at $FIXTURE" >&2; exit 2; }
[[ -z ${TASKS:-} || -f $TASKS ]] || { echo "no file at TASKS=$TASKS" >&2; exit 2; }
RATE=${LAMP_SAY_RATE:-185}
OUT="$PLAYED_OUT/scenarios/calendar-$L.txt"
REMOTE=/usr/local/lib/played-stand-in   # /run may be noexec; this folder is removed when the run ends
./lamp_ssh 'sudo -n true' </dev/null || { echo "the lamp's user needs sudo -n" >&2; exit 2; }
./quiet_gate.sh 45 || { echo "the lamp is not quiet: nothing played" >&2; exit 3; }

# The stand-in's plan: its own machine for `machines`, an agent for `new`, and for `show` the answer: two made-up
# events, with words no one in the room says ("Lantern", "dentist").
PLAN='{"machines":{"default":true},"new":{"default":true},"show":{"stdout":"{\"id\":\"11111111-1111-4111-8111-111111111111\",\"state\":\"idle\",\"ask\":\"What is on my calendar today?\",\"answer\":\"You have two things on your calendar today: at 10:00 a design review in the Lantern Room for 45 minutes, and at 15:30 a dentist appointment.\",\"truncated\":false,\"tools\":2}"}}'

line() {  # VOICE TEXT: say it from this Mac's speaker, made once and kept
  local f="$PLAYED_OUT/prompts/generated/calendar-$(print -rn -- "$1|$2" | md5 -q).aiff"
  [[ -s $f ]] || say -v "$1" -r "$RATE" -o "$f" "$2"
  print "$(date +%H:%M:%S) PLAY $1: $2" | tee -a "$OUT"
  afplay "$f"
}
journal() {  # the voice's and lamp-tasks' lines since this run began
  ./lamp_ssh "sudo -n journalctl -u lamp-voice -u lamp-tasks --since @$START --no-pager -o short-iso" </dev/null
}
called() {  # how many times the stand-in was called with OPERATION so far: one number
  local n
  n=$(./lamp_ssh "sudo -n grep -c '\"args\":\[\"$1\"' $REMOTE/home/calls.jsonl || true" </dev/null 2>/dev/null)
  print -r -- "${n:-0}"
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
wait_called() {  # OPERATION SECONDS: wait until the stand-in was called with OPERATION
  local until=$(( $(date +%s) + $2 ))
  while (( $(date +%s) < until )); do
    (( $(called "$1") > 0 )) && { print "the stand-in's $1 was called"; return 0; }
    sleep 2
  done
  return 1
}

print "calendar $L  $(date -u +%Y-%m-%dT%H:%M:%SZ)  fixture $(shasum -a 256 "$FIXTURE" | cut -c1-16)" | tee "$OUT"
./lamp_ssh "sudo -n mkdir -p $REMOTE && sudo -n tee $REMOTE/harness-fixture >/dev/null && sudo -n chmod 0755 $REMOTE/harness-fixture" < "$FIXTURE" || exit 9
print -r -- "$PLAN" | ./lamp_ssh "sudo -n tee $REMOTE/plan.json >/dev/null" || exit 9
SETUP_ENV="PLAYED_REMOTE=$REMOTE PLAYED_TARGET=auto PLAYED_VOICE_ENV=LAMP_TASKS_ACCOUNTS=on"
if [[ -n ${TASKS:-} ]]; then
  print "lamp-tasks for this run: $(shasum -a 256 "$TASKS" | cut -c1-12)" | tee -a "$OUT"
  ./lamp_ssh "sudo -n tee $REMOTE/lamp-tasks >/dev/null && sudo -n chmod 0755 $REMOTE/lamp-tasks" < "$TASKS" || exit 9
  SETUP_ENV="$SETUP_ENV PLAYED_TASKS=$REMOTE/lamp-tasks"
fi

# The stand-in in (stand-in.sh, on the lamp as root), and put back whatever happens. A setup that fails says which
# step and stops here: nothing is played.
put_back() {
  ./lamp_ssh "sudo -n env PLAYED_REMOTE=$REMOTE sh -s -- out" < stand-in.sh
}
trap 'put_back 2>&1 | tee -a "$OUT"' EXIT
if ! ./lamp_ssh "sudo -n env $SETUP_ENV sh -s -- in" < stand-in.sh >> "$OUT" 2>&1; then
  tail -2 "$OUT" >&2
  echo "calendar $L: the stand-in's setup failed; nothing was played (see $OUT)" | tee -a "$OUT" >&2
  exit 9
fi
tail -2 "$OUT"
sleep 20
START=$(./lamp_ssh 'date +%s' </dev/null)

typeset -A result
check() {  # NAME LINE-OR-EMPTY; kept here, not in a pipe's subshell
  if [[ -n $2 ]]; then result[$1]=PASS; else result[$1]=FAIL; fi
  print -r -- "${result[$1]} $1${2:+: ${2:0:200}}" | tee -a "$OUT"
}
line Samantha "Hey Lamp, what's on my calendar today?"
check "offered" "$(wait_for 'asked for start_agent_task' 40)"
# Its read-back ends with "Shall I start it?": the yes after it.
[[ ${result[offered]} == PASS ]] && { sleep 8; line Samantha "Yes, please."; }
check "handed over" "$(wait_called new 40)"
check "the answer read" "$(wait_called show 90)"
told=$(wait_for 'telling news from the work handed over' 120)
check "told" "$told"
# Heard whole: not cut short, and the lamp's own words have both made-up events.
sleep 25
words=$(./lamp_ssh 'sudo -n sh -c '"'"'d=/var/lib/lampOS/voice/sessions; s=$(ls -t $d | head -n 1); grep -h "\"kind\":\"transcript\"" $d/$s/metadata/events-*.jsonl'"'" </dev/null)
cut=$(journal | grep -E 'news \(.*\) was cut short' | tail -1)
said=""
[[ -n $told && -z $cut ]] && print -r -- "$words" | grep -qi lantern && print -r -- "$words" | grep -qi dentist \
  && said="both events said, not cut short"
check "heard whole" "$said"
[[ -n $cut ]] && print -r -- "  cut short: ${cut:0:200}" | tee -a "$OUT"
journal | grep -E 'start_agent_task|confirm_agent_task|telling news|cut short' | cut -c1-220 >> "$OUT"
fails=${#${(M)${(v)result}:#FAIL}}
print "calendar $L: ${#result} checks, $fails failed. Lines and the journal's hand-over lines: $OUT" | tee -a "$OUT"
exit $(( fails > 0 ))
