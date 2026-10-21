#!/bin/zsh
# #2702's lamp check: a sign-in's question cut short is asked again, then heard whole, and the yes after it is taken.
# With a stand-in Harness (lamp-tasks-harness-fixture's `code_flow`: a code, the question at once, the answer read,
# success), so nothing signs in for real: lamp-tasks gets --sign-in-by-code and the stand-in as its Harness, and the
# voice LAMP_TASKS_SIGN_IN=on, all through /run drop-ins put back when this ends (and by a reboot). The lamp's own
# Harness, its HOME and its sign-in are not touched; hand-overs fail while it runs.
# PLAYS SOUND IN THE ROOM. About 3 minutes. Needs `sudo -n` on the lamp and the lamp's voice quiet for 45 s.
# argv: LABEL FIXTURE, FIXTURE being an ARM64 lamp-tasks-harness-fixture (from a lampOS ARM64 run's artifact).
# Prints each step and PASS or FAIL: the question told, talk over its first second cutting it short, "offered again"
# about 30 s after it was taken (ASK_AGAIN_AFTER), the question told again and heard whole ("question told in
# request N"), and the yes then taken ("answered yes", "signed in").
cd "${0:A:h}" || exit 9
. ./settings.sh
L=${1:?LABEL}
FIXTURE=${2:?FIXTURE: an ARM64 lamp-tasks-harness-fixture}
[[ -f $FIXTURE ]] || { echo "no file at $FIXTURE" >&2; exit 2; }
RATE=${LAMP_SAY_RATE:-185}
OUT="$PLAYED_OUT/scenarios/ask-again-$L.txt"
REMOTE=/usr/local/lib/played-ask-again   # /run may be noexec; this folder is removed when the run ends
DROP=zz-played-ask-again.conf
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

# The stand-in in, and put back whatever happens.
put_back() {
  ./lamp_ssh "sudo -n sh -s" <<EOF
rm -f /run/systemd/system/lamp-tasks.service.d/$DROP /run/systemd/system/lamp-voice.service.d/$DROP
systemctl daemon-reload
systemctl restart lamp-tasks lamp-voice
sleep 5
rm -rf $REMOTE
echo "put back: lamp-tasks \$(systemctl is-active lamp-tasks), lamp-voice \$(systemctl is-active lamp-voice)"
EOF
}
trap 'put_back | tee -a "$OUT"' EXIT
./lamp_ssh "sudo -n sh -s" <<EOF | tee -a "$OUT" || exit 9
set -e
home=$REMOTE/home
mkdir -p \$home
printf '{"login":{"code_flow":true}}\n' > \$home/plan.json
printf '#!/bin/sh\nHOME=%s exec %s "\$@"\n' "\$home" "$REMOTE/harness-fixture" > $REMOTE/harness
chmod 0755 $REMOTE/harness
chown -R lampos:lampos \$home
# lamp-tasks' own ExecStart, its --harness swapped for the stand-in, with --sign-in-by-code.
exec_line=\$(systemctl cat lamp-tasks | awk '/^ExecStart=./ { line = substr(\$0, 11) } END { print line }')
case "\$exec_line" in *--harness\ *) ;; *) echo "lamp-tasks has no --harness: nothing changed"; exit 1;; esac
exec_line=\$(printf '%s\n' "\$exec_line" | sed -E "s#--harness [^ ]+#--harness $REMOTE/harness#")
case "\$exec_line" in *--sign-in-by-code*) ;; *) exec_line="\$exec_line --sign-in-by-code";; esac
mkdir -p /run/systemd/system/lamp-tasks.service.d /run/systemd/system/lamp-voice.service.d
printf '[Service]\nExecStart=\nExecStart=%s\n' "\$exec_line" > /run/systemd/system/lamp-tasks.service.d/$DROP
printf '[Service]\nEnvironment=LAMP_TASKS_SIGN_IN=on\n' > /run/systemd/system/lamp-voice.service.d/$DROP
systemctl daemon-reload
systemctl restart lamp-tasks lamp-voice
sleep 20
echo "stand-in in: lamp-tasks \$(systemctl is-active lamp-tasks), lamp-voice \$(systemctl is-active lamp-voice)"
EOF
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
