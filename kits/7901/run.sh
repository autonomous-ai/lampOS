#!/bin/bash
# Mac. #7901 ROW on one lamp (settings: row/kit.sh; the runner's env file: KIT_ENV). The runner installs #7901's CI
# ARM64 artifact by a real round from a test channel before, and rounds the lamp back to stable's bytes after: this
# kit never installs, rounds or puts back a build (../optional/ has a binary swap, an optional path only).
#   ./run.sh [from-step [then-step]]   steps: lock render check notes micon save legs unrow micoff release
#   With then-step, the first step runs and the run goes on at then-step (e.g. `./run.sh micon legs`).
#   UNTIL=<step> stops before that step (the EXIT trap still runs).
# On ANY exit (a STOP, a FAIL, Ctrl-C) the EXIT trap puts the recorded mute back (MICGUARD), once; the lock is kept.
# The mute is lifted (micon) only with MIC_YES=yes: the lamp's person's yes for THIS ROW. No volume is ever set;
# lamp-cloud is never stopped; GATE keeps each unit at 10 or fewer starts in any 10 min.
set -u; K=$(cd "$(dirname "$0")" && pwd); D=$K/row; . $D/kit.sh
L=$D/logs; mkdir -p $L
STEPS="lock render check notes micon save legs unrow micoff release"
FROM=${1:-lock}; THEN=${2:-}
for x in $FROM $THEN; do case " $STEPS " in *" $x "*) ;; *) echo "no step '$x' (steps: $STEPS)"; exit 2;; esac; done
T(){ echo "$(date '+%T') $*" | tee -a $L/times.log; }
need(){ grep -qE "$2" $L/$1.txt 2>/dev/null || { T "STOP at $1: no '$2' (read row/logs/$1.txt)"; exit 3; }; }
MICGUARD(){ grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null || return 0; grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null && return 0
  T "exit with the mic unmuted by the kit: putting the mute back now"; RENEW || { T "WARN: lock lost, mute NOT back: ./run.sh micoff"; return 0; }
  GATE lamp-voice || { T "WARN: no start budget, mute NOT back: wait 10 min, then ./run.sh micoff"; return 0; }
  LAMP "" $D/micoff.lamp.sh > $L/micoff.txt 2>&1; cat $L/micoff.txt; grep -q '^MUTE BACK OK' $L/micoff.txt && T "mute put back (MUTE BACK OK)" || T "WARN: mute NOT back: ./run.sh micoff"; }
