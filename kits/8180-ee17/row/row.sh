#!/bin/bash
# Mac. #8180 ROW legs on $LAMP (row/kit.sh settings), on the runner's own install of #8180's CI ARM64 artifact. Run by
# ../run.sh's `legs` and `tails` steps, after `check` (logs/verify.txt VERIFY PASS, logs/ready.txt READY), `micon` and
# `save` (logs/save.txt SAVED OK); a lock is held and renewed when KIT_LL is set. Hands-free: pre-rendered ElevenLabs
# clips from the Mac speaker (afplay -v gain only; Mac volume read, <= 70; no volume is ever set, the lamp's speaker is
# never touched). Verdicts from each leg's journal (logs/leg-<L>.journal.txt: lamp-voice, -tasks, -skills,
# -skills-manage@, -control, -openclaw-gateway, -update) and read-only lamp scripts.
#   A  DAYS make (days left in the year: no account, no net, so it runs signed out): SKILL.md has Uses/Result/Answer,
#      no stub, made_with = engine (made.json + card), its tried run's journal lines (exit 0, said >= 1), kept; then
#      DAYSASK once: the made skill's own run (or task) starts, ends exit 0 having said >= 1, and its say is spoken.
#   B  PRICE make (a `run` step), then PRICEASK: "skill run R: hand-over to script start N ms", ended exit 0
#      said >= 1, NO agent session (no "skill task" start/Harness/gateway line), clip end -> first say <= LIMIT_B s.
#   C  lemon-probe fixture (grants speak) asks the camera: lamp-control "refused: no_grant". C, H and P's fixtures go
#      in by the product's push install (FIXTURE, push.sh), the network door (the Mac's pairing) skipped.
#   D  (needs a linked Google account; VOID without one) MAIL make (a `task` step), then MAILASK: the task's prompt names <folder>/SKILL.md first, the agent's
#      session names no lamp file beyond its prompt's, clip end -> answer <= LIMIT_D s (about 30).
#   E  LAMP_MAKE_MAY_RUN_ON=on by a /run drop-in: LONGMAKE runs on past its minute, READY is answered
#      "still making" (journal: listed / asked-while-running-on), drop-in removed: RUNON OFF.
#   F  in the TRIED runs of A, B, D, seen from outside the fence: admin.sock not visible; B's
#      net try DOES reach out through its egress socket (a try runs with its grants live).
#   G  kernel >= 5.14, prlimit in the image, lampos tasks > 64, and A's try and B's run both PASS;
#      #8180's cgroup: during B's run a run-* leaf under lamp-tasks' cgroup with memory.max, memory.swap.max=0, cpu.max
#      and pids.max set, gone after the run; serve's pid in its own leaf (serve or main); no 'no own cgroup' refusal.
#   K  a camera + speaker skill: made, tried, run; the try's and the run's tokens are exactly [camera, speak]; said; no yes.
#   H  one lamp-tasks restart while slow-probe's script runs: old leaf gone, serve back, set-up line, fresh run.
#   P  fork-probe past its process cap refused while B's price run answers.
#   W  (#8130) tools.deny holds skill_workshop, the gateway accepts it, workshop-skills/ unchanged by the legs' makes/runs.
#      Not run by default: #8130 is not in #8180's artifact, so W is VOID for this ROW.
#   N  (#7269) LAMP_NAMED_AGENT_TAKES_LINE by a /run drop-in. Off first (as on stable): NAMED ("Hey Lamp, OpenClaw,
#      <a task>") is NOT handed to the agent in code (no hand-over line; the voice's own tool answers); on: the voice
#      journals "request R: the heard words named <agent> for the work: round 1 hands them over in code
#      (LAMP_NAMED_AGENT_TAKES_LINE)"; then off again (NAMED OFF). CHECK N1/N2/N3. The switch is taken off on ANY exit.
#   Every make prints its "Made it, and I tried it" line (MADELINE) when the journal has it.
#   ./row.sh A B C D K H P N E, then ./row.sh F G (F and G read A, B and D's logs and verdicts from logs/verdicts.env).
#   With no arguments: LEGS="A B C D K H P N E" TAILS="F G" (E last: its long make runs on past its leg). Exit 0 on
#   VERDICT PASS (this invocation's legs). A rerun of chosen legs: ./row.sh A B C D K H G P (G reads A and B of the same
#   invocation).
#   Every fresh-turn ask (ASK; its clip opens with "Hey Lamp,") is played only once the lamp's own say has ended (HUSH),
#   as a person would. A leg that would FAIL is VOID instead (POST) when (a) the voice held one of its asks back as said
#   to nobody ("its answer is held back: nobody spoke to the lamp", voice attention.rs NOT_SPOKEN_TO and
#   TURNED_NOT_SPOKEN_TO): "ask not heard as a call"; or (b) a make was still in flight: before each make MAKEIDLE waits
#   (IDLEWAIT, 600 s) until no long make runs on, and lamp-tasks' "skill make: refused busy, one is being made"
#   (tasks skill_make.rs:2140) is "a make still running". The kept session's cold start (#8237: "kept session new ... in
#   N ms", "slow chat send ... stage=startup") is printed as INFO, never a verdict.

