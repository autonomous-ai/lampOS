#!/bin/bash
# Mac, lock held. The gate's core rows on the lamp after round 2 (the first builder's 12:02 list): each a clip from the
# Mac's speaker (afplay -v gain only, Mac volume read <= 70; the lamp's speaker volume read, <= VOLCAP; nothing set),
# the lamp's journal followed, and the lines that prove the row quoted into gate/logs/core.txt.
#   TIME  "Lamp, what time is it?": the request heard, and an answer said (a pieces line) within ANSWERWAIT s.
#   TASK  one task handed to OpenClaw that reaches the lamp: a handed line (TASK_HANDED_RE) and a skill-start run
#         (SKILLSTART_RE: lamp-tasks' "skill start: run <id>'s script started") within TASKWAIT s, with no refusal.
#   MAKE  one ordinary voice make, kept: "a skill made here was kept in N ms", then on the lamp (at once, or on the
#         played YES after a read-back). The made skill stays (its name is printed for the runner).
# Prints "ROW <TIME|TASK|MAKE> PASS|FAIL <why>" with "  QUOTE <journal line>" lines, then CORE PASS | CORE FAIL.
set -u; D=$(cd "$(dirname "$0")" && pwd); . $D/kit.sh
LG=$D/logs; ROWS=${ROWS:-TIME TASK MAKE}
POLL=${POLL:-1}; HEARWAIT=${HEARWAIT:-60}; ANSWERWAIT=${ANSWERWAIT:-60}; TASKWAIT=${TASKWAIT:-300}; MAKEWAIT=${MAKEWAIT:-660}
TASK_HANDED_RE=${TASK_HANDED_RE:-"handed to the lamp's agent|skill task [^ ]+: started on the lamp's default agent|the heard words named [^ ]+ for the work"}
SKILLSTART_RE=${SKILLSTART_RE:-"\\[lamp-tasks\\] skill start: run [^ ]+'s script started"}
HEARD="lamp-voice.*request [0-9]+"
PIECES="[0-9]+ of [0-9]+ pieces began"
REFUSE="no token made|skill start: refused|was refused already|fence never said it started|not run: "
KEPT="a skill made here was kept in [0-9]+ ms"
NOTKEPT="a skill made here wasn't kept in|a skill wasn't made|the lamp's model said no made skill can do it|wasn't kept: |was not kept: "
ATONCE="is on at once \\(LAMP_MADE_CODE_ON_AT_ONCE\\)"
READBACK="read back: it waits for the person's yes"
ON="a made skill is on the lamp: "
for c in $ROWS; do [ -f $A/$c-el.wav ] || { echo "no clip $A/$c-el.wav: ../run.sh render"; exit 1; }; done
JP=; trap '[ -n "$JP" ] && kill $JP 2>/dev/null' EXIT; trap 'exit 130' INT TERM HUP
lines(){ wc -l < $J | tr -d ' '; }
waitfor(){ local s=$(date +%s) r=$(date +%s) n lim=$3
  [ -n "${WAITMAX:-}" ] && [ "$lim" -gt "$WAITMAX" ] && lim=$WAITMAX     # WAITMAX: mock runs only
  while [ $(( $(date +%s) - s )) -lt $lim ]; do
    n=$(tail -n +$(( $2 + 1 )) $J | grep -nE -- "$1" | head -1 | cut -d: -f1); [ -n "$n" ] && { echo $(( n + $2 )); return 0; }
    [ $(( $(date +%s) - r )) -ge 120 ] && { RENEW || return 2; r=$(date +%s); }
    sleep $POLL; done; return 1; }
show(){ sed -n "${1}p" $J | cut -c1-240; }
Q(){ echo "  QUOTE $(show $1)"; }
quiet(){ local a b i=0; while [ $i -lt 15 ]; do a=$(grep -cE "$HEARD|$PIECES" $J); sleep ${QUIETS:-6}; b=$(grep -cE "$HEARD|$PIECES" $J); [ "$a" = "$b" ] && return 0; i=$((i+1)); done; }
BEGIN(){ LEG=$1; J=$LG/core-$1.journal.txt; echo "== row $1 $(date +%T)"; RENEW || return 1; : > $J; JP=$(JF $J); sleep 3; }
END(){ sleep ${TAIL:-8}; kill $JP 2>/dev/null; JP=; }
VT=FAIL; VK=FAIL; VM=FAIL
rowTIME(){ local n0 h a; BEGIN TIME || return 1; n0=$(lines); PLAY TIME || { END; echo "ROW TIME FAIL the clip was not played"; return 0; }
  h=$(waitfor "$HEARD" $n0 $HEARWAIT) || { END; echo "ROW TIME FAIL the ask was not heard in $HEARWAIT s (mute, Mac volume, speaker placement)"; return 0; }
  a=$(waitfor "$PIECES" $h $ANSWERWAIT) || { END; echo "ROW TIME FAIL heard, no answer said in $ANSWERWAIT s"; Q $h; return 0; }
  END; VT=PASS; echo "ROW TIME PASS heard, and its answer said"; Q $h; Q $a; }