NOTESNOTE(){ grep -q '^NOTES ON OK' $L/notes.txt 2>/dev/null && ! grep -q '^UNROW OK' $L/unrow.txt 2>/dev/null && T "NOTE: the row's LAMP_MADE_NOTES drop-ins are still on: ./run.sh unrow before your round back to stable"; return 0; }
# A resumed run after the trap put the mute back: the clips can't be heard until micon runs again.
HEARD(){ grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null && grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null && { T "STOP before $1: the mute was put back after micon (row/logs/micoff.txt): run ./run.sh micon $1"; exit 3; }; return 0; }
GUARDS(){ trap - EXIT; trap 'T "guard cut by a signal: check the mute by hand"; exit 130' INT TERM; MICGUARD; NOTESNOTE; }
trap GUARDS EXIT; trap 'exit 130' INT TERM
go=0
for st in $STEPS; do
  [ $st = $FROM ] && go=1; [ -n "$THEN" ] && [ $st = $THEN ] && go=1; [ $go = 1 ] || continue
  [ -n "${UNTIL:-}" ] && [ $st = "$UNTIL" ] && { T "UNTIL $st: stop before it"; exit 0; }
  [ $st = lock ] || RENEW || exit 1
  T "step $st"
  case $st in
  lock)    if [ -z "$LL" ]; then echo "LOCK none (KIT_LL unset): no lock taken" | tee $L/lock.txt
           else $LL $LOCK_LAMP try $LOCK_WHO $LOCK_SESSION "#7901 ROW" > $L/lock.txt 2>&1; rc=$?
             [ $rc = 0 ] || { T "LOCK NOT TAKEN (try exit $rc): no ssh. $(tail -2 $L/lock.txt)"; exit 1; }
             rm -f $L/RELEASED
             ( while [ ! -f $L/RELEASED ]; do $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 && echo "$(date +%T) renewed" >> $L/renew.log || echo "$(date +%T) RENEW FAILED" >> $L/renew.log
                 sleep 60; done ) >/dev/null 2>&1 &
             echo $! > $L/renew.pid; echo "LOCK TAKEN $(date +%T); renew loop pid $(cat $L/renew.pid)" | tee -a $L/lock.txt; fi;;
  render)  bash $D/render.sh 2>&1 | tee $L/render.txt; need render '^RENDER (OK|nothing to do)';;
  check)   LAMP "PR_VOICE_SHA=${PR_VOICE_SHA:-} PR_SKILLS_SHA=${PR_SKILLS_SHA:-} PR_CONTROL_SHA=${PR_CONTROL_SHA:-} PR_TASKS_SHA=${PR_TASKS_SHA:-}" $D/check.lamp.sh > $L/check.txt 2>&1; cat $L/check.txt
           need check '^VERIFY PASS'; need check '^READY';;
  notes)   GATE lamp-voice lamp-control || { T "STOP at notes: start budget"; exit 3; }
           LAMP "MODE=on" $D/notes.lamp.sh > $L/notes.txt 2>&1; cat $L/notes.txt; need notes '^NOTES ON OK';;
  micon)   # The lamp's person's yes for THIS ROW only: the privacy (software) mute recorded and lifted (lamp-voice
           # stopped, the record written unmuted, started: 1 start). A closed physical slide stops here.
           [ "${MIC_YES:-}" = yes ] || { T "STOP at micon: the mute is lifted only on the lamp's person's yes for this ROW: set MIC_YES=yes"; exit 3; }
           GATE lamp-voice || { T "STOP at micon: start budget"; exit 3; }
           rm -f $L/micoff.txt
           LAMP "" $D/micon.lamp.sh > $L/micon.txt 2>&1; cat $L/micon.txt; sed "s/^/$(date +%T) micon /" $L/micon.txt >> $L/mic.txt
           need micon '^MUTE (LIFTED OK|NOT MUTED)';;
  save)    VOLOK || exit 2; HEARD save
           LAMP "" $D/mute.lamp.sh > $L/mute.txt 2>&1; cat $L/mute.txt; need mute '^MUTE OFF OK'
           rm -f $L/verdicts.env $L/skill-a.name $L/skill-c.name $L/push-*.txt $L/unpush.txt
           LAMP "" $D/save.lamp.sh > $L/save.txt 2>&1; cat $L/save.txt; need save '^SAVED OK';;
  legs)    HEARD legs; bash $D/row.sh ${LEGS:-A B C D S K}; T "legs: $(grep '^VERDICT' $L/row-$(echo ${LEGS:-A B C D S K} | tr -d ' ').txt | cut -c1-90)";;
  unrow)   # The row's put-back: the notes drop-ins (1 lamp-voice and 1 lamp-control start if there), the row's skills
           # (removed by the lamp's own lamp-skills), made.json, the legs' notes files, the row's /tmp files.
           GATE lamp-voice lamp-control || { T "STOP at unrow: start budget"; exit 3; }
           # Leg S's pushed fixture, taken off by the computer that pushed it (the door's skill.remove). K's (push.sh, PUSH
           # INSTALLED in its log) went in by the manage helper with no pairing: unrow.lamp.sh's remove_pushed takes it off.
           : > $L/unpush.txt; for f in $L/push-*.txt; do [ -f $f ] && grep -q '^PUSHED ' $f || continue; grep -q '^PUSH INSTALLED' $f && continue; s=${f##*/push-}; s=${s%.txt}
             ${PUSHCMD:-python3 $D/push.py} remove $s ${LAMP_PAIRED:-} 2>&1 | tail -1 | tee -a $L/unpush.txt; done
           grep -q 'REMOVE REFUSED' $L/unpush.txt 2>/dev/null && { T "STOP at unrow: a pushed fixture was not removed (row/logs/unpush.txt)"; exit 3; }
           LAMP "" $D/unrow.lamp.sh > $L/unrow.txt 2>&1; cat $L/unrow.txt; need unrow '^UNROW OK';;
  micoff)  if grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null; then GATE lamp-voice || { T "STOP at micoff: start budget: wait, then ./run.sh micoff"; exit 3; }
             LAMP "" $D/micoff.lamp.sh > $L/micoff.txt 2>&1; cat $L/micoff.txt; sed "s/^/$(date +%T) micoff /" $L/micoff.txt >> $L/mic.txt
             need micoff '^MUTE BACK OK'
           else T "micoff: nothing lifted by the kit"; echo "MUTE BACK OK (nothing lifted)" > $L/micoff.txt; fi;;
  release) if [ -z "$LL" ]; then echo "LOCK none to release"
           else grep -q '^UNROW OK' $L/unrow.txt 2>/dev/null && grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null || { T "no UNROW OK + MUTE BACK OK: keep the lock (FORCE=1 releases anyway)"; [ "${FORCE:-0}" = 1 ] || exit 1; }
             touch $L/RELEASED; [ -f $L/renew.pid ] && { kill $(cat $L/renew.pid) 2>/dev/null; rm -f $L/renew.pid; }
             $LL $LOCK_LAMP release $LOCK_WHO $LOCK_SESSION && echo "LOCK RELEASED $(date +%T)"; fi;;
  esac
  [ -n "$THEN" ] && [ $st = $FROM ] && [ $FROM != $THEN ] && { go=0; T "then: $THEN"; }
done
T "run.sh done: $(grep -h '^VERDICT' $L/row-*.txt 2>/dev/null | cut -c1-80 | tr '\n' ';')"
