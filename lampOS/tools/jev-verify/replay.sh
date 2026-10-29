#!/usr/bin/env bash
# replay.sh preflight|run OUT_DIR: plays the labelled set (lines.txt) from the Mac speaker to the lamp,
# PASSES times (a quiet block, then a busy one, each pass in its own fixed order), and afterwards pulls
# that window's policy decisions and the voice's verdicts into OUT_DIR for score.sh.
#   preflight: every check and the wake, then it stops (no labelled line is played).
#   run:       the same, then the passes.
# Run it only inside your own booked slot, under the lamp lock (the preflight checks the holder):
#   tools/lamp-lock.sh lamp-52e6 run jev-verifier <session> "jev-verify set" -- tools/jev-verify/replay.sh run OUT
# The one change it makes on the lamp: its Speaker is set to 50% for the run and put back on exit.
# The Mac's output volume is put back too. No unit, drop-in, skill or file on the lamp is touched;
# lamp-policy must already run in shadow (no --act): this kit never turns --act on.
# Any failed check prints "ABORT <why>; nothing ran" and writes it to OUT/aborted.
# OUT_DIR holds what the lamp heard in the window: keep it on the Mac, never commit or post it.
# Env: LAMP (lamp-52e6), MACVOL (45, raised to 60/75 if the wake needs it), PASSES (3), ONLY (an id
# regex, default all), DRY=1 with DRY_SSH=<stub> (tests: ssh, afplay, osascript and sleep are faked).
set -u
MODE=${1:-} OUT=${2:-}
case $MODE in preflight|run) [ -n "$OUT" ] ;; *) false ;; esac || { sed -n 2,6p "$0"; exit 2; }
# OUT holds what the lamp heard: made absolute before the cd, and never inside a git work tree.
mkdir -p "$OUT" && OUT=$(cd "$OUT" && pwd) || exit 1
git -C "$OUT" rev-parse --is-inside-work-tree >/dev/null 2>&1 && { echo "OUT $OUT is inside a git work tree: put it outside the repo" >&2; exit 2; }
cd "$(dirname "$0")" || exit 1
LAMP=${LAMP:-lamp-52e6} MACVOL=${MACVOL:-45} PASSES=${PASSES:-3} ONLY=${ONLY:-.} HOLDER=jev-verifier
GAP=22   # Jev counts the lamp "in conversation" for 20 s after it last spoke
L="ssh -n -o BatchMode=yes -o ConnectTimeout=15 orangepi@$LAMP.local"
LI="ssh -o BatchMode=yes -o ConnectTimeout=15 orangepi@$LAMP.local"   # for scripts on stdin
V='request [0-9]+: its answer is (played|held back)'   # lamp-voice second (and provider)
if [ -n "${DRY:-}" ]; then
  ssh() { "${DRY_SSH:?DRY=1 needs DRY_SSH, a stub ssh}" "$@"; }
  export LAMP_SSH=$DRY_SSH   # lamp-lock.sh's ssh
  afplay() { echo "[dry] afplay $*"; [ -z "${DRY_PLAYLOG:-}" ] || echo "$*" >> "$DRY_PLAYLOG"; }
  osascript() { case $* in *"get volume"*) echo 30 ;; *) echo "[dry] osascript $*" >&2 ;; esac; }
  sleep() { :; }
fi
rm -f "$OUT/aborted"; : > "$OUT/run.txt"
PLAYED=0 PULLED= SPK0= MACVOL_WAS= start_s=

note() { echo "$*" | tee -a "$OUT/run.txt"; }
ok() { note "ok $*"; }
abort() { local m="nothing ran"; [ "$PLAYED" = 0 ] || m="$PLAYED lines played, their results pulled"
  echo "ABORT $*; $m" | tee "$OUT/aborted" >> "$OUT/run.txt"; cat "$OUT/aborted"; exit 1; }
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'; }
lamp_ms() { $L 'date +%s%3N'; }
vol() { osascript -e "set volume output volume $1"; }
play() { afplay "clips/$1.wav"; }
talk() { play t01; play t02; }
ids() { awk -F'|' -v r="$1" '!/^#/ && NF >= 7 && $3 == r {print $1}' lines.txt | grep -E "$ONLY"; }
# order PASS ROOM: the block's ids in that pass's fixed order (md5 of pass+id), so a run is repeatable.
order() { local id; for id in $(ids "$2"); do echo "$(md5 -q -s "$1$id") $id"; done | sort | cut -d' ' -f2; }
wait_until() { local d=$(( $1 - $(date +%s) )); [ "$d" -le 0 ] || sleep "$d"; }

