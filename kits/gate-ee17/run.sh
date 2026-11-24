#!/bin/bash
# Mac. The promote gate of build CAND_SEQ (15860) on lamp-ee17 (#7814, the first builder's 12:02 and 12:08 lists).
# Settings: gate/kit.sh (numbers with defaults; the runner's env file KIT_ENV). Read RUN-GATE.md first.
#   ./run.sh [from-step [then-step]]   steps: lock render restore round1 after1 round2 after2 core putback release summary
#   With then-step, the first step runs and the run goes on at then-step. UNTIL=<step> stops before that step.
# Each step stops the run on a FAIL (exit 3). On ANY exit the EXIT trap puts lamp-peek's monitor drop-in back if the
# gate laid it (PEEKGUARD); the lock is kept. The kit never sets a volume (it refuses above VOLCAP), never stops
# lamp-cloud, never resets or forgets; GATE keeps each unit at 10 or fewer starts in any 10 min.
set -u; K=$(cd "$(dirname "$0")" && pwd); D=$K/gate; . $D/kit.sh
L=$D/logs; mkdir -p $L
STEPS="lock render restore round1 after1 round2 after2 core putback release summary"
FROM=${1:-lock}; THEN=${2:-}
for x in $FROM $THEN; do case " $STEPS " in *" $x "*) ;; *) echo "no step '$x' (steps: $STEPS)"; exit 2;; esac; done
T(){ echo "$(date '+%T') $*" | tee -a $L/times.log; }
STOP(){ T "STOP at $1: $2"; exit 3; }
need(){ grep -qE "$2" $L/$1.txt 2>/dev/null || STOP $1 "no '$2' (read gate/logs/$1.txt)"; }
EV(){ python3 -I $D/eval.py "$@"; }
inst(){ sed -n 's/^STATE installed=\([^ ]*\).*/\1/p' $1 | tail -1; }
updof(){ sed -n 's/^UPD \([0-9a-f]*\).*/\1/p' $1 | tail -1; }
refof(){ sed -n 's/^STATE .*refused=\([^ ]*\).*/\1/p' $1 | tail -1; }
busyof(){ sed -n "s/^STATE .*busy='\([^']*\)'.*/\1/p" $1 | tail -1; }
t0of(){ sed -n 's/^T0 //p' $1 | head -1; }
manof(){ sed -n 's/^MANIFEST .*lamp-update=\([0-9a-f]*\).*/\1/p' $1 | tail -1; }
READ(){ LAMP "MODE=read" $D/restore.lamp.sh > $L/$1.txt 2>&1; cat $L/$1.txt; grep -q '^STATE installed=[0-9]' $L/$1.txt || STOP $1 "lamp-update's state unread (gate/logs/$1.txt)"; }
# PEEKGUARD: lamp-peek's monitor drop-in put back once, on any exit, when the gate laid it and did not put it back.
PEEKGUARD(){ [ -f $L/PEEK_LAID ] || return 0; T "exit with the gate's lamp-peek monitor drop-in laid: putting it back now"
  RENEW || { T "WARN: lock lost, drop-in NOT back: ./run.sh putback"; return 0; }
  GATE lamp-peek || { T "WARN: no start budget for lamp-peek: wait 10 min, then ./run.sh putback"; return 0; }
  LAMP "MODE=off" $D/peek.lamp.sh > $L/peek-guard.txt 2>&1; cat $L/peek-guard.txt
  grep -q '^PEEK BACK OK' $L/peek-guard.txt && { rm -f $L/PEEK_LAID; T "lamp-peek put back (PEEK BACK OK)"; } || T "WARN: lamp-peek NOT back: ./run.sh putback"; }
