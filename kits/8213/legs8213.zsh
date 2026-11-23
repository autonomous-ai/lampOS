#!/bin/zsh
# #8213 legs (split ROW kit, piece B runs FROM=6 UNTIL=8), on the PR build a REAL round put on the lamp; written first
# for lamp-c0a4. Per engine, OpenClaw (the default) first, then Hermes: MAKE (a skill made by voice), TRY ("try <ask>"),
# RUN ("run <ask>"), then a door check (OpenClaw: passive, no door fallback exists for a gateway agent). Since 11bc59c692
# (#8213's HOLD, (a)): Hermes's kept session is held to no exact exec allow-list, so its TRY and RUN go PER RUN (its own
# pane, its token file, said once), never through the door; its MAKE stays in the kept pane.
# Reused from #8156's kit: lamp.sh (engine, since, ask, snap verbs; copied to /tmp/sd8213 with its paths renamed),
# say()/quiet()/other_quiet() (Mac volume read only, <=70, never set; the lamp's speaker never touched), onea().
#   LAMP=lamp-ee17 LAMP_SSH=<runner stand-in> LL=<lock script> CLIPS=<dir of rendered wavs> [FROM=1..9] [UNTIL=1..9]
#   [AGENT_FIRST=0] [TEXT=1] [ENGINE_BACK=<engine found>] ./legs8213.zsh
# Split kit changes: UNTIL; the agent-first drop-in is set whenever AGENT_FIRST=1, whatever FROM is (FROM>1 skipped it,
# which would leave a Hermes MAKE to DeepSeek: VOID); the put-back restores ENGINE_BACK; LAMP_SSH is un-exported so
# lamp-lock.sh never takes it as its ssh command.
set -u
D=${0:a:h}; W=${LAMP_SSH:?LAMP_SSH=<the runner stand-in>}; LAMP=${LAMP:-lamp-ee17}; ENGINE_BACK=${ENGINE_BACK:-openclaw}
typeset +x LAMP_SSH 2>/dev/null
HEAD8213=${HEAD8213:-11bc59c692}; GAIN=${GAIN:-1.0}; CLIPS=${CLIPS:?CLIPS=<dir of rendered wavs>}; FROM=${FROM:-1}; UNTIL=${UNTIL:-9}
AGENT_FIRST=${AGENT_FIRST:-1}; MAKE_WAIT=${MAKE_WAIT:-480}; RUN_WAIT=${RUN_WAIT:-360}
LL=${LL:?LL=<the runner lamp lock script>}; LOCK_WHO=${LOCK_WHO:-row-kit}; LOCK_SESSION=${LOCK_SESSION:-split-row}
L=${LEGS_LOG:-$D/logs}/legs-$(date +%H%M%S); mkdir -p $L; V=$L/verdict.txt; : > $V
LK=/tmp/sd8213; PB=""; FIRST_SET=""; HID=""
log(){ echo "$*" | tee -a $V; }
r(){ $W "sudo -n sh $LK/lamp.sh $*" 2>&1 | grep -v 'error was encountered while opening journal'; }   # #8156's lamp.sh
x(){ $W "sudo -n sh $LK/x.sh $*" 2>&1; }                                                             # this kit's extras
lt(){ $W 'date "+%F %T"'; }
step(){ (( $1 >= FROM && $1 <= UNTIL )); }
renew(){ $LL $LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 || { log "STOP: the $LAMP lock is not ours"; putback; exit 3; }; }

# ---------------------------------------------------------------- copied helpers (legs8156.zsh, unchanged in substance)
quiet(){ local i=0 c; while (( i < 90 )); do
    c=$($W "sudo -n journalctl -u lamp-voice --since -6s --no-pager -o cat 2>/dev/null | grep -cE 'saying what|ElevenLabs|request [0-9]+'"); [ "${c:-1}" = 0 ] && return 0; sleep 2; i=$((i+2)); done; return 1; }
