#!/bin/bash
# Mac. #7901 ROW legs on $LAMP (row/kit.sh settings), on the runner's install of #7901's CI ARM64 artifact (a real
# round from a test channel). Run by ../run.sh's `legs` step, after `check` (logs/check.txt VERIFY PASS and READY),
# `notes`, `micon` and `save` (logs/save.txt SAVED OK); a lock is held and renewed when KIT_LL is set. Hands-free:
# pre-rendered ElevenLabs clips from the Mac speaker (afplay -v gain only; Mac volume read, <= 70; no volume is ever
# set). Verdicts from each leg's journal (logs/leg-<L>.journal.txt) and read-only lamp scripts (state, fence).
# #7901's scope: a made skill may hold net beside the camera, notes and every other grant; the net-beside-private
# refusal is gone everywhere. admin.sock (skill_run::HIDDEN) stays hidden from a run; the per-run cgroup stays.
#   A  MAKEA "make a skill that checks the price of coffee beans online and keeps notes of the prices it found":
#      kept, with net AND notes, on the lamp, and no net-beside-private refusal anywhere in the journal.
#   B  RUN "run the coffee price skill", twice: each run's lamp-control token names notes (net is never a
#      lamp-control grant: it is the skill's grant and lamp-tasks' egress), each run ends with no refusal; run 1
#      leaves the skill's notes file (n_<16 hex>.json), run 2 starts with it there and its code reads it back
#      (lamp.notes.get).
#   C  (flipped from the old scope) MAKEC "make a skill that looks at my desk with the camera and searches online for
#      what it sees": KEPT, one skill holding net AND camera, on the lamp; no net-beside-private refusal and no split
#      ("make it two skills") anywhere in the journal.
#   D  RUNC runs C's skill, watched from outside its fence (fence.lamp.sh): admin.sock not visible to the run, the run's
#      token names camera, its net reaches out by its egress socket, it ends with no refusal, and it runs in its own
#      cgroup leaf under lamp-tasks.
#   C6 (First, 16:06): C's make goes on with nothing asked (#8180's at-once path, no YES played); its tried line names
#      what it uses ("It uses the camera and the internet", journaled as "uses named", never its words).
#   S  a PUSHED skill (fixtures/photo-search: grants camera + net, a do.task; not made on the lamp, so its grants wait
#      for the owner's kept yes), put on by push.py from the runner's paired Mac. RUNS asks it to run: the yes is asked
#      aloud with the line saying what can go out (ASKED_FILE watched: "camera ... can go to the internet"); NO: not
#      kept, no token. USES asks it again (its read-back); YES: kept. RUNS again: the run's token is exactly [camera]
#      (net is never a lamp-control grant: it is carried as the run's egress, "granted net: its egress socket"), it
#      reaches out, and its try at the notes is refused no_grant (no skill reads another's notes).
#   K  a PUSHED skill whose task needs a key never added in the app (fixtures/key-probe, secrets): after its yes, its
#      run is refused by lamp-tasks before any token, with its journal line (keys stay refused).
#      Its fixture goes in by the product's push install with no pairing (push.sh: the SDK's package staged as
#      lamp-control's door keeps a push, then the manage helper's install_pushed); the network door is skipped.
#   ./row.sh [legs]   (default A B C D S K). Each leg prints CHECK lines and LEG x PASS|FAIL|VOID; then VERDICT.
#   VOID = could not be shown (clip not heard, no make end, a fixture missing); FAIL = shown wrong. Exit 0 on PASS.
set -u; D=$(cd "$(dirname "$0")" && pwd); . $D/kit.sh
LG=$D/logs
[ $# -gt 0 ] && LEGS="$*"
LEGS=${LEGS:-A B C D S K}
RENEW || exit 1; VOLOK || exit 2
grep -q '^VERIFY PASS' $LG/check.txt 2>/dev/null || { echo "no VERIFY PASS in row/logs/check.txt: #7901's build is not verified running (../run.sh check)"; exit 1; }
grep -q '^READY' $LG/check.txt 2>/dev/null || { echo "no READY in row/logs/check.txt (../run.sh check)"; exit 1; }
grep -q '^SAVED OK' $LG/save.txt 2>/dev/null || { echo "no SAVED OK in row/logs/save.txt (../run.sh save first)"; exit 1; }
for c in MAKEA RUN MAKEC RUNC YES RUNS USES NO KEYUSES KEYRUN; do [ -f $A/$c-el.wav ] || { echo "no clip $A/$c-el.wav: ../run.sh render"; exit 1; }; done
JP=; FP=; trap '[ -n "$JP" ] && kill $JP 2>/dev/null; [ -n "$FP" ] && kill $FP 2>/dev/null' EXIT; trap 'exit 130' INT TERM HUP
POLL=${POLL:-1}; MAKEWAIT=${MAKEWAIT:-660}; RUNWAIT=${RUNWAIT:-120}; ENDWAIT=${ENDWAIT:-300}
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
KEPT="lamp-voice: a skill made here was kept in [0-9]+ ms"
NOTKEPT="a skill made here wasn't kept in|a skill wasn't made|the lamp's model said no made skill can do it|wasn't kept: |was not kept: "
READBACK="read back: it waits for the person's yes|said back unasked waits for the [a-z]+'s yes|with no yes is said back unasked"
YESOK="is on, on the person's yes|approved on the person's yes"
YESNOT="a made skill's yes not (taken|kept)"
ON="a made skill is on the lamp: "
HEARD="lamp-voice.*request [0-9]+|$KEPT|$NOTKEPT"
# The net-beside-private refusal as origin/lamp-voice journaled it before #7901 (each gone from #7901's code):
#   skill_task_yes.rs / skill_task.rs NET_BESIDE_PRIVATE, lamp-skills' load (task.rs), lamp-control's yes (task_yes.rs),
#   upload_agent.rs net_beside_private (made_skill.rs's scripts_not_held), make_no_cannot.rs Uncovered::NetPrivate
#   (its journal class net_private, and its spoken line).
OLDREF="a skill that reaches the internet can't also use|grants: \"net\" beside|grants: net beside|and one skill can't do both|cannot was not made a task for the lamp's agent: net_private|one skill can't have both"
SPLIT="make it two skills|[Aa]sk me for two skills|two skills instead"
# A run's refusal (lamp-control: no token, or a call refused) and the egress lines of a run granted net.
RUNREF="no token made|lamp-control: .*: refused: |lamp-control: .* refused: "
EGJ="egress ready in its fence|granted net: its egress socket|\[lamp-tasks\] egress: "
BEGIN(){ LEG=$1; J=$LG/leg-$1.journal.txt; echo "== leg $1 $(date +%T)"; RENEW || return 1; : > $J; JP=$(JF $J); sleep 3; }
END(){ sleep ${TAIL:-8}; kill $JP 2>/dev/null; JP=; }
STATE(){ LAMP "STAGE=$1" $D/state.lamp.sh > $LG/state-$1.txt 2>&1; sed 's/^/  /' $LG/state-$1.txt | grep -v '^  STAGE' | cut -c1-400; }
WATCH(){ SSHR -n $H "rm -f /tmp/t7901-stop.$1"; LAMP "SECS=$(( RUNWAIT + ENDWAIT + 120 )) STOPF=/tmp/t7901-stop.$1" $D/fence.lamp.sh > $LG/fence-$1.txt 2>&1 & FP=$!; }
UNWATCH(){ SSHR -n $H "touch /tmp/t7901-stop.$1"; sleep 2; kill $FP 2>/dev/null; FP=; grep -E '^(RUNPID|ADMIN|NET|CG|EGRESS)' $LG/fence-$1.txt | cut -c1-220 | sed 's/^/  fence /'; }
# #8180 (LAMP_MADE_CODE_ON_AT_ONCE, on in deploy/product's lamp-voice and lamp-control): a made skill with code of its
# own that is not a task for the lamp's agent goes on at once, with no read-back and no yes (make_skill_local.rs
# code_on_at_once). Its lines come before the kept line: "is on at once (...); its task kept|not kept", "a made skill
# is on the lamp: <name>", and, when a try ran, the tried line "Made it..., and I tried it: <the try's first words>",
# journaled by its length only, never its words (made_skill.rs tried_journal). Otherwise (the switch off, or an agent
# task) the read-back waits for the yes, as before. The try ([lamp-tasks] make try, make_try.rs) ends exit 0 having
# said >= 1: its say is answered at once by lamp-control (f45ad0f9ec) and said after the make in the tried line.
ATONCE="is on at once \(LAMP_MADE_CODE_ON_AT_ONCE\); its task (kept|not kept)"
TRY="\[lamp-tasks\] make try: "
SAIDON="lamp-voice: a made skill is said on with its try \([0-9]+ chars, (no )?uses named\)"
# MAKE <clip>: the voice make, then its path: at-once (no yes played) or read-back + yes. Sets MSTAT
# (unheard|noend|notkept|kept), MW, Y, O, MPATH (at-once|yes|none), AT (at-once line), TRYL (try lines), SO (tried line).
MAKE(){ local n0 rb; MSTAT=; MW=; Y=; O=; MPATH=none; AT=; TRYL=; SO=; n0=$(lines); PLAY $1
  waitfor "$HEARD" $n0 ${HEARWAIT:-60} >/dev/null || { MSTAT=unheard; return 1; }
  MW=$(waitfor "$KEPT|$NOTKEPT" $n0 $MAKEWAIT) || { MSTAT=noend; return 1; }
  echo "  make end: $(show $MW)"
  TRYL=$(sed -n "$(( n0 + 1 )),${MW}p" $J | grep -E "$TRY"); [ -n "$TRYL" ] && echo "$TRYL" | cut -c1-220 | sed 's/^/  try: /'
  sed -n "${MW}p" $J | grep -qE "$KEPT" || { MSTAT=notkept; return 1; }; MSTAT=kept
  # The at-once line can follow the make end by a few hundred ms (run 2, 24 Nov: 127 ms after it): wait for it.
  waitfor "$ATONCE" $n0 ${ATWAIT:-10} >/dev/null; AT=$(tail -n +$(( n0 + 1 )) $J | grep -E "$ATONCE" | head -1)
  if [ -n "$AT" ]; then MPATH=at-once; echo "  PATH at-once (no read-back, no yes played): $(echo "$AT" | cut -c1-200)"
    O=$(waitfor "$ON" $n0 ${ONWAIT:-90}) && echo "  on: $(show $O)"
    SO=$(tail -n +$(( n0 + 1 )) $J | grep -oE "$SAIDON" | head -1); echo "  tried line: ${SO:-none in the journal}"
    saydone $MW
  else
    rb=$(waitfor "$READBACK" $n0 ${RBWAIT:-60}) && { MPATH=yes; echo "  PATH yes (read back): $(show $rb)"; } || { echo "  PATH none: no at-once and no read-back line in ${RBWAIT:-60} s: the yes is played as before"; rb=$MW; }
    saydone $rb; PLAY YES
    Y=$(waitfor "$YESOK|$YESNOT" $rb ${YESWAIT:-60})
    if [ -n "$Y" ] && sed -n "${Y}p" $J | grep -qE "$YESNOT"; then echo "  $(show $Y): one more yes"; sleep ${YES_AGAIN:-10}; PLAY YES; Y=$(waitfor "$YESOK" $Y ${YESWAIT:-60}); fi
    [ -n "$Y" ] && echo "  yes: $(show $Y)" || echo "  no yes-taken line"
    O=$(waitfor "$ON" $rb ${ONWAIT:-90}) && echo "  on: $(show $O)"; fi
  echo "PATH $LEG $MPATH" >> $LG/paths.txt; quiet; return 0; }
# MKCHK: TW empty when the make's path and try are right: at-once needs "its task kept"; a try that ran ends exit 0
# having said >= 1; at once, a try that ran is said on in the tried line, naming what the skill uses (it holds net).
MKCHK(){ TW=
  [ "$MPATH" = at-once ] && ! echo "$AT" | grep -q 'its task kept' && TW="$TW put on at once but its task not kept (its code runs nowhere);"
  if [ -n "$TRYL" ]; then
    echo "$TRYL" | grep -qE 'was not run|exit [^0]|limit passed|said (0|unknown)' && TW="$TW its try failed: $(echo "$TRYL" | head -1 | cut -c1-120);"
    echo "$TRYL" | grep -qE 'exit 0, .*said [1-9]' || TW="$TW its try did not end exit 0 having said;"
    if [ "$MPATH" = at-once ]; then [ -n "$SO" ] || TW="$TW no tried line ('Made it..., and I tried it') after its try;"
      [ -n "$SO" ] && echo "$SO" | grep -q 'no uses named' && TW="$TW the tried line names no uses;"; fi
  fi; return 0; }
tck(){ [ -n "$TW" ] && echo "FAIL ($TW)" || echo "PASS (path $MPATH, $( [ -n "$TRYL" ] && echo "try $(echo "$TRYL" | grep -oE 'exit [0-9a-z]+|said [0-9a-z]+' | tr '\n' ' ')${SO:+, tried line said}" || echo "no try ran"))"; }
mwhy(){ case $MSTAT in unheard) echo "the clip was not heard (no request line in ${HEARWAIT:-60} s: mute, Mac volume, speaker placement)";;
  noend) echo "no make end in $MAKEWAIT s";; notkept) echo "not kept: $(show $MW | cut -c1-170)";; *) echo "$MSTAT";; esac; }
