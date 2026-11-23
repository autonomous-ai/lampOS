#!/bin/bash
# Mac. #8162 ROW on one lamp (settings: row/kit.sh; the runner's env file: KIT_ENV). The ROW comes from a real update
# round (the PR's lead reviewer: it changes lamp-update): stable (15543) -> round 1 to the CI build of the PR's head
# (ROUND1_SEQ) -> verify (Harness 0.6.78) -> the legs -> round 2 to stable's bytes at a higher sequence (ROUND2_SEQ)
# -> the put-back (Harness back to stable's 0.6.75) -> two reads 60 s apart -> the ROW comment.
#   ./run.sh [from-step [then-step]]   steps: lock clips hfetch base round1 verify micon legs micoff round2 putback hdown read report release
#   LEGS="L0 L1" picks the legs step's legs (default L0 .. L9); KEEP_VERDICTS=1 keeps the earlier pieces' verdicts.
#   UNTIL=<step> stops before that step; SKIP="<step> ..." leaves steps out; with then-step, the first step runs and
#   the run goes on at then-step.
# The rounds: the kit holds no key and signs nothing. Either the runner's own commands (ROUND1_CMD, ROUND2_CMD: they get
# ROUND_N, ROUND_SEQ, LAMP_HOST, HEAD_SHA) make the lamp's own lamp-update take each round; or the runner gives two
# channel folders they made and signed (CH1, CH2) and the public key (TEST_PUB), and the kit points lamp-update's own
# product line at them by /run drop-ins (removed at putback). Either way the kit then waits for lamp-update's own end.
# On ANY exit (a STOP, a FAIL, Ctrl-C) the EXIT trap, each once: SIGCONTs a pid leg L4 stopped, puts L7/L8's wrapper and
# config copies back, runs UNHOLD_CMD after L9's hold, and puts the recorded mute back. The lock is kept.
# The mute is lifted (micon) only with MIC_YES=yes: the lamp's person's yes for THIS ROW. No volume is ever set (the
# Mac's is read, <= 70); lamp-cloud is never stopped; no factory reset; GATE keeps each unit at 10 or fewer starts in
# any 10 min.
set -u; K=$(cd "$(dirname "$0")" && pwd); D=$K/row; . $D/kit.sh
STEPS="lock clips hfetch base round1 verify micon legs micoff round2 putback hdown read report release"
FROM=${1:-lock}; THEN=${2:-}
for x in $FROM $THEN; do case " $STEPS " in *" $x "*) ;; *) echo "no step '$x' (steps: $STEPS)"; exit 2;; esac; done
ALL_LEGS="L0 L1 L2 L3 L4 L5 L6 L7 L8 L9"
ROUNDWAIT=${ROUNDWAIT:-900}; ROUNDPOLL=${ROUNDPOLL:-20}; VERIFYWAIT=${VERIFYWAIT:-900}; VERIFYPOLL=${VERIFYPOLL:-30}; READGAP=${READGAP:-60}
HMETA=${HMETA:-$D/harness/cli-$HV_STABLE.metadata.json}; HF=$D/harness/$HV_STABLE
T(){ echo "$(date '+%T') $*" | tee -a $L/times.log; }
STOP(){ T "STOP at $st: $*"; exit 3; }
need(){ grep -qE "$2" $L/$1.txt 2>/dev/null || STOP "no '$2' (read row/logs/$1.txt)"; }
MODE_R(){ if [ -n "${CH1:-}" ]; then echo channel; elif [ -n "${ROUND1_CMD:-}${ROUND2_CMD:-}" ]; then echo cmd; else echo none; fi; }
# A resumed run after the trap put the mute back: the clips can't be heard until micon runs again.
HEARD(){ grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null && grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null && STOP "the mute was put back after micon (row/logs/micoff.txt): run ./run.sh micon first"; return 0; }
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
      f=$(mktemp -d "${TMPDIR:-/tmp}/kit8162-ch.XXXXXX"); ln -s "$CH1" $f/ch1; ln -s "$CH2" $f/ch2; cp "$TEST_PUB" $f/test.pub
      ( cd $f && find -L test.pub ch1 ch2 -type f | LC_ALL=C sort | xargs shasum -a 256 > SHAS )
      $SSH -n $H 'rm -rf /tmp/t8162 && mkdir -p /tmp/t8162' && $SCP -q -r $f/ch1 $f/ch2 $f/test.pub $f/SHAS $H:/tmp/t8162/ || { rm -rf $f; STOP "the copy failed"; }
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
           else $LL $LOCK_LAMP try $LOCK_WHO $LOCK_SESSION "#8162 ROW" > $L/lock.txt 2>&1; rc=$?
             [ $rc = 0 ] || { T "LOCK NOT TAKEN (try exit $rc): no ssh. $(tail -2 $L/lock.txt)"; exit 1; }
             rm -f $L/RELEASED $L/renew.log
             ( while [ ! -f $L/RELEASED ]; do $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 && echo "$(date +%T) renewed" >> $L/renew.log || echo "$(date +%T) RENEW FAILED" >> $L/renew.log
                 sleep ${RENEWEVERY:-60}; done ) >/dev/null 2>&1 &
             echo $! > $L/renew.pid; echo "LOCK TAKEN $(date +%T); renew loop pid $(cat $L/renew.pid)" | tee -a $L/lock.txt; fi;;
  clips)   ( cd $A && shasum -a 256 -c --quiet SHAS ) > $L/clips.txt 2>&1 && echo "CLIPS OK ($(wc -l < $A/SHAS | tr -d ' ') clips)" | tee -a $L/clips.txt; need clips '^CLIPS OK';;
  hfetch)  # Harness's stable pin (HV_STABLE) for hdown, fetched on the Mac from the URLs in stable's own pin file
           # (lampOS/deploy/harness/cli-<v>.metadata.json at stable's commit), each file's sha256 and size checked.
           mkdir -p $HF; python3 -I - $HMETA $HV_STABLE $HF <<'P' > $L/hfetch.txt 2>&1; cat $L/hfetch.txt; need hfetch '^HFETCH OK'
