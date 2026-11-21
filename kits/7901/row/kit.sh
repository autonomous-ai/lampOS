# Mac helpers (bash 3.2), sourced by every Mac script of row/, by ../run.sh and by ../optional/. #7901 ROW kit, any lamp.
# Every lamp-specific value is a setting, read from the runner's env file (KIT_ENV) or the environment:
#   LAMP          the lamp's name (required); used for the lock and the logs only
#   LAMP_HOST     its address (required)
#   LAMP_USER     its ssh user (required); its home on the lamp keeps the row's records (~/t7901, ~/changes.txt)
#   KIT_ENV       the runner's env file (sourced, never copied or printed). It holds LAMP_SUDO_PASS (the lamp's sudo
#                 password; with none, sudo must work without one) and, optionally, LAMP_SSH_PASS (ssh password for
#                 sshpass -e; with none, ssh uses the runner's keys)
#   LOCK_WHO      the lock's holder (required with KIT_LL); LOCK_SESSION its session (default row-7901)
#   KIT_LL        the lamp-lock script (optional). With none, NO lock is taken or renewed (RENEW always passes).
#   PR_{VOICE,SKILLS,CONTROL,TASKS}_SHA  optional: the sha256 of #7901's CI ARM64 artifact binaries. check.lamp.sh
#                 then requires each running part to be exactly that build; with none set it prints what runs.
D=${D:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}
KIT=$(cd "$D/.." && pwd)                          # the kit (kits/7901)
KIT_ENV=${KIT_ENV:-}
[ -n "$KIT_ENV" ] && { [ -f "$KIT_ENV" ] || { echo "KIT_ENV=$KIT_ENV: no such file"; exit 2; }; . "$KIT_ENV"; }
for v in LAMP LAMP_HOST LAMP_USER; do eval "[ -n \"\${$v:-}\" ]" || { echo "$v is not set: put it in the env file (KIT_ENV)"; exit 2; }; done
LL=${KIT_LL:-}
[ -n "$LL" ] && [ -z "${LOCK_WHO:-}" ] && { echo "LOCK_WHO is not set (KIT_LL is): put it in the env file"; exit 2; }
LOCK_WHO=${LOCK_WHO:-row}; LOCK_LAMP=$LAMP; LOCK_SESSION=${LOCK_SESSION:-row-7901}
H=$LAMP_USER@$LAMP_HOST
A=${A:-$D/clips}                                  # pre-rendered ElevenLabs clips (render.sh)
PR=7901
mkdir -p $D/logs
pw(){ if [ -n "${LAMP_SUDO_PASS:-}" ]; then printf '%s\n' "$LAMP_SUDO_PASS"; else echo; fi; }
# ssh/scp: sshpass -e when LAMP_SSH_PASS is set (by the environment, never on a command line).
if [ -z "${SSH:-}" ]; then
  if [ -n "${LAMP_SSH_PASS:-}" ]; then export SSHPASS="$LAMP_SSH_PASS"; SSH="sshpass -e ssh -o StrictHostKeyChecking=accept-new"; SCP="sshpass -e scp -o StrictHostKeyChecking=accept-new"
  else SSH="ssh -o StrictHostKeyChecking=accept-new"; SCP="scp -o StrictHostKeyChecking=accept-new"; fi
fi
SCP=${SCP:-scp}
# Run a lamp body as root: the password (only to sudo's stdin), the lamp settings, the row prelude, an optional
# "VAR=val" line, the files.
LAMP(){ local pre=$1; shift; { pw; printf 'LAMP_USER=%s; LOCK_WHO=%s\n' "$LAMP_USER" "$LOCK_WHO"; cat $D/common.lamp.sh; [ -n "$pre" ] && printf '%s\n' "$pre"; cat "$@"; } |
  $SSH -o ServerAliveInterval=10 -o ServerAliveCountMax=3 -o ConnectTimeout=15 $H 'if sudo -n true 2>/dev/null; then read -r _x; sudo sh -s; else sudo -S -p "" sh -s; fi'; }
# Our lock only, when there is one (KIT_LL): a failed renew stops a script before any ssh.
RENEW(){ [ -n "$LL" ] || return 0
  $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 || { echo "LOCK: not ours ($LOCK_LAMP $LOCK_WHO $LOCK_SESSION): stop, no ssh"; return 1; }; }
# Mac volume, READ ONLY (<= 70; never set). The lamp's speaker volume is never touched by any script here.
MACVOL(){ ${OSASCRIPT:-osascript} -e 'output volume of (get volume settings)'; }
VOLOK(){ local v; v=$(MACVOL); [ "$v" -le 70 ] 2>/dev/null || { echo "Mac volume $v > 70 (or unread): stop and ask (never set it)"; return 2; }; }
GAIN(){ awk -F'\t' -v c=$1 '$1 == c {print ($3 == "" ? "1.0" : $3)}' $A/clips.txt | head -1; }
# PLAY id: clips/<id>-el.wav from the Mac speaker, gain by afplay -v only (clips.txt 3rd column).
PLAY(){ local id=$1 f=$A/$1-el.wav g; VOLOK || return 2; [ -f "$f" ] || { echo "no clip $f (../run.sh render)"; return 3; }
  g=$(GAIN $id); g=${g:-1.0}
  echo "$(date '+%T') PLAY $id gain=$g leg=${LEG:-?} (Mac vol $(MACVOL))" | tee -a $D/logs/plays.log
  ${AFPLAY:-afplay} -v "$g" "$f"; echo "$(date '+%T') PLAYED $id" >> $D/logs/plays.log; }
# Journal follower to a file.
JF(){ $SSH -n $H "journalctl -f -n0 -o short-unix -u lamp-voice -u lamp-tasks -u lamp-skills -u 'lamp-skills-manage@*' -u lamp-control -u lamp-openclaw-gateway" >> $1 2>&1 & echo $!; }
# Start budget: at most 10 starts in any 10 min per unit (the lamp's own journal counts). A start is allowed at <= 8.
STARTS(){ $SSH -n $H "journalctl -u $1 --since -10min -o cat --no-pager" 2>/dev/null | grep -c "^Started " ; }
GATE(){ local u c; for u in ${@:-lamp-voice}; do c=$(STARTS $u); [ "${c:-0}" -le 8 ] || { echo "GATE: $u started $c times in the last 10 min: wait"; return 1; }; done; }
