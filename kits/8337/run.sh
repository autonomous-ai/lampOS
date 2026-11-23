#!/bin/bash
# Mac. #8337 ROW on one lamp (settings: row/kit.sh; the runner's env file: KIT_ENV). #8337 changes lamp-update, so the
# ROW comes from a real update round (First's rule of 19 Nov): stable -> round 1 to the CI build of the PR's head
# (ROUND1_SEQ) -> verify -> leg E, the agents list the app is shown (update/status's engines) before and after ->
# round 2 to stable's bytes at a higher sequence (ROUND2_SEQ) -> the put-back -> two reads 60 s apart.
# From the #8162 kit (kits/8162-e82d), with its voice legs, the mute and Harness's step-down left out: #8337 pins
# Harness 0.6.75, as stable does, and plays nothing.
#   ./run.sh [from-step [then-step]]   steps: lock base engines0 round1 verify engines round2 putback read release
#   UNTIL=<step> stops before that step; SKIP="<step> ..." leaves steps out; with then-step, the first step runs and
#   the run goes on at then-step.
# The rounds: the kit holds no key and signs nothing. Either the runner's own commands (ROUND1_CMD, ROUND2_CMD: they get
# ROUND_N, ROUND_SEQ, LAMP_HOST, HEAD_SHA) make the lamp's own lamp-update take each round; or the runner gives two
# channel folders they made and signed (CH1, CH2) and the public key (TEST_PUB), and the kit points lamp-update's own
# product line at them by /run drop-ins (removed at putback). Either way the kit then waits for lamp-update's own end.
# No volume is ever set (the lamp's speaker is read at base: over 40% stops); lamp-cloud is never stopped; no factory
# reset; GATE keeps each unit at 10 or fewer starts in any 10 min. The lock is released only after PUTBACK OK and
# READ TWICE same.
set -u; K=$(cd "$(dirname "$0")" && pwd); D=$K/row; . $D/kit.sh
STEPS="lock base engines0 round1 verify engines round2 putback read release"
FROM=${1:-lock}; THEN=${2:-}
for x in $FROM $THEN; do case " $STEPS " in *" $x "*) ;; *) echo "no step '$x' (steps: $STEPS)"; exit 2;; esac; done
ROUNDWAIT=${ROUNDWAIT:-900}; ENGWAIT=${ENGWAIT:-900}; ROUNDPOLL=${ROUNDPOLL:-20}; VERIFYWAIT=${VERIFYWAIT:-900}; VERIFYPOLL=${VERIFYPOLL:-30}; READGAP=${READGAP:-60}
T(){ echo "$(date '+%T') $*" | tee -a $L/times.log; }
STOP(){ T "STOP at $st: $*"; exit 3; }
need(){ grep -qE "$2" $L/$1.txt 2>/dev/null || STOP "no '$2' (read row/logs/$1.txt)"; }
MODE_R(){ if [ -n "${CH1:-}" ]; then echo channel; elif [ -n "${ROUND1_CMD:-}${ROUND2_CMD:-}" ]; then echo cmd; else echo none; fi; }
# A resumed run after the trap put the mute back: the clips can't be heard until micon runs again.
GUARDS(){ trap - EXIT; trap 'T "guard cut by a signal: check the stopped pid, the wrapper and the mute by hand"; exit 130' INT TERM
  STOPGUARD; WRAPGUARD; HOLDGUARD; MICGUARD; }
