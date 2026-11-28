#!/bin/sh
# Run as root on lamp-75c9 (the recorder runs it; ../run.sh after runs it as "live"): sh snap.sh NAME.
# One after-reset snapshot, KEY=VALUE lines, counts and kinds only (no log line, no file content is printed):
#   the seeded markers (by path), each home's kind, the voice's paths, the KEPT paths, the log files' inodes;
#   setup's state (marker, journal, units); the reset's own lines counted in the live journal since the reset;
#   a kit probe line (`logger -t el8671-kit`) counted in the journal and in /var/log/syslog: the logs still write;
#   lamp-voice's journal lines since the reset; the lamp-voice lines OLDER than the reset left in /var/log,
#   /var/log.hdd and /var/log.hdd/journal (logcount.py), and the newest mtime in /var/log.hdd (a ramlog sync?).
. "$(dirname "$0")/common.sh"
host_ok; root_ok
N=${1:-live}
T=$(sed -n 's/^RESET_WALL_US=//p' $K/reset-start.txt 2>/dev/null); [ -n "$T" ] || { echo "REFUSE: no $K/reset-start.txt"; exit 1; }
TS=$(( T / 1000000 ))
echo "SNAP=$N SNAP_WALL_US=$(wall_us) SINCE_RESET_S=$(( $(date +%s) - TS )) BOOT_ID=$(boot_id) BOOT_BEFORE=$(sed -n 's/^BOOT_ID=//p' $K/reset-start.txt)"
while IFS="$(printf '\t')" read -r f kind made; do
  [ -e "$f" ] && echo "MARK $kind present $f" || echo "MARK $kind absent $f"
done < $K/seeded.tsv
echo "HOMES=$(kind_of $HOMES)"; for h in $(awk -F'\t' '$2=="home"{print $1}' $K/seeded.tsv); do h=${h%/*}; echo "HOME $h $(kind_of $h)"; done
for p in $KEPT_PATHS $MARKS $HFORGET $MARKED $CLIPS_NEW $CLIPS_OLD; do echo "KIND $p $(kind_of $p)"; done
echo "CLIPS_OLD_OTHER=$(find $CLIPS_OLD -mindepth 1 -maxdepth 1 ! -name "$MNAME" 2>/dev/null | wc -l | tr -d ' ')"
log_inodes | sed 's/^/LOGFILE /'
echo "SETUP_MARKER=$( [ -e $MARKER ] && echo present || echo absent) RESET_JOURNAL=$( [ -e $RJOURNAL ] && echo present || echo absent)"
for u in lampos.target lamp-setup lamp-setup-reset lamp-voice rsyslog systemd-journald $RECORDER; do printf 'UNIT %s=%s\n' $u "$(systemctl is-active $u 2>/dev/null)"; done
echo "TMPFS=$(grep -q " $BIN_DIR " /proc/mounts && echo mounted || echo gone)"
j=$(journalctl -q --no-pager -o cat -u $WIPE_UNIT --since "@$TS" 2>/dev/null)
echo "LIVE_HOMES_LINE=$(printf '%s\n' "$j" | grep -cE "$HOMES_LINE") LIVE_LOGS_LINE=$(printf '%s\n' "$j" | grep -cE "$LOGS_LINE") LIVE_STARTED_LINE=$(printf '%s\n' "$j" | grep -cF "$STARTED_LINE") LIVE_LOG_FAIL=$(printf '%s\n' "$j" | grep -cF "$LOG_FAIL") LIVE_WIPE_LINES=$(printf '%s\n' "$j" | grep -c .)"
# -- the logs still write: one kit probe line, counted where it should land
tag="kit row 8671 probe $N $(date +%s)"
logger -t el8671-kit "$tag"; sleep 3
echo "PROBE_JOURNAL=$(journalctl -q --no-pager -o cat -t el8671-kit --since "@$TS" 2>/dev/null | grep -cxF "$tag") PROBE_SYSLOG=$(grep -cF "$tag" $LOGS_RAM/syslog 2>/dev/null)"
# lamp-voice lines since the wipe ENDED (the voice runs until the reset stops it): its lines again, after a re-pair
we=$(awk '$1=="WIPE_END"{print $NF}' $AFTER/status.txt 2>/dev/null | tail -1); WE=$(( ${we:-$T} / 1000000 ))
echo "VOICE_SINCE_END=$(journalctl -q --no-pager -o short-unix -u lamp-voice --since "@$WE" 2>/dev/null | grep -cE '^[0-9]') WIPE_END_S=$WE"
logcount $TS --live
echo "HDD_NEWEST_MTIME=$(find $LOGS_HDD -type f -printf '%T@\n' 2>/dev/null | sort -n | tail -1 | cut -d. -f1) RESET_S=$TS"
echo "WLAN0=$(ip -br addr show wlan0 2>/dev/null | awk '{print $2}') WPA_CONF=$(kind_of /etc/wpa_supplicant/wpa_supplicant-wlan0.conf | cut -d' ' -f1)"
echo SNAP-DONE
