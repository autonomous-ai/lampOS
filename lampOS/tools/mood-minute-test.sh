#!/usr/bin/env bash
# Tests for mood-minute.sh with no lamp: stand-ins for systemctl, systemd-run
# and lamp-bus, whose taps give a minute of made-up messages at once.
# Run: lampOS/tools/mood-minute-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/mood-minute.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/cg/system.slice"
printf 'usage_usec 1000000\n' >"$T/cg/system.slice/cpu.stat"

# systemctl: the voice and Expression are active; lamp-mood-check while the
# systemd-run stand-in has started it; lamp-mood never (FAKE_MOOD_RUNS: it is).
cat >"$T/systemctl" <<'EOF'
#!/usr/bin/env bash
case "$1 ${*: -1}" in
  "is-active lamp-voice.service" | "is-active lamp-expression.service") [[ -z ${FAKE_DOWN:-} ]] ;;
  "is-active lamp-mood-check.service") [[ -e $FAKE_DIR/mood-running ]] ;;
  "is-active lamp-mood.service") [[ -n ${FAKE_MOOD_RUNS:-} ]] ;;
  "stop lamp-mood-check.service") rm -f "$FAKE_DIR/mood-running"; echo stop >>"$FAKE_DIR/stops" ;;
  show*) echo /system.slice ;;
  *) exit 1 ;;
esac
EOF
cat >"$T/systemd-run" <<'EOF'
#!/usr/bin/env bash
echo "$*" >"$FAKE_DIR/run-args"
touch "$FAKE_DIR/mood-running"
EOF
# lamp-bus tap TOPIC ...: a minute of messages, 50 a second for the head and
# one a second for Mood. Off, the head breathes the plain 5.2 s. On, Mood's
# pace is FAKE_PACE, and the head follows Mood (FAKE_HEAD=mood) or stays
# plain (FAKE_HEAD=plain).
cat >"$T/lamp-bus" <<'EOF'
#!/usr/bin/env bash
topic=$2
on=0; [[ -e $FAKE_DIR/mood-running ]] && on=1
awk -v topic="$topic" -v on="$on" -v pace="${FAKE_PACE:-1.0}" -v head="${FAKE_HEAD:-mood}" 'BEGIN {
  start = on ? 2000000000 : 1000000000
  breath = 5200000
  tau = 8 * atan2(1, 1)
  if (topic == "expression/body") {
    for (i = 0; i < 3000; i++) {
      t = start + i * 20000
      plain = (t % breath) / breath
      mood = 0.3 + (t - start) / breath * pace
      mood -= int(mood)
      phase = (on && head == "mood") ? mood : plain
      printf "{\"topic\":\"%s\",\"source\":\"expression\",\"seq\":%d,\"at_us\":%.0f} {\"offsets\":{\"head_pitch\":%.9f,\"head_roll\":0.0}}\n", topic, i + 1, t, 0.02 * sin(tau * phase)
    }
  } else if (topic == "mood/state" && on) {
    for (i = 0; i < 60; i++) {
      t = start + i * 1000000
      mood = 0.3 + (t - start) / breath * pace
      mood -= int(mood)
      printf "{\"topic\":\"%s\",\"source\":\"mood\",\"seq\":%d,\"at_us\":%.0f} {\"stamp\":{\"clock_id\":\"boot\",\"at_us\":%.0f,\"sequence\":%d},\"session_id\":\"s1\",\"awake\":0.7,\"energy\":0.3,\"warmth\":0.6,\"asleep\":false,\"familiarity\":\"unknown\",\"breath\":{\"phase\":%.6f,\"pace\":%s}}\n", topic, i + 1, t, t, i + 1, mood, pace
    }
  }
}'
EOF
chmod +x "$T/systemctl" "$T/systemd-run" "$T/lamp-bus"
printf '#!/bin/sh\nexit 0\n' >"$T/lamp-mood"
chmod +x "$T/lamp-mood"
export FAKE_DIR=$T SYSTEMCTL=$T/systemctl SYSTEMD_RUN=$T/systemd-run BUS=$T/lamp-bus
export MOOD_BIN=$T/lamp-mood CGROUP_ROOT=$T/cg SECONDS_EACH=2 SETTLE=0
# The numbers are lamp-voice-eval's: this workspace's build of it.
ROOT=$(cd "$(dirname "$0")/.." && pwd)
if [[ -z ${EVAL:-} ]]; then
  (cd "$ROOT" && cargo build -q -p lamp-voice-eval) || exit 1
  EVAL=${CARGO_TARGET_DIR:-$ROOT/target}/debug/lamp-voice-eval
fi
export EVAL

pass=0 fail=0
check() { # name expected-rc actual-rc
  if [[ $2 == "$3" ]]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL: $1 (expected exit $2, got $3)"; fi
}
has() { # name file pattern
  if grep -qF -- "$3" "$2"; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL: $1 (no '$3' in $2)"; cat "$2"; fi
}
M() { rm -f "$T/stops" "$T/run-args" "$T/mood-running"; OUT=$T/out-$1 "$SCRIPT" "${@:2}" >"$T/said" 2>&1; }

# The head follows Mood's quicker breath: in step, with Mood's checks.
FAKE_PACE=1.2 FAKE_HEAD=mood M follows; check "a run ends well" 0 $?
has "off is the plain breath" "$T/said" "period_s=5.20 fit_to_plain_breath=1.00"
has "Mood's state is checked" "$T/said" "sessions=1 sequences_rising=true pace_min=1.20 pace_max=1.20 pace_bounded=true"
has "the head is in step with Mood" "$T/said" "fit_to_mood_breath=1.00 in_step=true"
has "on's period is Mood's" "$T/said" "period_s=4.34"
has "CPU is said for each part" "$T/said" "on cpu_ms voice=0 expression=0 mood=0"
has "Mood runs with its switch, as the bus's user" "$T/run-args" "--unit=lamp-mood-check --uid=orangepi --gid=orangepi"
has "LAMP_MOOD is on" "$T/run-args" "-E LAMP_MOOD=on"
has "with the lamp's zone" "$T/run-args" "serve --zone system --bus-root /run/lampos-bus"
has "Mood is stopped after" "$T/stops" "stop"
[[ ! -e $T/mood-running ]]; check "nothing left running" 0 $?

# The head keeps the plain breath while Mood's is quicker: not in step.
FAKE_PACE=1.2 FAKE_HEAD=plain M plain; check "a run that is not in step still ends" 0 $?
has "not in step" "$T/said" "in_step=false"

# Refused before anything runs.
FAKE_DOWN=1 M down; check "a part down is refused" 3 $?
[[ ! -e $T/run-args ]]; check "nothing started for a part down" 0 $?
FAKE_MOOD_RUNS=1 M runs; check "Mood already running is refused" 3 $?
MOOD_BIN=/nonexistent M nobin; check "no Mood binary is refused" 2 $?
EVAL=/nonexistent M noeval; check "no lamp-voice-eval is refused" 2 $?
M badstop --stop-at 5; check "a stop past the minute is refused" 2 $?
M badflag --loud; check "an unknown option is refused" 2 $?

echo "mood-minute-test: $pass passed, $fail failed"
[[ $fail == 0 ]]
