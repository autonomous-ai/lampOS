#!/usr/bin/env bash
# The desk kit: a person at a desk asks the lamp things hesitantly (pauses of 0.2 to 0.9 s, "uh", "um", a restart,
# trailing off; hesitant.tsv) and three un-named follow-ups, between colleagues' side talk not for the lamp (AMI
# meeting clips, else `say`-voiced exchanges). Each arm is a set of the voice's switches, put on through a /run
# drop-in; per arm it scores the lines (desk_kit.py, README.md "The desk kit") and lists the journal's reasons.
#
#   ARMS="off:LAMP_X=off on:LAMP_X=on,LAMP_Y=500" LAMP_HOST=<lamp> lampOS/tools/played/desk-kit.sh LABEL
#   ARMS=... lampOS/tools/played/desk-kit.sh --dry [SCENARIO ...]   # no lamp, nothing played: the drop-ins, the
#                                                                   # scene, the scorer on SCENARIO files
# Settings (with the kit's, README.md):
#   ARMS       space-separated NAME:ENV=v,ENV=v (required)       TRIALS    scenes per arm (default 2)
#   BASE       ENV=v ... set in every arm (optional)            VOICE_BIN a lamp-voice on the lamp for the run only
#   FACE       an image or page on this Mac, shown full screen on the built-in display for the whole run (optional:
#              without it no face is shown); FACE_SIZE its window, default 1728,1117; CHROME the browser to show it
#   LOAN_NAME  who holds the lamp, for the loans line (default: this Mac's user); LOANS the file on the lamp the
#              line goes to (default ~/loans.txt; LOANS= writes none)
#   DESK_KIT_AMI  the AMI clips folder on this Mac, outside the repo (default ~/lamp-played/ami)
# It plays sound in the room: the Mac about 1 m from the lamp, where a person sits, its volume 50. About 7 minutes a
# scene, 1.5 minutes a restart and 70 s of quiet between scenes. Not between 12:00 and 14:00 on weekdays.
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
cd "$HERE" || exit 9
DRY=no
[[ ${1:-} == --dry ]] && { DRY=yes; shift; }
ARMS=${ARMS:-}
TRIALS=${TRIALS:-2}
BASE=${BASE:-}
VOICE_BIN=${VOICE_BIN:-}
FACE=${FACE:-}
FACE_SIZE=${FACE_SIZE:-1728,1117}
CHROME=${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}
LOAN_NAME=${LOAN_NAME:-$(id -un)}
LOANS=${LOANS-'~/loans.txt'}
DROPIN=/run/systemd/system/lamp-voice.service.d/zz-desk-kit.conf
PROFILE=${TMPDIR:-/tmp}/desk-kit-face-chrome

for a in $ARMS; do
  [[ $a =~ ^[A-Za-z0-9_-]+:[A-Z0-9_]+=[^,]*(,[A-Z0-9_]+=[^,]*)*$ ]] || { echo "desk-kit.sh: arm '$a' is not NAME:ENV=v,ENV=v" >&2; exit 2; }
done
[[ -n $ARMS && $TRIALS =~ ^[1-9]$ ]] || { echo "desk-kit.sh: set ARMS (NAME:ENV=v,ENV=v ...) and TRIALS (1 to 9)" >&2; exit 2; }

