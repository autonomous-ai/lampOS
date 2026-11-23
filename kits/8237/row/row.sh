#!/bin/bash
# Mac. #8237's legs: P (the pushed fixture's yes kept, warm gateway), G (cold gateway: the getting-ready line), A (one
# approvals read in flight, with back-off, sampled during G), W (warm gateway: nothing said). Journal strings are the
# product's own (see ../RUN-8237.md "Journal lines"). Each leg: CHECK lines, then LEG <x> PASS|FAIL|VOID.
#   row.sh [legs]   default: P G A W  -> logs/row.txt, last line VERDICT PASS|FAIL|VOID
set -u; D=$(cd "$(dirname "$0")" && pwd); . $D/kit.sh; L=$D/logs; O=$L/row.txt; : > $O
POLL=${POLL:-1}; GWAIT=${GWAIT:-300}; WWAIT=${WWAIT:-120}; YESAFTER=${YESAFTER:-2}; YESWAIT=${YESWAIT:-180}; SETTLE=${SETTLE:-15}; RUNAFTER=${RUNAFTER:-12}
o(){ echo "$*" | tee -a $O; }
S=$SKILL
# patterns (lampOS/crates/tasks/src/skill_task.rs, lampOS/crates/skills/src/main.rs at #8237's head)
TOLD_T="skill task: the agent's start-up wait passed 5 s: told getting ready"
TOLD_S="skill:$S: do.task: told the lamp is getting ready: "
ANS="skill:$S: do.task: lamp-tasks' answer carries "
ASKED="skill:$S: do.task: not handed over: waiting for the owner's yes (the lamp says it back and asks); asked aloud: say: spoken"
KEPT="lamp-control: the owner's yes to $S's task kept"   # lampOS/crates/control/src/task_yes.rs
RESEND="do.task: lamp-tasks is from before the getting-ready notice: handed over again without it"
NOTSENT="the hand-over is not sent"
c(){ grep -cF -- "$1" "$2" 2>/dev/null | tr -d ' '; }
waitfor(){ local pat=$1 f=$2 lim=$3 s=0; while [ $s -lt $lim ]; do grep -qF -- "$pat" $f 2>/dev/null && return 0
  grep -qF -- "$NOTSENT" $f 2>/dev/null && return 1; sleep $POLL; s=$((s + POLL)); done; return 1; }
jstart(){ : > $L/$1.journal; JP=$(JF $L/$1.journal); sleep 2; }
jstop(){ kill $JP 2>/dev/null; wait $JP 2>/dev/null; }
leg(){ o "LEG $1 $2${3:+ ($3)}"; echo "$1=$2" >> $L/verdicts.env; }
: > $L/verdicts.env
for x in ${@:-P G A W}; do RENEW || { o "VERDICT VOID (lock lost before leg $x)"; exit 1; }; LEG=$x; export LEG
case $x in
P) o "== P: the fixture's yes, asked once with a warm gateway"
   # The yes is played only once the lamp has asked for it (said the task back): on a lamp whose kept session
   # opens cold after put, that ask comes a minute or more after RUN, and a yes played before it is not kept.
   jstart p; PLAY RUN
   if ! waitfor "$ASKED" $L/p.journal $YESWAIT; then jstop; o "CHECK (P0) the lamp never asked for the yes within ${YESWAIT} s (logs/p.journal)"; leg P VOID "no ask for the yes: G and W can't run"; continue; fi
   o "CHECK (P0) the lamp asked for the yes: $(grep -F -- "$ASKED" $L/p.journal | tail -1 | cut -d: -f4- | cut -c1-90)"
   sleep $YESAFTER; PLAY YES
   # A yes kept runs the task at its next trigger, never this one (skills task.rs "On the yes, the task runs at its
   # next trigger"; voice skill_task_yes.rs APPROVED_NEXT): RUN is asked again once lamp-control kept the yes.
   if ! waitfor "$KEPT" $L/p.journal $WWAIT; then jstop; o "CHECK (P1) lamp-control kept no yes within ${WWAIT} s (logs/p.journal)"; leg P VOID "the yes was not kept: G and W can't run"; continue; fi
   o "CHECK (P1) lamp-control kept the owner's yes"
   sleep $RUNAFTER; PLAY RUN
   if waitfor "$ANS" $L/p.journal $GWAIT; then sleep $SETTLE; jstop; o "CHECK (P2) a run handed over and answered: $(grep -F -- "$ANS" $L/p.journal | tail -1 | cut -d: -f4- | cut -c1-90)"; leg P PASS
   else jstop; o "CHECK (P2) no answer from lamp-tasks within ${GWAIT} s (logs/p.journal)"; leg P VOID "no answer after the yes: G and W can't run"; fi;;