# ---- lamp reads, copied from the Mac kits (lib.sh, onb-walk3.sh, row-faces-run2.sh)
nv() { $L "sudo -n journalctl -u lamp-voice --since @$1 --no-pager -o cat | grep -cE '$V'"; }
# jwait <unit> <since> <ERE> [secs=30] [poll=1] [n=1]: one ssh, the loop on the lamp (lib.sh: polling journalctl
# over ssh filled 52e6's journal with sudo session lines). Prints the count; returns 1 if not reached in time.
jwait() { $LI "sudo -n sh -s -- '$1' '$2' '$3' '${4:-30}' '${5:-1}' '${6:-1}'" <<'EOF'
e=$(( $(date +%s) + $4 ))
while c=$(journalctl -u "$1" --since "$2" --no-pager -o cat 2>/dev/null | grep -cE "$3"); [ "$c" -lt "$6" ]; do
  [ "$(date +%s)" -ge "$e" ] && { echo "$c"; exit 1; }; sleep "$5"; done; echo "$c"
EOF
}
muted() { $L "sudo -n journalctl _PID=\$(systemctl show -p MainPID --value lamp-voice) --no-pager -o cat | grep -c 'muted by the restart guard'; true"; }
heard() { $L "sudo -n journalctl _PID=\$(systemctl show -p MainPID --value lamp-voice) --since -20s --no-pager -o cat | grep -acE 'request [0-9]+ heard [1-9][0-9]* words'; true"; }
# face: lamp-eyes reports every 10 s; no report in 15 s is "no eyes report", never "no face".
face() { local n; n=$($L "sudo -n journalctl -u lamp-eyes --since -15s --no-pager -o cat | grep -oE 'a face in [0-9]+' | tail -1 | grep -oE '[0-9]+$'")
  if [ -z "$n" ]; then echo "no eyes report"; elif [ "$n" -gt 0 ]; then echo "a face (in $n)"; else echo "no face"; fi; }
standing() { local s; s=$($L "sudo -n journalctl -u lamp-body --since -35s --no-pager -o cat | grep -oE 'stand [0-9.]+' | tail -1")
  note "lamp-body: ${s:-no stand line}"; [ -n "$s" ] && [ "$s" != "stand 0.00" ]; }
room() { $L "$BUS latest room/state 2>/dev/null" | grep -oE 'quiet|busy|unknown' | head -1; }
# verdict SINCE COUNT: the voice's first new verdict within 25 s: played|held|none.
verdict() { local last
  jwait lamp-voice "@$1" "$V" 25 1 $(( $2 + 1 )) >/dev/null || { echo none; return; }
  last=$($L "sudo -n journalctl -u lamp-voice --since @$1 --no-pager -o cat | grep -oE '$V' | tail -1")
  case $last in *played*) echo played ;; *) echo held ;; esac; }
# speech_done: waits (on the lamp, up to 60 s) until voice/state has not read speaking for 2 s;
# prints the seconds it read speaking.
speech_done() { $LI "sh -s -- '$BUS' voice/state" <<'EOF'
s=0 q=0 i=0
while [ $q -lt 2 ] && [ $i -lt 60 ]; do
  if $1 latest "$2" 2>/dev/null | grep -q speaking; then s=$((s+1)); q=0; else q=$((q+1)); fi
  i=$((i+1)); sleep 1; done; echo $s
EOF
}
# rest VERDICT ROOM: no carryover. If the lamp spoke, the next line waits GAP s from its last speech;
# in the busy block one colleagues' exchange plays inside that gap (and resets it if the lamp answers it).
# The talk's own verdicts and decisions land within SETTLE s, before the next line starts.
SETTLE=8
rest() { local s t0 tk=0 spoke=0; s=$(speech_done); { [ "${s:-0}" -gt 0 ] || [ "$1" = played ]; } && spoke=1
  t0=$(date +%s)
  if [ "$2" = busy ]; then talk; tk=$(date +%s); s=$(speech_done); [ "${s:-0}" -gt 0 ] && { spoke=1; t0=$(date +%s); }; fi
  if [ $spoke = 1 ]; then wait_until $(( t0 + GAP )); else sleep 3; fi
  wait_until $(( tk + SETTLE )); }

