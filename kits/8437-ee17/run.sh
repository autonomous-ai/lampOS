#!/bin/bash
# Mac. #8180 legs ROW on one lamp, copied for #8437's legs Y S L T (RUN-8437.md) (settings: row/kit.sh; the runner's env file: KIT_ENV). The runner installs #8180's
# CI ARM64 artifact by their own /run drop-in before, and puts the lamp back to stable themselves after: this kit
# never installs, rounds or puts back a build.
#   ./run.sh [from-step [then-step]]   steps: lock render swap check queue micon save legs tails boot unrow micoff putback release
#   LEGS="A B C D K H G P" picks the legs step's legs (default A B C D K H P N E); SKIP="tails boot" leaves steps out.
#   putback is optional (PUTBACK_CMD unset: skipped): the runner's own command re-signs stable's bytes at PUTBACK_SEQ,
#   which must be above the lamp's installed sequence (lamp-update refuses a lower or equal one), then the step waits
#   (PUTBACK_WAIT, 1200 s) for lamp-update to have installed PUTBACK_SEQ. The kit holds no key and signs nothing.
#   With then-step, the first step runs and the run goes on at then-step (e.g. `./run.sh micon boot` after the trap put
#   the mute back during the boot leg). UNTIL=<step> stops before that step (the EXIT trap still runs).
# One run goes from the step named (default: lock) to the end. On ANY exit (a STOP, a FAIL, Ctrl-C) the EXIT trap
# takes leg N's switch off (NAMEDGUARD) and puts the recorded mute back (MICGUARD), each once; the lock is kept.
# The mute is lifted (micon) only with MIC_YES=yes: the lamp's person's yes for THIS ROW. No volume is ever set;
# lamp-cloud is never stopped; GATE keeps each unit at 10 or fewer starts in any 10 min.
set -u; K=$(cd "$(dirname "$0")" && pwd); D=$K/row; . $D/kit.sh
L=$D/logs; mkdir -p $L
# ee17 (23 Nov): the lamp stays on stable; `swap` binds #8180's four parts over it, `restore` takes them away.
STEPS="lock render swap check queue micon save legs tails boot unrow restore micoff putback release"
# #8437 (RUN-8437.md): SKIP holding "swap" is a baseline run on the installed stable (leg L "before"): no binds, so
# check reads no PR sha and skips #8180's cgroup checks (CG_CHECK=0), and restore has nothing to put back.
BASE=0; case " ${SKIP:-} " in *" swap "*) BASE=1;; esac
FROM=${1:-lock}; THEN=${2:-}
[ $BASE = 1 ] && [ "$FROM" = lock ] && rm -f $L/swap.txt $L/swap-run.txt
for x in $FROM $THEN; do case " $STEPS " in *" $x "*) ;; *) echo "no step '$x' (steps: $STEPS)"; exit 2;; esac; done
T(){ echo "$(date '+%T') $*" | tee -a $L/times.log; }
need(){ grep -qE "$2" $L/$1.txt 2>/dev/null || { T "STOP at $1: no '$2' (read row/logs/$1.txt)"; exit 3; }; }
NAMEDGUARD(){ [ -f $L/named-on.txt ] || return 0; grep -q '^NAMED OFF' $L/named-off.txt 2>/dev/null && return 0
  T "exit with leg N's switch maybe on: taking it off now"; RENEW || { T "WARN: lock lost, named switch NOT off: ./run.sh unrow"; return 0; }
  GATE lamp-voice || { T "WARN: no start budget, named switch NOT off: ./run.sh unrow"; return 0; }
  LAMP "MODE=off" $D/named.lamp.sh > $L/named-off.txt 2>&1; grep -q '^NAMED OFF' $L/named-off.txt || T "WARN: named switch NOT off: ./run.sh unrow"; }
