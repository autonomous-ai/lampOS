#!/bin/bash
# run-case.sh <case>: row/legs.sh (a leg case) or run.sh (a RUN case) against mock/row/fx/<case> in its own copy of the
# kit; prints one line: case, expected, got, OK|MISMATCH, why. No ssh, scp, sound or lock: bin/ stubs only.
set -u; M=$(cd "$(dirname "$0")" && pwd); KIT=$(cd $M/../.. && pwd); c=$1; FXD=$M/fx/$c
W=$M/work/$c; rm -rf $W; mkdir -p $W/state $W/h
cp -R $KIT/row $W/row; cp $KIT/run.sh $W/run.sh; rm -rf $W/row/logs $W/row/__pycache__ $W/row/harness/0.6.75; mkdir -p $W/row/logs
[ -n "${MUT_FROM:-}" ] && python3 -I - "$W/row/verdict.py" <<'P'
import os, sys
p = sys.argv[1]; s = open(p).read(); a = os.environ["MUT_FROM"]
if a not in s: print("MUTATION NOT FOUND"); sys.exit(1)
open(p, "w").write(s.replace(a, os.environ["MUT_TO"], 1))
P
printf 'LAMP_HOST=mock-lamp\nLAMP_SUDO_PASS=not-a-password\n' > $W/kit.env
RUNSH=; MIC_YES=; SKIP=; UNTIL=; FROM=; THEN=; WANT=; SPLIT=; CHANNEL=; HOLD_CMD=; UNHOLD_CMD=; APP_L6=; LEGS=
ROUND1_SEQ=; ROUND2_SEQ=; ROUND1_CMD=; ROUND2_CMD=; . $FXD/case.env
# Harness's stable pin, small stand-in files and their own pin file (hfetch checks sha256 and size)
echo "mock cli.js" > $W/h/cli.js; echo "mock notify.mjs" > $W/h/notify.mjs
python3 -I - $W/h <<'P'
import hashlib, json, os, sys
d = sys.argv[1]; e = lambda n: {"url": "https://mock.invalid/harness/cli/0.6.75/" + n, "size": os.path.getsize(os.path.join(d, n)), "sha256": hashlib.sha256(open(os.path.join(d, n), "rb").read()).hexdigest()}
json.dump({"cli": {"version": "0.6.75", "cli": e("cli.js"), "notify": e("notify.mjs")}}, open(os.path.join(d, "meta.json"), "w"))
P
if [ -n "$CHANNEL" ]; then mkdir -p $W/ch1 $W/ch2; echo '{}' > $W/ch1/lampos-manifest.json; echo '{}' > $W/ch2/lampos-manifest.json; echo pub > $W/test.pub
  export CH1=$W/ch1 CH2=$W/ch2 TEST_PUB=$W/test.pub; fi
set -a; . $FXD/case.env; set +a
export MOCK_FX=$FXD MOCK_ROW=$W/row MOCK_LOG=$W/calls.log MOCK_STATE=$W/state MOCK_HFILES=$W/h HMETA=$W/h/meta.json
export PATH=$M/bin:$PATH BASH_ENV=$M/shim.bash SLEEPDIV=${SLEEPDIV:-50}
export KIT_ENV=$W/kit.env KIT_LL=$M/bin/lamplock SSH=$M/bin/ssh SCP=$M/bin/scp AFPLAY=$M/bin/afplay OSASCRIPT=$M/bin/osascript
export POLL=0.1 WAITMAX=${WAITMAX:-4} TAIL=0 FSETTLE=0.5 ROUNDPOLL=1 VERIFYPOLL=1 RENEWEVERY=6000 LEGS
L=$W/row/logs; TO=; command -v timeout >/dev/null && TO="timeout 300"
fin(){ touch $W/state/stop $L/RELEASED; [ -f $L/renew.pid ] && kill $(cat $L/renew.pid) 2>/dev/null; }
say(){ exp=$(cat $FXD/expect); [ "$got" = "$exp" ] && ok=OK || ok=MISMATCH
  printf '%-30s expect %-11s got %-11s %-8s %s\n' "$c" "$exp" "$got" "$ok" "$(echo "$1" | tr '\n' ' ' | cut -c1-150)"; exit 0; }
if [ -z "$RUNSH" ]; then
  echo "VERIFY PASS" > $L/verify.txt
  $TO bash $W/row/legs.sh $LEGS > $W/out.txt 2>&1; echo "exit $?" >> $W/out.txt; fin
  v=$(grep "^LEG $LEGS " $W/out.txt | tail -1); got=$(echo "$v" | awk '{print $3}' | tr -d :); got=${got:-none}
  say "$(echo "$v" | cut -d' ' -f4-)"; fi