pull() { local day0 day1; PULLED=1; sleep 5
  day0=$(date -u -r "$start_s" +%F); day1=$(date -u +%F)
  $L "sudo -n cat /var/lib/lampos-policy/decisions-$day0.jsonl /var/lib/lampos-policy/decisions-$day1.jsonl 2>/dev/null | sort -u" |
    jq -c --argjson s "$(( start_s * 1000 ))" 'select(.at_unix_ms >= $s)' > "$OUT/decisions.jsonl"
  $L "sudo -n journalctl -u lamp-voice --since @$start_s --no-pager -o short-unix | grep -E '$V'" > "$OUT/voice.txt"
  note "pulled: $(( $(wc -l < "$OUT/plays.tsv") - 1 )) lines played, $(wc -l < "$OUT/decisions.jsonl" | tr -d ' ') decisions, $(wc -l < "$OUT/voice.txt" | tr -d ' ') verdicts"; }
# Restores first: under lamp-lock.sh run a TERM is followed by a KILL 10 s later.
cleanup() { [ -n "$MACVOL_WAS" ] && vol "$MACVOL_WAS"
  [ -n "$SPK0" ] && $L "amixer -q -c device_cmedia sset Speaker $SPK0" && note "lamp Speaker back to $SPK0"
  [ "$PLAYED" -gt 0 ] && [ -z "$PULLED" ] && pull; true; }
trap cleanup EXIT
trap 'exit 130' INT TERM

# ---- ETA: a line the voice answers ~37 s (clip, verdict, answer, GAP), one it holds ~12 s, +12 s of talk in busy
nl=$(ids quiet | wc -l | tr -d ' ') nb=$(ids busy | wc -l | tr -d ' ')
eta=$(awk -F'|' -v o="$ONLY" '!/^#/ && NF >= 7 && $1 ~ o { t += ($5 == "lamp") ? 37 : ($3 == "busy" ? 24 : 12) }
  END { print t + 0 }' lines.txt)
eta=$(( (eta + 120) * PASSES + 90 ))   # + per pass 20 s quiet lead, 2 exchanges, ~60 s for busy to end; + wake and pull
note "replay $MODE: lamp=$LAMP lines=$((nl + nb)) (quiet $nl, busy $nb) x passes $PASSES; ETA of run ~$(( eta / 60 )) min"

# ---- preflight: read-only checks first, then the two volume changes, then the wake
miss=; for id in $(awk -F'|' '!/^#/ && NF >= 7 {print $1}' lines.txt) w00 t01 t02; do
  [ -s "clips/$id.wav" ] || miss="$miss $id"; done
[ -z "$miss" ] || abort "clips missing:$miss (run render.sh)"
ok "clips: every line in lines.txt and w00 t01 t02"

lk=$(../lamp-lock.sh "$LAMP" status 2>&1)
printf '%s\n' "$lk" | grep -qE "^ *holder: $HOLDER\$" ||
  abort "the lamp lock is not held by $HOLDER (run under lamp-lock.sh run): $(printf '%s' "$lk" | head -1)"
ok "lock held by $HOLDER"

st=$($L "systemctl is-active lamp-voice lamp-policy lamp-body lamp-eyes; true" | tr '\n' ' ')
[ "$st" = "active active active active " ] || abort "a unit is not active (lamp-voice lamp-policy lamp-body lamp-eyes: $st)"
ok "units active: lamp-voice lamp-policy lamp-body lamp-eyes"

di=$($L "ls -d /run/systemd/system/lamp-voice.service.d/*.conf /run/systemd/system/lamp-policy.service.d/*.conf /run/systemd/system/lamp-body.service.d/*.conf 2>/dev/null; true" | tr '\n' ' ')
[ -z "$di" ] || abort "a foreign /run drop-in is on lamp-voice, lamp-policy or lamp-body (another run?): $di"
ok "no /run drop-in on lamp-voice lamp-policy lamp-body"

m=$(muted); [ "${m:-x}" = 0 ] || abort "the voice is muted by the restart guard (MainPID journal: ${m:-no answer})"
$L "sudo -n cat /run/lampos-bus/voice/mute/latest 2>/dev/null" | grep -q '"muted":true' &&
  abort "the voice says it is muted (voice/mute on the bus): a chosen mute is never cleared here"
ok "voice not muted"

pc=$($L "sudo -n tr '\\0' ' ' < /proc/\$(systemctl show lamp-policy -p MainPID --value)/cmdline")
[ -n "$pc" ] || abort "cannot read lamp-policy's MainPID cmdline"
note "policy_cmdline=$pc"
case " $pc " in *" --act"*) abort "lamp-policy runs with --act (this kit only scores shadow): $pc" ;; esac
ok "lamp-policy in shadow (no --act)"

