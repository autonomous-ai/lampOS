#!/usr/bin/env bash
# row-cutin-words.sh's canned DRY matrix (plan-cutin-words.md). DRY only: no lamp, no audio, no post. One row per case:
# PASS when the expected line is in the run's log (or stderr) and no Python traceback.
cd "$(dirname "$0")/.."; C=$PWD/canned-cutin M=${M:-${TMPDIR:-/tmp}/dry-cutin-m}; rm -rf "$M"; mkdir -p "$M"
CS=$M/cases; "$C/make.sh" "$CS" # the canned events, built fresh outside the checkout
export DRY_CASE=$CS/pass WAVS=${WAVS:-$M/no-clips} # DRY needs no clips: a fresh checkout has none
export PATH=$C/ghq:$PATH # the other-lamp gate's gh: a stub, #3336 quiet unless STUB_PLAYING names a lamp
export L="dry orangepi@lamp-52e6.invalid" SEQ=9063 HARD_END=23:59 NO_PERSON=yes DRY=1
SOON=$(date -v+16M +%H:%M) # the hard-end cases: under NEED_MIN (44) left, and the DRY clock runs out in arm W
one() { # one NAME 'ENV...' REGEX: run the kit with ENV, show the first line matching REGEX
  local name=$1 envs=$2 re=$3 seen tb
  eval "env $envs OUT=\"$M/$name\" ./row-cutin-words.sh" >"$M/$name.out" 2>"$M/$name.err"; echo $? >"$M/$name.rc"
  seen=$(grep -E -h -m1 -- "$re" "$M/$name/log.txt" "$M/$name.err" "$M/$name.out" 2>/dev/null | head -1)
  tb=$(cat "$M/$name.err" "$M/$name/log.txt" 2>/dev/null | grep -c Traceback)
  printf '%s\t%s\t%s\t%s\n' "$([[ -n $seen && $tb == 0 ]] && echo PASS || echo "FAIL(tb=$tb)")" "$name" "${envs:-(default)}" "${seen:-<not seen>}"; }
