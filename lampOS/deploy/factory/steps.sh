#!/usr/bin/env bash
# The factory's per-lamp steps for one lamp (#1682), in install.md's order:
#   1. the body's poses: lamp-body-measure at rest, then (with --write) the
#      arm stood at 30% and 100% to take where this arm stops, into
#      /etc/lampOS/body/lamp.env
#   2. the touch pads: on GPIO head pads, which lines this unit's pads are on
#      and which way they read (lamp-controls discover-head, a person holding
#      each pad), into the pins file; on an MPR121, calibrate-touch
#      only while lamp-controls runs with LAMP_CONTROLS_STRIP=on
#   3. the update key's public half: /etc/lampOS/update/key.pub; optionally
#      the separate Store key: /etc/lampOS/skills/store-key.pub (never enables Store)
#   4. the self-test: lamp-selftest --occasion factory, the lamp first stood
#      up by its running lamp-body ("listening" on its mood socket) so
#      that head_move is checked whether or not the voice is up (#2232),
#      with lamp-expression stopped for the run and started again after;
#      then, passed, the lamp is paused (lamp-keeper pause, keeper.md) so it
#      folds and stays at rest, taking no turn, until it is next started
# Run on the lamp, as root. Every step is dry unless --write: it reads and
# says what it would write, and nothing is written to /etc. A step that
# fails stops the rest and says which. Written, steps 1, 2 and 4 need a
# person at the lamp (a hand by the arm as it stands; a swipe, a touch, a
# press; the self-test moves the head a little and lights the ring), so each
# asks first and is refused without a console.
#
#   steps.sh --unit-id ID [--write] [--update-key FILE] [--store-key FILE] [--from N] [--to N]
#            [--device /dev/ttyACM0] [--lock-directory /run/lampos-locks/motor]
#            [--body-user lampos] [--pins /etc/lampOS/controls/controls-pins.json]
#
# Ctrl-C (or SIGTERM) during a stand eases the arm back, as a stand's own end
# does, waits for the body to fold and end, and stops the steps.
#
# The binaries are the installed ones; the body needs --let-go-at-stop (#1776).
# For a test, MEASURE_BIN, BODY_BIN, MOOD_BIN, CONTROLS_BIN, SELFTEST_BIN,
# KEEPER_BIN, SYSTEMCTL, JQ, SETPRIV and ETC_ROOT (a folder standing for /) replace them;
# STAND_WAIT is the seconds a stand is given (34), HEAD_STAND_WAIT the
# seconds the running body is given to stand before the self-test (9),
# BODY_START_WAIT the seconds lamp-body, started for step 4 when it is not
# running, is given to open its mood socket (30),
# QUIET_WAIT the seconds waited after lamp-expression stops (61), STOP_WAIT
# the seconds a stopped body is given to fold and end (12), and
# AS_BODY_USER=1 runs the body as --body-user under ETC_ROOT too.
# HEAD_LINES_FROM=FILE, for a test on a lamp with no person (#2818), passes
# discover-head --lines-from FILE: the head lines read from FILE (the high
# lines' numbers, or none), which a script writes to touch them. Its found
# pads never reach the live pins file, here or under ETC_ROOT: they are put
# in $PINS.test-input beside it, and lamp-controls is not restarted.
set -uo pipefail

# The header: the comment lines from line 2 to the first that is not one.
usage() { awk 'NR > 1 && !/^#/ { exit } NR > 1' "$0" | sed 's/^# \{0,1\}//'; }

