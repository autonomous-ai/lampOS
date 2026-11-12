#!/bin/sh
# Kit 5754 row 3: nobody at the lamp, comfort-break and meal-check. Run as root, as a
# transient unit so it outlives ssh (RUN.md step 5):
#   systemd-run --unit=kit5754-row3 --collect -E MODE=fast sh /root/kit5754/row3.sh
#
# MODE=fast (default): lamp-skills, on its own stock binary, is restarted with one /run
# drop-in that adds `--clock-control` (docs/skills.md, "A test's clock"). Its clock is then
# skipped SKIP_S (7500 s = 125 min) in 60 s steps, 1 s of real time apart, so each
# skill's `for` and `wait` (comfort-break 90 + 30 min, meal-check 75 + 30 min) passes on
# the skills' own clock with nobody at the desk. The wall clock is not skipped, so
# `[active]`, `[quiet]` and meal-check's 11:30-13:30 / 18:30-20:30 stay real: the row
# must run inside a meal window for meal-check. Then the drop-in goes and lamp-skills is
# restarted on its found argv. About 6 minutes.
# MODE=real: no drop-in and no restart. It records from now until END (HHMM, default the
# end of the meal window it starts in, else now + 125 min). Start it by 11:35 or 18:35.
#
# The verdict lines are made on the Mac by kit5754.py from the copied recordings.
# This script prints a lamp-side reading of the journal, which is not the verdict.
set -u
. /root/kit5754/common.sh
exec >> $B/logs/row3.out 2>&1
MODE=${MODE:-fast}
SKIP_S=${SKIP_S:-7500}
W=$B/row3.window
say() { echo "$(date '+%H:%M:%S') $*"; }
stop() { say "ROW3 STOP: $*"; echo "result=stop $*" >> $W; exit 1; }
[ "$(id -u)" = 0 ] || stop "not root"
[ -e $B/setup.done ] || stop "setup.sh has not finished"
[ ! -e $B/putback.done ] || stop "already put back"
[ ! -e $W ] || stop "row 3 already ran ($W): move it aside to run again"
case "$MODE" in fast|real) ;; *) stop "MODE is fast or real";; esac
not_quiet || stop "quiet hours"
N=$(now_min)
# The meal window the row starts in, if any (lunch 11:30-13:30, dinner 18:30-20:30).
MW=none; MW_END=0
if [ $N -ge 690 ] && [ $N -lt 810 ]; then MW=lunch; MW_END=810; fi
if [ $N -ge 1110 ] && [ $N -lt 1230 ]; then MW=dinner; MW_END=1230; fi
MEAL=no; installed meal-check && MEAL=yes
if [ $MODE = fast ]; then
  # The whole row inside comfort-break's [active] (09:00-20:00), and inside the meal window for meal-check.
  [ $N -ge 540 ] && [ $N -le 1188 ] || stop "fast mode runs 09:00-19:48 (comfort-break's [active] 09:00-20:00)"
  if [ $MEAL = yes ] && [ $MW = none ]; then stop "meal-check is installed: start inside 11:30-13:20 or 18:30-19:48 so its window is open"; fi
  if [ $MW != none ] && [ $((MW_END - N)) -lt 10 ]; then stop "less than 10 min left in the $MW window"; fi
else
  [ $N -ge 540 ] || stop "real mode starts after 09:00"
  if [ -n "${END:-}" ]; then END_MIN=$(mins "$END")
  elif [ $MW != none ]; then END_MIN=$((MW_END + 1))
  else END_MIN=$((N + 125)); fi
  [ $END_MIN -le 1320 ] || stop "END must be before 22:00"
fi
F=$(faces_now); set -- $F
[ "$2" = "?" ] && say "NOTE: humans/faces/tracked keeps no value now (the verdict needs the eyes' looks in the row)"
[ "$2" = 0 ] || [ "$2" = "?" ] || stop "a face at the desk now (faces near max_h: $F): nobody may sit at the lamp; hide the Mac's face"
[ "$(mute_now)" = '"muted":false' ] || stop "voice muted or silent: $(mute_now)"
in_off comfort-break && stop "comfort-break is in the off file: lift it in setup (LIFT_OFF=yes) or the row is void"

