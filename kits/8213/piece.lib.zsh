# piece.lib.zsh: sourced by pieceA.zsh / pieceB.zsh (D and PIECE set first). The steps are the #8156 round kit's round.zsh,
# copied, as functions s_<step>; go <step> runs one (lock renewed first) and exits 3 on a STOP, so the piece's EXIT
# trap runs its put-back. Put-back steps (pb <step>) never exit: each runs once (its flag), a failure is logged.
set -u
source $D/mac.zsh
OUT=${KIT_OUT:-$D/out}; P=$OUT/plan.env
[ -f $P ] || { echo "no $P: run prep-local.zsh first (README, TODO)"; exit 1; }
source $P
: ${EXPECT_STABLE:?EXPECT_STABLE=<stable sequence the build was made on> (no default: README)}
[[ $STABLE_SEQ == $EXPECT_STABLE ]] || { echo "STOP: $P says STABLE_SEQ=$STABLE_SEQ, not EXPECT_STABLE=$EXPECT_STABLE: prep again"; exit 1; }
# The lamp's Harness before the round: the build's pin unless LAMP_HARNESS says the lamp keeps a newer one (a round never
# moves a newer Harness down; lamp-c0a4 kept 0.6.78 over a 0.6.75 pin).
BUILD_PIN=$HARNESS_WANT; HARNESS_WANT=${LAMP_HARNESS:-$HARNESS_WANT}
H8156=${HEAD8156:-9abee5c4ef13ff5439a459bfe8f51bb1182061e5}; H8213=${HEAD8213:-11bc59c692}
L=$LOGROOT/piece$PIECE; mkdir -p $L
V="STABLE_SEQ=$STABLE_SEQ STABLE_UPD_SHA=$STABLE_UPD_SHA STABLE_TASKS_SHA=$STABLE_TASKS_SHA SEQ1=$SEQ1 SEQ2=$SEQ2 PR_UPD_SHA=$PR_UPD_SHA PR_TASKS_SHA=$PR_TASKS_SHA HARNESS_WANT=$HARNESS_WANT"
export KIT_PIECE_PID=$$
EFDONE=0 FOUND_ENGINE=${FOUND_ENGINE:-} FOUND_B64=${FOUND_B64:-} LOCKED=0 R1DONE=0 R2DONE=0 UNMUTED=0 REMUTED=0 NEAR_ON=0 LEGS=0 PUTBACK=0 ONSEQ1=0 RELEASED=0 PBDONE=0 STOPPED=""
T(){ print -r -- "$(date '+%T') [$PIECE] $*" | tee -a $L/times.log; }
need(){ grep -q "$2" $L/$1.txt || { T "STOP at $1: no '$2' (read $L/$1.txt)"; return 3; }; }
go(){ local st=$1
  [[ $st == lock ]] || RENEW || { T "STOP at $st: the $LAMP lock is not ours"; STOPPED=$st; exit 1; }
  T "step $st"; s_$st || { STOPPED=$st; exit 3; }; }
pb(){ local st=$1; RENEW || { T "PUT-BACK $st NOT RUN: the $LAMP lock is not ours (the lamp is left as it is)"; return 1; }
  T "put-back $st"; s_$st || T "put-back $st: FAILED (read $L/$st.txt)"; }

# ---------------------------------------------------------------- the round's steps (round.zsh, copied)
s_lock(){ LAMP_SSH=$W $D/lock.zsh take | tee $L/lock.txt; need lock "LOCK TAKEN" && LOCKED=1; }
s_pre0(){ curl -fsSL https://github.com/autonomous-ai/lampOS/releases/download/lampos-stable/lampos-manifest.json \
    | python3 -I -c 'import base64,json,sys; print("stable feed", json.loads(base64.b64decode(json.load(sys.stdin)["manifest"]))["sequence"])' | tee $L/pre0.txt
  grep -q "stable feed $STABLE_SEQ\$" $L/pre0.txt || { T "STOP: the stable feed moved from $STABLE_SEQ: prep again"; return 3; }; }
s_stable0(){ EARLIER; GATE stable0 1 || return 1
  LAMP "$V" $D/stable0.lamp.sh 2>&1 | tee $L/stable0.txt; need stable0 "^STABLE0 OK"; }