set -u; D=$(cd "$(dirname "$0")" && pwd); . $D/kit.sh
LG=${ROWLOGS:-$D/logs}; mkdir -p $LG
[ $# -gt 0 ] && { LEGS="$*"; TAILS=; }
RENEW || exit 1; VOLOK || exit 2
grep -q '^VERIFY PASS' $D/logs/verify.txt 2>/dev/null || { echo "no VERIFY PASS in row/logs/verify.txt: #8180's build is not verified running (../run.sh check)"; exit 1; }
grep -q '^READY' $D/logs/ready.txt 2>/dev/null || { echo "no READY in row/logs/ready.txt (../run.sh check)"; exit 1; }
grep -q '^SAVED OK' $LG/save.txt 2>/dev/null || { echo "no SAVED OK in row/logs/save.txt (../run.sh save first)"; exit 1; }
for c in DAYS DAYSASK PRICE PRICEASK PROBE MAIL MAILASK LONGMAKE READY YES2 LOOK LOOKASK SLOW FORK NAMED; do [ -f $A/$c-el.wav ] || { echo "no clip $A/$c-el.wav: ../run.sh render (clips/TODO.txt)"; exit 1; }; done
# Leg N's switch off on ANY exit once it was asked on (logs/named-on.txt) and NAMED OFF not read since (MODE=off is
# idempotent; 1 lamp-voice start).
NOFF(){ LAMP "MODE=off" $D/named.lamp.sh > $LG/named-off.txt 2>&1; grep -q '^NAMED OFF' $LG/named-off.txt; }
NGUARD(){ [ -f $LG/named-on.txt ] || return 0; grep -q '^NAMED OFF' $LG/named-off.txt 2>/dev/null && return 0
  echo "EXIT with leg N's switch maybe on: taking it off"; RENEW || { echo "WARN lock lost: named switch NOT off: ../run.sh unrow"; return 0; }
  NOFF && echo "  NAMED OFF" || echo "  WARN named switch NOT off (logs/named-off.txt): ../run.sh unrow"; }
JP=; FP=; trap '[ -n "$JP" ] && kill $JP 2>/dev/null; [ -n "$FP" ] && kill $FP 2>/dev/null; [ -n "${CGP:-}" ] && kill $CGP 2>/dev/null; RECSTOP; trap - INT TERM HUP; NGUARD' EXIT; trap 'exit 130' INT TERM HUP
POLL=${POLL:-1}; LIMIT_B=${LIMIT_B:-6}; LIMIT_D=${LIMIT_D:-45}; MAKEWAIT=${MAKEWAIT:-660}
# Each ask's wait for its run or answer: at least 180 s, since the first make or task after a round may wait on a cold
# kept OpenClaw session (e82d, 22 Nov 14:38: "kept session new ... in 84883 ms"). A make has MAKEWAIT (660 s).
RUNWAIT=${RUNWAIT:-180}; TASKWAIT=${TASKWAIT:-240}; IDLEWAIT=${IDLEWAIT:-600}; IDLEPOLL=${IDLEPOLL:-15}
lines(){ wc -l < $J | tr -d ' '; }
# first line number after line $2 matching ERE $1 and not ERE $4 (optional), waiting at most $3 s (lock renewed every
# 120 s)
waitfor(){ local s=$(date +%s) r=$(date +%s) n lim=$3 x=${4:-}
  [ -n "${WAITMAX:-}" ] && [ "$lim" -gt "$WAITMAX" ] && lim=$WAITMAX     # WAITMAX: mock runs only
  while [ $(( $(date +%s) - s )) -lt $lim ]; do
    n=$(tail -n +$(( $2 + 1 )) $J | grep -nE -- "$1" | { if [ -n "$x" ]; then grep -vE -- "$x"; else cat; fi; } | head -1 | cut -d: -f1); [ -n "$n" ] && { echo $(( n + $2 )); return 0; }
    [ $(( $(date +%s) - r )) -ge 120 ] && { RENEW || return 2; r=$(date +%s); }
    sleep $POLL; done; return 1; }
ACT='request [0-9]+|pieces began'
quiet(){ local a b i=0; while [ $i -lt 15 ]; do a=$(grep -cE "$ACT" $J); sleep ${QUIETS:-6}; b=$(grep -cE "$ACT" $J); [ "$a" = "$b" ] && return 0; i=$((i+1)); done; }
saydone(){ local t c; t=$(waitfor "[0-9]+ of [0-9]+ pieces began" $1 ${SAYWAIT:-40}) || { echo "  no pieces line: waiting 35 s"; sleep 35; return; }
  c=$(sed -n "${t}p" $J | grep -oE "[0-9]+ chars" | tail -1 | grep -oE "[0-9]+"); echo "  say-back ${c:-?} chars: ends in ~$(( ${c:-0} / 12 + 3 )) s"; sleep $(( ${c:-0} / 12 + 3 )); }
show(){ sed -n "${1}p" $J | cut -c1-240; }
ep(){ sed -n "${1}p" $J | awk '{print $1}'; }                       # the lamp's epoch of line $1
since(){ python3 -c "import sys;print('%.2f'%(float(sys.argv[1])-float(sys.argv[2])-float(sys.argv[3])))" $1 $2 $SK_; }  # lamp t - Mac t - skew
KEPT="a skill made here was kept in [0-9]+ ms"
# A later round's make_skill for the same ask while the first make runs (ee17 rest1, 23 Nov: a second chat round, 0
# words heard): lamp-voice "a skill wasn't made again: the same ask is being made already". It is not the make's end:
# MAKE skips it and waits for the first make's own kept / not-kept line.
AGAIN="a skill wasn't made again: the same ask is being made already"
NOTKEPT="a skill made here wasn't kept in|a skill wasn't made|the lamp's model said no made skill can do it|wasn't kept: |was not kept: |a long make made nothing"
READBACK="read back: it waits for the person's yes|said back unasked waits for the [a-z]+'s yes|with no yes is said back unasked"
# A pushed skill's run asks once (ee17 23 Nov): "do.run: not handed over: waiting for the owner's yes (the lamp says it
# back and asks); asked aloud: say: spoken". ASKYES plays the clip, and on that line answers YES2. The run that
# waited spent nothing and comes again only on the next ask (crates/skills, "the next time its trigger holds"), so
# once lamp-control keeps the yes ("the owner's yes to <skill>'s task kept"), the lamp is let finish and asked again.
RUNYES="do.run: not handed over: waiting for the [a-z]+'s yes .*asked aloud: say: spoken"
YESKEPT="yes to [^ ]+'s task kept"
ASKYES(){ local a y k; a=$(lines); ASK $1; y=$(waitfor "$RUNYES|$STARTED" $a ${YESWAIT:-90}) || return 0
  sed -n "${y}p" $J | grep -qE "$RUNYES" || return 0
  echo "  run yes asked: $(show $y | cut -c1-160)"; sleep 2; PLAY YES2; echo "  YES2 played for the run's yes"
  k=$(waitfor "$YESKEPT" $y ${YESWAIT:-90}) || { echo "  the yes was not kept in ${YESWAIT:-90} s: not asked again"; return 0; }
  echo "  yes kept: $(show $k | cut -c1-140)"; quiet; HUSH; ASK $1; echo "  $1 asked again after the yes"; }
YESOK="is on, on the person's yes|approved on the person's yes"
TRY="\[lamp-tasks\] make try: "
# The voice's journal of the made line (voice/src/made_skill.rs tried_journal, worktree 21 Nov): only its length and
# whether it named what the skill uses, never its words: "a made skill is said on with its try (N chars, uses named)".
# The older form ("...with its try: <the line>") is read too. The words are checked from the Mac recording (MAC_REC=1).
SAIDON="lamp-voice: a made skill is said on with its try"
# The spoken line's most: "Made it" 7 + ". It uses " 10 + uses (~90) + ", and I tried it" 16 + ": " 2 + words
# (TRIED_WORDS_MOST 120) + "." 1 + extra (CODE_RUNS_NOWHERE_YET 40) + " Say 'remove it' to undo." 25 = 311.
LIMIT_MADE=${LIMIT_MADE:-320}
# Grants uses_briefly names (camera, microphone, the internet, notes, speech, a service's -read/-write)
USEG='(^|,)(camera|look|microphone|hear|net|notes|speech|[a-z0-9_]+-(read|write))(,|$)'
# MADECHK: MADEWHY empty when the said-on line is right for $ML's grants (sets MADEN chars, MADEU named|none)
MADECHK(){ local g k; MADEWHY=; MADEN=; MADEU=; g=$(echo "$ML" | field grants); k=$(echo "$ML" | field kind)
  if [ -z "$MADELINE" ]; then case "$k" in run|both) MADEWHY=" no said-on line ('$SAIDON') in the journal;";; esac; return 0; fi
  case "$MADELINE" in
    *"chars, uses named)"*) MADEU=named;; *"chars, no uses named)"*) MADEU=none;;
    *"with its try: "*) MADEN=$(printf '%s' "${MADELINE#*with its try: }" | wc -m | tr -d ' ')
       case "$MADELINE" in *". It uses "*) MADEU=named;; *) MADEU=none;; esac;;
    *) MADEWHY=" said-on line not read: $MADELINE;"; return 0;; esac
  [ -n "$MADEN" ] || MADEN=$(echo "$MADELINE" | sed -n 's/.*with its try (\([0-9]*\) chars.*/\1/p')
  echo "$g" | grep -qE "$USEG" && [ "$MADEU" != named ] && MADEWHY="$MADEWHY the said-on line names no uses, grants $g;"
  [ -n "$MADEN" ] && [ "$MADEN" -le "$LIMIT_MADE" ] || MADEWHY="$MADEWHY said-on line ${MADEN:-?} chars > $LIMIT_MADE;"
  return 0; }
SAY="lamp-control: .*: say \(|[0-9]+ of [0-9]+ pieces began"
AGENT="\[lamp-tasks\] skill task [^ ]+: (started on the lamp's default agent|ended, Harness says|granted net: its egress socket .* default agent)|Harness .*(new|session)"
EGJ="egress ready in its fence|granted net: its egress socket|\[lamp-tasks\] egress: "
BEGIN(){ LEG=$1; J=$LG/leg-$1.journal.txt; ASKN=; echo "== leg $1 $(date +%T)"; RENEW || return 1; : > $J; JP=$(JF $J); RECSTART $1 $LG/leg-$1.mac.wav
  SK_=$(SKEW | awk '{print $1}'); echo "  skew lamp-Mac ${SK_} s"; sleep 3; }
END(){ sleep ${TAIL:-8}; kill $JP 2>/dev/null; JP=; RECSTOP; COLD; }
# The kept session's cold start (#8237; tasks/src/one_session.rs:3197 and :3242, the Gateway's "slow chat send Nms
# stage=startup"): INFO only, never a verdict.
COLDRE="one session: kept session new .* in [0-9]+ ms|slow chat send [0-9]+ms.*stage=startup|kept session's first words not taken"
COLD(){ grep -E "$COLDRE" $J 2>/dev/null | cut -c1-220 | sed "s/^/INFO leg $LEG cold start (#8237, no verdict): /"; return 0; }
# HUSH: wait for the lamp's last say in this leg's journal to end (its pieces line + chars/12 + 3 s, at most 45 s) before
# a fresh-turn ask: a person does not talk over the lamp, and an ask played over its say is heard faintly.
HUSH(){ local t w; t=$(grep -nE "[0-9]+ of [0-9]+ pieces began" $J 2>/dev/null | tail -1 | cut -d: -f1); [ -n "$t" ] || { IDLE; return 0; }
  w=$(python3 -c "import sys,time;e=float(sys.argv[1])+int(sys.argv[2] or 0)/12+3;w=e-(time.time()+float(sys.argv[3]));print(int(min(max(w,0),45)+0.999))" "$(ep $t)" "$(sed -n "${t}p" $J | grep -oE '[0-9]+ chars' | tail -1 | grep -oE '[0-9]+')" "${SK_:-0}")
  [ "${w:-0}" -gt 0 ] && { echo "  HUSH the lamp's say ends in ~$w s: waiting"; sleep $w; }; IDLE; return 0; }
