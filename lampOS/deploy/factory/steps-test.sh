#!/usr/bin/env bash
# Tests for steps.sh with no lamp: stand-ins for lamp-body-measure, lamp-body,
# lamp-body-mood, lamp-controls, lamp-selftest and systemctl, and a folder standing for /.
# Run: lampOS/deploy/factory/steps-test.sh
set -uo pipefail

STEPS=$(cd "$(dirname "$0")" && pwd)/steps.sh
T=$(mktemp -d)
# A stand-in body still running at the end is a failure the checks name; it
# is stopped here so that none outlives the test.
trap 'pkill -f "$T/bin/body" 2>/dev/null; rm -rf "$T"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; failures=$((failures + 1)); fi
}

# The stand-ins. FAKE_BODY_STATE: what systemctl is-active says of lamp-body. FAKE_MEASURE_FAIL:
# the read is refused. FAKE_NO_STAND: the arm never reaches its stand.
# FAKE_SELFTEST_EXIT: the self-test's exit. FAKE_HELD: the lamp is held,
# torque on, until a body run with --let-go-at-stop lets it go.
mkdir -p "$T/bin"
cat >"$T/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
echo "systemctl $*" >>"$FAKE_LOG"
[[ $1 == is-active ]] && { echo "${FAKE_BODY_STATE:-inactive}"; [[ ${FAKE_BODY_STATE:-inactive} == active ]]; exit; }
exit 0
EOF
cat >"$T/bin/measure" <<'EOF'
#!/usr/bin/env bash
echo "measure $*" >>"$FAKE_LOG"
[[ -n ${FAKE_MEASURE_FAIL:-} ]] && { echo "joint 2 could not be read" >&2; exit 1; }
[[ -n ${FAKE_HELD:-} && ! -e $FAKE_LOG.let-go ]] && { echo "lamp-body-measure: joint 1 has torque on: read the lamp at rest with torque off on all five" >&2; exit 1; }
text='BODY_POSES="--home 1774,2054,2033 --folded-arm 2049,2047 --stand 1772,2054,2033,2657,2602 --pitch-window 1740,2030 --torque on"'
comment="# home, folded-arm: found at rest. stand: lamp-4ace's with this lamp's roll and yaw. pitch-window: the default, checked."
echo "$text"
while [[ $# -gt 0 ]]; do [[ $1 == --write ]] && printf '%s\n%s\n' "$comment" "$text" >"$2"; shift; done
EOF
# The body: the lines lamp-body says, at its full stand at 100% (unless
# FAKE_NO_STAND), then held until SIGTERM eases it back.
cat >"$T/bin/body" <<'EOF'
#!/usr/bin/env bash
echo "body $*" >>"$FAKE_LOG"
let_go=no
[[ " $* " == *" --let-go-at-stop on "* ]] && let_go=yes
share=0
while [[ $# -gt 0 ]]; do [[ $1 == --stand-share ]] && share=$2; shift; done
echo "lamp-body: it may stand"
if [[ $share == 100 && -z ${FAKE_NO_STAND:-} ]]; then
  echo "lamp-body: step 781 Resting goals [1769, 2054, 2034] positions [1769, 2054, 2034] loads [0, 0, 0] stand 1.00 arm goals [2657, 2549] positions [2637, 2541] loads [92, 44] warmth [39, 38]"
else
  echo "lamp-body: step 781 Resting goals [1772, 2054, 2033] positions [1772, 2054, 2033] loads [0, 0, 0] stand 0.30 arm goals [2180, 2167] positions [2138, 2120] loads [180, 200] warmth [39, 38]"
fi
trap 'echo "body eased back" >>"$FAKE_LOG"; [[ $let_go == yes ]] && touch "$FAKE_LOG.let-go"; exit 0' TERM
while :; do sleep 0.1; done
EOF
cat >"$T/bin/mood" <<'EOF'
#!/usr/bin/env bash
echo "mood $*" >>"$FAKE_LOG"
EOF
# discover-head: FAKE_DISCOVERY (the report), FAKE_DISCOVERY_EXIT, and
# FAKE_DISCOVERY_SLEEP (seconds it takes). By default lamp-52e6's finding
# (#1362): 37 and 96 pulled up with nothing on them, 97 and 98 driven low at
# idle, each touched twice.
cat >"$T/bin/controls" <<'EOF'
#!/usr/bin/env bash
echo "controls $*" >>"$FAKE_LOG"
if [[ $1 == discover-head ]]; then
  [[ -n ${FAKE_DISCOVERY_SLEEP:-} ]] && sleep "$FAKE_DISCOVERY_SLEEP"
  line() { printf '{"line":%s,"high_with_pull_down":%s,"high_with_pull_up":true,"idle_high":%s,"completed_pulses":%s,"longest_pulse_us":%s,"active_low_candidate":%s}' "$@"; }
  if [[ -n ${FAKE_DISCOVERY:-} ]]; then
    echo "$FAKE_DISCOVERY"
  else
    echo "{\"lines\":[$(line 37 false null 0 0 null),$(line 96 false null 0 0 null),$(line 97 false false 2 3256546 false),$(line 98 false false 2 3256884 false)],\"observations\":5000,\"gaps\":0,\"started_us\":1,\"last_us\":2,\"hardware_mapping_installed\":false}"
  fi
  exit "${FAKE_DISCOVERY_EXIT:-0}"
fi
while [[ $# -gt 0 ]]; do [[ $1 == --output ]] && echo '{"calibrated":true}' >"$2"; shift; done
EOF
cat >"$T/bin/selftest" <<'EOF'
#!/usr/bin/env bash
echo "selftest $*" >>"$FAKE_LOG"
exit "${FAKE_SELFTEST_EXIT:-0}"
EOF
chmod +x "$T"/bin/*
KEYFILE=$T/key.pub
head -c 32 /dev/zero | base64 >"$KEYFILE"

# A fresh lamp: the image's GPIO pins (no MPR121), nothing measured, no key.
lamp() {
  rm -rf "$T/root" && mkdir -p "$T/root/etc/lampOS/controls" "$T/root/tmp"
  printf '{"chip":"/dev/gpiochip0","head_pads":[{"name":"head-1","line":37}]}\n' \
    >"$T/root/etc/lampOS/controls/controls-pins.json"
  : >"$T/log"
  rm -f "$T/log.let-go"
}
run() {
  ETC_ROOT=$T/root MEASURE_BIN=$T/bin/measure BODY_BIN=$T/bin/body MOOD_BIN=$T/bin/mood STAND_WAIT=0 \
    CONTROLS_BIN=$T/bin/controls SELFTEST_BIN=$T/bin/selftest SYSTEMCTL=$T/bin/systemctl \
    FAKE_LOG=$T/log bash "$STEPS" --unit-id lamp-test "$@" </dev/null >"$T/out" 2>&1
}
ENV=etc/lampOS/body/lamp.env

# Dry, all four: the body only read at rest, into /tmp; nothing in /etc.
lamp; run --update-key "$KEYFILE"; status=$?
check "a dry run passes all four" '[[ $status == 0 ]] && grep -q "all four steps done (dry)" "$T/out"'
check "a dry run writes no lamp.env" '[[ ! -e $T/root/$ENV ]]'
check "a dry run places no key" '[[ ! -e $T/root/etc/lampOS/update/key.pub ]]'
check "a dry run reads the body at rest only, into /tmp" 'grep -q "^measure --device /dev/ttyACM0 --unit-id lamp-test --lock-directory /run/lampos-locks/motor --write $T/root/tmp/" "$T/log" && ! grep -q "^body" "$T/log"'
check "a dry run never runs the self-test" '! grep -q "^selftest" "$T/log"'
check "GPIO pads, dry: says how it would find them and reads nothing" 'grep -q "would stop lamp-controls, run .*discover-head --seconds 30" "$T/out" && ! grep -q "^controls" "$T/log" && ! grep -q "systemctl stop" "$T/log"'

# Without a person to say yes, a written run moves nothing and writes nothing.
lamp; run --write --update-key "$KEYFILE"; status=$?
check "a written run without a yes stops before the arm moves" '[[ $status == 1 ]] && grep -q "step 1 (the body.s poses) failed" "$T/out" && ! grep -q "^body" "$T/log" && [[ ! -e $T/root/$ENV ]]'

# Written, with the person's yes: the arm stood at 30% then 100%, and the
# file holds where this arm stopped, not lamp-4ace's stand.
lamp; ASK_ANSWER=y run --write --update-key "$KEYFILE"; status=$?
check "a written run passes all four" '[[ $status == 0 ]] && grep -q "all four steps done (write)" "$T/out"'
check "the arm stands at 30% then 100%" '[[ $(grep -c "^body --device" "$T/log") == 2 ]] && grep "^body --device" "$T/log" | head -1 | grep -q -- "--stand-share 30" && grep "^body --device" "$T/log" | tail -1 | grep -q -- "--stand-share 100"'
check "on a private socket, with nothing else that moves on its own, and let go at the stop" 'grep "^body --device" "$T/log" | grep -q -- "--mood-socket $T/root/tmp/factory-stand" && [[ $(grep "^body --device" "$T/log" | grep -c -- "--force-limit 600 --wake off --rest-glance off --look-at-name off --let-go-at-stop on --stand-share .* --idle off") == 2 ]] && ! grep "^body" "$T/log" | grep -q "/run/lampos-body"'
check "each stand is told listening on its own socket, and eased back" '[[ $(grep -c "^mood --socket $T/root/tmp/factory-stand.*/mood.sock --mood listening --level 128$" "$T/log") == 2 && $(grep -c "^body eased back" "$T/log") == 2 ]]'
check "lamp.env holds this arm's stand" 'grep -q -- "--stand 1772,2054,2033,2637,2541 " "$T/root/$ENV" && grep -q "this arm.s, stood at 100%" "$T/root/$ENV"'
check "a written run places the key" 'cmp -s "$KEYFILE" "$T/root/etc/lampOS/update/key.pub"'
check "a written run runs the self-test after its yes" 'grep -q "^selftest --occasion factory" "$T/log"'
lamp; ASK_ANSWER=y FAKE_SELFTEST_EXIT=3 run --write --update-key "$KEYFILE"; status=$?
check "a part not checked (exit 3) counts as done" '[[ $status == 0 ]]'
lamp; ASK_ANSWER=y FAKE_SELFTEST_EXIT=1 run --write --update-key "$KEYFILE"; status=$?
check "a failed part fails step 4" '[[ $status == 1 ]] && grep -q "step 4 (the self-test) failed" "$T/out"'
lamp; ASK_ANSWER=y FAKE_NO_STAND=1 run --write --update-key "$KEYFILE"; status=$?
check "an arm that never reaches its stand writes nothing" '[[ $status == 1 ]] && grep -q "did not reach its full stand" "$T/out" && [[ ! -e $T/root/$ENV ]]'
lamp; ASK_ANSWER=n run --write --update-key "$KEYFILE"; status=$?
check "a no at the console moves nothing" '[[ $status == 1 ]] && ! grep -q "^body" "$T/log"'

# Going on from step 4 after the rest, with the steps before it untouched.
lamp; : >"$T/log"; ASK_ANSWER=y run --write --from 4 --update-key "$KEYFILE"; status=$?
check "--from 4 runs only step 4" '! grep -q "^measure" "$T/log" && grep -q "step 4 of 4" "$T/out" && [[ $status == 0 ]]'

# One step at a time, as a station with a person at it runs them.
lamp; : >"$T/log"; ASK_ANSWER=y run --write --from 2 --to 3 --update-key "$KEYFILE"; status=$?
check "--from 2 --to 3 runs only steps 2 and 3" '[[ $status == 0 ]] && grep -q "steps 2 to 3 done (write)" "$T/out" && ! grep -q "^measure\|^selftest" "$T/log" && grep -q "^controls discover-head" "$T/log"'
lamp; run --from 3 --to 2; status=$?
check "--to before --from is refused" '[[ $status == 2 ]]'

# A step that fails stops the rest and says which.
lamp; FAKE_BODY_STATE=activating run --write --update-key "$KEYFILE"; status=$?
check "a body restarting by itself (activating) stops step 1 before anything" '[[ $status == 1 ]] && grep -q "lamp-body is activating: stop it first" "$T/out" && ! grep -q "^measure\|^body" "$T/log"'
lamp; FAKE_BODY_STATE=active run --write --update-key "$KEYFILE"; status=$?
check "a running body stops step 1 and the rest" '[[ $status == 1 ]] && grep -q "step 1 (the body.s poses) failed" "$T/out" && ! grep -q "^measure" "$T/log" && [[ ! -e $T/root/etc/lampOS/update/key.pub ]]'
lamp; FAKE_MEASURE_FAIL=1 run --update-key "$KEYFILE"; status=$?
check "a refused read stops at step 1 with its reason" '[[ $status == 1 ]] && grep -q "could not be read" "$T/out" && grep -q "step 1 (the body.s poses) failed" "$T/out" && ! grep -q "^body" "$T/log"'

# A lamp held since a run of the body (lamp-cb94, 19 Oct 06:11Z): written,
# it is let go by a body run that only stops, then read; dry, it says how.
lamp; FAKE_HELD=1 ASK_ANSWER=y run --write --to 1; status=$?
check "a held lamp is let go by a run that only stops, then read and stood" '[[ $status == 0 ]] && grep -q "letting the lamp go" "$T/out" && [[ $(grep -c "^body --device" "$T/log") == 3 ]] && grep "^body --device" "$T/log" | head -1 | grep -q -- "--let-go-at-stop on" && ! grep "^body --device" "$T/log" | head -1 | grep -q -- "--stand-share" && [[ $(grep -c "^mood" "$T/log") == 2 ]] && [[ -e $T/root/$ENV ]]'
lamp; FAKE_HELD=1 run --to 1; status=$?
check "a held lamp, dry: says a written run lets it go first, and moves nothing" '[[ $status == 1 ]] && grep -q "a written run lets it go first" "$T/out" && ! grep -q "^body" "$T/log"'
lamp; FAKE_HELD=1 ASK_ANSWER=n run --write --to 1; status=$?
check "a held lamp with no yes is not let go" '[[ $status == 1 ]] && ! grep -q "^body" "$T/log"'
lamp; mkdir -p "$T/root/etc/lampOS/body"; echo old >"$T/root/$ENV"; ASK_ANSWER=y run --write --update-key "$KEYFILE"; status=$?
check "lamp.env already there is never measured over" '[[ $status == 1 ]] && grep -q "already there" "$T/out" && [[ $(cat "$T/root/$ENV") == old ]] && ! grep -q "^measure" "$T/log"'

# The key: a different one is never replaced; a bad one is refused.
lamp; mkdir -p "$T/root/etc/lampOS/update"; head -c 32 /dev/urandom | base64 >"$T/root/etc/lampOS/update/key.pub"
run --update-key "$KEYFILE"; status=$?
check "a different key in place is not replaced" '[[ $status == 1 ]] && grep -q "different key" "$T/out" && ! cmp -s "$KEYFILE" "$T/root/etc/lampOS/update/key.pub"'
lamp; echo not-a-key >"$T/bad.pub"; run --update-key "$T/bad.pub"; status=$?
check "a key that is not 32 bytes is refused" '[[ $status == 1 ]] && grep -q "not a 32-byte" "$T/out"'
lamp; run; status=$?
check "no key given and none in place fails step 3" '[[ $status == 1 ]] && grep -q "step 3 (the update key) failed" "$T/out"'

# An MPR121 lamp: dry, the touch step only says what it would do; written,
# it asks for a person, so it stops without one.
mpr121() {
  printf '{"chip":"/dev/gpiochip0","base":{"bus":0,"address":90,"pads":[0,1]}}\n' \
    >"$T/root/etc/lampOS/controls/controls-pins.json"
}
lamp; mpr121; run --update-key "$KEYFILE"; status=$?
check "an MPR121 lamp's dry touch step stops and runs nothing" '[[ $status == 0 ]] && grep -q "would stop lamp-controls, run" "$T/out" && ! grep -q "^controls" "$T/log" && ! grep -q "systemctl stop" "$T/log"'
lamp; mpr121; run --write --from 2 --update-key "$KEYFILE"; status=$?
check "an MPR121 lamp's written touch step needs a console" '[[ $status == 1 ]] && grep -q "step 2 (the touch pads) failed" "$T/out" && ! grep -q "^controls" "$T/log" && ! grep -q "systemctl stop" "$T/log"'
lamp; mpr121; ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "an MPR121 lamp's written touch step calibrates and starts controls again" '[[ $status == 0 ]] && grep -q "^controls calibrate-touch --pins $T/root/etc/lampOS/controls/controls-pins.json --output" "$T/log" && grep -q "systemctl start lamp-controls" "$T/log" && ls "$T/root/etc/lampOS/controls/" | grep -q "controls-pins.json.before-factory-"'

# GPIO head pads, written (the first builder on #1362, 12:54): the pair and
# polarity this unit showed go into its pins file, the rest of it kept.
PINSF=etc/lampOS/controls/controls-pins.json
lamp; ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "the head pads found are written with their polarity, the rest kept" '[[ $status == 0 ]] && [[ $(jq -c .head_pads "$T/root/$PINSF") == "[{\"name\":\"head-1\",\"line\":97,\"active_low\":false,\"pull\":\"down\"},{\"name\":\"head-2\",\"line\":98,\"active_low\":false,\"pull\":\"down\"}]" ]] && [[ $(jq -r .chip "$T/root/$PINSF") == /dev/gpiochip0 ]] && ls "$T/root/etc/lampOS/controls/" | grep -q "controls-pins.json.before-factory-"'
check "lamp-controls is stopped for the read, started again, then restarted on the new pins" '[[ $(grep "lamp-controls" "$T/log" | tr "\n" "|") == "systemctl stop lamp-controls|systemctl start lamp-controls|systemctl restart lamp-controls|" ]] && grep -q "^controls discover-head --seconds 30$" "$T/log"'
pulled() { printf '{"line":%s,"high_with_pull_down":%s,"high_with_pull_up":%s,"idle_high":%s,"completed_pulses":%s,"longest_pulse_us":1,"active_low_candidate":%s}' "$@"; }
lamp; before=$(cat "$T/root/$PINSF")
FAKE_DISCOVERY="{\"lines\":[$(pulled 37 true true true 2 true),$(pulled 96 true true true 1 null),$(pulled 97 false false false 0 null),$(pulled 98 false true null 0 null)],\"hardware_mapping_installed\":false}" \
  ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "one pad touched twice is not a pair: nothing written, controls started again" '[[ $status == 1 ]] && grep -q "head pads found on \[37\] lines, not two" "$T/out" && [[ $(cat "$T/root/$PINSF") == "$before" ]] && grep -q "systemctl start lamp-controls" "$T/log" && ! grep -q "systemctl restart" "$T/log"'
lamp; JQ=/nonexistent/jq ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "no jq: the head pads step says so, before stopping anything" '[[ $status == 1 ]] && grep -q "/nonexistent/jq is not installed" "$T/out" && ! grep -q "systemctl stop" "$T/log"'
lamp; FAKE_DISCOVERY_EXIT=1 ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "a read that fails writes nothing and starts controls again" '[[ $status == 1 ]] && grep -q "discover-head could not read" "$T/out" && [[ $(jq -c .head_pads "$T/root/$PINSF") == *"\"line\":37"* ]] && grep -q "systemctl start lamp-controls" "$T/log"'
lamp; set -m
ETC_ROOT=$T/root MEASURE_BIN=$T/bin/measure BODY_BIN=$T/bin/body MOOD_BIN=$T/bin/mood STAND_WAIT=0 \
  CONTROLS_BIN=$T/bin/controls SELFTEST_BIN=$T/bin/selftest SYSTEMCTL=$T/bin/systemctl FAKE_DISCOVERY_SLEEP=30 \
  FAKE_LOG=$T/log ASK_ANSWER=y bash "$STEPS" --unit-id lamp-test --write --from 2 --update-key "$KEYFILE" </dev/null >"$T/out" 2>&1 &
script=$!
set +m
for _ in $(seq 100); do grep -q "^controls discover-head" "$T/log" && break; sleep 0.1; done
kill -INT -- -"$script"; wait "$script"; status=$?
check "Ctrl-C while the pads are read starts lamp-controls again" '[[ $status == 130 ]] && grep -q "systemctl start lamp-controls" "$T/log" && [[ $(jq -c .head_pads "$T/root/$PINSF") == *"\"line\":37"* ]]'

# Ctrl-C mid-stand (the reviewer on #1744): the terminal's SIGINT reaches the
# script's whole process group; the body, a background job, ignores it, so
# the script must send it SIGTERM to ease the arm back. Job control puts the
# script in its own group, as a console would, so it starts with SIGINT not
# ignored.
lamp; set -m
ETC_ROOT=$T/root MEASURE_BIN=$T/bin/measure BODY_BIN=$T/bin/body MOOD_BIN=$T/bin/mood STAND_WAIT=30 \
  CONTROLS_BIN=$T/bin/controls SELFTEST_BIN=$T/bin/selftest SYSTEMCTL=$T/bin/systemctl \
  FAKE_LOG=$T/log ASK_ANSWER=y bash "$STEPS" --unit-id lamp-test --write --update-key "$KEYFILE" </dev/null >"$T/out" 2>&1 &
script=$!
set +m
for _ in $(seq 100); do grep -q "^mood" "$T/log" && break; sleep 0.1; done
started=$(date +%s)
kill -INT -- -"$script"
wait "$script"; status=$?
took=$(($(date +%s) - started))
check "Ctrl-C mid-stand eases the arm back at once and stops the steps" '[[ $status == 130 && $took -le 3 ]] && grep -q "^body eased back" "$T/log" && grep -q "the arm was eased back" "$T/out" && [[ $(grep -c "^body --device" "$T/log") == 1 ]] && [[ ! -e $T/root/$ENV ]] && [[ ! -e $T/root/etc/lampOS/update/key.pub ]]'

check "no stand-in body outlives its stand" '! pgrep -f "$T/bin/body" >/dev/null'
echo "$failures failed"
[[ $failures == 0 ]]