dropin() { # dropin ARM EXEC_LINE: the drop-in's text
  local envs=${1#*:}
  echo "[Service]"
  [[ -n $VOICE_BIN ]] && { echo "ExecStart="; echo "ExecStart=$2"; }
  for e in $BASE ${envs//,/ }; do echo "Environment=$e"; done
}

if [[ $DRY == yes ]]; then
  export PLAYED_OUT=${PLAYED_OUT:-$HOME/lamp-played}
  for a in $ARMS; do echo "== drop-in $DROPIN, arm ${a%%:*}"; dropin "$a" "$VOICE_BIN <lamp-voice's own arguments>"; done
  plan=$(mktemp)
  for t in $(seq 1 "$TRIALS"); do
    echo "== scene $t (mark ask | step)"
    python3 ./desk_kit.py scene "$t" "$plan" > "$plan.steps" || exit 9
    paste <(grep -v '^#' "$plan" | cut -f1,2) "$plan.steps"
  done
  rm -f "$plan" "$plan.steps"
  python3 ./desk_kit.py selftest || exit 1
  [[ $# -gt 0 ]] && python3 ./desk_kit.py score --arm dry "$@"
  exit 0
fi

LABEL=${1:-}
[[ -n $LABEL ]] || { echo "usage: desk-kit.sh LABEL   or   desk-kit.sh --dry [SCENARIO ...]" >&2; exit 2; }
# shellcheck source=settings.sh
. ./settings.sh
if [[ $(date +%u) -le 5 && $(date +%H) -ge 12 && $(date +%H) -lt 14 && ${LAMP_PLAY_AT_LUNCH:-} != yes ]]; then
  echo "desk-kit.sh: not between 12:00 and 14:00 local, when the office rests" >&2
  exit 2
fi
LAMP_SSH=${LAMP_SSH:-$HERE/lamp_ssh}
OUT=$PLAYED_OUT/scenarios
LOG=$OUT/desk-kit-$LABEL.txt
lamp() { "$LAMP_SSH" "$@" </dev/null; }
loan() { [[ -n $LOANS ]] && echo "$(date +%H:%M) $LOAN_NAME: $1" | "$LAMP_SSH" "cat >> $LOANS"; true; }

FACE_PID=
hide_face() { [[ -n $FACE_PID ]] && kill "$FACE_PID" 2>/dev/null; FACE_PID=; pkill -f "user-data-dir=$PROFILE" 2>/dev/null; true; }
put_back() {
  trap '' INT TERM; trap - EXIT
  hide_face
  osascript -e "set volume output volume 50"
  lamp "sudo -n rm -f $DROPIN; sudo -n systemctl daemon-reload; sudo -n systemctl restart lamp-voice; systemctl is-active lamp-voice"
  loan "desk kit $LABEL done, lamp-voice back without $DROPIN, face closed; loan ended"
}
trap put_back EXIT
trap 'exit 130' INT TERM HUP

lamp "ls ${DROPIN%/*}/ 2>/dev/null" | grep -v "^${DROPIN##*/}\$" && { echo "desk-kit.sh: another /run drop-in on lamp-voice; not starting" >&2; trap - EXIT; exit 3; }
pgrep -f "user-data-dir=$PROFILE" >/dev/null && { echo "desk-kit.sh: a face is already shown; not starting" >&2; trap - EXIT; exit 3; }
n=0; for a in $ARMS; do n=$((n + 1)); done
loan "desk kit $LABEL, lamp-voice + /run ${DROPIN##*/}, Mac voices${FACE:+ + a face on the Mac screen}, no person, ~$((n * (TRIALS * 8 + 3))) min"
exec_line=
if [[ -n $VOICE_BIN ]]; then
  exec_line=$(lamp "systemctl cat lamp-voice | grep '^ExecStart=.' | tail -1 | cut -d= -f2-")
  exec_line="$VOICE_BIN ${exec_line#* }"
fi
osascript -e "set volume output volume 50"
mkdir -p "$OUT"

arm() { # arm NAME:ENV=v,...
  local name=${1%%:*} envs="$BASE ${1#*:}" e
  envs=${envs//,/ }
  dropin "$1" "$exec_line" | "$LAMP_SSH" "sudo -n mkdir -p ${DROPIN%/*} && sudo -n tee $DROPIN >/dev/null && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-voice" \
    || { echo "desk-kit.sh: the drop-in could not be put on (sudo -n)" | tee -a "$LOG"; exit 4; }
  sleep 20
  local names; names=$(for e in $envs; do printf '%s|' "${e%%=*}"; done)
  local got; got=$(lamp "p=\$(systemctl show -p MainPID --value lamp-voice); sudo -n readlink /proc/\$p/exe; sudo -n cat /proc/\$p/environ | tr '\\0' '\\n' | grep -E '^(${names%|})=' | sort | tr '\\n' ' '")
  echo "preflight $name: $got" | tee -a "$LOG"
  for e in $envs; do [[ " $got " == *" $e "* ]] || { echo "preflight: $e not set" | tee -a "$LOG"; exit 6; }; done
  [[ -z $VOICE_BIN || $got == "$VOICE_BIN"* ]] || { echo "preflight: lamp-voice is not $VOICE_BIN" | tee -a "$LOG"; exit 6; }
  local s; s=$(lamp "date +%s")
  local files=() t steps plan f
  for t in $(seq 1 "$TRIALS"); do
    echo "=== arm $name scene $t $(date +%H:%M:%S)" | tee -a "$LOG"
    plan=$(mktemp)
    steps=(); while IFS= read -r l; do steps+=("$l"); done < <(python3 ./desk_kit.py scene "$t" "$plan")
    ./scenario.py "desk-kit-$LABEL-$name-$t" "${steps[@]}" >/dev/null 2>&1
    f=$(ls -t "$OUT"/*-"desk-kit-$LABEL-$name-$t.txt" 2>/dev/null | head -1)
    [[ -n $f ]] || { echo "desk-kit.sh: scene $t of $name left no scenario file" | tee -a "$LOG"; rm -f "$plan"; exit 7; }
    mv "$plan" "$f.plan"; files+=("$f")
    ((t < TRIALS)) && sleep 70  # past the busy-room count and the conversation's window: a quiet room again
  done
  python3 ./desk_kit.py score --arm "$LABEL/$name" --tsv "$OUT/desk-kit.tsv" "${files[@]}" | tee -a "$LOG"
  echo "== journal reasons, arm $name" | tee -a "$LOG"
  lamp "sudo -n journalctl -u lamp-voice --since @$s --no-pager" | grep -oE "second: request [0-9]+: (its answer is played: .{0,90}|held.{0,90}|.{0,40}(not spoken to|left alone|stays quiet).{0,60})" |
    sed -E 's/request [0-9]+: //' | sort | uniq -c | sort -rn | head -15 | tee -a "$LOG"
  sleep 70
}

if [[ -n $FACE ]]; then
  "$CHROME" --user-data-dir="$PROFILE" --no-first-run --allow-file-access-from-files \
    --kiosk --window-position=0,0 --window-size="$FACE_SIZE" "file://$FACE" >/dev/null 2>&1 &
  FACE_PID=$!; sleep 4; screencapture -x -D1 "$OUT/desk-kit-$LABEL-shown.png" 2>/dev/null
else
  echo "desk-kit.sh: no FACE given, no face shown" | tee -a "$LOG"
fi

for a in $ARMS; do
  arm "$a"
  if lamp "sudo -n journalctl -u lamp-voice --since -10min --no-pager | grep -q 'HTTP 429'"; then echo "HTTP 429 from the relay: stopping" | tee -a "$LOG"; exit 5; fi
done
echo "done $(date +%H:%M:%S); scores in $OUT/desk-kit.tsv" | tee -a "$LOG"