# IDLE: then the voice's own state, resting for 3 s (row/idle.lamp.sh); a say the journal's lines did not show (the
# make's tried line, the made line) still holds the ask back (Migrate, 23 Nov 09:16). Never a verdict.
IDLE(){ local o; o=$(LAMP "" $D/idle.lamp.sh 2>&1 | grep -E '^(NOT )?IDLE' | tail -1); echo "  HUSH ${o:-IDLE unread}"; return 0; }
# ASK <clip>: a fresh turn, noted (its first journal line) for HELDOF, then played
ASK(){ ASKN="$ASKN $(lines):$1"; PLAY $1; }
# HELDOF: the first of this leg's asks the voice held back as said to nobody, with its line; else nothing (exit 1)
HELDRE="its answer is held back: nobody spoke to the lamp"
HELDOF(){ local x n c nx h; set -- $ASKN; while [ $# -gt 0 ]; do x=$1; shift; n=${x%%:*}; c=${x#*:}; nx=${1:-}; nx=${nx%%:*}
    h=$(tail -n +$(( n + 1 )) $J 2>/dev/null | head -n $(( ${nx:-999999} - n )) | grep -E "$HELDRE" | head -1 | cut -c1-200)
    [ -n "$h" ] && { echo "$c: $h"; return 0; }; done; return 1; }
# POST <leg>: a FAIL is VOID when an ask was not heard as a call, or a make was still running (MKBUSY)
POST(){ local v h; eval "v=\$V$1"; [ "$v" = FAIL ] || return 0
  if h=$(HELDOF); then eval "V$1=VOID"; WHY="ask not heard as a call ($h); was: $WHY"
  elif [ -n "$MKBUSY" ]; then eval "V$1=VOID"; WHY="a make still running ($WHY)"; fi; return 0; }
# MAKEIDLE: no make in flight on the lamp: a long make that ran on (second_provider.rs, long_make.rs) with no end after
# it ("a long make was made", "a long make made nothing", lamp-tasks' "skill make: the run ended with an answer" or
# "refused <code>: "), read every IDLEPOLL s for at most IDLEWAIT s
RUNONRE="the make has no answer [0-9]+ s after its waiting line: it runs on|a long make's turn was cut before its minute: it runs on"
MENDRE="a long make was made|a long make made nothing|\[lamp-tasks\] skill make: (the run ended with an answer|refused [a-z_]+: )"
BUSYRE="\[lamp-tasks\] skill make: refused busy, one is being made"
MAKEIDLE(){ local s=$(date +%s) l said=0 lim=$IDLEWAIT; [ -n "${WAITMAX:-}" ] && [ "$lim" -gt "$WAITMAX" ] && lim=$WAITMAX
  while :; do l=$(SSHR -n $H "journalctl -u lamp-voice -u lamp-tasks --since -20min -o short-unix --no-pager" 2>/dev/null | grep -E "$RUNONRE|$MENDRE" | tail -1)
    echo "$l" | grep -qE "$RUNONRE" || { [ $said = 1 ] && echo "  MAKEIDLE no make in flight after $(( $(date +%s) - s )) s"; return 0; }
    [ $said = 0 ] && { echo "  MAKEIDLE a make is in flight: $(echo "$l" | cut -c1-160): waiting (at most $IDLEWAIT s)"; said=1; }
    [ $(( $(date +%s) - s )) -ge $lim ] && return 1; RENEW || return 1; sleep $IDLEPOLL; done; }
# Leg G's cgroup watch (cgwatch.lamp.sh) around B's run: logs/cg-<L>.txt
CGP=
CGW(){ SSHR -n $H "rm -f /tmp/t8180-stop.cg$1"; LAMP "SECS=${CGSECS:-240} STOPF=/tmp/t8180-stop.cg$1" $D/cgwatch.lamp.sh > $LG/cg-$1.txt 2>&1 & CGP=$!; sleep 2; }
UNCGW(){ SSHR -n $H "touch /tmp/t8180-stop.cg$1"; sleep 4; kill $CGP 2>/dev/null; CGP=; grep -E '^(SERVELEAF|RUNLEAF|CGEND|CGROOT)' $LG/cg-$1.txt | cut -c1-220 | sed 's/^/  cg /'; }
WATCH(){ SSHR -n $H "rm -f /tmp/t8180-stop.$1"; LAMP "SECS=$(( MAKEWAIT + 120 )) STOPF=/tmp/t8180-stop.$1" $D/fence.lamp.sh > $LG/fence-$1.txt 2>&1 & FP=$!; }
UNWATCH(){ SSHR -n $H "touch /tmp/t8180-stop.$1"; sleep 2; kill $FP 2>/dev/null; FP=; grep -E '^(TRYPID|ADMIN|NET|EGRESS)' $LG/fence-$1.txt | cut -c1-220 | sed 's/^/  fence /'; }
field(){ sed -n "s/.* $1=\([^ ]*\).*/\1/p"; }
# MAKE <clip>: the voice make, its read-back and yes when asked; sets MK (kept line no.), MN (new skill), ML (MADE line)
MAKE(){ local n0 w r y; MK=; MN=; ML=; RB=0; MADELINE=
  MAKEIDLE || { WHY="still in flight after $IDLEWAIT s, $1 not asked"; MKBUSY=1; return 1; }
  HUSH; n0=$(lines); MK0=$n0; WATCH $LEG; ASK $1
  w=$(waitfor "$KEPT|$NOTKEPT|$BUSYRE" $n0 $MAKEWAIT "$AGAIN") || { UNWATCH $LEG; WHY="no make end in $MAKEWAIT s"; tail -n +$(( n0 + 1 )) $J | grep -qE "$BUSYRE" && MKBUSY=1; return 1; }
  UNWATCH $LEG; echo "  make end: $(show $w)"
  tail -n +$(( n0 + 1 )) $J | head -n $(( w - n0 )) | grep -E "$AGAIN" | head -1 | cut -c1-200 | sed 's/^/  NOTE a later make of the same ask, not this make'"'"'s end: /'
  sed -n "${w}p" $J | grep -qE "$BUSYRE" && { WHY="lamp-tasks refused $1: busy, one is being made"; MKBUSY=1; return 1; }
  sed -n "${w}p" $J | grep -qE "$KEPT" || { WHY="not kept: $(show $w | cut -c1-160)"; return 1; }; MK=$w
  echo "  make time: $(sed -n "${w}p" $J | grep -oE 'kept in [0-9]+ ms')"
  sed -n "$(( n0 + 1 )),${w}p" $J | grep -E "$TRY" | cut -c1-220 | sed 's/^/  try: /'
  r=$(waitfor "$READBACK" $n0 ${RBWAIT:-40}) && { RB=1; echo "  read back: $(show $r)"; saydone $r; PLAY YES2
    y=$(waitfor "$YESOK" $r 60) && echo "  yes: $(show $y)" || echo "  no yes-taken line"; }
  quiet
  # The made line (21 Nov): "Made it. It uses ..., and I tried it: ..." / "Made <name>. It uses ..., and I tried it.",
  # journaled as its length and "uses named" only (SAIDON); its words come from the Mac recording.
  MADELINE=$(tail -n +$(( n0 + 1 )) $J | grep -oE "$SAIDON( \([0-9]+ chars, (no )?uses named\)|: .*)" | head -1)
  echo "  MADELINE ${MADELINE:-none (not in the journal)}"; echo "MADELINE $LEG ${MADELINE:-none}" >> $LG/madelines.txt
  LAMP "" $D/made.lamp.sh > $LG/made-$LEG.txt 2>&1
  # This make's own skill, by name from its journal line ("a made skill is on the lamp: <name>, sha256 ..."); the first
  # unseen made skill only when that line is missing (ee17 23 Nov: an earlier leg's skill was taken for K's).
  ON=$(tail -n +$(( n0 + 1 )) $J | head -n $(( w - n0 + 40 )) | grep -oE 'a made skill is on the lamp: [^ ,]+' | head -1 | sed 's/.*: //')
  if [ -n "$ON" ]; then ML=$(grep "^MADE $ON " $LG/made-$LEG.txt | head -1)
  else ML=$(grep '^MADE ' $LG/made-$LEG.txt | while read -r _ nm rest; do grep -qx "$nm" $LG/seen.names 2>/dev/null || { echo "MADE $nm $rest"; break; }; done); fi
  MN=$(echo "$ML" | awk '{print $2}'); [ -n "$MN" ] && echo "$MN" >> $LG/seen.names
  echo "  $ML" | cut -c1-400; grep "^CARD $MN " $LG/made-$LEG.txt | cut -c1-300 | sed 's/^/  /'
  [ -n "$MN" ] || { WHY="kept line seen but no new made skill found"; return 1; }
  MADECHK; echo "  SAIDON ${MADEU:-?} ${MADEN:-?} chars (grants $(echo "$ML" | field grants))${MADEWHY:+: FAIL$MADEWHY}"; return 0; }
# TRYOK: the make's tried run(s) between the ask and the kept line: each exit 0 and said >= 1
TRYOK(){ local t; t=$(sed -n "1,${MK}p" $J | grep -E "$TRY"); [ -n "$t" ] || return 1
  echo "$t" | grep -qE 'was not run|exit [^0]|limit passed|said (0|unknown)' && return 1
  echo "$t" | grep -qE 'exit 0, .*said [1-9]'; }
: > $LG/seen.names
MADEWHY=; VA=FAIL VB=FAIL VC=FAIL VD=FAIL VE=FAIL VF=FAIL VG=FAIL VK=FAIL VW=FAIL VH=FAIL VP=FAIL VN=FAIL; MNB=; MKB=; MN=; MK=; RB=0; MADELINE=
# Verdicts (and B's price skill) of earlier invocations of this ROW (run.sh save clears it): F, G and P read them.
[ -f $LG/verdicts.env ] && . $LG/verdicts.env
# ---- A: the days-left make (signed out: no account, no net), its card, its try, kept, one run that says its line
legA(){ local n0 e s st rid
  BEGIN A || return 1; quiet; MAKE DAYS || { END; return 0; }
  local why=; [ "$(echo "$ML" | field uses)$(echo "$ML" | field result)$(echo "$ML" | field answer)" = 111 ] || why="$why card lacks a heading (uses/result/answer=$(echo "$ML" | field uses)/$(echo "$ML" | field result)/$(echo "$ML" | field answer));"
  [ "$(echo "$ML" | field stub)" = 0 ] || why="$why the stub line is in its files;"
  [ "$(echo "$ML" | field prints)" = 0 ] || echo "  NOTE scripts/main.py has $(echo "$ML" | field prints) print( line(s) (not the stub; read it)"
  eng=$(echo "$ML" | field engine); [ "$(echo "$ML" | field fm_with)" = "$eng" ] && [ "$(echo "$ML" | field idx_with)" = "$eng" ] || why="$why made_with card=$(echo "$ML" | field fm_with) index=$(echo "$ML" | field idx_with), engine $eng;"
  TRYOK || why="$why no passed tried run (exit 0, said >= 1) before kept;"
  why="$why$MADEWHY"
  sleep 10; quiet; HUSH; n0=$(lines); ASK DAYSASK
  st=$(waitfor "skill run [^ ]+: $MN handed over|skill task [^ ]+: started" $n0 $RUNWAIT)
  rid=$(sed -n "${st:-1}p" $J | sed -n "s/.*skill run \([^ :]*\): $MN handed over.*/\1/p")
  # a run step: its own end, whose "said N" counts its own says (a run's client is c_<hex>, runs.rs:118, so a say line
  # does not name the run), and a say after its start; a task step: the task's end, a say after it
  if [ -n "$rid" ]; then e=$(waitfor "skill run $rid: ended in" $st $RUNWAIT); s=$(waitfor "$SAY" $st $RUNWAIT)
  else e=$(waitfor "skill task [^ ]+: (ended|its agent's answer)" $n0 $RUNWAIT); s=$(waitfor "$SAY" ${e:-$n0} $RUNWAIT); fi
  [ -n "$e" ] && echo "  run end: $(show $e) ($(since $(ep $e) $PEND) s after the clip)"
  [ -n "$s" ] && echo "  first say $(since $(ep $s) $PEND) s after the clip: $(show $s | cut -c1-160)"
  quiet; END
  # its run says its line: the made skill's own start, its end exit 0 with a say (a task: its end), a say spoken
  if [ -z "$st" ]; then why="$why DAYSASK started no run of $MN (answered by the voice alone?);"
  elif [ -z "$e" ]; then why="$why no run/task end line for $MN's run;"
  elif [ -n "$rid" ] && ! RUNOK $e; then why="$why its run did not end exit 0 having said: $(show $e | cut -c1-120);"
  elif [ -z "$s" ]; then why="$why no say after its run started;"; fi
  [ -z "$why" ] && { VA=PASS; WHY="$MN kept, card 3/3, no stub, made_with $eng, try passed$( [ -n "$e" ] && echo ", run $(since $(ep $e) $PEND) s")"; } || WHY="$MN:$why"; }
# ---- B: the price run step: no agent, hand-over ms, first say
legB(){ local n0 h e s a lat
  BEGIN B || return 1; quiet; MAKE PRICE || { END; return 0; }
  [ "$(echo "$ML" | field kind)" = run ] || { WHY="$MN is kind=$(echo "$ML" | field kind), not a run step"; END; return 0; }
  TRYOK || echo "  NOTE no passed try line before kept"
  sleep 10; quiet; CGW B; HUSH; n0=$(lines); ASK PRICEASK
  h=$(waitfor "skill run [^ ]+: hand-over to script start [0-9]+ ms" $n0 $RUNWAIT) || { WHY="no hand-over line in $RUNWAIT s$(NOCG $n0)"; UNCGW B; END; return 0; }
  echo "  $(show $h)"; rid=$(sed -n "${h}p" $J | sed -n 's/.*skill run \([^ :]*\): hand-over.*/\1/p')
  e=$(waitfor "skill run $rid: ended in" $h 60); [ -n "$e" ] && echo "  $(show $e)"
  s=$(waitfor "$SAY" $n0 30); [ -n "$s" ] && { lat=$(since $(ep $s) $PEND); echo "  first say $lat s after the clip's end: $(show $s | cut -c1-160)"; }
  sleep 10; a=$(tail -n +$(( n0 + 1 )) $J | grep -E "$AGENT"); [ -n "$a" ] && echo "$a" | cut -c1-200 | sed 's/^/  AGENT /'
  UNCGW B; quiet; END
  if [ -n "$a" ]; then WHY="an agent session line for the run: $(echo "$a" | head -1 | cut -c1-160)"
  elif [ -n "$MADEWHY" ]; then WHY="$MN:$MADEWHY"
  elif [ -z "$e" ] || ! sed -n "${e}p" $J | grep -qE "exited 0, said [1-9]"; then WHY="run $rid did not end exit 0 with a say: $(show ${e:-1} | cut -c1-160)"
  elif [ -z "$lat" ] || ! python3 -c "import sys;sys.exit(0 if float(sys.argv[1])<=float(sys.argv[2]) else 1)" $lat $LIMIT_B; then WHY="first say ${lat:-none} s after the clip (limit $LIMIT_B)"
  else VB=PASS; WHY="$MN: $(sed -n "${h}p" $J | grep -oE 'hand-over to script start [0-9]+ ms'), no agent session, first say $lat s after the clip"; fi; }
# ---- C: the lemon-probe fixture asks the camera
legC(){ local n0 e x
  BEGIN C || return 1; quiet
  FIXTURE lemon-probe || { WHY="probe not pushed: $(grep -E '^PUSH REFUSED' $LG/probe-lemon-probe.txt | tail -1 | cut -c1-160) (VOID: kit, not the build)"; VC=VOID; END; return 0; }
  sleep 5; for i in 1 2; do HUSH; n0=$(lines); ASKYES PROBE
    e=$(waitfor "refused: no_grant|of skill lemon-probe: (no token made|its time is up)|skill run [^ ]+: ended in|$READBACK" $n0 $RUNWAIT)
    [ -n "$e" ] && echo "  $(show $e)"
    sed -n "${e:-1}p" $J | grep -qE "$READBACK" && { saydone $e; PLAY YES2; sleep 15; continue; }
    break; done
  sleep 8; x=$(tail -n +$(( n0 + 1 )) $J | grep -E "lamp-control: .*refused: no_grant"); tail -n +$(( n0 + 1 )) $J | grep -E "of skill lemon-probe" | cut -c1-200 | sed 's/^/  /'
  quiet; END
  # ee17 23 Nov: "lemon probe" alone drew an answer with no call; a probe the model never ran tests no grant: VOID.
  if [ -n "$x" ]; then VC=PASS; WHY="$(echo "$x" | head -1 | cut -c1-200)"
  elif ! tail -n +$(( n0 + 1 )) $J | grep -qE "of skill lemon-probe|skill (run|task) [^ ]+: hand-over"; then VC=VOID; WHY="the model never ran lemon-probe ($(tail -n +$(( n0 + 1 )) $J | grep -oE 'tools: [^ ]+ calls=[^ ]+' | head -1)): no grant was tested (VOID: the ask, not the build)"
  else WHY="no lamp-control 'refused: no_grant' line after the probe's ask"; fi; }
# ---- D: the mail task step: SKILL.md first, no lamp files, ~30 s. An optional Google leg: its verdict says so.
DNOTE="needs a linked Google account; VOID without one"
legD(){ local n0 st e s t
  BEGIN D || return 1; quiet
  # an unkept mail make is VOID only when save read no usable Google mail link (LINKED google none); linked, it FAILs
  local lk; lk=$(sed -n 's/^LINKED //p' $LG/save.txt 2>/dev/null | head -1)
  MAKE MAIL || { END; case "$lk" in "google none"*) WHY="$WHY (no linked Google mail account: $lk)"; VD=VOID;;
    "") WHY="$WHY (no LINKED line in logs/save.txt: the link not read, so a FAIL)";; *) WHY="$WHY (linked: $lk)";; esac; return 0; }
  [ "$(echo "$ML" | field kind)" = task ] || echo "  NOTE $MN is kind=$(echo "$ML" | field kind), not a task step"
  sleep 10; quiet; SSHR -n $H 'touch /tmp/t8180-markD'; HUSH; n0=$(lines); ASK MAILASK
  st=$(waitfor "skill task [^ ]+: started on the lamp's default agent|skill task [^ ]+: granted net" $n0 $RUNWAIT) && echo "  $(show $st)"
  e=$(waitfor "skill task [^ ]+: (ended|its agent's answer)" $n0 $TASKWAIT) && echo "  $(show $e | cut -c1-200)"
  s=$(waitfor "$SAY" ${e:-$n0} 60)
  [ -n "$e" ] && t=$(since $(ep $e) $PEND) && echo "  answer $t s after the clip; first say after it $( [ -n "$s" ] && since $(ep $s) $PEND ) s"
  quiet; END
  LAMP "SKILL=$MN MARK=/tmp/t8180-markD" $D/task.lamp.sh > $LG/task-D.txt 2>&1; grep -E '^(PROMPT |SESSION|  LAMPPATH)' $LG/task-D.txt | cut -c1-240 | sed 's/^/  /'
  local P; P=$(grep '^PROMPT ' $LG/task-D.txt)
  local lp; lp=$(grep '^SESSION ' $LG/task-D.txt | sed -n 's/.*lamp_paths=\([0-9]*\).*/\1/p' | awk '{s+=$1} END {print s+0}')
  if [ -z "$e" ]; then WHY="no task end/answer line in $TASKWAIT s"
  elif [ -n "$MADEWHY" ]; then WHY="$MN:$MADEWHY"
  elif [ "$(echo "$P" | field card)" != yes ] || [ "$(echo "$P" | field first)" != yes ]; then WHY="the prompt doesn't send its agent to its SKILL.md first: $P"
  elif [ "${lp:-0}" != 0 ]; then WHY="its session names $lp lamp path(s) beyond its prompt's (logs/task-D.txt)"
  elif ! python3 -c "import sys;sys.exit(0 if float(sys.argv[1])<=float(sys.argv[2]) else 1)" $t $LIMIT_D; then WHY="answer $t s after the clip (limit $LIMIT_D)"
  else VD=PASS; WHY="$MN: prompt names SKILL.md first, no lamp files read ($(grep -c '^SESSION ' $LG/task-D.txt) session file(s)), answer $t s"; fi
  grep -q '^SESSIONS none' $LG/task-D.txt && WHY="$WHY; NOTE no session file found to read"; }
