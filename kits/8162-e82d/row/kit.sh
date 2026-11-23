# Mac helpers (bash 3.2), sourced by every Mac script of row/ and by ../run.sh (#8162 ROW kit, lamp-ee17 by default).
# Every lamp-specific value is a setting, read from the runner's env file (KIT_ENV) or the environment:
#   LAMP          the lamp's name (default lamp-ee17); used for the lock (lamp-lock.sh <LAMP>), the logs and the ROW line
#   LAMP_HOST     its address (default <LAMP>.local)
#   LAMP_USER     its ssh user (default orangepi; its home keeps the row's records on the lamp: ~/t8162)
#   KIT_ENV       the runner's env file (sourced, never copied or printed). It holds LAMP_SUDO_PASS (the lamp's sudo
#                 password) and, optional, LAMP_SSH_PASS for sshpass (unset: ssh uses the runner's keys)
#   LOCK_WHO      the lock's holder (default thirteen); LOCK_SESSION its session (default row-8162)
#   KIT_LL        the lamp-lock script (optional). With none, NO lock is taken or renewed (RENEW always passes).
#   ROUND1_SEQ, ROUND2_SEQ   the two rounds' sequences (required by round1/round2): the PR build, then stable's bytes
#   ROUND1_CMD, ROUND2_CMD   the runner's own command that makes the lamp's lamp-update take that round (cmd mode)
#   CH1, CH2, TEST_PUB       or: two channel folders the runner made and signed, and their public key (channel mode)
#   PR_UPDATE_SHA, PR_TASKS_SHA   optional: the sha256 of the PR build's lamp-update and lamp-tasks (verify checks them)
D=${D:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}
KIT=$(cd "$D/.." && pwd)                          # the kit (kits/8162-e82d)
KIT_ENV=${KIT_ENV:-}
[ -n "$KIT_ENV" ] && [ -f "$KIT_ENV" ] && . "$KIT_ENV"
LAMP=${LAMP:-lamp-ee17}; LAMP_USER=${LAMP_USER:-orangepi}; LAMP_HOST=${LAMP_HOST:-$LAMP.local}
H=$LAMP_USER@$LAMP_HOST
LL=${KIT_LL:-}
LOCK_LAMP=$LAMP; LOCK_WHO=${LOCK_WHO:-thirteen}; LOCK_SESSION=${LOCK_SESSION:-row-8162}
# The PR and the start state (#8162's posts 7081048222 and 7081052398: stable 15543, updater e988ff0a4d27; Harness pin
# 0.6.75 on stable, 0.6.78 in the PR build). The stable values were read from the stable feed's signed manifest.
PR=8162
HEAD_SHA=${HEAD_SHA:-4c663af40fd1928ce5ebb8fc20cca2d08d1604e6}
STABLE_SEQ=${STABLE_SEQ:-15543}
STABLE_UPD_SHA=${STABLE_UPD_SHA:-e988ff0a4d275afcae5cff5062df59133c838efd673b0573384b5adcc162f7f4}
STABLE_TASKS_SHA=${STABLE_TASKS_SHA:-b93a1471db20d0372a824ee004ac16abba37d0857ca60732b6657dc84914a175}
HV_STABLE=${HV_STABLE:-0.6.75}; HV_PR=${HV_PR:-0.6.78}
SPK_MAX=${SPK_MAX:-40}                            # the lamp's speaker volume, read only at base: at most this %
A=${A:-$D/clips}                                  # pre-rendered clips (clips.txt, SHAS)
L=${ROWLOGS:-$D/logs}; mkdir -p $L
PWF=${PWF:-}
pw(){ if [ -n "${LAMP_SUDO_PASS:-}" ]; then printf '%s\n' "$LAMP_SUDO_PASS"; elif [ -n "$PWF" ]; then cat "$PWF"; echo; else echo; fi; }
# ssh/scp: sshpass -e when LAMP_SSH_PASS is set (the password goes by the environment, never on a command line).
if [ -z "${SSH:-}" ]; then
  if [ -n "${LAMP_SSH_PASS:-}" ]; then export SSHPASS="$LAMP_SSH_PASS"; SSH="sshpass -e ssh -o StrictHostKeyChecking=accept-new"; SCP="sshpass -e scp -o StrictHostKeyChecking=accept-new"
  else SSH="ssh -o StrictHostKeyChecking=accept-new"; SCP="scp -o StrictHostKeyChecking=accept-new"; fi
