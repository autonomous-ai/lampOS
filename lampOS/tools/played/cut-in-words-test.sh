#!/usr/bin/env bash
# Tests for cut-in-words.sh with no lamp and no sound: a stand-in lamp_ssh, scenario.py and quiet_gate.sh answer from the
# FAKE_ settings, and the stand-in scene writes the PLAY lines and the session events a voice would give.
# Run: lampOS/tools/played/cut-in-words-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/cut-in-words.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

cat >"$T/lamp_ssh" <<'EOF2'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_DIR/calls"
case "$*" in
  *"systemctl show"*) printf 'ActiveState=active\nEnvironment=%s\nExecStart={ path=/opt/lampOS/voice/current/lamp-voice ; argv[]=/opt/lampOS/voice/current/lamp-voice serve --turn-taking duplex --end-silence-ms %s ; }\n' "$FAKE_ENV" "$FAKE_SILENCE" ;;
  *sha256sum*) echo "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  /opt/lampOS/voice/current/lamp-voice" ;;
  *sessions*) cat "$FAKE_DIR/events" ;;
  *) exit 1 ;;
esac
EOF2

# The scene: the lines' #PLAY_US, and the events the voice would write; leg from the scene's name (...-words-A-1).
cat >"$T/scenario.py" <<'EOF2'
#!/usr/bin/env bash
echo "$1" >>"$FAKE_DIR/scenes"
leg=${1#*-words-}; leg=${leg%%-*}
case $leg in
  A) printf '#PLAY_US 1000000 v: ask\n#PLAY_US 11000000 v: line\n' ;;
  B) printf '#PLAY_US 1000000 v: ask\n#PLAY_US 11000000 v: stop\n#PLAY_US 13500000 v: line\n' ;;
  C) printf '#PLAY_US 1000000 v: ask\n#PLAY_US 4600000 v: line\n' ;;
esac >"$PLAYED_OUT/scenarios/20261124T000000Z-$1.txt"
line=11000000; [[ $leg == B ]] && line=13500000; [[ $leg == C ]] && line=4600000
heard=${FAKE_HEARD:-"Lamp, make a skill called tech news."}
{
  echo '{"kind":"transcript","at_us":2000000,"role":"output","turn":5,"text":"The keeper climbed twenty four steps to the lamp every evening."}'
  echo '{"kind":"speech_final_sample_retired","at_us":'"${FAKE_LAST:-4500000}"',"owner":{"turn":5}}'
  [[ $FAKE_MODE == unheard ]] || echo '{"kind":"transcript","at_us":'$((line + 3000000))',"role":"input","text":"'"$heard"'"}'
  echo 'not json'
} >"$FAKE_DIR/events"
EOF2
printf '#!/usr/bin/env bash\nexit 0\n' >"$T/quiet_gate.sh"
chmod +x "$T/lamp_ssh" "$T/scenario.py" "$T/quiet_gate.sh"

fail=0
run() { # run NAME EXPECTED_EXIT GREP...: the script with this test's FAKE_ settings, one round of each leg
  local name=$1 want=$2 got out
  shift 2
  rm -rf "$T/d" "$T/out"; mkdir -p "$T/d"
  out=$(FAKE_DIR=$T/d LAMP_HOST=fake PLAYED_VOICE=v LAMP_SSH=$T/lamp_ssh CUT_IN_SCENARIO=$T/scenario.py CUT_IN_QUIET_GATE=$T/quiet_gate.sh \
    PLAYED_OUT=$T/out LAMP_PLAY_AT_LUNCH=yes "$SCRIPT" "$name" 1 2>&1)
  got=$?
  [[ $got == "$want" ]] || { echo "FAIL $name: exit $got, wanted $want"; echo "$out"; fail=1; }
  for g in "$@"; do grep -qF -- "$g" <<<"$out" || { echo "FAIL $name: no '$g' in:"; echo "$out"; fail=1; }; done
}

export FAKE_ENV="LAMP_FIRST_WORDS=kept LAMP_REPLY_END_REACH=on LAMP_SOFT_START=on LAMP_SECOND_OWN_ECHO=on" FAKE_SILENCE=600 FAKE_MODE=normal

# The product's voice, the lines heard whole: each leg passes; nothing on the lamp was changed.
run normal 0 \
  'sha256 0123456789abcdef' \
  'A round 1: heard "Lamp, make a skill called tech news.": none of the lamp'"'"'s words, first words kept  PASS' \
  'B round 1: heard "Lamp, make a skill called tech news.": none of the lamp'"'"'s words, first words kept  PASS' \
  'C round 1: heard "Lamp, make a skill called tech news.": none of the lamp'"'"'s words, first words kept  PASS' \
  'A: trials 1 of 1; pass 1, fail 0' 'B: trials 1 of 1; pass 1, fail 0' 'C: trials 1 of 1; pass 1, fail 0'
grep -qE 'tee |rm -f|systemctl (restart|daemon)' "$T/d/calls" && { echo "FAIL normal: the kit changed the lamp"; fail=1; }

# The lamp's own words ahead of the line: FAIL with the words named.
FAKE_HEARD="Twenty four steps. make a skill called tech news" run own 0 \
  'A round 1: heard "Twenty four steps. make a skill called tech news": own words "twenty four"' '  FAIL' \
  'A: trials 1 of 1; pass 0, fail 1'

# The person's first words cut: FAIL.
FAKE_HEARD="a skill called tech news" run cut 0 'first words NOT kept  FAIL' 'B: trials 1 of 1; pass 0, fail 1'

# The line not heard: FAIL.
FAKE_MODE=unheard run unheard 0 'the line was not heard (no input transcript within 15 s); FAIL' 'A: trials 1 of 1; pass 0, fail 1'

# C, but the line began 2 s after the lamp's last sound: not a trial.
FAKE_LAST=2600000 run late 0 'C round 1: the line began +2.0 s from the lamp'"'"'s last sound, not within 0.5 s; not a trial' 'C: trials 0 of 1'

# The voice not as the product's: nothing played.
FAKE_SILENCE=1200 run silence 3 "'--end-silence-ms 600' is missing"
[[ ! -s $T/d/scenes ]] || { echo "FAIL silence: it played"; fail=1; }
FAKE_ENV="LAMP_FIRST_WORDS=kept LAMP_REPLY_END_REACH=on LAMP_SOFT_START=on" run noecho 3 "'LAMP_SECOND_OWN_ECHO=on' is missing"
[[ ! -s $T/d/scenes ]] || { echo "FAIL noecho: it played"; fail=1; }

# No voice named, no LAMP_HOST: nothing at all.
out=$(env -u PLAYED_VOICE LAMP_HOST=fake "$SCRIPT" x 2>&1); [[ $? == 2 && $out == *"PLAYED_VOICE is not set"* ]] || { echo "FAIL no-voice: $out"; fail=1; }
out=$(env -u LAMP_HOST PLAYED_VOICE=v "$SCRIPT" x 2>&1); [[ $? == 2 && $out == *"LAMP_HOST is not set"* ]] || { echo "FAIL no-host: $out"; fail=1; }

[[ $fail == 0 ]] && echo "cut-in-words-test: all passed"
exit $fail