# run.sh in its own process group (a Ctrl-C case signals that group only)
R(){ ( cd $W && $TO python3 -I -c 'import os,sys; os.setpgid(0,0); os.execvp(sys.argv[1], sys.argv[1:])' bash $W/run.sh "$@" ) >> $W/out.txt 2>&1; }
if [ -n "$SPLIT" ]; then rc=0
  for p in "verify|||" "micon||verify|" "round2|L0 L1|micon|" "round2|L2 L3|micon|1" "round2|L4 L5 L6|micon|1" "round2|L7|micon|1" "round2|L8 L9|micon|1" "hdown||round2|" "||hdown|"; do
    IFS='|' read u lg fr kv <<< "$p"; echo "=== piece UNTIL=$u LEGS=$lg from=$fr KEEP=$kv" >> $W/out.txt
    UNTIL=$u LEGS=${lg:-$LEGS} KEEP_VERDICTS=$kv R $fr; rc=$?; [ $rc = 0 ] || break; done
else R $FROM $THEN; rc=$?; fi
echo "exit $rc" >> $W/out.txt; fin
steps=$(sed -n 's/^[0-9:]* step //p' $L/times.log 2>/dev/null | tr '\n' ' ' | sed 's/ $//')
lifted(){ grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null; }
back(){ grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null; }
left(){ ls $L/stopped.on $L/wrap.on $L/hold.on 2>/dev/null; }
grep -q 'set volume' $W/calls.log 2>/dev/null && { got=BAD; say "a volume was SET"; }
why=$(grep -E 'STOP|LOCK|Mac volume|run.sh done|guard' $W/out.txt | tail -2)
case $(cat $FXD/expect) in
PASS|EFAIL|EVOID)
  w=$(cat $FXD/expect | sed 's/^E//'); e=$(grep '^LEG E ' $L/engines.txt 2>/dev/null | awk '{print $3}')
  if [ $rc = 0 ] && [ "$e" = "$w" ] && grep -q '^LOCK RELEASED' $W/out.txt && grep -q '^READ TWICE same' $L/read.txt && grep -q '^PUTBACK OK' $L/putback.txt \
     && grep -q 'LAMP round.lamp.sh leg=run mode=read n=2' $W/calls.log; then
    if [ "$steps" != "lock base engines0 round1 verify engines round2 putback read release" ]; then got=BAD; why="step order: $steps"
    else got=$(cat $FXD/expect); why="$(grep '^LEG E ' $L/engines.txt)"; fi
  else got=BAD; why="rc=$rc E='$e' $why"; fi;;
STOPPED) if [ $rc = 0 ] && [ "$steps" = "$WANT" ] ; then got=STOPPED; why="steps: $steps"; else got=BAD; why="rc=$rc steps: $steps"; fi;;
TRAPPED) # the run stopped; the trap SIGCONTed the stopped pid (when one was stopped) and put the mute back, last
  ns=$(grep -n 'LAMP jam.lamp.sh leg=L4 mode=stop' $W/calls.log | head -1 | cut -d: -f1)
  nn=$(grep -n 'LAMP nostop.lamp.sh' $W/calls.log | tail -1 | cut -d: -f1); nm=$(grep -n 'LAMP micoff.lamp.sh' $W/calls.log | tail -1 | cut -d: -f1)
  if [ $rc != 0 ] && lifted && back && [ -z "$(left)" ] && [ -n "$nm" ] && { [ -z "$ns" ] || { [ -n "$nn" ] && [ "$nn" -gt "$ns" ]; }; }; then got=TRAPPED; why="rc=$rc nostop line ${nn:-none} micoff line $nm; $why"
  else got=BAD; why="rc=$rc lifted/back=$(lifted && echo y)/$(back && echo y) left='$(left)' ns=$ns nn=$nn nm=$nm"; fi;;
LOCKSTOP) nr=$(grep -n 'RENEW REFUSED' $W/calls.log | head -1 | cut -d: -f1)
  after=0; [ -n "$nr" ] && after=$(tail -n +$nr $W/calls.log | grep -c 'ssh LAMP')
  if [ "${MOCK_LOCK_RC:-0}" != 0 ]; then n=$(grep -c '^[0-9.]* ssh ' $W/calls.log 2>/dev/null); [ $rc != 0 ] && [ "$n" = 0 ] && got=LOCKSTOP || got=BAD; why="rc=$rc ssh calls $n"
  else [ $rc = 1 ] && [ -n "$nr" ] && [ "$after" = 0 ] && got=LOCKSTOP || got=BAD; why="rc=$rc ssh after the refused renew: $after; $why"; fi;;
VOLSTOP) n=$(grep -c '^[0-9.]* ssh ' $W/calls.log 2>/dev/null); [ $rc = 2 ] && [ "$n" = 0 ] && got=VOLSTOP || got=BAD; why="rc=$rc ssh calls $n";;
STOP-*) s=${FXD##*/}; want=$(cat $FXD/expect | sed 's/^STOP-//')
  if [ $rc = 3 ] && grep -q "STOP at $want:" $L/times.log && { ! lifted || back; } && [ -z "$(left)" ] && ! grep -q '^LOCK RELEASED' $W/out.txt; then got=STOP-$want; else got=BAD; fi
  why="rc=$rc $(grep 'STOP at' $L/times.log | tail -1 | cut -d' ' -f2-)";;
esac
say "$why"
