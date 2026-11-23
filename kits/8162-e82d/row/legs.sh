#!/bin/bash
# Mac. #8162 ROW legs on $LAMP, on the PR build round 1 installed (run.sh's `legs` step, after verify and micon), or by
# hand: row/legs.sh L0 L1 ... (default: L0 .. L9). Each leg: followers (the lamp's journal, short-unix; tasks/agent's
# kept value + {"op":"agent"}), the break, a pre-rendered clip played at the Mac (afplay -v gain only; the Mac's
# volume is read, <= 70, and never set; the lamp's speaker is never touched), the waits, then verdict.py.
#   L0 a healthy ask (spoken, then by the sockets)          L1 the gateway restarted under an idle session
#   L2 the gateway restarted mid-task (a long socket task)  L3 the kept pane's openclaw pid killed (by number)
#   L4 that pid stopped (SIGSTOP; SIGCONT after, and on ANY exit)
#   L5 Harness's daemon restarted                           L6 the app's engines.install (read only; APP_L6= from the app)
#   L7 OpenClaw's install (first-install config, wrapper line): its order and lamp-tasks' merge live
#   L8 that install cut off between lamp-tasks' stop and start   L9 a lasting hold (only with the runner's HOLD_CMD)
# A leg that could not run (the start budget, a muted mic, the agent not ready at its start, no pane pid, no hold) is
# VOID, not FAIL. A fault leg whose agent never comes back to ready stops the legs that follow (the lamp needs a look).
# Writes logs/<L>.* (journal, aw, marks, ...), logs/verdicts.txt (every LEG line), logs/verdicts.env (L<n>=<verdict>),
# and logs/legs-<names>.txt with this invocation's VERDICT PASS|VOID|FAIL. Exit 0 on VERDICT PASS or VOID.
set -u; D=$(cd "$(dirname "$0")" && pwd); . $D/kit.sh; . $D/patterns.env
[ $# -gt 0 ] || set -- L0 L1 L2 L3 L4 L5 L6 L7 L8 L9
for g in "$@"; do case " L0 L1 L2 L3 L4 L5 L6 L7 L8 L9 " in *" $g "*) ;; *) echo "no leg '$g' (L0 .. L9)"; exit 2;; esac; done
RENEW || exit 1; VOLOK || exit 2
grep -q '^VERIFY PASS' $L/verify.txt 2>/dev/null || { echo "no VERIFY PASS in row/logs/verify.txt: the PR build is not verified (../run.sh verify)"; exit 1; }
for c in 20-ask-a 21-ask-b 22-ask-c 23-ask-d 24-ask-e 26-ask-g; do [ -s $A/$c.wav ] || { echo "no clip $A/$c.wav"; exit 1; }; done
export HEAD_SHA LAMP
POLL=${POLL:-1}; RUNWAIT=${RUNWAIT:-240}; BACKWAIT=${BACKWAIT:-300}; LEAVEWAIT=${LEAVEWAIT:-20}; READYWAIT=${READYWAIT:-120}
TOLDWAIT=${TOLDWAIT:-90}; REDOWATCH=${REDOWATCH:-120}; HOLDFOR=${HOLDFOR:-180}; L7SETTLE=${L7SETTLE:-30}; ASKWAIT=${ASKWAIT:-240}
LONGWAIT=${LONGWAIT:-400}; SETTLE=${SETTLE:-5}; MIDTURN=${MIDTURN:-8}; HDSETTLE=${HDSETTLE:-20}; FSETTLE=${FSETTLE:-3}
NOTTAKE='off|not_installed|installing|starting|recovering|error|none'
BG=""; SICK=""; JP=; AP=; AWN=
trap 'for p in $BG; do kill $p 2>/dev/null; done; trap - INT TERM HUP; LEGGUARDS' EXIT; trap 'exit 130' INT TERM HUP
cap(){ if [ -n "${WAITMAX:-}" ] && [ "$1" -gt "$WAITMAX" ]; then echo $WAITMAX; else echo $1; fi; }   # WAITMAX: mocks only
LN(){ wc -l < $L/$LEG.$1 2>/dev/null | tr -d ' '; }
LOCKSTOP(){ echo "LOCK: not ours: the legs stop here, no ssh"; exit 1; }
# WF <ERE> <file> <s> [after line k]: wait for a line in a file; RENEW every 60 s (a lost lock stops the legs).
WF(){ local s=$(date +%s) r=$(date +%s) lim; lim=$(cap $3)
  while :; do tail -n +$(( ${4:-0} + 1 )) "$2" 2>/dev/null | grep -qE -- "$1" && return 0
    [ $(( $(date +%s) - s )) -ge $lim ] && return 1
    [ $(( $(date +%s) - r )) -ge 60 ] && { RENEW >/dev/null || LOCKSTOP; r=$(date +%s); }
    sleep $POLL; done; }
