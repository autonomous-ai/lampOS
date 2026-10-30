#!/usr/bin/env bash
# slot.sh DAY START LAST END [OUT]: replay.sh run in a booked lamp-52e6 slot. Times are HH:MM in GMT+7 on
# DAY (YYYY-MM-DD). It re-runs itself under caffeinate so the Mac stays awake:
#   SESS=<session> tools/jev-verify/slot.sh 2026-10-30 02:30 02:40 03:50
# From START it tries the lamp lock every 20 s. Not taken by LAST: "aborted, nothing ran" goes to
# loans.txt and it exits 4. Taken: PLAYING on #3336, then the run with DEADLINE = END - 4 min (the pull
# still fits), renewing the lock every 4 min; a lost lock, no renewal confirmed for 8 min (the lease is 10),
# or END - 1 min stops the run (its cleanup puts the lamp back and pulls). On exit: QUIET on #3336 with
# the result, then the lock released.
# OUT defaults to ~/jev-verify-runs/DAY-HHMM (outside any repo: it holds what the lamp heard).
set -u
[ -n "${CAFF:-}" ] || CAFF=1 exec caffeinate -dimsu "$0" "$@"   # the Mac stays awake for the whole slot
DAY=${1:?usage: slot.sh DAY START LAST END [OUT]} START=${2:?} LAST=${3:?} END=${4:?}
HERE=$(cd "$(dirname "$0")" && pwd) || exit 1
LOCK=${LOCK:-$HERE/../lamp-lock.sh} REPLAY=${REPLAY:-$HERE/replay.sh} LAMP=${LAMP:-lamp-52e6} HOLDER=jev-verifier SESS=${SESS:?SESS: the session id}
ISSUE=${ISSUE:-3336} REPO=autonomous-ai/lampOS
OUT=${5:-$HOME/jev-verify-runs/$DAY-${START/:/}}
L="ssh -n -o BatchMode=yes -o ConnectTimeout=15 orangepi@$LAMP.local"
at() { TZ=Asia/Ho_Chi_Minh date -j -f '%Y-%m-%d %H:%M:%S' "$DAY $1:00" +%s; }
hm() { TZ=Asia/Ho_Chi_Minh date +%H:%M; }
loans() { $L "echo \"\$(date '+%F %H:%M') $HOLDER: $*\" | sudo -n tee -a ~/loans.txt >/dev/null"; }
post() { gh issue comment "$ISSUE" -R "$REPO" --body "$*" >/dev/null || echo "post to #$ISSUE failed" >&2; }
t0=$(at "$START") && tl=$(at "$LAST") && te=$(at "$END") || exit 2

while [ "$(date +%s)" -lt "$t0" ]; do sleep 20; done
until "$LOCK" "$LAMP" try "$HOLDER" "$SESS" "jev-verify #4306" >/dev/null 2>&1; do
  if [ "$(date +%s)" -ge "$tl" ]; then
    echo "ABORT the lock was not free by $LAST; nothing ran"
    loans "#4306: the lock was not free by $LAST; aborted, nothing ran"; exit 4; fi
  sleep 20; done

pid=
finish() { rc=$?; [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null && { kill -TERM "$pid"; wait "$pid"; }
  # replay.sh writes its own loans.txt lines once the lock check passed; before that, this writes one.
  grep -q '^loans.txt: start line written' "$OUT/run.txt" 2>/dev/null || loans "#4306: aborted, nothing ran"
  r=$(cat "$OUT/aborted" 2>/dev/null || grep -E '^pulled:' "$OUT/run.txt" 2>/dev/null | tail -1)
  back="Speaker and lamp-policy put back"
  grep -q 'LAMP-POLICY NOT PUT BACK' "$OUT/run.txt" 2>/dev/null && back="LAMP-POLICY NOT PUT BACK: its /run drop-in needs removing by hand"
  # Busy-labelled lines played, and how many of them read room/state busy just before (the rest are background talk).
  bz=$(awk -F'|' 'NR==FNR { if ($3 == "busy") b[$1] = 1; next } FNR > 1 && ($2 in b) { n++; if ($7 == "busy") k++ }
    END { printf "busy-labelled lines played %d, room read busy before %d (the rest scored as background talk)", n, k }' \
    "$HERE/lines.txt" FS='\t' "$OUT/plays.tsv" 2>/dev/null)
  post "Jev-verifier, QUIET $LAMP $(hm): #4306 run ended (exit $rc). ${r:-no result}. ${bz:+$bz. }$back; lock released."
  "$LOCK" "$LAMP" release "$HOLDER" "$SESS" >/dev/null 2>&1 || echo "lock release failed" >&2; }
trap finish EXIT
trap 'exit 130' INT TERM

mkdir -p "$OUT" || exit 1
post "Jev-verifier, PLAYING $LAMP $(hm)–$END: #4306's labelled set from the Mac speaker, preflight then 3 passes. Changes put back on exit: lamp Speaker 50%, lamp-policy restarted in shadow with --hello --progress. QUIET follows."
DEADLINE=$(( te - 240 )) "$REPLAY" run "$OUT" > "$OUT/console.txt" 2>&1 &
pid=$!
ok_at=$(date +%s) next=$(( ok_at + 240 ))
while kill -0 "$pid" 2>/dev/null; do
  sleep 5; now=$(date +%s)
  if [ "$now" -ge $(( te - 60 )) ]; then echo "END - 1 min reached: stopping the run" >> "$OUT/console.txt"; kill -TERM "$pid"; break; fi
  if [ "$now" -ge "$next" ]; then
    "$LOCK" "$LAMP" renew "$HOLDER" "$SESS" >/dev/null 2>&1; k=$?
    case $k in
      0) ok_at=$now ;;
      1) echo "the lock was lost: stopping the run" >> "$OUT/console.txt"; kill -TERM "$pid"; break ;;
    esac
    [ $(( now - ok_at )) -ge 480 ] && { echo "no lock renewal confirmed for 8 min: stopping the run" >> "$OUT/console.txt"; kill -TERM "$pid"; break; }
    next=$(( now + $([ "$k" = 0 ] && echo 240 || echo 30) )); fi
done
wait "$pid"; rc=$?; pid=
tail -1 "$OUT/console.txt"; echo "OUT=$OUT"
exit $rc
