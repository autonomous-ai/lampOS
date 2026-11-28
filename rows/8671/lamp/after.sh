#!/bin/sh
# The recorder (#8671 row kit): run by trigger.sh as the transient unit lamp8671-after, never by hand. Outside
# lampos.target and the reset's WRITERS/STARTERS, so the reset's stop leaves it; a reboot (the reset's fallback)
# ends it. It writes only to /root/el-8671/after (/root is on the reset's PROTECTED list: never wiped):
#   1. a journal follower of the reset's units (its PID kept; stopped by this script), so the reset's lines before
#      its `journalctl --vacuum-time=1s` are kept: reset-journal.txt;
#   2. lamp-setup-wipe watched (1 s): its exe (the PR's?), start, end, result; at most WIPE_MOST_S (600 s);
#   3. the PR's tmpfs unmounted once the wipe has ended (nothing of the kit left in /run);
#   4. snap.sh at +0, +2 min, +15 min, +1 h, then every 30 min, and 2 min after lamp-voice first runs again (the
#      lead's re-pairing), then it ends; at most AFTER_WAIT_S (6 h) in all. status.txt says each step.
# Never touches lamp-update, Wi-Fi or any unit; reads counts only.
. "$(dirname "$0")/common.sh"
mkdir -p $AFTER; chmod 700 $AFTER
S=$AFTER/status.txt
rec() { echo "$1 $(wall_us)" >> $S; }

journalctl -f -n 0 -o short-iso-precise --no-pager -u $WIPE_UNIT -u lamp-setup -u lamp-setup-reset -u lampos-setup-done.path \
  > $AFTER/reset-journal.txt 2>/dev/null &
FPID=$!; echo $FPID > $AFTER/follower.pid
stop_follower() { [ -d /proc/$FPID ] && tr '\0' ' ' < /proc/$FPID/cmdline | grep -q '^journalctl -f' && kill $FPID; rm -f $AFTER/follower.pid; }
trap 'stop_follower' EXIT
sleep 1; rec "RECORDER_START boot=$(boot_id)"; touch $AFTER/ready
# -- the wipe
seen=0; i=0; WIPE_MOST_S=${WIPE_MOST_S:-600}
while [ $i -lt $WIPE_MOST_S ]; do
  st=$(systemctl show -p ActiveState --value $WIPE_UNIT 2>/dev/null); ld=$(systemctl show -p LoadState --value $WIPE_UNIT 2>/dev/null)
  if [ "$st" = active ] || [ "$st" = activating ]; then
    if [ $seen = 0 ]; then seen=1; p=$(systemctl show -p MainPID --value $WIPE_UNIT)
      rec "WIPE_SEEN exe=$(readlink /proc/$p/exe 2>/dev/null) argv=$(tr '\0' ' ' < /proc/$p/cmdline 2>/dev/null | tr ' ' '|')"; fi
    r=$(systemctl show -p Result --value $WIPE_UNIT 2>/dev/null)
  elif [ $seen = 1 ]; then
    rec "WIPE_END state=$st load=$ld result=${r:-?}/$(systemctl show -p Result --value $WIPE_UNIT 2>/dev/null)"; break
  elif [ $i -ge 30 ]; then rec "WIPE_NEVER state=$st load=$ld"; break; fi
  i=$((i+1)); sleep 1
done
[ $i -ge $WIPE_MOST_S ] && rec "WIPE_TIMEOUT ${WIPE_MOST_S}s"
sleep 5; stop_follower; rec "FOLLOWER_STOPPED lines=$(wc -l < $AFTER/reset-journal.txt | tr -d ' ')"
# -- the PR's tmpfs: nothing of the kit left in /run
if grep -q " $BIN_DIR " /proc/mounts; then umount $BIN_DIR && rec "TMPFS_UNMOUNTED" || rec "TMPFS_BUSY"; fi
rmdir $BIN_DIR 2>/dev/null
# -- the snapshots
n=0; next=0; t0=$(date +%s); vact=0
snap() { sh "$KIT/snap.sh" "$1" > $AFTER/snap-$1.txt 2>&1; rec "SNAP $1"; }
while :; do
  el=$(( $(date +%s) - t0 ))
  if [ $el -ge $next ]; then snap $n; n=$((n+1))
    case $n in 1) next=120;; 2) next=900;; 3) next=3600;; *) next=$((next + 1800));; esac; fi
  if [ $vact = 0 ] && systemctl is-active --quiet lamp-voice; then vact=$(date +%s); rec "VOICE_ACTIVE"; fi
  if [ $vact != 0 ] && [ $(( $(date +%s) - vact )) -ge 120 ]; then snap repaired; rec "DONE repaired"; exit 0; fi
  [ $el -ge $AFTER_WAIT_S ] && { snap last; rec "DONE wait-over"; exit 0; }
  sleep 10
done
