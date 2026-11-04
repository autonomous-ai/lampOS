#!/usr/bin/env bash
# factory-run.sh — one lamp's whole factory run, on the lamp itself, with no
# one at a console: started by factory-lamp.sh (systemd-run), it says every
# step on the lamp's speaker and keeps its results on the lamp, so the Mac
# can leave for the next lamp at once.
#
#   factory-run.sh unset ID      lamp not set up: checks (r2, 4b, 4e–4g), the
#                                head onto its stop (head-nudge.py), step 1
#                                (the arm stand), every input in one window
#                                (input-check.py: head pads, button, reset,
#                                privacy), step 3 (the key), 4d (--gaze)
#   factory-run.sh selftest ID   lamp set up from the app: step 4 (4c4), the
#                                ring judged within 4 s, not checked allowed
#                                only for touch_by_hand and storage (#3530);
#                                with RESET=yes a
#                                passed lamp is reset for packing, after its
#                                record is written, as steps.sh resets it
#   factory-run.sh note ID K=V…  set keys in the record (used before a reset)
#
# On the lamp, in /var/lib/lamp-test (outside the reset's wipe list, so it
# survives the 5 s reset):
#   factory.env         the record: KEY=value, every result of every run
#   factory-run.log     everything the steps printed
#   factory-run.status  RUNNING <step> | PASS <mode> | FAIL <mode>: <why>
#
# The ring: a colour drawn within 4 s passes (the hardware lead, 27 Oct);
# when lamp-selftest fails only the ring (it allows 1 s), ring-check.py
# decides it.
#
# Needs in /home/orangepi: steps.sh, factory-voice.py, head-nudge.py,
# input-check.py, ring-check.py; the prompts in /var/lib/lamp-test/prompts.
# As root.
set -uo pipefail