GUARDS(){ trap - EXIT; trap 'T "guard cut by a signal: check lamp-peek by hand"; exit 130' INT TERM; PEEKGUARD; }
trap GUARDS EXIT; trap 'exit 130' INT TERM
# MAN <name> <seq>: the release's signed manifest, read on the Mac: its sequence and lamp-update entry.
MAN(){ EV manifest "$MANIFEST_BASE" $2 > $L/manifest-$1.txt 2>&1; cat $L/manifest-$1.txt
  grep -q "^MANIFEST seq=$2 " $L/manifest-$1.txt || STOP $1 "release lampos-lamp-voice-$2's manifest is not build $2 (gate/logs/manifest-$1.txt)"
  [ -n "$(manof $L/manifest-$1.txt)" ] || STOP $1 "release $2's manifest names no lamp-update"; }
# ROUND <tag> <url>: one round by the installed updater; exit 4 (no quiet moment, or serve held the lock) once more.
ROUND(){ local t=$1 u=$2 t0
  LAMP "RTAG=$t URL=$u" $D/round.lamp.sh > $L/$t.txt 2>&1; cat $L/$t.txt
  if grep -q '^EXIT 4$' $L/$t.txt && [ "${ROUND_RETRY:-1}" = 1 ]; then t0=$(t0of $L/$t.txt); mv $L/$t.txt $L/$t.try1.txt
    T "$t: EXIT 4 (nothing switched: no quiet moment, or another lamp-update held the lock): once more in ${RETRYWAIT:-60} s"
    sleep ${RETRYWAIT:-60}; RENEW || exit 1; GATE $ROUND_UNITS || STOP $t "start budget before the second try"
    LAMP "RTAG=$t URL=$u T0=$t0" $D/round.lamp.sh > $L/$t.txt 2>&1; cat $L/$t.txt; fi
  grep -q '^EXIT ' $L/$t.txt || STOP $t "the round's output has no EXIT line (ssh cut?): read gate/logs/$t.txt"; }
