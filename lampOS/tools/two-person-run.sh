#!/usr/bin/env bash
# #1895's two-person run, automated (the lead's decision, #1711, 20 Oct
# 11:22): two people at one lamp never hear each other's kept things on the
# second route with LAMP_SECOND_SCOPED_CONTEXT=on. Run on the Mac beside the
# lamp, with the lamp's speaker in earshot of the Mac's:
#
#   LAMP=orangepi@lamp-52e6.local lampOS/tools/two-person-run.sh
#
# A and B are the Mac's two voices (VOICE_A, VOICE_B), every line starting
# "Lamp,". Who is at the lamp is not a camera: the voice is pointed, for the
# run only, at a stand-in for the eyes' known file (LAMP_EYES_WHO_SOCKET in
# its own folder; the voice reads `known` beside it), which names A, B or
# nobody on the run's timeline and is kept fresh every second. The real eyes
# go on as they are; nobody need sit at the lamp.
#
# Steps, as the sheet on #1895: 4 each keeps a code (A 417, B 2580); 5 A asks,
# then B at once (B's first line), then B asks what it just asked; 6 A, B, A,
# B alternate; 7 nobody named, one asks; 8 each forgets the code. After each
# line it waits for the answer's exchange in the voice's thread
# (`/run/lampos-voice/thread.json`) and reads only whose it is and which codes
# the answer holds. A line passes when its exchange is the speaker's (the
# room's in 7) and holds no code but its own. Nothing said is printed.
# The thread is read with jq on the lamp, as the factory steps do.
#
# Set-up and put-back, on any exit: a drop-in under /run (removed, the voice
# restarted), the stand-in stopped and its folder removed. The drop-in sets
# LAMP_SECOND_SCOPED_CONTEXT=on and the stand-in's socket; the worker's own
# environment is checked for the switch before the first line.
#
# Environment: LAMP (ssh target, sudo there), LAMP_SH (a command that runs
# the shell script on its stdin as root on the lamp; default
# `ssh $LAMP sudo sh -s`), SAY (say),
# VOICE_A (Samantha), VOICE_B (Daniel), WAIT_S (25, the most to wait for an
# answer), PAUSE_S (5, between lines), and on the lamp RUN_DIR
# (/run/lampos-two-person), THREAD (/run/lampos-voice/thread.json), UNIT_DIR
# (/run/systemd/system); for the tests, WORKER_ENVIRON (a file holding the
# worker's environment, NUL-separated, instead of /proc) and SLEEP (sleep).
set -uo pipefail

SAY=${SAY:-say}
VOICE_A=${VOICE_A:-Samantha} VOICE_B=${VOICE_B:-Daniel}
WAIT_S=${WAIT_S:-25} PAUSE_S=${PAUSE_S:-5}
RUN_DIR=${RUN_DIR:-/run/lampos-two-person}
THREAD=${THREAD:-/run/lampos-voice/thread.json}
UNIT_DIR=${UNIT_DIR:-/run/systemd/system}
SLEEP=${SLEEP:-sleep}
if [[ -z ${LAMP_SH:-} ]]; then
  [[ -n ${LAMP:-} ]] || { echo "LAMP must name the lamp (user@host), or LAMP_SH how to reach it" >&2; exit 2; }
  LAMP_SH="ssh $LAMP sudo sh -s"
fi
on_lamp() { $LAMP_SH <<<"$1"; }
DROPIN=$UNIT_DIR/lamp-voice.service.d/zz-two-person.conf
NAME_A=TwoPersonA NAME_B=TwoPersonB

# The stand-in: `who` names the person at the lamp now (empty: nobody), and
# `known` beside the socket is rewritten from it every second while it
# names someone, as the eyes keep theirs fresh (3 s). It ends when its
# folder is removed.
STAND_IN='while [ -d "$1" ]; do
  n=$(cat "$1/who" 2>/dev/null)
  if [ -n "$n" ]; then printf "%s\n" "$n" >"$1/known.tmp" && mv "$1/known.tmp" "$1/known"; else rm -f "$1/known"; fi
  sleep 1
done'

put_back() {
  on_lamp "rm -rf '$RUN_DIR'; rm -f '$DROPIN'; systemctl daemon-reload; systemctl restart lamp-voice" >/dev/null 2>&1
  echo "put back: the drop-in and the stand-in removed, lamp-voice restarted"
}
trap put_back EXIT
trap 'exit 130' INT TERM

on_lamp "mkdir -p '$RUN_DIR' && chmod 0755 '$RUN_DIR' && : >'$RUN_DIR/who' && mkdir -p '$(dirname "$DROPIN")' && printf '[Service]\nEnvironment=LAMP_SECOND_SCOPED_CONTEXT=on\nEnvironment=LAMP_EYES_WHO_SOCKET=$RUN_DIR/who.sock\n' >'$DROPIN' && systemctl daemon-reload && systemctl restart lamp-voice" ||
  { echo "the drop-in could not be set" >&2; exit 3; }