ROOT=${ETC_ROOT:-}
MEASURE_BIN=${MEASURE_BIN:-/opt/lampOS/body/current/lamp-body-measure}
BODY_BIN=${BODY_BIN:-/opt/lampOS/body/current/lamp-body}
MOOD_BIN=${MOOD_BIN:-/opt/lampOS/body/current/lamp-body-mood}
STAND_WAIT=${STAND_WAIT:-34}
HEAD_STAND_WAIT=${HEAD_STAND_WAIT:-9}
BODY_START_WAIT=${BODY_START_WAIT:-30}
QUIET_WAIT=${QUIET_WAIT:-61}
# A body told SIGTERM eases the arm back and folds, up to about 6 s; twice
# that is waited before it is called stuck.
STOP_WAIT=${STOP_WAIT:-12}
SETPRIV=${SETPRIV:-setpriv}
CONTROLS_BIN=${CONTROLS_BIN:-/opt/lampOS/controls/current/lamp-controls}
SELFTEST_BIN=${SELFTEST_BIN:-/opt/lampOS/selftest/current/lamp-selftest}
SYSTEMCTL=${SYSTEMCTL:-systemctl}
KEEPER_BIN=${KEEPER_BIN:-/usr/local/bin/lamp-keeper}
JQ=${JQ:-jq}
UNIT_ID='' WRITE=0 KEY='' STORE_KEY='' FROM=1 TO=4
DEVICE=/dev/ttyACM0 LOCKS=/run/lampos-locks/motor BODY_USER=lampos
PINS=$ROOT/etc/lampOS/controls/controls-pins.json
while [[ $# -gt 0 ]]; do
  case $1 in
    --unit-id) UNIT_ID=${2:-}; shift 2 ;;
    --write) WRITE=1; shift ;;
    --update-key) KEY=${2:-}; shift 2 ;;
    --store-key)
      [[ $# -ge 2 && -n ${2:-} && ${2:-} != --* ]] || { echo "factory: --store-key needs FILE" >&2; exit 2; }
      STORE_KEY=$2; shift 2 ;;
    --from) FROM=${2:-}; shift 2 ;;
    --to) TO=${2:-}; shift 2 ;;
    --device) DEVICE=${2:-}; shift 2 ;;
    --lock-directory) LOCKS=${2:-}; shift 2 ;;
    --body-user) BODY_USER=${2:-}; shift 2 ;;
    --pins) PINS=${2:-}; shift 2 ;;
    --help) usage; exit 0 ;;
    *) echo "factory: unknown option $1" >&2; usage >&2; exit 2 ;;
  esac
done
[[ -n $UNIT_ID ]] || { echo "factory: --unit-id is required" >&2; exit 2; }
[[ $FROM =~ ^[1-4]$ ]] || { echo "factory: --from takes 1 to 4" >&2; exit 2; }
[[ $TO =~ ^[1-4]$ && $TO -ge $FROM ]] || { echo "factory: --to takes $FROM to 4" >&2; exit 2; }
# On the lamp, as root, the body and its tools run as the body's user; under
# a test's ETC_ROOT, or not as root, as is.
if [[ -z ${AS_BODY_USER:-} ]]; then
  AS_BODY_USER=$([[ -z $ROOT && $(id -u) == 0 ]] && echo 1 || echo 0)
fi

LAMP_ENV=$ROOT/etc/lampOS/body/lamp.env
KEY_PUB=$ROOT/etc/lampOS/update/key.pub
STORE_KEY_PUB=$ROOT/etc/lampOS/skills/store-key.pub
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
MODE=$([[ $WRITE == 1 ]] && echo write || echo dry)

say() { echo "factory: $*"; }
fail() { echo "factory: FAILED: $*" >&2; return 1; }

# A person at the lamp says yes on the console, or the step is refused.
ask() {
  local answer
  if [[ -n ${ASK_ANSWER:-} ]]; then
    answer=$ASK_ANSWER # tests only (steps-test.sh): the person's answer
  else
    [[ -t 0 ]] || { fail "$1 needs a person at the lamp and a console to answer"; return 1; }
    read -r -p "factory: $1 Ready? [y/N] " answer
  fi
  [[ $answer == y || $answer == Y ]] || { fail "not ready for: $1"; return 1; }
}

# Run as the body's user on the lamp; as is under a test's ETC_ROOT. setpriv
# execs the command, so it is the process started, and a signal sent to it
# reaches it; runuser forwards SIGTERM, then kills its child 2 s later, which
# would cut a fold short (#2541). --init-groups keeps the user's groups
# (dialout, for the servo port).
as_body() {
  if [[ $AS_BODY_USER == 1 ]]; then
    "$SETPRIV" --reuid="$BODY_USER" --regid="$(id -g "$BODY_USER")" --init-groups -- "$@"
  else
    "$@"
  fi
}