s_pre(){ LAMP "$V" $D/pre.lamp.sh 2>&1 | tee $L/pre.txt; need pre "^PRE OK" || return 3
  grep -qE "^HARNESS .*$HARNESS_WANT" $L/pre.txt || { T "PRE STOP harness=$(grep -m1 "^HARNESS" $L/pre.txt) (want $HARNESS_WANT; the build pins $BUILD_PIN)"; return 3; }; T "Harness $HARNESS_WANT"; }
s_copy(){ tar -C $OUT -cf - ch1 ch2 test.pub SHAS plan.env | $W "rm -rf /tmp/t8156 && mkdir -p /tmp/t8156 && tar -xf - -C /tmp/t8156" \
    && T "copied $(du -sh $OUT | cut -f1)" || { T "STOP: copy failed"; return 3; }; }
s_stage(){ LAMP "$V" $D/stage.lamp.sh 2>&1 | tee $L/stage.txt; need stage "^STAGED OK"; }
s_r1(){ EARLIER; GATE r1 3 || return 1; R1DONE=1   # from here the lamp may be on SEQ1
  LAMP "N=1 SEQ=$SEQ1 $V" $D/round.lamp.sh 2>&1 | tee $L/r1.txt; need r1 "ROUND 1 DONE" || return 3
  grep -qE "switched to build $SEQ1|up to date: build $SEQ1" $L/r1.txt || { T "STOP: round 1 did not land $SEQ1"; return 3; }
  # #8302: the ExecStart systemd loaded must name the kit's file:// channel (zz-t8156.conf sorts after zz-cdn-feed.conf)
  LAMP "" $D/readback.lamp.sh > $L/r1-execstart.txt 2>&1
  local ch="--channel-url file:///var/lib/lampos-t8156/ch1/" arg=$(sed -n 's/^UPD_ARGV //p' $L/r1-execstart.txt)
  [[ $arg == *"$ch "* ]] && grep -qF 'url=file:///var/lib/lampos-t8156/ch1/' $L/r1.txt \
    && T "r1 ExecStart loaded: the kit's file:// channel ($(sed -n 's/^UPD_DROPINS_ALL //p' $L/r1-execstart.txt))" \
    || { T "STOP: r1 ExecStart does not name the kit's file:// channel: ${arg:-none} (drop-ins: $(sed -n 's/^UPD_DROPINS_ALL //p' $L/r1-execstart.txt))"; return 3; }; }
s_verify(){ LAMP "$V" $D/verify.lamp.sh 2>&1 | tee $L/verify.txt; need verify "^VERIFY PASS"; }
s_ready(){ local i; for i in 1 2 3 4 5 6 7 8 9 10; do LAMP "" $D/ready.lamp.sh 2>&1 | tee -a $L/ready.txt | grep -q '^READY' && break; sleep 30; done
  need ready "^READY"; }
s_unmute(){ GATE unmute 1 || return 1; UNMUTED=1
  LAMP "" $D/mute-lift.lamp.sh 2>&1 | tee $L/unmute.txt; grep -qE "^MUTE (LIFTED OK|NOT MUTED)" $L/unmute.txt || { T "STOP at unmute (read $L/unmute.txt)"; return 3; }
  T "MIC_UNMUTED $(grep -E '^(RECORD_BEFORE|MUTE )' $L/unmute.txt | tr '\n' ' ' | cut -c1-200)"; }
s_r2(){ EARLIER; GATE r2 3 || return 1; R2DONE=1
  LAMP "N=2 SEQ=$SEQ2 $V" $D/round.lamp.sh 2>&1 | tee $L/r2.txt
  grep -qE "switched to build $SEQ2|up to date: build $SEQ2" $L/r2.txt && T "r2 landed $SEQ2: stable's own binaries, served at SEQ2 above SEQ1 (a round never installs a lower sequence)" \
    || T "WARN: round 2 did not land $SEQ2: read $L/r2.txt before putback"; }
s_remute(){ GATE remute 1 || return 1; REMUTED=1
  LAMP "" $D/mute-back.lamp.sh 2>&1 | tee $L/remute.txt; T "MIC_REMUTED $(grep -E '^MUTE BACK' $L/remute.txt | tail -1)"; }
s_nostop(){ LAMP "" $D/nostop.lamp.sh 2>&1 | tee $L/nostop.txt; need nostop "^NOSTOP OK"; }
s_putback(){ s_nostop || return 3; EARLIER; GATE putback 1 || return 1; PUTBACK=1
  LAMP "$V" $D/putback.lamp.sh 2>&1 | tee $L/putback.txt; need putback "^PUTBACK OK"; }