other_quiet(){ local i=0 last; while (( i < 900 )); do
    last=$(gh api "repos/autonomous-ai/lampOS/issues/7875/comments?per_page=100&since=$(python3 -I -c 'import datetime as d;print((d.datetime.now(d.timezone.utc)-d.timedelta(minutes=60)).strftime("%Y-%m-%dT%H:%M:%SZ"))')" --jq '.[].body' 2>/dev/null | grep -oE '^Fifth: (PLAYING|QUIET)' | tail -1 | sed 's/^Fifth: //')
    [ "$last" != PLAYING ] && return 0; echo "$(date +%T) #7875 PLAYING: waiting" >> $L/plays.txt; sleep 30; i=$((i+30)); done; return 1; }
say(){ # say TAG TEXT [now]: the clip, or TEXT=1's hand-over (no sound; only for a fallback, never a PASS of a voice leg)
  if [ -n "${TEXT:-}" ] || [ ! -s $CLIPS/$1.wav ]; then log "$1 note: no clip $CLIPS/$1.wav (or TEXT=1): handed over as text"
    log "$1 TEXT handover: $(r "handover '$2'" | tail -1 | cut -c1-120)"; return 0; fi
  local vol=$(osascript -e 'output volume of (get volume settings)')
  (( vol <= 70 )) || { log "$1 STOP: the Mac's volume is $vol (>70); not changed by this kit"; return 1; }
  x mute | grep -q 'MUTE OFF OK' || { log "$1 STOP: the lamp's software mute is on (the round's unmute step first)"; return 1; }
  other_quiet || { log "$1 STOP: another team still PLAYING on #7875 after 15 min"; return 1; }
  [ "${3:-}" = now ] || quiet || log "$1 note: the lamp was not quiet for 6 s within 90 s; played anyway"
  echo "$(lt) play $1 gain $GAIN vol $vol" >> $L/plays.txt; afplay -v $GAIN $CLIPS/$1.wav; }
agent(){ $W "sudo -n cat /var/lib/lampos-tasks/store/one-session.json" | python3 -I -c 'import json,sys;d=json.load(sys.stdin);print(d.get("agent"),d.get("engine"))' 2>/dev/null; }
onea(){ # ONE_AGENT <tag> <engine>: one kept pane of <engine>, none of the other (legs8156.zsh)
  local e=$2 o; [ $e = hermes ] && o=openclaw || o=hermes
  x panes > $L/onea-$1.txt 2>&1; echo "GW $($W 'systemctl is-active lamp-openclaw-gateway')" >> $L/onea-$1.txt
  local mine=$(cnt "^lamp-one-$e " $L/onea-$1.txt) other=$(cnt "^lamp-one-$o " $L/onea-$1.txt) gw=$(sed -n 's/^GW //p' $L/onea-$1.txt)
  (( mine >= 1 && other == 0 )) && log "ONE_AGENT $1 PASS lamp-one-$e only (gateway $gw)" || log "ONE_AGENT $1 FAIL $e=$mine $o=$other gateway=$gw (onea-$1.txt)"; }
gate(){ local n=$1 c; while :; do  # <=10 service starts in any 10 min (the lamp's journal), as mac.zsh GATE
    c=$($W "journalctl --since -10min --no-pager -o cat -u lamp-tasks -u lamp-voice -u lamp-skills -u lamp-openclaw-gateway -u lamp-update -u lamp-cloud 2>/dev/null | grep -c '^Started '")
    (( ${c:-10} + n <= 10 )) && return 0; echo "$(date +%T) gate: $c starts in 10 min + $n > 10: waiting 60 s" | tee -a $L/starts.txt; renew; sleep 60; done; }

putback(){ [ -n "$PB" ] && return; PB=1; log "== put-back (legs)"
  [ -n "$HID" ] && x launcher-show | tee -a $V
  [ "$(agent | cut -d' ' -f2)" = "$ENGINE_BACK" ] || { gate 1; r "engine $ENGINE_BACK" > $L/eng-putback.txt; log "put-back: engine $ENGINE_BACK ($(grep -m1 SINCE $L/eng-putback.txt))"; }
  [ -n "$FIRST_SET" ] && { gate 1; x first off | tee -a $V; }
  x rowfiles clean | tee -a $V; x made-left | tee -a $V
  r snap > $L/snap-end.txt; $W "rm -rf $LK"; log "lamp $LK removed"; }
