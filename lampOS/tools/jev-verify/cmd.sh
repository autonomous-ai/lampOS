#!/usr/bin/env bash
# cmd.sh preflight|run OUT_DIR: plays the device-command set (commands.txt) from the Mac speaker to the
# lamp on today's path (#4861: the first builder's measure), PASSES times, and records per line the voice's
# verdict, the device calls it made, light/state after the turn, and the time from the end of the clip to
# the lamp's first sound (voice/state speaking). Afterwards it pulls `lamp-voice turn-times` for the run's
# session (end-of-speech decision to first sound, numbers and function names only) for cmd-score.sh.
#   preflight: every check, the restore once and the wake, then it stops (no labelled line is played).
#   run:       the same, then the passes.
# Run it only inside your own booked slot, under the lamp lock (the preflight checks the holder).
# What it changes on the lamp, each put back: the Speaker is set to 50% for the run, and the lines change the
# light and the volume. After every line, and on exit, /run/jev-cmd-restore.py puts the light (through
# lamp-actions' own calls, so its state stays in step) and the Speaker back, then light/state is checked
# against the start. A dead-man (a systemd-run timer, DEADMAN min) runs the same restore if the Mac is gone;
# it is stopped once the exit restore is checked. A loans.txt line is written at the start and one at the end.
# Nothing else is touched: no unit, drop-in, skill or Jev setting. Lamp-policy is not read or restarted.
# OUT_DIR holds what the lamp heard in the window: keep it on the Mac, never commit or post it.
# Env: LAMP (lamp-52e6), MACVOL (45, raised to 60/75 if the wake is held back), PASSES (2), ONLY (an id
# regex), DEADLINE (epoch s on the Mac), DEADMAN (minutes, 75), DRY=1 with DRY_SSH=<stub> (tests).
set -u
MODE=${1:-} OUT=${2:-}
case $MODE in preflight|run) [ -n "$OUT" ] ;; *) false ;; esac || { sed -n 2,8p "$0"; exit 2; }
mkdir -p "$OUT" && OUT=$(cd "$OUT" && pwd) || exit 1
git -C "$OUT" rev-parse --is-inside-work-tree >/dev/null 2>&1 && { echo "OUT $OUT is inside a git work tree: put it outside the repo" >&2; exit 2; }
cd "$(dirname "$0")" || exit 1
LAMP=${LAMP:-lamp-52e6} MACVOL=${MACVOL:-45} PASSES=${PASSES:-2} ONLY=${ONLY:-.} HOLDER=jev-verifier
DEADLINE=${DEADLINE:-} DEADMAN=${DEADMAN:-75} SET=commands.txt
GAP=22   # a line within 20 s of the lamp's last words may be taken as going on with it
SO="-o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=10 -o ServerAliveCountMax=3 -o ControlMaster=auto -o ControlPath=/tmp/jev-cm-%C -o ControlPersist=120"
L="ssh -n $SO orangepi@$LAMP.local"
LI="ssh $SO orangepi@$LAMP.local"
V='request [0-9]+: its answer is (played|held back)'
BR=/run/lampos-bus RESTORE=/run/jev-cmd-restore.py TIMER=jev-cmd-deadman
if [ -n "${DRY:-}" ]; then
  ssh() { "${DRY_SSH:?DRY=1 needs DRY_SSH, a stub ssh}" "$@"; }
  export LAMP_SSH=$DRY_SSH
  afplay() { echo "[dry] afplay $*"; [ -z "${DRY_PLAYLOG:-}" ] || echo "$*" >> "$DRY_PLAYLOG"; }
  osascript() { case $* in *"output muted"*) echo "${DRY_MACMUTED:-false}" ;; *"get volume"*) echo 30 ;; *) echo "[dry] osascript $*" >&2 ;; esac; }
  sleep() { :; }
fi
rm -f "$OUT/aborted"; : > "$OUT/run.txt"
PLAYED=0 PULLED= SPK0= SPK50= LIGHT0= MACVOL_WAS= start_s= LOANED= ARMED= VD= REQ=