s_read(){ LAMP "$V" $D/read.lamp.sh > $L/read1.txt 2>&1; sleep 60; LAMP "$V" $D/read.lamp.sh > $L/read2.txt 2>&1
  cat $L/read1.txt; diff <(grep -vE '^J ' $L/read1.txt) <(grep -vE '^J ' $L/read2.txt) && T "READ TWICE: same" || T "READ TWICE: differs (above)"
  LAMP "" $D/ready.lamp.sh 2>&1 | tee -a $L/read2.txt; }
s_release(){ LAMP_SSH=$W $D/lock.zsh release | tee $L/release.txt; LOCKED=0; RELEASED=1; STARTS | tee -a $L/times.log; }

# ---------------------------------------------------------------- split kit's own steps
# LAMP_NAME_NEAR=off in its own lamp-voice drop-in (near.lamp.sh), one per piece, removed by that piece's put-back.
s_near_on(){ GATE near_on 1 || return 1; NEAR_ON=1
  LAMP "ACT=on TAG=piece$PIECE" $D/near.lamp.sh 2>&1 | tee $L/near_on.txt; need near_on "^NEAR OFF SET" || return 3
  T "LAMP_NAME_NEAR=off: lamp-voice drop-in zz-sd-split-near.conf written for this piece, lamp-voice restarted, env read back off"; }
s_near_off(){ GATE near_off 1 || return 1
  LAMP "ACT=off TAG=piece$PIECE" $D/near.lamp.sh 2>&1 | tee $L/near_off.txt; grep -qE '^NEAR (REMOVED|none to remove)' $L/near_off.txt || return 3
  NEAR_ON=0; T "LAMP_NAME_NEAR drop-in: $(grep -m1 '^NEAR ' $L/near_off.txt)"; }
# The as-found record (piece A's start) and the end compare (piece B's end), by sha (readback.lamp.sh).
s_asfound(){ LAMP "" $D/readback.lamp.sh > $LOGROOT/asfound.txt 2>&1; cp $LOGROOT/asfound.txt $L/asfound.txt; need asfound "^READBACK DONE" || return 3
  FOUND_ENGINE=$(sed -n 's/^KEPT_ENGINE //p' $L/asfound.txt); FOUND_B64=$(sed -n 's/^ENGINE_B64 //p' $L/asfound.txt)
  T "AS FOUND record: $(grep -E '^(BUILD|ENGINE|KEPT_ENGINE|FILE|UPD_DROPINS_KEEP)' $L/asfound.txt | awk '{print $1, $2, substr($3,1,16)}' | tr '\n' ';')"
  [[ $FOUND_ENGINE == (openclaw|hermes) ]] || { T "STOP: the engine at start is '$FOUND_ENGINE': the put-back can restore only openclaw or hermes"; return 3; }
  print -r -- "FOUND_ENGINE=$FOUND_ENGINE FOUND_B64=$FOUND_B64" > $LOGROOT/asfound.env; T "engine found: $FOUND_ENGINE (every put-back restores it)"; }