fi
SCP=${SCP:-scp}
# SSHR: $SSH, run again (at most 3 tries, 2 s apart) only when the lamp refused the login (exit 255 or 5 with
# "Permission denied"); any other failure is returned at once. stdin (a LAMP body) is kept in memory and fed to each try.
SSHR(){ local i=1 rc e a n=0 in=; e=$(mktemp "${TMPDIR:-/tmp}/kit8162-ssh.XXXXXX")
  for a in "$@"; do [ "$a" = -n ] && n=1; done
  [ $n = 1 ] || { in=$(cat; echo x); in=${in%x}; }
  while :; do
    if [ $n = 1 ]; then $SSH "$@" 2>"$e"; else printf '%s' "$in" | $SSH "$@" 2>"$e"; fi; rc=$?
    if [ $i -lt 3 ] && { [ $rc = 255 ] || [ $rc = 5 ]; } && grep -q 'Permission denied' "$e"; then
      echo "ssh: the lamp refused the login (try $i of 3): again in 2 s" >&2; i=$((i + 1)); sleep 2; continue; fi
    cat "$e" >&2; rm -f "$e"; return $rc
  done; }
# A lamp body as root: the password (only to sudo's stdin), the settings line, the prelude, an optional "VAR=val ..."
# line, the files.
LAMP(){ local pre=$1; shift; { pw; printf 'LAMP_USER=%s; LOCK_WHO=%s; PR=%s\n' "$LAMP_USER" "$LOCK_WHO" "$PR"; cat $D/common.lamp.sh; [ -n "$pre" ] && printf '%s\n' "$pre"; cat "$@"; } |
  SSHR -o ServerAliveInterval=10 -o ServerAliveCountMax=3 -o ConnectTimeout=15 $H 'if sudo -n true 2>/dev/null; then read -r _x; sudo sh -s; else sudo -S -p "" sh -s; fi'; }
# Our lock only, when there is one (KIT_LL): every step and leg starts with RENEW; a failed renew stops it before any ssh.
RENEW(){ [ -n "$LL" ] || return 0
  $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 || { echo "LOCK: not ours ($LOCK_LAMP $LOCK_WHO $LOCK_SESSION): stop, no ssh"; return 1; }; }
# The renew loop (lock step) writes RENEW FAILED to logs/renew.log when a renew fails: LOCKLOST stops at the next look.
LOCKLOST(){ [ -n "$LL" ] && [ -f $L/renew.log ] && tail -1 $L/renew.log | grep -q 'RENEW FAILED' && { echo "LOCK: the renew loop failed ($(tail -1 $L/renew.log)): stop, no ssh"; return 0; }; return 1; }
# Mac volume, READ ONLY (<= 70; never set). The lamp's speaker volume is never read or set by any script here.
MACVOL(){ ${OSASCRIPT:-osascript} -e 'output volume of (get volume settings)'; }
VOLOK(){ local v; v=$(MACVOL); [ "$v" -le 70 ] 2>/dev/null || { echo "Mac volume $v > 70 (or unread): stop and ask (never set it)"; return 2; }; }
NOW(){ python3 -c 'import time;print("%.3f"%time.time())'; }
GAIN(){ awk -F'\t' -v c=$1 '$1 == c {print ($3 == "" ? "1.0" : $3)}' $A/clips.txt | head -1; }
WORDS(){ awk -F'\t' -v c=$1 '$1 == c {print $2}' $A/clips.txt | sed -e 's/^Hey Lamp, //' -e 's/^OpenClaw, //'; }
# PLAY id: clips/<id>.wav at the clip's gain (afplay -v, the Mac's own output); logs/plays.log has the Mac epoch.
PLAY(){ local id=$1 f=$A/$1.wav g; VOLOK || return 2; [ -f "$f" ] || { echo "no clip $f"; return 3; }
  g=$(GAIN $id); g=${g:-1.0}
  echo "$(NOW) $(date '+%T') PLAY $id gain=$g leg=${LEG:-?} (Mac vol $(MACVOL))" | tee -a $L/plays.log
  ${AFPLAY:-afplay} -v "$g" "$f"; echo "$(NOW) $(date '+%T') PLAYED $id leg=${LEG:-?}" >> $L/plays.log; }
# Followers. JF: the lamp's journal (short-unix: the lamp's epoch first) for SECS s. AF: tasks/agent's kept value
# (AW lines) and {"op":"agent"} (AO lines), read only (aw.lamp.sh).
JUNITS="-u lamp-voice -u lamp-tasks -u lamp-openclaw-gateway -u lamp-update -u lamp-update-harness -u lamp-cloud"
JF(){ $SSH -n $H "LEG=${LEG:-none} timeout ${2:-1200} journalctl -f -n0 -o short-unix $JUNITS" >> $1 2>/dev/null & echo $!; }
AF(){ LAMP "LEG=${LEG:-none} SECS=${2:-1200}" $D/aw.lamp.sh >> $1 2>/dev/null < /dev/null & echo $!; }
# The skew, lamp - Mac, in s (the midpoint of one date read).
SKEW(){ local a l b; a=$(NOW); l=$(SSHR -n $H 'date +%s.%N'); b=$(NOW)
  python3 -c "import sys;a,l,b=map(float,sys.argv[1:]);print('%.3f %.3f'%(l-(a+b)/2,b-a))" $a $l $b; }