note() { echo "$*" | tee -a "$OUT/run.txt"; }
ok() { note "ok $*"; }
abort() { local m="nothing ran"; [ "$PLAYED" = 0 ] || m="$PLAYED lines played, their results pulled"
  echo "ABORT $*; $m" | tee "$OUT/aborted" >> "$OUT/run.txt"; cat "$OUT/aborted"; exit 1; }
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'; }
lamp_ms() { $L 'date +%s%3N'; }
vol() { osascript -e "set volume output volume $1"; }
play() { afplay "clips/$1.wav"; }
ids() { awk -F'|' '!/^#/ && NF >= 8 {print $1}' "$SET" | grep -E "$ONLY"; }
field() { awk -F'|' -v i="$1" -v n="$2" '$1 == i {print $n}' "$SET"; }
due() { [ -n "$DEADLINE" ] && [ "$(date +%s)" -ge "$DEADLINE" ]; }
loans() { $L "echo \"\$(date '+%F %H:%M') $HOLDER: $*\" | sudo -n tee -a ~/loans.txt >/dev/null"; }
order() { local id; for id in $(ids); do echo "$(md5 -q -s "$1$id") $id"; done | sort | cut -d' ' -f2; }
wait_until() { local t=$1 d; [ -n "$DEADLINE" ] && [ "$DEADLINE" -lt "$t" ] && t=$DEADLINE
  d=$(( t - $(date +%s) )); [ "$d" -le 0 ] || sleep "$d"; }

# ---- lamp reads, as replay.sh
nv() { $L "sudo -n journalctl -u lamp-voice --since @$1 --no-pager -o cat | grep -cE '$V'"; }
jwait() { $LI "sudo -n sh -s -- '$1' '$2' '$3' '${4:-30}' '${5:-1}' '${6:-1}'" <<'EOF'
e=$(( $(date +%s) + $4 ))
while c=$(journalctl -u "$1" --since "$2" --no-pager -o cat 2>/dev/null | grep -cE "$3"); [ "$c" -lt "$6" ]; do
  [ "$(date +%s)" -ge "$e" ] && { echo "$c"; exit 1; }; sleep "$5"; done; echo "$c"
EOF
}
muted() { $L "sudo -n journalctl _PID=\$(systemctl show -p MainPID --value lamp-voice) --no-pager -o cat | grep -c 'muted by the restart guard'; true"; }
heard() { $L "sudo -n journalctl _PID=\$(systemctl show -p MainPID --value lamp-voice) --since -20s --no-pager -o cat | grep -acE 'request [0-9]+ heard [1-9][0-9]* words'; true"; }
face() { local n; n=$($L "sudo -n journalctl -u lamp-eyes --since -15s --no-pager -o cat | grep -oE 'a face in [0-9]+' | tail -1 | grep -oE '[0-9]+$'")
  if [ -z "$n" ]; then echo "no eyes report"; elif [ "$n" -gt 0 ]; then echo "a face (in $n)"; else echo "no face"; fi; }
standing() { local s; s=$($L "sudo -n journalctl -u lamp-body --since -35s --no-pager -o cat | grep -oE 'stand [0-9.]+' | tail -1")
  note "lamp-body: ${s:-no stand line}"; [ -n "$s" ] && [ "$s" != "stand 0.00" ]; }
# verdict SINCE COUNT: "VERDICT REQUEST", the voice's first new verdict within 25 s (played|held|none) and its
# request (none if not known).
verdict() { local last q
  jwait lamp-voice "@$1" "$V" 25 1 $(( $2 + 1 )) >/dev/null || { echo none none; return; }
  last=$($L "sudo -n journalctl -u lamp-voice --since @$1 --no-pager -o cat | grep -oE '$V' | tail -1")
  q=$(printf '%s' "$last" | grep -oE '^request [0-9]+' | grep -oE '[0-9]+')
  case $last in *played*) echo "played ${q:-none}" ;; *) echo "held ${q:-none}" ;; esac; }