on_lamp "cat >'$RUN_DIR/stand-in.sh' <<'STAND_IN'
$STAND_IN
STAND_IN
nohup sh '$RUN_DIR/stand-in.sh' '$RUN_DIR' </dev/null >/dev/null 2>&1 &"
"$SLEEP" 5
environ=$(on_lamp "if [ -n '${WORKER_ENVIRON:-}' ]; then cat '${WORKER_ENVIRON:-}'; else P=\$(pgrep -f '[l]amp-voice worker second' | head -1); [ -n \"\$P\" ] && cat /proc/\$P/environ; fi" | tr '\0' '\n')
grep -qx LAMP_SECOND_SCOPED_CONTEXT=on <<<"$environ" ||
  { echo "the second worker runs without LAMP_SECOND_SCOPED_CONTEXT=on (not the second route?); nothing measured" >&2; exit 4; }

who() { on_lamp "printf '%s' '$1' >'$RUN_DIR/who'"; }
# The number of exchanges in the thread now.
count() { on_lamp "jq length '$THREAD' 2>/dev/null || echo 0"; }
# The newest exchange: "whose codes", the codes said in its answer, as
# digits, by word or figure; never its words.
NEWEST='max_by(.at_unix_ms)
  | (.lamp | ascii_downcase | [scan("[a-z]+|[0-9]")]
     | map({zero: "0", oh: "0", one: "1", two: "2", three: "3", four: "4", five: "5",
            six: "6", seven: "7", eight: "8", nine: "9"}[.] // (select(test("^[0-9]$"))))
     | join("")) as $digits
  | [("417", "2580") | select(. as $code | $digits | contains($code))] as $codes
  | "\(.whose // "room") \(if $codes == [] then "-" else ($codes | join(",")) end)"'
newest() { on_lamp "jq -r '$NEWEST' '$THREAD'"; }
id_of() { # the memory's person id for a name, as the voice makes it
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | shasum -a 256 | cut -c1-16 | sed 's/^/p-/'
}
ID_A=$(id_of "$NAME_A") ID_B=$(id_of "$NAME_B")

passed=0 failed=0
line() { # line STEP SPEAKER(A|B) WHO(A|B|-) OWN_CODE TEXT
  local step=$1 speaker=$2 at=$3 own=$4 text=$5 before voice expected whose codes
  before=$(count)
  case $speaker in A) voice=$VOICE_A ;; B) voice=$VOICE_B ;; esac
  case $at in A) expected=$ID_A ;; B) expected=$ID_B ;; -) expected=room ;; esac
  "$SAY" -v "$voice" "$text"
  for _ in $(seq "$WAIT_S"); do
    [[ $(count) -gt $before ]] && break
    "$SLEEP" 1
  done
  if [[ $(count) -le $before ]]; then
    echo "step $step $speaker: no answer kept in ${WAIT_S} s: FAIL"
    failed=$((failed + 1))
    return
  fi
  read -r whose codes <<<"$(newest)"
  local verdict=PASS
  [[ $whose == "$expected" ]] || verdict=FAIL
  for code in ${codes//,/ }; do [[ $code == - || $code == "$own" ]] || verdict=FAIL; done
  local as=$speaker; [[ $whose == room ]] && as=room
  [[ $whose != room && $whose != "$ID_A" && $whose != "$ID_B" ]] && as=other
  echo "step $step $speaker: kept as $as (expected $( [[ $at == - ]] && echo room || echo "$at")), codes in the answer: $codes: $verdict"
  if [[ $verdict == PASS ]]; then passed=$((passed + 1)); else failed=$((failed + 1)); fi
  "$SLEEP" "$PAUSE_S"
}

echo "two-person run: A is $VOICE_A, B is $VOICE_B; scoped context on"
# 4: each keeps a code, alone at the lamp.
who "$NAME_A"; "$SLEEP" 3
line 4 A A 417 "Lamp, remember that my test code is four one seven."
who "$NAME_B"; "$SLEEP" 3
line 4 B B 2580 "Lamp, remember that my test code is two five eight zero."
# 5: A, then B at once: B's first line, then B's follow-up.
who "$NAME_A"; "$SLEEP" 3
line 5 A A 417 "Lamp, what is my test code?"
who "$NAME_B"; "$SLEEP" 3
line 5 B B 2580 "Lamp, what is my test code?"
line 5 B B 2580 "Lamp, what did I just ask you?"
# 6: alternating.
for speaker in A B A B; do
  if [[ $speaker == A ]]; then who "$NAME_A"; own=417; else who "$NAME_B"; own=2580; fi
  "$SLEEP" 3
  line 6 "$speaker" "$speaker" "$own" "Lamp, what is my test code?"
done
# 7: nobody named; neither code may be said.
who ""; "$SLEEP" 5
line 7 A - none "Lamp, what is my test code?"
# 8: each forgets the code.
who "$NAME_A"; "$SLEEP" 3
line 8 A A 417 "Lamp, forget my test code."
who "$NAME_B"; "$SLEEP" 3
line 8 B B 2580 "Lamp, forget my test code."

echo "$passed passed, $failed failed"
[[ $failed == 0 ]]