# DOWN <step>: the lamp on stable STABLE_SEQ with stable's updater, by lamp-update's own path. Leftover row/gate /run
# drop-ins removed first (none added to any refused list). Installed below stable: a round to stable's release.
# Above it (lamp-update refuses an older build): the runner's PUTBACK_CMD, or with REWIND_YES=yes state.json's
# installed set to stable - 1 and then that round. Writes gate/logs/<step>-down.txt ending DOWN OK.
DOWN(){ local t=$1 f=$L/$1-read.txt i s us
  READ $1-read
  grep -q '^BIND ' $f && STOP $t "a mount over /opt/lampOS is on (an earlier kit's binary swap): run that kit's restore first; nothing changed"
  [ -z "$(busyof $f)" ] || STOP $t "a lamp-update round is under way ($(busyof $f)): wait, then ./run.sh $t"
  for s in $CAND_SEQ $NEXT_SEQ; do echo ",$(refof $f)," | grep -q ",$s," && STOP $t "build $s is in lamp-update's refused list, so a round to it is refused; the kit never edits that list: ask the owner"; done
  if grep -q '^DROPIN ' $f; then us=$(sed -n 's#^DROPIN /run/systemd/system/\(.*\)\.d/.*#\1#p' $f | grep -v '@' | sed 's/\.service$//' | sort -u)
    [ -z "$us" ] || GATE $us || STOP $t "start budget for $us"
    LAMP "MODE=clean" $D/restore.lamp.sh > $L/$t-clean.txt 2>&1; cat $L/$t-clean.txt; need $t-clean '^CLEAN OK'
    READ $1-read; fi
  i=$(inst $f)
  if [ "$i" = "$STABLE_SEQ" ]; then T "$t: installed is stable $STABLE_SEQ already: no round"
  elif [ "$i" -lt "$STABLE_SEQ" ] 2>/dev/null; then GATE $ROUND_UNITS || STOP $t "start budget"; ROUND $t-round "$(curl_of $STABLE_SEQ)"
  elif [ -n "${PUTBACK_CMD:-}" ]; then
    PUTBACK_SEQ=$STABLE_SEQ LAMP_HOST=$LAMP_HOST bash -c "$PUTBACK_CMD" > $L/$t-cmd.txt 2>&1 || { cat $L/$t-cmd.txt; STOP $t "PUTBACK_CMD failed (gate/logs/$t-cmd.txt)"; }
    s0=$(date +%s); while :; do RENEW || exit 1; READ $1-read > /dev/null; [ "$(inst $f)" = "$STABLE_SEQ" ] && [ -z "$(busyof $f)" ] && break
      [ $(( $(date +%s) - s0 )) -lt ${PUTBACK_WAIT:-1200} ] || STOP $t "stable $STABLE_SEQ not installed in ${PUTBACK_WAIT:-1200} s after PUTBACK_CMD"; sleep ${PUTBACK_POLL:-30}; done
  elif [ "${REWIND_YES:-}" = yes ]; then
    GATE lamp-update || STOP $t "start budget for lamp-update"
    LAMP "MODE=rewind TO=$(( STABLE_SEQ - 1 ))" $D/restore.lamp.sh > $L/$t-rewind.txt 2>&1; cat $L/$t-rewind.txt; need $t-rewind '^REWOUND OK'
    GATE $ROUND_UNITS || STOP $t "start budget"; ROUND $t-round "$(curl_of $STABLE_SEQ)"
  else STOP $t "installed $i is above stable $STABLE_SEQ and lamp-update refuses an older build: set REWIND_YES=yes (the owner's yes: state.json's installed set to $(( STABLE_SEQ - 1 )), then a round to stable's release) or PUTBACK_CMD (your own put-back), then ./run.sh $t"; fi
  READ $t-check
  [ "$(inst $L/$t-check.txt)" = "$STABLE_SEQ" ] || STOP $t "installed is $(inst $L/$t-check.txt), not stable $STABLE_SEQ"
  case "$(updof $L/$t-check.txt)" in $STABLE_UPD*) ;; *) STOP $t "lamp-update is $(updof $L/$t-check.txt | cut -c1-12), not stable's $STABLE_UPD";; esac
  grep -q '^DROPIN ' $L/$t-check.txt && STOP $t "a row/gate drop-in is still there"
  echo "DOWN OK installed $STABLE_SEQ, lamp-update $(updof $L/$t-check.txt | cut -c1-12), no row/gate drop-in" | tee $L/$t-down.txt; }
# PEEKRUN <before|after>: #8417's read: the monitor drop-in laid, one idle minute, one minute with the TASK clip's
# OpenClaw task, the drop-in put back exactly. Writes gate/logs/peek-<label>.txt: PEEK <label> idle bytes= cpu= task ...
PEEKRUN(){ local p=peek-$1 mp
  MUTEOK=0 VOLCHK $p > $L/$p-vol.txt 2>&1 || { cat $L/$p-vol.txt; STOP $p "volume or mute (gate/logs/$p-vol.txt)"; }; cat $L/$p-vol.txt
  VOLOK || STOP $p "Mac volume"; [ -f $A/TASK-el.wav ] || STOP $p "no TASK clip: ./run.sh render"
  GATE lamp-peek || STOP $p "start budget for lamp-peek"
  : > $L/PEEK_LAID; LAMP "MODE=on" $D/peek.lamp.sh > $L/$p-on.txt 2>&1; cat $L/$p-on.txt; need $p-on '^PEEK ON OK'
  T "$p: one idle minute"; LAMP "MODE=measure LABEL=idle SECS=$PEEKSECS" $D/peek.lamp.sh > $L/$p-idle.txt 2>&1; cat $L/$p-idle.txt
  T "$p: one minute with the TASK clip's OpenClaw task"
  LAMP "MODE=measure LABEL=task SECS=$PEEKSECS" $D/peek.lamp.sh > $L/$p-task.txt 2>&1 & mp=$!
  sleep ${TASKLEAD:-3}; LEG=$p PLAY TASK; wait $mp; cat $L/$p-task.txt
  LAMP "MODE=off" $D/peek.lamp.sh > $L/$p-off.txt 2>&1; cat $L/$p-off.txt; grep -q '^PEEK BACK OK' $L/$p-off.txt && rm -f $L/PEEK_LAID
  need $p-off '^PEEK BACK OK'
  EV peek $1 $L/$p-idle.txt $L/$p-task.txt > $L/$p.txt 2>&1; cat $L/$p.txt; need $p "^PEEK $1 idle bytes="; }
