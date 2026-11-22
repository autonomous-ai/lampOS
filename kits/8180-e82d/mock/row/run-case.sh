#!/bin/bash
# run-case.sh <case>: row.sh (and, for BOOT=1 cases, boot7170.sh) against mock/row/fx/<case> in its own copy of the
# kit; prints the case's verdict line. No ssh, scp, sound or lock: bin/ stubs only.
set -u; M=$(cd "$(dirname "$0")" && pwd); KIT=$(cd $M/../.. && pwd); c=$1; FXD=$M/fx/$c
W=$M/work/$c; rm -rf $W; mkdir -p $W/state $W/clips
cp -R $KIT/row $W/row; rm -rf $W/row/logs; mkdir -p $W/row/logs
printf 'J [lamp-tasks] each run gets its own cgroup under /sys/fs/cgroup/system.slice/lamp-tasks.service: memory.max 512M, cpu.max 100000 100000, pids.max 72\nCGROUP OK serve in its own leaf %s\nVERIFY PASS\n' "'serve'" > $W/row/logs/verify.txt
echo READY > $W/row/logs/ready.txt; echo "SAVED OK" > $W/row/logs/save.txt; echo "MUTE LIFTED OK after 4s" > $W/row/logs/micon.txt
for x in DAYS DAYSASK PRICE PRICEASK PROBE MAIL MAILASK LONGMAKE READY YES2 LOOK LOOKASK SLOW FORK NAMED; do : > $W/clips/$x-el.wav; done
: > $W/clips/clips.txt; printf 'LAMP=lamp-mock\nLAMP_HOST=mock-lamp\nLAMP_SUDO_PASS=not-a-password\n' > $W/kit.env
BOOT=0; MIC_YES=; TAILS2=; RUNSH=0; UNIT=; . $FXD/case.env
if [ -n "$UNIT" ]; then   # a kit helper alone (unit.sh), no row: PASS when it does what the test asks
  out=$(bash $M/unit.sh $UNIT $W $KIT); got=$(echo "$out" | grep -q '^UNIT PASS' && echo PASS || echo FAIL)
  exp=$(cat $FXD/expect); [ "$got" = "$exp" ] && ok=OK || ok=MISMATCH
  printf '%-24s expect %-4s got %-4s %-8s %s\n' "$c" "$exp" "$got" "$ok" "$(echo "$out" | tail -1 | cut -c1-170)"; exit 0; fi
export MOCK_FX=$FXD MOCK_ROW=$W/row MOCK_LOG=$W/calls.log MOCK_STATE=$W/state MIC_YES
[ -n "${MOCK_LAMPOS_TASKS:-}" ] && export MOCK_LAMPOS_TASKS
export PATH=$M/bin:$PATH BASH_ENV=$M/shim.bash SLEEPDIV=${SLEEPDIV:-25}
export KIT_ENV=$W/kit.env KIT_LL=$M/bin/lamplock SSH=$M/bin/ssh SCP=$M/bin/scp AFPLAY=$M/bin/afplay OSASCRIPT=$M/bin/osascript A=$W/clips
export POLL=0.1 QUIETS=0.1 WAITMAX=${WAITMAX:-4} MAKEWAIT=4 RUNWAIT=4 TASKWAIT=4 SAYWAIT=1 RBWAIT=2 BETWEEN=0 TAIL=0 NWAIT=4
TO=; command -v timeout >/dev/null && TO="timeout 600"
export BOOTWAIT=30 PORTWAIT=4 RUNWAIT7170=4 ENDWAIT=2
if [ "${RUNSH:-0}" = 1 ]; then
  # run.sh from lock to release; verdict: PASS = exit 0, both VERDICT PASS, LEG 7170 PASS, UNROW OK, MUTE BACK OK;
  # FAIL = stopped AND the trap put the mute back; VOID = stopped before any lift (no micon call made)
  cp $KIT/run.sh $W/run.sh; rm -f $W/row/logs/micon.txt
  LEGS="$LEGS" TAILS="$TAILS" $TO bash $W/run.sh > $W/out.txt 2>&1; rc=$?; echo "exit $rc" >> $W/out.txt; L=$W/row/logs
  touch $L/RELEASED; [ -f $L/renew.pid ] && kill $(cat $L/renew.pid) 2>/dev/null    # a stopped run keeps its lock loop
  if [ $rc = 0 ] && [ "$(cat $L/row-*.txt | grep -c '^VERDICT PASS')" = 2 ] && grep -q '^LEG 7170 PASS' $L/boot7170.txt && grep -q '^UNROW OK' $L/unrow.txt && grep -q '^MUTE BACK OK' $L/micoff.txt; then got=PASS
  elif [ $rc != 0 ] && grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null && grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null; then got=FAIL
  elif [ $rc != 0 ] && ! grep -q 'LAMP micon.lamp.sh' $W/calls.log 2>/dev/null; then got=VOID; else got=BAD; fi
  exp=$(cat $FXD/expect); [ "$got" = "$exp" ] && ok=OK || ok=MISMATCH
  printf '%-24s expect %-4s got %-4s %-8s %s\n' "$c" "$exp" "$got" "$ok" "$(grep -E 'STOP|run.sh done|exit with' $W/out.txt | tail -2 | cut -c1-170 | tr '\n' ' ')"
  exit 0; fi
LEGS="$LEGS" TAILS="$TAILS" $TO bash $W/row/row.sh > $W/out.txt 2>&1; echo "exit $?" >> $W/out.txt
last=$(echo $LEGS $TAILS | awk '{print $NF}')
# TAILS2: a second invocation, as run.sh's `tails` step (F, G read the first one's verdicts from logs/verdicts.env)
[ -n "${TAILS2:-}" ] && { $TO bash $W/row/row.sh $TAILS2 >> $W/out.txt 2>&1; echo "exit $?" >> $W/out.txt; last=$(echo $TAILS2 | awk '{print $NF}'); }
[ "$BOOT" = 1 ] && { $TO bash $W/row/boot7170.sh >> $W/out.txt 2>&1; echo "exit $?" >> $W/out.txt; last=7170; }
exp=$(cat $FXD/expect)
got=$(grep "^LEG $last " $W/out.txt | tail -1 | awk '{print $3}')
[ "$got" = "$exp" ] && ok=OK || ok=MISMATCH
printf '%-24s expect %-4s got %-4s %-8s %s\n' "$c" "$exp" "${got:-none}" "$ok" "$(grep "^LEG $last " $W/out.txt | tail -1 | cut -d' ' -f4- | cut -c1-170)"