MODE=${1:-}; ID=${2:-}
DIR=/var/lib/lamp-test; HOME_DIR=/home/orangepi
REC=$DIR/factory.env; LOG=$DIR/factory-run.log; STATUS=$DIR/factory-run.status
GAZE=${GAZE:-755,555}; NUDGE_TARGET=${NUDGE_TARGET:-2010}
PINS=/etc/lampOS/controls/controls-pins.json; ENV_FILE=/etc/lampOS/body/lamp.env
[[ $MODE == unset || $MODE == selftest || $MODE == note ]] && [[ $ID =~ ^[0-9a-f]{4}$ ]] || { sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
mkdir -p "$DIR"
exec >>"$LOG" 2>&1
cd "$HOME_DIR" || exit 2

say() { echo "[$(date '+%F %T')] factory-run: $*"; }
status() { echo "$*" > "$STATUS"; say "status: $*"; }
speak() { [[ -f $DIR/prompts/$1.wav ]] && aplay -q -D device_speaker "$DIR/prompts/$1.wav" 2>/dev/null; }
# Set KEY=value lines in the record, replacing a key already there.
rec() {
  local kv key
  touch "$REC"
  for kv in "$@"; do
    key=${kv%%=*}
    grep -v "^$key=" "$REC" > "$REC.new"; echo "$kv" >> "$REC.new"; mv "$REC.new" "$REC"
  done
  chmod 0644 "$REC"
}
fail() { rec "RESULT_${MODE^^}=FAIL: $*" "RESULT_${MODE^^}_AT=$(date '+%F %T %z')"; status "FAIL $MODE: $*"; speak factory_failed; exit 1; }
# steps.sh under factory-voice: spoken, and each question answered after "Starting in five seconds".
steps() { python3 "$HOME_DIR/factory-voice.py" --auto "$@" </dev/null; }

if [[ $MODE == note ]]; then
  shift 2; rec "$@"; say "noted: $*"; exit 0
fi

rec "LAMP_ID=$ID" "HOSTNAME=$(hostname)" \
    "BUILD=$(jq -r .installed.sequence /var/lib/lampos-update/state.json)" \
    "BUILD_COMMIT=$(jq -r .installed.commit /var/lib/lampos-update/state.json)" \
    "STEPS_SHA=$(sha256sum "$HOME_DIR/steps.sh" | cut -c1-64)"
say "start $MODE on $(hostname), lamp $ID"
[[ $(hostname) == "lamp-$ID" ]] || fail "this lamp is $(hostname), not lamp-$ID"

if [[ $MODE == selftest ]]; then
  status "RUNNING 4c4 self-test"
  [[ -e /var/lib/lampos-setup/done ]] || fail "not set up from the app: step 4 runs on a set-up lamp"
  began=$(date +%s)
  # steps.sh's reset question is answered no: this decides the ring and
  # resets the lamp itself, the way steps.sh would.
  steps --decline-reset -- ./steps.sh --unit-id "$ID" --write --from 4
  latest=$(ls -t /var/lib/lampOS/selftest/*.json 2>/dev/null | grep -v latest | head -1)
  [[ -n $latest && $(stat -c %Y "$latest") -ge $began ]] || fail "step 4 did not run the self-test (see the log)"
  parts() { jq -r --arg v "$1" '[.. | objects | select(.verdict? == $v and (.part // "") != "" and .part != "selftest") | .part] | join(",")' "$latest"; }
  failed=$(parts fail); unchecked=$(parts not_checked)
  rec "SELFTEST_FAILED=${failed:-none}" "SELFTEST_NOT_CHECKED=${unchecked:-none}" "SELFTEST_RECORD=$latest"
  # The ring: the hardware lead accepts a colour drawn within 4 s (#3530,
  # 27 Oct); lamp-selftest allows 1 s. When it is the only failed part,
  # ring-check.py decides it.
  if [[ $failed == ring ]]; then
    status "RUNNING ring within 4 s"
    out=$(python3 "$HOME_DIR/ring-check.py" 2>&1); rc=$?; echo "$out"
    mapfile -t ring < <(sed -n 's/^RECORD //p' <<<"$out"); rec "${ring[@]}"
    [[ $rc == 0 ]] && failed="" || fail "ring: $(sed -n 's/^RECORD RING=//p' <<<"$out")"
  fi
  [[ -z $failed ]] || fail "self-test failed: $failed"
  # Not checked counts as a pass only for the parts #3530 allows (no person
  # touched in its own window: input-check.py recorded the inputs; the
  # kernel's log rotated). Any other part not checked (head_move while the
  # lamp listens or a joint is warm, the speaker, the camera) is run again,
  # never reset and packed (hw-agent2nd on gray, 27 Oct).
  allowed=",${ALLOWED_NOT_CHECKED:-touch_by_hand,storage},"
  other=""; IFS=, read -ra seen_parts <<<"$unchecked"
  for part in ${seen_parts[@]+"${seen_parts[@]}"}; do [[ $allowed == *",$part,"* ]] || other="${other:+$other,}$part"; done
  [[ -z $other ]] || fail "not checked: $other (only ${ALLOWED_NOT_CHECKED:-touch_by_hand,storage} may be): let the lamp rest and run step 4 again"
  rec "FACTORY_4C4=PASS" "RESULT_SELFTEST=PASS" "RESULT_SELFTEST_AT=$(date '+%F %T %z')"
  if [[ ${RESET:-no} != yes ]]; then
    status "PASS selftest (not reset: RESET=yes resets it for packing)"; exit 0
  fi
  # As steps.sh resets after a passed step 4: the lamp stopped first and each
  # part waited on (a part writing as it stops wrote back into what the reset
  # had just emptied, lamp-29e7 on 26 Oct), the path unit first, then
  # lamp-setup's reset-now in a unit of its own, which reboots into setup.
  status "RUNNING reset"
  listed=$(systemctl show -p ConsistsOf --value lampos.target) || fail "the lamp's parts could not be read to stop them before the reset"
  read -ra lamp_parts <<<"$listed"
  systemctl stop lampos-setup-done.path && systemctl stop lampos.target ${lamp_parts[@]+"${lamp_parts[@]}"} \
    || fail "the lamp could not be stopped before the reset"
  rec "RESET=started $(date '+%F %T %z')"
  status "PASS selftest, resetting"
  speak factory_resetting
  systemd-run --no-block --collect --unit=lamp-setup-wipe --property=Type=oneshot \
    /opt/lampOS/setup/current/lamp-setup reset-now --plan /etc/lampOS/setup/reset-plan.json --enable \
    || fail "could not start the reset: hold the button 5 s"
  exit 0
fi

# unset
status "RUNNING checks"
# SET_UP_OK=yes: a lamp set up from the app before its factory steps
# (lamp-ee17): steps.sh stops what holds the touch lines itself; lamp-body,
# failing without lamp.env, is stopped so the servo port is free.
if [[ -e /var/lib/lampos-setup/done ]]; then
  [[ ${SET_UP_OK:-} == yes ]] || fail "already set up: the unset run is for a lamp before the app setup (SET_UP_OK=yes runs it anyway)"
  systemctl stop lamp-body; systemctl reset-failed lamp-body 2>/dev/null
  rec "SET_UP_BEFORE_STEPS=yes"
fi
grep -q ^country_code /etc/hostapd/hostapd.conf && fail "old image (country_code in hostapd.conf)"
# The camera by its ids on any USB port: the product's eyes take it so since
# #4074 (lamp-dbda, a63e and cb94 have it on 3-1.3.4, the rest on 3-1.1.4).
cam=""; for d in /sys/bus/usb/devices/*; do
  [[ $(cat "$d/idVendor" 2>/dev/null):$(cat "$d/idProduct" 2>/dev/null) == 01da:5875 ]] && { cam=${d##*/}; break; }
