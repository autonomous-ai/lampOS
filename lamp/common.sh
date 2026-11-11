# Kit 5194: shared pieces, sourced by setup.sh, switch.sh, q.sh and putback.sh (plain POSIX sh, dash).
# The kit lives in /run/kit5194 (noexec: every script runs with `sh`). It never stops, restarts,
# masks or disables lamp-update or lamp-update-back, never takes the updater's lock, never touches
# a `current` link. The only unit it restarts is lamp-voice: stopped on its own first (a clean
# SIGTERM releases the restart guard), then started.
ON=kit5194
R=/run/kit5194
S=$R/state
L=$R/logs
SW=LAMP_OPENS_FURTHER
DDIR=/run/systemd/system/lamp-voice.service.d
DROPIN=$DDIR/zz-$ON.conf
EDIR=/etc/systemd/system/lamp-voice.service.d
# #5235, the switch's merge, in train 678.
MERGE=29cc36da91

lock_held() { i=$(stat -c %i /var/lib/lampos-update/lock 2>/dev/null) || return 1; grep -q ":$i " /proc/locks; }
upd_busy() {
  [ -e /var/lib/lampos-update/self.json ] || systemctl is-active -q lamp-update-back || lock_held \
    || case "$(systemctl show -p ActiveState --value lamp-update)" in activating|deactivating) true;; *) false;; esac
}
wait_upd() {   # $1: what for. 0 = quiet; 1 = still busy after 20 min
  i=0; while upd_busy; do i=$((i+1)); [ $i -gt 120 ] && return 1; echo "waiting for lamp-update's round before $1 ($i)"; sleep 10; done; return 0
}
links_now() { for l in /opt/lampOS/*/current; do echo "$l $(readlink $l)"; done; }
voice_pid() { systemctl show -p MainPID --value lamp-voice; }
# env compared SAME/DIFF later: systemd's per-start values masked
envof() { tr '\0' '\n' < /proc/$1/environ | grep -v -E '^(INVOCATION_ID|JOURNAL_STREAM|SYSTEMD_EXEC_PID|MAINPID|MANAGERPID|WATCHDOG_PID|WATCHDOG_USEC|NOTIFY_SOCKET)=' | sort | tr '\n' ' '; }
mask() { sed -E 's|sessions/[0-9a-f]{32}|sessions/<id>|g'; }
# Every file in the voice's drop-in folders, with its sha256: put back byte-equal.
dropin_sums() {
  for d in $DDIR $EDIR; do
    if [ -d $d ]; then echo "dir $d"; find $d -type f -exec sha256sum {} + 2>/dev/null | sort -k2; else echo "nodir $d"; fi
  done
}
loud() { echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"; echo "!!! $*"; echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"; }

# The lamp's own bus tool (night 7's order); none: the kit cannot read replies, setup stops.
BUSBIN=""
for c in "$(cat $S/busbin 2>/dev/null)" "$(command -v lamp-bus 2>/dev/null || true)" /usr/local/bin/lamp-bus /opt/lampOS/bus/current/lamp-bus; do
  [ -n "$c" ] && [ -x "$c" ] && { BUSBIN=$c; break; }
done
BUS="$BUSBIN --root /run/lampos-bus"
mute_now() { $BUS latest voice/mute 2>&1 | grep -o -E '"muted":[a-z]*(,"muted_by":"[a-z_]*")?' | head -1; }
vstate_now() { $BUS latest voice/state 2>&1 | grep -o '"state":"[a-z_]*"' | head -1; }
asleep_now() { $BUS latest expression/state 2>&1 | grep -o '"asleep":[a-z]*' | head -1; }

# listening_ready said since $1 (a journalctl --since time), waiting up to 90 s. 0 = said.
ready_since() { i=0; until journalctl -u lamp-voice --since "$1" --no-pager | grep -q listening_ready; do
  i=$((i+1)); [ $i -gt 30 ] && return 1; sleep 3; done; return 0; }
# listening_ready said by the voice's current run (kit 5703's count).
ready_current() {
  m=$(systemctl show -p ExecMainStartTimestampMonotonic --value lamp-voice)
  case "$m" in ''|0|*[!0-9]*) journalctl -u lamp-voice -b --no-pager 2>/dev/null | grep -c listening_ready; return;; esac
  up=$(awk '{ printf "%d", $1 }' /proc/uptime)
  since=$(( $(date +%s) - up + m / 1000000 - 2 ))
  journalctl -u lamp-voice --since "@$since" --no-pager 2>/dev/null | grep -c listening_ready
}
guard_muted() { journalctl -u lamp-voice --since "$1" --no-pager | grep -q 'muted by the restart guard' \
  || $BUS latest voice/mute 2>&1 | grep -q '"muted_by":"restart_guard"'; }
# The voice restarted the night-7 way: stopped alone (Result=success checked), then started.
# 0 = stopped cleanly and started; 1 = the stop was not clean (it is started anyway).
voice_restart() {
  systemctl stop lamp-voice
  vr=$(systemctl show -p Result --value lamp-voice)
  echo "lamp-voice stop: Result=$vr"
  sleep 2
  systemctl start lamp-voice
  sleep 8
  [ "$vr" = success ]
}