# calls REQ SINCE: the device functions the voice asked lamp-actions for in that request (worker.rs: "request N
# asked for NAME"), comma separated.
calls() { [ "$1" != none ] || return 0
  $L "sudo -n journalctl -u lamp-voice --since @$2 --no-pager -o cat | grep -oE 'request $1 asked for [a-z_]+' | grep -oE '[a-z_]+\$'" |
    grep -E '^(light|volume)_' | paste -sd, -; }
light() { $L "sudo -n tail -n +2 $BR/light/state/latest 2>/dev/null" | jq -c '{on,colour,level}' 2>/dev/null; }
spk() { $L "amixer -c device_cmedia sget Speaker" | grep -oE 'Playback [0-9]+ \[' | head -1 | grep -oE '[0-9]+'; }
speech_done() { $LI "sudo -n sh -s -- '$BR' voice/state" <<'EOF'
s=0 q=0 i=0
while [ $q -lt 2 ] && [ $i -lt 60 ]; do
  if tail -n +2 "$1/$2/latest" 2>/dev/null | grep -q speaking; then s=$((s+1)); q=0; else q=$((q+1)); fi
  i=$((i+1)); sleep 1; done; echo $s
EOF
}
# first_sound SECS: on the lamp, polls voice/state every 0.1 s for up to SECS; prints the header's at_unix_ms
# when it first reads speaking (the lamp's clock), or none.
first_sound() { $LI "sudo -n sh -s -- '$BR/voice/state/latest' '$1'" <<'EOF'
i=0 n=$(( $2 * 10 ))
while [ $i -lt $n ]; do o=$(cat "$1" 2>/dev/null)
  if printf '%s\n' "$o" | tail -n +2 | grep -q speaking; then
    printf '%s\n' "$o" | head -1 | grep -oE '"at_unix_ms":[0-9]+' | grep -oE '[0-9]+$'; exit 0; fi
  i=$((i+1)); sleep 0.1; done; echo none
EOF
}

# ---- the restore: lamp-actions' own calls back to the start light, then the Speaker raw value; prints light/state
PY=$(cat <<'EOF'
import json, socket, subprocess, sys, time
want, raw = json.loads(sys.argv[1]), sys.argv[2]
def call(name, args=None):
    s = socket.socket(socket.AF_UNIX); s.settimeout(5); s.connect("/run/lampos-actions/call.sock")
    s.sendall((json.dumps({"op": "call", "name": name, "args": args or {}}) + "\n").encode())
    r = s.makefile().readline(); s.close(); return r
def light():
    with open("/run/lampos-bus/light/state/latest") as f:
        f.readline(); return json.loads(f.read())
call("light_colour", {"colour": want["colour"]}); time.sleep(0.8)
for _ in range(6):
    now = light()["level"]
    if now == want["level"]: break
    call("light_brighter" if now < want["level"] else "light_dimmer"); time.sleep(0.8)
call("light_on" if want["on"] else "light_off"); time.sleep(0.8)
subprocess.run(["amixer", "-q", "-c", "device_cmedia", "sset", "Speaker", raw], check=False)
l = light(); print(json.dumps({"on": l["on"], "colour": l["colour"], "level": l["level"]}, separators=(",", ":")))
EOF
)
# restore RAW: runs the restore; true only if light/state reads LIGHT0 and the Speaker reads RAW after it.
restore() { local got s; got=$($L "sudo -n python3 $RESTORE '$LIGHT0' '$1'" | tail -1); s=$(spk)
  [ "$got" = "$LIGHT0" ] && [ "$s" = "$1" ] || { note "restore: light/state ${got:-none} (want $LIGHT0), Speaker ${s:-none} (want $1)"; return 1; }; }