done
# /home/orangepi/NO_CAMERA_OK: test the rest of a lamp whose camera is out (recorded, not passed)
if [[ -n $cam ]]; then rec "CAMERA_4B=PASS $cam 01da:5875"
elif [[ -e /home/orangepi/NO_CAMERA_OK ]]; then rec "CAMERA_4B=FAIL no camera, rest tested anyway"; say "WARN no camera (4b): testing the rest"
else fail "no camera 01da:5875 on any USB port (4b)"; fi
systemctl cat lamp-update | grep -q lampos-stable && rec "UPDATE_CHANNEL_4F=PASS lampos-stable" || fail "lamp-update is not on lampos-stable (4f)"
r=""; for u in bootstrap os-server hal; do systemctl is-active -q $u && r="$r $u"; done
[[ -z $r ]] && rec "V1_UNITS_4G=PASS none running" || fail "V1 units running:$r (4g)"

# The speaker's hardware level, kept across boots (75%, kyle-aus 3 Nov), and
# set first, before the inputs (4 Nov), so a run that fails on an input still
# leaves the level set. lamp-cmedia-speaker pins the C-Media "Speaker" at step 9 at every boot, so
# alsactl store cannot hold a level. A drop-in replaces that pin with
# SPEAKER_LEVEL and keeps the unit's other ExecStart lines (the first PCM
# open that makes the softvol control). Delete the drop-in for the default.
status "RUNNING speaker level"
SPEAKER_LEVEL=${SPEAKER_LEVEL:-75%}
SPK_UNIT=lamp-cmedia-speaker.service
SPK_DROPIN=/etc/systemd/system/$SPK_UNIT.d/zz-factory-speaker.conf
systemctl cat "$SPK_UNIT" >/dev/null 2>&1 || fail "no $SPK_UNIT: the speaker level cannot be kept"
mkdir -p "${SPK_DROPIN%/*}"
# At boot the unit can run before udev names the card device_cmedia
# ("Invalid card number", lamp-b626 3 Nov: the level stayed at the card's
# power-on 46%), so it waits up to 15 s for the name first.
{ echo "# factory-run.sh: the speaker at $SPEAKER_LEVEL at every boot instead of the unit's pin"
  echo "[Service]"
  echo 'ExecStartPre=/bin/sh -c "for i in $(seq 30); do [ -e /proc/asound/device_cmedia ] && exit 0; sleep 0.5; done; exit 0"'
  echo "ExecStart="
  echo "ExecStart=/usr/bin/amixer -q -c device_cmedia sset Speaker $SPEAKER_LEVEL"
  systemctl cat "$SPK_UNIT" | grep '^ExecStart=' | grep -v -e amixer -e '^ExecStart=$'
} >"$SPK_DROPIN"
systemctl daemon-reload
systemctl restart "$SPK_UNIT" || fail "$SPK_UNIT failed with the speaker at $SPEAKER_LEVEL"
spk=$(amixer -c device_cmedia sget Speaker | grep -o '\[[0-9]*%\]' | head -1 | tr -d '[]')
[[ -n $spk ]] || fail "the speaker level cannot be read after setting $SPEAKER_LEVEL"
rec "SPEAKER_LEVEL=$spk (asked $SPEAKER_LEVEL, $SPK_DROPIN)"