G) o "== G: cold gateway: the line once, before the answer"
   grep -q '^P=PASS' $L/verdicts.env || { leg G VOID "no P PASS"; continue; }
   GATE lamp-openclaw-gateway || { leg G VOID "start budget"; continue; }
   jstart g; LAMP "SECS=$((GWAIT + 40))" $D/reads.lamp.sh > $L/g.reads 2>&1 & RP=$!
   LAMP "" $D/gw.lamp.sh | tee $L/gw.txt; grep -q '^GW RESTARTED' $L/gw.txt || { jstop; kill $RP 2>/dev/null; leg G VOID "gateway not restarted"; continue; }
   sleep ${ASKAFTER:-10}; PLAY RUN
   got=0; waitfor "$ANS" $L/g.journal $GWAIT && got=1; sleep $SETTLE; jstop; kill $RP 2>/dev/null; wait $RP 2>/dev/null
   nt=$(c "$TOLD_T" $L/g.journal); ns=$(c "$TOLD_S" $L/g.journal); sp=$(grep -F -- "$TOLD_S" $L/g.journal | grep -c "${TOLD_S}say: spoken" | tr -d ' ')
   out=$(grep -F -- "$TOLD_S" $L/g.journal | sed "s/.*getting ready: //" | tr '\n' ';')
   o "CHECK (G1) lamp-tasks told getting ready once: $nt line(s)"
   o "CHECK (G2) lamp-skills asked the line once and it was spoken: $ns ask(s), outcome: ${out:-none}"
   o "CHECK (G3) the run's answer was read: $([ $got = 1 ] && grep -F -- "$ANS" $L/g.journal | tail -1 | sed 's/.*answer carries //' || echo none)"
   o "CHECK (G4) spoken means it began before the answer was read (taken back otherwise, main.rs hand_over_saying_getting_ready): $([ "$sp" = 1 ] && echo yes || echo no)"
   o "CHECK (G5) no re-send without the field (both parts #8237's): $(c "$RESEND" $L/g.journal) line(s)"
   o "ROOM (G) by ear, the runner notes: the line came after the lamp's own reply to the ask and never over it; nothing was said after the answer"
   if grep -qF -- "$NOTSENT" $L/g.journal; then leg G VOID "the gateway was not ready within the run's deadline: $(grep -F -- "$NOTSENT" $L/g.journal | tail -1 | cut -c1-120)"
   elif [ $got = 1 ] && [ "$nt" = 1 ] && [ "$ns" = 1 ] && [ "$sp" = 1 ] && [ "$(c "$RESEND" $L/g.journal)" = 0 ]; then leg G PASS
   elif [ $got = 1 ] && [ "$nt" = 0 ] && [ "$ns" = 0 ]; then leg G VOID "the gateway served within 5 s of the ask: no cold start"
   else leg G FAIL; fi;;
A) o "== A: one approvals read in flight, with back-off (sampled during G, logs/g.reads)"
   [ -s $L/g.reads ] || { leg A VOID "no G samples"; continue; }
   python3 -I - $L/g.reads <<'PY' | tee -a $O
import sys
seen, mx = {}, 0
for l in open(sys.argv[1]):
    f = l.split()
    if len(f) != 3 or f[0] != "READS": continue
    t = float(f[1]); ps = [] if f[2] == "-" else f[2].split(",")
    mx = max(mx, len(ps))
    for p in ps: seen.setdefault(p, [t, t])[1] = t
r = sorted(seen.values())
gaps = [round(b[0] - a[1], 1) for a, b in zip(r, r[1:])]
print("CHECK (A1) reads in flight at once, at most: %d (samples every 0.5 s)" % mx)
print("CHECK (A2) reads seen: %d, gaps between them (s): %s" % (len(r), gaps or "-"))
ok_gaps = all(g >= 2.4 for g in gaps) and all(g <= 16.5 for g in gaps)
grow = all(b >= a - 1.0 or b >= 14.0 for a, b in zip(gaps, gaps[1:]))
print("CHECK (A3) back-off: every gap 3-15 s (+-0.6 s sampling) and not shrinking: %s" % ("yes" if ok_gaps and grow else "no"))
if not r: print("LEG A VOID (no approvals read seen during G)")
elif mx > 1: print("LEG A FAIL")
elif len(r) == 1: print("LEG A PASS (one read only: the back-off was not needed)")
elif ok_gaps and grow: print("LEG A PASS")
else: print("LEG A FAIL")
PY
   v=$(grep '^LEG A ' $O | tail -1 | awk '{print $3}'); echo "A=$v" >> $L/verdicts.env;;
W) o "== W: warm gateway: nothing said"
   grep -q '^P=PASS' $L/verdicts.env || { leg W VOID "no P PASS"; continue; }
   jstart w; PLAY RUN; got=0; waitfor "$ANS" $L/w.journal $WWAIT && got=1; sleep $SETTLE; jstop
   nt=$(c "$TOLD_T" $L/w.journal); ns=$(c "$TOLD_S" $L/w.journal)
   o "CHECK (W1) lamp-tasks told nothing: $nt line(s)"; o "CHECK (W2) lamp-skills asked no line: $ns ask(s)"
   o "CHECK (W3) the run's answer was read: $([ $got = 1 ] && echo yes || echo no)"
   if [ $got = 0 ]; then leg W VOID "no answer within ${WWAIT} s"; elif [ "$nt" = 0 ] && [ "$ns" = 0 ]; then leg W PASS; else leg W FAIL; fi;;
*) o "no leg $x"; exit 2;;
esac; done
if grep -q '=FAIL' $L/verdicts.env; then o "VERDICT FAIL $(tr '\n' ' ' < $L/verdicts.env)"
elif grep -q '=VOID' $L/verdicts.env || [ ! -s $L/verdicts.env ]; then o "VERDICT VOID $(tr '\n' ' ' < $L/verdicts.env)"
else o "VERDICT PASS $(tr '\n' ' ' < $L/verdicts.env)"; fi
