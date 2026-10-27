#!/usr/bin/env bash
# Tests for steps.sh with no lamp: stand-ins for lamp-body-measure, lamp-body,
# lamp-body-mood, lamp-controls, lamp-selftest, systemctl and setpriv, and a folder standing for /.
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
[[ $1 == is-active && ($2 == lamp-voice || $2 == lamp-ears) ]] && { echo "${FAKE_VOICE_STATE:-inactive}"; [[ ${FAKE_VOICE_STATE:-inactive} == active ]]; exit; }
[[ $1 == is-active && $2 == lamp-expression ]] && { echo "${FAKE_EXPRESSION_STATE:-inactive}"; [[ ${FAKE_EXPRESSION_STATE:-inactive} == active ]]; exit; }
# "start lamp-body": active from then on, its mood socket opened unless
# FAKE_BODY_NO_SOCKET.
if [[ $1 == start && $2 == lamp-body ]]; then
  touch "$FAKE_LOG.body-started"
  [[ -n ${FAKE_BODY_NO_SOCKET:-} ]] && exit 0
  mkdir -p "$ETC_ROOT/run/lampos-body"
  python3 -c 'import socket, sys; socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM).bind(sys.argv[1])' "$ETC_ROOT/run/lampos-body/mood.sock"
  exit
fi
[[ $1 == is-active && -e $FAKE_LOG.body-started ]] && { echo active; exit 0; }
[[ $1 == is-active ]] && { echo "${FAKE_BODY_STATE:-inactive}"; [[ ${FAKE_BODY_STATE:-inactive} == active ]]; exit; }
# The target's parts (FAKE_PARTS), and their stop refused with FAKE_PARTS_STOP_FAIL.
[[ $1 == show && $3 == ConsistsOf ]] && { echo "${FAKE_PARTS-lamp-voice.service lamp-memory.service}"; exit 0; }
[[ $1 == stop && $2 == lampos.target && -n ${FAKE_PARTS_STOP_FAIL:-} ]] && exit 1
[[ $1 == show ]] && { echo "Environment=${FAKE_CONTROLS_ENV:-}"; exit 0; }
exit 0
EOF
cat >"$T/bin/measure" <<'EOF'
#!/usr/bin/env bash
echo "measure $*" >>"$FAKE_LOG"
[[ -n ${FAKE_MEASURE_FAIL:-} ]] && { echo "joint 2 could not be read" >&2; exit 1; }
[[ -n ${FAKE_HELD:-} && ! -e $FAKE_LOG.let-go ]] && { echo "lamp-body-measure: joint 1 has torque on: read the lamp at rest with torque off on all five" >&2; exit 1; }
# FAKE_TIPPED: the head rests tipped up off its stop until head-home brings it
# home; "stays", head-home does not; "under", tipped down instead (#3504).
if [[ $1 == head-home ]]; then
  echo "lamp-body-measure: stopped: reached 2020; peak load 8%"
  [[ ${FAKE_TIPPED:-} != stays ]] && touch "$FAKE_LOG.head-home"
  exit 0
fi
if [[ -n ${FAKE_TIPPED:-} && ! -e $FAKE_LOG.head-home ]]; then
  side=over; [[ $FAKE_TIPPED == under ]] && side=under
  echo "lamp-body-measure: home pitch 2213 is 193 $side the body's home range 1760..2020, more than 20: a head off its stop, not a rest reading." >&2
  exit 1
