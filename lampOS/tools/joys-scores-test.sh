#!/usr/bin/env bash
# Tests for joys-scores.sh with no lamp: stand-ins for systemctl and lamp-bus,
# whose recording is made up, and the real lamp-skills (SKILLS, else from
# PATH) replaying it to a laugh skill written here.
# Run: SKILLS=path/to/lamp-skills lampOS/tools/joys-scores-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/joys-scores.sh
SKILLS=${SKILLS:-$(command -v lamp-skills)}
[[ -x $SKILLS ]] || { echo "SKILLS must name a built lamp-skills" >&2; exit 2; }
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
fails=0
check() { # check WHAT CONDITION...
  local what=$1; shift
  if "$@"; then echo "ok: $what"; else echo "FAIL: $what"; fails=$((fails + 1)); fi
}

# systemctl: lamp-ears active, its environment FAKE_ENV; every call logged.
cat >"$T/systemctl" <<'EOF'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_DIR/systemctl.log"
# At each restart, the route the drop-ins set and whether a yes is stored.
if [[ $1 == restart ]]; then
  echo "route $(cat "$FAKE_DIR"/units/*/zz-joys.conf 2>/dev/null | grep -o 'LAMP_PRIVACY_CONTROLS=[a-z-]*' | sort -u | tr '\n' ' ')yes $(cat "$FAKE_DIR/onboarding/room-sounds.json" 2>/dev/null | grep -o '"room_sounds":"[a-z]*"')" >>"$FAKE_DIR/systemctl.log"
fi
case $1 in
  is-active) exit 0 ;;
  show) echo "$FAKE_ENV" ;;