pull() { local sd; PULLED=1; sleep 3
  $L "sudo -n journalctl -u lamp-voice --since @$start_s --no-pager -o short-unix | grep -E '$V|asked for'" > "$OUT/voice.txt"
  sd=$($L "sudo -n ls -td /var/lib/lampOS/voice/sessions/*/ 2>/dev/null | head -1")
  if [ -n "$sd" ]; then
    $L "sudo -n /opt/lampOS/voice/current/lamp-voice turn-times '$sd' /var/lib/lampOS/voice/first-output" > "$OUT/turn-times.json" 2>"$OUT/turn-times.err"
  else note "turn-times NOT PULLED: no session folder"; fi
  note "pulled: $(( $(wc -l < "$OUT/plays.tsv") - 1 )) lines played, $(wc -l < "$OUT/voice.txt" | tr -d ' ') voice lines, turn-times $(wc -c < "$OUT/turn-times.json" 2>/dev/null | tr -d ' ') bytes"; }
cleanup() { [ -n "$MACVOL_WAS" ] && vol "$MACVOL_WAS"
  if [ -n "$ARMED" ]; then
    if restore "$SPK0"; then note "light and Speaker back ($LIGHT0, Speaker $SPK0)"
      $L "sudo -n systemctl stop $TIMER.timer; sudo -n rm -f $RESTORE" && note "dead-man stopped"
    else note "LIGHT OR SPEAKER NOT PUT BACK: the dead-man ($TIMER.timer) stays armed and runs the restore again"; fi
  elif [ -n "$SPK0" ]; then $L "amixer -q -c device_cmedia sset Speaker $SPK0" && note "lamp Speaker back to $SPK0"; fi
  [ "$PLAYED" -gt 0 ] && [ -z "$PULLED" ] && pull
  if [ -n "$LOANED" ]; then
    if [ -e "$OUT/aborted" ]; then
      if [ "$PLAYED" = 0 ]; then loans "aborted, nothing ran; $LAMP back"; else loans "aborted after $PLAYED lines played, results pulled; $LAMP back"; fi
    elif [ "$MODE" = preflight ]; then loans "preflight only, no labelled line played; $LAMP back"
    else loans "done ($PLAYED lines played); $LAMP back"; fi
  fi; true; }
trap cleanup EXIT
trap 'exit 130' INT TERM

n=$(ids | wc -l | tr -d ' ')
note "cmd $MODE: lamp=$LAMP lines=$n x passes $PASSES; ETA of run ~$(( (n * 40 * PASSES + 150) / 60 )) min"

# ---- preflight: read-only checks, the start state, the dead-man and one restore, then the wake
miss=; for id in $(ids) w00; do [ -s "clips/$id.wav" ] || miss="$miss $id"; done
[ -z "$miss" ] || abort "clips missing:$miss (run SET=commands.txt render.sh)"
ok "clips: every line in $SET and w00"

lk=$(../lamp-lock.sh "$LAMP" status 2>&1)
printf '%s\n' "$lk" | grep -qE "^ *holder: $HOLDER\$" ||
  abort "the lamp lock is not held by $HOLDER (run under lamp-lock.sh run): $(printf '%s' "$lk" | head -1)"
ok "lock held by $HOLDER"
loans "#4861 device-command set ($MODE), voice, light and volume$([ -n "$DEADLINE" ] && date -r "$DEADLINE" '+, until %H:%M')" && LOANED=1 && note "loans.txt: start line written"
due && abort "the deadline has passed already"

st=$($L "systemctl is-active lamp-voice lamp-actions lamp-body lamp-eyes; true" | tr '\n' ' ')
[ "$st" = "active active active active " ] || abort "a unit is not active (lamp-voice lamp-actions lamp-body lamp-eyes: $st)"
ok "units active: lamp-voice lamp-actions lamp-body lamp-eyes"
di=$($L "ls -d /run/systemd/system/lamp-voice.service.d/*.conf /run/systemd/system/lamp-actions.service.d/*.conf 2>/dev/null; true" | tr '\n' ' ')
[ -z "$di" ] || abort "a /run drop-in is on lamp-voice or lamp-actions (another run?): $di"
ok "no /run drop-in on lamp-voice lamp-actions"
$L "sudo -n systemctl list-timers --all --no-legend 2>/dev/null" | grep -q "$TIMER" && abort "a $TIMER timer is already there (a run that did not put back?)"