# Verdicts
one v-pass      "DRY_CASE=$CS/pass"      'VERDICT: PASS \(W: cut-ins 4/4, 0 wordless commits against P.s 3'
one v-norepro   "DRY_CASE=$CS/norepro"   'VERDICT: NO REPRO'
one v-wordless  "DRY_CASE=$CS/wordless"  'VERDICT: FAIL \(W made 3 wordless commits\)'
one v-missed    "DRY_CASE=$CS/missed"    'VERDICT: FAIL \(a real cut-in on W did not commit: resume'
one v-slowyield "DRY_CASE=$CS/slowyield" 'VERDICT: FAIL \(time to yield changed: median W 700 ms against P 400 ms'
one v-void      "DRY_CASE=$CS/void"      'VERDICT: VOID \(valid tries: P C 4, P O 3, W C 0, W O 3'
one v-five      "C_TRIALS=5 O_TRIALS=5" 'VERDICT: PASS \(W: cut-ins 5/5, 0 wordless commits against P.s 5' # the first builder's 5 per arm
one v-noanswer  "DRY_CASE=$CS/pass DRY_NOANSWER=WC" 'VERDICT: VOID \(valid tries: P C 4, P O 3, W C 0'
# Gates
one g-person    "NO_PERSON=" 'would stop here: manual check: read the Mac camera'
one g-switched  "DRY_SWITCHED='switched to build B 9070'" 'would stop here: lamp-update switched in the last 3 min'
one g-foreign   "DRY_FOREIGN=zz-cutin-check.conf" 'would stop here: another /run drop-in on lamp-voice: zz-cutin-check.conf'
one g-hardend   "HARD_END=$SOON" 'would stop here: the hard end .* leaves [0-9]+ min, under the 44 min'
one g-hardstop  "HARD_END=$SOON" 'stopped at the hard end .* before W'
one g-seqretry  "DRY_SEQFAIL=1" 'PC 1: the seq read came back empty, retrying once'
one g-seq2fail  "DRY_SEQFAIL=2" 'would stop here: the lamp left 9063 \(now \) before PC 1'
one g-seqmoved  "DRY_SEQ=9070" 'would stop here: the lamp left 9063 \(now 9070\) before PC 1'
one g-exe       "DRY_EXE=/opt/lampOS/sixth-x/lamp-voice" 'would stop here: arm P: lamp-voice runs /opt/lampOS/sixth-x/lamp-voice, not the installed'
one g-envP      "ARMS=P DRY_ENVIRON='1 0'" 'would stop here: arm P: lamp-voice has LAMP_STOP_WORDS or LAMP_BACKCHANNEL_HOLDS'
one g-envW      "ARMS=W DRY_ENVIRON='0 1'" 'would stop here: arm W: LAMP_STOP_WORDS=heard and LAMP_BACKCHANNEL_HOLDS=on are not both'
one g-startup   "ARMS=W DRY_STARTUP='lamp-voice: LAMP_BACKCHANNEL_HOLDS is set, but backchannels are held only with LAMP_STOP_WORDS=heard or heard-always: not held'" "would stop here: arm W: no 'stop words: heard, in a yield' line"
one g-startup2  "ARMS=W DRY_STARTUP='lamp-voice: stop words: heard, in a yield, from the processed blocks'" "would stop here: arm W: no 'backchannel holds' line"
one g-hello     "DRY_HELLO=fail" 'would stop here: arm P: hello not heard'
one g-stray     "DRY_STRAY=99999" 'FAIL a stray afplay before PC 1 \(pid 99999\)'
one g-stray-q   "DRY_STRAY=99999" 'QUIET: .*stopped early \(a stray afplay before PC 1'
one g-putback   "DRY_PUTBACK='PUT-BACK FAILED: drop-in-left'" 'QUIET: .*Put-back: FAILED \(PUT-BACK FAILED: drop-in-left\)'
one g-putrc     "DRY_PUTBACK='PUT-BACK FAILED: drop-in-left'" '^PUT-BACK FAILED: '
one g-other     "STUB_PLAYING=4ace" 'FAIL another lamp is PLAYING on #3336 \(or #3336 unreadable\) before the start'
one g-other-q   "STUB_PLAYING=4ace" 'QUIET: .*stopped early|QUIET: .*aborted, nothing ran.*Put-back'
one g-self      "STUB_PLAYING=52e6" 'VERDICT: PASS' # the kit's own lamp PLAYING is not another lamp
one g-other-arm "STUB_PLAYING=4ace STUB_AFTER=1 STUB_COUNT=$M/g-other-arm.calls" 'FAIL another lamp is PLAYING on #3336 \(or #3336 unreadable\) before arm P'
# TERM mid-try: real naps, TERM once a try's ask has started, time to the QUIET line
( OUT=$M/term DRY_REALNAP=1 ./row-cutin-words.sh >"$M/term.out" 2>"$M/term.err" ) & k=$!
for i in $(seq 2400); do ls "$M"/term/scenarios/dry-*-ask.txt >/dev/null 2>&1 && break; sleep 0.1; done; sleep 2
kp=$(pgrep -f "row-cutin-words.sh" | while read p; do [[ $(ps -o ppid= -p $p | tr -d ' ') == $k ]] && echo $p; done | head -1)
t0=$(python3 -c 'import time; print(time.time())'); kill -TERM "${kp:-$k}"
for i in $(seq 100); do grep -q 'QUIET:' "$M/term/log.txt" 2>/dev/null && break; sleep 0.05; done
dt=$(python3 -c "import time; print(round(time.time()-$t0,2))"); wait $k 2>/dev/null
q=$(grep -m1 'QUIET:' "$M/term/log.txt" | cut -c1-150); p=$(grep -c 'sixth-cutin-putback-now' "$M/term/log.txt")
printf '%s\t%s\t%s\t%s\n' "$([[ -n $q && $p -gt 0 ]] && python3 -c "import sys; sys.exit(0 if $dt<=2 else 1)" && echo PASS || echo FAIL)" term-mid-try "DRY_REALNAP=1 + kill -TERM" "TERM->QUIET ${dt}s; systemd-run put-back lines $p; $q"
# lamp-lock's stop: TERM to the kit's process group, KILL ~1 s later; the QUIET still posted (stubbed gh) by the detached poster
set -m; mkdir -p "$M/kill"
( OUT=$M/kill DRY_REALNAP=1 DRY_GH=$C/gh-stub.sh GH_STUB_OUT=$M/kill/gh.txt exec ./row-cutin-words.sh >"$M/kill.out" 2>"$M/kill.err" ) & k=$!; set +m
for i in $(seq 2400); do ls "$M"/kill/scenarios/dry-*-ask.txt >/dev/null 2>&1 && break; sleep 0.1; done; sleep 2
t0=$(python3 -c 'import time; print(time.time())'); kill -TERM -"$k"; sleep 1; kill -KILL -"$k" 2>/dev/null; wait $k 2>/dev/null
for i in $(seq 400); do [[ -s $M/kill/gh.txt ]] && break; sleep 0.05; done
dt=$(python3 -c "import time; print(round(time.time()-$t0,2))"); g=$(cut -c1-200 "$M/kill/gh.txt" 2>/dev/null)
printf '%s\t%s\t%s\t%s\n' "$([[ $g == *"issue comment 3336"*QUIET*"stopped by TERM"* ]] && echo PASS || echo FAIL)" term-then-kill "TERM to the group, KILL 1 s later, DRY_GH=stub" "TERM->gh ${dt}s; gh: ${g:-<none>}"
