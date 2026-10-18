#!/bin/sh
# eyes-cost.sh: what lamp-eyes costs the lamp, read-only, for #720's mesh switches
# (--mouth-on-heard, --mouth-runtime): run it once with them off and once with
# them on, in the same empty room, and compare the two summaries (#705).
#
# Every INTERVAL seconds for SECONDS seconds it prints one line:
#   time, lamp-eyes' CPU (% of one core, from /proc/<pid>/stat), the core it last
#   ran on, the hottest thermal zone (C), and the mesh's runs since the last line
#   (the journal's "lamp-eyes: mouth open" lines).
# At the end, one summary line. It writes nothing on the lamp and changes nothing.
#
# Usage: eyes-cost.sh [SECONDS [INTERVAL]]   (default 1200 and 10)
# Reading lamp-eyes' journal needs root or the systemd-journal group; without it
# the mesh's runs read "-".
set -u
SECONDS_ALL=${1:-1200}
INTERVAL=${2:-10}
case "$SECONDS_ALL$INTERVAL" in *[!0-9]*) echo "usage: $0 [SECONDS [INTERVAL]]" >&2; exit 2 ;; esac
[ "$INTERVAL" -gt 0 ] || { echo "INTERVAL must be above 0" >&2; exit 2; }

TICKS=$(getconf CLK_TCK)
pid_now() { systemctl show -p MainPID --value lamp-eyes; }
# utime + stime, in ticks, and the core it last ran on (fields 14, 15 and 39 of
# /proc/<pid>/stat, counted after the command name, which may hold spaces).
cpu_of() { sed 's/^.*) //' "/proc/$1/stat" 2>/dev/null | awk '{ print $12 + $13, $37 }'; }
hottest() {
	best=0
	for zone in /sys/class/thermal/thermal_zone*/temp; do
		read -r t <"$zone" 2>/dev/null || continue
		[ "$t" -gt "$best" ] && best=$t
	done
	echo $((best / 1000))
}
# The mesh's runs after the journal cursor in CURSOR, which moves on to the newest
# entry read, so no line is counted twice at an interval's edge. Sets R; it
# runs in this shell, not a subshell, so the cursor moves.
mesh_runs() {
	out=$(journalctl -u lamp-eyes --after-cursor "$CURSOR" --show-cursor -o cat -q 2>/dev/null)
	next=$(printf '%s\n' "$out" | sed -n 's/^-- cursor: //p' | tail -n 1)
	[ -n "$next" ] && CURSOR=$next
	R=$(printf '%s\n' "$out" | grep -c '^lamp-eyes: mouth open' || true)
}
can_read_journal() { journalctl -u lamp-eyes -n 1 -q >/dev/null 2>&1 && [ -n "$(journalctl -u lamp-eyes -n 1 -o cat -q 2>/dev/null)" ]; }

PID=$(pid_now)
[ -n "$PID" ] && [ "$PID" != 0 ] || { echo "lamp-eyes is not running" >&2; exit 1; }
can_read_journal && JOURNAL=1 || JOURNAL=0
# Count from the newest entry now.
CURSOR=$(journalctl -u lamp-eyes -n 1 --show-cursor -o cat -q 2>/dev/null | sed -n 's/^-- cursor: //p' | tail -n 1)
[ -n "$CURSOR" ] || JOURNAL=0
START=$(date +%s)
echo "lamp-eyes pid $PID, $(tr '\0' ' ' </proc/$PID/cmdline | grep -o -- '--mouth[^ ]* [^ ]*' | tr '\n' ' ')"
echo "time cpu% core hottest_C mesh_runs"

set -- $(cpu_of "$PID")
LAST_TICKS=$1
LAST=$(date +%s)
END=$((LAST + SECONDS_ALL))
SUM=0 N=0 HOT=0 RUNS=0 RESTARTS=0
while [ "$(date +%s)" -lt "$END" ]; do
	sleep "$INTERVAL"
	NOW=$(date +%s)
	CUR=$(pid_now)
	if [ "$CUR" != "$PID" ]; then
		# A restart: the new process's counts start again; say so and go on.
		RESTARTS=$((RESTARTS + 1))
		echo "$(date +%T) restarted: pid $PID -> $CUR"
		PID=$CUR
		set -- $(cpu_of "$PID")
		LAST_TICKS=${1:-0}
		LAST=$NOW
		continue
	fi
	set -- $(cpu_of "$PID")
	[ -n "${1:-}" ] || { echo "$(date +%T) lamp-eyes is gone"; break; }
	PCT=$(( ( ($1 - LAST_TICKS) * 100 * 10 / TICKS ) / (NOW - LAST) ))
	TEMP=$(hottest)
	if [ "$JOURNAL" = 1 ]; then mesh_runs; RUNS=$((RUNS + R)); else R=-; fi
	printf '%s %d.%d %s %s %s\n' "$(date +%T)" $((PCT / 10)) $((PCT % 10)) "$2" "$TEMP" "$R"
	SUM=$((SUM + PCT)) N=$((N + 1))
	[ "$TEMP" -gt "$HOT" ] && HOT=$TEMP
	LAST_TICKS=$1 LAST=$NOW
done
[ "$N" -gt 0 ] || exit 1
MEAN=$((SUM / N))
# Over the time actually read, shorter if lamp-eyes went away early.
RAN=$(($(date +%s) - START))
[ "$RAN" -gt 0 ] || RAN=1
printf 'summary: %d samples, lamp-eyes cpu mean %d.%d%%, hottest %s C, mesh runs %s (%s a minute), restarts %d\n' \
	"$N" $((MEAN / 10)) $((MEAN % 10)) "$HOT" \
	"$([ "$JOURNAL" = 1 ] && echo "$RUNS" || echo -)" \
	"$([ "$JOURNAL" = 1 ] && echo $((RUNS * 60 / RAN)) || echo -)" "$RESTARTS"