# The body run by hand for a stand, while it runs. A non-interactive shell's
# background job ignores SIGINT, so Ctrl-C reaches only this script: it
# sends the body SIGTERM, which eases the arm back (the reviewer on #1744).
# The body is then left to fold and end by itself, STOP_WAIT at most, in
# pauses the traps can cut short; killed while folding it would leave the arm
# held part-way down (#2541), so it is killed only past that, and said so.
STAND_PID=''
EASED='was eased back'
ease_back() {
  [[ -n $STAND_PID ]] || return 0
  local pid=$STAND_PID tenths=0 folded=0
  EASED='was eased back'
  kill -TERM "$pid" 2>/dev/null
  while kill -0 "$pid" 2>/dev/null && ((tenths < STOP_WAIT * 10)); do
    pause 0.1
    tenths=$((tenths + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    echo "factory: WARNING: the body did not fold and end within $STOP_WAIT s of SIGTERM: it is killed now, and the arm may be held where it is; keep a hand on it and take the power off" >&2
    kill -KILL "$pid" 2>/dev/null
    EASED='was not eased back (see the warning above)'
    folded=1
  fi
  wait "$pid" 2>/dev/null
  STAND_PID=''
  return "$folded"
}
# A body killed while folding left the arm part-way down, torque on: nothing
# after it may run, not even a rest read (the reviewer-lead on #2597).
not_folded() {
  echo "factory: the arm did not fold in time: stopped here, the arm held; a person supports it and the lamp is let go with step 1's let-go run" >&2
  exit 1
}
trap 'ease_back; echo "factory: stopped; the arm $EASED and no step after it was run" >&2; exit 130' INT TERM
# lamp-controls is stopped while the touch step reads its lines; it is
# started again however the step or the script ends.
CONTROLS_STOPPED=0
controls_back() {
  [[ $CONTROLS_STOPPED == 1 ]] || return 0
  CONTROLS_STOPPED=0
  "$SYSTEMCTL" start lamp-controls || fail "could not start lamp-controls again"
}
# lamp-expression is stopped while the self-test moves the head (#2232); it is
# started again however the step or the script ends.
EXPRESSION_STOPPED=0
expression_back() {
  [[ $EXPRESSION_STOPPED == 1 ]] || return 0
  EXPRESSION_STOPPED=0
  "$SYSTEMCTL" start lamp-expression || fail "could not start lamp-expression again"
}
# lamp-voice reads the privacy switch from lamp-controls, and lamp-ears may
# listen only on the voice's lease (privacy-switch.md): with Controls stopped
# under a listening voice, the conversation ends on an unreadable switch and
# keeps the restart guard's mute (controls.md), so the voice, and the ears
# with it, stay closed and the ring stops saying what it shows, even after a
# restart (#2818 on lamp-52e6: ears/level and light/state as old as the touch
# step's stop, and record.json.restart-guard left). So those of the two that
# run are stopped before Controls is, a stop that releases the guard and
# keeps a person's own mute, and started again once Controls is back, after
# the step, however it or the script ends; the self-test still needs a fresh
# read of each, so a dead microphone or ring still fails.
LISTENERS='' LISTENERS_READ=0
listeners_down() {
  [[ $LISTENERS_READ == 1 ]] && return 0
  LISTENERS_READ=1
  local unit
  for unit in lamp-voice lamp-ears; do
    [[ $("$SYSTEMCTL" is-active "$unit" 2>/dev/null) == active ]] && LISTENERS+=" $unit"
  done
  [[ -n $LISTENERS ]] || return 0
  # shellcheck disable=SC2086 # the unit names, one word each
  "$SYSTEMCTL" stop $LISTENERS || { fail "could not stop$LISTENERS before lamp-controls"; return 1; }
  say "stopped$LISTENERS while lamp-controls is stopped, so the voice ends cleanly, not on a lost privacy switch"
}
listeners_back() {
  LISTENERS_READ=0
  [[ -n $LISTENERS ]] || return 0
  local units=$LISTENERS
  LISTENERS=''
  # shellcheck disable=SC2086 # the unit names, one word each
  "$SYSTEMCTL" start $units || { fail "could not start$units again after lamp-controls"; return 1; }
  say "started$units again, lamp-controls back"
}
trap 'ease_back; controls_back; expression_back; listeners_back' EXIT
# A wait the traps can cut short: bash runs a trap only once a foreground
# command ends, and a wait ends at once.
pause() {
  sleep "$1" &
  wait $!
}

# One run of the body by hand, as the watched run on lamp-cb94 (#1717):
# lamp-body on a private mood socket the voice does not know, nothing else
# that moves on its own. With a SHARE, one stand: one "listening", then
# SIGTERM eases it back and folds it. Without one, it only starts and is
# stopped. Either way --let-go-at-stop (#1776) lets the lamp go once it is
# down, so that it can be read at rest again; a lamp that did not get down
# keeps holding. The body's lines are left in STAND_LOG, and shown when it
# fails. Run in this shell, never in $( ): a subshell would not ease the arm
# back on Ctrl-C.
STAND_LOG=''
body_once() {
  local share=$1 poses=$2 dir log pid body
  dir=$(mktemp -d "${ROOT:-}/tmp/factory-stand.XXXXXX") || return 1
  [[ $AS_BODY_USER == 1 ]] && chown "$BODY_USER" "$dir"
  log=$dir/body.log
  STAND_LOG=$log
  body=("$BODY_BIN" --device "$DEVICE" --unit-id "$UNIT_ID" --lock-directory "$LOCKS"
    --mood-socket "$dir/mood.sock" --reach 150,15,1,250 --speed 120 --servo-speed 1000
    --servo-acceleration 60 --force-limit 600 --rest-glance off --look-at-name off
    --let-go-at-stop on)
  # A stand wakes a head that sagged when the lamp was let go (lamp-a63e,
  # #2396: pitch 1754 under its start window) slowly home first, as the
  # product unit's body does, within 400 counts; the let-go run moves nothing.
  # shellcheck disable=SC2206 # the poses are flags, split as the unit splits them
  if [[ -n $share ]]; then
    body+=(--wake on --stand-share "$share" --stand-seconds 6 --awake-seconds 20 --idle off $poses)
  else
    body+=(--wake off)
  fi
  [[ $AS_BODY_USER == 1 ]] &&
    body=("$SETPRIV" --reuid="$BODY_USER" --regid="$(id -g "$BODY_USER")" --init-groups -- "${body[@]}")
  # Started as itself, never through a function: "as_body ... &" would make
  # $! a subshell, and SIGTERM to it would leave the body running, arm up.
  # setpriv execs the body, so $! is the body itself, and SIGTERM reaches it
  # with nothing to kill it before it has folded (as_body says why not runuser).
  "${body[@]}" >"$log" 2>&1 &
  pid=$!
  STAND_PID=$pid
  pause 4
  if ! kill -0 "$pid" 2>/dev/null; then STAND_PID=''; cat "$log" >&2; return 1; fi
  if [[ -n $share ]]; then
    as_body "$MOOD_BIN" --socket "$dir/mood.sock" --mood listening --level 128 \
      || { ease_back || not_folded; cat "$log" >&2; return 1; }
    pause "$STAND_WAIT"
  fi
  ease_back || not_folded
}
stand_once() { body_once "$1" "$2"; }

# The rest read: lamp-body-inspect's plan, no register writes, no motion.
# Its lines are printed; its words kept in READ_SAID.
READ_SAID=''
rest_read() {
  READ_SAID=$(as_body "$MEASURE_BIN" --device "$DEVICE" --unit-id "$UNIT_ID" --lock-directory "$LOCKS" \
    --write "$1" 2>&1)
  local status=$?
  printf '%s\n' "$READ_SAID"
  return "$status"
}

# Where the arm stopped standing: the last body line at its full stand,
# "arm goals [..] positions [SHOULDER, ELBOW]". Empty if it never stood.
stood_at() {
  grep -E 'stand (0\.9[5-9]|1\.00)' | grep -oE 'arm goals \[[0-9, ]+\] positions \[[0-9]+, [0-9]+\]' \
    | tail -1 | sed -E 's/.*positions \[([0-9]+), ([0-9]+)\]/\1 \2/'
}

step_body() {
  # Stopped, not only "not active": a unit waiting on its missing lamp.env
  # restarts by itself, and would start the moment this step writes it
  # (lamp-c0a4, 19 Oct: "activating", auto-restart).
  local body_state
  body_state=$("$SYSTEMCTL" is-active lamp-body 2>/dev/null)
  if [[ $body_state != inactive && $body_state != failed && $body_state != unknown ]]; then
    fail "lamp-body is $body_state: stop it first (systemctl stop lamp-body); the read needs the servo port to itself, and a unit that restarts by itself would start as soon as lamp.env is written"
    return 1
  fi
  if [[ $WRITE == 1 && -e $LAMP_ENV ]]; then
    fail "$LAMP_ENV is already there: never measured over; move it aside to measure again"
    return 1
  fi
  local read_file=${ROOT:-}/tmp/factory-lamp-env-$STAMP
  if ! rest_read "$read_file"; then
    # Any run of the body leaves the lamp held, torque on, until the power
    # goes (body.md); only a run with --let-go-at-stop lets it go.
    if [[ $READ_SAID != *"has torque on"* ]]; then
      fail "lamp-body-measure refused or could not read the body (its reason is above)"
      return 1
    fi
    if [[ $WRITE != 1 ]]; then
      fail "the lamp is held, torque on (a run of the body since power-on leaves it so): a written run lets it go first, with a person at the arm"
      return 1
    fi
    ask "the lamp is held, torque on: it is let go where it lies, folded, and may sag a little; keep a hand by the arm and the head." || return 1
    say "letting the lamp go"
    # A head found outside its start window is let go only with
    # --folded-arm SHOULDER,ELBOW given alone and the arm within 20 counts
    # of it (#1682); without it, as here, the run is refused, nothing written.
    # The folded counts must come from a read the person confirmed was
    # folded, never from this run's own read of where the arm is now.
    body_once "" "" || { fail "the body would not run to let the lamp go (its lines are above)"; return 1; }
    rest_read "$read_file" || { fail "lamp-body-measure still refused after the lamp was let go (its reason is above)"; return 1; }
  fi
  if [[ $WRITE != 1 ]]; then
    say "would stand the arm at 30% and 100%, with a person's hand on it, to take where it stops into --stand (dry)"
    return 0
  fi
  # The stand: lamp-4ace's until this arm's is measured (#1717's run).
  ask "the arm's stand: it rises to 30%, then to its full stand, and folds after each; keep a hand resting on the arm and say stop at any time." || return 1
  local poses stood shoulder elbow standing
  poses=$(sed -n 's/^BODY_POSES="\(.*\)"$/\1/p' "$read_file")
  [[ -n $poses ]] || { fail "the read gave no BODY_POSES"; return 1; }
  say "standing at 30%"
  stand_once 30 "$poses" || { fail "the body would not run for the 30% stand (its lines are above)"; return 1; }
  say "standing at 100%"
  stand_once 100 "$poses" || { fail "the body would not run for the 100% stand (its lines are above)"; return 1; }
  stood=$(stood_at <"$STAND_LOG")
  [[ -n $stood ]] || { tail -5 "$STAND_LOG" >&2; fail "the arm did not reach its full stand, so its stand was not measured"; return 1; }
  read -r shoulder elbow <<<"$stood"
  say "this arm stands at shoulder $shoulder, elbow $elbow"
  # --stand PITCH,ROLL,YAW,SHOULDER,ELBOW: this arm's own last two.
  standing=$(sed -n 's/.*--stand \([0-9]*,[0-9]*,[0-9]*\),[0-9]*,[0-9]*.*/\1/p' <<<"$poses")
  [[ -n $standing ]] || { fail "the read gave no --stand"; return 1; }
  install -d -m 0755 "$(dirname "$LAMP_ENV")" || return 1
  sed -e "s/--stand [0-9]*,[0-9]*,[0-9]*,[0-9]*,[0-9]*/--stand $standing,$shoulder,$elbow/" \
    -e "s/^# home, folded-arm: found at rest. stand: lamp-4ace's with this lamp's roll and yaw./# home, folded-arm: found at rest. stand: this arm's, stood at 100% by the factory's steps, with this lamp's roll and yaw./" \
    "$read_file" >"$LAMP_ENV.new" && mv -n "$LAMP_ENV.new" "$LAMP_ENV" && [[ ! -e $LAMP_ENV.new ]] \
    || { fail "could not write $LAMP_ENV"; return 1; }
  chmod 0644 "$LAMP_ENV"
  say "wrote $LAMP_ENV"
  cat "$LAMP_ENV"
}

# GPIO head pads differ by unit: each lamp's are on any of lines 37, 96, 97
# and 98, and the hardware team's test passes when two or more read a touch
# (the hardware lead, #2182): lamp-52e6's and lamp-cb94's on 97 and 98,
# lamp-dbda's on 37, 96 and 97. The product's pins read all four until this
# step writes the unit's own (#1798). discover-head samples the four with
# both pulls and counts complete pulses; a line driven at idle that gave two
# or more, or one long hold, is a pad, and its idle level gives its polarity.
# Two to four such lines, or nothing is written. The pull follows the
# polarity, so a line with nothing on it reads as not touched. Each unit's pads also rest and read
# differently (#1764): lamp-cb94's line 97 flipped at rest and read touched
# for 77 s, lamp-c0a4's pulsed under a hand. So the read begins with 10 s of
# hands away, a pad whose line changed then is refused, and each pad's own
# baseline is written beside it (head_rest: its rest, and whether a touch
# holds its level or pulses), which lamp-controls reads it by (#711).
step_head_pads() {
  if [[ $WRITE != 1 ]]; then
    say "would stop lamp-controls, run $CONTROLS_BIN discover-head --seconds 40 --rest-seconds 10 with a person's hands away for 11 s, then holding each head pad about 2 s, write the pads' lines, polarity and rest into $PINS and start lamp-controls again (dry)"
    return 0
  fi
  command -v "$JQ" >/dev/null || { fail "$JQ is not installed: the head pads step reads discover-head's report with it"; return 1; }
  ask "the head pads: for 40 s, hands away from the lamp for the first 11 s (count them), then hold head pad 1 about 2 s and let go, then head pad 2 the same, and any other head pad the same; a hand on two at once is fine (the button is off meanwhile)." || return 1
  local report=$ROOT/var/tmp/factory-head-pads-$STAMP.json out=$PINS.factory-$STAMP pads
  mkdir -p "$(dirname "$report")"
  listeners_down || return 1
  "$SYSTEMCTL" stop lamp-controls || { fail "could not stop lamp-controls"; return 1; }
  CONTROLS_STOPPED=1
  "$CONTROLS_BIN" discover-head --seconds 40 --rest-seconds 10 ${HEAD_LINES_FROM:+--lines-from "$HEAD_LINES_FROM"} >"$report"
  local status=$?
  controls_back
  [[ $status == 0 ]] || { fail "discover-head could not read the head lines (report: $report)"; return 1; }
  say "the head pads report: $report"
  pads=$("$JQ" -c '[.lines[] | select(.active_low_candidate != null)
    | {line, active_low: .active_low_candidate, pull: (if .active_low_candidate then "up" else "down" end),
       rest_changes, reads}]' "$report") \
    || { fail "the head pads report could not be read"; return 1; }
  # Each pad as it was found, so the person hears which touches counted
  # (thirteen-P4).
  local found
  while read -r found; do
    [[ -n $found ]] && say "$found"
  done < <("$JQ" -r '.[] | "a head pad on line \(.line): rests \(if .active_low then "high" else "low" end), \(.reads // "touch not told") under a hand"' <<<"$pads")
  local restless
  restless=$("$JQ" -c '[.[] | select((.rest_changes // 0) > 0) | .line]' <<<"$pads")
  if [[ $restless != "[]" ]]; then
    fail "head pad lines $restless changed with hands away, so their rest does not hold and a touch can't be told on them (#1764): the pins are not changed (report: $report)"
    return 1
  fi
  local unread
  unread=$("$JQ" -c '[.[] | select(.rest_changes != null and .reads == null) | .line]' <<<"$pads")
  if [[ $unread != "[]" ]]; then
    fail "head pad lines $unread were neither held half a second nor pulsing under the hand: hold each pad about 2 s, and run the step again (report: $report)"
    return 1
  fi
  if (( $("$JQ" length <<<"$pads") < 2 )); then
    fail "head pads found on $("$JQ" -c '[.[].line]' <<<"$pads") lines, not two or more, each held or touched twice: the pins are not changed (report: $report)"
    return 1
  fi
  # The baseline only from a discover-head that measured it (rest and reads),
  # so an older lamp-controls is never given a key it refuses.
  "$JQ" --argjson pads "$pads" \
    '.head_pads = [$pads | to_entries[] | {name: "head-\(.key + 1)"} + (.value | {line, active_low, pull})]
     | if all($pads[]; .reads != null and .rest_changes != null)
       then .head_rest = [$pads | to_entries[] | {name: "head-\(.key + 1)",
         rest: (if .value.active_low then "high" else "low" end), reads: .value.reads}]
       else del(.head_rest) end' "$PINS" >"$out" \
    || { fail "could not write the head pads into $out"; return 1; }
  say "head pads: $("$JQ" -c '[.head_pads[] | {line, active_low}]' "$out"), rest: $("$JQ" -c '.head_rest // "not measured"' "$out")"
  # A test input's lines were read with the probes forced off, so its pads
  # would all be written active-high: they go beside the live pins, never
  # into them (#2915's P2).
  if [[ -n ${HEAD_LINES_FROM:-} ]]; then
    mv -f "$out" "$PINS.test-input" || { fail "could not write the found lines to $PINS.test-input"; return 1; }
    say "test input: live pins not written, found lines in $PINS.test-input"
    return 0
  fi
  # The profile is copied aside and the new pins renamed over it, so the
  # pins file is never missing or half written.
  cp -p "$PINS" "$PINS.before-factory-$STAMP" && mv -f "$out" "$PINS" || { fail "could not put the head pads in place"; return 1; }
  say "head pads in $PINS; the profile's kept as $PINS.before-factory-$STAMP"
  "$SYSTEMCTL" restart lamp-controls || { fail "lamp-controls did not restart with the head pads"; return 1; }
}

step_touch() {
  [[ -r $PINS ]] || { fail "no pins file at $PINS"; return 1; }
  # An MPR121 pins file names its chip in a top-level "base" (controls.md).
  if ! grep -q '"base"' "$PINS"; then
    step_head_pads
    return
  fi
  # The base strip is off unless lamp-controls runs with LAMP_CONTROLS_STRIP=on
  # (#1891, controls.md "The base strip is off"): nothing would read its
  # calibration, so only the head pads are found.
  if ! "$SYSTEMCTL" show -p Environment lamp-controls 2>/dev/null | grep -qw "LAMP_CONTROLS_STRIP=on"; then
    say "the base strip is off (LAMP_CONTROLS_STRIP, controls.md): its calibration is skipped; the head pads are found as on any lamp"
    step_head_pads
    return
  fi
  if [[ $WRITE != 1 ]]; then
    say "would stop lamp-controls, run $CONTROLS_BIN calibrate-touch --pins $PINS with a person at the lamp (45 s), put its pins in place and start lamp-controls again (dry)"
    return 0
  fi
  ask "the touch calibration: 15 s with hands away, then 30 s swiping each pad of the strip end to end." || return 1
  local out=$PINS.factory-$STAMP report=$ROOT/var/tmp/factory-touch-report-$STAMP.json
  mkdir -p "$(dirname "$report")"
  # Calibration takes the chip's lock, which lamp-controls holds: it is
  # stopped for it and started again however the step ends.
  listeners_down || return 1
  "$SYSTEMCTL" stop lamp-controls || { fail "could not stop lamp-controls"; return 1; }
  CONTROLS_STOPPED=1
  "$CONTROLS_BIN" calibrate-touch --pins "$PINS" --output "$out" >"$report"
  local status=$?
  controls_back
  [[ $status == 0 ]] || { fail "calibrate-touch did not give every working pad a threshold (report: $report)"; return 1; }
  say "the touch report: $report"
  cp -p "$PINS" "$PINS.before-factory-$STAMP" && mv -f "$out" "$PINS" || { fail "could not put the calibrated pins in place"; return 1; }
  say "calibrated pins in $PINS; the profile's kept as $PINS.before-factory-$STAMP"
  "$SYSTEMCTL" restart lamp-controls || { fail "lamp-controls did not restart with the calibrated pins"; return 1; }
}

# One public key per purpose. Missing optional Store material never copies
# the update key, and provisioning either key changes no service or switch.
place_key() {
  local kind=$1 source=$2 destination=$3 option=$4 required=$5 bytes encoded canonical
  if [[ -z $source ]]; then
    if [[ -e $destination ]]; then
      source=$destination
    elif [[ $required == 0 ]]; then
      say "no Store key supplied: Store stays off; use --store-key FILE when its public key is available"
      return 0
    else
      fail "no $kind key: give the owner's public half with $option FILE"
      return 1
    fi
  fi
  [[ -r $source && -f $source ]] || { fail "cannot read $source"; return 1; }
  # The public half: one line of base64, exactly 32 bytes of Ed25519.
  # Rust's reader rejects internal whitespace and noncanonical pad bits.
  # Validate the bytes we will actually copy, not a newline-stripped form.
  encoded=$(cat "$source")
  [[ $encoded =~ ^[A-Za-z0-9+/]{43}=$ ]] || { fail "$source is not a 32-byte Ed25519 public key in base64 on one line"; return 1; }
  if ! bytes=$(printf '%s' "$encoded" | base64 -d 2>/dev/null | wc -c | tr -d ' '); then
    fail "$source is not a 32-byte Ed25519 public key in base64"
    return 1
  fi
  [[ $bytes == 32 ]] || { fail "$source is not a 32-byte Ed25519 public key in base64 (decoded $bytes bytes)"; return 1; }
  canonical=$(printf '%s' "$encoded" | base64 -d | base64 | tr -d '\n')
  [[ $canonical == "$encoded" ]] || { fail "$source is not canonical base64"; return 1; }
  if [[ -e $destination || -L $destination ]]; then
    [[ ! -L $destination ]] || { fail "$destination is a link: not followed"; return 1; }
    cmp -s "$source" "$destination" && { say "the same $kind key is already in $destination"; return 0; }
    fail "a different key is already in $destination: not replaced"
    return 1
  fi
  if [[ $WRITE == 1 ]]; then
    install -d -m 0755 "$(dirname "$destination")" && install -m 0644 "$source" "$destination" \
      || { fail "could not place $destination"; return 1; }
    say "the $kind key is in $destination"
  else
    say "would place $source as $destination (dry)"
  fi
}

step_key() {
  place_key update "$KEY" "$KEY_PUB" --update-key 1 || return 1
  place_key Store "$STORE_KEY" "$STORE_KEY_PUB" --store-key 0
}

# head_move is checked only on a lamp at rest and standing, and never stands
# one up itself (selftest.md); with the voice down nothing else does, so it
# stayed not_checked (#2232). The running lamp-body, the one the self-test
# reads, is told "listening" on its own mood socket, as the voice tells it:
# it stands (--stand-seconds 6), rests again 1.5 s after the last one
# (MOOD_STALE), standing, and folds by itself --awake-seconds (90) after it,
# however this script ends. Said a few times: a voice that is up tells it
# "resting" every 40 ms, and the body keeps the newest in each step.
# lamp-expression asks the body on expression/body all the time (its resting
# breath), and the body keeps only the newest request, so head_move stays
# not_checked while it runs, and needs a minute with nobody else asking
# (lamp-52e6 on #2238: "asked to move 0 s ago by someone else"). So it is
# stopped, the minute waited, and then the lamp stood: the stand comes after
# the minute, so that the lamp has not folded again (--awake-seconds 90).
BODY_MOOD=$ROOT/run/lampos-body/mood.sock
HEAD_STOOD=0
stand_for_head() {
  local body_state tenths=0
  # A lamp paused by an earlier pass (below) takes every mood as rest and
  # would not stand; the pause is the steps' own, so it is cleared first.
  if [[ -x $KEEPER_BIN ]] && ! "$KEEPER_BIN" resume; then
    fail "could not resume the lamp with $KEEPER_BIN before the head check"
    return 1
  fi
  body_state=$("$SYSTEMCTL" is-active lamp-body 2>/dev/null)
  # Step 1 needs lamp-body stopped and nothing after it starts it again, so a
  # body not running is started here, whatever step the run began at, and
  # given BODY_START_WAIT at most for its mood socket, as addon-say.sh waits
  # for the voice's (#2818); a self-test without it leaves head_move unchecked.
  if [[ $body_state != active ]]; then
    say "lamp-body is $body_state: starting it; waiting up to ${BODY_START_WAIT} s for its mood socket"
    "$SYSTEMCTL" start lamp-body || { fail "could not start lamp-body: fix it (journalctl -u lamp-body says why) before the self-test, which checks head_move only on a running body"; return 1; }
    until [[ $("$SYSTEMCTL" is-active lamp-body 2>/dev/null) == active && -S $BODY_MOOD ]]; do
      ((tenths < BODY_START_WAIT * 10)) \
        || { fail "lamp-body did not come up with $BODY_MOOD within ${BODY_START_WAIT} s: fix it (journalctl -u lamp-body says why) before the self-test, which checks head_move only on a running body"; return 1; }
      pause 0.1
      tenths=$((tenths + 1))
    done
  fi
  if [[ $("$SYSTEMCTL" is-active lamp-expression 2>/dev/null) == active ]]; then
    "$SYSTEMCTL" stop lamp-expression || { fail "could not stop lamp-expression"; return 1; }
    EXPRESSION_STOPPED=1
    say "lamp-expression stopped for the head's small move; waiting ${QUIET_WAIT} s for the body to be left alone"
    pause "$QUIET_WAIT"
  fi
  say "standing the lamp up for the head's small move"
  local _
  for _ in 1 2 3 4 5; do
    as_body "$MOOD_BIN" --socket "$BODY_MOOD" --mood listening --level 128 \
      || { fail "could not tell lamp-body to stand on $BODY_MOOD"; return 1; }
    pause 0.2
  done
  pause "$HEAD_STAND_WAIT"
  HEAD_STOOD=1
}

step_selftest() {
  if [[ $WRITE != 1 ]]; then
    say "would run: $SELFTEST_BIN --occasion factory (dry; it stands the lamp up for head_move, asks for a touch, moves the head a little and lights the ring)"
    return 0
  fi
  ask "the self-test: the lamp stands up for the head's small move (keep a hand by the arm as it rises, not on it, and say stop at any time), then it moves the head a little, lights the ring, and asks for a swipe, a touch on the head and a press of the button." || return 1
  stand_for_head || return 1
  "$SELFTEST_BIN" --occasion factory
  local status=$?
  expression_back
  [[ $HEAD_STOOD == 1 ]] && say "the lamp folds by itself within 90 s of standing (lamp-body's --awake-seconds)"
  # Exit 3, a part not checked, counts as the unit's success (selftest.md).
  [[ $status == 0 || $status == 3 ]] || { fail "the self-test found a part that failed (exit $status)"; return 1; }
  # Left as it is, an up voice wakes the lamp to anyone talking, and an awake
  # lamp stands (#2818 on lamp-52e6, in a talking office; a factory is no
  # quieter). The keeper's pause (keeper.md) makes every mood rest and drops
  # calls, looks and Expression's requests, so the body folds once its awake
  # time runs out and holds there; it lives in /run, so the lamp's next
  # start is not paused.
  "$KEEPER_BIN" pause || { fail "could not pause the lamp with $KEEPER_BIN, so it would wake to the room and stand: the steps passed, but the arm is not left at rest"; return 1; }
  say "the lamp is paused until it is next started: it folds and stays at rest, taking no turn"
}

say "lamp $UNIT_ID, $MODE, from step $FROM"
NAMES=("" "the body's poses" "the touch pads" "the update key" "the self-test")
FUNCS=("" step_body step_touch step_key step_selftest)
for n in 1 2 3 4; do
  [[ $n -lt $FROM || $n -gt $TO ]] && continue
  say "step $n of 4, ${NAMES[$n]}"
  if ! "${FUNCS[$n]}" || ! listeners_back; then
    echo "factory: step $n (${NAMES[$n]}) failed; steps after it were not run. Fix it and go on with --from $n." >&2
    exit 1
  fi
  say "step $n done"
done
if [[ $FROM == 1 && $TO == 4 ]]; then say "all four steps done ($MODE)"; else say "steps $FROM to $TO done ($MODE)"; fi