# ---- E: LAMP_MAKE_MAY_RUN_ON by a /run drop-in, reversibly
legE(){ local n0 ro q m
  RENEW || return 1
  MAKEIDLE || { MKBUSY=1; WHY="a make in flight after $IDLEWAIT s: the switch not turned on, LONGMAKE not asked"; return 0; }
  GATE lamp-voice || { WHY="start budget (VOID: not run)"; VE=VOID; return 0; }
  LAMP "MODE=on PR_VOICE_SHA=${PR_VOICE_SHA:-}" $D/runon.lamp.sh 2>&1 | tee $LG/runon-on.txt | sed 's/^/  /'; grep -q '^RUNON ON' $LG/runon-on.txt || { WHY="switch not on (VOID: not run; logs/runon-on.txt)"; VE=VOID; return 0; }
  BEGIN E || return 1; quiet; HUSH; n0=$(lines); ASK LONGMAKE
  ro=$(waitfor "the make has no answer [0-9]+ s after its waiting line: it runs on|a long make's turn was cut before its minute: it runs on|$KEPT|$NOTKEPT" $n0 150 "$AGAIN")
  [ -n "$ro" ] && echo "  $(show $ro)"
  if [ -n "$ro" ] && sed -n "${ro}p" $J | grep -qE "runs on"; then saydone $ro; sleep 8; ASK READY
    q=$(waitfor "made skills listed|a make asked while a long make runs on|long make" $ro 60) && echo "  $(show $q)"
    m=$(sed -n "${ro},${q:-$(lines)}p" $J | grep -E "a long make was made|a long make made nothing")
    saydone ${q:-$ro}; echo "  LISTEN: the READY answer must say it is still making (ear check, note it in the ROW)"
  fi
  quiet; END
  GATE lamp-voice || sleep 60
  LAMP "MODE=off PR_VOICE_SHA=${PR_VOICE_SHA:-}" $D/runon.lamp.sh 2>&1 | tee $LG/runon-off.txt | sed 's/^/  /'
  if [ -z "$ro" ] || ! sed -n "${ro}p" $J | grep -qE "runs on"; then WHY="the make didn't run on past its minute: $(show ${ro:-1} | cut -c1-140)"
  elif [ -z "$q" ]; then WHY="READY not answered from the made list"
  elif [ -n "$m" ]; then WHY="the long make ended before READY was answered (VOID: use a longer make)"; VE=VOID
  elif ! grep -q '^RUNON OFF' $LG/runon-off.txt; then WHY="drop-in not reversed"
  else VE=PASS; WHY="ran on, READY answered while making ($(show $q | cut -c1-100)), switch off again"; fi; }