import hashlib, json, os, subprocess, sys
meta, v, out = sys.argv[1:4]; m = json.load(open(meta))["cli"]
if m["version"] != v: print("HFETCH STOP: %s is %s, not %s" % (meta, m["version"], v)); sys.exit(3)
env = []
for k, name in (("cli", "cli.js"), ("notify", "notify.mjs")):
    e = m[k]; p = os.path.join(out, name)
    good = lambda: os.path.exists(p) and os.path.getsize(p) == e["size"] and hashlib.sha256(open(p, "rb").read()).hexdigest() == e["sha256"]
    if not good():
        subprocess.run([os.environ.get("CURL", "curl"), "-fsS", "-o", p, e["url"]], check=False)
    if not good(): print("HFETCH STOP: %s's sha256 or size differs from %s" % (name, meta)); sys.exit(3)
    print("ok %s %d bytes %s" % (name, e["size"], e["sha256"][:16]))
    env.append("%s_SHA=%s %s_SIZE=%d" % (k.upper(), e["sha256"], k.upper(), e["size"]))
open(os.path.join(out, "meta.env"), "w").write("HV=%s %s\n" % (v, " ".join(env)))
print("HFETCH OK (Harness %s)" % v)
P
           ;;
  base)    LAMP "STABLE_SEQ=$STABLE_SEQ STABLE_UPD_SHA=$STABLE_UPD_SHA HV_STABLE=$HV_STABLE SPK_MAX=$SPK_MAX${SPK_CTL:+ SPK_CTL='$SPK_CTL'}" $D/base.lamp.sh > $L/base.txt 2>&1; cat $L/base.txt
           # The lamp's speaker volume, read only (never set): at most SPK_MAX %, else stop and ask.
           sp=$(sed -n 's/^SPEAKER [^ ]* \([0-9]*\)%$/\1/p' $L/base.txt)
           [ -n "$sp" ] && [ "$sp" -le "$SPK_MAX" ] || STOP "the lamp's speaker volume is ${sp:-unread}% (at most $SPK_MAX%): stop and ask; the kit never sets it"
           # Before any change: OpenClaw the default agent, the physical mic slide open (unread: noted). Harness's sign-in
           # is a fact line only (T HAUTH): the lamp's own OpenClaw runs while Harness is signed out (#6407).
           grep -q '^ENGINE openclaw$' $L/base.txt || STOP "BASE FAIL OpenClaw is not the default agent ($(grep '^ENGINE' $L/base.txt))"
           grep -q '^PRIVACY .*physical_muted=True' $L/base.txt && STOP "BASE FAIL the physical mic slide is closed (a hand only)"
           grep -q '^PRIVACY .*physical_muted=False' $L/base.txt || T "base: the physical slide not read apart from the mute: $(grep '^PRIVACY' $L/base.txt)"
           need base '^BASE OK'
           grep '^EXESHA ' $L/base.txt > $L/exesha0.txt || STOP "no EXESHA line: the units' running exes not recorded"
           mx=$(sed -n "s/^SEQ installed=[0-9]* max=\([0-9]*\).*/\1/p" $L/base.txt)
           [ -n "${ROUND1_SEQ:-}" ] && [ "$ROUND1_SEQ" -gt "${mx:-0}" ] 2>/dev/null || STOP "ROUND1_SEQ '${ROUND1_SEQ:-}' is not above the highest sequence the lamp has seen (${mx:-unread}): a round takes only a higher one"
           [ -n "${ROUND2_SEQ:-}" ] && [ "$ROUND2_SEQ" -gt "$ROUND1_SEQ" ] 2>/dev/null || STOP "ROUND2_SEQ '${ROUND2_SEQ:-}' is not above ROUND1_SEQ $ROUND1_SEQ (a corrective build needs a higher sequence)"
           sed -n 's/^FAILED0 //p' $L/base.txt > $L/failed0.txt
           T "base: the lamp has seen up to ${mx}; round 1 -> $ROUND1_SEQ, round 2 -> $ROUND2_SEQ ($(MODE_R) mode)";;
  round1)  grep -q '^BASE OK' $L/base.txt 2>/dev/null || STOP "no BASE OK (./run.sh base)"
           ROUND 1 "${ROUND1_SEQ:-}" "${ROUND1_CMD:-}";;
  verify)  # The PR build, its updater (a self-switch), Harness at the PR's pin (its move runs on the new updater's own
           # round), nothing newly failed, tasks/agent ready with engine openclaw. Looked at every VERIFYPOLL s.
           s0=$(date +%s); rm -f $L/verify.txt
           while :; do RENEW || exit 1
             LAMP "SEQ1=${ROUND1_SEQ:-} HV_PR=$HV_PR STABLE_UPD_SHA=$STABLE_UPD_SHA PR_UPDATE_SHA=${PR_UPDATE_SHA:-} PR_TASKS_SHA=${PR_TASKS_SHA:-} FAILED0='$(cat $L/failed0.txt 2>/dev/null)'" $D/verify.lamp.sh > $L/verify-look.txt 2>&1
             grep -q '^VERIFY PASS' $L/verify-look.txt && break
             [ $(( $(date +%s) - s0 )) -ge $VERIFYWAIT ] && break; sleep $VERIFYPOLL; done
           cp $L/verify-look.txt $L/verify.txt; cat $L/verify.txt; need verify '^VERIFY PASS';;
  micon)   [ "${MIC_YES:-}" = yes ] || STOP "the mute is lifted only on the lamp's person's yes for this ROW: set MIC_YES=yes"
           GATE 1 lamp-voice > $L/micon-gate.txt 2>&1 || STOP "start budget: $(cat $L/micon-gate.txt)"
           rm -f $L/micoff.txt
           LAMP "" $D/micon.lamp.sh > $L/micon.txt 2>&1; cat $L/micon.txt; sed "s/^/$(date +%T) micon /" $L/micon.txt >> $L/mic.txt
           need micon '^MUTE (LIFTED OK|NOT MUTED)';;
  legs)    VOLOK || exit 2; HEARD legs
           [ "${KEEP_VERDICTS:-}" = 1 ] || rm -f $L/verdicts.txt $L/verdicts.env
           bash $D/legs.sh ${LEGS:-$ALL_LEGS}; rc=$?
           T "legs: $(tail -1 $L/legs-$(echo ${LEGS:-$ALL_LEGS} | tr -d ' ').txt 2>/dev/null | cut -c1-120)"
           [ $rc = 3 ] && STOP "the agent is not back to ready after a fault leg: look at the lamp (row/logs/legs.log)"
           [ $rc = 2 ] && STOP "the Mac volume or a clip stopped the legs"
           RENEW || exit 1
           [ -f $L/legs-$(echo ${LEGS:-$ALL_LEGS} | tr -d ' ').txt ] || STOP "the legs ended with no verdict (exit $rc)";;
  micoff)  if grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null; then GATE 1 lamp-voice > $L/micoff-gate.txt 2>&1 || STOP "start budget: wait, then ./run.sh micoff"
             LAMP "" $D/micoff.lamp.sh > $L/micoff.txt 2>&1; cat $L/micoff.txt; sed "s/^/$(date +%T) micoff /" $L/micoff.txt >> $L/mic.txt
             need micoff '^MUTE BACK OK'
           else T "micoff: nothing lifted by the kit"; echo "MUTE BACK OK (nothing lifted)" > $L/micoff.txt; fi;;
  round2)  # Back to stable's bytes by a real round, from the PR build's own lamp-update, at a sequence above round 1's.
           grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null || [ ! -f $L/micon.txt ] || STOP "the mute is not back (./run.sh micoff)"
           for f in stopped.on wrap.on hold.on; do [ -f $L/$f ] && STOP "a leg's change is not put back ($f): ./run.sh micoff runs the guards"; done
           [ -n "${ROUND2_SEQ:-}" ] && [ "$ROUND2_SEQ" -gt "${ROUND1_SEQ:-0}" ] 2>/dev/null || STOP "ROUND2_SEQ must be above ROUND1_SEQ"
           ROUND 2 "$ROUND2_SEQ" "${ROUND2_CMD:-}"
           grep -q "^UPDATER running=$STABLE_UPD_SHA" $L/round2-read.txt || STOP "round 2 ended, but lamp-update is not stable's again ($(grep '^UPDATER' $L/round2-read.txt))";;
  putback) LAMP "" $D/nostop.lamp.sh > $L/nostop.txt 2>&1; cat $L/nostop.txt; need nostop '^NOSTOP OK'
           LAMP "" $D/l7back.lamp.sh > $L/l7back.txt 2>&1; need l7back '^L7BACK OK'
           pm=read; [ "$(MODE_R)" = channel ] && { pm=clean; GATE 1 lamp-update > $L/putback-gate.txt 2>&1 || STOP "start budget"; }
           LAMP "MODE=$pm SEQ2=${ROUND2_SEQ:-} STABLE_SEQ=$STABLE_SEQ STABLE_UPD_SHA=$STABLE_UPD_SHA" $D/putback.lamp.sh > $L/putback.txt 2>&1; cat $L/putback.txt
           need putback '^PUTBACK OK';;
  hdown)   # Harness back to stable's pin by hand (lamp-update never steps Harness down). 1 start (lamp-tasks).
           [ -s $HF/meta.env ] || STOP "no $HF/meta.env: ./run.sh hfetch"
           GATE 1 lamp-tasks > $L/hdown-gate.txt 2>&1 || STOP "start budget: $(cat $L/hdown-gate.txt)"
           $SSH -n $H 'rm -rf /tmp/t8162h && mkdir -p /tmp/t8162h' && $SCP -q $HF/cli.js $HF/notify.mjs $H:/tmp/t8162h/ || STOP "the copy failed"
           LAMP "$(cat $HF/meta.env)" $D/hdown.lamp.sh > $L/hdown.txt 2>&1; cat $L/hdown.txt; need hdown '^HDOWN OK';;
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
  report)  HEAD_SHA=$HEAD_SHA LAMP=$LAMP python3 -I $D/verdict.py $L --report "${ROUND1_SEQ:-?}" > $L/row-report.md; rc=$?; cat $L/row-report.md
           [ $rc = 0 ] || STOP "$(head -1 $L/row-report.md)"
           T "report: row/logs/row-report.md (post it yourself; the kit posts nothing)";;
  release) if [ -z "$LL" ]; then echo "LOCK none to release"
           else ok=1; grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null || ok=0; grep -q '^PUTBACK OK' $L/putback.txt 2>/dev/null || ok=0
             grep -q '^HDOWN OK' $L/hdown.txt 2>/dev/null || ok=0; grep -q '^READ TWICE same' $L/read.txt 2>/dev/null || ok=0
             for f in stopped.on wrap.on hold.on; do [ -f $L/$f ] && ok=0; done
             [ $ok = 1 ] || { T "no MUTE BACK OK + PUTBACK OK + HDOWN OK + READ TWICE same: keep the lock (FORCE=1 releases anyway)"; [ "${FORCE:-0}" = 1 ] || exit 1; }
             touch $L/RELEASED; [ -f $L/renew.pid ] && { kill $(cat $L/renew.pid) 2>/dev/null; rm -f $L/renew.pid; }
             $LL $LOCK_LAMP release $LOCK_WHO $LOCK_SESSION && echo "LOCK RELEASED $(date +%T)"; fi;;
  esac
  [ -n "$THEN" ] && [ $st = $FROM ] && [ $FROM != $THEN ] && { go=0; T "then: $THEN"; }
done
T "run.sh done: $(grep -h '^VERDICT' $L/legs-*.txt 2>/dev/null | cut -c1-80 | tr '\n' ';')"
