#!/bin/sh
# fold-and-start.sh: Fifth's step 4a for lamp-4ace (#705), run on the lamp by the
# operator over ssh with the owner at the arm. It folds a half-raised arm by hand,
# starts the integration body through body-integration.sh and prints its first report.
#
#   1. lamp-body stopped, nothing else holding the servo port or the motor lock
#   2. read shoulder (2) and elbow (3): position, load, torque (lamp-body-inspect)
#   3. torque off on shoulder and elbow only, the owner's hands on the arm   [MANUAL]
#   4. the owner folds it; torque back on where it lies                      [MANUAL]
#      then a re-read: both within 80 of --folded-arm, torque on
#   5. the owner looks at the elbow and its cable
#   6. body-integration.sh (the operator's: stand, fold, report, left on)
#   7. the run's exit report (`lamp-body exit-report`) and the sheet's pass lines
#
# Steps 3 and 4's torque writes are manual: no repo tool writes torque to two joints
# only (lamp-body-inspect is read-only by design), so the operator does them with the
# tool used for the 07:19 elbow recovery. This script never writes to a servo.
#
# Usage: fold-and-start.sh [--dry-run]
#   --dry-run prints every command and the owner's lines, runs nothing, waits for nothing.
# Override with the environment: INSPECT (lamp-body-inspect), BODY_INTEGRATION
# (the operator's script), BODY_INTEGRATION_ARGS. Any failed check stops here.
set -u

DRY=0
case "${1:-}" in
--dry-run) DRY=1 ;;
"") ;;
*) echo "usage: $0 [--dry-run]" >&2; exit 2 ;;
esac

INSPECT=${INSPECT:-$HOME/.local/share/lampOS-voice/motor-inspect-50e8de4e/lamp-body-inspect}
BODY_INTEGRATION=${BODY_INTEGRATION:-$HOME/body-integration.sh}
BODY_INTEGRATION_ARGS=${BODY_INTEGRATION_ARGS:-}
NEAR=80
TMP=${TMPDIR:-/tmp}/fold-and-start.$$
mkdir -p "$TMP" || exit 1
trap 'rm -rf "$TMP"' EXIT

abort() { printf '\nSTOPPED: %s\nNothing more is done. lamp-body is left as it is now.\n' "$*" >&2; exit 1; }

# run CMD: print it; run it unless --dry-run.
run() {
	printf '  $ %s\n' "$*"
	[ "$DRY" = 1 ] && return 0
	sh -c "$*"
}

# step N OWNER_LINE: say the step and what the owner does, then wait for Enter.
step() {
	n=$1; shift
	printf '\n=== Step %s\nOwner: %s\n' "$n" "$*"
	[ "$DRY" = 1 ] && return 0
	printf 'Enter when ready (Ctrl-C stops): '
	read -r _ || abort "no input"
}

# The lamp's own unit names the binary, device, lock folder and fold.
if [ "$DRY" = 1 ]; then
	EXEC='<ExecStart from systemctl cat lamp-body>'
	BIN='<binary from ExecStart>' DEV='<--device>' UNIT_ID='<--unit-id>' LOCKS='<--lock-directory>'
	FOLD_S=2048 FOLD_E=2047
