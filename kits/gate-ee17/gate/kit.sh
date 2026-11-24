# Mac helpers (bash 3.2), sourced by ../run.sh and gate/core.sh. The promote gate on lamp-ee17 (#7814, the first
# builder's 12:02 and 12:08 lists). Ported from kits/7901/row/kit.sh: the lamp exec (LAMP), the ssh retry (SSHR), the
# lock (RENEW), the start budget (GATE), the Mac volume read (VOLOK) and the clip play (PLAY).
# Every lamp-specific value is a setting, read from the runner's env file (KIT_ENV) or the environment:
#   LAMP          the lamp's name (default lamp-ee17); for the lock and the logs only
#   LAMP_HOST     its address (required)
#   LAMP_USER     its ssh user (default orangepi); its home keeps the gate's records (~/tgate, ~/changes.txt)
#   KIT_ENV       the runner's env file (sourced, never copied or printed): LAMP_SUDO_PASS, optional LAMP_SSH_PASS
#   LOCK_WHO      the lock's holder (required with KIT_LL); LOCK_SESSION its session (default gate-ee17)
#   KIT_LL        the lamp-lock script (optional). With none, NO lock is taken or renewed (RENEW always passes).
# The gate's numbers (each a setting, with today's defaults):
#   STABLE_SEQ=15759  STABLE_UPD=b213d9041a88   stable, and its lamp-update's sha256 prefix (round 1's updater)
#   CAND_SEQ=15860    CAND_UPD=e00e49d4785d     the candidate, and its lamp-update's sha256 prefix (round 2's updater)
#   NEXT_SEQ=15866    round 2's build (train 1202's, efed4a4d39; First 12:29). Set empty, round2 refuses to run
#   NEXT_UPD=e00e49d4785d  prefix of NEXT's lamp-update sha256: 15866 carries 15860's same updater (empty: not checked)
#   CHANNEL_BASE      the releases' folder (https://github.com/autonomous-ai/lampOS/releases/download/)
#   MANIFEST_BASE     where the Mac reads each release's lampos-manifest.json (default CHANNEL_BASE; mock: file://)
D=${D:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}
KIT=$(cd "$D/.." && pwd)
KIT_ENV=${KIT_ENV:-}
[ -n "$KIT_ENV" ] && { [ -f "$KIT_ENV" ] || { echo "KIT_ENV=$KIT_ENV: no such file"; exit 2; }; . "$KIT_ENV"; }
LAMP=${LAMP:-lamp-ee17}; LAMP_USER=${LAMP_USER:-orangepi}
[ -n "${LAMP_HOST:-}" ] || { echo "LAMP_HOST is not set: put it in the env file (KIT_ENV)"; exit 2; }
LL=${KIT_LL:-}
[ -n "$LL" ] && [ -z "${LOCK_WHO:-}" ] && { echo "LOCK_WHO is not set (KIT_LL is): put it in the env file"; exit 2; }
LOCK_WHO=${LOCK_WHO:-gate}; LOCK_LAMP=$LAMP; LOCK_SESSION=${LOCK_SESSION:-gate-ee17}
H=$LAMP_USER@$LAMP_HOST
A=${A:-$D/clips}
STABLE_SEQ=${STABLE_SEQ:-15759}; STABLE_UPD=${STABLE_UPD:-b213d9041a88}
CAND_SEQ=${CAND_SEQ:-15860}; CAND_UPD=${CAND_UPD:-e00e49d4785d}
NEXT_SEQ=${NEXT_SEQ-15866}; NEXT_UPD=${NEXT_UPD-e00e49d4785d}
CHANNEL_BASE=${CHANNEL_BASE:-https://github.com/autonomous-ai/lampOS/releases/download/}
MANIFEST_BASE=${MANIFEST_BASE:-$CHANNEL_BASE}
VOLCAP=${VOLCAP:-28}                                  # ee17's speaker cap: the kit refuses above it, never sets it
GWWIN=${GWWIN:-96}                                    # an exit 1 that listens again within this many s is a FINDING
PEEKSECS=${PEEKSECS:-60}
# the units a round restarts: each must have room in its start budget before a round
ROUND_UNITS="lamp-voice lamp-skills lamp-control lamp-tasks lamp-openclaw-gateway lamp-update"
curl_of(){ echo "${CHANNEL_BASE%/}/lampos-lamp-voice-$1/"; }
mkdir -p $D/logs
pw(){ if [ -n "${LAMP_SUDO_PASS:-}" ]; then printf '%s\n' "$LAMP_SUDO_PASS"; else echo; fi; }
# ssh/scp: sshpass -e when LAMP_SSH_PASS is set (by the environment, never on a command line).
if [ -z "${SSH:-}" ]; then
  if [ -n "${LAMP_SSH_PASS:-}" ]; then export SSHPASS="$LAMP_SSH_PASS"; SSH="sshpass -e ssh -o StrictHostKeyChecking=accept-new"; SCP="sshpass -e scp -o StrictHostKeyChecking=accept-new"
  else SSH="ssh -o StrictHostKeyChecking=accept-new"; SCP="scp -o StrictHostKeyChecking=accept-new"; fi
fi
SCP=${SCP:-scp}
# SSHR: $SSH, run again (at most 3 tries, 2 s apart) only when the lamp refused the login (exit 255 or 5 with
# "Permission denied"); any other failure is returned at once. Without -n, stdin (a LAMP body) is kept in memory.
SSHR(){ local i=1 rc e a n=0 in=; e=$(mktemp "${TMPDIR:-/tmp}/kitgate-ssh.XXXXXX")
  for a in "$@"; do [ "$a" = -n ] && n=1; done
  [ $n = 1 ] || { in=$(cat; echo x); in=${in%x}; }
  while :; do
    if [ $n = 1 ]; then $SSH "$@" 2>"$e"; else printf '%s' "$in" | $SSH "$@" 2>"$e"; fi; rc=$?
    if [ $i -lt 3 ] && { [ $rc = 255 ] || [ $rc = 5 ]; } && grep -q 'Permission denied' "$e"; then
      echo "ssh: the lamp refused the login (try $i of 3): again in 2 s" >&2; i=$((i + 1)); sleep 2; continue; fi
    cat "$e" >&2; rm -f "$e"; return $rc
  done; }
# Run a lamp body as root: the password (only to sudo's stdin), the lamp settings, the gate prelude, an optional
# "VAR=val ..." line, the files.
LAMP(){ local pre=$1; shift; { pw; printf 'LAMP_USER=%s; LOCK_WHO=%s\n' "$LAMP_USER" "$LOCK_WHO"; cat $D/common.lamp.sh; [ -n "$pre" ] && printf '%s\n' "$pre"; cat "$@"; } |
  SSHR -o ServerAliveInterval=10 -o ServerAliveCountMax=3 -o ConnectTimeout=15 $H 'if sudo -n true 2>/dev/null; then read -r _x; sudo sh -s; else sudo -S -p "" sh -s; fi'; }
RENEW(){ [ -n "$LL" ] || return 0
  $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 || { echo "LOCK: not ours ($LOCK_LAMP $LOCK_WHO $LOCK_SESSION): stop, no ssh"; return 1; }; }
# Mac volume, READ ONLY (<= 70; never set).
MACVOL(){ ${OSASCRIPT:-osascript} -e 'output volume of (get volume settings)'; }
VOLOK(){ local v; v=$(MACVOL); [ "$v" -le 70 ] 2>/dev/null || { echo "Mac volume $v > 70 (or unread): stop and ask (never set it)"; return 2; }; }
GAIN(){ awk -F'\t' -v c=$1 '$1 == c {print ($3 == "" ? "1.0" : $3)}' $A/clips.txt | head -1; }
PLAY(){ local id=$1 f=$A/$1-el.wav g; VOLOK || return 2; [ -f "$f" ] || { echo "no clip $f (../run.sh render)"; return 3; }
  g=$(GAIN $id); g=${g:-1.0}
  echo "$(date '+%T') PLAY $id gain=$g leg=${LEG:-?} (Mac vol $(MACVOL))" | tee -a $D/logs/plays.log
  ${AFPLAY:-afplay} -v "$g" "$f"; echo "$(date '+%T') PLAYED $id" >> $D/logs/plays.log; }
JF(){ $SSH -n $H "journalctl -f -n0 -o short-unix -u lamp-voice -u lamp-tasks -u lamp-skills -u 'lamp-skills-manage@*' -u lamp-control -u lamp-openclaw-gateway" >> $1 2>&1 & echo $!; }
# Start budget: at most 10 starts in any 10 min per unit (the lamp's own journal counts). A start is allowed at <= 8.
STARTS(){ SSHR -n $H "journalctl -u $1 --since -10min -o cat --no-pager" 2>/dev/null | grep -c "^Started " ; }
GATE(){ local u c; for u in ${@:-lamp-voice}; do c=$(STARTS $u); [ "${c:-0}" -le 8 ] || { echo "GATE: $u started $c times in the last 10 min: wait"; return 1; }; done; }
# The lamp's speaker volume and mute, READ ONLY (gate/vol.lamp.sh). VOLCHK <tag>: STOP (return 2) when the kept level
# or the bus value is above VOLCAP, or both are unread (VOL_UNREAD_OK=1 goes on). With MUTEOK=0 a muted voice stops too.
VOLCHK(){ local o k b m; o=$(LAMP "" $D/vol.lamp.sh 2>&1); echo "$o" | sed "s/^/  $1: /"
  k=$(echo "$o" | sed -n 's/^VOL kept=\([^ ]*\).*/\1/p'); b=$(echo "$o" | sed -n 's/^VOL .* bus=\([^ ]*\).*/\1/p'); m=$(echo "$o" | sed -n 's/^MUTE live muted=\([^ ]*\).*/\1/p')
  for v in $k $b; do case $v in none|'') ;; *) [ "${v%.*}" -le $VOLCAP ] 2>/dev/null || { echo "VOLUME $v > $VOLCAP on $LAMP (the cap): refused, the kit never sets a volume"; return 2; };; esac; done
  [ "${k:-none}" = none ] && [ "${b:-none}" = none ] && [ "${VOL_UNREAD_OK:-0}" != 1 ] && { echo "VOLUME unread (kept and bus): refused (VOL_UNREAD_OK=1 goes on)"; return 2; }
  [ "${MUTEOK:-1}" = 0 ] && [ "$m" = True ] && { echo "the voice is muted: the kit does not lift a mute: lift it in the app, then run the step again"; return 2; }
  return 0; }
