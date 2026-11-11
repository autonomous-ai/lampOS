#!/bin/zsh
# Kit 5194 driver, run on the Mac beside lamp-06c1 (ssh alias blue-lamp), after setup.sh said
# `setup done`. PLAYS SOUND from the Mac speaker at volume 45 (never above), with afplay.
#   nohup zsh $K/mac/driver.zsh > $K/driver.out 2>&1 & echo $! > $K/driver.pid
#   PASSES="on" zsh $K/mac/driver.zsh     # only the on pass (the off pass already played)
# Order: the off pass (stock, LAMP_OPENS_FURTHER unset) -> switch.sh on (drop-in + one voice
# restart) -> the on pass -> putback.sh -> logs copied -> analyze.py (transcript.tsv, report.md).
# Every row is its own conversation: after the lamp's reply it waits GAP s (50: the voice's
# conversation window GOING_ON_FOR is 45 s), so "at most two openings in one conversation" and
# "never right after an opening" never join two rows.
# Stops early and puts back when $K/STOP appears, when the voice reads muted before a row, or when a
# weekday's 12:00-14:00 (the office rests; LAMP_PLAY_AT_LUNCH=yes overrides) would be played into.
set -u
K=${0:A:h:h}; PASS=orangepi; HOST=blue-lamp; LOG=$K/driver.log; CL=$K/clips
VOL=45; GAP=${GAP:-50}; NOREPLY=${NOREPLY:-25}; MAXWAIT=${MAXWAIT:-75}; PASSES=${PASSES:-"off on"}
BUSY_LEAD=${BUSY_LEAD:-30}; STOP_INTO=${STOP_INTO:-4}; TALK_VOL=${TALK_VOL:-0.6}
mkdir -p $K/logs
L() { sshpass -p $PASS ssh -o ConnectTimeout=15 -o LogLevel=ERROR $HOST "echo $PASS | sudo -S -p '' sh /run/kit5194/$*"; }
Lq() { L q.sh "$@" 2>/dev/null; }
now() { python3 -c 'import time; print(f"{time.time():.3f}")'; }
log() { print -r -- "$(date '+%Y-%m-%dT%H:%M:%S%z') $*" | tee -a $LOG; }
loud() { log "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"; log "!!! $*"; log "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"; }
stopped() { [[ -e $K/STOP ]]; }
lunch() { [[ ${LAMP_PLAY_AT_LUNCH:-} == yes ]] && return 1; local d=$(date +%u) h=$(date +%H); (( d <= 5 && 10#$h >= 12 && 10#$h < 14 )); }
vol_set() {
  osascript -e "set volume output volume $VOL without output muted" >/dev/null
  local v=$(osascript -e 'output volume of (get volume settings)') m=$(osascript -e 'output muted of (get volume settings)')
  (( v <= VOL && v >= VOL - 1 )) && [[ $m == false ]] || { log "VOLUME WRONG: $v muted=$m, not playing"; return 1; }
}
vol_cap() { local v=$(osascript -e 'output volume of (get volume settings)'); (( v > VOL )) && osascript -e "set volume output volume $VOL" >/dev/null; }
TALKPID=""
talk_off() { [[ -n $TALKPID ]] && kill $TALKPID 2>/dev/null; TALKPID=""; }
putback() {
  mkdir $K/.putback 2>/dev/null || return 0
  talk_off; vol_cap
  log "put-back: $1"
  L putback.sh 2>&1 | tee $K/putback.out | tee -a $LOG
  grep -q '^OWED' $K/putback.out && { touch $K/putback-owed; loud "PUT-BACK OWED: run putback.sh again once lamp-update is quiet"; }
  grep -q 'THE VOICE IS MUTED' $K/putback.out && { touch $K/VOICE-MUTED; loud "THE LAMP'S VOICE IS MUTED AFTER PUT-BACK: tell the person; say so in released"; }
  grep -q '^VOICE OK' $K/putback.out && log "PUT BACK; read the SAME/DIFF lines"
}
copy_logs() {   # built on the lamp with GNU tar: no AppleDouble files
  Lq pack >/dev/null \
    && sshpass -p $PASS scp -o LogLevel=ERROR $HOST:/tmp/kit5194-logs.tgz $K/logs/ \
    && (cd $K/logs && tar -xzf kit5194-logs.tgz) \
    && sshpass -p $PASS ssh -o LogLevel=ERROR $HOST "echo $PASS | sudo -S -p '' rm -f /tmp/kit5194-logs.tgz" \
    && log "logs copied to $K/logs"
}
finish() {
  mkdir $K/.finishing 2>/dev/null || return 0
  log "finish: $1"
  putback "$1"
  copy_logs
  python3 -I $K/mac/analyze.py $K > $K/analyze.out 2>&1 && log "analysis: $K/transcript.tsv, $K/report.md" || log "analysis failed: $K/analyze.out"
  [[ -f $K/caffeinate.pid ]] && kill $(cat $K/caffeinate.pid) 2>/dev/null
  [[ -e $K/VOICE-MUTED ]] && loud "REMINDER: the voice was left MUTED (see putback.out)"
  log "remove the kit from the lamp once putback.out and the logs are read: rm -rf /run/kit5194"
  touch $K/finished
  exit 0
}
trap 'finish "driver interrupted"' INT TERM
guard() {
  stopped && finish "STOP before $1"
  lunch && finish "12:00-14:00 on a weekday before $1 (the office rests)"
  local m=$(Lq mute)
  if [[ $m != *'"muted":false'* ]]; then
    sleep 5; m=$(Lq mute)
    [[ $m == *'"muted":false'* ]] || { loud "VOICE MUTED (or unread) before $1: $m -- no more rows"; finish "voice muted before $1"; }
  fi
}
# Waits for the lamp's reply to end: done once it was speaking and then not, two reads running; no
# reply when it never speaks within NOREPLY s; at most MAXWAIT s. Prints "spoke" or "silent".
wait_reply() {
  local t0=$(date +%s) seen=0 q=0 st
  while (( $(date +%s) - t0 < MAXWAIT )); do
    st=$(Lq vstate)
    if [[ $st == speaking ]]; then seen=1; q=0
    elif (( seen )); then (( q += 1 )); (( q >= 2 )) && break
    elif (( $(date +%s) - t0 >= NOREPLY )); then break
    fi
    sleep 1
  done
  (( seen )) && print spoke || print silent
}
wait_speaking() {   # up to $1 s for voice/state speaking; 0 = speaking
  local t0=$(date +%s)
  while (( $(date +%s) - t0 < $1 )); do [[ $(Lq vstate) == speaking ]] && return 0; sleep 1; done; return 1
}
# One row: pass, row, kind, how, clip. Logs a line to plays.tsv with the lamp's clocks at the clip's start.
play_row() {
  local pass=$1 row=$2 kind=$3 how=$4 clip=$CL/$5.aiff pre="" lt as r
  guard "$pass/$row"
  as=$(Lq asleep)
  vol_set || finish "volume could not be set to $VOL"
  case $how in
    stop)
      local pt=$(Lq now); afplay $CL/${row}pre.aiff; vol_cap
      Lq mark "$pass $row pre-played" >/dev/null
      if wait_speaking 30; then sleep $STOP_INTO; pre="pre_at=${pt%% *} speaking"
      else pre="pre_at=${pt%% *} NOT-SPEAKING"; log "$row: the long ask got no speech in 30 s; the stop is played anyway (marked)"; fi;;
    busy)
      afplay -v $TALK_VOL $CL/talk.aiff & TALKPID=$!
      pre="talk_from=$(now)"
      sleep $BUSY_LEAD;;
  esac
  lt=$(Lq now)
  local m0=$(now); afplay $clip; local m1=$(now)
  vol_cap
  r=$(wait_reply)
  [[ $how == busy ]] && { sleep 3; talk_off; }
  log "PLAY $pass $row lamp=$lt mac=$m0-$m1 $as reply=$r $pre"
  print -r -- "$pass	$row	$kind	${lt%% *}	${lt##* }	$m0	$m1	$r	${as:-?}	$pre" >> $K/plays.tsv
  local i=0; while (( i < GAP )); do stopped && finish "STOP after $pass/$row"; sleep 5; (( i += 5 )); done
}
play_pass() {
  local pass=$1
  log "pass $pass: lamp clock $(Lq now) vs Mac $(now)"
  Lq mark "pass $pass begins" >/dev/null
  grep -v '^#' $K/mac/lines.tsv | while IFS=$'\t' read -r row kind how clip line; do
    [[ $kind == pre || -z $row ]] && continue
    [[ -n ${ROWS:-} && " $ROWS " != *" $row "* ]] && continue
    play_row $pass $row $kind $how $clip </dev/null
  done
  Lq mark "pass $pass ends" >/dev/null
  log "$(Lq events $pass)"
}

[[ -e $K/.finishing || -e $K/.putback ]] && { print "an earlier run finished here: move plays.tsv, driver.log and logs/ aside, then rm -rf $K/.putback $K/.finishing $K/finished"; exit 1; }
[[ -e $K/plays.tsv ]] || print "pass	row	kind	lamp_wall	lamp_mono_us	mac_start	mac_end	reply	asleep_before	pre" > $K/plays.tsv
[[ -e $CL/SHA256SUMS ]] && (cd $CL && shasum -a 256 -c --quiet SHA256SUMS) || { print "clips/SHA256SUMS does not check: run mac/render.zsh"; exit 1; }
[[ -n $(Lq now) ]] || { print "the lamp does not answer q.sh (setup.sh not run?)"; exit 1; }
caffeinate -dimsu -t 14400 & print $! > $K/caffeinate.pid
log "driver up; passes: $PASSES; gap $GAP s; Mac volume $VOL"
for p in ${=PASSES}; do
  if [[ $p == on ]]; then
    guard "the switch"
    log "switch on"
    L switch.sh on 2>&1 | tee $K/switch.out | tee -a $LOG
    grep -q '^SWITCH-ON-OK' $K/switch.out || { loud "the switch did not go on (switch.out); no on pass"; finish "switch failed"; }
    sleep 20
  fi
  play_pass $p
done
finish "both passes played"