oldref(){ grep -E "$OLDREF" $J | head -1 | cut -c1-200; }
VA=FAIL VB=FAIL VC=FAIL VD=FAIL VS=FAIL VK=FAIL; TW=; MPATH=none
# ---- helpers of legs S and K (ported from the #8180 e82d kit, ebcdaf1c27)
ep(){ sed -n "${1}p" $J | awk '{print $1}'; }
# HUSH: wait for the lamp's last say in this leg's journal to end (its pieces line + chars/12 + 3 s, at most 45 s) before
# a fresh-turn ask (ASK; its clip opens with "Hey Lamp,"): a person does not talk over the lamp.
HUSH(){ local t w; t=$(grep -nE "[0-9]+ of [0-9]+ pieces began" $J 2>/dev/null | tail -1 | cut -d: -f1); [ -n "$t" ] || return 0
  w=$(python3 -c "import sys,time;e=float(sys.argv[1])+int(sys.argv[2] or 0)/12+3;print(int(min(max(e-time.time(),0),45)+0.999))" "$(ep $t)" "$(sed -n "${t}p" $J | grep -oE '[0-9]+ chars' | tail -1 | grep -oE '[0-9]+')" 2>/dev/null)
  [ "${w:-0}" -gt 0 ] && { echo "  HUSH the lamp's say ends in ~$w s: waiting"; sleep $w; }; return 0; }