# The day's file is made at its first decision (crates/policy/src/record.rs, by UTC): the wake below must write one.
pl=$($L "sudo -n ls -l /var/lib/lampos-policy/ && echo READABLE")
case $pl in *READABLE*) ;; *) abort "/var/lib/lampos-policy cannot be listed with sudo -n: ${pl:-none}" ;; esac
ok "policy log dir listed ($(printf '%s\n' "$pl" | grep -c 'decisions-') decisions files)"

BUS=$($L 'for b in lamp-bus "sudo -n lamp-bus" /opt/lampOS/bus/current/lamp-bus "sudo -n /opt/lampOS/bus/current/lamp-bus"; do $b latest room/state >/dev/null 2>&1 && { echo "$b"; exit 0; }; done; exit 1')
[ -n "$BUS" ] || abort "no lamp-bus answers 'latest room/state' (tried lamp-bus, /opt/lampOS/bus/current/lamp-bus, with sudo -n): the busy block depends on it"
r=$(room); case $r in quiet|busy|unknown) ;; *) abort "room/state reads '${r}' via $BUS, not quiet/busy/unknown" ;; esac
ok "room/state=$r via $BUS"

k=0; while h=$(heard); [ "${h:-0}" != 0 ] && [ $k -lt 60 ]; do
  [ $k = 0 ] && note "room not quiet ($h requests heard words in 20 s): waiting up to 60 s"; sleep 5; k=$((k+5)); done
[ "${h:-0}" = 0 ] || abort "the room is not quiet ($h requests heard words in the last 20 s after 60 s): a person may be talking near the lamp"
ok "room quiet (no words heard in 20 s)"

case "$(face)" in "a face"*) abort "lamp-eyes saw a face in the last 15 s (a real person near the lamp?): we never record people" ;;
  "no eyes report") abort "lamp-eyes printed no report in 15 s: a face could not be seen" ;; esac
ok "no face seen in 15 s"

SPK0=$($L "amixer -c device_cmedia sget Speaker" | grep -oE 'Playback [0-9]+ \[' | head -1 | grep -oE '[0-9]+')   # raw: exact on restore
[ -n "$SPK0" ] || abort "cannot read the lamp's Speaker level"
$L "amixer -q -c device_cmedia sset Speaker 50%" || abort "cannot set the lamp's Speaker to 50%"
ok "lamp Speaker 50% (was $SPK0, put back on exit)"

MACVOL_WAS=$(osascript -e 'output volume of (get volume settings)')
vol "$MACVOL"; ok "Mac volume $MACVOL (was $MACVOL_WAS, put back on exit)"

m0=$(now_ms); l0=$(lamp_ms); m1=$(now_ms)
off=$(( l0 - (m0 + m1) / 2 ))              # lamp clock minus Mac clock, ms
start_s=$(( l0 / 1000 - 5 ))
note "offset_ms=$off start_s=$start_s"