WJ(){ WF "$1" $L/$LEG.journal $2 ${3:-0}; }
# WAITAW <states ERE> <after line k> <s>: AWN = the first AW line number after k in one of the states (else empty).
WAITAW(){ local s=$(date +%s) r=$(date +%s) lim; lim=$(cap $3); AWN=
  while :; do AWN=$(awk -v k=$2 -v re="^($1)\$" 'NR > k && $1 == "AW" && $3 ~ re {print NR; exit}' $L/$LEG.aw 2>/dev/null)
    [ -n "$AWN" ] && return 0
    [ $(( $(date +%s) - s )) -ge $lim ] && return 1
    [ $(( $(date +%s) - r )) -ge 60 ] && { RENEW >/dev/null || LOCKSTOP; r=$(date +%s); }
    sleep $POLL; done; }
PL(){ PLAY "$@" || { echo "STOP: clip $1 not played"; exit 2; }; }
VOIDM(){ echo "VOID $*" >> $L/$LEG.marks; echo "  $LEG VOID: $*"; }
MARK(){ echo "$1 $(python3 -c 'import sys,time;print("%.3f"%(time.time()+float(sys.argv[1])))' ${SKEWS:-0})" >> $L/$LEG.marks; }   # on the lamp's clock
KEPT(){ LAMP "LEG=$LEG" $D/kept.lamp.sh > $L/$LEG.kept-$1 2>&1; grep -E '^(KEPT_AGENT|PANEPID)' $L/$LEG.kept-$1 | tr '\n' ' '; echo; }
SOCKASK(){ LAMP "LEG=$LEG ASK_B64=$(WORDS $2 | base64 | tr -d '\n') ASK_WAIT=${3:-$ASKWAIT}" $D/ask.lamp.sh > $L/$LEG.$1 2>&1; grep -E '^ASK_(ANSWER|NOT_SENT|REFUSED|NO_ANSWER|ASKING)' $L/$LEG.$1 | head -1; }
TURN(){ local k=$(LN journal); PL $1; WJ "$P_T_DONE|$P_V_REFUSED|$P_V_HELD" $RUNWAIT $k || echo "  $LEG: no turn end for $1 in $RUNWAIT s"; }
START(){ LEG=$1; local n=$2; shift 2; echo "== leg $LEG $(date +%T)" | tee -a $L/legs.log
  rm -f $L/$LEG.*; : > $L/$LEG.marks; : > $L/$LEG.journal; : > $L/$LEG.aw; JP=; AP=; MARK START
  RENEW >/dev/null || LOCKSTOP; VOLOK || exit 2
  if [ $n -gt 0 ]; then GATE $n "$@" > $L/$LEG.gate 2>&1 || { VOIDM "start budget: $(cat $L/$LEG.gate)"; return 1; }; fi
  LAMP "LEG=$LEG" $D/mute.lamp.sh > $L/$LEG.mute 2>&1; grep -q '^MUTE OFF OK' $L/$LEG.mute || { VOIDM "the mic is muted ($(grep '^MUTE' $L/$LEG.mute)): ../run.sh micon"; return 1; }
  JP=$(JF $L/$LEG.journal 1500); AP=$(AF $L/$LEG.aw 1500); BG="$BG $JP $AP"; sleep $FSETTLE
  WAITAW ready 0 $READYWAIT || { VOIDM "the agent is not ready at the start ($(awk '$1=="AW"{s=$3"("$4")"} END{print s}' $L/$LEG.aw))"; return 1; }
  return 0; }
