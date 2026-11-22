# Mac helpers (bash 3.2), sourced by every Mac script of row/ and by ../run.sh. #8237 ROW kit, any lamp on OpenClaw.
# Every lamp-specific value is a setting, read from the runner's env file (KIT_ENV) or the environment:
#   LAMP          the lamp's name (required); used for the lock and the logs only
#   LAMP_HOST     its address (required)
#   LAMP_USER     its ssh user (required); its home on the lamp keeps the row's records (~/t8237, ~/changes.txt)
#   KIT_ENV       the runner's env file (sourced, never copied or printed). It holds LAMP_SUDO_PASS (the lamp's sudo
#                 password; with none, sudo must work without one) and, optionally, LAMP_SSH_PASS (ssh password for
#                 sshpass -e; with none, ssh uses the runner's keys)
#   LOCK_WHO      the lock's holder (required with KIT_LL); LOCK_SESSION its session (default row-8237)
#   KIT_LL        the lamp-lock script (optional). With none, NO lock is taken or renewed (RENEW always passes).
#   PR_SKILLS_BIN PR_TASKS_BIN    #8237's CI ARM64 lamp-skills and lamp-tasks on this Mac (step put)
#   PR_SKILLS_SHA PR_TASKS_SHA    their sha256 (required by put: each file must be exactly it)
#   STABLE_SKILLS_SHA STABLE_TASKS_SHA  optional: stable's installed sha256s; check then requires the lamp to run them
D=${D:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}
KIT=$(cd "$D/.." && pwd)                          # the kit (kits/8237)
KIT_ENV=${KIT_ENV:-}
[ -n "$KIT_ENV" ] && { [ -f "$KIT_ENV" ] || { echo "KIT_ENV=$KIT_ENV: no such file"; exit 2; }; . "$KIT_ENV"; }
for v in LAMP LAMP_HOST LAMP_USER; do eval "[ -n \"\${$v:-}\" ]" || { echo "$v is not set: put it in the env file (KIT_ENV)"; exit 2; }; done
LL=${KIT_LL:-}
[ -n "$LL" ] && [ -z "${LOCK_WHO:-}" ] && { echo "LOCK_WHO is not set (KIT_LL is): put it in the env file"; exit 2; }
LOCK_WHO=${LOCK_WHO:-row}; LOCK_LAMP=$LAMP; LOCK_SESSION=${LOCK_SESSION:-row-8237}
H=$LAMP_USER@$LAMP_HOST
A=${A:-$D/clips}                                  # pre-rendered ElevenLabs clips (render.sh)
PR=8237
SKILL=desk-hello                                  # the pushed fixture (fixtures/desk-hello)
mkdir -p $D/logs
pw(){ if [ -n "${LAMP_SUDO_PASS:-}" ]; then printf '%s\n' "$LAMP_SUDO_PASS"; else echo; fi; }
# ssh/scp: sshpass -e when LAMP_SSH_PASS is set (by the environment, never on a command line).
if [ -z "${SSH:-}" ]; then
  if [ -n "${LAMP_SSH_PASS:-}" ]; then export SSHPASS="$LAMP_SSH_PASS"; SSH="sshpass -e ssh -o StrictHostKeyChecking=accept-new"; SCP="sshpass -e scp -o StrictHostKeyChecking=accept-new"
  else SSH="ssh -o StrictHostKeyChecking=accept-new"; SCP="scp -o StrictHostKeyChecking=accept-new"; fi
fi
SCP=${SCP:-scp}
# SSHR: $SSH, run again (at most 3 tries, 2 s apart) only when the lamp refused the login: exit 255 (ssh) or 5
# (sshpass) with "Permission denied" in its stderr; any other failure is returned at once. Without -n, stdin (a LAMP
# body) is kept in memory, never in a file, and fed again to each try. Not for JF (the journal follow).
SSHR(){ local i=1 rc e a n=0 in=; e=$(mktemp "${TMPDIR:-/tmp}/kit8237-ssh.XXXXXX")
  for a in "$@"; do [ "$a" = -n ] && n=1; done
  [ $n = 1 ] || { in=$(cat; echo x); in=${in%x}; }
  while :; do
    if [ $n = 1 ]; then $SSH "$@" 2>"$e"; else printf '%s' "$in" | $SSH "$@" 2>"$e"; fi; rc=$?
    if [ $i -lt 3 ] && { [ $rc = 255 ] || [ $rc = 5 ]; } && grep -q 'Permission denied' "$e"; then
      echo "ssh: the lamp refused the login (try $i of 3): again in 2 s" >&2; i=$((i + 1)); sleep 2; continue; fi
    cat "$e" >&2; rm -f "$e"; return $rc
  done; }
# Run a lamp body as root: the password (only to sudo's stdin), the lamp settings, the row prelude, an optional
# "VAR=val" line, the files.
LAMP(){ local pre=$1; shift; { pw; printf 'LAMP_USER=%s; LOCK_WHO=%s\n' "$LAMP_USER" "$LOCK_WHO"; cat $D/common.lamp.sh; [ -n "$pre" ] && printf '%s\n' "$pre"; cat "$@"; } |
  SSHR -o ServerAliveInterval=10 -o ServerAliveCountMax=3 -o ConnectTimeout=15 $H 'if sudo -n true 2>/dev/null; then read -r _x; sudo sh -s; else sudo -S -p "" sh -s; fi'; }
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
# Journal follower to a file (lamp epoch timestamps: short-unix).
JF(){ $SSH -n $H "journalctl -f -n0 -o short-unix -u lamp-voice -u lamp-tasks -u lamp-skills -u 'lamp-skills-manage@*' -u lamp-control -u lamp-openclaw-gateway" >> $1 2>&1 & echo $!; }
# Start budget, per unit, from the lamp's own journal ("Started" lines in the last 10 min). A start is refused at 9 or
# more, and at 3 or more for a unit whose StartLimitBurst is 5 in 600 s (deploy/product/lamp-skills.service:20-21,
# lamp-tasks.service:16-17, update/src/openclaw_install.rs:565-566), so a crash-restart still has room.
STARTS(){ SSHR -n $H "journalctl -u $1 --since -10min -o cat --no-pager" 2>/dev/null | grep -c "^Started " ; }
GATE(){ local u c lim; for u in ${@:-lamp-voice}; do case $u in lamp-skills|lamp-tasks|lamp-openclaw-gateway) lim=2;; *) lim=8;; esac
  c=$(STARTS $u); [ "${c:-0}" -le $lim ] || { echo "GATE: $u started $c times in the last 10 min (a start needs <= $lim): wait"; return 1; }; done; }