trap GUARDS EXIT; trap 'exit 130' INT TERM
# ROUND n seq cmd: one real round of the lamp's own lamp-update, then the wait for its end (RENEW on every look).
ROUND(){ local n=$1 seq=$2 cmd=$3 m f s0 ok=0 J
  m=$(MODE_R); [ $m != none ] || STOP "no round: set ROUND${n}_CMD (your own command) or CH1, CH2 and TEST_PUB (your signed channels)"
  [ -n "$seq" ] || STOP "ROUND${n}_SEQ is not set (the sequence round $n installs)"
  GATE 3 lamp-update > $L/round$n-gate.txt 2>&1 && GATE 2 lamp-voice lamp-tasks lamp-skills lamp-control lamp-openclaw-gateway >> $L/round$n-gate.txt 2>&1 || STOP "start budget: $(cat $L/round$n-gate.txt)"
  LAMP "MODE=mark N=$n" $D/round.lamp.sh > $L/round$n.txt 2>&1; cat $L/round$n.txt; grep -q '^MARK ' $L/round$n.txt || STOP "the journal's cursor not kept"
  if [ $m = channel ]; then
    if [ $n = 1 ]; then [ -d "${CH1:-}" ] && [ -d "${CH2:-}" ] && [ -f "${TEST_PUB:-}" ] || STOP "channel mode needs CH1, CH2 (folders) and TEST_PUB (a file)"
      for c in "$CH1" "$CH2"; do [ -f "$c/lampos-manifest.json" ] || STOP "$c has no lampos-manifest.json (a signed channel)"; done
      f=$(mktemp -d "${TMPDIR:-/tmp}/kit8337-ch.XXXXXX"); ln -s "$CH1" $f/ch1; ln -s "$CH2" $f/ch2; cp "$TEST_PUB" $f/test.pub
      ( cd $f && find -L test.pub ch1 ch2 -type f | LC_ALL=C sort | xargs shasum -a 256 > SHAS )
      $SSH -n $H 'rm -rf /tmp/t8337 && mkdir -p /tmp/t8337' && $SCP -q -r $f/ch1 $f/ch2 $f/test.pub $f/SHAS $H:/tmp/t8337/ || { rm -rf $f; STOP "the copy failed"; }
      rm -rf $f; LAMP "" $D/stage.lamp.sh > $L/stage.txt 2>&1; cat $L/stage.txt; need stage '^STAGED OK'; fi
    LAMP "MODE=go N=$n" $D/round.lamp.sh >> $L/round$n.txt 2>&1; grep -q "^GO round $n" $L/round$n.txt || STOP "round $n not started ($(tail -1 $L/round$n.txt))"
  else
    [ -n "$cmd" ] || STOP "ROUND${n}_CMD is not set"
    ROUND_N=$n ROUND_SEQ=$seq LAMP_HOST=$LAMP_HOST HEAD_SHA=$HEAD_SHA bash -c "$cmd" >> $L/round$n.txt 2>&1 || STOP "ROUND${n}_CMD failed (row/logs/round$n.txt)"
  fi
  T "round $n to build $seq asked: waiting (at most $ROUNDWAIT s) for lamp-update's own end"
  s0=$(date +%s)
  while [ $(( $(date +%s) - s0 )) -lt $ROUNDWAIT ]; do RENEW || exit 1
    LAMP "MODE=read N=$n" $D/round.lamp.sh > $L/round$n-read.txt 2>&1; J=$(grep '^J ' $L/round$n-read.txt)
    echo "$J" | grep -qE "refused, nothing switched|not confirmed: |build $seq put back|not switched: " && break
    if grep -q "^SEQ installed=$seq busy=''" $L/round$n-read.txt && echo "$J" | grep -qE "switched to build $seq; every part is well|up to date: build $seq"; then
      if echo "$J" | grep -qE "$P_U_SELF"; then echo "$J" | grep -qE "$P_U_CONFIRMED" && echo "$J" | sed -n "/$P_U_CONFIRMED/,\$p" | grep -q "up to date: build $seq" && { ok=1; break; }
      else ok=1; break; fi; fi
    sleep $ROUNDPOLL; done
  cat $L/round$n-read.txt >> $L/round$n.txt; grep -E "^J " $L/round$n-read.txt | cut -c1-200
  [ $ok = 1 ] || STOP "round $n did not end on build $seq (an end line, or not within $ROUNDWAIT s: $(grep -E 'refused|not confirmed|put back|not switched' $L/round$n-read.txt | tail -1 | cut -c1-160))"
  echo "ROUND $n OK build $seq ($(grep '^UPDATER' $L/round$n-read.txt))" | tee -a $L/round$n.txt; }