# ---- F: the tried runs' fences (from A, B, D's watches and journals)
legF(){ local ad hid vis eg ec ej f5
  ad=$(cat $LG/fence-[ABD].txt 2>/dev/null | grep -c '^ADMIN '); hid=$(cat $LG/fence-[ABD].txt 2>/dev/null | grep -c '^ADMIN .*admin_sock=hidden'); vis=$(cat $LG/fence-[ABD].txt 2>/dev/null | grep '^ADMIN .*admin_sock=visible')
  eg=$(grep -c '^EGRESS +' $LG/fence-B.txt 2>/dev/null); ec=$(grep -c '^EGRESSCONN' $LG/fence-B.txt 2>/dev/null)
  ej=$(sed -n "1,${MKB:-\$}p" $LG/leg-B.journal.txt 2>/dev/null | grep -E "$EGJ")
  f5=$(grep '^NET ' $LG/fence-B.txt 2>/dev/null | grep -v 'fd5=-' | head -1)
  echo "  admin: $hid of $ad tried pids hidden; B's try: egress sockets +$eg, connections $ec, fd5 '${f5:-none}'"; [ -n "$ej" ] && echo "$ej" | cut -c1-200 | sed 's/^/  EGRESS-J /'
  if [ "$ad" = 0 ]; then WHY="no tried run's pid seen (VOID: try too short for the 0.2 s watch; rerun a make)"; VF=VOID
  elif [ -n "$vis" ]; then WHY="admin.sock visible in a try's fence: $(echo "$vis" | head -1 | cut -c1-160)"
  elif [ "$(grep -c '^TRYPID' $LG/fence-B.txt 2>/dev/null)" = 0 ]; then WHY="admin hidden in $hid tries, but B's net try not seen (VOID for f.2)"; VF=VOID
  elif [ "${eg:-0}" = 0 ] && [ "${ec:-0}" = 0 ] && [ -z "$ej" ] && [ -z "$f5" ]; then WHY="B's net try did NOT reach out (design 21 Nov: a try runs with its grants live): no egress socket, connection, fd5 or journal line"
  else VF=PASS; WHY="admin.sock hidden in all $ad tried pids; B's net try reached out through its egress socket (sockets +$eg, conns $ec, journal $(echo "$ej" | grep -c .), fd5 ${f5:+yes})"; fi
  echo "CHECK (F1) admin.sock hidden in every try: $([ "$ad" != 0 ] && [ -z "$vis" ] && echo PASS || echo FAIL)"
  echo "CHECK (F2) B's net try reaches out by its egress socket: $({ [ "${eg:-0}" != 0 ] || [ "${ec:-0}" != 0 ] || [ -n "$ej" ] || [ -n "$f5" ]; } && echo PASS || echo FAIL)"; }