END(){ [ -n "$JP$AP" ] && { sleep ${TAIL:-3}; kill $JP $AP 2>/dev/null; }; RENEW >/dev/null || LOCKSTOP
  python3 -I $D/verdict.py $L $LEG > $L/$LEG.out 2>&1; cat $L/$LEG.out; v=$(grep "^LEG $LEG " $L/$LEG.out | tail -1)
  [ -n "$v" ] || v="LEG $LEG FAIL: verdict.py gave no verdict ($(tail -1 $L/$LEG.out))"
  echo "$v" >> $L/verdicts.txt; grep -v "^$LEG=" $L/verdicts.env > $L/verdicts.env.t 2>/dev/null; echo "$LEG=$(echo $v | awk '{print $3}' | tr -d :)" >> $L/verdicts.env.t; mv $L/verdicts.env.t $L/verdicts.env; }
# A fault leg's agent never back to ready: the legs after it are not run.
BACKED(){ [ "$1" = ok ] || { SICK=$LEG; echo "  $LEG: the agent is not back to ready in $BACKWAIT s"; }; }

leg_L0(){ START L0 0 || { END; return; }
  TURN 20-ask-a; SOCKASK ask-after 23-ask-d; END; }

leg_L1(){ START L1 1 lamp-openclaw-gateway || { END; return; }
  KEPT before; local k=$(LN aw) kl b=ok
  LAMP "LEG=L1" $D/gw.lamp.sh > $L/L1.gw 2>&1 & local gp=$!; BG="$BG $gp"
  WF '^GWR_AT' $L/L1.gw 30 || { wait $gp; VOIDM "the gateway was not restarted ($(tail -1 $L/L1.gw))"; END; return; }
  WAITAW "$NOTTAKE" $k $LEAVEWAIT || echo "  L1: tasks/agent did not leave ready in $LEAVEWAIT s"; kl=$AWN
  PL 21-ask-b; WAITAW ready ${kl:-$k} $BACKWAIT || b=no
  wait $gp; WJ "$P_V_READYTOLD" $TOLDWAIT || echo "  L1: no 'ready again' told in $TOLDWAIT s"
  KEPT after; [ $b = ok ] && TURN 22-ask-c; END; BACKED $b; }

leg_L2(){ START L2 1 lamp-openclaw-gateway || { END; return; }
  KEPT before; local kj=$(LN journal) k kl b=ok
  LAMP "LEG=L2 ASK_B64=$(WORDS 40-long | base64 | tr -d '\n') ASK_WAIT=$LONGWAIT" $D/ask.lamp.sh > $L/L2.ask-long 2>&1 & local ap=$!; BG="$BG $ap"
  WJ "$P_T_TAKEN" 90 $kj || { kill $ap 2>/dev/null; VOIDM "the long task was not handed over in 90 s"; END; return; }
  sleep $MIDTURN; k=$(LN aw)
  LAMP "LEG=L2" $D/gw.lamp.sh > $L/L2.gw 2>&1 & local gp=$!; BG="$BG $gp"
  WF '^GWR_AT' $L/L2.gw 30 || { wait $gp; VOIDM "the gateway was not restarted"; END; return; }
  WJ "$P_T_BROKE|$P_T_REFUSEDHAND|$P_T_STUCK" 60 $kj || echo "  L2: no 'the task ends now' line in 60 s"
  WAITAW "$NOTTAKE" $k $LEAVEWAIT; kl=$AWN; WAITAW ready ${kl:-$k} $BACKWAIT || b=no; wait $gp
  MARK REDO_FROM; sleep $REDOWATCH; MARK REDO_TO      # 120 s watched for the task run again
  KEPT after; [ $b = ok ] && TURN 23-ask-d
  WF '^ASK_(NOT_SENT|ANSWER|NO_ANSWER|REFUSED)' $L/L2.ask-long 60 || kill $ap 2>/dev/null; END; BACKED $b; }