# Today's product path: say-at-once in the voice, short replies in lamp-actions, in the live processes.
for u in lamp-voice:LAMP_SECOND_SAY_WITH_CALL lamp-actions:LAMP_SHORT_DONE; do
  e=$($L "sudo -n sh -c \"tr '\\\\0' '\\\\n' < /proc/\$(systemctl show -p MainPID --value ${u%%:*})/environ\"" | grep -E "^${u#*:}=")
  [ "$e" = "${u#*:}=on" ] || abort "${u%%:*}'s process has no ${u#*:}=on (${e:-unset}): not today's product path"
done
ok "LAMP_SECOND_SAY_WITH_CALL=on in lamp-voice, LAMP_SHORT_DONE=on in lamp-actions (live environ)"
$L "systemctl cat lamp-voice lamp-actions" > "$OUT/units.txt" && note "units recorded (units.txt)"
b=$($L "sudo -n grep -o '\"build\":{[^}]*}' /var/lib/lampos-update/state.json 2>/dev/null; readlink /opt/lampOS/voice/current /opt/lampOS/actions/current 2>/dev/null | tr '\n' ' '")
note "installed_build=${b//$'\n'/ }"

m=$(muted); [ "${m:-x}" = 0 ] || abort "the voice is muted by the restart guard (MainPID journal: ${m:-no answer})"
$L "sudo -n cat $BR/voice/mute/latest 2>/dev/null" | grep -q '"muted":true' && abort "the voice says it is muted (voice/mute on the bus)"
ok "voice not muted"

k=0; while h=$(heard); [ "${h:-0}" != 0 ] && [ $k -lt 60 ]; do
  [ $k = 0 ] && note "room not quiet ($h requests heard words in 20 s): waiting up to 60 s"; sleep 5; k=$((k+5)); done
[ "${h:-0}" = 0 ] || abort "the room is not quiet ($h requests heard words in the last 20 s after 60 s)"
ok "room quiet (no words heard in 20 s)"
case "$(face)" in "a face"*) abort "lamp-eyes saw a face in the last 15 s (a real person near the lamp?): we never record people" ;;
  "no eyes report") abort "lamp-eyes printed no report in 15 s: a face could not be seen" ;; esac
ok "no face seen in 15 s"

LIGHT0=$(light); case $LIGHT0 in '{"on":'*'"level":'*'}') ;; *) abort "light/state cannot be read (${LIGHT0:-none})" ;; esac
c=$(printf '%s' "$LIGHT0" | jq -r .colour)
case " yellow red green blue cyan purple orange pink white warm " in *" $c "*) ;; *) abort "light/state's colour '$c' is not one light_colour can set back" ;; esac
SPK0=$(spk); [ -n "$SPK0" ] || abort "cannot read the lamp's Speaker level"
note "start state: light/state $LIGHT0, Speaker raw $SPK0"

# The dead-man first, then the run's Speaker: if the Mac is gone, the timer restores the start state.
printf '%s\n' "$PY" | $LI "sudo -n tee $RESTORE >/dev/null" || abort "cannot write $RESTORE"
$L "sudo -n systemd-run --quiet --unit=$TIMER --on-active=${DEADMAN}m /usr/bin/python3 $RESTORE '$LIGHT0' '$SPK0'" ||
  abort "cannot arm the dead-man (sudo -n systemd-run refused?)"
ARMED=1; ok "dead-man armed: $TIMER.timer restores the start state in $DEADMAN min unless stopped"
$L "amixer -q -c device_cmedia sset Speaker 50%" || abort "cannot set the lamp's Speaker to 50%"
SPK50=$(spk); ok "lamp Speaker 50% (raw $SPK50; was $SPK0, put back on exit)"
restore "$SPK50" || abort "the restore did not give back the start state (see above): no line is played"
ok "restore checked once: light/state $LIGHT0, Speaker $SPK50"