# ---- G (Reviewer-lead 04:23): the run's process cap counts the run's own tasks only (read-only ssh)
legG(){ local k pl n cg sv up bad gone left nc
  k=$(SSHR -n $H 'uname -r'); pl=$(SSHR -n $H 'command -v prlimit'); n=$(SSHR -n $H "ps -eLo user= | grep -c '^lampos'")
  echo "  kernel $k; prlimit '${pl:-none}'; lampos tasks now $n; A $VA, B $VB"
  # #8180 cgroup, from logs/cg-B.txt (B's run) and every leg's journal
  cg=$LG/cg-B.txt
  sv=$(grep '^SERVELEAF ' $cg 2>/dev/null | sed -n 's/.* leaf=\([^ ]*\).*/\1/p' | sort -u | tr '\n' ' ')
  up=$(grep -c '^RUNLEAF +run-' $cg 2>/dev/null); up=${up:-0}
  bad=$(grep '^RUNLEAF +run-' $cg 2>/dev/null | grep -vE 'memory\.max=[0-9]+ ' | head -1)
  [ -z "$bad" ] && bad=$(grep '^RUNLEAF +run-' $cg 2>/dev/null | grep -vE 'memory\.swap\.max=0 ' | head -1)
  [ -z "$bad" ] && bad=$(grep '^RUNLEAF +run-' $cg 2>/dev/null | grep -vE 'cpu\.max=[0-9]+_[0-9]+ ' | head -1)
  [ -z "$bad" ] && bad=$(grep '^RUNLEAF +run-' $cg 2>/dev/null | grep -vE 'pids\.max=[0-9]+ ' | head -1)
  # the exact values of run_cgroup.rs (5a1bdffd20): 512M, 448M, swap 0, one core, 72 tasks
  [ -z "$bad" ] && bad=$(grep '^RUNLEAF +run-' $cg 2>/dev/null | grep -vE 'memory\.max=536870912 memory\.high=469762048 memory\.swap\.max=0 cpu\.max=100000_100000 pids\.max=72 ' | head -1)
  grep -q "each run gets its own cgroup under" $LG/leg-*.journal.txt $D/logs/verify.txt 2>/dev/null && echo "CHECK (1a) own-cgroup start line: PASS" || echo "CHECK (1a) own-cgroup start line: FAIL (not in verify or the legs)"
  grep -qE "runs have no own cgroup|no own cgroup" $LG/leg-*.journal.txt 2>/dev/null && echo "CHECK (1b) no 'no own cgroup' line: FAIL" || echo "CHECK (1b) no 'no own cgroup' line: PASS"
  grep -q '^CGROUP OK serve in its own leaf' $D/logs/verify.txt 2>/dev/null && ! grep -q 'procs-in-unit-root' $D/logs/verify.txt && echo "CHECK (2) serve holds MainPID, unit root procs empty: PASS" || echo "CHECK (2) serve holds MainPID, unit root procs empty: FAIL (row/logs/verify.txt)"
  gone=1; for r in $(grep '^RUNLEAF +run-' $cg 2>/dev/null | awk '{print substr($2,2)}'); do grep -q "^RUNLEAF -$r gone" $cg || gone=0; done
  [ "$up" != 0 ] && [ -z "$bad" ] && [ "$gone" = 1 ] && echo "CHECK (3) run-* leaf with 512M/448M/0/100000 100000/72, gone after: PASS" || echo "CHECK (3) run-* leaf with exact caps, gone after: FAIL"
  [ "$VB" = PASS ] && echo "CHECK (4) the price run skill answers: PASS" || echo "CHECK (4) the price run skill answers: FAIL (leg B $VB)"
  left=$(sed -n 's/^CGEND left=\([^ ]*\).*/\1/p' $cg 2>/dev/null)
  nc=$(cat $LG/leg-*.journal.txt 2>/dev/null | grep -cE 'refused, no own cgroup|not handed down|not written: ')
  echo "  cgroup: serve leaf '${sv:-?}', run leaves seen $up, all gone $gone, left '${left:-}', no-cgroup refusals $nc"
  if [ ! -s "$cg" ] && [ "$VB" = VOID ]; then WHY="no cgroup watch of B's run: B VOID, not reached (VOID)"; VG=VOID
  elif [ ! -s "$cg" ]; then WHY="no cgroup watch of B's run (logs/cg-B.txt): B did not reach its ask"
  elif ! echo " $sv" | grep -qE '^ (serve|main) $'; then WHY="serve's pid not in its own leaf (leaf '${sv:-none}')"
  elif [ "$up" = 0 ]; then WHY="no run-* leaf seen under lamp-tasks' cgroup during B's run ($(grep -c '^RUNLEAF ~' $cg) made with no process)"
  elif [ -n "$bad" ]; then WHY="a run leaf without its caps: $(echo "$bad" | cut -c1-200)"
  elif [ "$gone" != 1 ] || [ -n "$left" ]; then WHY="a run leaf not removed after its run (left '${left:-}')"
  elif [ "${nc:-0}" != 0 ]; then WHY="$nc 'no own cgroup' refusal line(s) in the legs' journals"
  elif [ -z "$pl" ]; then WHY="no prlimit in the image"
  elif [ "$(printf '5.14\n%s\n' "${k%%-*}" | sort -V | head -1)" != 5.14 ]; then WHY="kernel $k is older than 5.14"
  elif [ "${n:-0}" -le 64 ]; then WHY="only $n lampos tasks (VOID: cap not shown per run)"; VG=VOID
  elif [ "$VA" = FAIL ] || [ "$VB" = FAIL ]; then WHY="a try or the run step failed (A $VA, B $VB) with $n lampos tasks"
  elif [ "$VA" != PASS ] || [ "$VB" != PASS ]; then WHY="A $VA, B $VB: not shown, so not (4) (VOID)"; VG=VOID
  else VG=PASS; WHY="serve in leaf $sv; $up run leaf/leaves in B's run with $(grep -m1 '^RUNLEAF +run-' $cg | grep -oE 'memory\.max=[^ ]+ .*pids\.max=[^ ]+' | cut -c1-140), gone after; kernel $k, prlimit at $pl, $n lampos tasks > 64, A and B passed"; fi; }

# ---- K (21 Nov): a skill that uses the camera (or the mic) and the speaker: made, tried and run, no yes asked
# A token's grants as lamp-control journals them (control/src/server.rs:404 a run's, :459 a try's, by grant_names
# :2202): the wire names of protocol.rs's Grant (the camera is "camera", the mic "microphone"; "look" and "hear" are
# presence and own-phrase grants, not the camera or the mic). K wants exactly KG, sorted, nothing more.
KGSET=${KG:+1}; KG=${KG:-camera,speak}
GSET(){ grep -oE 'a token for \[[^]]*\]' | head -1 | sed 's/^a token for \[//; s/\]$//' | tr -d ' ' | tr ',' '\n' | sort | paste -sd, -; }
legK(){ local n0 w tt rt s rb2 e k1 k2 k3 k4 k5
  BEGIN K || return 1; quiet; MAKE LOOK || { END; echo "CHECK (K1) made and kept: FAIL ($WHY)"; return 0; }
  tt=$(sed -n "$(( MK0 + 1 )),${MK}p" $J | grep -E "a make's tried run, a token for \[" | head -1)
  sleep 10; quiet; HUSH; n0=$(lines); ASK LOOKASK
  rt=$(waitfor "run [^ ]+ of skill $MN: a token for \[" $n0 $RUNWAIT)
  e=$(waitfor "skill run [^ ]+: ended in|skill task [^ ]+: (ended|its agent's answer)" $n0 $RUNWAIT)
  s=$(waitfor "$SAY" $n0 $RUNWAIT); rb2=$(tail -n +$(( n0 + 1 )) $J | grep -E "$READBACK|$YESOK" | head -1)
  [ -n "$rt" ] && echo "  $(show $rt)"; [ -n "$e" ] && echo "  $(show $e)"; [ -n "$s" ] && echo "  first say $(since $(ep $s) $PEND) s after the clip: $(show $s | cut -c1-140)"
  quiet; END
  k1=PASS; k2=FAIL; k3=FAIL; k4=FAIL; k5=FAIL
  # The tokens must be exactly the made skill's own grants (least privilege), and those must hold the camera or the
  # mic. A run step speaks by its own say (speak among its grants); a task step speaks by its agent's answer, so its
  # grants need no speak (ee17 23 Nov: camera-look, kind task, grants camera). KG, when set, still overrides.
  local kg; kg=$(echo "$ML" | field grants | tr ',' '\n' | sort | paste -sd, -); [ "${KGSET:-}" = 1 ] && kg=$KG
  case ",$kg," in *,camera,*|*,microphone,*) ;; *) k1="FAIL:grants [$kg] hold no camera or mic";; esac
  case "$(echo "$ML" | field kind)" in run|both) case ",$kg," in *,speak,*) ;; *) k1="FAIL:a run step without speak";; esac;; esac
  [ -n "$tt" ] && [ "$(echo "$tt" | GSET)" = "$kg" ] && k2=PASS; [ -n "$rt" ] && [ "$(show $rt | GSET)" = "$kg" ] && k3=PASS
  [ -n "$s" ] && { [ -z "$e" ] || sed -n "${e}p" $J | grep -qvE "exited [1-9]|said 0"; } && k4=PASS
  [ "$RB" = 0 ] && [ -z "$rb2" ] && k5=PASS
  echo "CHECK (K1) $MN made, kept, its said-on line right: $k1 (kind $(echo "$ML" | field kind), grants $(echo "$ML" | field grants))"
  echo "CHECK (K2) its try's token is exactly its grants [$kg]: $k2 ${tt:+($(echo "$tt" | grep -oE 'a token for \[[^]]*\]'))}"
  echo "CHECK (K3) its run's token is exactly its grants [$kg]: $k3 ${rt:+($(show $rt | grep -oE 'a token for \[[^]]*\]'))}"
  echo "CHECK (K4) the run's say was spoken: $k4"
  echo "CHECK (K5) no yes asked (make or run): $k5"
  [ -n "$MADEWHY" ] && k1="FAIL:$MADEWHY"
  [ "$k1$k2$k3$k4$k5" = PASSPASSPASSPASSPASS ] && { VK=PASS; WHY="$MN (kind $(echo "$ML" | field kind)): try and run tokens exactly its grants [$kg], said, no yes"; } || WHY="$MN: K1 $k1 K2 $k2 K3 $k3 K4 $k4 K5 $k5"; }