ASK(){ PLAY $1; }
# An ask or answer the voice held back as said to nobody (voice attention.rs): the leg is VOID, not FAIL.
HELDRE="its answer is held back: nobody spoke to the lamp"
held(){ grep -E "$HELDRE" $J 2>/dev/null | head -1 | cut -c1-200; }
# PUSH <fixture>: the install route of S (push.py: the developer door's skill.push, then the manage helper's
# install_pushed_at). PUSHCMD is the runner's python3 with the lampos SDK; LAMP_PAIRED names the pairing (optional).
PUSHCMD=${PUSHCMD:-python3 $D/push.py}
PUSH(){ $PUSHCMD push $D/fixtures/$1 ${LAMP_PAIRED:-} > $LG/push-$1.txt 2>&1; sed 's/^/  push: /' $LG/push-$1.txt | cut -c1-240; grep -q '^PUSHED ' $LG/push-$1.txt; }
pushwhy(){ grep -E 'REFUSED' $LG/push-$1.txt 2>/dev/null | head -1 | cut -c1-200 || echo "no answer"; }
# PUSHL <fixture>: K's install route, with no pairing (push.sh: the SDK's package from this worktree, staged on the lamp
# as lamp-control's door keeps a push, then the manage helper's {"install_pushed": SHA}; unrow's remove_pushed takes
# it off). Same log and PUSHED marker as PUSH; PUSH INSTALLED is its end. LPUSHCMD overrides the command (mocks).
LPUSHCMD=${LPUSHCMD:-bash $D/push.sh}
PUSHL(){ $LPUSHCMD push $D/fixtures/$1 > $LG/push-$1.txt 2>&1; sed 's/^/  push: /' $LG/push-$1.txt | cut -c1-240
  grep -q '^PUSHED ' $LG/push-$1.txt && grep -q '^PUSH INSTALLED' $LG/push-$1.txt; }
