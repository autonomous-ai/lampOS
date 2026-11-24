#!/bin/bash
# run-case.sh <case>: one mock case (mock/fx/<case>, from gen.py) in its own copy of the kit; prints one line: case,
# expected, got, OK|MISMATCH, the reason. No ssh, scp, sound or lock: bin/ stubs only.
#   KIND=after    gate/eval.py after round.txt facts.txt $ARGS        got = AFTER's verdict
#   KIND=peekcmp  gate/eval.py peekcmp before.txt after.txt           got = PEEK 8417's verdict
#   KIND=peek     gate/eval.py peek x idle.txt task.txt               got = the PEEK line
#   KIND=run      ../run.sh from lock to summary                      got = PASS|FAIL (the GATE line) or STOP:<step>
#                 MUST_OUT / MUST_CALL / MUST_NOCALL: a regex the output must hold / calls.log must / must not hold
set -u; M=$(cd "$(dirname "$0")" && pwd); KIT=$(cd $M/.. && pwd); c=$1; FXD=$M/fx/$c
W=$M/work/$c; rm -rf $W; mkdir -p $W/state
KIND=; NEXT_DEFAULT=; ARGS=; MUST_OUT=; MUST_CALL=; MUST_NOCALL=; NEXT_SEQ=; REWIND_YES=; TASKWAIT=; . $FXD/case.env
exp=$(cat $FXD/expect); E="python3 -I $KIT/gate/eval.py"
case $KIND in
after)   out=$($E after $FXD/round.txt $FXD/facts.txt $ARGS 96 2>&1); got=$(echo "$out" | sed -n 's/^AFTER \([A-Z]*\).*/\1/p')
         why=$(echo "$out" | grep -E '^CHECK [^ ]+ (FAIL|FINDING)' | head -2 | tr '\n' ' ');;
peekcmp) out=$($E peekcmp $FXD/before.txt $FXD/after.txt 2>&1); got=$(echo "$out" | sed -n 's/^PEEK 8417 \([A-Z]*\).*/\1/p'); why=$out;;
peek)    out=$($E peek x $FXD/idle.txt $FXD/task.txt 2>&1); got=$(echo "$out" | head -1); why=;;
run)
  cp -R $KIT/gate $W/gate; cp $KIT/run.sh $W/run.sh; rm -rf $W/gate/logs; mkdir -p $W/gate/logs
  while IFS=$'\t' read -r id text gain; do [ -n "$id" ] || continue; : > $W/gate/clips/$id-el.wav; printf '%s\tpad=0.4\n' "$text" > $W/gate/clips/$id-el.txt; done < $W/gate/clips/clips.txt
  printf 'LAMP=lamp-ee17\nLAMP_HOST=mock-host\nLAMP_USER=mockuser\nLOCK_WHO=mock-runner\nLAMP_SUDO_PASS=mock-sudo-value\n' > $W/kit.env
  export MOCK_FX=$FXD MOCK_ROW=$W/gate MOCK_LOG=$W/calls.log MOCK_STATE=$W/state MOCK_STARTS=${MOCK_STARTS:-0}
  export PATH=$M/bin:$PATH BASH_ENV=$M/shim.bash SLEEPDIV=${SLEEPDIV:-50}
  export KIT_ENV=$W/kit.env KIT_LL=$M/bin/lamplock SSH=$M/bin/ssh SCP=$M/bin/scp AFPLAY=$M/bin/afplay OSASCRIPT=$M/bin/osascript
  export MANIFEST_BASE=file://$FXD/rel/ REWIND_YES; [ -n "$NEXT_DEFAULT" ] && unset NEXT_SEQ || export NEXT_SEQ
  export POLL=0.1 QUIETS=0.1 WAITMAX=${WAITMAX:-4} TAIL=0 HEARWAIT=4 ANSWERWAIT=4 MAKEWAIT=6 ONWAIT=4 RBWAIT=2 YESGAP=0
  [ -n "$TASKWAIT" ] && export TASKWAIT
  TO=; command -v timeout >/dev/null && TO="timeout 300"; command -v gtimeout >/dev/null && TO="gtimeout 300"
  ( cd $W && $TO bash $W/run.sh lock ) > $W/out.txt 2>&1; rc=$?; echo "exit $rc" >> $W/out.txt
  touch $W/gate/logs/RELEASED; [ -f $W/gate/logs/renew.pid ] && kill $(cat $W/gate/logs/renew.pid) 2>/dev/null
  st=$(grep -oE 'STOP at [a-z0-9-]+' $W/out.txt | head -1 | sed 's/STOP at /STOP:/')
  if [ -n "$st" ]; then got=$st; else got=$(sed -n 's/^GATE [0-9]* on [^:]*: \([A-Z]*\).*/\1/p' $W/out.txt | head -1); [ $rc = 0 ] || got="${got:-BAD}(exit $rc)"; fi
  why=$(grep -E 'STOP at|^GATE |why FAIL' $W/out.txt | head -2 | cut -c1-150 | tr '\n' ' ')
  [ -n "$MUST_OUT" ] && ! grep -qE "$MUST_OUT" $W/out.txt && { got="$got,no-out"; why="no '$MUST_OUT' in out.txt; $why"; }
  [ -n "$MUST_CALL" ] && ! grep -qE "$MUST_CALL" $W/calls.log && { got="$got,no-call"; why="no '$MUST_CALL' in calls.log; $why"; }
  [ -n "$MUST_NOCALL" ] && grep -qE "$MUST_NOCALL" $W/calls.log 2>/dev/null && { got="$got,bad-call"; why="'$MUST_NOCALL' was called; $why"; }
  grep -q 'NO FIXTURE' $W/calls.log 2>/dev/null && why="$why [$(grep -o 'NO FIXTURE [^ ]*' $W/calls.log | head -2 | tr '\n' ' ')]";;
*) got="no KIND"; why=;;
esac
[ "$got" = "$exp" ] && ok=OK || ok=MISMATCH
printf '%-30s expect %-14s got %-14s %-8s %s\n' "$c" "$exp" "$got" "$ok" "$(echo "$why" | cut -c1-170)"
