# Mac helpers (bash 3.2), sourced by every Mac script of row/ and by ../run.sh (#8180 legs kit, any lamp).
# Every lamp-specific value is a setting, read from the runner's env file (KIT_ENV) or the environment:
#   LAMP        the lamp's name (default lamp-ee17); used for the lock and the logs only
#   LAMP_HOST   its address (required; e.g. 172.168.20.193)
#   LAMP_USER   its ssh user (default orangepi; also the home that keeps the row's baselines on the lamp)
#   KIT_ENV     the runner's env file (sourced, never copied or printed); it holds LAMP_SUDO_PASS (the lamp's sudo
#               password) and, for sshpass, LAMP_SSH_PASS (the ssh password; with none, ssh uses the runner's keys)
#   LOCK_WHO    the lock's holder (default migrate); LOCK_SESSION its session (default row-8180)
#   KIT_LL      the lamp-lock script (optional). With none, NO lock is taken or renewed (RENEW always passes).
#   PWF         optional 0600 file holding the sudo password, if LAMP_SUDO_PASS is not in KIT_ENV
#   PR_{TASKS,CONTROL,SKILLS,VOICE,UPDATE}_SHA  optional: the sha256 of #8180's artifact binaries; check.lamp.sh
#               then requires the running units to be exactly them (else it only prints what runs)
D=${D:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}
KIT=$(cd "$D/.." && pwd)                          # the kit (kits/8180-e82d)
KIT_ENV=${KIT_ENV:-}
[ -n "$KIT_ENV" ] && [ -f "$KIT_ENV" ] && . "$KIT_ENV"
LAMP=${LAMP:-lamp-ee17}; LAMP_USER=${LAMP_USER:-orangepi}
[ -n "${LAMP_HOST:-}" ] || { echo "LAMP_HOST is not set (KIT_ENV=$KIT_ENV): set it in the env file"; exit 2; }
H=$LAMP_USER@$LAMP_HOST
A=${A:-$D/clips}                                  # pre-rendered ElevenLabs clips (render.sh)
LL=${KIT_LL:-}
LOCK_LAMP=$LAMP; LOCK_WHO=${LOCK_WHO:-migrate}; LOCK_SESSION=${LOCK_SESSION:-row-8180}
PR=8180
PWF=${PWF:-}
mkdir -p $D/logs
pw(){ if [ -n "${LAMP_SUDO_PASS:-}" ]; then printf '%s\n' "$LAMP_SUDO_PASS"; elif [ -n "$PWF" ]; then cat "$PWF"; echo; else echo; fi; }
# ssh/scp: sshpass -e when LAMP_SSH_PASS is set (the password goes by the environment, never on a command line).
# accept-new: a first connection to the lamp records its host key instead of waiting on a prompt sshpass can't answer.
if [ -z "${SSH:-}" ]; then
  if [ -n "${LAMP_SSH_PASS:-}" ]; then export SSHPASS="$LAMP_SSH_PASS"; SSH="sshpass -e ssh -o StrictHostKeyChecking=accept-new"; SCP="sshpass -e scp -o StrictHostKeyChecking=accept-new"
  else SSH="ssh -o StrictHostKeyChecking=accept-new"; SCP="scp -o StrictHostKeyChecking=accept-new"; fi
fi
SCP=${SCP:-scp}
# SSHR: $SSH, run again (at most 3 tries, 2 s apart) only when the lamp refused the login: exit 255 (ssh) or 5
# (sshpass) with "Permission denied" in its stderr; any other failure is returned at once. Without -n, stdin (a LAMP
# body) is kept in memory, never in a file, and fed again to each try. Its stderr is passed on once the try ends.
SSHR(){ local i=1 rc e a n=0 in=; e=$(mktemp "${TMPDIR:-/tmp}/kit8180-ssh.XXXXXX")
  for a in "$@"; do [ "$a" = -n ] && n=1; done
  [ $n = 1 ] || { in=$(cat; echo x); in=${in%x}; }
  while :; do
    if [ $n = 1 ]; then $SSH "$@" 2>"$e"; else printf '%s' "$in" | $SSH "$@" 2>"$e"; fi; rc=$?
    if [ $i -lt 3 ] && { [ $rc = 255 ] || [ $rc = 5 ]; } && grep -q 'Permission denied' "$e"; then
      echo "ssh: the lamp refused the login (try $i of 3): again in 2 s" >&2; i=$((i + 1)); sleep 2; continue; fi
    cat "$e" >&2; rm -f "$e"; return $rc
  done; }
# Run a lamp body as root: password (only to sudo's stdin), the lamp settings, the row prelude, an optional
# "VAR=val" line, the files.
LAMP(){ local pre=$1; shift; { pw; printf 'LAMP_USER=%s; LOCK_WHO=%s\n' "$LAMP_USER" "$LOCK_WHO"; cat $D/common.lamp.sh; [ -n "$pre" ] && printf '%s\n' "$pre"; cat "$@"; } |
  SSHR -o ServerAliveInterval=10 -o ServerAliveCountMax=3 -o ConnectTimeout=15 $H 'if sudo -n true 2>/dev/null; then read -r _x; sudo sh -s; else sudo -S -p "" sh -s; fi'; }