# After round 1: the three files the PRs change, read back by sha against the build's own (piece A's verify).
s_readback(){ LAMP "" $D/readback.lamp.sh > $L/readback.txt 2>&1; need readback "^READBACK DONE" || return 3
  local got_ns=$(sed -n 's/^FILE agent-ns \([^ ]*\) .*/\1/p' $L/readback.txt) got_h=$(sed -n 's/^FILE hermes \([^ ]*\) .*/\1/p' $L/readback.txt)
  local ni=$(sed -n 's/^HERMES_NONINTERACTIVE //p' $L/readback.txt) bad=0 v=$L/readback-verdict.txt; : > $v
  if [ -n "${WANT_SHA_AGENT_NS:-}" ]; then
    [ "$got_ns" = "$WANT_SHA_AGENT_NS" ] && echo "READBACK agent-ns PASS /usr/local/lib/lampos/agent-ns sha $got_ns = the build's (WANT_SHA_AGENT_NS)" >> $v \
      || { echo "READBACK agent-ns FAIL /usr/local/lib/lampos/agent-ns sha ${got_ns:-?}, the build's $WANT_SHA_AGENT_NS" >> $v; bad=1; }
  else # No exact sha of the generated text (README): #8213's start lines and lamp-update's mark in what the round wrote.
    ns_start=$(sed -n 's/^AGENT_NS_START //p' $L/readback.txt); ns_mark=$(sed -n 's/^AGENT_NS_MARK //p' $L/readback.txt)
    [ -n "$got_ns" ] && [ "$got_ns" != absent ] && (( ${ns_start:-0} >= 1 && ${ns_mark:-0} == 1 )) \
      && echo "READBACK agent-ns PASS (content, no WANT_SHA_AGENT_NS) /usr/local/lib/lampos/agent-ns sha $got_ns: lamp-update's mark, #8213's /run/lampos-skill-start in $ns_start line(s)" >> $v \
      || { echo "READBACK agent-ns FAIL (content) sha ${got_ns:-?}, mark ${ns_mark:-0}, /run/lampos-skill-start lines ${ns_start:-0}" >> $v; bad=1; }
  fi
  [ "$got_h" = "$WANT_SHA_HERMES" ] && echo "READBACK one-path-hermes PASS /usr/local/bin/hermes sha $got_h = $MERGE_SHORT:$OPH" >> $v \
    || { echo "READBACK one-path-hermes FAIL /usr/local/bin/hermes sha ${got_h:-?}, $MERGE_SHORT:$OPH is $WANT_SHA_HERMES" >> $v; bad=1; }
  [ "$got_h" = "$WANT_SHA_HERMES" ] && (( ${ni:-0} >= 1 )) && echo "READBACK hermes-launcher PASS /usr/local/bin/hermes the build's bytes, carries HERMES_NONINTERACTIVE=1 ($ni line)" >> $v \
    || { echo "READBACK hermes-launcher FAIL /usr/local/bin/hermes sha ${got_h:-?} (want $WANT_SHA_HERMES), HERMES_NONINTERACTIVE=1 lines ${ni:-0}" >> $v; bad=1; }
  while read -r x; do T "$x"; done < $v
  (( bad == 0 )) || { T "STOP: a READBACK FAIL: before the legs"; return 3; }; }
# The engine back to the one found at piece A's start, once, only if it is not already (lamp.sh `engine`, uploaded again).
kept_engine(){ $W "sudo -n cat /var/lib/lampos-tasks/store/one-session.json" < /dev/null | python3 -I -c 'import json,sys;print(json.load(sys.stdin).get("engine"))' 2>/dev/null; }
s_engine_back(){ local e=$(kept_engine) f=$FOUND_ENGINE
  if [ "$e" = "$f" ]; then T "engine already $f, as found (the legs' own put-back did it)"; else
    GATE engine_back 1 || return 1
    $W "mkdir -p /tmp/sd8156 && cat > /tmp/sd8156/lamp.sh" < $D/lamp.sh && $W "sudo -n sh /tmp/sd8156/lamp.sh engine $f" < /dev/null > $L/engine_back.txt 2>&1
    grep -qE "kept session [a-z]+ [0-9a-f]+ \\($f" $L/engine_back.txt && T "engine back to $f: $(grep -oE "kept session [a-z]+ [0-9a-f]+ \\(${f}[^)]*\\)" $L/engine_back.txt | tail -1)" \
      || { T "engine back: no $f kept-session line (engine_back.txt)"; return 3; }; fi; }
# engine-default.json's bytes as found (the engine verb writes its own), only once the kept engine is the found one.
s_enginefile(){ [ "$(kept_engine)" = "$FOUND_ENGINE" ] || { T "engine file NOT restored: the kept engine is not $FOUND_ENGINE"; return 3; }
  LAMP "FOUND_B64=$FOUND_B64" $D/enginefile.lamp.sh 2>&1 | tee $L/enginefile.txt; grep -qE '^ENGINEFILE (AS FOUND|RESTORED)' $L/enginefile.txt || return 3
  EFDONE=1; T "$(grep -m1 '^ENGINEFILE' $L/enginefile.txt)"; }
s_tmpgone(){ $W "rm -rf /tmp/sd8156 /tmp/sd8213" && T "the kit's lamp temp dirs /tmp/sd8156 /tmp/sd8213 removed"; }
# Every PASS/FAIL line, whole files fenced for the drafts.
fence(){ echo '```'; cat "$@" 2>/dev/null | cut -c1-260; echo '```'; }