# AFTER <n> <round tag> <seq> <prev> <manifest file> <prefix>: the after-round checks, straight after the round.
AFTER(){ LAMP "RTAG=$2 T0=$(t0of $L/$2.txt)" $D/after.lamp.sh > $L/after$1-facts.txt 2>&1
  EV after $L/$2.txt $L/after$1-facts.txt $3 $4 "$(manof $5)" "${6:--}" $GWWIN > $L/after$1.txt 2>&1; cat $L/after$1.txt
  need after$1 '^AFTER (PASS|FINDING)'; }
go=0
for st in $STEPS; do
  [ $st = $FROM ] && go=1; [ -n "$THEN" ] && [ $st = $THEN ] && go=1; [ $go = 1 ] || continue
  [ -n "${UNTIL:-}" ] && [ $st = "$UNTIL" ] && { T "UNTIL $st: stop before it"; exit 0; }
  case $st in lock|summary) ;; *) RENEW || exit 1;; esac
  T "step $st"
  case $st in
  lock)    if [ -z "$LL" ]; then echo "LOCK none (KIT_LL unset): no lock taken" | tee $L/lock.txt
           else $LL $LOCK_LAMP try $LOCK_WHO $LOCK_SESSION "gate $CAND_SEQ" > $L/lock.txt 2>&1; rc=$?
             [ $rc = 0 ] || { T "LOCK NOT TAKEN (try exit $rc): no ssh. $(tail -2 $L/lock.txt)"; exit 1; }
             rm -f $L/RELEASED
             ( while [ ! -f $L/RELEASED ]; do $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 && echo "$(date +%T) renewed" >> $L/renew.log || echo "$(date +%T) RENEW FAILED" >> $L/renew.log
                 sleep 60; done ) >/dev/null 2>&1 &
             echo $! > $L/renew.pid; echo "LOCK TAKEN $(date +%T); renew loop pid $(cat $L/renew.pid)" | tee -a $L/lock.txt; fi;;
  render)  bash $D/render.sh 2>&1 | tee $L/render.txt; need render '^RENDER (OK|nothing to do)';;
  restore) VOLCHK restore > $L/restore-vol.txt 2>&1 || { cat $L/restore-vol.txt; STOP restore "speaker volume (gate/logs/restore-vol.txt)"; }; cat $L/restore-vol.txt
           DOWN restore; echo "RESTORE OK $(sed 's/^DOWN OK //' $L/restore-down.txt)" | tee $L/restore.txt;;
  round1)  need restore '^RESTORE OK'
           MAN cand $CAND_SEQ; case "$(manof $L/manifest-cand.txt)" in $CAND_UPD*) ;; *) STOP round1 "build $CAND_SEQ's manifest names lamp-update $(manof $L/manifest-cand.txt | cut -c1-12), not $CAND_UPD";; esac
           READ round1-pre; [ "$(inst $L/round1-pre.txt)" = "$STABLE_SEQ" ] || STOP round1 "installed is $(inst $L/round1-pre.txt), not stable $STABLE_SEQ: ./run.sh restore"
           case "$(updof $L/round1-pre.txt)" in $STABLE_UPD*) ;; *) STOP round1 "the installed updater is $(updof $L/round1-pre.txt | cut -c1-12), not stable's $STABLE_UPD";; esac
           GATE $ROUND_UNITS || STOP round1 "start budget"
           ROUND round1 "$(curl_of $CAND_SEQ)";;
  after1)  [ -f $L/round1.txt ] || STOP after1 "no round 1 (./run.sh round1)"
           AFTER 1 round1 $CAND_SEQ $STABLE_SEQ $L/manifest-cand.txt $CAND_UPD
           PEEKRUN before;;
  round2)  [ -n "$NEXT_SEQ" ] || STOP round2 "NEXT_SEQ is empty: round 2's build (default 15866, train 1202's)"
           need after1 '^AFTER (PASS|FINDING)'; need peek-before '^PEEK before idle bytes='
           MAN next $NEXT_SEQ; [ -z "$NEXT_UPD" ] || case "$(manof $L/manifest-next.txt)" in $NEXT_UPD*) ;; *) STOP round2 "build $NEXT_SEQ's manifest names lamp-update $(manof $L/manifest-next.txt | cut -c1-12), not $NEXT_UPD";; esac
           READ round2-pre; [ "$(inst $L/round2-pre.txt)" = "$CAND_SEQ" ] || STOP round2 "installed is $(inst $L/round2-pre.txt), not $CAND_SEQ"
           case "$(updof $L/round2-pre.txt)" in $CAND_UPD*) ;; *) STOP round2 "the installed updater is $(updof $L/round2-pre.txt | cut -c1-12), not $CAND_SEQ's $CAND_UPD";; esac
           GATE $ROUND_UNITS || STOP round2 "start budget"
           ROUND round2 "$(curl_of $NEXT_SEQ)";;
  after2)  [ -f $L/round2.txt ] || STOP after2 "no round 2 (./run.sh round2)"
           AFTER 2 round2 $NEXT_SEQ $CAND_SEQ $L/manifest-next.txt "$NEXT_UPD"
           PEEKRUN after
           EV peekcmp $L/peek-before.txt $L/peek-after.txt > $L/peek-8417.txt 2>&1; cat $L/peek-8417.txt
           grep -q '^PEEK 8417 PASS' $L/peek-8417.txt || { [ "${PEEK_FAIL_STOPS:-0}" = 1 ] && STOP after2 "#8417's read (gate/logs/peek-8417.txt)"; T "NOTE #8417's read is not PASS (its own verdict, not 15860's): going on"; };;
  core)    need after2 '^AFTER (PASS|FINDING)'
           bash $D/core.sh 2>&1 | tee $L/core.txt; need core '^CORE PASS';;
  putback) DOWN putback
           LAMP "MODE=check" $D/peek.lamp.sh > $L/putback-peek.txt 2>&1; cat $L/putback-peek.txt
           grep -q '^PEEK BACK OK' $L/putback-peek.txt || { GATE lamp-peek || STOP putback "start budget for lamp-peek"
             LAMP "MODE=off" $D/peek.lamp.sh > $L/putback-peek.txt 2>&1; cat $L/putback-peek.txt; need putback-peek '^PEEK BACK OK'; rm -f $L/PEEK_LAID; }
           echo "PUTBACK OK $(sed 's/^DOWN OK //' $L/putback-down.txt); lamp-peek's monitor drop-in as before" | tee $L/putback.txt;;
  release) if [ -z "$LL" ]; then echo "LOCK none to release"
           else grep -q '^PUTBACK OK' $L/putback.txt 2>/dev/null || { T "no PUTBACK OK: keep the lock (FORCE=1 releases anyway)"; [ "${FORCE:-0}" = 1 ] || exit 1; }
             touch $L/RELEASED; [ -f $L/renew.pid ] && { kill $(cat $L/renew.pid) 2>/dev/null; rm -f $L/renew.pid; }
             $LL $LOCK_LAMP release $LOCK_WHO $LOCK_SESSION && echo "LOCK RELEASED $(date +%T)"; fi;;
  summary) bash $D/summary.sh | tee $L/summary.txt;;
  esac
  [ -n "$THEN" ] && [ $st = $FROM ] && [ $FROM != $THEN ] && { go=0; T "then: $THEN"; }
done
T "run.sh done: $(grep -h '^GATE ' $L/summary.txt 2>/dev/null | head -1)"