ASKW(){ SSHR -n $H "rm -f /tmp/t7901-stop.ask$1"; LAMP "SECS=$(( RUNWAIT * 3 + 300 )) STOPF=/tmp/t7901-stop.ask$1" $D/asked.lamp.sh > $LG/asked-$1.txt 2>&1 & FP=$!; }
UNASKW(){ SSHR -n $H "touch /tmp/t7901-stop.ask$1"; sleep 2; kill $FP 2>/dev/null; FP=; }
# A pushed skill's task yes (skills task.rs ask_yes, voice skill_task_yes.rs, control task_yes.rs, skill_write_yes.rs)
UNASKED="a skill's task said back unasked waits for the owner's yes"
ASKLATER="do\.task: not handed over: waiting for the owner's yes.*asked aloud: say: refused|not asked aloud: |kept for later"
RBWAITS="a skill's task read back: it waits for the person's yes"
RBNOT="a skill's task read back by name only|a skill's task read back: its yes can't be taken on this route"
RBAGREED="a skill's task read back: agreed already"
TYESOK="lamp-control: the owner's yes to %s's task kept|a skill's task approved on the person's yes"
TNO="the answer to %s's say-back was (no|unclear)|the line after %s's .* was a new ask"
# What the yes says can go out (#7901's head generalises AIR_BESIDE_NET in voice skill_task_yes.rs): the sentence
# naming the camera must say it "can go to the internet". A stable fragment only: the exact words are #7901's head's.
GOOUT="can go to the internet"
saidline(){ grep '^ASKED ' $LG/asked-S.txt 2>/dev/null | grep "\"skill\":\"$1\"" | head -1 | sed 's/^ASKED //' |
  python3 -c 'import json,sys
try: print(json.loads(sys.stdin.read()).get("say_back",""))
except Exception: pass' 2>/dev/null; }
# ---- A: the coffee make: kept with net AND notes, no refusal
legA(){ local L g ref c1=0 c2=0 c3=0 c4=0
  BEGIN A || return 1; quiet; MAKE MAKEA; END; STATE A
  L=$(grep '^NEW ' $LG/state-A.txt | grep 'grants=[^ ]*net' | head -1); [ -n "$L" ] || L=$(grep '^NEW ' $LG/state-A.txt | head -1)
  SKILL=$(echo "$L" | awk '{print $2}'); g=$(echo "$L" | field grants); echo "$SKILL" > $LG/skill-a.name
  ref=$(oldref)
  [ "$MSTAT" = kept ] && c1=1; [ -n "$SKILL" ] && has "$g" net && has "$g" notes && c2=1; [ -z "$ref" ] && c3=1; [ -n "$O" ] && c4=1
  echo "CHECK (A1) kept: $(ck A $c1) ($MSTAT)"
  echo "CHECK (A2) its grants hold net AND notes: $(ck A $c2) (${SKILL:-no new skill} grants=${g:--})"
  echo "CHECK (A3) no net-beside-private refusal in the journal: $(ck A $c3)${ref:+ ($ref)}"
  echo "CHECK (A4) on the lamp: $(ck A $c4)${Y:+ (yes taken)}"
  MKCHK; [ "$MSTAT" = kept ] && echo "CHECK (A5) its path and try: $(tck)"
  if [ -n "$ref" ]; then WHY="the old net-beside-private refusal: $ref"
  elif [ "$MSTAT" = unheard ] || [ "$MSTAT" = noend ]; then VA=VOID; WHY=$(mwhy)
  elif [ -n "$TW" ]; then WHY="$SKILL:$TW"
  elif [ "$c1$c2$c4" = 111 ]; then VA=PASS; WHY="$SKILL kept, grants=$g, on ($MPATH), no refusal"
  elif [ $c1 = 0 ]; then WHY=$(mwhy)
  elif [ $c2 = 0 ]; then WHY="${SKILL:-no new skill}: grants=${g:--}, not net+notes"
  else WHY="no 'is on the lamp' line (yes: $([ -n "$Y" ] && echo taken || echo not seen))"; fi; }
# ---- B: two runs; the token names notes, no refusal, the notes file between them, read back
legB(){ local i n0 t e g rid nf nf1= k s1 s2 sg rd ref c1=1 c2=1 c3=0 c4=0 why=
  [ -n "${SKILL:-}" ] || SKILL=$(cat $LG/skill-a.name 2>/dev/null)
  [ -n "$SKILL" ] || { VB=VOID; WHY="no skill from leg A (logs/skill-a.name)"; return 0; }
  BEGIN B || return 1; quiet
  for i in 1 2; do n0=$(lines); PLAY RUN
    t=$(waitfor "lamp-control: run [^ ]+ of skill $SKILL: (a token for \[|no token made)" $n0 $RUNWAIT) || {
      waitfor "$HEARD" $n0 1 >/dev/null && why="run $i: no token for $SKILL in $RUNWAIT s" || { VB=VOID; why="run $i: the clip was not heard"; }; c1=0; break; }
    echo "  run $i token: $(show $t)"
    sed -n "${t}p" $J | grep -q 'no token made' && { why="run $i: $(show $t | cut -c1-160)"; c1=0; c2=0; break; }
    rid=$(sed -n "${t}p" $J | sed -n 's/.*lamp-control: run \([^ ]*\) of skill .*/\1/p')
    g=$(sed -n "${t}p" $J | sed -n 's/.*a token for \[\([^]]*\)\].*/\1/p' | tr -d ' ')
    has "$g" notes || { c1=0; why="run $i token grants [$g] lack notes"; }
    e=$(waitfor "lamp-control: run $rid of skill $SKILL: " $t $ENDWAIT); while [ -n "$e" ] && sed -n "${e}p" $J | grep -qE "a token for"; do e=$(waitfor "lamp-control: run $rid of skill $SKILL: " $e $ENDWAIT); done
    [ -n "$e" ] && echo "  run $i end: $(show $e)" || { c2=0; why="${why:+$why; }run $i: no end line"; break; }
    saydone $e; quiet; STATE B$i
    nf=$(grep "^NEW $SKILL " $LG/state-B$i.txt | field notes); k=$(grep "^NEW $SKILL " $LG/state-B$i.txt | field keys)
    [ $i = 1 ] && { nf1=$nf; [ -n "$nf" ] && [ "$nf" != none ] && [ "${k:-0}" != 0 ] && c3=1 || why="${why:+$why; }run 1 left no notes (file ${nf:-?} keys ${k:-?})"; }
  done
  END
  grep -E "$EGJ" $J | cut -c1-200 | sed 's/^/  egress /'
  ref=$(grep -E "$RUNREF|$OLDREF" $J | head -1 | cut -c1-200); [ -n "$ref" ] && c2=0
  sg=$(grep "^NEW $SKILL " $LG/state-B2.txt 2>/dev/null | field grants); rd=$(grep "^NEW $SKILL " $LG/state-B2.txt 2>/dev/null | field reads_notes)
  s1=$(grep "^NEW $SKILL " $LG/state-B1.txt 2>/dev/null | field sha); s2=$(grep "^NEW $SKILL " $LG/state-B2.txt 2>/dev/null | field sha)
  nf=$(grep "^NEW $SKILL " $LG/state-B2.txt 2>/dev/null | field notes)
  [ $c3 = 1 ] && [ "$rd" = yes ] && [ "$nf" = "$nf1" ] && has "$sg" net && has "$sg" notes && c4=1
  echo "CHECK (B1) each run's token names notes: $(ck B $c1)"
  echo "CHECK (B2) each run ends with no refusal: $(ck B $c2)${ref:+ ($ref)}"
  echo "CHECK (B3) run 1 leaves the notes file: $(ck B $c3) (${nf1:-none})"
  echo "CHECK (B4) run 2 reads it back (file there before run 2, code calls lamp.notes.get, grants net+notes): $(ck B $c4) (reads_notes=${rd:--} grants=${sg:--} sha ${s1:--} -> ${s2:--})"
  grep -h '^NOTESJSON' $LG/state-B1.txt $LG/state-B2.txt 2>/dev/null | cut -c1-400 | sed 's/^/  /'
  if [ "$VB" = VOID ]; then WHY=$why
  elif [ "$c1$c2$c3$c4" = 1111 ]; then VB=PASS; WHY="2 runs, tokens name notes, no refusal, notes file $nf1 there before run 2 (sha $s1 -> $s2), code reads it"
  elif [ -n "$ref" ]; then WHY="a refusal: $ref"
  elif [ -n "$why" ]; then WHY=$why
  elif [ $c4 = 0 ]; then WHY="run 2 can't read run 1's notes: reads_notes=${rd:--} grants=${sg:--} file ${nf:-none}"; fi; }
# ---- C (flipped): net beside the camera is KEPT, one skill, no refusal, no split
legC(){ local L g ref sp newc nnew c1=0 c2=0 c3=0 c4=0 c6=0
  BEGIN C || return 1; quiet; STATE C0 >/dev/null; MAKE MAKEC; END; STATE C
  newc=$(grep '^NEW ' $LG/state-C.txt | awk '{print $2}' | while read -r nm; do grep -q "^NEW $nm " $LG/state-C0.txt || echo $nm; done)
  nnew=$(echo "$newc" | grep -c .)
  L=; for nm in $newc; do g=$(grep "^NEW $nm " $LG/state-C.txt | field grants); has "$g" net && has "$g" camera && { L=$nm; break; }; done
  [ -n "$L" ] || { L=$(echo "$newc" | head -1); g=$(grep "^NEW $L " $LG/state-C.txt 2>/dev/null | field grants); }
  SKILLC=$L; echo "$SKILLC" > $LG/skill-c.name
  ref=$(oldref); sp=$(grep -E "$SPLIT" $J | head -1 | cut -c1-200)
  [ "$MSTAT" = kept ] && c1=1; [ -n "$L" ] && has "$g" net && has "$g" camera && c2=1; [ -z "$ref$sp" ] && [ "$nnew" -le 1 ] && c3=1; [ -n "$O" ] && c4=1
  echo "CHECK (C1) kept: $(ck C $c1) ($MSTAT)"
  echo "CHECK (C2) one skill holds net AND camera: $(ck C $c2) (${L:-no new skill} grants=${g:--})"
  echo "CHECK (C3) no net-beside-private refusal and no split in the journal: $(ck C $c3)${ref:+ (refusal: $ref)}${sp:+ (split: $sp)} (new skills: $nnew)"
  echo "CHECK (C4) on the lamp: $(ck C $c4)${Y:+ (yes taken)}"
  MKCHK; [ "$MSTAT" = kept ] && echo "CHECK (C5) its path and try: $(tck)"
  [ "$MPATH" = at-once ] && c6=1
  [ "$MSTAT" = kept ] && echo "CHECK (C6) live with nothing asked (at once, no YES played): $(ck C $c6) (path $MPATH; tried line: $( [ -n "$SO" ] && echo "uses named: the room hears 'It uses the camera and the internet'" || echo "not in the journal"))"
  grep -E "$OLDREF|$SPLIT|$NOTKEPT" $J | cut -c1-220 | head -4 | sed 's/^/  /'
  if [ -n "$ref" ]; then WHY="the old net-beside-private refusal: $ref"
  elif [ -n "$sp" ] || [ "$nnew" -gt 1 ]; then [ -n "$sp" ] && WHY="a split in the journal: $sp" || WHY="split into $nnew skills: $(echo $newc)"
  elif [ "$MSTAT" = unheard ] || [ "$MSTAT" = noend ]; then VC=VOID; WHY=$(mwhy)
  elif [ "$MSTAT" = notkept ] && grep -qE "cannot was not made a task for the lamp's agent: sensing" $J; then VC=VOID; WHY="not kept as a trigger that watches (class sensing), not #7901's refusal: rerun leg C once"
  elif [ -n "$TW" ]; then WHY="$L:$TW"
  elif [ "$c1$c2$c4" = 111 ] && [ $c6 = 0 ]; then WHY="$L kept with net+camera, but on the $MPATH path: something was asked (First, 16:06: live with nothing asked)"
  elif [ "$c1$c2$c4" = 111 ]; then VC=PASS; WHY="$L kept with grants=$g, on ($MPATH, nothing asked), no refusal, no split"
  elif [ $c1 = 0 ]; then WHY=$(mwhy)
  elif [ $c2 = 0 ]; then WHY="${L:-no new skill}: grants=${g:--}, not net+camera"
  else WHY="no 'is on the lamp' line (yes: $([ -n "$Y" ] && echo taken || echo not seen))"; fi; }
# ---- D: C's skill's run: admin.sock hidden, token names camera, net by its egress socket, own cgroup leaf
legD(){ local n0 t e g rid ad vis eg ec ej f5 cgl cgp cgu ref other c1=0 c2=0 c3=0 c4=0 c5=0 why=
  [ -n "${SKILLC:-}" ] || SKILLC=$(cat $LG/skill-c.name 2>/dev/null)
  [ -n "$SKILLC" ] || { VD=VOID; WHY="no skill from leg C (logs/skill-c.name)"; return 0; }
  BEGIN D || return 1; quiet; WATCH D; sleep 2; n0=$(lines); PLAY RUNC
  t=$(waitfor "lamp-control: run [^ ]+ of skill $SKILLC: (a token for \[|no token made)" $n0 $RUNWAIT)
  if [ -z "$t" ]; then other=$(grep -E "lamp-control: run [^ ]+ of skill [^ ]+: a token for" $J | head -1)
    if [ -n "$other" ]; then VD=VOID; why="RUNC ran another skill, not $SKILLC: set the RUNC line in row/clips/clips.txt to name it, ../run.sh render, then LEGS=D ../run.sh legs"
    elif waitfor "$HEARD" $n0 1 >/dev/null; then why="no token for $SKILLC in $RUNWAIT s"
    else VD=VOID; why="the clip was not heard"; fi
  else echo "  run token: $(show $t)"
    rid=$(sed -n "${t}p" $J | sed -n 's/.*lamp-control: run \([^ ]*\) of skill .*/\1/p')
    g=$(sed -n "${t}p" $J | sed -n 's/.*a token for \[\([^]]*\)\].*/\1/p' | tr -d ' ')
    has "$g" camera && c2=1
    e=$(waitfor "lamp-control: run $rid of skill $SKILLC: " $t $ENDWAIT); while [ -n "$e" ] && sed -n "${e}p" $J | grep -qE "a token for"; do e=$(waitfor "lamp-control: run $rid of skill $SKILLC: " $e $ENDWAIT); done
    [ -n "$e" ] && { echo "  run end: $(show $e)"; saydone $e; } || why="no end line for run $rid"
  fi
  quiet; UNWATCH D; END
  ad=$(grep -c '^ADMIN ' $LG/fence-D.txt 2>/dev/null); vis=$(grep '^ADMIN .*admin_sock=visible' $LG/fence-D.txt 2>/dev/null | head -1)
  eg=$(grep -c '^EGRESS +' $LG/fence-D.txt 2>/dev/null); ec=$(grep -c '^EGRESSCONN' $LG/fence-D.txt 2>/dev/null)
  f5=$(grep '^NET ' $LG/fence-D.txt 2>/dev/null | grep 'fd5=socket' | head -1); ej=$(grep -E "$EGJ" $J | head -1 | cut -c1-200)
  cgl=$(grep '^CG ' $LG/fence-D.txt 2>/dev/null | head -1); cgp=$(echo "$cgl" | awk '{print $3}'); cgu=$(echo "$cgl" | sed -n 's/.* unit=//p')
  ref=$(grep -E "$RUNREF|$OLDREF" $J | head -1 | cut -c1-200)
  [ "${ad:-0}" != 0 ] && [ -z "$vis" ] && c1=1; [ -n "$vis" ] && c1=0; [ "${ad:-0}" = 0 ] && [ -z "$vis" ] && c1=v
  { [ "${eg:-0}" != 0 ] || [ "${ec:-0}" != 0 ] || [ -n "$f5" ] || [ -n "$ej" ]; } && c3=1
  [ -n "$t" ] && [ -n "${e:-}" ] && [ -z "$ref" ] && c4=1
  if [ -z "$cgl" ]; then c5=v; else case "$cgp" in "$cgu"/serve|"$cgu"/main|"$cgu") c5=0;; "$cgu"/*) c5=1;; *) c5=0;; esac; fi
  echo "  fence: $ad run pids seen; egress sockets +${eg:-0}, connections ${ec:-0}, fd5 ${f5:+socket}${f5:-none}; cgroup ${cgp:-unread}"; [ -n "$ej" ] && echo "  EGRESS-J $ej"
  echo "CHECK (D1) admin.sock not visible to the run: $(ck D $c1)${vis:+ ($vis)}"
  echo "CHECK (D2) the run's token names camera: $(ck D $c2) ([${g:-no token}])"
  echo "CHECK (D3) the net reaches out by its egress socket: $(ck D $c3)"
  echo "CHECK (D4) the run ends with no refusal: $(ck D $c4)${ref:+ ($ref)}"
  echo "CHECK (D5) the run in its own cgroup leaf under lamp-tasks: $(ck D $c5) (${cgp:-unread} under ${cgu:-?})"
  if [ "$VD" = VOID ]; then WHY=$why
  elif [ -n "$vis" ]; then WHY="admin.sock visible in the run's fence: $(echo "$vis" | cut -c1-160)"
  elif [ -n "$ref" ] && echo "$ref" | grep -q private; then VD=VOID; WHY="the lamp was private during the run (camera refused): $ref"
  elif [ -n "$ref" ]; then WHY="a refusal: $ref"
  elif [ -n "$why" ]; then WHY=$why
  elif [ $c2 = 0 ]; then WHY="the token [$g] does not name camera"
  elif [ $c3 = 0 ]; then WHY="the run's net did NOT reach out: no egress socket, connection, fd5 or journal line"
  elif [ $c5 = 0 ]; then WHY="the run is not in its own cgroup leaf: $cgp (unit $cgu)"
  elif [ $c1 = v ]; then VD=VOID; WHY="no run pid seen by the fence (run shorter than the 0.2 s watch): rerun LEGS=D"
  else VD=PASS; WHY="$SKILLC ran: admin.sock hidden ($ad pids), token [$g], egress (+$eg sockets, $ec conns${ej:+, journal}), leaf $cgp"; fi; }
# ---- S: a pushed camera+net skill: asked with the can-go-out line, kept only on the yes, its run [camera] + egress
legS(){ local sk=photo-search n0 n1 n2 a rb y t g gs e rid ej ref nt sb kp tk h c1=0 c2=0 c3=0 c4=0 c5=0 c6=0 why=
  BEGIN S || return 1; quiet
  PUSH $sk || { VS=VOID; WHY="the install route is unavailable: the push was refused ($(pushwhy $sk))"; END; return 0; }
  # 1: a run asked, with no yes kept: lamp-skills waits for the yes and has the voice ask it aloud (watched)
  ASKW S; sleep 2; HUSH; n0=$(lines); ASK RUNS
  a=$(waitfor "$UNASKED|$ASKLATER|lamp-control: run [^ ]+ of skill $sk: a token for" $n0 $RUNWAIT)
  [ -n "$a" ] && echo "  ask 1: $(show $a)"
  if [ -z "$a" ]; then why="no ask said for $sk in $RUNWAIT s"; VS=VOID
  elif sed -n "${a}p" $J | grep -qE "a token for"; then why="$sk ran with no yes asked: $(show $a | cut -c1-160)"
  elif sed -n "${a}p" $J | grep -qE "$ASKLATER"; then VS=VOID; why="the ask was not said now: $(show $a | cut -c1-160)"
  else saydone $a; PLAY NO
    t=$(waitfor "$(printf "$TNO" $sk $sk)" $a ${YESWAIT:-60}) && echo "  no: $(show $t)" || echo "  no 'say-back was no' line"
    sleep ${NOHOLD:-10}; quiet; n1=$(lines)
    kp=$(sed -n "$(( n0 + 1 )),${n1}p" $J | grep -E "$(printf "$TYESOK" $sk)" | head -1 | cut -c1-160)
    tk=$(sed -n "$(( n0 + 1 )),${n1}p" $J | grep -E "lamp-control: run [^ ]+ of skill $sk: a token for" | head -1 | cut -c1-160)
    [ -z "$kp$tk" ] && c2=1
    # 2: asked again (its read-back), YES
    HUSH; n1=$(lines); ASK USES
    rb=$(waitfor "$RBWAITS|$RBNOT|$RBAGREED|$UNASKED" $n1 $RUNWAIT); [ -n "$rb" ] && echo "  ask 2: $(show $rb)"
    if [ -z "$rb" ]; then VS=VOID; why="no read-back of $sk in $RUNWAIT s"
    elif sed -n "${rb}p" $J | grep -qE "$RBAGREED"; then c2=0; kp="${kp:-the read-back after NO says agreed already}"
    elif sed -n "${rb}p" $J | grep -qE "$RBNOT"; then VS=VOID; why="the read-back could take no yes: $(show $rb | cut -c1-160)"
    else saydone $rb; PLAY YES
      y=$(waitfor "$(printf "$TYESOK" $sk)" $rb ${YESWAIT:-60}) && { c3=1; echo "  yes: $(show $y)"; }
      # 3: its run, with the yes kept
      sleep ${YES_AGAIN:-10}; quiet; HUSH; n2=$(lines); ASK RUNS
      t=$(waitfor "lamp-control: run [^ ]+ of skill $sk: (a token for \[|no token made)" $n2 $RUNWAIT)
      if [ -n "$t" ]; then echo "  run token: $(show $t)"
        rid=$(sed -n "${t}p" $J | sed -n 's/.*lamp-control: run \([^ ]*\) of skill .*/\1/p')
        g=$(sed -n "${t}p" $J | sed -n 's/.*a token for \[\([^]]*\)\].*/\1/p' | tr -d ' ')
        # server.rs:341 prints lamp-control's own grants only; net is never one (task_yes.rs, skill_task_yes.rs NET)
        gs=$(echo "$g" | tr ',' '\n' | grep -v '^net$' | LC_ALL=C sort | tr '\n' ',' | sed 's/,$//')
        [ "$gs" = camera ] && c4=1
        e=$(waitfor "lamp-control: run $rid of skill $sk: " $t $ENDWAIT); while [ -n "$e" ] && sed -n "${e}p" $J | grep -qE "a token for"; do e=$(waitfor "lamp-control: run $rid of skill $sk: " $e $ENDWAIT); done
        [ -n "$e" ] && { echo "  run end: $(show $e)"; saydone $e; } || why="no end line for run $rid"
        ej=$(tail -n +$(( n2 + 1 )) $J | grep -E "$EGJ" | head -1 | cut -c1-200); [ -n "$ej" ] && [ -n "$e" ] && c5=1
        nt=$(tail -n +$(( n2 + 1 )) $J | grep -E "lamp-control: .*notes\.get: refused: no_grant" | head -1 | cut -c1-200); [ -n "$nt" ] && c6=1
        ref=$(tail -n +$(( n2 + 1 )) $J | grep -E "$RUNREF|$OLDREF" | grep -v "notes\.get: refused: no_grant" | head -1 | cut -c1-200); [ -n "$ref" ] && c5=0
      else why="no token for $sk in $RUNWAIT s after the yes"; fi
    fi
  fi
  quiet; UNASKW S; END
  sb=$(saidline $sk); [ -n "$sb" ] || sb=$(grep -E "$GOOUT" $J | head -1 | cut -c1-400)
  if [ -z "$sb" ]; then c1=v; elif echo "$sb" | grep -qE "camera[^.]*$GOOUT"; then c1=1; fi
  echo "  said line: ${sb:-not captured (no ASKED line in logs/asked-S.txt)}" | cut -c1-400
  echo "CHECK (S1) the yes is asked with the line saying what can go out ('camera ... $GOOUT'): $(ck S $c1)"
  echo "CHECK (S2) NO: not kept, no token: $(ck S $c2)${kp:+ (kept: $kp)}${tk:+ (token: $tk)}"
  echo "CHECK (S3) YES: kept: $(ck S $c3)"
  echo "CHECK (S4) its run's token is exactly [camera] (net carried as its egress): $(ck S $c4) ([${g:-no token}])"
  echo "CHECK (S5) the run reaches out by its egress socket and ends with no refusal: $(ck S $c5)${ej:+ ($ej)}${ref:+ (refused: $ref)}"
  echo "CHECK (S6) its try at the notes (no notes grant) is refused no_grant: no skill reads another's notes: $(ck S $c6)"
  h=$(held)
  if [ "$VS" = VOID ]; then WHY=$why
  elif [ $c2 = 0 ]; then WHY="kept or run on NO: ${kp:-$tk}"
  elif [ -n "$why" ] && [ -n "$h" ]; then VS=VOID; WHY="an ask or answer held back ($h); was: $why"
  elif [ -n "$why" ]; then WHY=$why
  elif [ "$c1" = 0 ]; then WHY="the yes's line does not say what the camera sees '$GOOUT': $(echo "$sb" | cut -c1-200)"
  elif [ $c3 = 0 ]; then [ -n "$h" ] && { VS=VOID; WHY="the YES held back: $h"; } || WHY="not kept on the YES"
  elif [ $c4 = 0 ]; then WHY="the run's token is [$g], not exactly [camera]"
  elif [ $c5 = 0 ]; then WHY="the run did not reach out cleanly: ${ref:-no egress line or no end}"
  elif [ $c6 = 0 ]; then WHY="no 'notes.get: refused: no_grant' for its notes try"
  elif [ "$c1" = v ]; then VS=VOID; WHY="the yes's line was not captured (no ASKED line): rerun LEGS=S"
  else VS=PASS; WHY="$sk pushed, asked with '$GOOUT', not kept on NO, kept on YES, token [$g], egress, notes refused"; fi; }