# Our lock only, when there is one (KIT_LL): every Mac script starts with RENEW; a failed renew stops it before any ssh.
RENEW(){ [ -n "$LL" ] || return 0
  $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 || { echo "LOCK: not ours ($LOCK_LAMP $LOCK_WHO $LOCK_SESSION): stop, no ssh"; return 1; }; }
# Mac volume, READ ONLY (<= 70; never set). The lamp's speaker volume is never touched by any script here.
MACVOL(){ ${OSASCRIPT:-osascript} -e 'output volume of (get volume settings)'; }
VOLOK(){ local v; v=$(MACVOL); [ "$v" -le 70 ] 2>/dev/null || { echo "Mac volume $v > 70 (or unread): stop and ask (never set it)"; return 2; }; }
NOW(){ python3 -c 'import time;print("%.3f"%time.time())'; }
GAIN(){ awk -F'\t' -v c=$1 '$1 == c {print ($3 == "" ? "1.0" : $3)}' $A/clips.txt | head -1; }
# PLAY id: plays clips/<id>-el.wav; sets PEND to the Mac epoch at the clip's end (the ask's end).
PLAY(){ local id=$1 f=$A/$1-el.wav g; VOLOK || return 2; [ -f "$f" ] || { echo "no clip $f (./render.sh)"; return 3; }
  g=$(GAIN $id); g=${g:-1.0}
  echo "$(date '+%T') PLAY $id gain=$g leg=${LEG:-?} (Mac vol $(MACVOL))" | tee -a $D/logs/plays.log
  ${AFPLAY:-afplay} -v "$g" "$f"; PEND=$(NOW); echo "$(date '+%T') PLAYED $id end=$PEND" >> $D/logs/plays.log; }
JF(){ $SSH -n $H "journalctl -f -n0 -o short-unix -u lamp-voice -u lamp-tasks -u lamp-skills -u 'lamp-skills-manage@*' -u lamp-control -u lamp-openclaw-gateway -u lamp-update" >> $1 2>&1 & echo $!; }
SKEW(){ local a l b; a=$(NOW); l=$(SSHR -n $H 'date +%s.%N'); b=$(NOW)
  python3 -c "import sys;a,l,b=map(float,sys.argv[1:]);print('%.3f %.3f'%(l-(a+b)/2,b-a))" $a $l $b; }
# Start budget: at most 10 starts in any 10 min per unit (the lamp's own journal counts). A start is allowed at <= 8.
STARTS(){ SSHR -n $H "journalctl -u $1 --since -10min -o cat --no-pager" 2>/dev/null | grep -c "^Started " ; }
GATE(){ local u c; for u in ${@:-lamp-voice}; do c=$(STARTS $u); [ "${c:-0}" -le 8 ] || { echo "GATE: $u started $c times in the last 10 min: wait"; return 1; }; done; }
# Mac-mic recording of a leg (MAC_REC=1 only): ffmpeg avfoundation, the Mac's built-in microphone, 16 kHz mono, at most
# 15 min, to <logs>/leg-<L>.mac.wav. The volume settings are read before and after (logs/macvol.txt); no volume,
# input or output, is ever set.
RECP=
VOLS(){ ${OSASCRIPT:-osascript} -e 'get volume settings' 2>&1; }
RECSTART(){ [ "${MAC_REC:-0}" = 1 ] || return 0; command -v ffmpeg >/dev/null || { echo "  MAC_REC: no ffmpeg: not recorded"; return 0; }
  local dev; dev=$(ffmpeg -hide_banner -f avfoundation -list_devices true -i "" 2>&1 | sed -n 's/.*\] \[\([0-9]*\)\] MacBook Pro Microphone.*/\1/p' | head -1)
  [ -n "$dev" ] || { echo "  MAC_REC: no MacBook Pro Microphone: not recorded"; return 0; }
  echo "$(date +%T) before $1: $(VOLS)" >> $D/logs/macvol.txt; RECV0=$(VOLS)
  ffmpeg -nostdin -hide_banner -loglevel error -y -f avfoundation -i ":$dev" -ac 1 -ar 16000 -t 900 "$2" 2>>$D/logs/macrec.err & RECP=$!; }
RECSTOP(){ [ -n "$RECP" ] || return 0; kill -INT $RECP 2>/dev/null; wait $RECP 2>/dev/null; RECP=
  local now iv0; now=$(VOLS); echo "$(date +%T) after: $now" >> $D/logs/macvol.txt
  iv0=$(echo "$RECV0" | sed -n 's/.*input volume:\([0-9]*\).*/\1/p')
  [ -n "$iv0" ] && [ "$(echo "$now" | sed -n 's/.*input volume:\([0-9]*\).*/\1/p')" != "$iv0" ] && { echo "$(date +%T) NOTE input volume changed from $iv0 during the recording: not set back by the kit (it never sets a volume); put it back by hand" | tee -a $D/logs/macvol.txt; }
  return 0; }
