#!/bin/zsh
# Piece B of the split #8156 + #8213 ROW kit: on the same lamp, right after piece A. Checks the lamp is still on SEQ1 (else
# STOP before anything), then #8213's Hermes legs 6-8 (MAKE, TRY per run, RUN per run, REMOVE), then r2 back to stable's
# bytes, remute, put-back, read twice, as-found compare, rounds, the two ROW drafts (posted by nobody), release.
#   ./pieceB.zsh          the piece          ./pieceB.zsh finish   only the put-back of a lamp piece A left on SEQ1
set -u; D=${0:a:h}; PIECE=B; MODE=${1:-run}
: ${CLIPS:?CLIPS=<dir of rendered wavs, clips.txt>}
source $D/piece.lib.zsh
[ -f $LOGROOT/asfound.env ] || { echo "STOP: no $LOGROOT/asfound.env: piece A's as-found record is needed (same LOGROOT)"; exit 1; }
source $LOGROOT/asfound.env
T "piece B ($MODE) on $LAMP: SEQ1 $SEQ1 -> SEQ2 $SEQ2 (stable $STABLE_SEQ's bytes)"
T "voice drop-ins of this piece: zz-sd-split-near.conf (LAMP_NAME_NEAR=off) and legs8213's agent-first zz-sd-8213-first.conf"

s_seq1(){ # the lamp still on SEQ1 and the PR build (verify.lamp.sh, read only, no wait), else STOP before anything
  LAMP "HWAIT=0 $V" $D/verify.lamp.sh 2>&1 | tee $L/seq1.txt
  local b=$(sed -n 's/^STATE \([0-9]*\).*/\1/p' $L/seq1.txt | head -1)
  [ "$b" = "$SEQ1" ] || { T "STOP: the lamp is on build '${b:-?}', not SEQ1 $SEQ1: nothing done (read $L/seq1.txt)"; return 3; }
  need seq1 "^VERIFY PASS" || { T "STOP: on $SEQ1 but not the PR build as piece A left it"; return 3; }
  ONSEQ1=1; grep -q 'PIECE A END: lamp on SEQ1' $LOGROOT/pieceA/times.log 2>/dev/null && T "on SEQ1 $SEQ1 as piece A left it" \
    || T "on SEQ1 $SEQ1 (note: no PIECE A END marker in $LOGROOT/pieceA/times.log)"; }
s_legsB(){ LEGS=1   # #8213's Hermes steps 6..8 only; its own put-back: engine openclaw, agent-first drop-in off, /tmp/sd8213 gone
  env CLIPS=$CLIPS LAMP=$LAMP LAMP_SSH=$W LL=$LL ENGINE_BACK=$FOUND_ENGINE HEAD8213=$H8213 \
    FROM=6 UNTIL=8 LEGS_LOG=$L zsh $D/legs8213.zsh 2>&1 | tee $L/legs.txt; local rc=${pipestatus[1]}
  LEGSDIR=$(ls -dt $L/legs-* 2>/dev/null | head -1); T "legs 6-8 ended rc=$rc ($LEGSDIR/verdict.txt)"
  (( rc == 0 )) || { T "STOP in the legs (rc $rc)"; return 3; }
  grep -qE ' STOP|^STOP' $LEGSDIR/verdict.txt && { T "STOP in the legs: $(grep -m1 -E ' STOP|^STOP' $LEGSDIR/verdict.txt)"; return 3; }
  return 0; }
s_kitleft(){ # a legs run cut before its own put-back: the engine back, legs8213's agent-first drop-in removed
  grep -q "lamp /tmp/sd8213 removed" ${LEGSDIR:-/nonexistent}/verdict.txt 2>/dev/null && { T "legs' own put-back seen"; return 0; }
  s_engine_back; GATE kitleft 1 || return 1
  LAMP "" $D/kitleft.lamp.sh 2>&1 | tee $L/kitleft.txt; s_tmpgone; need kitleft "^KITLEFT CLEAN"; }
