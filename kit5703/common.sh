# Kit 5703: shared pieces, sourced by setup.sh, row.sh and putback.sh (plain POSIX sh, dash).
# Never stops, restarts, masks or disables lamp-update or lamp-update-back, never takes the
# updater's lock, never touches a `current` link. The only unit it restarts is lamp-body.
ON=kit5703
B=/root/kit5703                      # the kit's state, logs and scripts (survives a power cycle)
X=/var/lib/kit5703                   # the staged binary: lampos must exec it, /run may be noexec
STAGED=$X/lamp-body
STOCK_PATH=/opt/lampOS/body/current/lamp-body
MEASURE=/opt/lampOS/body/current/lamp-body-measure
INSPECT=/opt/lampOS/body/current/lamp-body-inspect
ED=/etc/systemd/system
DROPIN=$ED/lamp-body.service.d/zz-$ON.conf          # /etc, so a real power cycle keeps it
DEADMAN=$ED/$ON-deadman
DEV=/dev/ttyACM0
LOCKS=/run/lampos-locks/motor
LAMP_ENV=/etc/lampOS/body/lamp.env
# The staged binary's sha256 (the CI artifact's own SHA256SUMS line for lamp-body).
STAGED_SHA=c2f6b9528960accd9ae2edf35c83c0c7b50b307a0ec9d9933e16ac410bd01886
HEAD=5caac04a46496e245a723b1fbddc8859d1f18925

lock_held() { i=$(stat -c %i /var/lib/lampos-update/lock 2>/dev/null) || return 1; grep -q ":$i " /proc/locks; }
upd_busy() {
  [ -e /var/lib/lampos-update/self.json ] || systemctl is-active -q lamp-update-back || lock_held \
    || case "$(systemctl show -p ActiveState --value lamp-update)" in activating|deactivating) true;; *) false;; esac
}
links_now() { for l in /opt/lampOS/*/current; do echo "$l $(readlink $l)"; done; }
body_pid() { systemctl show -p MainPID --value lamp-body; }
body_exe_sha() { p=$(body_pid); [ "${p:-0}" -gt 0 ] && sha256sum /proc/$p/exe 2>/dev/null | cut -d' ' -f1; }
# The unit's monotonic stamps (µs since boot): a wall clock that jumps at boot (NTP) does not move them.
mono() { systemctl show -p "$1" --value lamp-body; }
secs() { awk -v a="$1" -v b="$2" 'BEGIN { if (a > 0 && b >= a) printf "%.1f", (b - a) / 1000000; else print "?" }'; }
up_us() { awk '{ printf "%d", $1 * 1000000 }' /proc/uptime; }

# The lamp's own bus tool, for the voice's mute and state; none: the journal only.
BUSBIN=""
for c in "$(command -v lamp-bus 2>/dev/null || true)" /usr/local/bin/lamp-bus /opt/lampOS/bus/current/lamp-bus; do
  [ -n "$c" ] && [ -x "$c" ] && { BUSBIN=$c; break; }
done
mute_now() { [ -n "$BUSBIN" ] && $BUSBIN --root /run/lampos-bus latest voice/mute 2>&1 | grep -o -E '"muted":[a-z]*(,"muted_by":"[a-z_]*")?' | head -1; }
vstate_now() { [ -n "$BUSBIN" ] && $BUSBIN --root /run/lampos-bus latest voice/state 2>&1 | grep -o '"state":"[a-z_]*"' | head -1; }
# listening_ready said by the voice's current run.
voice_ready() {
  m=$(systemctl show -p ExecMainStartTimestampMonotonic --value lamp-voice)
  case "$m" in ''|0|*[!0-9]*) journalctl -u lamp-voice -b --no-pager 2>/dev/null | grep -c listening_ready; return;; esac
  since=$(( $(date +%s) - $(up_us) / 1000000 + m / 1000000 - 2 ))
  journalctl -u lamp-voice --since "@$since" --no-pager 2>/dev/null | grep -c listening_ready
}

# The body's poses from lamp.env: --home PITCH,ROLL,YAW  --folded-arm SHOULDER,ELBOW  --stand P,R,Y,S,E
pose_arg() { grep -o -E -- "--$1 [0-9,]+" $LAMP_ENV 2>/dev/null | head -1 | cut -d' ' -f2; }
field() { echo "$1" | cut -d, -f"$2"; }

# The servos as they are now, read-only (no register written, no torque, no motion), as the body's
# user: "yaw shoulder elbow roll pitch torque=t1t2t3t4t5". Only while lamp-body is stopped.
read_joints() {
  setpriv --reuid=lampos --regid=lampos --groups=dialout -- $INSPECT --device $DEV \
    --unit-id "$(uname -n)" --lock-directory $LOCKS 2>/dev/null \
  | sed 's/{"joint_id":/\n@/g' | awk '
    /^@/ { id = substr($0, 2) + 0
      if (match($0, /"position_raw":[0-9]+/)) pos[id] = substr($0, RSTART + 15, RLENGTH - 15)
      if (match($0, /"torque_enable_raw":[0-9]+/)) tq[id] = substr($0, RSTART + 20, RLENGTH - 20) }
    END { if (pos[1] == "") { print "unread"; exit 1 }
      printf "%s %s %s %s %s torque=%s%s%s%s%s\n", pos[1], pos[2], pos[3], pos[4], pos[5], tq[1], tq[2], tq[3], tq[4], tq[5] }'
}
# The product's park before the power goes (lamp-setup shutdown-now's own call): torque off on all
# five, only for a lamp that is down. One JSON line; exit 0 only when every joint reads back let go.
let_go() {
  setpriv --reuid=lampos --regid=lampos --groups=dialout -- $MEASURE let-go --device $DEV \
    --unit-id "$(uname -n)" --lock-directory $LOCKS --lamp-env $LAMP_ENV
}
# lamp-body's journal since a `journalctl --since` time: the lines this row is read on.
body_lines() {
  journalctl -u lamp-body --since "$1" --no-pager -o short-monotonic 2>/dev/null \
  | grep -E 'found shoulder|folds first|waits for a hand|will not stand|waiting to be set straight|set straight: taken hold|met resistance|let go under overload|after a strain|sits the lamp down|seated|unplug step|lamp-body failed|Watchdog|timed out|Failed with result|Main process exited|Started |Stopping |Stopped |repeated too quickly|Scheduled restart'
}
# Of those, the ones that fail a row: a load, strain or overload stop, a failed or timed-out start.
stop_lines() {
  body_lines "$1" | grep -E 'met resistance|let go under overload|after a strain|sits the lamp down|unplug step|lamp-body failed|Watchdog|timed out|Failed with result|Main process exited|repeated too quickly|Scheduled restart'
}
# The newest exit report in lamp-body's journal since $1, as plain lines (the staged binary reads it).
exit_report_since() {
  journalctl -u lamp-body --since "$1" --no-pager -o short-iso 2>/dev/null | $STAGED exit-report 2>&1
}
loud() { echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"; echo "!!! $*"; echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"; }