. $D/patterns.env
go=0
for st in $STEPS; do
  [ $st = $FROM ] && go=1; [ -n "$THEN" ] && [ $st = $THEN ] && go=1; [ $go = 1 ] || continue
  [ -n "${UNTIL:-}" ] && [ $st = "$UNTIL" ] && { T "UNTIL $st: stop before it"; exit 0; }
  case " ${SKIP:-} " in *" $st "*) T "SKIP $st"; continue;; esac
  if [ $st != lock ]; then RENEW || exit 1; LOCKLOST && exit 1; fi
  T "step $st"
  case $st in
  lock)    VOLOK || exit 2
           if [ -z "$LL" ]; then echo "LOCK none (KIT_LL unset): no lock taken" | tee $L/lock.txt
           else $LL $LOCK_LAMP try $LOCK_WHO $LOCK_SESSION "#8337 ROW" > $L/lock.txt 2>&1; rc=$?
             [ $rc = 0 ] || { T "LOCK NOT TAKEN (try exit $rc): no ssh. $(tail -2 $L/lock.txt)"; exit 1; }
             rm -f $L/RELEASED $L/renew.log
             ( while [ ! -f $L/RELEASED ]; do $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 && echo "$(date +%T) renewed" >> $L/renew.log || echo "$(date +%T) RENEW FAILED" >> $L/renew.log
                 sleep ${RENEWEVERY:-60}; done ) >/dev/null 2>&1 &
             echo $! > $L/renew.pid; echo "LOCK TAKEN $(date +%T); renew loop pid $(cat $L/renew.pid)" | tee -a $L/lock.txt; fi;;
  base)    LAMP "STABLE_SEQ=$STABLE_SEQ STABLE_UPD_SHA=$STABLE_UPD_SHA HV_STABLE=$HV_STABLE SPK_MAX=$SPK_MAX${SPK_CTL:+ SPK_CTL='$SPK_CTL'}" $D/base.lamp.sh > $L/base.txt 2>&1; cat $L/base.txt
           # The lamp's speaker volume, read only (never set): at most SPK_MAX %, else stop and ask.
           sp=$(sed -n 's/^SPEAKER [^ ]* \([0-9]*\)%$/\1/p' $L/base.txt)
           [ -n "$sp" ] && [ "$sp" -le "$SPK_MAX" ] || STOP "the lamp's speaker volume is ${sp:-unread}% (at most $SPK_MAX%): stop and ask; the kit never sets it"
           # Before any change: OpenClaw the default agent. Harness's sign-in
           # is a fact line only (T HAUTH): the lamp's own OpenClaw runs while Harness is signed out (#6407).
           grep -q '^ENGINE openclaw$' $L/base.txt || STOP "BASE FAIL OpenClaw is not the default agent ($(grep '^ENGINE' $L/base.txt))"
           need base '^BASE OK'
           grep '^EXESHA ' $L/base.txt > $L/exesha0.txt || STOP "no EXESHA line: the units' running exes not recorded"
           mx=$(sed -n "s/^SEQ installed=[0-9]* max=\([0-9]*\).*/\1/p" $L/base.txt)
           [ -n "${ROUND1_SEQ:-}" ] && [ "$ROUND1_SEQ" -gt "${mx:-0}" ] 2>/dev/null || STOP "ROUND1_SEQ '${ROUND1_SEQ:-}' is not above the highest sequence the lamp has seen (${mx:-unread}): a round takes only a higher one"
           [ -n "${ROUND2_SEQ:-}" ] && [ "$ROUND2_SEQ" -gt "$ROUND1_SEQ" ] 2>/dev/null || STOP "ROUND2_SEQ '${ROUND2_SEQ:-}' is not above ROUND1_SEQ $ROUND1_SEQ (a corrective build needs a higher sequence)"
           sed -n 's/^FAILED0 //p' $L/base.txt > $L/failed0.txt
           T "base: the lamp has seen up to ${mx}; round 1 -> $ROUND1_SEQ, round 2 -> $ROUND2_SEQ ($(MODE_R) mode)";;
  round1)  grep -q '^BASE OK' $L/base.txt 2>/dev/null || STOP "no BASE OK (./run.sh base)"
           ROUND 1 "${ROUND1_SEQ:-}" "${ROUND1_CMD:-}";;
  verify)  # The PR build, its updater (a self-switch), Harness at the PR's pin (0.6.75, as stable),
           # round), nothing newly failed, tasks/agent ready with engine openclaw. Looked at every VERIFYPOLL s.
           s0=$(date +%s); rm -f $L/verify.txt
           while :; do RENEW || exit 1
             LAMP "SEQ1=${ROUND1_SEQ:-} HV_PR=$HV_PR STABLE_UPD_SHA=$STABLE_UPD_SHA PR_UPDATE_SHA=${PR_UPDATE_SHA:-} PR_TASKS_SHA=${PR_TASKS_SHA:-} FAILED0='$(cat $L/failed0.txt 2>/dev/null)'" $D/verify.lamp.sh > $L/verify-look.txt 2>&1
             grep -q '^VERIFY PASS' $L/verify-look.txt && break
             [ $(( $(date +%s) - s0 )) -ge $VERIFYWAIT ] && break; sleep $VERIFYPOLL; done
           cp $L/verify-look.txt $L/verify.txt; cat $L/verify.txt; need verify '^VERIFY PASS';;
  engines0) # The app's agents list on stable, before any change (the leg's "before").
           LAMP "N=0" $D/engines.lamp.sh > $L/engines0.txt 2>&1; cat $L/engines0.txt
           grep -q '^ENGINES0 {' $L/engines0.txt || T "engines0: stable's list not read ($(grep '^ENGINES0' $L/engines0.txt)): leg E can only be VOID";;
  engines) # #8337's leg E on the PR build: wait (at most ENGWAIT s) for lamp-update's first Harness look since round 1,
           # then read the list the app is shown. PASS: read; Hermes and OpenClaw as stable said them; no engine stable
           # said `installed` is now `not_installed` (#8286's harm); with Harness's look answered, its engines listed.
           s0=$(date +%s)
           while :; do RENEW || exit 1
             LAMP "N=1" $D/engines.lamp.sh > $L/engines1.txt 2>&1
             # A round's own OpenClaw (or Hermes) step reinstalls it on stable as on the PR build (lamp-ee17, 23 Nov:
             # "OpenClaw 2026.11.2 is not set up ... started to put it there" after each round): judge only once both
             # are settled (not installing, frozen or waiting) and the Harness look has spoken.
             grep -qE '^HLOOK1 (said|not)' $L/engines1.txt && ! grep -qE '"(hermes|openclaw)":\{[^}]*"state":"(installing|frozen|waiting)"' $L/engines1.txt && break
             [ $(( $(date +%s) - s0 )) -ge $ENGWAIT ] && break; sleep $VERIFYPOLL; done
           cat $L/engines1.txt
           python3 -I - $L/engines0.txt $L/engines1.txt <<'P' | tee $L/engines.txt