s_rounds(){ local a b e
  a=$(grep -cE 'serve channel=.* url=file:///var/lib/lampos-t8156/ch1/' $LOGROOT/pieceA/r1.txt 2>/dev/null); b=$(grep -cE 'serve channel=.* url=file:///var/lib/lampos-t8156/ch2/' $L/r2.txt 2>/dev/null)
  e=$(cat $LOGROOT/pieceA/r1.txt $L/r2.txt 2>/dev/null | grep -cE '^J lamp-update: .*lamp-update: (up to date: build [0-9]+|switching to build [0-9]+)')
  ROUNDS=$(( ${a:-0} + ${b:-0} ))
  T "ROUNDS $ROUNDS (the lamp's own updater on the test channel: r1 ${a:-0}, r2 ${b:-0} serve starts, each rounding at its start; 'up to date'/'switching' lines: ${e:-0})"; }
s_asfound_cmp(){ local f0=$LOGROOT/asfound.txt f1=$L/asfound-end.txt k a b diffs="" notes=""
  [ -f $f0 ] || { T "AS FOUND FAIL no record from piece A's start ($f0)"; return 3; }
  LAMP "" $D/readback.lamp.sh > $f1 2>&1; grep -q '^READBACK DONE' $f1 || { T "AS FOUND FAIL the end read failed ($f1)"; return 3; }
  g(){ sed -n "s/^$1 //p" $2 | head -1; }
  for k in "FILE agent-ns" "FILE hermes" "FILE lamp-update" ENGINE ENGINE_FILE KEPT_ENGINE DROPINS KITLEFT UPD_DROPINS_KEEP UPD_ARGV; do
    a=$(g "$k" $f0); b=$(g "$k" $f1); [ "$a" = "$b" ] || diffs="$diffs; ${k#FILE } ${a[1,16]} -> ${b[1,16]}"; done
  a=$(g BUILD $f0 | cut -d' ' -f1); b=$(g BUILD $f1 | cut -d' ' -f1)
  if [ "$a" = "$b" ]; then :; elif [ "$a" = "$STABLE_SEQ" ] && [ "$b" = "$SEQ2" ]; then
    notes="$notes; build $a -> $b: stable came back as SEQ2, stable's own binaries served above SEQ1 (a round never installs a lower sequence)"
  elif grep -q "^STATE before $a" $LOGROOT/pieceA/stable0.txt 2>/dev/null && grep -q "^STABLE0 OK$" $LOGROOT/pieceA/stable0.txt && [ "$b" = "$SEQ2" ]; then
    notes="$notes; build $a -> $b: found off stable, so piece A's ordinary stable round (stable0) took it to stable $STABLE_SEQ first; it ended as SEQ2, stable's own binaries above SEQ1 (the kit cannot go back below a sequence)"
  else diffs="$diffs; build $a -> $b"; fi
  if [ -z "$diffs" ]; then T "AS FOUND PASS sequence, engine and its file, agent-ns, hermes, lamp-update by sha; lamp-update drop-ins outside /run by sha and its ExecStart as at the start${notes}"; else T "AS FOUND FAIL ${diffs#; }${notes}"; fi
  T "put-back left by design: /home/orangepi/t8156 (the round's evidence, mute record) and changes.txt lines"; }