# ---- K: a pushed skill whose run needs a key never added: refused before any token, with its journal line
KEYREF="\[lamp-tasks\] skill task: %s not run: [0-9]+ key\(s\) not added in the app|a run given its keys can't be given them on this lamp yet"
legK(){ local sk=key-probe n0 n1 rb y k tk h c1=0 c2=0 why=
  BEGIN K || return 1; quiet
  PUSHL $sk || { VK=VOID; WHY="the install route is unavailable: the push was refused ($(pushwhy $sk))"; END; return 0; }
  HUSH; n0=$(lines); ASK KEYUSES
  rb=$(waitfor "$RBWAITS|$RBNOT|$RBAGREED|$UNASKED" $n0 $RUNWAIT); [ -n "$rb" ] && echo "  ask: $(show $rb)"
  if [ -z "$rb" ]; then VK=VOID; why="no read-back of $sk in $RUNWAIT s"
  elif sed -n "${rb}p" $J | grep -qE "$RBNOT"; then VK=VOID; why="the read-back could take no yes: $(show $rb | cut -c1-160)"
  else
    if ! sed -n "${rb}p" $J | grep -qE "$RBAGREED"; then saydone $rb; PLAY YES
      y=$(waitfor "$(printf "$TYESOK" $sk)" $rb ${YESWAIT:-60}) && echo "  yes: $(show $y)"; fi
    sleep ${YES_AGAIN:-10}; quiet; HUSH; n1=$(lines); ASK KEYRUN
    k=$(waitfor "$(printf "$KEYREF" $sk)|lamp-control: run [^ ]+ of skill $sk: a token for" $n1 $RUNWAIT)
    [ -n "$k" ] && echo "  run: $(show $k)"; sleep ${TAIL:-8}
    tk=$(tail -n +$(( n1 + 1 )) $J | grep -E "lamp-control: run [^ ]+ of skill $sk: a token for" | head -1 | cut -c1-160)
    [ -n "$k" ] && sed -n "${k}p" $J | grep -qE "$(printf "$KEYREF" $sk)" && c1=1; [ -z "$tk" ] && c2=1
    [ -n "$k" ] || why="no keys refusal and no token for $sk in $RUNWAIT s"
  fi
  quiet; END
  echo "CHECK (K1) its run is refused for its key, with its journal line: $(ck K $c1)"
  echo "CHECK (K2) no token made for it: $(ck K $c2)${tk:+ ($tk)}"
  h=$(held)
  if [ "$VK" = VOID ]; then WHY=$why
  elif [ $c2 = 0 ]; then WHY="a run needing a key never added got a token: $tk"
  elif [ $c1 = 1 ]; then VK=PASS; WHY="$sk refused for its key: $(sed -n "${k}p" $J | cut -c1-200)"
  elif [ -n "$h" ]; then VK=VOID; WHY="an ask or answer held back ($h); was: $why"
  elif [ -z "$y" ] && [ -z "$k" ]; then VK=VOID; WHY="no yes kept for $sk, so its run was never handed over"
  else WHY=${why:-no keys refusal line}; fi; }