rowTASK(){ local n0 h k s r; BEGIN TASK || return 1; quiet; n0=$(lines); PLAY TASK || { END; echo "ROW TASK FAIL the clip was not played"; return 0; }
  h=$(waitfor "$HEARD" $n0 $HEARWAIT) || { END; echo "ROW TASK FAIL the ask was not heard in $HEARWAIT s"; return 0; }
  k=$(waitfor "$TASK_HANDED_RE" $n0 $TASKWAIT) || { END; echo "ROW TASK FAIL heard, not handed to OpenClaw in $TASKWAIT s"; Q $h; return 0; }
  s=$(waitfor "$SKILLSTART_RE" $k $TASKWAIT) || { END; echo "ROW TASK FAIL handed, but no skill-start run reached the lamp in $TASKWAIT s"; Q $h; Q $k; return 0; }
  END; r=$(tail -n +$(( n0 + 1 )) $J | grep -nE "$REFUSE" | head -1 | cut -d: -f1)
  [ -n "$r" ] && { echo "ROW TASK FAIL a refusal in the task"; Q $h; Q $k; Q $s; Q $(( r + n0 )); return 0; }
  VK=PASS; echo "ROW TASK PASS handed to OpenClaw, and its skill-start run reached the lamp"; Q $h; Q $k; Q $s
  tail -n +$(( s + 1 )) $J | grep -E "lamp-control: run [^ ]+ of skill [^ ]+: (a token for|ended)" | head -2 | cut -c1-240 | sed 's/^/  QUOTE /'; }
rowMAKE(){ local n0 h w at rb o nm; BEGIN MAKE || return 1; quiet; n0=$(lines); PLAY MAKE || { END; echo "ROW MAKE FAIL the clip was not played"; return 0; }
  h=$(waitfor "$HEARD" $n0 $HEARWAIT) || { END; echo "ROW MAKE FAIL the ask was not heard in $HEARWAIT s"; return 0; }
  w=$(waitfor "$KEPT|$NOTKEPT" $n0 $MAKEWAIT) || { END; echo "ROW MAKE FAIL no make end in $MAKEWAIT s"; Q $h; return 0; }
  show $w | grep -qE "$KEPT" || { END; echo "ROW MAKE FAIL not kept"; Q $h; Q $w; return 0; }
  at=$(tail -n +$(( n0 + 1 )) $J | grep -nE "$ATONCE" | head -1 | cut -d: -f1)
  if [ -z "$at" ]; then rb=$(waitfor "$READBACK" $n0 ${RBWAIT:-60}) || rb=$w
    sleep ${YESGAP:-15}; PLAY YES; fi
  o=$(waitfor "$ON" $n0 ${ONWAIT:-120}); END
  [ -n "$o" ] || { echo "ROW MAKE FAIL kept, but not on the lamp in ${ONWAIT:-120} s"; Q $h; Q $w; return 0; }
  nm=$(show $o | sed -n "s/.*$ON\([^,]*\).*/\1/p"); echo "$nm" > $LG/core-made.name
  VM=PASS; echo "ROW MAKE PASS kept and on the lamp ($( [ -n "$at" ] && echo "at once" || echo "on the played yes")): $nm stays (say 'remove it' or remove it in the app after the gate)"
  Q $h; Q $w; [ -n "$at" ] && Q $(( at + n0 )); Q $o; }
VOLOK || exit 2; MUTEOK=0 VOLCHK core || exit 2
for r in $ROWS; do row$r; done
v=PASS; for r in $ROWS; do case $r in TIME) [ $VT = PASS ] || v=FAIL;; TASK) [ $VK = PASS ] || v=FAIL;; MAKE) [ $VM = PASS ] || v=FAIL;; esac; done
echo "CORE $v (rows: $ROWS)"
