#!/usr/bin/env bash
# Tests for addon-say.sh with no lamp: a stand-in lamp_ssh answers each step from the FAKE_ settings.
# Run: lampOS/tools/played/addon-say-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/addon-say.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

cat >"$T/lamp_ssh" <<'EOF'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_DIR/calls"
case "$*" in
  *"systemctl show"*) printf 'ActiveState=active\nEnvironment=%s\n' "$FAKE_ENV" ;;
  *"tee "*) touch "$FAKE_DIR/dropin" ;;
  *"rm -f "*) rm -f "$FAKE_DIR/dropin"; echo removed >>"$FAKE_DIR/put-back" ;;
  *"test -S"*) true ;;
  *monotonic_ns*) echo 1000 ;;
  *socket.socket*) [[ -e $FAKE_DIR/dropin ]] || exit 1; printf '{"event":"begun","at_us":1200}\n%s\n' "$FAKE_OUTCOME" ;;
  *sessions*) printf '%s\n' "$FAKE_EVENTS" ;;
  *) exit 1 ;;
esac
EOF
chmod +x "$T/lamp_ssh"

said() { # said TEXT...: transcript events of the lamp's turn 7, after the start, with one heard line and one before it
  printf '{"kind":"transcript","at_us":500,"turn":3,"text":"An old line."}\n'
  printf '{"kind":"transcript","at_us":1500,"turn":8,"role":"input","text":"Someone talking."}\n'
  for t in "$@"; do printf '{"kind":"transcript","at_us":1600,"turn":7,"text":"%s"}\n' "$t"; done
}

fail=0
run() { # run NAME EXPECTED_EXIT GREP...: the script with this test's FAKE_ settings
  local name=$1 want=$2 got out
  shift 2
  rm -rf "$T/d"; mkdir -p "$T/d"
  out=$(FAKE_DIR=$T/d LAMP_HOST=fake LAMP_SSH=$T/lamp_ssh PLAYED_OUT=$T/out ADDON_SAY_SETTLE=0 LAMP_PLAY_AT_LUNCH=yes \
    "$SCRIPT" "$name" "${TEXT:-The rain should stop by four this afternoon.}" 2>&1)
  got=$?
  [[ $got == "$want" ]] || { echo "FAIL $name: exit $got, wanted $want"; echo "$out"; fail=1; }
  for g in "$@"; do grep -qF -- "$g" <<<"$out" || { echo "FAIL $name: no '$g' in:"; echo "$out"; fail=1; }; done
  [[ ! -e $T/d/dropin ]] || { echo "FAIL $name: the drop-in was left on"; fail=1; }
}

export FAKE_ENV="LAMP_VOICE_ROUTE=second LAMP_SAY=on"

# Said word for word, across two transcript pieces; the earlier and the heard lines are not counted.
FAKE_OUTCOME='{"outcome":"spoken"}' FAKE_EVENTS=$(said "The rain should stop" "by four this afternoon.") \
  run word-for-word 0 "outcome spoken" "kept words 8, kept in order 8, changed 0, added 0, refused no"
grep -q removed "$T/d/put-back" || { echo "FAIL word-for-word: the voice was not put back"; fail=1; }
grep -qF "Environment=LAMP_ADDON_SAY=on" "$T/d/calls" || { echo "FAIL word-for-word: the switch was never put on"; fail=1; }
grep -qF '"quoted": "The rain should stop by four this afternoon."' "$T/d/calls" || { echo "FAIL word-for-word: the ask was not quoted"; fail=1; }
[[ -s $T/out/scenarios/addon-say-word-for-word.txt ]] || { echo "FAIL word-for-word: no report file"; fail=1; }

# Said in the lamp's own words: some kept, some changed, some added.
FAKE_OUTCOME='{"outcome":"spoken"}' FAKE_EVENTS=$(said "Looks like the rain stops around four today.") \
  run own-words 0 "kept words 8, kept in order 3, changed 5, added 5, refused no"

# Left unsaid: nothing said, every word changed.
FAKE_OUTCOME='{"outcome":"dropped","why":"said nothing"}' FAKE_EVENTS=$(said) \
  run unsaid 0 "outcome dropped (said nothing)" "said:  (nothing)" "kept in order 0, changed 8"

# Refused by the say service.
FAKE_OUTCOME='{"outcome":"refused","why":"add-on says need the second route"}' FAKE_EVENTS=$(said) \
  run refused 0 "outcome refused (add-on says need the second route)" "refused yes"

# Not on the second route, the switch already set, or no say service: nothing changed.
FAKE_ENV="LAMP_VOICE_ROUTE=live LAMP_SAY=on" FAKE_OUTCOME= FAKE_EVENTS= run live 3 "not on the second route"
[[ ! -e $T/d/put-back ]] || { echo "FAIL live: it put back what it never changed"; fail=1; }
FAKE_ENV="LAMP_VOICE_ROUTE=second LAMP_SAY=on LAMP_ADDON_SAY=on" FAKE_OUTCOME= FAKE_EVENTS= run already 3 "already set"
FAKE_ENV="LAMP_VOICE_ROUTE=second" FAKE_OUTCOME= FAKE_EVENTS= run no-say 3 "say service is off"

# A kept text over 300 bytes is refused here, before the lamp is touched.
TEXT=$(printf 'a%.0s' $(seq 301)) FAKE_OUTCOME= FAKE_EVENTS= run too-long 2 "1 to 300 bytes"
[[ ! -e $T/d/calls ]] || { echo "FAIL too-long: the lamp was reached"; fail=1; }

# No LAMP_HOST: nothing at all.
out=$(env -u LAMP_HOST "$SCRIPT" x 2>&1); [[ $? == 2 && $out == *"LAMP_HOST is not set"* ]] || { echo "FAIL no-host: $out"; fail=1; }

[[ $fail == 0 ]] && echo "addon-say-test: all passed"
exit $fail