# ---- wake: answered within 12 s, raising the Mac 45 -> 60 -> 75 if not (RULES: small MacBook speaker); then standing
woke=; v=$MACVOL
for k in 1 2 3; do s=$(( $(lamp_ms) / 1000 )); c=$(nv "$s"); vol "$v"; play w00
  if jwait lamp-voice "@$s" 'its answer is played' 12 1 $(( ${c:-0} + 1 )) >/dev/null; then
    note "wake: answered at Mac $v (try $k)"; sleep 6; standing && { woke=1; break; }
  else note "wake: no answer at Mac $v (try $k)"; for x in 60 75; do [ "$x" -gt "$v" ] && { v=$x; break; }; done; fi
done
[ -n "$woke" ] || abort "the lamp was not woken and standing after 3 wake tries (last Mac $v)"
MACVOL=$v; ok "woken and standing at Mac $MACVOL; labelled lines play at Mac $MACVOL"
sleep 5   # the wake's decision: asked within the 3 s deadline
wd=$($L "sudo -n cat /var/lib/lampos-policy/decisions-$(date -u +%F).jsonl 2>/dev/null | sort -u" |
  jq -c --argjson s "$(( start_s * 1000 ))" 'select(.at_unix_ms >= $s and .about == "attention")' 2>/dev/null | tail -1)
[ -n "$wd" ] || abort "lamp-policy wrote no attention decision for the wake: nothing would be scored"
wf=$(printf '%s' "$wd" | jq -r '.failed // empty')
[ -z "$wf" ] || abort "lamp-policy's decision on the wake failed ($wf): every decision would be failed"
ok "policy decided on the wake (request $(printf '%s' "$wd" | jq -r '.request // "none"'))"
if [ "$MODE" = preflight ]; then
  # Busy probe (no labelled line): whether two exchanges make room/state busy, as the busy block needs.
  rest played quiet; talk; talk; sleep "$SETTLE"; r=$(room)
  note "busy probe: room/state=$r after two exchanges$([ "$r" = busy ] || echo " (the busy block would be skipped)")"
  note "preflight done"; exit 0; fi
rest played quiet   # the wake's answer: GAP s before the first line

# ---- passes
printf 'pass\tid\tlamp_start_ms\tlamp_end_ms\tvoice_seen\tmacvol\n' > "$OUT/plays.tsv"
for pass in $(seq 1 "$PASSES"); do
  for blk in quiet busy; do
    if [ "$blk" = busy ]; then
      talk; talk; sleep "$SETTLE"; r=$(room); note "pass $pass busy block: room/state=$r"
      [ "$r" = busy ] || { note "ABORT busy block (pass $pass): room/state=$r after two exchanges; busy lines not played, quiet results kept"; continue; }
    else
      # busy ends 60 s after the last talk with LAMP_BUSY_ENDS_SOONER, 180 s without (crates/voice/src/room_told.rs)
      k=0; while r=$(room); [ "$r" != quiet ] && [ $k -lt 200 ]; do sleep 5; k=$((k+5)); done
      note "pass $pass quiet block: room/state=$r"
      [ "$r" = busy ] && { note "ABORT quiet block (pass $pass): room/state still busy after 200 s; not played"; continue; }
      sleep 20                             # a quiet room: nobody has talked for a while
    fi
    for id in $(order "$pass" "$blk"); do
      case "$(face)" in "a face"*|"no eyes report") abort "before $id (pass $pass): a face seen, or no eyes report, in 15 s" ;; esac
      s=$(( $(lamp_ms) / 1000 )); c=$(nv "$s")
      a=$(( $(now_ms) + off )); play "$id"; b=$(( $(now_ms) + off ))
      v=$(verdict "$s" "${c:-0}"); PLAYED=$((PLAYED + 1))
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$pass" "$id" "$a" "$b" "$v" "$MACVOL" >> "$OUT/plays.tsv"
      echo "$(date +%H:%M:%S) pass $pass $id voice=$v"
      rest "$v" "$blk"
    done
  done
done
pull