# ---- W (#8130, folded in): tools.deny holds skill_workshop, OpenClaw accepts the file, workshop-skills/ unchanged
wpre(){ LAMP "MODE=hash" $D/wshop.lamp.sh > $LG/wshop-before.txt 2>&1; echo "  workshop before: $(grep '^WSHOP ' $LG/wshop-before.txt)"; }
legW(){ local b a
  LAMP "MODE=config" $D/wshop.lamp.sh > $LG/wshop-config.txt 2>&1; LAMP "MODE=hash" $D/wshop.lamp.sh > $LG/wshop-after.txt 2>&1
  grep -E '^(DENY|GATEWAY|NOTE|CHECK)' $LG/wshop-config.txt | cut -c1-220
  b=$(grep '^WSHOP ' $LG/wshop-before.txt 2>/dev/null); a=$(grep '^WSHOP ' $LG/wshop-after.txt)
  echo "  workshop before '$b' after '$a' (legs between: ${LEGS:-A B C D E K H P})"
  [ -n "$b" ] && [ "$b" = "$a" ] && echo "CHECK (W3) a make plus a run leave workshop-skills/ unchanged: PASS" || echo "CHECK (W3) a make plus a run leave workshop-skills/ unchanged: FAIL ('$b' -> '$a')"
  grep -c ': PASS' $LG/wshop-config.txt | grep -qx 2 && [ -n "$b" ] && [ "$b" = "$a" ] && { VW=PASS; WHY="skill_workshop denied, gateway accepts it, workshop-skills unchanged ($a)"; } || WHY="$(grep -h '^CHECK (W[12])' $LG/wshop-config.txt | grep -v ': PASS' | cut -c1-90 | tr '\n' ';')$([ -n "$b" ] && [ "$b" = "$a" ] || echo " W3 workshop-skills '$b' -> '$a'")"; }
# a row fixture (row/probe/<name>) for C, H and P, by the product's push install (push.sh: the SDK's package, staged as
# lamp-control's door keeps a push, then the manage helper's install_pushed; the network door skipped). A hand install
# is refused on the lamp: a script needs a Store or pushed skill (skills task.rs). unrow takes it off (remove_pushed).
FIXTURE(){ bash $D/push.sh push $D/probe/$1 > $LG/probe-$1.txt 2>&1; sed 's/^/  /' $LG/probe-$1.txt; grep -q '^PROBE INSTALLED' $LG/probe-$1.txt; }
# NOCG n0: the refusal for want of an own cgroup after line n0, if any (run_cgroup.rs NO_CGROUP / for_run_on)
NOCG(){ local x; x=$(tail -n +$(( $1 + 1 )) $J | grep -oE "run [^ ]+: refused, no own cgroup: .*|the run's own memory and CPU bounds could not be set, so it was not run|runs have no own cgroup: [^;]*" | head -1 | cut -c1-160); [ -n "$x" ] && echo ": REFUSED, $x"; }
RUNOK(){ sed -n "${1}p" $J | grep -qE "exited 0, said [1-9]"; }
# a run step's start (skill_run.rs:609); its "hand-over to script start N ms" line comes only after its script ended
STARTED="skill run [^ ]+: [^ ]+ handed over, token minted in [0-9]+ ms"
# ---- H (Reviewer-lead, 5a1bdffd20): lamp-tasks restarted while a run step's script runs
legH(){ local n0 h rid e2 h2 rid2 x h1 h2c h3 h4 old
  BEGIN H || return 1; quiet
  FIXTURE slow-probe || { WHY="slow-probe fixture not installed (VOID: kit)"; VH=VOID; END; return 0; }
  GATE lamp-tasks || { WHY="start budget (lamp-tasks) (VOID: not run)"; VH=VOID; END; return 0; }
  CGSECS=150 CGW H; HUSH; n0=$(lines); ASKYES SLOW
  h=$(waitfor "$STARTED" $n0 $RUNWAIT) || { UNCGW H; WHY="no run start (handed over) for slow-probe$(NOCG $n0)"; END; return 0; }
  rid=$(sed -n "${h}p" $J | sed -n 's/.*skill run \([^ :]*\): .*/\1/p'); echo "  $(show $h)"
  for x in $(seq 1 20); do grep -q '^RUNLEAF +run-' $LG/cg-H.txt && break; sleep 0.5; done
  old=$(grep -m1 '^RUNLEAF +run-' $LG/cg-H.txt | awk '{print substr($2,2)}'); echo "  run leaf before the restart: ${old:-none seen}"
  sleep 3; LAMP "" $D/restart-tasks.lamp.sh 2>&1 | tee $LG/restart-H.txt | sed 's/^/  /'
  sleep 25; UNCGW H
  tail -n +$(( n0 + 1 )) $J | grep -E "skill run $rid:" | cut -c1-200 | sed 's/^/  old run: /'
  h1=FAIL; [ -n "$old" ] && grep -q "^RUNLEAF -$old gone" $LG/cg-H.txt && h1=PASS
  h2c=FAIL; grep -qE '^AFTER .* leaf=serve .*subtree=[a-z,]*memory' $LG/restart-H.txt && grep -q 'subtree=[a-z,]*cpu' $LG/restart-H.txt && grep -q 'subtree=[a-z,]*pids' $LG/restart-H.txt \
     && grep -q 'mainpid_in_serve=yes' $LG/restart-H.txt && grep -q 'root_procs=\[\]' $LG/restart-H.txt && h2c=PASS
  h3=FAIL; grep -q '^J .*each run gets its own cgroup under' $LG/restart-H.txt && ! grep -qE '^J .*(runs have no own cgroup|refused, no own cgroup|could not be set)' $LG/restart-H.txt && h3=PASS
  # a fresh run step after it: slow-probe again (its own skill, so H needs no other leg)
  quiet; HUSH; n0=$(lines); ASK SLOW
  h2=$(waitfor "skill run [^ ]+: hand-over to script start [0-9]+ ms" $n0 60); rid2=$(sed -n "${h2:-1}p" $J | sed -n 's/.*skill run \([^ :]*\): hand-over.*/\1/p')
  e2=; [ -n "$rid2" ] && e2=$(waitfor "skill run $rid2: ended in" $h2 60); [ -n "$e2" ] && echo "  fresh run: $(show $e2)"
  h4=FAIL; [ -n "$e2" ] && RUNOK $e2 && h4=PASS
  quiet; END
  echo "CHECK (H1) the old run-* leaf ($old) is gone after the restart: $h1"
  echo "CHECK (H2) serve back in serve, controllers in subtree_control, unit root empty: $h2c ($(grep '^AFTER .*leaf=' $LG/restart-H.txt | grep -oE 'leaf=[^ ]+ subtree=[^ ]+'))"
  echo "CHECK (H3) the own-cgroup set-up line is back, no NO_CGROUP / Missing line: $h3"
  echo "CHECK (H4) a fresh run step passes after it: $h4"
  [ "$h1$h2c$h3$h4" = PASSPASSPASSPASS ] && { VH=PASS; WHY="restart mid-run: old leaf gone, serve back, set-up line back, fresh run passed"; } || WHY="H1 $h1 H2 $h2c H3 $h3 H4 $h4"; }