RT=$LG/row-$(echo $LEGS | tr -d ' ').txt
echo "== row.sh $LEGS on $LAMP $(date '+%F %T')" | tee $RT >> $LG/row.txt
for g in $LEGS; do RENEW || { echo "LOCK lost before leg $g: stop"; exit 1; }; WHY=
  case $g in A) legA; echo "LEG A $VA ($WHY)";; B) legB; echo "LEG B $VB ($WHY)";; C) legC; echo "LEG C $VC ($WHY)";;
    D) legD; echo "LEG D $VD ($WHY)";; S) legS; echo "LEG S $VS ($WHY)";; K) legK; echo "LEG K $VK ($WHY)";; *) echo "no leg $g"; continue;; esac
  eval "echo V$g=\$V$g" >> $LG/verdicts.env
  sleep ${BETWEEN:-8}; done 2>&1 | tee -a $RT
tail -n +2 $RT >> $LG/row.txt
want=$(echo $LEGS | wc -w | tr -d ' '); v=$(grep -c '^LEG [A-Z] PASS' $RT)
vf=$(grep -c '^LEG [A-Z] FAIL' $RT); vv=$(grep -c '^LEG [A-Z] VOID' $RT)
if [ "$v" = "$want" ]; then R=PASS; elif [ "$vf" = 0 ] && [ "$vv" != 0 ]; then R=VOID; else R=FAIL; fi
echo "VERDICT $R ($v of $want legs PASS, $vf FAIL, $vv VOID; legs $LEGS; ROW #$PR on $LAMP; journals row/logs/leg-*.journal.txt)" | tee -a $RT >> $LG/row.txt
grep '^VERDICT' $RT
[ $R = PASS ]
