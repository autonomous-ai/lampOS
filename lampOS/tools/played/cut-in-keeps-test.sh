#!/usr/bin/env bash
# Tests for cut-in-keeps.sh with no lamp and no sound: a stand-in lamp_ssh, scenario.py and quiet_gate.sh answer
# from the FAKE_ settings, and the stand-in scene writes the session events the drop-in's value would give.
# Run: lampOS/tools/played/cut-in-keeps-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/cut-in-keeps.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

cat >"$T/lamp_ssh" <<'EOF'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_DIR/calls"
case "$*" in
  *"systemctl show"*) printf 'ActiveState=active\nEnvironment=%s\nExecStart={ path=/opt/lampOS/voice/current/lamp-voice ; argv[]=/opt/lampOS/voice/current/lamp-voice serve --turn-taking %s --end-silence-ms 600 ; }\n' "$FAKE_ENV" "$FAKE_TT" ;;
  *"grep -q -a"*) exit "${FAKE_OLD:-0}" ;;
  *"KEEPS_LISTENING=on"*"tee "*) echo on >"$FAKE_DIR/dropin" ;;
  *"KEEPS_LISTENING=off"*"tee "*) echo off >"$FAKE_DIR/dropin" ;;
  *"rm -f "*) rm -f "$FAKE_DIR/dropin"; echo removed >>"$FAKE_DIR/put-back" ;;
  *sessions*) cat "$FAKE_DIR/events" ;;
  *) exit 1 ;;
esac
EOF

# The scene: the four lines' #PLAY_US, and the events the voice would write with the drop-in's value.
cat >"$T/scenario.py" <<'EOF'
#!/usr/bin/env bash
v=$(cat "$FAKE_DIR/dropin")
printf '#PLAY_US 1000000 Daniel: ask\n#PLAY_US 12000000 Daniel: a1\n#PLAY_US 14400000 Daniel: a2\n#PLAY_US 19500000 Daniel: third\n' \
  >"$PLAYED_OUT/scenarios/20261022T000000Z-$1.txt"
{
  if [[ $v == on && $FAKE_MODE != no-switch ]]; then
    echo '{"kind":"duplex_policy","at_us":500000,"keeps_listening":{"over_blocks":30,"more_fruitless_yields":2}}'
  else
    echo '{"kind":"duplex_policy","at_us":500000,"keeps_listening":null}'
  fi
  echo '{"kind":"speaker_first_write","at_us":4000000,"owner":{"turn":5}}'
  echo '{"kind":"duplex_yield","at_us":12500000}'
  echo '{"kind":"duplex_resume","at_us":13200000}'
  echo '{"kind":"duplex_yield","at_us":14900000}'
  echo '{"kind":"duplex_resume","at_us":15600000}'
  if [[ $v == on && $FAKE_MODE == normal ]]; then
    echo '{"kind":"duplex_yield","at_us":19900000}'
    echo '{"kind":"duplex_commit","at_us":20100000}'
    echo '{"kind":"turn_finished","at_us":20300000,"turn":5,"outcome":"user_interrupted"}'
    echo '{"kind":"transcript","at_us":21000000,"role":"input","text":"Lamp, stop."}'
  fi
  echo 'not json'
} >"$FAKE_DIR/events"
EOF
printf '#!/usr/bin/env bash\nexit 0\n' >"$T/quiet_gate.sh"
chmod +x "$T/lamp_ssh" "$T/scenario.py" "$T/quiet_gate.sh"

fail=0
run() { # run NAME EXPECTED_EXIT GREP...: the script with this test's FAKE_ settings, one round each way
  local name=$1 want=$2 got out
  shift 2
  rm -rf "$T/d" "$T/out"; mkdir -p "$T/d"
  out=$(FAKE_DIR=$T/d LAMP_HOST=fake LAMP_SSH=$T/lamp_ssh CUT_IN_SCENARIO=$T/scenario.py CUT_IN_QUIET_GATE=$T/quiet_gate.sh \
    PLAYED_OUT=$T/out LAMP_PLAY_AT_LUNCH=yes "$SCRIPT" "$name" 1 2>&1)
  got=$?
  [[ $got == "$want" ]] || { echo "FAIL $name: exit $got, wanted $want"; echo "$out"; fail=1; }
  for g in "$@"; do grep -qF -- "$g" <<<"$out" || { echo "FAIL $name: no '$g' in:"; echo "$out"; fail=1; }; done
  [[ ! -e $T/d/dropin ]] || { echo "FAIL $name: the drop-in was left on"; fail=1; }
}

export FAKE_ENV="LAMP_VOICE_ROUTE=second" FAKE_TT=duplex FAKE_MODE=normal

# Off as today, on heard and stopped in 0.8 s.
run normal 0 \
  'off round 1: asides 2 yields, 2 resumes, 0 commits, third line 5.1 s after the second; third line heard no (no yield, no candidate), story not stopped (went on)' \
  "on  round 1: asides 2 yields, 2 resumes, 0 commits, third line 5.1 s after the second; third line heard yes (yield, commit, words \"Lamp, stop.\"), story stopped 0.80 s after the line's start  PASS" \
  'off: trials 1 of 1; third line heard 0, story stopped 0; gaps to the stop none' \
  'on: trials 1 of 1; third line heard 1, story stopped 1; gaps to the stop 0.80 s' \
  'drop-in removed, lamp-voice restarted without it'
[[ $(grep -c removed "$T/d/put-back") == 1 ]] || { echo "FAIL normal: not put back exactly once"; fail=1; }
grep -qF 'Environment=LAMP_CUT_IN_KEEPS_LISTENING=off' "$T/d/calls" || { echo "FAIL normal: the off arm never put on"; fail=1; }
grep -qF 'Environment=LAMP_CUT_IN_KEEPS_LISTENING=on' "$T/d/calls" || { echo "FAIL normal: the on arm never put on"; fail=1; }
[[ -s $T/out/scenarios/cut-in-keeps-normal.txt ]] || { echo "FAIL normal: no report file"; fail=1; }

# On, but the third line still not heard: FAIL.
FAKE_MODE=on-unheard run unheard 0 'third line heard no (no yield, no candidate), story not stopped (went on)  FAIL' \
  'on: trials 1 of 1; third line heard 0, story stopped 0'

# On, but the voice's policy says the switch is not in force: not a trial.
FAKE_MODE=no-switch run no-switch 0 'the switch was not in force (duplex_policy keeps_listening False); not a trial' \
  'on: trials 0 of 1'

# Not duplex, the switch already set, or a binary older than #2739: nothing changed.
FAKE_TT=v1-baseline run baseline 3 "not on --turn-taking duplex"
[[ ! -e $T/d/put-back ]] || { echo "FAIL baseline: it put back what it never changed"; fail=1; }
FAKE_ENV="LAMP_CUT_IN_KEEPS_LISTENING=on" run already 3 "already set"
FAKE_OLD=1 run old 3 "older than #2739"
grep -qF 'grep -q -a LAMP_CUT_IN_KEEPS_LISTENING /opt/lampOS/voice/current/lamp-voice' "$T/d/calls" \
  || { echo "FAIL old: the binary was not looked at"; fail=1; }

# No LAMP_HOST: nothing at all.
out=$(env -u LAMP_HOST "$SCRIPT" x 2>&1); [[ $? == 2 && $out == *"LAMP_HOST is not set"* ]] || { echo "FAIL no-host: $out"; fail=1; }

[[ $fail == 0 ]] && echo "cut-in-keeps-test: all passed"
exit $fail