else
	EXEC=$(systemctl cat lamp-body | sed -n 's/^ExecStart=//p' | tail -1)
	[ -n "$EXEC" ] || abort "no ExecStart in systemctl cat lamp-body"
	opt() { printf '%s\n' "$EXEC" | sed -n "s/.* $1 \([^ ]*\).*/\1/p"; }
	BIN=${EXEC%% *} DEV=$(opt --device) UNIT_ID=$(opt --unit-id) LOCKS=$(opt --lock-directory)
	FOLD=$(opt --folded-arm)
	FOLD_S=${FOLD%,*} FOLD_E=${FOLD#*,}
	[ -n "$DEV" ] && [ -n "$UNIT_ID" ] && [ -n "$LOCKS" ] && [ -n "$FOLD" ] ||
		abort "the unit lacks --device, --unit-id, --lock-directory or --folded-arm"
	[ -x "$INSPECT" ] || abort "lamp-body-inspect not found at $INSPECT (set INSPECT)"
	[ -x "$BODY_INTEGRATION" ] || abort "body-integration.sh not found at $BODY_INTEGRATION (set BODY_INTEGRATION)"
	command -v jq >/dev/null || abort "jq is not installed"
fi
printf 'body %s\ndevice %s, lock folder %s, fold shoulder %s elbow %s\n' "$BIN" "$DEV" "$LOCKS" "$FOLD_S" "$FOLD_E"

# read_arm LABEL: one read-only inspection; leaves "joint position load torque temperature status"
# for the shoulder and the elbow in $TMP/arm.
read_arm() {
	run "'$INSPECT' --device '$DEV' --unit-id '$UNIT_ID' --lock-directory '$LOCKS' --status-faults report > '$TMP/read.json'" ||
		abort "lamp-body-inspect failed ($1)"
	[ "$DRY" = 1 ] && return 0
	jq -e '.completed == true' "$TMP/read.json" >/dev/null || abort "the inspection did not complete ($1): $(jq -c .fault "$TMP/read.json")"
	jq -r '[.transactions[].replies[]] as $r | (2, 3) as $j
		| ([$r[] | select(.joint_id == $j and .data.kind == "telemetry")] | last.data) as $t
		| ([$r[] | select(.joint_id == $j and .data.kind == "torque_and_profile")] | last.data) as $q
		| "\($j) \($t.position_raw) \($t.load_magnitude_raw) \($q.torque_enable_raw) \($t.temperature_raw) \($t.status_raw)"' \
		"$TMP/read.json" >"$TMP/arm" || abort "cannot read the inspection ($1)"
	echo "  $1 (joint position load torque temperature status):"
	sed 's/^2 /  shoulder /; s/^3 /  elbow    /' "$TMP/arm"
}

step 1 "hands off the lamp for now; the operator checks that nothing moves it."
run "systemctl is-active lamp-body" && [ "$DRY" = 0 ] && abort "lamp-body is running; it must stay stopped for the fold"
run "pgrep -a -x lamp-body; pgrep -a -f '[e]lbow-recover|[l]amp-body-inspect'" && [ "$DRY" = 0 ] &&
	abort "a lamp-body or servo tool is running (above)"
# Any process with the port or a motor lock open, found through /proc (no fuser needed).
run "sudo sh -c 'ls -l /proc/[0-9]*/fd 2>/dev/null' | grep -E '$DEV|motor-[0-9]+-[0-9]+\.lock'" && [ "$DRY" = 0 ] &&
	abort "something holds the servo port or the motor lock (above)"
echo "  stopped, and nothing holds the port or the lock."

step 2 "still hands off; the operator reads the arm as it is now."
read_arm "as found"

step 3 "one hand under the head, one holding the arm. When the operator says so, the shoulder and elbow go soft in your hands; keep holding."
cat <<EOF
  MANUAL, the operator: with the owner holding the arm, switch torque OFF on joint 2
  (shoulder) and joint 3 (elbow) only, with your own tool (as for the 07:19 recovery),
  under the lock folder $LOCKS. Head and base stay on. Enter when both are off.
EOF
[ "$DRY" = 0 ] && { printf 'Enter: '; read -r _ || abort "no input"; }

step 4 "lower the arm slowly by hand to its folded mat pose (as in Friday's rest photos), then hold it still there until the operator says it is held."
cat <<EOF
  MANUAL, the operator: with the arm folded and still, take hold of joints 2 and 3
  where they lie (goal = present position, then torque ON), as at 07:19. Enter when done.
EOF
[ "$DRY" = 0 ] && { printf 'Enter: '; read -r _ || abort "no input"; }
read_arm "folded"
if [ "$DRY" = 0 ]; then
	while read -r j pos load torque temp status; do
		[ "$j" = 2 ] && { name=shoulder; fold=$FOLD_S; } || { name=elbow; fold=$FOLD_E; }
		off=$((pos - fold)); [ "$off" -lt 0 ] && off=$((-off))
		[ "$off" -le "$NEAR" ] || abort "$name at $pos, $off from its fold $fold (more than $NEAR): fold it again by hand"
		[ "$torque" = 1 ] || abort "$name torque is not on: the body would refuse to stand; take hold of it first"
		[ "$temp" -lt 55 ] || abort "$name at $temp C: leave it folded until it cools"
		[ "$status" = 0 ] || echo "  note: $name status $status (post it)"
	done <"$TMP/arm"
	echo "  folded: both within $NEAR, torque on."
fi

step 5 "hands off now. Look at the elbow and its cable: nothing pinched, stretched, rubbed or loose. Tell the operator yes or no."
if [ "$DRY" = 0 ]; then
	printf 'Elbow and cable fine? [y/N] '
	read -r ok || abort "no input"
	[ "$ok" = y ] || abort "the owner did not pass the elbow and its cable"
fi

step 6 "hands off and watch the first minute: it stands up in about 6 s and folds about 90 s later. Say stop at anything odd; the operator runs sudo systemctl stop lamp-body."
T=$(date '+%F %T')
START=$(date +%s)
run "'$BODY_INTEGRATION' $BODY_INTEGRATION_ARGS" || abort "body-integration.sh failed; it has put the old unit back and left the body stopped"

step 7 "nothing to do; the operator reads the report."
LEFT=$((110 - ($(date +%s) - START)))
[ "$LEFT" -gt 0 ] && run "sleep $LEFT"
# The integration body's binary, from the unit as body-integration.sh left it.
[ "$DRY" = 0 ] && BIN=$(systemctl cat lamp-body | sed -n 's/^ExecStart=//p' | tail -1) && BIN=${BIN%% *}
run "sudo journalctl -u lamp-body -o short-iso --since '$T' | '$BIN' exit-report" ||
	abort "no readable exit report since $T (exit-report above says why)"
run "systemctl show lamp-body -p ActiveState,NRestarts"
cat <<'EOF'

Pass lines (Fifth's body sheet, #705 14:49), the report must show:
  stop: cancelled
  found: shoulder and elbow within 80 of 2048 / 2047
  stood: true, standing_refused: null, retreats: [], lowered: null, released: []
  no window stop; 0 OUT arm goals (body-integration.sh's awk)
  bad_reads: all 0; no settle of 60 or more; peaks under 55 (55-61: post it, leave it folded)
  torque_writes: 0 (1 means the elbow's old overload latched: post it); goal_writes above 0
  then: ActiveState=active, NRestarts=0 on the left-on run
Post the report, the two reads above and the stills on #705.
EOF