leg_jam(){ local mode=$1 ask=$2 after=$3 k kj kl b=ok
  KEPT before | sed "s/^/  /"
  grep -qE '^PANEPID [0-9]+$' $L/$LEG.kept-before || { VOIDM "no single pane openclaw pid ($(grep '^PANEPID' $L/$LEG.kept-before))"; END; return; }
  k=$(LN aw); kj=$(LN journal)
  [ $mode = stop ] && touch $L/stopped.on      # before the stop: a cut between leaves the SIGCONT guard on
  LAMP "LEG=$LEG MODE=$mode" $D/jam.lamp.sh > $L/$LEG.jam 2>&1; grep -E '^(JAM_PID|JAM_AT|KILL|STOP|GW_SAME|JAM REFUSED)' $L/$LEG.jam | sed 's/^/  /'
  grep -q '^JAM_AT ' $L/$LEG.jam || { VOIDM "the pane was not jammed ($(grep '^JAM REFUSED' $L/$LEG.jam))"; STOPGUARD; END; return; }
  if [ $mode = kill ]; then WAITAW "$NOTTAKE" $k $LEAVEWAIT; kl=$AWN; PL $ask
  else sleep ${JAMSETTLE:-3}; PL $ask; WJ "$P_T_REFUSEDHAND|$P_T_WARMING|$P_T_STUCK|$P_T_BROKE|$P_V_REFUSED" 120 $kj || echo "  $LEG: no end line in 120 s"
    WAITAW "$NOTTAKE" $k $LEAVEWAIT; kl=$AWN; fi
  WAITAW ready ${kl:-$k} $BACKWAIT || b=no
  if [ $mode = stop ]; then LAMP "LEG=$LEG" $D/unfreeze.lamp.sh > $L/$LEG.unfreeze 2>&1; cat $L/$LEG.unfreeze | sed 's/^/  /'
    grep -q '^UNFREEZE OK' $L/$LEG.unfreeze && rm -f $L/stopped.on; fi
  WJ "$P_V_READYTOLD" $TOLDWAIT || echo "  $LEG: no 'ready again' told in $TOLDWAIT s"
  KEPT after > /dev/null; sleep $SETTLE; [ $b = ok ] && SOCKASK ask-after $after; END; BACKED $b; }
leg_L3(){ START L3 0 || { END; return; }; leg_jam kill 24-ask-e 25-ask-f; }
leg_L4(){ START L4 0 || { END; return; }; leg_jam stop 26-ask-g 27-ask-h; }

leg_L5(){ START L5 0 || { END; return; }
  local kj=$(LN journal)
  LAMP "LEG=L5" $D/hd.lamp.sh > $L/L5.hd 2>&1 & local dp=$!; BG="$BG $dp"
  WF '^HDR_AT' $L/L5.hd 30 || { wait $dp; VOIDM "Harness's daemon was not restarted"; END; return; }
  sleep ${HDASK:-2}; PL 20-ask-a; wait $dp
  WJ "$P_T_WATCHAGAIN" 120 $kj || echo "  L5: no 'ensured and watched again' in 120 s"
  sleep $HDSETTLE; SOCKASK ask-after 27-ask-h; END; }

leg_L6(){ START L6 0 || { END; return; }
  LAMP "LEG=L6" $D/l6.lamp.sh > $L/L6.read 2>&1; cat $L/L6.read | sed 's/^/  /'
  [ -n "${APP_L6:-}" ] && echo "APP $APP_L6" >> $L/L6.marks; END; }

leg_L7(){ START L7 4 lamp-update lamp-openclaw-gateway lamp-tasks || { END; return; }
  local k=$(LN aw) kl g b=ok
  touch $L/wrap.on
  LAMP "LEG=L7 MODE=go" $D/l7.lamp.sh > $L/L7.install 2>&1; grep -E '^(L7 STOP|L7_AT|STRIPPED|ORDER|L7_ORDER|L7_GW_HEALTH_S|GW_PID_AFTER|WRAPPER)' $L/L7.install | sed 's/^/  /'
  if grep -q '^L7 STOP' $L/L7.install; then VOIDM "$(grep '^L7 STOP' $L/L7.install)"; WRAPGUARD; END; return; fi
  g=$(sed -n 's/^GW_PID_AFTER \([0-9]*\).*/\1/p' $L/L7.install)
  WAITAW "$NOTTAKE" $k 10; kl=$AWN; WAITAW ready ${kl:-$k} $BACKWAIT || b=no
  sleep $L7SETTLE
  LAMP "LEG=L7 GWPID=$g" $D/l7check.lamp.sh > $L/L7.check 2>&1; grep '^L7_CONFIG' $L/L7.check | sed 's/^/  /'
  [ $b = ok ] && SOCKASK ask-after 25-ask-f
  WRAPGUARD
  if grep -q '^L7_CONFIG NOT LIVE' $L/L7.check; then
    echo "  L7: lamp-tasks' merge not live: the gateway restarted once, so the lamp is not left without its provider"
    GATE 1 lamp-openclaw-gateway && LAMP "LEG=L7fix" $D/gw.lamp.sh > $L/L7.fix 2>&1; fi
  END; BACKED $b; }