[ "$(osascript -e 'output muted of (get volume settings)')" = false ] || abort "the Mac's output is muted (or cannot be read)"
MACVOL_WAS=$(osascript -e 'output volume of (get volume settings)')
vol "$MACVOL"; ok "Mac volume $MACVOL (was $MACVOL_WAS, put back on exit)"

rtt=999999 off= lb=
for _ in 1 2 3 4 5; do m0=$(now_ms); l0=$(lamp_ms); m1=$(now_ms)
  [ -n "$l0" ] && [ $(( m1 - m0 )) -lt "$rtt" ] && rtt=$(( m1 - m0 )) off=$(( l0 - (m0 + m1) / 2 )) lb=$l0; done
[ -n "$off" ] && [ "$rtt" -le 1000 ] || abort "cannot read the lamp's clock in under 1 s (best round trip ${rtt} ms)"
start_s=$(( lb / 1000 - 5 ))
note "offset_ms=$off rtt_ms=$rtt start_s=$start_s"

woke=; v=$MACVOL
for k in 1 2 3 4; do s=$(( $(lamp_ms) / 1000 )); c=$(nv "$s"); vol "$v"; play w00; read -r w _ <<<"$(verdict "$s" "${c:-0}")"
  case $w in
    played) note "wake: answered at Mac $v (try $k)"; sleep 6; standing && { woke=1; break; } ;;
    held) note "wake: held back at Mac $v (try $k)"; for x in 60 75; do [ "$x" -gt "$v" ] && { v=$x; break; }; done ;;
    *) note "wake: no verdict at Mac $v (try $k): replayed at the same level"; sleep 5 ;;
  esac
done
[ -n "$woke" ] || abort "the lamp was not woken and standing after 4 wake tries (last Mac $v)"
MACVOL=$v; ok "woken and standing at Mac $MACVOL; labelled lines play at Mac $MACVOL"
[ "$MODE" = preflight ] && { note "preflight done"; exit 0; }
speech_done >/dev/null; sleep "$GAP"

# ---- passes. trial ID: the face check, the clip with a first-sound poll from its end, the verdict, the calls,
# light/state after the answer, then the restore (a failed one stops the run).
printf 'pass\tid\tlamp_end_ms\tvoice_seen\trequest\tcalls\tlight_after\tfirst_sound_ms\n' > "$OUT/plays.tsv"
trial() { local id=$1 s c b fs fp la cl
  due && abort "deadline reached before $id (pass $pass)"
  case "$(face)" in "a face"*|"no eyes report") abort "before $id (pass $pass): a face seen, or no eyes report, in 15 s" ;; esac
  s=$(( $(lamp_ms) / 1000 )); c=$(nv "$s")
  play "$id"; b=$(( $(now_ms) + off ))
  first_sound 15 > "$OUT/.fs" & fp=$!
  read -r VD REQ <<<"$(verdict "$s" "${c:-0}")"; wait "$fp"; fs=$(cat "$OUT/.fs"); PLAYED=$((PLAYED + 1))
  speech_done >/dev/null; sleep 2
  cl=$(calls "$REQ" "$s"); la=$(light)
  case $fs in ''|none) fs=none ;; *) fs=$(( fs - b )) ;; esac
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$pass" "$id" "$b" "$VD" "$REQ" "${cl:-none}" "${la:-none}" "$fs" >> "$OUT/plays.tsv"
  echo "$(date +%H:%M:%S) pass $pass $id voice=$VD calls=${cl:-none} first_sound_ms=$fs"
  restore "$SPK50" || abort "after $id (pass $pass): the restore did not give back the start state"; }
for pass in $(seq 1 "$PASSES"); do
  for id in $(order "$pass"); do trial "$id"; wait_until $(( $(date +%s) + GAP )); done
done
pull