s_drafts(){ local la=$(ls -dt $LOGROOT/pieceA/legs-* 2>/dev/null | head -1) lb=${LEGSDIR:-$(ls -dt $L/legs-* 2>/dev/null | head -1)} okA=FAIL okB=FAIL af rd
  af=$(grep -hoE 'AS FOUND (PASS|FAIL).*' $L/times.log | tail -1); rd=$(grep -hoE 'ROUNDS [0-9]+.*' $L/times.log | tail -1)
  head -1 $la/row.txt 2>/dev/null | grep -q ': PASS$' && ! grep -q 'READBACK .* FAIL' $LOGROOT/pieceA/readback-verdict.txt 2>/dev/null \
    && grep -q '^NEAR OFF SET' $LOGROOT/pieceA/near_on.txt 2>/dev/null && ! grep -q 'STOP' $LOGROOT/pieceA/times.log && okA=PASS
  head -1 $lb/row.txt 2>/dev/null | grep -q ': PASS$' && grep -q '^NEAR OFF SET' $L/near_on.txt 2>/dev/null && [ -z "$STOPPED" ] && okB=PASS
  { echo "ROW #8156 $H8156 on $LAMP build $SEQ1: $okA (LAMP_NAME_NEAR=off)"; echo
    echo "Local build ${${LOCAL_ART:-?}:t} (tree $TREE, merge ${MERGE:-?}) by a real round from stable $STABLE_SEQ; r2 back to stable's own binaries as build $SEQ2. ${rd}. ${af}."
    echo; echo "Read back after round 1 (by sha against the build's files):"; fence $LOGROOT/pieceA/readback-verdict.txt
    echo; echo "Legs 3-6 (engine hermes + READY, T1, T2, T4):"; fence $la/verdict.txt
    echo; echo "Journal (lamp-tasks, lamp-voice; kept session):"; fence $la/eng-hermes.txt $la/j-tasks.txt $la/j-voice.txt; } > $LOGROOT/row8156-draft.md
  { echo "ROW #8213 $H8213 on $LAMP build $SEQ1: $okB (LAMP_NAME_NEAR=off)"; echo
    echo "Same lamp and build as #8156's row, legs 6-8 (Hermes MAKE, TRY per run, RUN per run, REMOVE). ${rd}. ${af}."
    echo; echo "Legs:"; fence $lb/verdict.txt
    echo; echo "Journal (lamp-tasks, lamp-voice, lamp-skills per leg):"; fence $lb/eng-hermes.txt $lb/j-H*-*.txt $lb/j-HDEL.txt; } > $LOGROOT/row8213-draft.md
  T "drafts written, posted by nobody: $LOGROOT/row8156-draft.md ($okA), $LOGROOT/row8213-draft.md ($okB)"; }

# Piece B's put-back, once (EXIT trap or normal end): a cut legs run's leftovers, then r2 if not done yet (the lamp was
# on SEQ1), then putback, the engine file as found, the LAMP_NAME_NEAR drop-in off, remute, as-found compare, rounds,
# drafts, release.
pbB(){ (( PBDONE )) && return 0; PBDONE=1
  T "== piece B put-back (${${STOPPED:+after STOP at $STOPPED}:-normal end})"
  if (( LOCKED )); then
    (( LEGS )) && pb kitleft
    (( ONSEQ1 && ! R2DONE )) && pb r2
    (( ONSEQ1 && R2DONE && ! PUTBACK )) && pb putback
    (( ONSEQ1 && ! EFDONE )) && pb enginefile
    (( NEAR_ON )) && pb near_off
    (( UNMUTED && ! REMUTED )) && pb remute
    (( R2DONE )) && { s_asfound_cmp; s_rounds; s_drafts; }
    (( RELEASED )) || s_release; fi
  (( ONSEQ1 )) || T "piece B touched nothing (not on SEQ1, or stopped before the check)"; }
trap 'STOPPED=${STOPPED:-signal}; T "signal: piece B put-back runs"; exit 3' INT TERM HUP
trap 'pbB' EXIT

if [ $MODE = finish ]; then go lock; ONSEQ1=1; for st in r2 near_off putback enginefile read asfound_cmp rounds drafts release; do go $st; done
else for st in lock seq1 ready near_on unmute legsB r2 near_off remute putback enginefile read asfound_cmp rounds drafts release; do go $st; done; fi
PBDONE=1; T "piece B done; the drafts are yours to read (never posted by the kit)"
