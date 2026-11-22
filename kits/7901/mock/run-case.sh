#!/bin/bash
# run-case.sh <case>: row.sh (or, for RUNSH=1 cases, ../../run.sh from lock to release) against mock/fx/<case>, in its
# own copy of the kit; prints the case's verdict line. No ssh, scp, sound or lock: bin/ stubs only.
set -u; M=$(cd "$(dirname "$0")" && pwd); KIT=$(cd $M/.. && pwd); c=$1; FXD=$M/fx/$c
W=$M/work/$c; rm -rf $W; mkdir -p $W/state
cp -R $KIT/row $W/row; rm -rf $W/row/logs $W/row/clips/*.wav $W/row/clips/*-el.txt; mkdir -p $W/row/logs
while IFS=$'\t' read -r id text gain; do [ -n "$id" ] || continue; : > $W/row/clips/$id-el.wav; printf '%s\tpad=0.4\n' "$text" > $W/row/clips/$id-el.txt; done < $W/row/clips/clips.txt
printf 'LAMP=lamp-mock\nLAMP_HOST=mock-host\nLAMP_USER=mockuser\nLOCK_WHO=mock-runner\nLAMP_SUDO_PASS=mock-sudo-value\n' > $W/kit.env
MIC_YES=; RUNSH=0; MOCK_STARTS=0; UNIT=; LEGS="A B C D"; . $FXD/case.env
if [ -n "$UNIT" ]; then   # a kit helper alone (unit.sh), no row: PASS when it does what the test asks
  out=$(bash $M/unit.sh $UNIT $W $KIT); got=$(echo "$out" | grep -q '^UNIT PASS' && echo PASS || echo FAIL)
  exp=$(cat $FXD/expect); [ "$got" = "$exp" ] && ok=OK || ok=MISMATCH
  printf '%-26s expect %-4s got %-4s %-8s %s\n' "$c" "$exp" "$got" "$ok" "$(echo "$out" | tail -1 | cut -c1-150)"; exit 0; fi
export MOCK_FX=$FXD MOCK_ROW=$W/row MOCK_LOG=$W/calls.log MOCK_STATE=$W/state MIC_YES MOCK_STARTS
export PATH=$M/bin:$PATH BASH_ENV=$M/shim.bash SLEEPDIV=${SLEEPDIV:-25}
export KIT_ENV=$W/kit.env KIT_LL=$M/bin/lamplock SSH=$M/bin/ssh SCP=$M/bin/scp AFPLAY=$M/bin/afplay OSASCRIPT=$M/bin/osascript
export POLL=0.1 QUIETS=0.1 WAITMAX=${WAITMAX:-4} SAYWAIT=1 RBWAIT=2 BETWEEN=0 TAIL=0 HEARWAIT=4
TO=; command -v timeout >/dev/null && TO="timeout 600"; command -v gtimeout >/dev/null && TO="gtimeout 600"
if [ "$RUNSH" = 1 ]; then
  # PASS = exit 0, VERDICT PASS, UNROW OK, MUTE BACK OK; FAIL = stopped or a leg failed, AND the mute was lifted and put
  # back; VOID = stopped before any lift (no micon call made); anything else BAD.
  cp $KIT/run.sh $W/run.sh
  LEGS="$LEGS" $TO bash $W/run.sh > $W/out.txt 2>&1; rc=$?; echo "exit $rc" >> $W/out.txt; L=$W/row/logs
  touch $L/RELEASED; [ -f $L/renew.pid ] && kill $(cat $L/renew.pid) 2>/dev/null
  vp=$(cat $L/row-*.txt 2>/dev/null | grep -c '^VERDICT PASS')
  if [ $rc = 0 ] && [ "$vp" = 1 ] && grep -q '^UNROW OK' $L/unrow.txt && grep -q '^MUTE BACK OK' $L/micoff.txt; then got=PASS
  elif { [ $rc != 0 ] || [ "$vp" = 0 ]; } && grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null && grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null; then got=FAIL
  elif [ $rc != 0 ] && ! grep -q 'LAMP micon.lamp.sh' $W/calls.log 2>/dev/null; then got=VOID; else got=BAD; fi
  exp=$(cat $FXD/expect); [ "$got" = "$exp" ] && ok=OK || ok=MISMATCH
  printf '%-26s expect %-4s got %-4s %-8s %s\n' "$c" "$exp" "$got" "$ok" "$(grep -E 'STOP|run.sh done|mute put back|legs: ' $W/out.txt | tail -2 | cut -c10-150 | tr '\n' ' ')"
  exit 0; fi
printf 'VERIFY PASS (mock)\nREADY\n' > $W/row/logs/check.txt; echo "SAVED OK" > $W/row/logs/save.txt
LEGS="$LEGS" $TO bash $W/row/row.sh > $W/out.txt 2>&1; echo "exit $?" >> $W/out.txt
last=$(echo $LEGS | awk '{print $NF}'); exp=$(cat $FXD/expect)
got=$(grep "^LEG $last " $W/out.txt | tail -1 | awk '{print $3}')
[ "$got" = "$exp" ] && ok=OK || ok=MISMATCH
printf '%-26s expect %-4s got %-4s %-8s %s\n' "$c" "$exp" "${got:-none}" "$ok" "$(grep "^LEG $last " $W/out.txt | tail -1 | cut -d' ' -f4- | cut -c1-150)"