# 4c2 beside 4c1 (kyle-aus 4 Nov, ~70 s a lamp): the inputs are read while
# the arm stands, so the person touches the pads and presses the buttons
# during the stand. input-check.py's touch window opens at the first touch
# (it waits up to 300 s), so nobody races the arm. INPUTS_WITH_STAND=no reads
# them after the stand, as before. A run that fails first stops it with
# SIGINT, so its finally starts lamp-controls and the voice again.
INPUT_OUT=$DIR/input-check.out; input_pid=""
if [[ ${INPUTS_WITH_STAND:-yes} == yes ]]; then
  python3 "$HOME_DIR/input-check.py" >"$INPUT_OUT" 2>&1 & input_pid=$!
  # waited for: the unit ending kills what is left, its finally included
  trap '[[ -n $input_pid ]] && { kill -INT $input_pid 2>/dev/null; wait $input_pid 2>/dev/null; }' EXIT
  say "4c2 inputs read beside the arm stand: touch the pads and press the buttons now"
fi

if ! grep -q ^BODY_POSES= "$ENV_FILE" 2>/dev/null; then
  # The speaker step's daemon-reload has systemd try lamp-body again, which
  # then waits to restart ("activating", lamp-dbd1 4 Nov) and holds step 1
  # off the servo port: stopped again here.
  systemctl stop lamp-body 2>/dev/null; systemctl reset-failed lamp-body 2>/dev/null
  status "RUNNING head rest"
  rest_read() { setpriv --reuid lampos --regid lampos --init-groups -- /opt/lampOS/body/current/lamp-body-measure \
    --device /dev/ttyACM0 --unit-id "$ID" --lock-directory /run/lampos-locks/motor 2>&1 | grep -v ^BODY_POSES; }
  out=$(rest_read); echo "$out"
  if grep -q 'a head off its stop' <<<"$out"; then
    say "the head is off its stop: nudging it toward $NUDGE_TARGET"
    python3 "$HOME_DIR/head-nudge.py" --target "$NUDGE_TARGET"
    out=$(rest_read); echo "$out"
    grep -q 'a head off its stop' <<<"$out" && { speak factory_head_off; fail "the head rests off its stop after the nudge: press it down by hand"; }
  fi
  grep -q '^lamp-body-measure: ' <<<"$out" && fail "the rest read refused: $(grep '^lamp-body-measure:' <<<"$out" | head -1)"
  rec "HEAD_REST=$(grep -o 'pitch [0-9]*;' <<<"$out" | head -1 | tr -d ';')"

  status "RUNNING 4c1 arm stand"
  steps -- ./steps.sh --unit-id "$ID" --write --to 1 || fail "step 1 (the arm stand) stopped"
fi
grep -q ^BODY_POSES= "$ENV_FILE" || fail "step 1 wrote no lamp.env"
rec "$(grep ^BODY_POSES= "$ENV_FILE")" "FACTORY_4C1=PASS"

status "RUNNING 4c2 inputs"
if [[ -n $input_pid ]]; then
  wait "$input_pid"; rc=$?; input_pid=""; out=$(cat "$INPUT_OUT")
else
  out=$(python3 "$HOME_DIR/input-check.py" 2>&1); rc=$?
fi
echo "$out"
mapfile -t inputs < <(sed -n 's/^RECORD //p' <<<"$out")
rec "${inputs[@]}"
[[ $rc == 0 ]] || fail "inputs: $(sed -n 's/^RECORD INPUTS=//p' <<<"$out")"
rec "FACTORY_4C2=PASS"

status "RUNNING 4c3 update key"
steps -- ./steps.sh --unit-id "$ID" --write --from 3 --to 3 || fail "step 3 (the update key) stopped"
rec "FACTORY_4C3=PASS"

status "RUNNING 4d gaze"
grep -q -- --gaze "$ENV_FILE" || { cp -p "$ENV_FILE" "$ENV_FILE.before-4d" && sed -i "s/^\(BODY_POSES=\".*\)\"\$/\1 --gaze $GAZE\"/" "$ENV_FILE"; }
grep -q -- "--gaze $GAZE" "$ENV_FILE" || fail "4d: --gaze $GAZE is not in lamp.env"
jq -e '(.head_pads | length) >= 2 and (.head_rest | length) == (.head_pads | length)' "$PINS" >/dev/null || fail "the pins file has no head pads"
rec "$(grep ^BODY_POSES= "$ENV_FILE")" "FACTORY_4D=PASS $GAZE"

rec "RESULT_UNSET=PASS" "RESULT_UNSET_AT=$(date '+%F %T %z')"
status "PASS unset"
speak factory_all_done
