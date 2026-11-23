#!/bin/zsh
# Piece A of the split #8156 + #8213 ROW kit, ~15 min typical: the round to the PR build (SEQ1) and #8156's
# legs 3-6 (engine hermes + READY, T1, T2, T4 in one kept Hermes session, one harness). It does NOT run r2: the lamp
# stays on SEQ1 at its end and piece B must run next on the same lamp. README.md has the env and the order.
#   LAMP=lamp-ee17 LAMP_SSH=... LL=... CLIPS=... GITDIR=... MERGE=... EXPECT_STABLE=... [WANT_SHA_AGENT_NS=...] ./pieceA.zsh
set -u; D=${0:a:h}; PIECE=A
: ${GITDIR:?GITDIR=<the git checkout holding MERGE>} ${CLIPS:?CLIPS=<dir of rendered wavs, clips.txt>}
: ${MERGE:?MERGE=<the merge commit the build was made from> (no default: README)}
OPH=lampOS/crates/update/src/agent_ns/wrapper/one-path-hermes.sh; MERGE_SHORT=${MERGE[1,10]}
WANT_SHA_HERMES=${WANT_SHA_HERMES:-$(git -C $GITDIR show ${MERGE}:${OPH} 2>/dev/null | shasum -a 256 | cut -d' ' -f1)}
[[ $WANT_SHA_HERMES == $(print -n | shasum -a 256 | cut -d' ' -f1) ]] && { echo "STOP: $OPH not readable at $MERGE in $GITDIR"; exit 1; }
source $D/piece.lib.zsh
T "piece A on $LAMP: stable $STABLE_SEQ -> SEQ1 $SEQ1 (#8156 $H8156, #8213 $H8213; merge $MERGE)"
T "voice drop-ins of this piece: zz-sd-split-near.conf (LAMP_NAME_NEAR=off) only; the round's drop-ins are lamp-update's"

# Piece A's put-back, once (the EXIT trap and the normal end both call it): the engine back to the one found, the kit's lamp
# temp dir removed, the LAMP_NAME_NEAR drop-in removed, remute, the nostop check, then the lock released. Never r2.
pbA(){ (( PBDONE )) && return 0; PBDONE=1
  T "== piece A put-back (${${STOPPED:+after STOP at $STOPPED}:-normal end})"
  if (( LOCKED )); then
    (( LEGS )) && { pb engine_back; pb enginefile; pb tmpgone; }
    (( NEAR_ON )) && pb near_off
    (( UNMUTED && ! REMUTED )) && pb remute
    (( R1DONE )) && pb nostop
    s_release; fi
  if (( R1DONE )); then
    if [ -z "$STOPPED" ]; then T "PIECE A END: lamp on SEQ1, piece B must run next on this lamp"
    else T "PIECE A STOPPED at $STOPPED after r1: lamp on SEQ1 ($SEQ1) or between; piece B (or \`pieceB.zsh finish\`) must run next on this lamp"; fi
  else T "PIECE A STOPPED at ${STOPPED:-?} before r1: the lamp was not moved to SEQ1 (read $L; stage leaves $CH only if it passed)"; fi; }
trap 'STOPPED=${STOPPED:-signal}; T "signal: piece A put-back runs"; exit 3' INT TERM HUP
trap 'pbA' EXIT

s_legsA(){ LEGS=1   # #8156's legs 3..6 only, on the PR build; its own put-back puts the engine back and removes /tmp/sd8156
  LAMP "" $D/mute.lamp.sh 2>&1 | tee $L/mute-check.txt
  env CLIPS=$CLIPS LAMP=$LAMP LAMP_SSH=$W LL=$LL ENGINE_BACK=$FOUND_ENGINE HEAD8156=$H8156 \
    FROM=3 UNTIL=6 LEGS_LOG=$L zsh $D/legs8156.zsh 2>&1 | tee $L/legs.txt; local rc=${pipestatus[1]}
  LEGSDIR=$(ls -dt $L/legs-* 2>/dev/null | head -1); T "legs 3-6 ended rc=$rc ($LEGSDIR/verdict.txt)"
  (( rc == 0 )) || { T "STOP in the legs (rc $rc)"; return 3; }
  grep -q ' STOP' $LEGSDIR/verdict.txt && { T "STOP in the legs: $(grep -m1 ' STOP' $LEGSDIR/verdict.txt)"; return 3; }
  return 0; }

for st in lock asfound pre0 stable0 pre copy stage r1 verify readback ready near_on unmute legsA; do go $st; done
pbA
