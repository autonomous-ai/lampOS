#!/bin/bash
# Mac. #8237 ROW on one OpenClaw lamp from stable (settings: row/kit.sh; the runner's env file: KIT_ENV).
#   ./run.sh [from-step [then-step]]   steps: lock render put check push queue legs unpush putback release
#   UNTIL=<step> stops before that step. On ANY exit after PUT OK with no PUTBACK OK, the EXIT trap runs putback
#   once (lock renewed and start budget checked first); the lock is kept until release.
set -u; K=$(cd "$(dirname "$0")" && pwd); D=$K/row; . $D/kit.sh
L=$D/logs; mkdir -p $L
STEPS="lock render put check push queue legs unpush putback release"
FROM=${1:-lock}; THEN=${2:-}
for x in $FROM $THEN; do case " $STEPS " in *" $x "*) ;; *) echo "no step '$x' (steps: $STEPS)"; exit 2;; esac; done
T(){ echo "$(date '+%T') $*" | tee -a $L/times.log; }
need(){ grep -qE "$2" $L/$1.txt 2>/dev/null || { T "STOP at $1: no '$2' (read row/logs/$1.txt)"; exit 3; }; }
PUTBACK(){ RENEW || { T "WARN: lock lost: stable NOT put back: ./run.sh putback"; return 1; }
  GATE lamp-skills lamp-tasks || { T "WARN: no start budget: stable NOT put back: wait 10 min, then ./run.sh putback"; return 1; }
  LAMP "" $D/putback.lamp.sh > $L/putback.txt 2>&1; cat $L/putback.txt; grep -q '^PUTBACK OK' $L/putback.txt; }
GUARD(){ trap - EXIT; trap 'T "guard cut by a signal: run ./run.sh putback"; exit 130' INT TERM
  grep -q '^PUT OK\|^PUT NOT OK' $L/put.txt 2>/dev/null || grep -q 'bind ' $L/put.txt 2>/dev/null || return 0
  grep -q '^PUTBACK OK' $L/putback.txt 2>/dev/null && return 0
  T "exit with #8237's bytes on: putting stable back now"; PUTBACK && T "stable put back (PUTBACK OK)" || T "WARN: PUTBACK NOT OK: ./run.sh putback"; }
trap GUARD EXIT; trap 'exit 130' INT TERM
go=0
for st in $STEPS; do
  [ $st = $FROM ] && go=1; [ -n "$THEN" ] && [ $st = $THEN ] && go=1; [ $go = 1 ] || continue
  [ -n "${UNTIL:-}" ] && [ $st = "$UNTIL" ] && { T "UNTIL $st: stop before it"; exit 0; }
  [ $st = lock ] || RENEW || exit 1
  T "step $st"
  case $st in
  lock)    if [ -z "$LL" ]; then echo "LOCK none (KIT_LL unset): no lock taken" | tee $L/lock.txt
           else $LL $LOCK_LAMP try $LOCK_WHO $LOCK_SESSION "#8237 ROW" > $L/lock.txt 2>&1; rc=$?
             [ $rc = 0 ] || { T "LOCK NOT TAKEN (try exit $rc): no ssh. $(tail -2 $L/lock.txt)"; exit 1; }
             rm -f $L/RELEASED
             ( while [ ! -f $L/RELEASED ]; do $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 && echo "$(date +%T) renewed" >> $L/renew.log || echo "$(date +%T) RENEW FAILED" >> $L/renew.log
                 sleep 60; done ) >/dev/null 2>&1 &
             echo $! > $L/renew.pid; echo "LOCK TAKEN $(date +%T); renew loop pid $(cat $L/renew.pid)" | tee -a $L/lock.txt; fi;;
  render)  bash $D/render.sh 2>&1 | tee $L/render.txt; need render '^RENDER (OK|nothing to do)';;
  put)     for v in PR_SKILLS_BIN PR_TASKS_BIN PR_SKILLS_SHA PR_TASKS_SHA; do eval "[ -n \"\${$v:-}\" ]" || { T "STOP at put: $v not set (env file)"; exit 2; }; done
           for b in "$PR_SKILLS_BIN:$PR_SKILLS_SHA" "$PR_TASKS_BIN:$PR_TASKS_SHA"; do f=${b%%:*}; w=${b##*:}
             [ "$(shasum -a 256 "$f" 2>/dev/null | cut -d' ' -f1)" = "$w" ] || { T "STOP at put: $f is not $w"; exit 2; }; done
           GATE lamp-skills lamp-tasks lamp-voice || { T "STOP at put: start budget"; exit 3; }
           SSHR -n $H "rm -rf /tmp/t8237; mkdir -p /tmp/t8237" && $SCP -q "$PR_SKILLS_BIN" $H:/tmp/t8237/lamp-skills && $SCP -q "$PR_TASKS_BIN" $H:/tmp/t8237/lamp-tasks || { T "STOP at put: copy failed"; exit 3; }
           LAMP "PR_SKILLS_SHA=$PR_SKILLS_SHA PR_TASKS_SHA=$PR_TASKS_SHA" $D/put.lamp.sh > $L/put.txt 2>&1; cat $L/put.txt; need put '^PUT OK';;
  check)   LAMP "" $D/check.lamp.sh > $L/check.txt 2>&1; cat $L/check.txt; need check '^READY$';;
  push)    ${PUSHCMD:-python3 $D/push.py} push $D/fixtures/$SKILL ${LAMP_PAIRED:-} 2>&1 | tail -1 | tee $L/push.txt; need push '^PUSHED ';;
  queue)   # A kept session lamp-tasks can't close queues every skill run: closed by Harness's own close --now (row/queue.lamp.sh).
           sleep ${QUEUE_WAIT:-20}; LAMP "" $D/queue.lamp.sh > $L/queue.txt 2>&1; cat $L/queue.txt; need queue '^QUEUE (CLEAR|CLOSED)';;
  legs)    VOLOK || exit 2; bash $D/row.sh ${LEGS:-P G A W}; T "legs: $(grep '^VERDICT' $L/row.txt | cut -c1-90)";;
  unpush)  if grep -q '^PUSHED ' $L/push.txt 2>/dev/null; then ${PUSHCMD:-python3 $D/push.py} remove $SKILL ${LAMP_PAIRED:-} 2>&1 | tail -1 | tee $L/unpush.txt; need unpush '^REMOVED '
           else echo "REMOVED nothing pushed" | tee $L/unpush.txt; fi;;
  putback) PUTBACK || { T "STOP at putback: PUTBACK NOT OK (row/logs/putback.txt)"; exit 3; };;
  release) if [ -z "$LL" ]; then echo "LOCK none to release"
           else grep -q '^PUTBACK OK' $L/putback.txt 2>/dev/null && grep -q '^REMOVED ' $L/unpush.txt 2>/dev/null || { T "no PUTBACK OK + REMOVED: keep the lock (FORCE=1 releases anyway)"; [ "${FORCE:-0}" = 1 ] || exit 1; }
             touch $L/RELEASED; [ -f $L/renew.pid ] && { kill $(cat $L/renew.pid) 2>/dev/null; rm -f $L/renew.pid; }
             $LL $LOCK_LAMP release $LOCK_WHO $LOCK_SESSION && echo "LOCK RELEASED $(date +%T)"; fi;;
  esac
  [ -n "$THEN" ] && [ $st = $FROM ] && [ $FROM != $THEN ] && { go=0; T "then: $THEN"; }
done
T "run.sh done: $(grep -h '^VERDICT' $L/row.txt 2>/dev/null | cut -c1-80)"