if [ $MODE = fast ]; then
  [ ! -e $DROPIN ] || stop "the clock drop-in is already there"
  T=$(date '+%Y-%m-%d %H:%M:%S')
  EX=$(cat $B/skills-execstart.txt)
  case "$EX" in "ExecStart=$SKILLS_BIN "*) ;; *) stop "lamp-skills' ExecStart doesn't run current: $EX";; esac
  mkdir -p $DD/lamp-skills.service.d
  printf '[Service]\nExecStart=\n%s --clock-control %s\n' "$EX" "$CLOCK_SOCK" > $DROPIN
  systemctl daemon-reload
  systemctl restart lamp-skills
  i=0; until [ -S $CLOCK_SOCK ]; do i=$((i+1)); [ $i -gt 30 ] && break; sleep 1; done
  P=$(skills_pid)
  say "lamp-skills: $(readlink /proc/$P/exe), argv: $(tr '\0' ' ' < /proc/$P/cmdline)"
  restore() {
    rm -f $DROPIN; rmdir $DD/lamp-skills.service.d 2>/dev/null; systemctl daemon-reload
    systemctl restart lamp-skills; sleep 5
    P=$(skills_pid)
    A=$(tr '\0' ' ' < /proc/$P/cmdline)
    if [ "$A" = "$(cat $B/skills-argv-before.txt)" ]; then say "lamp-skills back on its found argv"; else say "DIFF lamp-skills argv: $A"; fi
  }
  [ -S $CLOCK_SOCK ] || { restore; stop "lamp-skills made no clock socket"; }
  [ "$(readlink /proc/$P/exe)" = "$(cat $B/skills-exe-before.txt)" ] || { restore; stop "lamp-skills is not the stock binary it ran before"; }
  i=0; until loaded_since "$T" | grep -q .; do i=$((i+1)); [ $i -gt 20 ] && break; sleep 1; done
  L=$(loaded_since "$T"); K=$(offkept_since "$T")
  say "$L"; say "$K"
  echo "$L" | grep -q '"comfort-break"' || { restore; stop "comfort-break not loaded"; }
  echo "$K" | grep -q '"comfort-break"' && { restore; stop "comfort-break is kept off"; }
  CB=yes; ML=no; echo "$L" | grep -q '"meal-check"' && ML=yes
  # Let every subscription settle and the voice be seen resting on the skills' clock.
  sleep 20
  T0=$(now_ms)
  printf 'mode=fast\nt0_ms=%s\nt0_local=%s\nmeal_window=%s\ncomfort_loaded=%s\nmeal_loaded=%s\nskip_asked_s=%s\n' \
    "$T0" "$(date +%H:%M:%S)" "$MW" "$CB" "$ML" "$SKIP_S" > $W
  say "ROW3 start (fast): skipping $SKIP_S s on lamp-skills' clock; faces $(faces_now); voice $(vstate_now) $(mute_now)"
  $B/kit5754-skip $CLOCK_SOCK "$SKIP_S" 60 1000 $B/STOP > $B/logs/row3-skips.txt 2>&1
  RC=$?
  DONE_S=$(tail -1 $B/logs/row3-skips.txt | sed -n 's/^skipped \([0-9]*\) s.*/\1/p')
  say "skip helper: exit $RC, $(tail -1 $B/logs/row3-skips.txt)"
  # Real time for anything now due to run and be said.
  sleep 60
  T1=$(now_ms)
  printf 't1_ms=%s\nt1_local=%s\nskipped_s=%s\nskip_exit=%s\n' "$T1" "$(date +%H:%M:%S)" "${DONE_S:-0}" "$RC" >> $W
  restore
else
  T=$(date '+%Y-%m-%d %H:%M:%S')
  L=$(journalctl -u lamp-skills --no-pager -o cat 2>/dev/null | grep 'lamp-skills: loaded \[' | tail -1)
  CB=no; echo "$L" | grep -q '"comfort-break"' && CB=yes
  ML=no; echo "$L" | grep -q '"meal-check"' && ML=yes
  T0=$(now_ms)
  printf 'mode=real\nt0_ms=%s\nt0_local=%s\nmeal_window=%s\ncomfort_loaded=%s\nmeal_loaded=%s\nend_min=%s\n' \
    "$T0" "$(date +%H:%M:%S)" "$MW" "$CB" "$ML" "$END_MIN" > $W
  say "ROW3 start (real) until minute $END_MIN of the day; faces $(faces_now); voice $(vstate_now) $(mute_now)"
  while [ "$(now_min)" -lt "$END_MIN" ]; do
    [ -e $B/STOP ] && { say "stopped by $B/STOP"; break; }
    sleep 10
  done
  T1=$(now_ms)
  printf 't1_ms=%s\nt1_local=%s\n' "$T1" "$(date +%H:%M:%S)" >> $W
fi
echo "result=done" >> $W
# Lamp-side reading only (the verdict is kit5754.py's).
EV=$(awk -v a=$((T0 / 1000)) -v b=$((T1 / 1000 + 1)) '$1 + 0 >= a && $1 + 0 <= b' $B/logs/journal.txt \
  | grep -E 'lamp-skills: (comfort-break|meal-check) (Triggered|Asked|Answered|Done)|lamp-skills: (comfort-break|meal-check) not run' || true)
if [ -n "$EV" ]; then say "ROW3 lamp-side: the skills did something in the row:"; echo "$EV"
else say "ROW3 lamp-side: no Triggered, Done or not-run line for comfort-break or meal-check in the row"; fi
say "ROW3 end; copy $B to the Mac and run kit5754.py for the verdict lines"