# ---- P (Heron (5)): a script past its process cap is refused while a second task answers
legP(){ local n0 h rid e n1 h2 rid2 e2 pe fp p1 p2 ov
  BEGIN P || return 1; quiet
  FIXTURE fork-probe || { WHY="fork-probe fixture not installed (VOID: kit)"; VP=VOID; END; return 0; }
  [ -n "$MNB" ] || { WHY="no price skill kept by leg B for the second task (run B first)"; VP=VOID; END; return 0; }
  CGSECS=150 CGW P; HUSH; n0=$(lines); ASKYES FORK
  h=$(waitfor "$STARTED" $n0 $RUNWAIT) || { UNCGW P; WHY="no run start (handed over) for fork-probe$(NOCG $n0)"; END; return 0; }
  rid=$(sed -n "${h}p" $J | sed -n 's/.*skill run \([^ :]*\): .*/\1/p'); echo "  fork run $rid: $(show $h)"
  sleep 2; n1=$(lines); ASK PRICEASK
  h2=$(waitfor "$STARTED" $n1 $RUNWAIT); rid2=$(sed -n "${h2:-1}p" $J | sed -n 's/.*skill run \([^ :]*\): .*/\1/p')
  [ "$rid2" = "$rid" ] && rid2=
  e=$(waitfor "skill run $rid: ended in" $h 60); e2=; [ -n "$rid2" ] && e2=$(waitfor "skill run $rid2: ended in" ${h2:-$n1} 60)
  sleep 5; UNCGW P; quiet; END
  [ -n "$e" ] && echo "  fork run end: $(show $e)"; [ -n "$e2" ] && echo "  second run end: $(show $e2)"
  pe=$(grep '^RUNLEAF -run-' $LG/cg-P.txt | sed -n 's/.*pids.events.max=\([0-9]*\).*/\1/p' | sort -n | tail -1)
  fp=$(grep -ohE "FORKPROBE started=[0-9]+ refused=[0-9]+" $J | tail -1)
  ov=no; [ -n "$h2" ] && [ -n "$e" ] && [ "$h2" -lt "$e" ] && ov=yes
  p1=FAIL; { [ "${pe:-0}" -gt 0 ] || echo "$fp" | grep -qE 'refused=[1-9]'; } && p1=PASS
  p2=FAIL; [ -n "$e2" ] && RUNOK $e2 && p2=PASS
  echo "CHECK (P1) the fork script past its cap is refused: $p1 (pids.events max ${pe:-?}; ${fp:-no FORKPROBE line in the journal})"
  echo "CHECK (P2) a second task answers meanwhile: $p2 (overlapped the fork run: $ov)"
  [ "$p1$p2" = PASSPASS ] && { VP=PASS; WHY="fork refused at the cap (max ${pe:-?}), the price run answered (overlap $ov)"; } || WHY="P1 $p1 P2 $p2"; }
# ---- N (#7269): a line naming an agent for the work, LAMP_NAMED_AGENT_TAKES_LINE off (as on stable), on, off again
HANDED="the heard words named [^ ]+ for the work: round 1 hands them over in code \\(LAMP_NAMED_AGENT_TAKES_LINE\\)"
TASKMAX(){ LAMP "MODE=task" $D/named.lamp.sh 2>/dev/null | sed -n 's/^TASKMAX //p' | head -1; }
NTASKS(){ LAMP "MODE=task SINCE=${1:-0} CLIP=$2" $D/named.lamp.sh > $LG/named-task-$2.txt 2>&1; grep -E '^TASKS? ' $LG/named-task-$2.txt | cut -c1-200 | sed 's/^/  /'; }
legN(){ local n0 n1 h1 h2 s1 t0 t1 n1c=FAIL n2c=FAIL n3c=FAIL
  RENEW || return 1
  LAMP "MODE=read" $D/named.lamp.sh > $LG/named-read.txt 2>&1; grep -E '^(NAMED|ENGINE|MUTE)' $LG/named-read.txt | cut -c1-200 | sed 's/^/  /'
  grep -q 'ENGINE default=openclaw' $LG/named-read.txt || echo "  NOTE the lamp's default agent does not read openclaw (the clip names OpenClaw)"
  rm -f $LG/named-on.txt $LG/named-off.txt
  if grep -q "^NAMED env='' dropin=absent" $LG/named-read.txt; then echo "  switch off already: no restart"
  else GATE lamp-voice || { WHY="start budget (VOID: not run)"; VN=VOID; return 0; }
    : > $LG/named-on.txt; NOFF || { WHY="switch not off before the leg (VOID: logs/named-off.txt)"; VN=VOID; return 0; }
    rm -f $LG/named-on.txt $LG/named-off.txt; fi
  BEGIN N || return 1; quiet
  # N1: off, as on stable: no hand-over in code; the voice's own tool answers
  t0=$(TASKMAX); HUSH; n0=$(lines); ASK NAMED
  h1=$(waitfor "$HANDED" $n0 ${NWAIT:-40}); s1=$(waitfor "$SAY" $n0 5)
  [ -n "$h1" ] && echo "  OFF but handed: $(show $h1)"; [ -n "$s1" ] && echo "  off: answered: $(show $s1 | cut -c1-160)" || echo "  NOTE off: no say/pieces line for the ask"
  quiet; NTASKS ${t0:-0} NAMED-off
  [ -z "$h1" ] && n1c=PASS
  # N2: on: the heard words handed to the named agent in code (the voice's own line)
  GATE lamp-voice || { END; WHY="start budget before the switch on (VOID: N1 $n1c)"; VN=VOID; return 0; }
  : > $LG/named-on.txt; LAMP "MODE=on" $D/named.lamp.sh 2>&1 | tee $LG/named-on.txt | grep -E '^(NAMED|NOTE|ENGINE)' | cut -c1-200 | sed 's/^/  /'
  if ! grep -q '^NAMED ON' $LG/named-on.txt; then END; GATE lamp-voice || sleep 60; NOFF; WHY="switch not on (VOID: logs/named-on.txt; N1 $n1c)"; VN=VOID; return 0; fi
  sleep 5; quiet; t1=$(TASKMAX); HUSH; n1=$(lines); ASK NAMED
  h2=$(waitfor "$HANDED" $n1 ${NWAIT:-40}); [ -n "$h2" ] && { echo "  on: $(show $h2)"; n2c=PASS; }
  saydone ${h2:-$n1}; quiet; NTASKS ${t1:-0} NAMED-on
  # N3: off again
  GATE lamp-voice || sleep 60
  NOFF && n3c=PASS; grep -E '^(NAMED|ENGINE)' $LG/named-off.txt | cut -c1-200 | sed 's/^/  /'
  END
  echo "CHECK (N1) off (as on stable): NAMED not handed to the agent in code: $n1c"
  echo "CHECK (N2) on: the voice's hand-over line ('round 1 hands them over in code'): $n2c"
  echo "CHECK (N3) the switch off again (NAMED OFF, drop-in gone): $n3c"
  [ "$n1c$n2c$n3c" = PASSPASSPASS ] && { VN=PASS; WHY="off: not handed; on: $(show $h2 | grep -oE 'request [0-9]+: the heard words named [^ ]+'); off again"; } || WHY="N1 $n1c N2 $n2c N3 $n3c"; }
LEGS=${LEGS-A B C D K H P N E}; TAILS=${TAILS-F G}
RT=$LG/row-$(echo $LEGS $TAILS | tr -d ' ').txt     # this invocation's lines; row.txt keeps every invocation's
echo "== row.sh $LEGS $TAILS on $LAMP $(date '+%F %T')" | tee $RT >> $LG/row.txt
case " $LEGS $TAILS " in *" W "*) { RENEW || exit 1; wpre; } 2>&1 | tee -a $RT;; esac
for g in $LEGS $TAILS; do RENEW || { echo "LOCK lost before leg $g: stop"; exit 1; }; WHY=; ASKN=; MKBUSY=; J=/dev/null
  case $g in A) legA; POST A; echo "LEG A $VA ($WHY)";; B) legB; POST B; MKB=$MK; [ -n "$MK" ] && [ -n "$MN" ] && MNB=$MN; echo "LEG B $VB ($WHY)"; printf 'MNB=%s\nMKB=%s\n' "$MNB" "$MKB" >> $LG/verdicts.env;; C) legC; POST C; echo "LEG C $VC ($WHY)";;
    D) legD; POST D; echo "LEG D $VD ($WHY) [$DNOTE]";; E) legE; POST E; echo "LEG E $VE ($WHY)";; F) legF; POST F; echo "LEG F $VF ($WHY)";; G) legG; POST G; echo "LEG G $VG ($WHY)";;
    K) legK; POST K; echo "LEG K $VK ($WHY)";; H) legH; POST H; echo "LEG H $VH ($WHY)";; P) legP; POST P; echo "LEG P $VP ($WHY)";; W) legW; POST W; echo "LEG W $VW ($WHY)";;
    N) legN; POST N; echo "LEG N $VN ($WHY)";; *) echo "no leg $g"; continue;; esac
  eval "echo V$g=\$V$g" >> $LG/verdicts.env
  sleep ${BETWEEN:-8}; done 2>&1 | tee -a $RT
cat $RT | tail -n +2 >> $LG/row.txt
want=$(echo $LEGS $TAILS | wc -w | tr -d ' '); v=$(grep -c '^LEG [A-Z] PASS' $RT)
vf=$(grep -c '^LEG [A-Z] FAIL' $RT); vv=$(grep -c '^LEG [A-Z] VOID' $RT)
if [ "$v" = "$want" ]; then R=PASS; elif [ "$vf" = 0 ] && [ "$vv" != 0 ]; then R=VOID; else R=FAIL; fi   # VOID: no leg failed, one or more not shown
echo "VERDICT $R ($v of $want legs PASS, $vf FAIL, $vv VOID; legs $LEGS $TAILS; ROW #$PR on $LAMP, the runner's install of #8180's artifact; journals row/logs/leg-*.journal.txt)" | tee -a $RT >> $LG/row.txt
grep '^VERDICT' $RT
[ $R = PASS ]