trap 'log "ABORT (signal)"; putback; exit 3' INT TERM HUP

# ---------------------------------------------------------------- the lamp-side extras (x.sh), written to $LK
copy(){ $W "mkdir -p $LK && cat > $LK/lamp.sh" < <(sed -e 's#/tmp/sd8156#/tmp/sd8213#g; s#/opt/sd-8156#/opt/sd-8213#g; s/#8156 ROW/#8213 ROW/g; s/zz-sd-8156-hold/zz-sd-8213-hold/g' $D/lamp.sh) || return 1
  $W "cat > $LK/x.sh" <<'X'
# lamp, root sh. #8213 legs' extras. Never the speaker volume, cloud, a factory reset or Hermes's store.
set -u; C=/home/orangepi/changes.txt; LK=/tmp/sd8213; n(){ echo "$(date '+%F %T') ROW kit #8213: $*" >> $C; }
SS=/usr/local/lib/lampos/skill-start; RUNS=/run/lampos-tasks/runs; FD=/run/systemd/system/lamp-voice.service.d/zz-sd-8213-first.conf
H(){ runuser -u lampos -- env HOME=/var/lib/lampos-tasks /usr/local/bin/harness "$@" 2>/dev/null; }
v=$1; shift
case $v in
pre) # read only: the door's pieces as through_door checks them, and lamp-voice's make switches
  echo "LAUNCHER $([ -f $SS ] && echo file $(sha256sum $SS | cut -c1-16) || echo absent)"
  echo "NS_START_LINE $(grep -c '^Q=/run/lampos-skill-start$' /usr/local/lib/lampos/agent-ns 2>/dev/null)"
  echo "START_SOCK $([ -S /run/lampos-skill-start/start.sock ] && echo yes || echo no)"
  p=$(systemctl show -p MainPID --value lamp-voice)
  for e in LAMP_NAME_NEAR LAMP_MAKE_SKILL_AGENT LAMP_MAKE_AGENT_ONE_PATH LAMP_MAKE_AGENT_FIRST LAMP_MAKE_NO_ASKS LAMP_MADE_SCRIPTS LAMP_SKILL_NAMED LAMP_SKILL_TASKS; do
    echo "VOICE $e=$(tr '\0' '\n' < /proc/$p/environ | sed -n "s/^$e=//p")"; done
  p=$(systemctl show -p MainPID --value lamp-tasks)
  for e in LAMP_TASKS_ONE_SESSION LAMP_SKILL_DEFAULT_AGENT LAMP_SKILL_TASKS LAMP_MAKE_AGENT_NET; do
    echo "TASKS $e=$(tr '\0' '\n' < /proc/$p/environ | sed -n "s/^$e=//p")"; done ;;
mute) python3 -I -c 'import json,sys
r=json.load(open("/var/lib/lampOS/voice/mute/record.json")); print("MUTE OFF OK" if r.get("muted") is False else "MUTE ON %s" % r.get("reason"))' 2>&1 ;;
panes) # every Harness pane of lamp-tasks: "name state" (read only)
  H ps --json | python3 -I -c 'import json,sys
def walk(x):
    if isinstance(x,dict):
        if "name" in x and ("state" in x or "id" in x): print(x.get("name"), x.get("state"), str(x.get("id",""))[:8])
        for v in x.values(): walk(v)
    elif isinstance(x,list):
        for v in x: walk(v)