# Start budget: at most 10 starts of a unit in any 10 min (the lamp's own journal counts every start, the kit's and
# the lamp's own). GATE <n> <unit...>: refused when a unit has started more than 10 - n times in the last 10 min.
STARTS(){ SSHR -n $H "journalctl -u $1 --since -10min -o cat --no-pager" 2>/dev/null | grep -c "^Started " ; }
GATE(){ local n=$1 u c; shift; for u in "$@"; do c=$(STARTS $u); [ $(( ${c:-0} + n )) -le 10 ] || { echo "GATE: $u started ${c:-?} times in the last 10 min, $n more would pass 10: wait"; return 1; }; done; }
# Guards, each idempotent and each run on ANY exit of run.sh or row/legs.sh (their EXIT traps), in this order:
#   STOPGUARD  a pid the kit SIGSTOPped (leg L4: logs/stopped.on) SIGCONTed (nostop.lamp.sh, by number only)
#   WRAPGUARD  the OpenClaw wrapper and openclaw.json copies of legs L7/L8 put back if still the kit's (logs/wrap.on)
#   HOLDGUARD  the runner's UNHOLD_CMD after their HOLD_CMD (leg L9: logs/hold.on)
#   MICGUARD   the recorded mute put back (micoff.lamp.sh), if micon lifted it
# A guard needs our lock (RENEW) before any ssh; with the lock lost it only says what is left to do.
G_SSH(){ RENEW >/dev/null || { echo "WARN: lock lost: $1 NOT done: do it by hand ($2)"; return 1; }; }
STOPGUARD(){ [ -f $L/stopped.on ] || return 0; echo "guard: a pid the kit stopped may still be stopped: SIGCONT now"
  G_SSH "SIGCONT" "./run.sh putback runs nostop" || return 0
  LAMP "" $D/nostop.lamp.sh > $L/nostop.txt 2>&1; cat $L/nostop.txt
  grep -q '^NOSTOP OK' $L/nostop.txt && rm -f $L/stopped.on || echo "WARN: a pid may still be stopped (row/logs/nostop.txt)"; }
WRAPGUARD(){ [ -f $L/wrap.on ] || return 0; echo "guard: the OpenClaw wrapper/config change of L7/L8 may be left: putting it back"
  G_SSH "the wrapper put-back" "./run.sh putback runs l7back" || return 0
  LAMP "" $D/l7back.lamp.sh > $L/l7back.txt 2>&1; cat $L/l7back.txt
  grep -q '^L7BACK OK' $L/l7back.txt && rm -f $L/wrap.on || echo "WARN: wrapper/config NOT back (row/logs/l7back.txt)"; }
HOLDGUARD(){ [ -f $L/hold.on ] || return 0; echo "guard: leg L9's hold may be on: running UNHOLD_CMD"
  [ -n "${UNHOLD_CMD:-}" ] || { echo "WARN: no UNHOLD_CMD: clear the hold by hand"; return 0; }
  LAMP_HOST=$LAMP_HOST bash -c "$UNHOLD_CMD" >> $L/hold.txt 2>&1 && { rm -f $L/hold.on; echo "  UNHOLD done"; } || echo "WARN: UNHOLD_CMD failed (row/logs/hold.txt)"; }
MICGUARD(){ grep -q '^MUTE LIFTED OK' $L/micon.txt 2>/dev/null || return 0; grep -q '^MUTE BACK OK' $L/micoff.txt 2>/dev/null && return 0
  echo "guard: the mic is unmuted by the kit: putting the mute back now"
  G_SSH "the mute put-back" "./run.sh micoff" || return 0
  GATE 1 lamp-voice || { echo "WARN: no start budget: mute NOT back: wait 10 min, then ./run.sh micoff"; return 0; }
  LAMP "" $D/micoff.lamp.sh > $L/micoff.txt 2>&1; cat $L/micoff.txt; grep -q '^MUTE BACK OK' $L/micoff.txt || echo "WARN: mute NOT back: ./run.sh micoff"; }
LEGGUARDS(){ STOPGUARD; WRAPGUARD; HOLDGUARD; }