fi
text='BODY_POSES="--home 1774,2054,2033 --folded-arm 2049,2047 --stand 1772,2054,2033,2657,2602 --pitch-window 1740,2030 --torque on"'
comment="# home, folded-arm: found at rest. stand: lamp-4ace's with this lamp's roll and yaw. pitch-window: the default, checked."
echo "$text"
while [[ $# -gt 0 ]]; do [[ $1 == --write ]] && printf '%s\n%s\n' "$comment" "$text" >"$2"; shift; done
EOF
# The body: the lines lamp-body says, at its full stand at 100% (unless
# FAKE_NO_STAND), then held until SIGTERM eases it back, taking
# FAKE_FOLD_SECONDS to fold.
cat >"$T/bin/body" <<'EOF'
#!/usr/bin/env bash
echo "body $*" >>"$FAKE_LOG"
echo "body-pid $$" >>"$FAKE_LOG"
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
trap 'echo "body eased back" >>"$FAKE_LOG"; sleep "${FAKE_FOLD_SECONDS:-0}"; echo "body folded" >>"$FAKE_LOG"; [[ $let_go == yes ]] && touch "$FAKE_LOG.let-go"; exit 0' TERM
while :; do sleep 0.1; done
EOF
# setpriv (util-linux, not on macOS): its pid and flags, then it execs the
# command after --, as setpriv does.
cat >"$T/bin/setpriv" <<'EOF'
#!/usr/bin/env bash
echo "setpriv $$ $*" >>"$FAKE_LOG"
while [[ $# -gt 0 && $1 != -- ]]; do shift; done
shift
exec "$@"
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
  line() { printf '{"line":%s,"high_with_pull_down":%s,"high_with_pull_up":true,"idle_high":%s,"completed_pulses":%s,"longest_pulse_us":%s,"active_low_candidate":%s,"rest_changes":%s,"reads":%s}' "$@"; }
  if [[ -n ${FAKE_DISCOVERY:-} ]]; then
    echo "$FAKE_DISCOVERY"
  else
    echo "{\"lines\":[$(line 37 false null 0 0 null 0 null),$(line 96 false null 0 0 null 0 null),$(line 97 false false 2 3256546 false 0 '"level"'),$(line 98 false false 2 84000 false 0 '"pulses"')],\"observations\":5000,\"gaps\":0,\"started_us\":1,\"last_us\":2,\"hardware_mapping_installed\":false,\"rest_us\":10000000}"
  fi
  exit "${FAKE_DISCOVERY_EXIT:-0}"
fi
while [[ $# -gt 0 ]]; do [[ $1 == --output ]] && echo '{"calibrated":true}' >"$2"; shift; done
EOF
# The keeper: independent resume and pause refusals.
cat >"$T/bin/keeper" <<'EOF'
#!/usr/bin/env bash
echo "keeper $*" >>"$FAKE_LOG"
case $1 in
  resume) [[ -z ${FAKE_KEEPER_RESUME_FAIL:-} ]] ;;
  pause) [[ -z ${FAKE_KEEPER_FAIL:-} ]] ;;
esac
EOF
cat >"$T/bin/selftest" <<'EOF'
#!/usr/bin/env bash
echo "selftest $*" >>"$FAKE_LOG"
exit "${FAKE_SELFTEST_EXIT:-0}"
EOF
# systemd-run, as the reset's unit is started: logged, refused with FAKE_RESET_FAIL.
cat >"$T/bin/systemd-run" <<'EOF'
#!/usr/bin/env bash
echo "systemd-run $*" >>"$FAKE_LOG"
[[ -z ${FAKE_RESET_FAIL:-} ]]
EOF
chmod +x "$T"/bin/*
KEYFILE=$T/key.pub
head -c 32 /dev/zero | base64 >"$KEYFILE"

# A lamp set up, as step 4 needs: the image's GPIO pins (no MPR121), nothing
# measured, no key, and setup's marker. unset_lamp: the same without the marker,
# as the factory gets it.
lamp() {
  rm -rf "$T/root" && mkdir -p "$T/root/etc/lampOS/controls" "$T/root/etc/lampOS/setup" "$T/root/tmp" "$T/root/var/lib/lampos-setup"
  : >"$T/root/var/lib/lampos-setup/done"
  echo '{}' >"$T/root/etc/lampOS/setup/reset-plan.json"
  printf '{"chip":"/dev/gpiochip0","head_pads":[{"name":"head-1","line":37}]}\n' \
    >"$T/root/etc/lampOS/controls/controls-pins.json"
  : >"$T/log"
  rm -f "$T/log.let-go" "$T/log.body-started" "$T/log.head-home"
}
run() {
  ETC_ROOT=$T/root MEASURE_BIN=$T/bin/measure BODY_BIN=$T/bin/body MOOD_BIN=$T/bin/mood STAND_WAIT=0 HEAD_STAND_WAIT=0 QUIET_WAIT=0 \
    CONTROLS_BIN=$T/bin/controls SELFTEST_BIN=$T/bin/selftest KEEPER_BIN=${KEEPER:-$T/bin/keeper} SYSTEMCTL=$T/bin/systemctl \
    SYSTEMD_RUN=$T/bin/systemd-run SETUP_BIN=/opt/lampOS/setup/current/lamp-setup \
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
check "GPIO pads, dry: says how it would find them and reads nothing" 'grep -q "would stop lamp-controls, run .*discover-head --seconds 40 --rest-seconds 10" "$T/out" && ! grep -q "^controls" "$T/log" && ! grep -q "systemctl stop" "$T/log"'

# Without a person to say yes, a written run moves nothing and writes nothing.
lamp; run --write --update-key "$KEYFILE"; status=$?
check "a written run without a yes stops before the arm moves" '[[ $status == 1 ]] && grep -q "step 1 (the body.s poses) failed" "$T/out" && ! grep -q "^body" "$T/log" && [[ ! -e $T/root/$ENV ]]'

# Written, with the person's yes: the arm stood at 30% then 100%, and the
# file holds where this arm stopped, not lamp-4ace's stand.
lamp; ASK_ANSWER=y run --write --update-key "$KEYFILE"; status=$?
check "a written run passes all four" '[[ $status == 0 ]] && grep -q "all four steps done (write)" "$T/out"'
check "the arm stands at 30% then 100%" '[[ $(grep -c "^body --device" "$T/log") == 2 ]] && grep "^body --device" "$T/log" | head -1 | grep -q -- "--stand-share 30" && grep "^body --device" "$T/log" | tail -1 | grep -q -- "--stand-share 100"'
check "on a private socket, with nothing else that moves on its own, and let go at the stop" 'grep "^body --device" "$T/log" | grep -q -- "--mood-socket $T/root/tmp/factory-stand" && [[ $(grep "^body --device" "$T/log" | grep -c -- "--force-limit 600 --rest-glance off --look-at-name off --let-go-at-stop on --wake on --stand-share .* --idle off") == 2 ]] && ! grep "^body" "$T/log" | grep -q "/run/lampos-body"'
check "the stands wake a sagged head home first (lamp-a63e, #2396)" '[[ $(grep "^body --device" "$T/log" | grep -c -- "--wake on --stand-share") == 2 ]] && ! grep "^body --device" "$T/log" | grep -q -- "--wake off"'
check "each stand is told listening on its own socket, and eased back" '[[ $(grep -c "^mood --socket $T/root/tmp/factory-stand.*/mood.sock --mood listening --level 128$" "$T/log") == 2 && $(grep -c "^body eased back" "$T/log") == 2 ]]'
check "lamp.env holds this arm's stand" 'grep -q -- "--stand 1772,2054,2033,2637,2541 " "$T/root/$ENV" && grep -q "this arm.s, stood at 100%" "$T/root/$ENV"'
check "a written run places the key" 'cmp -s "$KEYFILE" "$T/root/etc/lampOS/update/key.pub"'
check "a written run runs the self-test after its yes" 'grep -q "^selftest --occasion factory" "$T/log"'
# Step 1 left lamp-body stopped: a whole run starts it before step 4 and
# stands it, so head_move is checked (#2818).
check "a whole run starts lamp-body after step 1 and stands it for the self-test" '[[ $(grep "^measure\|systemctl start lamp-body\|^mood --socket $T/root/run/lampos-body\|^selftest" "$T/log" | sed "s/ --.*//" | uniq | tr "\n" "|") == "measure|systemctl start lamp-body|mood|selftest|" ]]'
lamp; ASK_ANSWER=y FAKE_BODY_NO_SOCKET=1 BODY_START_WAIT=1 run --write --update-key "$KEYFILE"; status=$?
check "a lamp-body that does not come up fails step 4 and says why" '[[ $status == 1 ]] && grep -q "lamp-body did not come up with .*mood.sock within 1 s: fix it" "$T/out" && grep -q "step 4 (the self-test) failed" "$T/out" && ! grep -q "^selftest" "$T/log"'
unset_lamp() { lamp; rm -f "$T/root/var/lib/lampos-setup/done"; }
# A lamp not set up (the V2 image's, lamp-52e6 on 2026-10-26): its parts wait
# for setup's marker, so step 4 refuses before it starts anything; steps 1 to 3
# pass, and a dry run says what a written one would do.
unset_lamp; run --update-key "$KEYFILE"; status=$?
check "unset, dry: all four pass, and step 4 says it waits for setup" '[[ $status == 0 ]] && grep -q "the lamp is not set up: a written step 4 refuses until it is" "$T/out" && ! grep -q "^selftest" "$T/log"'
unset_lamp; ASK_ANSWER=y run --write --update-key "$KEYFILE"; status=$?
check "unset, written: steps 1 to 3 done, step 4 refuses and says to set the lamp up" '[[ $status == 1 ]] && grep -q "step 3 done" "$T/out" && grep -q "the lamp is not set up, so its parts are not running and the self-test would check nothing: set it up from the app, run this again with --from 4" "$T/out" && grep -q "step 4 (the self-test) failed" "$T/out" && ! grep -q "^selftest" "$T/log" && ! grep -q "systemctl start lamp-body" "$T/log"'
unset_lamp; ASK_ANSWER=y run --write --update-key "$KEYFILE" --to 3; status=$?
check "unset, written --to 3: steps 1 to 3 done" '[[ $status == 0 ]] && grep -q "steps 1 to 3 done (write)" "$T/out"'
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

# The self-test's head_move needs the lamp standing (#2232): the running body
# is told listening on its own socket before the self-test, after the yes.
lamp; FAKE_BODY_STATE=active ASK_ANSWER=y run --write --from 4; status=$?
check "step 4 stands the running body up before the self-test" '[[ $status == 0 ]] && [[ $(grep -c "^mood --socket $T/root/run/lampos-body/mood.sock --mood listening --level 128$" "$T/log") == 5 ]] && [[ $(grep "^mood\|^selftest" "$T/log" | tail -1) == "selftest --occasion factory" ]] && grep -q "folds by itself" "$T/out"'
lamp; FAKE_BODY_STATE=active FAKE_EXPRESSION_STATE=active ASK_ANSWER=y run --write --from 4; status=$?
check "step 4 stops lamp-expression before the stand and starts it after the self-test" '[[ $status == 0 ]] && [[ $(grep "lamp-expression\|^mood\|^selftest" "$T/log" | grep -v is-active | sed "s/ --.*//" | uniq | tr "\n" "|") == "systemctl stop lamp-expression|mood|selftest|systemctl start lamp-expression|" ]]'
lamp; FAKE_BODY_STATE=active FAKE_EXPRESSION_STATE=active FAKE_SELFTEST_EXIT=1 ASK_ANSWER=y run --write --from 4; status=$?
check "a failed self-test still starts lamp-expression again" '[[ $status == 1 ]] && [[ $(grep -c "systemctl start lamp-expression" "$T/log") == 1 ]]'
lamp; FAKE_BODY_STATE=inactive ASK_ANSWER=y run --write --from 4; status=$?
check "--from 4 with lamp-body down: it is started and stood, so head_move is checked (#2818)" '[[ $status == 0 ]] && grep -q "lamp-body is inactive: starting it" "$T/out" && [[ $(grep "systemctl start lamp-body\|^mood --socket $T/root/run/lampos-body\|^selftest" "$T/log" | sed "s/ --.*//" | uniq | tr "\n" "|") == "systemctl start lamp-body|mood|selftest|" ]]'
# Reviewer-three's P2 on 547d568f0: a passed step 4 leaves the lamp set up as
# the bench left it, so it asks for the reset and runs it as the button does,
# and says to remove the lamp from the factory account and pack.
lamp; FAKE_BODY_STATE=active FAKE_VOICE_STATE=active ASK_ANSWER=y run --write --from 4; status=$?
check "a passed self-test runs the reset, an earlier pause cleared before the stand" '[[ $status == 0 ]] && [[ $(grep "^keeper\|^mood\|^selftest\|^systemd-run" "$T/log" | sed "s/ --.*//" | uniq | tr "\n" "|") == "keeper resume|mood|selftest|systemd-run|" ]] && grep -q "^systemd-run --no-block --collect --unit=lamp-setup-wipe --property=Type=oneshot /opt/lampOS/setup/current/lamp-setup reset-now --plan $T/root/etc/lampOS/setup/reset-plan.json --enable$" "$T/log" && grep -q "Then remove it from the factory account in the app, and pack it." "$T/out" && ! grep -q "^keeper pause" "$T/log"'
# lamp-29e7 on 26 Oct: the reset's own stop returns before the parts have
# stopped, and Voice's onboarding written on its stop outlived the wipe. A
# passed step 4 stops the path unit, then the target and each part by name,
# and only then starts the reset.
lamp; FAKE_BODY_STATE=active ASK_ANSWER=y run --write --from 4; status=$?
check "the lamp is stopped, each part named, before the reset starts (lamp-29e7)" '[[ $status == 0 ]] && [[ $(grep "^systemctl stop lampos\|^systemd-run" "$T/log" | sed "s/ --.*//" | tr "\n" "|") == "systemctl stop lampos-setup-done.path|systemctl stop lampos.target lamp-voice.service lamp-memory.service|systemd-run|" ]] && grep -q "every part finished: lamp-voice.service lamp-memory.service" "$T/out"'
lamp; FAKE_BODY_STATE=active FAKE_PARTS_STOP_FAIL=1 ASK_ANSWER=y run --write --from 4; status=$?
check "a lamp that cannot be stopped is not reset, and step 4 fails" '[[ $status == 1 ]] && ! grep -q "^systemd-run" "$T/log" && grep -q "the lamp could not be stopped before the reset" "$T/out" && grep -q "step 4 (the self-test) failed" "$T/out"'
lamp; FAKE_BODY_STATE=active FAKE_PARTS= ASK_ANSWER=y run --write --from 4; status=$?
check "a target with no parts listed: the target is stopped, then the reset" '[[ $status == 0 ]] && grep -q "^systemctl stop lampos.target$" "$T/log" && grep -q "^systemd-run" "$T/log"'
lamp; KEEPER=$T/bin/no-keeper FAKE_BODY_STATE=active ASK_ANSWER=y run --write --from 4; status=$?
check "the product image has no keeper (lamp-52e6): a passed self-test still resets" '[[ $status == 0 ]] && grep -q "^systemd-run .* reset-now " "$T/log" && ! grep -q "^keeper" "$T/log"'
lamp; FAKE_BODY_STATE=active ASK_ANSWER=y ASK_RESET_ANSWER=n run --write --from 4; status=$?
check "the reset refused: the lamp is paused, and step 4 fails as not ready to pack" '[[ $status == 1 ]] && grep -q "^keeper pause" "$T/log" && ! grep -q "^systemd-run" "$T/log" && grep -q "the lamp is NOT READY TO PACK: .*Hold the button 5 s (the reset), then remove the lamp from the factory account in the app, then pack" "$T/out" && grep -q "step 4 (the self-test) failed" "$T/out"'
lamp; KEEPER=$T/bin/no-keeper FAKE_BODY_STATE=active ASK_ANSWER=y ASK_RESET_ANSWER=n run --write --from 4; status=$?
check "the reset refused with no keeper: not ready to pack, nothing paused" '[[ $status == 1 ]] && ! grep -q "^keeper" "$T/log" && grep -q "NOT READY TO PACK" "$T/out"'
lamp; FAKE_BODY_STATE=active FAKE_RESET_FAIL=1 ASK_ANSWER=y run --write --from 4; status=$?
check "a reset that cannot start fails step 4 and says to hold the button" '[[ $status == 1 ]] && grep -q "could not start the reset: hold the button 5 s" "$T/out"'
lamp; rm -f "$T/root/etc/lampOS/setup/reset-plan.json"; FAKE_BODY_STATE=active ASK_ANSWER=y run --write --from 4; status=$?
check "no reset plan: step 4 fails and says to hold the button" '[[ $status == 1 ]] && grep -q "there is no reset plan at" "$T/out" && ! grep -q "^systemd-run" "$T/log"'
lamp; FAKE_BODY_STATE=active FAKE_SELFTEST_EXIT=1 ASK_ANSWER=y run --write --from 4; status=$?
check "a failed self-test does not pause or reset the lamp" '[[ $status == 1 ]] && ! grep -q "^keeper pause\|^systemd-run" "$T/log"'
lamp; FAKE_BODY_STATE=active FAKE_KEEPER_FAIL=1 ASK_ANSWER=y ASK_RESET_ANSWER=n run --write --from 4; status=$?
check "a pause the keeper cannot say fails step 4 and says the arm is not left at rest" '[[ $status == 1 ]] && grep -q "could not pause the lamp with .*the arm is not left at rest" "$T/out" && grep -q "step 4 (the self-test) failed" "$T/out"'
for body_state in active inactive; do
  lamp; FAKE_BODY_STATE=$body_state FAKE_EXPRESSION_STATE=active FAKE_KEEPER_RESUME_FAIL=1 FAKE_SELFTEST_EXIT=3 ASK_ANSWER=y run --write --from 4; status=$?
  check "a refused resume stops step 4 before body, expression, mood or self-test ($body_state)" '[[ $status == 1 ]] && grep -q "^keeper resume$" "$T/log" && grep -q "could not resume the lamp with .*before the head check" "$T/out" && grep -q "step 4 (the self-test) failed" "$T/out" && ! grep -q "systemctl start lamp-body\|systemctl stop lamp-expression\|^mood\|^selftest\|^keeper pause" "$T/log"'
done
lamp; FAKE_BODY_STATE=active ASK_ANSWER=n run --write --from 4; status=$?
check "step 4 without a yes stands nothing" '[[ $status == 1 ]] && ! grep -q "^mood\|^selftest" "$T/log"'
lamp; FAKE_BODY_STATE=active run --from 4; status=$?
check "step 4 dry stands nothing and pauses nothing" '[[ $status == 0 ]] && ! grep -q "^mood\|^selftest\|^keeper" "$T/log"'

# One step at a time, as a station with a person at it runs them.
lamp; : >"$T/log"; ASK_ANSWER=y run --write --from 2 --to 3 --update-key "$KEYFILE"; status=$?
check "--from 2 --to 3 runs only steps 2 and 3" '[[ $status == 0 ]] && grep -q "steps 2 to 3 done (write)" "$T/out" && ! grep -q "^measure\|^selftest" "$T/log" && grep -q "^controls discover-head" "$T/log"'
# A listening voice that loses the privacy switch keeps the restart guard's
# mute (#2818): those of the voice and ears that run are stopped before
# lamp-controls is, and started again only once Controls is back (after its
# restart on the new pins) and before the self-test; none running, none.
line() { grep -n "$1" "$T/log" | "${2:-head}" -1 | cut -d: -f1; }
# An empty line number would compare as 0, so a missing match fails here
# instead of passing an ordering check.
before() { [[ -n $1 && -n $2 ]] && (( $1 < $2 )); }
lamp; FAKE_VOICE_STATE=active ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "the voice and ears are stopped before Controls is, started after Controls is back, before the self-test" '[[ $status == 0 ]] && before "$(line "^systemctl stop lamp-voice lamp-ears$")" "$(line "^systemctl stop lamp-controls$")" && before "$(line "lamp-controls$" tail)" "$(line "^systemctl start lamp-voice lamp-ears$")" && before "$(line "^systemctl start lamp-voice")" "$(line "^selftest")" && [[ $(grep -c "^systemctl \(start\|stop\) lamp-voice" "$T/log") == 2 ]] && ! grep -q "^systemctl restart lamp-voice" "$T/log"'
lamp; ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
# The voice as the first unit named: the reset's stop names it among the
# target's parts.
check "a voice and ears not running are neither stopped nor started" '[[ $status == 0 ]] && ! grep -q "^systemctl \(stop\|start\|restart\) lamp-\(voice\|ears\)" "$T/log"'
lamp; FAKE_VOICE_STATE=active FAKE_DISCOVERY_EXIT=1 ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "a failed touch step still starts the voice and ears again, after Controls" '[[ $status == 1 ]] && before "$(line "^systemctl start lamp-controls$" tail)" "$(line "^systemctl start lamp-voice lamp-ears$")"'
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
# A head tipped up off its rest by hand (#3504): written, with a yes, brought
# home once by head-home and read again; the second refusal stands; dry, or
# a head tipped down, nothing moves.
lamp; FAKE_TIPPED=1 ASK_ANSWER=y run --write --to 1; status=$?
check "a head tipped up is brought home once, then read and stood" '[[ $status == 0 ]] && grep -q "bringing the head home slowly, keep hands off" "$T/out" && [[ $(grep "^measure" "$T/log" | cut -d" " -f2 | tr "\n" " ") == "--device head-home --device " ]] && [[ -e $T/root/$ENV ]]'
lamp; FAKE_TIPPED=stays ASK_ANSWER=y run --write --to 1; status=$?
check "a head still tipped up after head-home stops step 1, nothing written" '[[ $status == 1 ]] && [[ $(grep -c "^measure head-home" "$T/log") == 1 ]] && [[ $(grep -c "^measure --device" "$T/log") == 2 ]] && ! grep -q "^body" "$T/log" && [[ ! -e $T/root/$ENV ]]'
lamp; FAKE_TIPPED=1 run --to 1; status=$?
check "a head tipped up, dry: says a written run brings it home, and moves nothing" '[[ $status == 1 ]] && grep -q "a written run brings it home slowly under a torque cap first" "$T/out" && ! grep -q "^measure head-home" "$T/log"'
lamp; FAKE_TIPPED=1 ASK_ANSWER=n run --write --to 1; status=$?
check "a head tipped up with no yes is not moved" '[[ $status == 1 ]] && ! grep -q "^measure head-home" "$T/log"'
lamp; FAKE_TIPPED=under ASK_ANSWER=y run --write --to 1; status=$?
check "a head tipped down is never moved" '[[ $status == 1 ]] && ! grep -q "^measure head-home" "$T/log" && [[ $(grep -c "^measure" "$T/log") == 1 ]]'
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

# Store key provisioning is optional, separate, dry by default, and never
# enables Store or starts a service. Existing key material is not replaced.
STOREKEY=$T/store-key.pub
head -c 32 /dev/urandom | base64 >"$STOREKEY"
STOREPUB=$T/root/etc/lampOS/skills/store-key.pub
lamp; run --from 3 --to 3 --update-key "$KEYFILE" --store-key "$STOREKEY"; status=$?
check "Store key dry run writes neither public key" '[[ $status == 0 && ! -e $STOREPUB && ! -e $T/root/etc/lampOS/update/key.pub ]]'
lamp; run --write --from 3 --to 3 --update-key "$KEYFILE"; status=$?
check "Store key is optional and never borrowed from update" '[[ $status == 0 && ! -e $STOREPUB ]] && grep -q "Store stays off" "$T/out"'
run --write --from 3 --to 3 --store-key "$STOREKEY"; status=$?
check "Store key goes to its own path and changes no switch or service" '[[ $status == 0 ]] && cmp -s "$STOREKEY" "$STOREPUB" && cmp -s "$KEYFILE" "$T/root/etc/lampOS/update/key.pub" && [[ ! -e $T/root/etc/lampOS/skills/store.env && ! -s $T/log ]]'
check "Store public key is readable, never writable by group or others" '[[ $(ls -l "$STOREPUB" | cut -c1-10) == -rw-r--r-- ]]'
run --write --from 3 --to 3 --store-key "$STOREKEY"; status=$?
check "the same Store key is kept on repeat" '[[ $status == 0 ]] && cmp -s "$STOREKEY" "$STOREPUB"'
run --write --from 3 --to 3 --store-key "$KEYFILE"; status=$?
check "a different Store key is refused, original kept" '[[ $status == 1 ]] && grep -q "different key" "$T/out" && cmp -s "$STOREKEY" "$STOREPUB"'
lamp; run --write --from 3 --to 3 --update-key "$KEYFILE" --store-key "$T/bad.pub"; status=$?
check "a malformed Store key is refused without writing it" '[[ $status == 1 && ! -e $STOREPUB ]] && grep -q "not a 32-byte" "$T/out"'
lamp; cp "$STOREKEY" "$T/invalid-base64.pub"; printf '!' >>"$T/invalid-base64.pub"
run --write --from 3 --to 3 --update-key "$KEYFILE" --store-key "$T/invalid-base64.pub"; status=$?
check "32 decoded bytes followed by invalid base64 are refused" '[[ $status == 1 && ! -e $STOREPUB ]]'
lamp; fold -w 22 "$STOREKEY" >"$T/wrapped.pub"
run --write --from 3 --to 3 --update-key "$KEYFILE" --store-key "$T/wrapped.pub"; status=$?
check "wrapped base64 rejected by Rust is also refused at the factory" '[[ $status == 1 && ! -e $STOREPUB ]]'
lamp; run --from 3 --to 3 --store-key; status=$?
check "a missing Store key argument exits with usage instead of looping" '[[ $status == 2 ]] && grep -q "needs FILE" "$T/out"'
lamp; run --from 3 --to 3 --store-key --write; status=$?
check "an option cannot stand in for the Store key file" '[[ $status == 2 ]]'
lamp; mkdir -p "$(dirname "$STOREPUB")"; ln -s "$KEYFILE" "$STOREPUB"
run --write --from 3 --to 3 --update-key "$KEYFILE" --store-key "$STOREKEY"; status=$?
check "Store key destination links are not followed" '[[ $status == 1 && -L $STOREPUB ]] && cmp -s "$KEYFILE" "$STOREPUB"'

# An MPR121 lamp: dry, the touch step only says what it would do; written,
# it asks for a person, so it stops without one.
mpr121() {
  printf '{"chip":"/dev/gpiochip0","base":{"bus":0,"address":90,"pads":[0,1]}}\n' \
    >"$T/root/etc/lampOS/controls/controls-pins.json"
}
lamp; mpr121; FAKE_CONTROLS_ENV="LAMP_BUS=on LAMP_CONTROLS_STRIP=on" run --update-key "$KEYFILE"; status=$?
check "an MPR121 lamp's dry touch step stops and runs nothing" '[[ $status == 0 ]] && grep -q "would stop lamp-controls, run" "$T/out" && ! grep -q "^controls" "$T/log" && ! grep -q "systemctl stop" "$T/log"'
lamp; mpr121; FAKE_CONTROLS_ENV="LAMP_CONTROLS_STRIP=on" run --write --from 2 --update-key "$KEYFILE"; status=$?
check "an MPR121 lamp's written touch step needs a console" '[[ $status == 1 ]] && grep -q "step 2 (the touch pads) failed" "$T/out" && ! grep -q "^controls" "$T/log" && ! grep -q "systemctl stop" "$T/log"'
lamp; mpr121; FAKE_CONTROLS_ENV="LAMP_CONTROLS_STRIP=on" ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "an MPR121 lamp's written touch step calibrates and starts controls again" '[[ $status == 0 ]] && grep -q "^controls calibrate-touch --pins $T/root/etc/lampOS/controls/controls-pins.json --output" "$T/log" && grep -q "systemctl start lamp-controls" "$T/log" && ls "$T/root/etc/lampOS/controls/" | grep -q "controls-pins.json.before-factory-"'

# An MPR121 lamp with the strip off (#1891): no calibration, the head pads
# found instead; the switch named anywhere but on its own is not on.
lamp; mpr121; ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "the strip off: no calibration, the head pads are found instead" '[[ $status == 0 ]] && grep -q "the base strip is off" "$T/out" && ! grep -q "^controls calibrate-touch" "$T/log" && grep -q "^controls discover-head" "$T/log" && [[ $(jq -c "[.head_pads[].line]" "$T/root/etc/lampOS/controls/controls-pins.json") == "[97,98]" ]]'
lamp; mpr121; FAKE_CONTROLS_ENV="LAMP_CONTROLS_STRIP=off" run --update-key "$KEYFILE"; status=$?
check "LAMP_CONTROLS_STRIP=off is off" 'grep -q "the base strip is off" "$T/out" && ! grep -q "calibrate-touch --pins" "$T/out"'

# GPIO head pads, written (the first builder on #1362, 12:54): the pair and
# polarity this unit showed go into its pins file, the rest of it kept.
PINSF=etc/lampOS/controls/controls-pins.json
lamp; ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "the head pads found are written with their polarity and baseline, the rest kept" '[[ $status == 0 ]] && [[ $(jq -c .head_pads "$T/root/$PINSF") == "[{\"name\":\"head-1\",\"line\":97,\"active_low\":false,\"pull\":\"down\"},{\"name\":\"head-2\",\"line\":98,\"active_low\":false,\"pull\":\"down\"}]" ]] && [[ $(jq -c .head_rest "$T/root/$PINSF") == "[{\"name\":\"head-1\",\"rest\":\"low\",\"reads\":\"level\"},{\"name\":\"head-2\",\"rest\":\"low\",\"reads\":\"pulses\"}]" ]] && [[ $(jq -r .chip "$T/root/$PINSF") == /dev/gpiochip0 ]] && ls "$T/root/etc/lampOS/controls/" | grep -q "controls-pins.json.before-factory-"'
check "each pad is named as it was found (thirteen-P4)" 'grep -q "a head pad on line 97: rests low, level under a hand" "$T/out" && grep -q "a head pad on line 98: rests low, pulses under a hand" "$T/out"'
check "lamp-controls is stopped for the read, started again, then restarted on the new pins" '[[ $(grep "lamp-controls" "$T/log" | tr "\n" "|") == "systemctl stop lamp-controls|systemctl start lamp-controls|systemctl restart lamp-controls|" ]] && grep -q "^controls discover-head --seconds 40 --rest-seconds 10$" "$T/log"'
# HEAD_LINES_FROM (#2818): the head lines from a test input, passed through.
# Its pads never reach the live pins (#2915's P2): they go to a side file.
lamp; before=$(cat "$T/root/$PINSF")
HEAD_LINES_FROM=$T/lines ASK_ANSWER=y run --write --from 2 --to 2 --update-key "$KEYFILE"; status=$?
check "HEAD_LINES_FROM passes discover-head --lines-from" '[[ $status == 0 ]] && grep -q "^controls discover-head --seconds 40 --rest-seconds 10 --lines-from $T/lines$" "$T/log"'
check "HEAD_LINES_FROM leaves the live pins byte-identical and writes the found lines beside them" '[[ $(cat "$T/root/$PINSF") == "$before" ]] && [[ $(jq -c "[.head_pads[].line]" "$T/root/$PINSF.test-input") == "[97,98]" ]] && grep -q "test input: live pins not written, found lines in $T/root/$PINSF.test-input" "$T/out" && ! ls "$T/root/etc/lampOS/controls/" | grep -q "before-factory-" && ! grep -q "restart lamp-controls" "$T/log"'
pulled() { printf '{"line":%s,"high_with_pull_down":%s,"high_with_pull_up":%s,"idle_high":%s,"completed_pulses":%s,"longest_pulse_us":1,"active_low_candidate":%s}' "$@"; }
lamp; before=$(cat "$T/root/$PINSF")
FAKE_DISCOVERY="{\"lines\":[$(pulled 37 true true true 2 true),$(pulled 96 true true true 1 null),$(pulled 97 false false false 0 null),$(pulled 98 false true null 0 null)],\"hardware_mapping_installed\":false}" \
  ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "one pad touched twice is not a pair: nothing written, controls started again" '[[ $status == 1 ]] && grep -q "head pads found on \[37\] lines, not two or more" "$T/out" && [[ $(cat "$T/root/$PINSF") == "$before" ]] && grep -q "systemctl start lamp-controls" "$T/log" && ! grep -q "systemctl restart" "$T/log"'
# A pad whose line changed with hands away (lamp-cb94's 97, #1764) is
# refused; a report from an older discover-head writes no baseline.
lamp; before=$(cat "$T/root/$PINSF")
FAKE_DISCOVERY="{\"lines\":[$(pulled 97 false false false 2 false | sed 's/}$/,"rest_changes":3,"reads":"level"}/'),$(pulled 98 false false false 2 false | sed 's/}$/,"rest_changes":0,"reads":"pulses"}/')],\"hardware_mapping_installed\":false}" \
  ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "a pad whose rest changed with hands away: nothing written, controls started again" '[[ $status == 1 ]] && grep -q "head pad lines \[97\] changed with hands away" "$T/out" && [[ $(cat "$T/root/$PINSF") == "$before" ]] && grep -q "systemctl start lamp-controls" "$T/log" && ! grep -q "systemctl restart" "$T/log"'
lamp; before=$(cat "$T/root/$PINSF")
FAKE_DISCOVERY="{\"lines\":[$(pulled 97 false false false 2 false | sed 's/}$/,"rest_changes":0,"reads":null}/'),$(pulled 98 false false false 2 false | sed 's/}$/,"rest_changes":0,"reads":"pulses"}/')],\"hardware_mapping_installed\":false}" \
  ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "a pad touched too quickly to tell how it reads: nothing written, asked again" '[[ $status == 1 ]] && grep -q "head pad lines \[97\] were neither held half a second nor pulsing" "$T/out" && [[ $(cat "$T/root/$PINSF") == "$before" ]] && ! grep -q "systemctl restart" "$T/log"'
lamp
FAKE_DISCOVERY="{\"lines\":[$(pulled 97 false false false 2 false),$(pulled 98 false false false 2 false)],\"hardware_mapping_installed\":false}" \
  ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "an older report without the rest: the pads are written, no baseline" '[[ $status == 0 ]] && [[ $(jq -c "[.head_pads[].line]" "$T/root/$PINSF") == "[97,98]" ]] && [[ $(jq -c .head_rest "$T/root/$PINSF") == null ]] && grep -q "rest: \"not measured\"" "$T/out"'
# lamp-dbda's three (#2182, #2208 item 4): each is written, as the hardware
# team's test passes at two or more.
lamp
FAKE_DISCOVERY="{\"lines\":[$(pulled 37 false false false 3 false | sed 's/}$/,"rest_changes":0,"reads":"level"}/'),$(pulled 96 false false false 2 false | sed 's/}$/,"rest_changes":0,"reads":"level"}/'),$(pulled 97 false false false 2 false | sed 's/}$/,"rest_changes":0,"reads":"pulses"}/'),$(pulled 98 false true null 0 null)],\"hardware_mapping_installed\":false}" \
  ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "three pads found: all three written, each with its baseline" '[[ $status == 0 ]] && [[ $(jq -c "[.head_pads[] | [.name, .line]]" "$T/root/$PINSF") == "[[\"head-1\",37],[\"head-2\",96],[\"head-3\",97]]" ]] && [[ $(jq -c "[.head_rest[] | [.name, .reads]]" "$T/root/$PINSF") == "[[\"head-1\",\"level\"],[\"head-2\",\"level\"],[\"head-3\",\"pulses\"]]" ]] && grep -q "systemctl restart lamp-controls" "$T/log"'
# All four lines with a pad on each: all four written.
lamp
FAKE_DISCOVERY="{\"lines\":[$(pulled 37 false false false 2 false | sed 's/}$/,"rest_changes":0,"reads":"level"}/'),$(pulled 96 false false false 2 false | sed 's/}$/,"rest_changes":0,"reads":"level"}/'),$(pulled 97 false false false 2 false | sed 's/}$/,"rest_changes":0,"reads":"level"}/'),$(pulled 98 false false false 2 false | sed 's/}$/,"rest_changes":0,"reads":"level"}/')],\"hardware_mapping_installed\":false}" \
  ASK_ANSWER=y run --write --from 2 --update-key "$KEYFILE"; status=$?
check "four pads found: all four written" '[[ $status == 0 ]] && [[ $(jq -c "[.head_pads[] | [.name, .line]]" "$T/root/$PINSF") == "[[\"head-1\",37],[\"head-2\",96],[\"head-3\",97],[\"head-4\",98]]" ]] && [[ $(jq -c "[.head_rest[].name]" "$T/root/$PINSF") == "[\"head-1\",\"head-2\",\"head-3\",\"head-4\"]" ]]'
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

# A stop mid-stand, with the body run as its user as on the lamp (#2541's
# P2): setpriv execs the body, so the SIGTERM reaches the body itself, and the
# body is given its fold (3 s here) to end by itself, not killed at 2 s as
# runuser kills its child.
lamp; set -m
ETC_ROOT=$T/root MEASURE_BIN=$T/bin/measure BODY_BIN=$T/bin/body MOOD_BIN=$T/bin/mood STAND_WAIT=30 \
  CONTROLS_BIN=$T/bin/controls SELFTEST_BIN=$T/bin/selftest SYSTEMCTL=$T/bin/systemctl \
  SETPRIV=$T/bin/setpriv AS_BODY_USER=1 FAKE_FOLD_SECONDS=3 \
  FAKE_LOG=$T/log ASK_ANSWER=y bash "$STEPS" --unit-id lamp-test --body-user "$(id -un)" --write --update-key "$KEYFILE" </dev/null >"$T/out" 2>&1 &
script=$!
set +m
for _ in $(seq 100); do grep -q "^mood" "$T/log" && break; sleep 0.1; done
started=$(date +%s)
kill -INT -- -"$script"
wait "$script"; status=$?
took=$(($(date +%s) - started))
check "the stand's body runs under setpriv as its user, the body itself the process started" 'grep -q "^setpriv [0-9]* --reuid=$(id -un) --regid=$(id -g) --init-groups -- $T/bin/body --device" "$T/log" && [[ $(grep "^setpriv .* $T/bin/body " "$T/log" | cut -d" " -f2) == $(grep "^body-pid" "$T/log" | cut -d" " -f2) ]]'
check "a stop waits for the body to fold and end by itself, not killed at 2 s" '[[ $status == 130 && $took -ge 3 && $took -le 6 ]] && grep -q "^body folded" "$T/log" && ! grep -q "WARNING" "$T/out" && grep -q "the arm was eased back" "$T/out"'
# A body that does not end within STOP_WAIT is killed, and said so loudly.
lamp; set -m
ETC_ROOT=$T/root MEASURE_BIN=$T/bin/measure BODY_BIN=$T/bin/body MOOD_BIN=$T/bin/mood STAND_WAIT=30 \
  CONTROLS_BIN=$T/bin/controls SELFTEST_BIN=$T/bin/selftest SYSTEMCTL=$T/bin/systemctl \
  STOP_WAIT=1 FAKE_FOLD_SECONDS=5 \
  FAKE_LOG=$T/log ASK_ANSWER=y bash "$STEPS" --unit-id lamp-test --write --update-key "$KEYFILE" </dev/null >"$T/out" 2>&1 &
script=$!
set +m
for _ in $(seq 100); do grep -q "^mood" "$T/log" && break; sleep 0.1; done
started=$(date +%s)
kill -INT -- -"$script"
wait "$script"; status=$?
took=$(($(date +%s) - started))
check "a body still running past STOP_WAIT is killed, and the person told" '[[ $status == 130 && $took -le 3 ]] && grep -q "did not fold and end within 1 s of SIGTERM: it is killed now" "$T/out" && grep -q "the arm was not eased back" "$T/out" && ! grep -q "the arm was eased back" "$T/out" && ! grep -q "^body folded" "$T/log"'

# A stand that ends on its own but whose body does not fold within STOP_WAIT
# stops the steps there: the arm is held part-way down, so nothing runs after
# it, not even a rest read (the reviewer-lead on #2597).
lamp; ASK_ANSWER=y STAND_WAIT=1 STOP_WAIT=1 FAKE_FOLD_SECONDS=5 run --write --update-key "$KEYFILE"; status=$?
check "a body that does not fold in time stops the steps, with no rest read after it" '[[ $status == 1 ]] && grep -q "the arm did not fold in time: stopped here, the arm held" "$T/out" && [[ $(grep -c "^body --device" "$T/log") == 1 ]] && [[ $(sed -n "/^body --device/,\$p" "$T/log" | grep -c "^measure") == 0 ]] && [[ ! -e $T/root/$ENV ]]'
# Ctrl-C while the body is left alone: lamp-expression is started again.
lamp; set -m
ETC_ROOT=$T/root MEASURE_BIN=$T/bin/measure BODY_BIN=$T/bin/body MOOD_BIN=$T/bin/mood QUIET_WAIT=30 HEAD_STAND_WAIT=0 \
  CONTROLS_BIN=$T/bin/controls SELFTEST_BIN=$T/bin/selftest SYSTEMCTL=$T/bin/systemctl FAKE_BODY_STATE=active FAKE_EXPRESSION_STATE=active \
  FAKE_LOG=$T/log ASK_ANSWER=y bash "$STEPS" --unit-id lamp-test --write --from 4 </dev/null >"$T/out" 2>&1 &
script=$!
set +m
for _ in $(seq 100); do grep -q "systemctl stop lamp-expression" "$T/log" && break; sleep 0.1; done
kill -INT -- -"$script"; wait "$script"; status=$?
check "Ctrl-C while the body is left alone starts lamp-expression again" '[[ $status == 130 ]] && grep -q "systemctl start lamp-expression" "$T/log" && ! grep -q "^selftest" "$T/log"'

check "no stand-in body outlives its stand" '! pgrep -f "$T/bin/body" >/dev/null'
echo "$failures failed"
[[ $failures == 0 ]]