try: walk(json.load(sys.stdin))
except Exception as e: print("PS unreadable", type(e).__name__)' ;;
runs) # read only: the runs folder's entries whenever they change, for SECS s (a token file is <run-id>)
  end=$(( $(date +%s) + ${1:-600} )); last=x; rm -f $LK/stop-$2
  while [ $(date +%s) -lt $end ] && [ ! -e $LK/stop-$2 ]; do now=$(ls -1 $RUNS 2>/dev/null | tr '\n' ' ')
    [ "$now" != "$last" ] && { echo "$(date +%T.%N | cut -c1-12) [$now]"; last=$now; }; sleep 0.5; done ;;
stopruns) touch $LK/stop-$1 ;;
made) # the made skill NAME's skill.toml (read only)
  for d in /var/lib/lampOS/skills/$1 /var/lib/lampOS/skills-dev/$1 /var/lib/lampOS/voice/made-skills/$1; do
    [ -f $d/skill.toml ] && { echo "== $d"; head -60 $d/skill.toml; ls $d/scripts 2>/dev/null | sed 's/^/script /'; }; done ;;
made-left) for w in lemon plum; do ls -d /var/lib/lampOS/skills/*$w* /var/lib/lampOS/skills-dev/*$w* 2>/dev/null | sed 's/^/LEFT made skill: /'; done
  echo "made-left checked" ;;
first) # on: lamp-voice's make goes to the lamp's agent first (test only, ships off), and the one run path, each only if not
  # already on; off: the drop-in removed. One lamp-voice restart each way.
  case $1 in
  on) p=$(systemctl show -p MainPID --value lamp-voice); env=$(tr '\0' '\n' < /proc/$p/environ); add=""
      for e in LAMP_MAKE_AGENT_FIRST LAMP_MAKE_AGENT_ONE_PATH; do echo "$env" | grep -qx "$e=on" || add="$add $e"; done
      [ -z "$add" ] && { echo "FIRST already on (nothing changed)"; exit 0; }
      mkdir -p ${FD%/*}; { echo '[Service]'; for e in $add; do echo "Environment=$e=on"; done; } > $FD
      systemctl daemon-reload; systemctl restart lamp-voice; n "lamp-voice drop-in $FD:$add =on; restarted"; sleep 15
      echo "FIRST SET$add lamp-voice $(systemctl is-active lamp-voice)" ;;
  off) [ -e $FD ] || { echo "FIRST none to remove"; exit 0; }
      rm -f $FD; systemctl daemon-reload; systemctl restart lamp-voice; n "lamp-voice drop-in removed; restarted"; sleep 15
      echo "FIRST REMOVED lamp-voice $(systemctl is-active lamp-voice)"
      # Put-back proof (manager 22 Nov: ships OFF): the drop-in file is gone and the running lamp-voice has no AGENT_FIRST=on.
      p=$(systemctl show -p MainPID --value lamp-voice)
      if [ ! -e $FD ] && ! tr '\0' '\n' < /proc/$p/environ | grep -qx 'LAMP_MAKE_AGENT_FIRST=on'; then
        echo "FIRST PROOF gone: no $FD; lamp-voice pid $p env has no LAMP_MAKE_AGENT_FIRST=on"
      else echo "FIRST PROOF FAIL: drop-in or env still there (pid $p)"; fi ;;
  esac ;;
launcher-hide) # the Hermes door-fallback leg only: /dev/null bound over the launcher, so it is no file (through_door)
  sha256sum $SS | cut -c1-16 > $LK/ss.sha; mount --bind /dev/null $SS && n "bound /dev/null over $SS (door-fallback leg)"
  [ -f $SS ] && echo "HIDE FAIL still a file" || echo "HIDDEN" ;;
launcher-show) while grep -q " $SS " /proc/self/mountinfo; do umount $SS || break; done; n "unbound $SS"
  [ "$(sha256sum $SS | cut -c1-16)" = "$(cat $LK/ss.sha 2>/dev/null)" ] && echo "LAUNCHER BACK same sha" || echo "LAUNCHER CHECK sha $(sha256sum $SS 2>/dev/null | cut -c1-16) vs $(cat $LK/ss.sha 2>/dev/null)" ;;
rowfiles) # the files the made skills' runs wrote (lemon, plum), newer than this kit's copy
  for f in $(find /var/lib/lampos-tasks/harnesses /var/lib/lampos-hermes /var/lib/lampos-openclaw -maxdepth 7 -type f \( -iname '*lemon*' -o -iname '*plum*' \) -newer $LK/x.sh 2>/dev/null); do
    if [ "${1:-}" = clean ]; then rm -f "$f" && n "removed the ROW's $f" && echo "removed $f"; else echo "== $f"; head -20 "$f"; fi; done; echo "rowfiles ${1:-listed}" ;;
*) echo "x verb?"; exit 2 ;;
esac
X
}

# ---------------------------------------------------------------- leg helpers
# grab TAG T0: the window's journal lines into files (tasks, voice, skills)
grab(){ r "since '$2' lamp-tasks 'tasks: |skill task|skill start|skill make'" > $L/j-$1-tasks.txt
  r "since '$2' lamp-voice 'a skill made here|a made skill|made by|read back|yes not taken|removed by voice'" > $L/j-$1-voice.txt
  r "since '$2' lamp-skills 'do\.task'" > $L/j-$1-skills.txt; }
c(){ grep -cE "$2" $L/j-$1-tasks.txt; true; }    # count of tasks lines matching (always one number)
cnt(){ grep -cE "$1" "$2" 2>/dev/null; true; }
kept_checks(){ # kept_checks TAG E K8: the Harness call lines of the window, as the PR's done-when says them
  local nw=$(c $1 'tasks: harness new:') rp=$(c $1 "tasks: harness reply:.* agent=$3.* exit=0") cl=$(c $1 "tasks: harness close:.* agent=$3")
  echo "new=$nw reply=$rp close=$cl"; }
lines(){ grep -hE "$2" $L/j-$1-*.txt | sed -E 's/^[A-Za-z]{3} [0-9]+ [0-9:.]+ [^ ]+ //' | cut -c1-200 | head -${3:-3} | tr '\n' '|'; }
watch_runs(){ x runs $1 $2 > $L/runs-$2.txt 2>&1 & WP=$!; }   # stopped by x stopruns TAG (else its own SECS bound)
unwatch(){ x stopruns $1 >/dev/null; sleep 1; kill $WP 2>/dev/null; }

leg_make(){ # leg_make E Evoice P(prefix O|H) WORD
  local e=$1 ev=$2 p=$3 w=$4 t a0 a1 i out name k
  renew; t=$(lt); a0=$(agent | cut -d' ' -f1); k=${a0:0:8}; watch_runs $((MAKE_WAIT+60)) ${p}MAKE
  say ${p}MAKE "Make me a skill that, when I say $w file, has an agent write three facts about ${w}s into a file." || { unwatch ${p}MAKE; return 1; }
  i=0; yes=0; out=""; while (( i < MAKE_WAIT )); do sleep 5; i=$((i+5)); (( i % 120 == 0 )) && renew
    out=$(r "since '$t' lamp-voice 'a made skill is on the lamp|made by DeepSeek|wasn.t kept|was not kept|was not put on|waits for the person.s yes|yes not taken'")
    echo "$out" | grep -qE 'a made skill is on the lamp|made by DeepSeek|wasn.t kept|was not kept|was not put on' && break
    if (( ! yes )) && echo "$out" | grep -q "waits for the person.s yes"; then sleep 8; say YES "Yes, go ahead." now; yes=1; fi; done
  sleep 5; unwatch ${p}MAKE; grab ${p}MAKE "$t"; a1=$(agent | cut -d' ' -f1)
  name=$(grep -oE 'a made skill is on the lamp: [^,]+' $L/j-${p}MAKE-voice.txt | tail -1 | sed 's/.*on the lamp: //'); echo "$name" > $L/name-$p.txt
  [ -n "$name" ] && x made "$name" > $L/made-$p.txt
  local hk=$(c ${p}MAKE "make: handed to $e in the lamp.s kept session in [0-9]+ ms") op=$(c ${p}MAKE "make: handed to $e on the one run path")
  local byag=$(cnt "made by the agent \($ev\)" $L/j-${p}MAKE-voice.txt) ds=$(cnt 'made by DeepSeek' $L/j-${p}MAKE-voice.txt)
  local kc=$(kept_checks ${p}MAKE $e $k)
  x panes > $L/panes-${p}MAKE.txt; local own=$(cnt '^(make|skill)-' $L/panes-${p}MAKE.txt)
  if (( ds )); then log "MAKE_$e VOID made by DeepSeek: the agent never got it (AGENT_FIRST=$AGENT_FIRST; j-${p}MAKE-voice.txt)"
  elif (( hk == 1 && op == 0 && byag >= 1 )) && [ -n "$name" ] && [ "$a0" = "$a1" ] && [[ $kc == "new=0 reply="[1-9]*" close=0" ]] && (( own == 0 )); then
    log "MAKE_$e PASS '$name' made in lamp-one-$e ($k, unchanged), $kc, no make-/skill- pane: $(lines ${p}MAKE 'make: handed to|made by the agent|a made skill is on|tasks: harness reply' 4)"
  else log "MAKE_$e FAIL kept-line=$hk one-run-path(new pane)=$op by-agent=$byag name='${name:-none}' agent $a0 -> $a1 $kc own-panes=$own (j-${p}MAKE-*.txt)"; fi
  grep -qE 'task *=|run *= *"scripts/' $L/made-$p.txt 2>/dev/null && log "MAKE_$e its skill has a do.task (made-$p.txt)" \
    || log "MAKE_$e CHECK '$name' has no do.task step: TRY/RUN won't reach lamp-tasks (made-$p.txt)"; }

leg_run(){ # leg_run E P KIND(TRY|RUN) WORD: the made skill run by voice, typed into lamp-one-E by `reply`
  local e=$1 p=$2 kd=$3 w=$4 t a0 a1 i id k
  renew; t=$(lt); a0=$(agent | cut -d' ' -f1); k=${a0:0:8}; watch_runs $((RUN_WAIT+60)) $p$kd
  say $p$kd "${(C)kd} $w file." || { unwatch $p$kd; return 1; }
  i=0; while (( i < RUN_WAIT )); do sleep 5; i=$((i+5))
    r "since '$t' lamp-skills 'do\.task: (handed to lamp-tasks|not handed)'" | grep -q . && break; done
  sleep 8; unwatch $p$kd; grab $p$kd "$t"; a1=$(agent | cut -d' ' -f1)
  id=$(grep -oE 'skill start: run [^ ]+ claimed by the launcher' $L/j-$p$kd-tasks.txt | head -1 | awk '{print $4}')
  local cl=$( [ -n "$id" ] && c $p$kd "skill start: run $id.s script started" || echo 0)
  local acc=$( [ -n "$id" ] && c $p$kd "tasks: $id: turn accepted in [0-9]+ ms" || echo 0)
  local fb=$(c $p$kd "its own pane, not the skill-start door|the kept session didn.t take it") kc=$(kept_checks $p$kd $e $k)
  local tok=$( [ -n "$id" ] && cnt "$id" $L/runs-$p$kd.txt || echo 0) end=$(cnt 'do.task: handed to lamp-tasks' $L/j-$p$kd-skills.txt)
  x panes > $L/panes-$p$kd.txt; local own=$( [ -n "$id" ] && cnt "^skill-$id " $L/panes-$p$kd.txt || echo 0) kp=$(cnt "^lamp-one-$e " $L/panes-$p$kd.txt)
  if [ -n "$id" ] && (( cl == 1 && acc == 1 && fb == 0 && tok == 0 && own == 0 && kp == 1 && end >= 1 )) && [ "$a0" = "$a1" ] && [[ $kc == "new=0 reply="[1-9]*" close=0" ]]; then
    log "${kd}_$e PASS run $id typed into lamp-one-$e ($k) by reply, $kc, no token file, no skill- pane: $(lines $p$kd "claimed by the launcher|turn accepted|tasks: harness reply|do.task: handed" 4)"
  else log "${kd}_$e FAIL id=${id:-none} script-started=$cl accepted=$acc fallback-lines=$fb token-file-seen=$tok own-pane=$own kept-pane=$kp do.task-ended=$end agent $a0 -> $a1 $kc (j-$p$kd-*.txt, runs-$p$kd.txt)"; fi
  log "${kd}_$e told: $(lines $p$kd 'what its agent found|what became of its run' 1)"; }

leg_perrun(){ # leg_perrun P KIND WORD: Hermes, (a): no exec allow-list, so the run keeps its own pane, its token file, said once
  local p=$1 kd=$2 w=$3 e=hermes t id k a0 a1 i; renew; t=$(lt); a0=$(agent | cut -d' ' -f1); k=${a0:0:8}; watch_runs $((RUN_WAIT+60)) $p$kd
  say $p$kd "${(C)kd} $w file." || { unwatch $p$kd; return 1; }
  i=0; while (( i < RUN_WAIT )); do sleep 5; i=$((i+5)); r "since '$t' lamp-skills 'do\.task: (handed to lamp-tasks|not handed)'" | grep -q . && break; done
  sleep 8; unwatch $p$kd; grab $p$kd "$t"; a1=$(agent | cut -d' ' -f1)
  id=$(grep -oE 'skill task [^ :]+: its own pane, not the skill-start door: its kept session is held to no exact exec allow-list' $L/j-$p$kd-tasks.txt | head -1 | awk '{print $3}' | tr -d :)
  local once=$( [ -n "$id" ] && c $p$kd "skill task $id: its own pane, not the skill-start door" || echo 0) nw=$(c $p$kd 'tasks: harness new:')
  local tok=$( [ -n "$id" ] && cnt "$id" $L/runs-$p$kd.txt || echo 0) claim=$(c $p$kd 'claimed by the launcher')
  local end=$(cnt 'do.task: handed to lamp-tasks' $L/j-$p$kd-skills.txt); x panes > $L/panes-$p$kd.txt; local kp=$(cnt "^lamp-one-$e " $L/panes-$p$kd.txt)
  if [ -n "$id" ] && (( once == 1 && nw >= 1 && tok >= 1 && claim == 0 && end >= 1 && kp == 1 )) && [ "$a0" = "$a1" ]; then
    log "${kd}_$e PASS per run (a): run $id kept its own pane (harness new $nw), its token file seen, no launcher claim, said once; lamp-one-$e ($k) kept: $(lines $p$kd 'its own pane, not the skill-start door|tasks: harness new|do.task: handed' 3)"
  else log "${kd}_$e FAIL id=${id:-none} said=$once new=$nw token-file-seen=$tok launcher-claims=$claim do.task-ended=$end kept-pane=$kp agent $a0 -> $a1 (j-$p$kd-*.txt, runs-$p$kd.txt)"; fi
  log "${kd}_$e told: $(lines $p$kd 'what its agent found|what became of its run' 1)"; }

leg_remove(){ # REMOVE: the made skill taken off by voice (its put-back, as a person does it)
  local p=$1 w=$2 t; renew; t=$(lt); say ${p}DEL "Remove the $w file skill." || return 1; sleep 30
  r "since '$t' lamp-voice 'removed by voice|a made skill'" > $L/j-${p}DEL.txt
  grep -q 'removed by voice' $L/j-${p}DEL.txt && log "REMOVE $w PASS $(head -1 $L/j-${p}DEL.txt | cut -c1-160)" || log "REMOVE $w CHECK not seen removed (j-${p}DEL.txt; put-back lists it)"; }

engine_to(){ # engine_to E: the app's choice as #8156's kit writes it, one lamp-tasks restart, kept line waited for
  gate 1; r "engine $1" > $L/eng-$1.txt; local s=$(sed -n 's/^SINCE .*(\([0-9]*\)s)$/\1/p' $L/eng-$1.txt)
  grep -qE "kept session [a-z]+ [0-9a-f]+ \($1" $L/eng-$1.txt && log "ENGINE $1 PASS kept session live in ${s}s: $(grep -oE "kept session [a-z]+ [0-9a-f]+ \($1[^)]*\)" $L/eng-$1.txt | tail -1)" \
    || log "ENGINE $1 FAIL no kept-session line within 120 s (eng-$1.txt)"; }

# ---------------------------------------------------------------- 1 pre (read only, then the agent-first drop-in)
log "ROW #8213 legs $HEAD8213 $L"
renew; copy || { log "STOP: copy failed"; exit 2; }
r snap > $L/snap0.txt; x pre | tee $L/pre.txt; agent | tee $L/agent0.txt
grep -q '^LAUNCHER file' $L/pre.txt && grep -q '^NS_START_LINE 1' $L/pre.txt && log "PRE PASS launcher and the agent namespace's skill-start line there (lamp-update's round ran)" \
  || { log "PRE FAIL the door's pieces are not there (pre.txt): the round did not write them; stop"; putback; exit 3; }
for t in OMAKE OTRY ORUN ODEL HMAKE HTRY HRUN HDEL; do [ -s $CLIPS/$t.wav ] || log "note: no clip $CLIPS/$t.wav (render first; see clips.txt)"; done
if (( AGENT_FIRST )); then gate 1; x first on | tee $L/first.txt; grep -q '^FIRST SET' $L/first.txt && FIRST_SET=1; fi

# ---------------------------------------------------------------- 2-5 OpenClaw (the no-regression baseline)
if step 2; then [ "$(agent | cut -d' ' -f2)" = openclaw ] && log "ENGINE openclaw already the default: $(agent)" || engine_to openclaw; onea oc openclaw
  leg_make openclaw OpenClaw O lemon; fi
step 3 && leg_run openclaw O TRY lemon
step 4 && leg_run openclaw O RUN lemon
if step 5; then n=$(cat $L/j-OTRY-tasks.txt $L/j-ORUN-tasks.txt 2>/dev/null | grep -cE 'its own pane, not the skill-start door|the kept session didn.t take it')
  (( ${n:-0} == 0 )) && log "DOOR_openclaw PASS no fallback line in TRY/RUN (a gateway agent has no door fallback; passive check)" || log "DOOR_openclaw FAIL $n fallback lines"
  leg_remove O lemon; fi

# ---------------------------------------------------------------- 6-8 Hermes
if step 6; then engine_to hermes; sleep 5; r ready > $L/ready-h.txt; onea h hermes; leg_make hermes Hermes H plum; fi
step 7 && leg_perrun H TRY plum
if step 8; then leg_perrun H RUN plum; leg_remove H plum; fi

# ---------------------------------------------------------------- 9 back to OpenClaw, its kept session live
if step 9; then engine_to openclaw
  i=0; while (( i < 120 )); do x panes | grep -q '^lamp-one-openclaw ' && break; sleep 5; i=$((i+5)); done
  x panes > $L/panes-end.txt; grep -q '^lamp-one-openclaw ' $L/panes-end.txt && log "OPENCLAW_REOPENED PASS lamp-one-openclaw in harness ps after ${i}s" || log "OPENCLAW_REOPENED FAIL not in harness ps within 120 s"
  onea back openclaw; fi

putback
ok=PASS; grep -qE ' FAIL|^STOP|LEFT ' $V && ok=FAIL; grep -q ' VOID' $V && [ $ok = PASS ] && ok=VOID-PART
B=$(grep -m1 '^build' $L/snap0.txt | cut -d' ' -f2)
{ echo "ROW #8213 $HEAD8213 on $LAMP build ${B:-?}: $ok"; grep -E 'PASS|FAIL|CHECK|VOID|LEFT|STOP' $V; echo "log: $L"; } | tee $L/row.txt