esac
EOF
# sleep: the wall clock moves on by that many seconds, at once.
cat >"$T/sleep" <<'EOF'
#!/usr/bin/env bash
echo $(($(cat "$NOW_FILE") + $1 * 1000)) >"$NOW_FILE"
EOF
# lamp-bus: `latest` has scores; `record DIR ...` writes the made-up run.
cat >"$T/lamp-bus" <<'EOF'
#!/usr/bin/env bash
case $1 in
  latest) echo '{"status":"scored"}' ;;
  record) mkdir -p "$2"; cp "$FAKE_DIR"/rec/*.jsonl "$2/" ;;
esac
EOF
chmod +x "$T/systemctl" "$T/sleep" "$T/lamp-bus"

# The run, with the clock at B when it starts: recording from B, the empty
# room 20 s from B+1 s, the laugh track 20 s, then 10 s of a line over it,
# during which the lamp speaks from 42 s to 46 s. Laughter 0.9 from 25 s to
# 35 s, and from 43 s to 45 s; 0.02 otherwise. A face in view throughout.
B=1792400000000
mkdir -p "$T/rec"
line() { # line TOPIC TYPE SEQ AT_MS PAYLOAD
  printf '{"header":{"topic":"%s","kind":"latest","enc":"json","type":"%s","source":"t","boot":"0123456789abcdef","seq":%d,"at_us":%d,"at_unix_ms":%d},"payload_b64":"%s"}\n' \
    "$1" "$2" "$3" $((($4 - B) * 1000 + 1)) "$4" "$(printf '%s' "$5" | base64 | tr -d '\n')"
}
{
  echo '{"topic":"ears/classes","declaration":{"type":"lamp_msgs/SoundClasses","version":1,"kind":"latest"}}'
  for i in $(seq 0 104); do
    at=$((B + i * 500)) s=$((i / 2)) laugh=0.02
    if [[ ($s -ge 25 && $s -lt 35) || ($s -ge 43 && $s -lt 45) ]]; then laugh=0.9; fi
    line ears/classes lamp_msgs/SoundClasses $((i + 1)) $at \
      "{\"status\":\"scored\",\"model_id\":\"yamnet-qai-v0.59.0\",\"window\":{\"start_us\":0,\"end_us\":1,\"completed_us\":2,\"capture_epoch\":1},\"scores\":{\"speech\":0.3,\"music\":0.0,\"laughter\":$laugh},\"own_voice\":\"unknown\"}"
  done
} >"$T/rec/ears_classes.jsonl"
{
  echo '{"topic":"humans/faces/tracked","declaration":{"type":"lamp_msgs/Faces","version":1,"kind":"latest"}}'
  for s in $(seq 0 52); do
    line humans/faces/tracked lamp_msgs/Faces $((s + 1)) $((B + s * 1000 + 100)) \
      '{"faces":[{"confidence":0.9,"id":"a1","xmax":0.6,"xmin":0.3,"ymax":0.6,"ymin":0.2}]}'
  done
} >"$T/rec/humans_faces_tracked.jsonl"
{
  echo '{"topic":"voice/state","declaration":{"type":"lamp_msgs/VoiceState","version":1,"kind":"latest"}}'
  line voice/state lamp_msgs/VoiceState 1 $((B + 250)) '{"state":"resting"}'
  line voice/state lamp_msgs/VoiceState 2 $((B + 42000)) '{"state":"speaking"}'
  line voice/state lamp_msgs/VoiceState 3 $((B + 46000)) '{"state":"resting"}'
} >"$T/rec/voice_state.jsonl"

# A laugh skill as Laugh glance is: laughter over 0.5 for 1 s, the voice
# resting and a face in view, once a minute.
mkdir -p "$T/laugh"
printf -- '---\nname: laugh\ndescription: A test.\n---\n' >"$T/laugh/SKILL.md"
cat >"$T/laugh/skill.toml" <<'EOF'
needs = ["ears", "eyes", "body"]
[permissions]
topics = ["ears/classes", "humans/faces/tracked", "voice/state"]
body = true
[trigger]
topic = "ears/classes"
field = "scores.laughter"
above = 0.5
for = "1s"
cooldown = "1m"
[[while]]
topic = "voice/state"
field = "state"
equals = "resting"
[[while]]
topic = "humans/faces/tracked"
present = true
[quiet]
from = "23:00"
to = "06:00"
[ask]
policy = "nudge"
[[do]]
body = { look_at = "face" }
EOF
# Its own recorded case, the made-up run: a glance at 26 s. A lamp-skills
# that can't read `scores.laughter` makes none and fails it.
mkdir -p "$T/laugh/tests/run"
cp "$T"/rec/*.jsonl "$T/laugh/tests/run/"
printf 'clock = "12:00"\n[[expect]]\nat = "26s"\nwithin = "1s"\ncall = "body.gaze"\n' >"$T/laugh/tests/run/expect.toml"
touch "$T/yamnet.onnx"

run() { # run [ENV...] [-- ARG]: the laugh row, its output in $T/out
  local arg=
  if [[ ${*: -1} == --* ]]; then arg=${*: -1}; set -- "${@:1:$#-1}"; fi
  rm -f "$T/systemctl.log"
  mkdir -p "$T/tmp"
  echo "$B" >"$T/now"
  env FAKE_DIR="$T" FAKE_ENV="LAMP_BUS=on LAMP_PRIVACY_CONTROLS=on" NOW_FILE="$T/now" \
    SYSTEMCTL="$T/systemctl" SLEEP="$T/sleep" BUS="$T/lamp-bus" SKILLS="$SKILLS" \
    UNIT_DIR="$T/units" ROOM_SOUNDS="$T/onboarding/room-sounds.json" TMPDIR="$T/tmp" SKILL_DIR="$T/laugh" EARS_MODEL="$T/yamnet.onnx" \
    QUIET_S=20 TRACK_S=20 OVER_S=10 "$@" bash "$SCRIPT" laugh $arg >"$T/out" 2>&1
}

run
status=$?
cat "$T/out" | sed 's/^/  | /'
check "it ends well" test "$status" = 0
check "the empty room never goes over the bar" grep -q '^empty laughter: 40 scored, 0 unavailable, mean 0.02, highest 0.02, over 0.5 0%' "$T/out"
check "the laugh track's spell over it is 10 s" grep -q '^track laughter: .* longest over 10.0 s' "$T/out"
check "one glance on the track, none over the line while the lamp speaks" \
  grep -q '^body.gaze asked: empty 0, track 1, over 0; while the voice spoke 0; at 25.0s$' "$T/out"
check "it plays the cues" grep -q '^PLAY: the laugh track now' "$T/out"
check "lamp-ears put back: the drop-in removed, restarted twice" \
  bash -c "[[ ! -e '$T/units/lamp-ears.service.d/zz-joys.conf' && \$(grep -c '^restart lamp-ears.service' '$T/systemctl.log') == 2 ]]"
check "the recording deleted: its own folder from mktemp, gone" bash -c "[[ -z \$(ls -A '$T/tmp') ]]"

run FAKE_ENV="LAMP_BUS=on"
status=$?
check "without privacy controls it does nothing" \
  bash -c "[[ $status == 3 ]] && ! grep -q restart '$T/systemctl.log' && grep -q 'LAMP_PRIVACY_CONTROLS=on' '$T/out'"

# On the privacy route for the run: all three units, put back in order.
run FAKE_ENV="LAMP_BUS=on" --with-privacy-controls
status=$?
check "with --with-privacy-controls, the route on in Controls, Voice and Ears for the run" \
  bash -c "[[ $status == 0 ]] && grep -q 'body.gaze asked: empty 0, track 1' '$T/out' &&
    [[ \$(grep '^restart' '$T/systemctl.log' | tr '\n' ' ') == 'restart lamp-controls.service restart lamp-voice.service restart lamp-ears.service restart lamp-controls.service restart lamp-voice.service restart lamp-ears.service ' ]] &&
    [[ -z \$(find '$T/units' -name zz-joys.conf) ]]"

# A lamp with no slide and no yes stored (a no leaves no file, as "never
# asked" does): refused, nothing touched, no yes written.
rm -rf "$T/onboarding"
run FAKE_ENV="LAMP_BUS=on" --no-slide
status=$?
check "with --no-slide and no person's yes, refused and nothing touched" \
  bash -c "[[ $status == 3 && ! -e '$T/onboarding' ]] && ! grep -q restart '$T/systemctl.log' && grep -q 'says yes in setup first' '$T/out'"

# With the person's own yes: the no-slide route in all three for the run, and
# the yes left byte for byte.
mkdir -p "$T/onboarding"
echo '{"room_sounds":"yes","at_unix_s":1}' >"$T/onboarding/room-sounds.json"
run FAKE_ENV="LAMP_BUS=on" --no-slide
status=$?
check "with --no-slide and the person's yes, the no-slide route in all three, the yes left" \
  bash -c "[[ $status == 0 ]] && grep -q 'body.gaze asked: empty 0, track 1' '$T/out' &&
    [[ \$(grep -c '^restart' '$T/systemctl.log') == 6 ]] &&
    grep -q '^route LAMP_PRIVACY_CONTROLS=no-slide yes \"room_sounds\":\"yes\"' '$T/systemctl.log' &&
    [[ \$(cat '$T/onboarding/room-sounds.json') == '{\"room_sounds\":\"yes\",\"at_unix_s\":1}' && -z \$(find '$T/units' -name zz-joys.conf) ]]"

# Already on no-slide: Ears alone.
run FAKE_ENV="LAMP_BUS=on LAMP_PRIVACY_CONTROLS=no-slide"
status=$?
check "on no-slide already with the person's yes, Ears alone restarted" \
  bash -c "[[ $status == 0 ]] && [[ \$(grep '^restart' '$T/systemctl.log' | sort -u) == 'restart lamp-ears.service' ]]"
rm -rf "$T/onboarding"

# A lamp-skills that fails the skill's own cases (one without #1920's dotted
# fields) is refused before anything is touched.
printf '#!/usr/bin/env bash\necho "skills/x/tests/a: made 0 calls, 1 expected: []"\nexit 1\n' >"$T/old-skills"
chmod +x "$T/old-skills"
run SKILLS="$T/old-skills"
status=$?
check "a lamp-skills that fails the skill's own cases is refused, nothing touched" \
  bash -c "[[ $status == 2 && ! -e '$T/systemctl.log' ]] && grep -q 'its replay would count nothing' '$T/out'"

echo "$fails failed"
[[ $fails == 0 ]]