import json, re, sys
def get(p, k):
    try: lines = open(p).read().splitlines()
    except OSError: return ""
    for l in lines:
        if l.startswith(k + " "): return l[len(k) + 1:].strip()
    return ""
e0, e1, look = get(sys.argv[1], "ENGINES0"), get(sys.argv[2], "ENGINES1"), get(sys.argv[2], "HLOOK1")
st = lambda e: {k: v.get("state") for k, v in e.items() if isinstance(v, dict)}
print("CHECK (E0) on stable the app was shown: %s" % (e0 or "-"))
print("CHECK (E1) on the PR build the app is shown: %s" % (e1 or "-"))
print("CHECK (E2) lamp-update's Harness look since round 1: %s" % (look or "-"))
if not e1.startswith("{"): print("LEG E VOID (the PR build's list not read)"); sys.exit()
busy = [k for k, v in json.loads(e1).items() if isinstance(v, dict) and v.get("state") in ("installing", "frozen", "waiting")]
if busy: print("LEG E VOID (%s still in its round's install step after ENGWAIT)" % ", ".join(sorted(busy))); sys.exit()
a = st(json.loads(e1)); b = st(json.loads(e0)) if e0.startswith("{") else {}
why = []
for k in ("hermes", "openclaw"):
    if k not in a: why.append("%s-not-listed" % k)
    elif b and a[k] != b.get(k): why.append("%s-%s-was-%s" % (k, a[k], b.get(k)))
lost = sorted(k for k, v in b.items() if v == "installed" and a.get(k) == "not_installed")
if lost: why.append("said-not-installed:" + ",".join(lost))
harness = sorted(k for k in a if k not in ("hermes", "openclaw", "default", "default_chosen_in_app"))
print("CHECK (E3) Hermes and OpenClaw as on stable, none installed said not_installed: %s" % ("yes" if not why else "no: " + " ".join(why)))
print("CHECK (E4) Harness's own engines listed: %s (%s)" % (", ".join("%s=%s" % (k, a[k]) for k in harness) or "none",
      "its look answered" if look == "said" else "its look did not answer: %s" % (look or "none seen")))