MICGUARD(){ grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null || return 0; grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null && return 0
  T "exit with the mic unmuted by the kit: putting the mute back now"; RENEW || { T "WARN: lock lost, mute NOT back: ./run.sh micoff"; return 0; }
  GATE lamp-voice || { T "WARN: no start budget, mute NOT back: wait 10 min, then ./run.sh micoff"; return 0; }
  LAMP "" $D/micoff.lamp.sh > $L/micoff.txt 2>&1; cat $L/micoff.txt; grep -q '^MUTE BACK OK' $L/micoff.txt || T "WARN: mute NOT back: ./run.sh micoff"; }
# A resumed run after the trap put the mute back: the clips can't be heard until micon runs again.
HEARD(){ grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null && grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null && { T "STOP before $1: the mute was put back after micon (row/logs/micoff.txt): run ./run.sh micon first (then UNTIL= to stop where needed)"; exit 3; }; return 0; }
SWAPNOTE(){ grep -q '^SWAPPED OK' $L/swap.txt 2>/dev/null && ! grep -q '^RESTORED OK' $L/restore.txt 2>/dev/null && T "NOTE: #8180's parts are still bound over the installed ones: ./run.sh unrow restore"; return 0; }
GUARDS(){ trap - EXIT; trap 'T "guard cut by a signal: check the named switch and the mute by hand"; exit 130' INT TERM; NAMEDGUARD; MICGUARD; SWAPNOTE; }
trap GUARDS EXIT; trap 'exit 130' INT TERM
go=0
for st in $STEPS; do
  [ $st = $FROM ] && go=1; [ -n "$THEN" ] && [ $st = $THEN ] && go=1; [ $go = 1 ] || continue
  [ -n "${UNTIL:-}" ] && [ $st = "$UNTIL" ] && { T "UNTIL $st: stop before it"; exit 0; }
  case " ${SKIP:-} " in *" $st "*) T "SKIP $st"; continue;; esac
  [ $st = lock ] || RENEW || exit 1
  T "step $st"
  case $st in
  lock)    if [ -z "$LL" ]; then echo "LOCK none (KIT_LL unset): no lock taken" | tee $L/lock.txt
           else $LL $LOCK_LAMP try $LOCK_WHO $LOCK_SESSION "#8180 legs ROW" > $L/lock.txt 2>&1; rc=$?
             [ $rc = 0 ] || { T "LOCK NOT TAKEN (try exit $rc): no ssh. $(tail -2 $L/lock.txt)"; exit 1; }
             rm -f $L/RELEASED
             ( while [ ! -f $L/RELEASED ]; do $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 && echo "$(date +%T) renewed" >> $L/renew.log || echo "$(date +%T) RENEW FAILED" >> $L/renew.log
                 sleep 60; done ) >/dev/null 2>&1 &
             echo $! > $L/renew.pid; echo "LOCK TAKEN $(date +%T); renew loop pid $(cat $L/renew.pid)" | tee -a $L/lock.txt; fi;;
  render)  bash $D/render.sh 2>&1 | tee $L/render.txt; need render '^RENDER (OK|nothing to do)';;
  swap)    bash $K/swap/swap.sh 2>&1 | tee $L/swap-run.txt; need swap '^SWAPPED OK';;
  check)   if [ $BASE = 1 ]; then LAMP "CG_CHECK=0" $D/check.lamp.sh > $L/check.txt 2>&1
           else LAMP "PR_TASKS_SHA=${PR_TASKS_SHA:-} PR_CONTROL_SHA=${PR_CONTROL_SHA:-} PR_SKILLS_SHA=${PR_SKILLS_SHA:-} PR_VOICE_SHA=${PR_VOICE_SHA:-} PR_UPDATE_SHA=${PR_UPDATE_SHA:-}" $D/check.lamp.sh > $L/check.txt 2>&1; fi; cat $L/check.txt
           sed -n '1,/^VERIFY /p' $L/check.txt > $L/verify.txt; grep '^READY\|^NOT_READY' $L/check.txt > $L/ready.txt
           need verify '^VERIFY PASS'; need ready '^READY';;
  queue)   # A kept session lamp-tasks can't close queues every make: closed by Harness's own close --now (row/queue.lamp.sh).
           LAMP "" $D/queue.lamp.sh > $L/queue.txt 2>&1; cat $L/queue.txt; need queue '^QUEUE (CLEAR|CLOSED)';;
  micon)   # The lamp's person's yes for THIS ROW only: the privacy (software) mute recorded and lifted (lamp-voice
           # stopped, the record written unmuted, started: 1 start). A closed physical slide stops here.
           [ "${MIC_YES:-}" = yes ] || { T "STOP at micon: the mute is lifted only on the lamp's person's yes for this ROW: set MIC_YES=yes"; exit 3; }
           GATE lamp-voice || { T "STOP at micon: start budget"; exit 3; }
           rm -f $L/micoff.txt
           LAMP "" $D/micon.lamp.sh > $L/micon.txt 2>&1; cat $L/micon.txt; cat $L/micon.txt | sed "s/^/$(date +%T) micon /" >> $L/mic.txt
           need micon '^MUTE (LIFTED OK|NOT MUTED)';;
  save)    VOLOK || exit 2; HEARD save
           LAMP "" $D/mute.lamp.sh > $L/mute.txt 2>&1; cat $L/mute.txt; need mute '^MUTE OFF OK'
           # KEEP_VERDICTS=1: a split rerun keeps the earlier runs' verdicts, so a later G still reads A, B and D.
           [ "${KEEP_VERDICTS:-}" = 1 ] || rm -f $L/verdicts.env
           rm -f $L/named-on.txt $L/named-off.txt
           LAMP "" $D/save.lamp.sh > $L/save.txt 2>&1; cat $L/save.txt; need save '^SAVED OK';;
  legs)    HEARD legs; bash $D/row.sh ${LEGS:-A B C D K H P N E}; T "legs: $(grep '^VERDICT' $L/row-$(echo ${LEGS:-A B C D K H P N E} | tr -d ' ').txt | cut -c1-90)";;
  tails)   HEARD tails; bash $D/row.sh ${TAILS:-F G}; T "tails: $(grep '^VERDICT' $L/row-$(echo ${TAILS:-F G} | tr -d ' ').txt | cut -c1-90)";;
  boot)    HEARD boot; bash $D/boot7170.sh 2>&1 | tee $L/boot7170.txt; T "boot: $(grep '^LEG 7170' $L/boot7170.txt)"
           grep -q '^LEG 7170 FAIL (ssh not back' $L/boot7170.txt && { T "STOP: the lamp is not back after the reboot: look at it; the trap tries to put the mute back, and once the lamp is back: ./run.sh unrow"; exit 3; };;
  unrow)   # The row's put-back: the runon and named drop-ins (1 lamp-voice start if either is there), the row's skills
           # (removed by the lamp's own lamp-skills), made.json and long-make.json back, the row's /tmp files.
           GATE lamp-voice || { T "STOP at unrow: start budget"; exit 3; }
           LAMP "" $D/unrow.lamp.sh > $L/unrow.txt 2>&1; cat $L/unrow.txt; need unrow '^UNROW OK';;
  restore) if [ $BASE = 1 ]; then echo "RESTORED OK (baseline run: swap skipped, nothing bound)" | tee $L/restore.txt
           else bash $K/swap/restore.sh 2>&1 | tee $L/restore-run.txt; need restore '^RESTORED OK'; fi;;
  micoff)  if grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null; then GATE lamp-voice || { T "STOP at micoff: start budget: wait, then ./run.sh micoff"; exit 3; }
             LAMP "" $D/micoff.lamp.sh > $L/micoff.txt 2>&1; cat $L/micoff.txt; cat $L/micoff.txt | sed "s/^/$(date +%T) micoff /" >> $L/mic.txt
             need micoff '^MUTE BACK OK'
           else T "micoff: nothing lifted by the kit"; echo "MUTE BACK OK (nothing lifted)" > $L/micoff.txt; fi;;
  putback) # Optional: stable's bytes re-signed at PUTBACK_SEQ by the runner's own PUTBACK_CMD (on this Mac; the kit
           # holds no key), needed when the ROW's sequence is at or above stable's: lamp-update refuses a lower or equal
           # sequence (lampOS docs/updates.md, "A corrective build needs a higher sequence").
           if [ -z "${PUTBACK_CMD:-}" ]; then echo "PUTBACK skipped (PUTBACK_CMD unset: the runner's own put-back)" | tee $L/putback.txt
           else [ -n "${PUTBACK_SEQ:-}" ] || { T "STOP at putback: PUTBACK_CMD needs PUTBACK_SEQ"; exit 3; }
             LAMP "MODE=read" $D/putback.lamp.sh > $L/putback.txt 2>&1; cat $L/putback.txt
             cur=$(sed -n 's/^SEQ installed=\([0-9]*\).*/\1/p' $L/putback.txt | tail -1)
             [ -n "$cur" ] && [ "$PUTBACK_SEQ" -gt "$cur" ] 2>/dev/null || { T "STOP at putback: PUTBACK_SEQ $PUTBACK_SEQ is not above the lamp's installed sequence '${cur:-unread}': re-sign stable's bytes at a higher sequence"; exit 3; }
             PUTBACK_SEQ=$PUTBACK_SEQ LAMP_HOST=$LAMP_HOST bash -c "$PUTBACK_CMD" >> $L/putback.txt 2>&1 || { T "STOP at putback: PUTBACK_CMD failed (row/logs/putback.txt)"; exit 3; }
             s0=$(date +%s); ok=0; while [ $(( $(date +%s) - s0 )) -lt ${PUTBACK_WAIT:-1200} ]; do RENEW || exit 1
               LAMP "MODE=read" $D/putback.lamp.sh > $L/putback-read.txt 2>&1
               grep -q "^SEQ installed=$PUTBACK_SEQ busy=''" $L/putback-read.txt && { ok=1; break; }; sleep ${PUTBACK_POLL:-30}; done
             cat $L/putback-read.txt >> $L/putback.txt
             [ $ok = 1 ] && echo "PUTBACK OK installed $PUTBACK_SEQ (was $cur)" | tee -a $L/putback.txt || { T "STOP at putback: $PUTBACK_SEQ not installed in ${PUTBACK_WAIT:-1200} s ($(cat $L/putback-read.txt))"; exit 3; }
           fi;;
  release) if [ -z "$LL" ]; then echo "LOCK none to release"
           else grep -q '^UNROW OK' $L/unrow.txt 2>/dev/null && grep -q '^RESTORED OK' $L/restore.txt 2>/dev/null && grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null && { [ -z "${PUTBACK_CMD:-}" ] || grep -q '^PUTBACK OK' $L/putback.txt 2>/dev/null; } || { T "no UNROW OK + RESTORED OK + MUTE BACK OK: keep the lock (FORCE=1 releases anyway)"; [ "${FORCE:-0}" = 1 ] || exit 1; }
             touch $L/RELEASED; [ -f $L/renew.pid ] && { kill $(cat $L/renew.pid) 2>/dev/null; rm -f $L/renew.pid; }
             $LL $LOCK_LAMP release $LOCK_WHO $LOCK_SESSION && echo "LOCK RELEASED $(date +%T)"; fi;;
  esac
  [ -n "$THEN" ] && [ $st = $FROM ] && [ $FROM != $THEN ] && { go=0; T "then: $THEN"; }
done
T "run.sh done: $(grep -h '^VERDICT\|^LEG 7170' $L/row-*.txt $L/boot7170.txt 2>/dev/null | cut -c1-80 | tr '\n' ';')"