leg_L8(){ START L8 4 lamp-update lamp-openclaw-gateway lamp-tasks || { END; return; }
  local k=$(LN aw) kj=$(LN journal) kl b=ok
  touch $L/wrap.on
  LAMP "LEG=L8 MODE=cut" $D/l7.lamp.sh > $L/L8.cut 2>&1; grep -E '^(L7 STOP|L8 STOP|L7_AT|CUT_AT|GAP)' $L/L8.cut | sed 's/^/  /'
  if grep -qE '^L[78] STOP' $L/L8.cut || ! grep -q '^CUT_AT ' $L/L8.cut; then VOIDM "$(grep -E '^L[78] STOP' $L/L8.cut || echo 'the install was not cut off')"; WRAPGUARD; END; return; fi
  PL 21-ask-b
  WJ "$P_U_CUTBACK" 120 $kj || echo "  L8: no 'started again: OpenClaw's install ended' in 120 s"
  WAITAW "$NOTTAKE" $k 10; kl=$AWN; WAITAW ready ${kl:-$k} $BACKWAIT || b=no
  LAMP "LEG=L8" $D/l8back.lamp.sh > $L/L8.back 2>&1; grep -E '^(ONESHOT_END|TASKS_BACK|CUTBACK_LINES|RECORD|L8BACK|GW)' $L/L8.back | sed 's/^/  /'
  WRAPGUARD; sleep $SETTLE; [ $b = ok ] && SOCKASK ask-after 22-ask-c; END; BACKED $b; }

leg_L9(){ START L9 0 || { END; return; }
  [ -n "${HOLD_CMD:-}" ] || { VOIDM "not induced: no HOLD_CMD (a lasting hold is unit-tested only)"; END; return; }
  local kj=$(LN journal) k=$(LN aw)
  touch $L/hold.on; MARK HOLD_AT
  LAMP_HOST=$LAMP_HOST bash -c "$HOLD_CMD" >> $L/hold.txt 2>&1 || { VOIDM "HOLD_CMD failed (row/logs/hold.txt)"; HOLDGUARD; END; return; }
  WJ "$P_T_HELD" 120 $kj || { VOIDM "the hold was not made: no held line in 120 s"; HOLDGUARD; END; return; }
  PL 23-ask-d; sleep $HOLDFOR
  MARK UNHOLD_AT; HOLDGUARD
  WAITAW ready $k $BACKWAIT || BACKED no; WJ "$P_V_READYTOLD" $TOLDWAIT || echo "  L9: no 'ready again' told in $TOLDWAIT s"
  END; }

S=$(SKEW); SKEWS=$(echo $S | awk '{print $1}'); echo "SKEW $SKEWS (rtt $(echo $S | awk '{print $2}'))" > $L/skew.txt
for g in "$@"; do
  [ -n "$SICK" ] && { echo "STOP: the agent is not back to ready after leg $SICK: leg $g and the rest not run (look at the lamp)" | tee -a $L/legs.log; break; }
  leg_$g
done
NAME=$(echo "$@" | tr -d ' ')
V=$(for g in "$@"; do grep "^LEG $g " $L/verdicts.txt 2>/dev/null | tail -1; done)
if echo "$V" | grep -q ' FAIL' || [ -n "$SICK" ]; then W=FAIL; elif echo "$V" | grep -q ' VOID'; then W=VOID; else W=PASS; fi
[ $(echo "$V" | grep -c '^LEG ') = $# ] || W=FAIL
{ echo "$V"; echo "VERDICT $W ($(echo "$V" | awk '{printf "%s=%s ", $2, $3}' | tr -d :))"; } > $L/legs-$NAME.txt
tail -1 $L/legs-$NAME.txt
[ -n "$SICK" ] && exit 3
[ $W = FAIL ] && exit 1; exit 0
