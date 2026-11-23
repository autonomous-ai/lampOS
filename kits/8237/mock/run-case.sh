#!/bin/bash
# run-case.sh <case>: ../run.sh from lock to release against mock/fx/<case> in its own copy of the kit; stub ssh, scp,
# afplay, osascript, lock and push only (bin/). Prints: case, expected, got, OK|MISMATCH, the run's last lines.
#   PASS/FAIL/VOID  exit 0, VERDICT that, PUTBACK OK, LOCK released
#   STOPBACK        stopped (exit != 0) after put, and the EXIT trap put stable back (PUTBACK OK), lock kept
#   KEPT            exit != 0, PUTBACK NOT OK, lock kept (no release)
#   NOSSH           exit != 0 and no lamp body was ever run
set -u; M=$(cd "$(dirname "$0")" && pwd); KIT=$(cd $M/.. && pwd); c=$1; FXD=$M/fx/$c
W=$M/work/$c; rm -rf $W; mkdir -p $W/state $W/bins
cp -R $KIT/row $W/row; cp $KIT/run.sh $W/run.sh; rm -rf $W/row/logs; mkdir -p $W/row/logs
while IFS=$'\t' read -r id text gain; do [ -n "$id" ] || continue; : > $W/row/clips/$id-el.wav; printf '%s\tpad=0.4\n' "$text" > $W/row/clips/$id-el.txt; done < $W/row/clips/clips.txt
echo skills-bin > $W/bins/lamp-skills; echo tasks-bin > $W/bins/lamp-tasks
s1=$(shasum -a 256 $W/bins/lamp-skills | cut -d' ' -f1); s2=$(shasum -a 256 $W/bins/lamp-tasks | cut -d' ' -f1)
MOCK_STARTS=0; MOCK_VOL=50; MOCK_LOCKFAIL=; BADSHA=; MOCK_PUSHCMD=; . $FXD/case.env; [ -n "$MOCK_PUSHCMD" ] && MOCK_PUSHCMD="bash $W/row/stage.sh"; [ -n "$BADSHA" ] && s1=0000
printf 'LAMP=lamp-mock\nLAMP_HOST=mock-host\nLAMP_USER=mockuser\nLOCK_WHO=mock-runner\nLAMP_SUDO_PASS=mock\nPR_SKILLS_BIN=%s\nPR_TASKS_BIN=%s\nPR_SKILLS_SHA=%s\nPR_TASKS_SHA=%s\n' $W/bins/lamp-skills $W/bins/lamp-tasks $s1 $s2 > $W/kit.env
export MOCK_FX=$FXD MOCK_LOG=$W/calls.log MOCK_STATE=$W/state MOCK_STARTS MOCK_VOL MOCK_LOCKFAIL
export PATH=$M/bin:$PATH BASH_ENV=$M/shim.bash SLEEPDIV=${SLEEPDIV:-25}
export STAGE_SDK=$(cd $KIT/../.. && pwd)/lampOS/sdk/python/src
export PUSHCMD="${MOCK_PUSHCMD:-$M/bin/push}" KIT_ENV=$W/kit.env KIT_LL=$M/bin/lamplock SSH=$M/bin/ssh SCP=$M/bin/scp AFPLAY=$M/bin/afplay OSASCRIPT=$M/bin/osascript
export POLL=1 GWAIT=8 WWAIT=6 YESWAIT=6 YESAFTER=1 SETTLE=1 ASKAFTER=1
bash $W/run.sh > $W/out.txt 2>&1; rc=$?; echo "exit $rc" >> $W/out.txt; L=$W/row/logs
touch $L/RELEASED; [ -f $L/renew.pid ] && kill $(cat $L/renew.pid) 2>/dev/null
v=$(grep -h '^VERDICT' $L/row.txt 2>/dev/null | awk '{print $2}'); pb=$(grep -c '^PUTBACK OK' $L/putback.txt 2>/dev/null); rel=$(grep -c 'release' $W/calls.log)
lamp=$(grep -c '^LAMP ' $W/calls.log 2>/dev/null)
if [ $rc = 0 ] && [ -n "$v" ] && [ "$pb" = 1 ] && [ "$rel" = 1 ]; then got=$v
elif [ $rc != 0 ] && [ "${lamp:-0}" = 0 ]; then got=NOSSH
elif [ $rc != 0 ] && [ "$pb" = 1 ] && [ "$rel" = 0 ]; then got=STOPBACK
elif [ $rc != 0 ] && [ "${pb:-0}" = 0 ] && [ "$rel" = 0 ]; then got=KEPT
else got="BAD(rc=$rc,v=$v,pb=$pb,rel=$rel)"; fi
exp=$(cat $FXD/expect); [ "$got" = "$exp" ] && ok=OK || ok=MISMATCH
printf '%-14s expect %-8s got %-8s %-8s %s\n' "$c" "$exp" "$got" "$ok" "$(grep -E '^LEG|STOP|WARN|VERDICT' $W/out.txt | grep -vE 'LEG . PASS$' | tail -3 | sed "s|$W/||g" | cut -c1-110 | tr '\n' ' ')"