if why: print("LEG E FAIL")
elif look == "said" and not harness: print("LEG E FAIL (the look answered, yet no engine of Harness's is listed)")
else: print("LEG E PASS" + ("" if look == "said" else " (Harness's look did not answer: its engines keep the last look, #8286's case not reached)"))
P
           grep -q '^LEG E ' $L/engines.txt || STOP "no leg E verdict";;
  round2)  # Back to stable's bytes by a real round, from the PR build's own lamp-update, at a sequence above round 1's.
           for f in stopped.on wrap.on hold.on; do [ -f $L/$f ] && STOP "a leg's change is not put back ($f): ./run.sh micoff runs the guards"; done
           [ -n "${ROUND2_SEQ:-}" ] && [ "$ROUND2_SEQ" -gt "${ROUND1_SEQ:-0}" ] 2>/dev/null || STOP "ROUND2_SEQ must be above ROUND1_SEQ"
           ROUND 2 "$ROUND2_SEQ" "${ROUND2_CMD:-}"
           grep -q "^UPDATER running=$STABLE_UPD_SHA" $L/round2-read.txt || STOP "round 2 ended, but lamp-update is not stable's again ($(grep '^UPDATER' $L/round2-read.txt))";;
  putback) LAMP "" $D/nostop.lamp.sh > $L/nostop.txt 2>&1; cat $L/nostop.txt; need nostop '^NOSTOP OK'
           LAMP "" $D/l7back.lamp.sh > $L/l7back.txt 2>&1; need l7back '^L7BACK OK'
           pm=read; [ "$(MODE_R)" = channel ] && { pm=clean; GATE 1 lamp-update > $L/putback-gate.txt 2>&1 || STOP "start budget"; }
           LAMP "MODE=$pm SEQ2=${ROUND2_SEQ:-} STABLE_SEQ=$STABLE_SEQ STABLE_UPD_SHA=$STABLE_UPD_SHA" $D/putback.lamp.sh > $L/putback.txt 2>&1; cat $L/putback.txt
           need putback '^PUTBACK OK';;
  read)    V="SEQ2=${ROUND2_SEQ:-} STABLE_UPD_SHA=$STABLE_UPD_SHA HV=$HV_STABLE FAILED0='$(cat $L/failed0.txt 2>/dev/null)'"
           : > $L/read-diff.txt; LAMP "$V" $D/read.lamp.sh > $L/read1.txt 2>&1; sleep $READGAP; RENEW || exit 1; LAMP "$V" $D/read.lamp.sh > $L/read2.txt 2>&1
           cat $L/read2.txt
           # As found: lamp-update, lamp-tasks and lamp-voice run the exes base recorded (the same stable bytes back).
           ex=ok; for r in read1 read2; do for u in lamp-update lamp-tasks lamp-voice; do
             a=$(tr ' ' '\n' < $L/exesha0.txt | sed -n "s/^$u=//p"); b=$(grep '^EXESHA ' $L/$r.txt | tr ' ' '\n' | sed -n "s/^$u=//p")
             [ -n "$a" ] && [ "$a" != none ] && [ "$a" = "$b" ] || { ex=no; echo "EXESHA $r $u: ${b:-unread}, base had ${a:-none}" >> $L/read-diff.txt; }; done; done
           if [ $ex = ok ] && grep -q '^READ OK' $L/read1.txt && grep -q '^READ OK' $L/read2.txt && diff <(grep -v '^T ' $L/read1.txt) <(grep -v '^T ' $L/read2.txt) >> $L/read-diff.txt; then echo "READ TWICE same, READ OK, EXESHA as found at base" | tee $L/read.txt
           else { echo "READ TWICE: not both READ OK and the same"; cat $L/read-diff.txt; grep '^READ' $L/read1.txt $L/read2.txt; } | tee $L/read.txt; fi
           need read '^READ TWICE same';;
  release) if [ -z "$LL" ]; then echo "LOCK none to release"
           else ok=1; grep -q '^PUTBACK OK' $L/putback.txt 2>/dev/null || ok=0; grep -q '^READ TWICE same' $L/read.txt 2>/dev/null || ok=0
             for f in stopped.on wrap.on hold.on; do [ -f $L/$f ] && ok=0; done
             [ $ok = 1 ] || { T "no PUTBACK OK + READ TWICE same: keep the lock (FORCE=1 releases anyway)"; [ "${FORCE:-0}" = 1 ] || exit 1; }
             touch $L/RELEASED; [ -f $L/renew.pid ] && { kill $(cat $L/renew.pid) 2>/dev/null; rm -f $L/renew.pid; }
             $LL $LOCK_LAMP release $LOCK_WHO $LOCK_SESSION && echo "LOCK RELEASED $(date +%T)"; fi;;
  esac
  [ -n "$THEN" ] && [ $st = $FROM ] && [ $FROM != $THEN ] && { go=0; T "then: $THEN"; }
done
T "run.sh done: $(grep -h '^LEG E' $L/engines.txt 2>/dev/null | cut -c1-120)"
