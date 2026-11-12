#!/bin/sh
# Kit 5754 row 5: comfort-break's words, with someone at the lamp (a made-up face on the
# Mac screen aimed at the lamp). Real time: comfort-break's [present] trigger needs the
# face at the desk 90 min AND the eyes' own sitting (room/activity sitting_s above 5399),
# which the eyes count in real seconds, so its clock can't be skipped here. Run as root,
# as a transient unit (RUN.md step 4):
#   systemd-run --unit=kit5754-row5 --collect sh /root/kit5754/row5.sh
# Waits up to WAIT_MIN (default 130) for `comfort-break Done`, then 45 s more for the line
# to play and its last voice/said piece. lamp-skills stays on its stock argv; nothing is
# restarted. The words are joined by kit5754.py on the Mac from the voice/said recording.
set -u
. /root/kit5754/common.sh
exec >> $B/logs/row5.out 2>&1
WAIT_MIN=${WAIT_MIN:-130}
W=$B/row5.window
say() { echo "$(date '+%H:%M:%S') $*"; }
stop() { say "ROW5 STOP: $*"; echo "result=stop $*" >> $W; exit 1; }
[ "$(id -u)" = 0 ] || stop "not root"
[ -e $B/setup.done ] || stop "setup.sh has not finished"
[ ! -e $B/putback.done ] || stop "already put back"
[ ! -e $W ] || stop "row 5 already ran ($W): move it aside to run again"
[ ! -e $DROPIN ] || stop "row 3's clock drop-in is still there"
not_quiet || stop "quiet hours"
N=$(now_min)
# Due about 90 min on, inside comfort-break's [active] 09:00-20:00 with its 30 min wait.
[ $N -ge 540 ] && [ $N -le 1080 ] || stop "start between 09:00 and 18:00, so the line is due by 19:30"
if installed meal-check; then
  # meal-check, due after 75 min at the desk inside a meal window, would take the wellbeing
  # group's 30 min first: keep comfort-break's due time (start + 88..125 min) clear of them.
  a=$((N + 88)); z=$((N + 125))
  for w in "765 840" "1185 1260"; do set -- $w
    [ $z -lt $1 ] || [ $a -gt $2 ] || stop "meal-check is installed and would come first: start so that start+90 min avoids 12:45-14:00 and 19:45-21:00"
  done
fi
in_off comfort-break && stop "comfort-break is in the off file: lift it in setup (LIFT_OFF=yes) or the row is void"
[ "$(mute_now)" = '"muted":false' ] || stop "voice muted or silent: $(mute_now)"
F=$(faces_now); set -- $F
[ "$2" != "?" ] && [ "$2" -ge 1 ] || stop "no face at the desk (faces near max_h: $F): put the made-up face on the Mac screen, aimed at the lamp"
[ "$1" -le 1 ] || say "NOTE: $1 faces in view now: comfort-break waits while anyone else is in view"
T0=$(now_ms); TJ=$(date '+%Y-%m-%d %H:%M:%S')
printf 't0_ms=%s\nt0_local=%s\nwait_min=%s\n' "$T0" "$(date +%H:%M:%S)" "$WAIT_MIN" > $W
say "ROW5 start: faces $F; voice $(vstate_now) $(mute_now); comfort-break due about $(date -d '+90 min' +%H:%M)"
END=$(( $(date +%s) + WAIT_MIN * 60 )); NEXT=0; DONE=no
while [ "$(date +%s)" -lt $END ]; do
  [ -e $B/STOP ] && { say "stopped by $B/STOP"; break; }
  if journalctl -u lamp-skills --since "$TJ" --no-pager -o cat 2>/dev/null | grep -q 'lamp-skills: comfort-break Done'; then
    DONE=yes; break
  fi
  if [ "$(date +%s)" -ge $NEXT ]; then
    NEXT=$(( $(date +%s) + 300 ))
    say "waiting: faces $(faces_now); voice $(vstate_now) $(mute_now); $(journalctl -u lamp-skills --since "$TJ" --no-pager -o cat 2>/dev/null | grep -E 'comfort-break|stretch-break|welcome-back' | tail -1)"
  fi
  sleep 15
done
[ $DONE = yes ] && sleep 45
T1=$(now_ms)
printf 't1_ms=%s\nt1_local=%s\ndone_seen=%s\nresult=done\n' "$T1" "$(date +%H:%M:%S)" "$DONE" >> $W
journalctl -u lamp-skills --since "$TJ" --no-pager -o short-iso 2>/dev/null | grep -E 'comfort-break' | tail -8
say "ROW5 end: comfort-break Done seen: $DONE. Hide the face; copy $B to the Mac and run kit5754.py for the words"
