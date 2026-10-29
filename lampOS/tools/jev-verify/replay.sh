#!/usr/bin/env bash
# replay.sh OUT_DIR: plays the labelled set (lines.txt) from the Mac speaker to the lamp, quiet block
# first, then busy, and afterwards pulls that window's policy decisions and the voice's verdicts into
# OUT_DIR for score.sh. Run it only inside your own booked slot, under the lamp lock:
#   tools/lamp-lock.sh lamp-52e6 run <holder> <session> "jev-verify set" -- tools/jev-verify/replay.sh OUT
# It changes nothing on the lamp (no unit, skill, file or volume), so there is nothing to put back.
# OUT_DIR holds what the lamp heard in the window: keep it on the Mac, never commit or post it.
# Env: LAMP (lamp-52e6), MACVOL (45), ONLY (a regex of ids to play, default all).
set -u
cd "$(dirname "$0")" || exit 1
OUT=${1:?usage: replay.sh OUT_DIR}
LAMP=${LAMP:-lamp-52e6} MACVOL=${MACVOL:-45} ONLY=${ONLY:-.}
L="ssh -n -o BatchMode=yes -o ConnectTimeout=15 orangepi@$LAMP.local"
V='its answer is (played|held back)'
mkdir -p "$OUT" || exit 1
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'; }
lamp_ms() { $L 'date +%s%3N'; }
verdicts() { $L "sudo -n journalctl -u lamp-voice --since @$1 --no-pager -o cat | grep -cE '$V'"; }
# wait_verdict <since_s> <count_before>: waits up to 25 s for a new verdict; prints played|held|none.
wait_verdict() { local k last
  for k in $(seq 1 25); do sleep 1
    [ "$(verdicts "$1")" -gt "$2" ] || continue
    last=$($L "sudo -n journalctl -u lamp-voice --since @$1 --no-pager -o cat | grep -oE '$V' | tail -1")
    case $last in *played*) echo played ;; *) echo held ;; esac; return; done
  echo none; }
play() { [ -s "clips/$1.wav" ] || { echo "ABORT no clip $1: run render.sh" >&2; exit 1; }; afplay "clips/$1.wav"; }
talk() { play t01; play t02; }

[ "$($L 'systemctl is-active lamp-voice lamp-policy' | sort -u)" = active ] ||
  { echo "ABORT lamp-voice or lamp-policy is not active; nothing ran" | tee "$OUT/aborted"; exit 1; }
osascript -e "set volume output volume $MACVOL"
m0=$(now_ms); l0=$(lamp_ms); m1=$(now_ms)
off=$(( l0 - (m0 + m1) / 2 ))              # lamp clock minus Mac clock, ms
start_s=$(( l0 / 1000 - 5 ))
echo "lamp=$LAMP macvol=$MACVOL offset_ms=$off start_s=$start_s" > "$OUT/run.txt"
printf 'id\tlamp_start_ms\tlamp_end_ms\tvoice_seen\n' > "$OUT/plays.tsv"

# Wake: the lamp must answer before any labelled line (a folded lamp hears nothing).
woke=
for k in 1 2 3; do s=$(( $(lamp_ms) / 1000 )); c=$(verdicts "$s"); play w00
  [ "$(wait_verdict "$s" "$c")" = played ] && { woke=1; sleep 9; break; }; done
[ -n "$woke" ] || { echo "ABORT the lamp did not answer the wake line; nothing scored" | tee "$OUT/aborted"; exit 1; }

for room in quiet busy; do
  if [ "$room" = busy ]; then
    talk; talk
    r=$($L 'lamp-bus latest room/state 2>/dev/null' | grep -oE 'quiet|busy|unknown' | head -1)
    echo "busy block: room/state=$r" | tee -a "$OUT/run.txt"
    [ "$r" = busy ] || { echo "ABORT the room did not reach busy; busy block not played" | tee -a "$OUT/run.txt"; break; }
  else sleep 20; fi                        # a quiet room: nobody has talked for a while
  grep -v '^#' lines.txt | awk -F'|' -v r="$room" '$3==r {print $1}' | grep -E "$ONLY" | while read -r id; do
    s=$(( $(lamp_ms) / 1000 )); c=$(verdicts "$s")
    a=$(( $(now_ms) + off )); play "$id"; b=$(( $(now_ms) + off ))
    v=$(wait_verdict "$s" "$c")
    printf '%s\t%s\t%s\t%s\n' "$id" "$a" "$b" "$v" >> "$OUT/plays.tsv"
    echo "$(date +%H:%M:%S) $id voice=$v"
    [ "$v" = played ] && sleep 9 || sleep 3   # let an answer finish before the next line
    [ "$room" = busy ] && talk
  done
done

sleep 5
day0=$(date -u -r $(( start_s )) +%F); day1=$(date -u +%F)
$L "sudo -n cat /var/lib/lampos-policy/decisions-$day0.jsonl /var/lib/lampos-policy/decisions-$day1.jsonl 2>/dev/null | sort -u" |
  jq -c --argjson s "$(( start_s * 1000 ))" 'select(.at_unix_ms >= $s)' > "$OUT/decisions.jsonl"
$L "sudo -n journalctl -u lamp-voice --since @$start_s --no-pager -o short-unix | grep -E 'request [0-9]+: $V'" > "$OUT/voice.txt"
echo "done: $(($(wc -l < "$OUT/plays.tsv") - 1)) lines played, $(wc -l < "$OUT/decisions.jsonl") decisions, $(wc -l < "$OUT/voice.txt") verdicts"
