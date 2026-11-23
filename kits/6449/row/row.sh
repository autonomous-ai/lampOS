#!/bin/bash
# Mac. #6449 ROW legs on $LAMP (row/kit.sh settings): the installed stable build with #6449's head's lamp-voice bound
# over it (`swap`) and #6449's product-unit line laid as the row's drop-in (`on`: LAMP_ACTION_CLAIM_WORDED=on, with
# LAMP_NAME_NEAR=off for the Mac's voice). Run by ../run.sh's `legs` step after check, micon, save and on. Hands-free:
# a pre-rendered ElevenLabs clip from the Mac speaker (afplay -v gain only; Mac volume read, <= 70, never set).
# Verdicts from the journal only (logs/leg-<L>.journal.txt): no words of the person's line or the lamp's reply are
# read (the product journal holds none).
#   E  lamp-voice's environment: LAMP_ACTION_CLAIM_WORDED=on (the row's drop-in, #6449's line) beside the product's
#      LAMP_ACTION_CLAIM_CHECKED=on, on #6449's head's lamp-voice (logs/on.txt SWITCH ON OK).
#   W  GO-159's line, "Hey Lamp, keep the light on until ten.", in TRIES (7) fresh tries (`fresh` before each: a new
#      lamp-voice, the kept rules, memory, schedule and light as saved). Each try: heard (a request line), its calls
#      (the turn's `tools: ... calls=` line), and any hold of a claim no call carried: "its reply claims a ... action
#      no call made ...: not said, asked again (LAMP_ACTION_CLAIM_CHECKED, in other words: LAMP_ACTION_CLAIM_WORDED,
#      ...)", #7140's worded family. PASS: every try heard, at least one WORDED hold, each hold answered by a later
#      round of the same request (asked again, not dropped), no lamp-voice exit. VOID: fewer than TRIES heard, or no
#      worded claim came up in any try (the model never made one: nothing to hold). FAIL: a hold not followed by a
#      round, or lamp-voice exited in a try.
#   ./row.sh [legs]   (default E W). Each leg prints CHECK lines and LEG x PASS|FAIL|VOID; then VERDICT.
set -u; D=$(cd "$(dirname "$0")" && pwd); . $D/kit.sh
LG=$D/logs
[ $# -gt 0 ] && LEGS="$*"
LEGS=${LEGS:-E W}
RENEW || exit 1; VOLOK || exit 2
grep -q '^VERIFY PASS' $LG/check.txt 2>/dev/null || { echo "no VERIFY PASS in row/logs/check.txt: #6449's head's lamp-voice is not verified running (../run.sh check)"; exit 1; }
grep -q '^READY' $LG/check.txt 2>/dev/null || { echo "no READY in row/logs/check.txt (../run.sh check)"; exit 1; }
grep -q '^SAVED OK' $LG/save.txt 2>/dev/null || { echo "no SAVED OK in row/logs/save.txt (../run.sh save first)"; exit 1; }
for c in GO159; do [ -f $A/$c-el.wav ] || { echo "no clip $A/$c-el.wav: ../run.sh render"; exit 1; }; done
JP=; FP=; trap '[ -n "$JP" ] && kill $JP 2>/dev/null; [ -n "$FP" ] && kill $FP 2>/dev/null' EXIT; trap 'exit 130' INT TERM HUP
POLL=${POLL:-1}; TURNWAIT=${TURNWAIT:-60}; TRIES=${TRIES:-7}
lines(){ wc -l < $J | tr -d ' '; }
# first line number after line $2 matching ERE $1, waiting at most $3 s (lock renewed every 120 s)
waitfor(){ local s=$(date +%s) r=$(date +%s) n lim=$3
  [ -n "${WAITMAX:-}" ] && [ "$lim" -gt "$WAITMAX" ] && lim=$WAITMAX     # WAITMAX: mock runs only
  while [ $(( $(date +%s) - s )) -lt $lim ]; do
    n=$(tail -n +$(( $2 + 1 )) $J | grep -nE -- "$1" | head -1 | cut -d: -f1); [ -n "$n" ] && { echo $(( n + $2 )); return 0; }
    [ $(( $(date +%s) - r )) -ge 120 ] && { RENEW || return 2; r=$(date +%s); }
    sleep $POLL; done; return 1; }
ACT='request [0-9]+|pieces began'
quiet(){ local a b i=0; while [ $i -lt 15 ]; do a=$(grep -cE "$ACT" $J); sleep ${QUIETS:-6}; b=$(grep -cE "$ACT" $J); [ "$a" = "$b" ] && return 0; i=$((i+1)); done; }
# the say-back's end after line $1: "K of K pieces began, ... M chars" -> M/12 + 3 s
saydone(){ local t c; t=$(waitfor "[0-9]+ of [0-9]+ pieces began" $1 ${SAYWAIT:-40}) || { echo "  no pieces line: waiting 35 s"; sleep 35; return; }
  c=$(sed -n "${t}p" $J | grep -oE "[0-9]+ chars" | tail -1 | grep -oE "[0-9]+"); echo "  say-back ${c:-?} chars: ends in ~$(( ${c:-0} / 12 + 3 )) s"; sleep $(( ${c:-0} / 12 + 3 )); }
show(){ sed -n "${1}p" $J | cut -c1-240; }
field(){ sed -n "s/.* $1=\([^ ]*\).*/\1/p"; }
has(){ echo ",$1," | grep -q ",$2,"; }
ck(){ [ "$2" = 1 ] && echo PASS || { [ "$2" = v ] && echo VOID || echo FAIL; }; }
BEGIN(){ LEG=$1; J=$LG/leg-$1.journal.txt; echo "== leg $1 $(date +%T)"; RENEW || return 1; : > $J; JP=$(JF $J); sleep 3; }
END(){ sleep ${TAIL:-8}; kill $JP 2>/dev/null; JP=; }

WORDHOLD="not said, asked again \(LAMP_ACTION_CLAIM_CHECKED, in other words: LAMP_ACTION_CLAIM_WORDED"
ANYHOLD="its reply claims a .* no call made .*: not said, asked again \(LAMP_ACTION_CLAIM_CHECKED"
EXITED="lamp-voice.service: (Main process exited|Failed with result)|lamp-voice\.service: Scheduled restart"
legE(){ BEGIN E; local o=1 w c
  w=$(grep -oE 'LAMP_ACTION_CLAIM_WORDED=[a-z]*' $LG/on.txt 2>/dev/null | head -1); c=$(grep -oE 'LAMP_ACTION_CLAIM_CHECKED=[a-z]*' $LG/on.txt 2>/dev/null | head -1)
  grep -q '^SWITCH ON OK' $LG/on.txt 2>/dev/null || o=0
  echo "  CHECK E1 the drop-in's switch in lamp-voice: ${w:-none}: $([ "$w" = LAMP_ACTION_CLAIM_WORDED=on ] && echo PASS || echo FAIL)"
  echo "  CHECK E2 the product's check beside it: ${c:-none}: $([ "$c" = LAMP_ACTION_CLAIM_CHECKED=on ] && echo PASS || echo FAIL)"
  echo "  CHECK E3 on #6449's head's lamp-voice: $(grep -E '^(PASS|FAIL) lamp-voice runs' $LG/check.txt | head -1 | cut -c1-120)"
  grep -q '^PASS lamp-voice runs' $LG/check.txt || o=0; [ "$w" = LAMP_ACTION_CLAIM_WORDED=on ] && [ "$c" = LAMP_ACTION_CLAIM_CHECKED=on ] || o=0
  VE=$(ck x $o); WHY="WORDED and CHECKED on in #6449's head's lamp-voice"; [ $o = 1 ] || WHY="the switch or the build is not as #6449 would have it (logs/on.txt, logs/check.txt)"; END; }
legW(){ BEGIN W; local t=1 heard=0 holds=0 bad=0 ex=0 n0 r q c h
  while [ $t -le $TRIES ]; do
    RENEW || return 1; GATE lamp-voice lamp-memory lamp-schedule || { echo "  wait 60 s for the start budget"; sleep 60; continue; }
    LAMP "" $D/fresh.lamp.sh > $LG/fresh-$t.txt 2>&1; grep -q '^FRESH OK' $LG/fresh-$t.txt || { echo "  try $t: $(tail -1 $LG/fresh-$t.txt)"; VW=FAIL; WHY="fresh failed at try $t (logs/fresh-$t.txt)"; END; return; }
    sleep ${SETTLE:-6}; n0=$(lines); PLAY GO159
    r=$(waitfor "lamp-voice second: request [0-9]+" $n0 ${HEARWAIT:-25}) || { echo "  try $t: not heard"; t=$((t+1)); continue; }
    q=$(sed -n "${r}p" $J | grep -oE 'request [0-9]+' | head -1 | grep -oE '[0-9]+'); heard=$((heard+1))
    c=$(waitfor "lamp-voice second: request $q: tools: " $r $TURNWAIT) && c=$(sed -n "${c}p" $J | field calls) || c="(no tools line)"
    quiet
    h=$(tail -n +$(( n0 + 1 )) $J | grep -nE "request $q chat round [0-9]+: .*$ANYHOLD" | cut -d: -f1)
    for x in $h; do x=$(( x + n0 )); sed -n "${x}p" $J | grep -qE "$WORDHOLD" && holds=$((holds+1))
      rd=$(sed -n "${x}p" $J | grep -oE 'chat round [0-9]+' | grep -oE '[0-9]+')
      tail -n +$(( x + 1 )) $J | grep -qE "request $q (chat round $(( rd + 1 ))|: tools: )" || { bad=$((bad+1)); echo "  try $t: a hold with no later round (request $q round $rd)"; }
      echo "  try $t HOLD: $(show $x | sed 's/.*lamp-voice second: //' | cut -c1-200)"; done
    tail -n +$(( n0 + 1 )) $J | grep -qE "$EXITED" && { ex=$((ex+1)); echo "  try $t: lamp-voice exited"; }
    echo "  try $t: request $q calls=${c:-none} holds=$(echo $h | wc -w | tr -d ' ')"
    t=$((t+1)); done
  echo "  CHECK W1 every try heard: $heard of $TRIES: $([ $heard = $TRIES ] && echo PASS || echo VOID)"
  echo "  CHECK W2 a worded claim held (LAMP_ACTION_CLAIM_WORDED): $holds: $([ $holds -ge 1 ] && echo PASS || echo VOID)"
  echo "  CHECK W3 each hold asked again, never dropped: $([ $bad = 0 ] && echo PASS || echo "FAIL ($bad)")"
  echo "  CHECK W4 lamp-voice never exited in a try: $([ $ex = 0 ] && echo PASS || echo "FAIL ($ex)")"
  if [ $bad -gt 0 ] || [ $ex -gt 0 ]; then VW=FAIL; WHY="a hold dropped ($bad) or lamp-voice exited ($ex)"
  elif [ $heard -lt $TRIES ]; then VW=VOID; WHY="$heard of $TRIES tries heard"
  elif [ $holds = 0 ]; then VW=VOID; WHY="no worded claim came up in $TRIES tries: nothing to hold"
  else VW=PASS; WHY="$holds worded hold(s) in $TRIES tries, each asked again"; fi; END; }
for g in $LEGS; do WHY=
  case $g in E) legE; echo "LEG E $VE ($WHY)";; W) legW; echo "LEG W $VW ($WHY)";; *) echo "no leg $g"; continue;; esac
  eval "echo V$g=\$V$g" >> $LG/verdicts.env
done 2>&1 | tee $LG/row-$(echo $LEGS | tr -d ' ').txt
f=$LG/row-$(echo $LEGS | tr -d ' ').txt
if grep -qE '^LEG [A-Z]+ FAIL' $f; then v=FAIL; elif grep -qE '^LEG [A-Z]+ VOID' $f; then v=VOID; else v=PASS; fi
echo "VERDICT $v ($(grep -E '^LEG ' $f | awk '{print $2"="$3}' | tr '\n' ' '))" | tee -a $f
[ $v = PASS ]
