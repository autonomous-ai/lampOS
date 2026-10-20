#!/usr/bin/env bash
# The joys' scores on a lamp, with nothing moving (#1669; the first builder on
# #1722, 20 Oct 03:24): how `ears/classes` scores an empty room, a laugh or a
# music track and a person's line over it, against Laugh glance's and Music
# sway's bars, and the glances or sways each skill would have asked for. Run on
# the lamp as root, with someone at the Mac to play the tracks:
#
#   sudo EARS_MODEL=/opt/lampOS/models/yamnet/yamnet.onnx lampOS/tools/joys-scores.sh laugh|music [--with-privacy-controls|--no-slide]
#
# Nothing moves: the skill is never installed. The bus is recorded
# (`ears/classes`, `voice/state`, `humans/faces/tracked`) while the sound
# plays, and the recording is then replayed to the skill offline
# (`lamp-skills test`) at noon, outside its quiet hours, to count what it would
# have asked. lamp-ears runs with `LAMP_EARS_CLASSES=on` and EARS_MODEL for the
# run only, in a drop-in under /run, removed on any exit and lamp-ears
# restarted.
#
# The scorer runs only on the privacy route, `LAMP_PRIVACY_CONTROLS=on`, which
# Controls, Voice and Ears take together (privacy-switch.md): Ears alone on it
# gets no lease from the voice and scores nothing. Without the flag it needs
# the route on the lamp already, and changes no privacy setting. With
# `--with-privacy-controls` it turns the route on in all three for the run, in
# the same /run drop-ins, removed on any exit and the three restarted: the
# route is the one the slide closes both microphones on, so it adds controls,
# but whether a lamp runs on it is the first builder's call (switches.md).
#
# A lamp with no slide (lamp-4ace) takes `--no-slide` instead: the route
# `LAMP_PRIVACY_CONTROLS=no-slide` in the same three drop-ins, on which Ears
# scores only with the person's stored yes to room sounds (privacy-switch.md).
# The tool never writes that yes: on `no-slide`, with the flag or already on
# the lamp, it runs only when the person's own yes is in ROOM_SOUNDS, given in
# setup, and otherwise touches nothing (a no leaves no file, as "never asked"
# does; the reviewer's P2 on #2113). A yes lamp-ears can't read scores
# nothing, and the 20 s check below says so.
#
# The model: Qualcomm AI Hub's YAMNet ONNX float export, release v0.59.0
# (`qai-hub-models/models/yamnet/releases/v0.59.0/yamnet-onnx-float.zip`):
# `yamnet.onnx` 25,789 bytes, SHA-256
# 970ea63f4dc1aa33c86ef7d61d854319fdfe6bdd078f5b3d674ee1db5dd96471, and
# `yamnet.data` beside it, 14,915,108 bytes, SHA-256
# 1d6dff488da4712190e10f1c841252aa58011381132d0126dcd5ee56a4e7ed31, pinned in
# `crates/ears/src/classes.rs`. Neither is in the repository or the image
# (its distribution review is open): copied to the lamp by hand, checked
# against these, under /opt/lampOS/models/yamnet/. A wrong file publishes
# `invalid_model`, and this script stops after 20 s with nothing measured.
#
# The recording (scores, face boxes and the voice's state; no sound, no
# picture) is deleted at the end, whatever happens. It prints counts and
# numbers only.
#
# The sound is the person's to choose, free to use, its source noted beside
# the numbers posted: a laugh track and a music track, each at least 2 min.
#
# The lamp's build must have `ears/classes` (#1923) and a lamp-skills that
# reads a dotted field (`scores.laughter`, #1920), with the skill's folder:
# all three are on #1712's branch, none yet on lamp-voice.
#
# Environment: EARS_MODEL (required), SKILL_DIR (../skills/laugh-glance or
# ../skills/music-sway beside this), BUS (lamp-bus, from PATH), SKILLS
# (lamp-skills, from PATH), BUS_ROOT (/run/lampos-bus), UNIT_DIR
# (/run/systemd/system), ROOM_SOUNDS
# (/var/lib/lampOS/consent/room-sounds.json), QUIET_S (120), TRACK_S (120), OVER_S (30),
# SYSTEMCTL (systemctl); and for the tests, NOW_FILE (a file holding the wall
# clock in ms) and SLEEP (sleep). Its own work is always a new folder from
# mktemp, never one given, since it is deleted at the end.
set -euo pipefail

ROW=${1:-}
USAGE="usage: joys-scores.sh laugh|music [--with-privacy-controls|--no-slide]"
ROUTE=
case ${2:-} in
  "") ;;
  --with-privacy-controls) ROUTE=on ;;
  --no-slide) ROUTE=no-slide ;;
  *) echo "$USAGE" >&2; exit 2 ;;
esac
case $ROW in
  laugh) NAME=laugh-glance FIELD=laughter BAR=0.5 HOLD_S=1 TRACK="the laugh track" ASK=body.gaze ;;
  music) NAME=music-sway FIELD=music BAR=0.6 HOLD_S=20 TRACK="the music track" ASK=body.move ;;
  *) echo "$USAGE" >&2; exit 2 ;;
esac
HERE=$(cd "$(dirname "$0")" && pwd)
SKILL_DIR=${SKILL_DIR:-$HERE/../skills/$NAME}
BUS=${BUS:-lamp-bus}
SKILLS=${SKILLS:-lamp-skills}
BUS_ROOT=${BUS_ROOT:-/run/lampos-bus}
UNIT_DIR=${UNIT_DIR:-/run/systemd/system}
ROOM_SOUNDS=${ROOM_SOUNDS:-/var/lib/lampOS/consent/room-sounds.json}
QUIET_S=${QUIET_S:-120} TRACK_S=${TRACK_S:-120} OVER_S=${OVER_S:-30}
SYSTEMCTL=${SYSTEMCTL:-systemctl}
SLEEP=${SLEEP:-sleep}
now_ms() { if [[ -n ${NOW_FILE:-} ]]; then cat "$NOW_FILE"; else echo $(($(date +%s%N) / 1000000)); fi; }

[[ -n ${EARS_MODEL:-} && -f $EARS_MODEL ]] || { echo "EARS_MODEL must name the pinned yamnet.onnx" >&2; exit 2; }
[[ -f $SKILL_DIR/skill.toml ]] || { echo "no $NAME skill at $SKILL_DIR (SKILL_DIR)" >&2; exit 2; }
command -v "$SKILLS" >/dev/null || { echo "SKILLS must name lamp-skills" >&2; exit 2; }
# The replay reads the skill's fields as this lamp-skills does: one that
# can't pass the skill's own recorded cases (a dotted field before #1920)
# would count nothing and seem to answer (the reviewer on #2039).
own=$("$SKILLS" test "$SKILL_DIR" 2>&1) && grep -q ' cases pass' <<<"$own" ||
  { echo "$SKILLS does not pass $NAME's own cases, so its replay would count nothing: $own" >&2; exit 2; }
"$SYSTEMCTL" is-active -q lamp-ears.service || { echo "lamp-ears is not active; nothing done" >&2; exit 3; }
# The units restarted for the run, in this order: Ears alone, or the three
# that take the privacy route together.
UNITS=lamp-ears
SET_ROUTE=$ROUTE
if [[ -n $ROUTE ]]; then
  UNITS="lamp-controls lamp-voice lamp-ears"
else
  ROUTE=$("$SYSTEMCTL" show -p Environment --value lamp-ears.service | tr ' ' '\n' | grep -oE '^LAMP_PRIVACY_CONTROLS=(on|no-slide)$' | tail -1 | cut -d= -f2 || true)
  [[ -n $ROUTE ]] || {
    echo "lamp-ears runs without LAMP_PRIVACY_CONTROLS=on or no-slide, which ears/classes needs (with Controls and Voice); nothing done. --with-privacy-controls (a lamp with a slide) or --no-slide turns the route on in all three for the run" >&2
    exit 3
  }
fi
if [[ $ROUTE == no-slide ]] && ! grep -q '"room_sounds": *"yes"' "$ROOM_SOUNDS" 2>/dev/null; then
  echo "no yes to room sounds in $ROOM_SOUNDS: on a lamp with no slide the person at it says yes in setup first; nothing done" >&2
  exit 3
fi

OUT=$(mktemp -d)
DROPINS=
RECORDER=
put_back() {
  [[ -n $RECORDER ]] && kill "$RECORDER" 2>/dev/null || true
  if [[ -n $DROPINS ]]; then
    # shellcheck disable=SC2086 # the drop-ins' paths have no spaces
    rm -f $DROPINS
    "$SYSTEMCTL" daemon-reload
    for unit in $UNITS; do "$SYSTEMCTL" restart "$unit.service"; done
  fi
  rm -rf "$OUT"
}
trap put_back EXIT
trap 'exit 130' INT TERM

for unit in $UNITS; do
  dropin=$UNIT_DIR/$unit.service.d/zz-joys.conf
  mkdir -p "$(dirname "$dropin")"
  DROPINS="$DROPINS $dropin"
  {
    echo '[Service]'
    [[ -n $SET_ROUTE ]] && echo "Environment=LAMP_PRIVACY_CONTROLS=$SET_ROUTE"
    [[ $unit == lamp-ears ]] && printf 'Environment=LAMP_EARS_CLASSES=on\nEnvironment=LAMP_EARS_CLASSES_MODEL=%s\n' "$EARS_MODEL"
    true
  } >"$dropin"
done
"$SYSTEMCTL" daemon-reload
for unit in $UNITS; do "$SYSTEMCTL" restart "$unit.service"; done
scored=0
for _ in $(seq 20); do
  if LAMP_BUS_ROOT=$BUS_ROOT "$BUS" latest ears/classes 2>/dev/null | grep -q '"scored"'; then scored=1; break; fi
  "$SLEEP" 1
done
[[ $scored == 1 ]] || { echo "lamp-ears says no scores on ears/classes in 20 s (the model, or privacy$([[ $ROUTE == no-slide ]] && echo ", or a yes in $ROOM_SOUNDS lamp-ears can't read")); nothing measured" >&2; exit 4; }

TOTAL=$((QUIET_S + TRACK_S + OVER_S))
LAMP_BUS_ROOT=$BUS_ROOT "$BUS" record "$OUT/rec" ears/classes voice/state humans/faces/tracked --seconds $((TOTAL + 2)) >/dev/null 2>&1 &
RECORDER=$!
"$SLEEP" 1
echo "joys-scores $ROW: $QUIET_S s empty room, $TRACK_S s $TRACK, $OVER_S s a line over it"
T0=$(now_ms)
echo "PLAY: nothing; the room empty and quiet, $QUIET_S s"
"$SLEEP" "$QUIET_S"
T1=$(now_ms)
echo "PLAY: $TRACK now, at a room's level, for $TRACK_S s, and keep it playing"
"$SLEEP" "$TRACK_S"
T2=$(now_ms)
echo "SAY: one line to someone else in the room, over $TRACK, now (not to the lamp); then stop the track at the end"
"$SLEEP" "$OVER_S"
T3=$(now_ms)
wait "$RECORDER" 2>/dev/null || true
RECORDER=

# Each message as "at_unix_ms payload", its payload decoded.
decoded() { # decoded FILE
  [[ -f $1 ]] || return 0
  grep '"payload_b64"' "$1" | while IFS= read -r line; do
    local at payload
    at=$(grep -o '"at_unix_ms": *[0-9]*' <<<"$line" | grep -o '[0-9]*$')
    payload=$(grep -o '"payload_b64": *"[^"]*"' <<<"$line" | sed 's/.*"\([^"]*\)"$/\1/' | base64 -d 2>/dev/null || true)
    echo "$at $payload"
  done
}
phase() { # phase AT_MS: empty, track, over or after
  awk -v at="$1" -v t0="$T0" -v t1="$T1" -v t2="$T2" -v t3="$T3" 'BEGIN {
    print (at < t0 ? "before" : at < t1 ? "empty" : at < t2 ? "track" : at < t3 ? "over" : "after") }'
}

# The scores against the bar, each phase: messages scored, mean, highest,
# the share at or over the bar, and its longest unbroken spell over it.
decoded "$OUT/rec/ears_classes.jsonl" | while read -r at payload; do
  grep -q '"scored"' <<<"$payload" || { echo "$(phase "$at") $at -"; continue; }
  echo "$(phase "$at") $at $(grep -o "\"$FIELD\": *[0-9.eE+-]*" <<<"$payload" | grep -o '[0-9.eE+-]*$')"
done | awk -v bar="$BAR" -v field="$FIELD" '
  $1 == "before" || $1 == "after" { next }
  $3 == "-" { unscored[$1]++; next }
  { n[$1]++; sum[$1] += $3; if ($3 > top[$1]) top[$1] = $3
    if ($3 > bar) { over[$1]++; if (!since[$1]) since[$1] = $2; spell = $2 - since[$1] + 500; if (spell > long[$1]) long[$1] = spell }
    else since[$1] = 0 }
  END { split("empty track over", order, " ")
    for (i = 1; i <= 3; i++) { p = order[i]
      printf "%s %s: %d scored, %d unavailable, mean %.2f, highest %.2f, over %.1f %d%%, longest over %.1f s\n",
        p, field, n[p], unscored[p], n[p] ? sum[p] / n[p] : 0, top[p], bar, n[p] ? 100 * over[p] / n[p] : 0, long[p] / 1000 } }'

# The skill on the recording, at noon: what it would have asked, and when.
CASE=$OUT/skill/$(basename "$SKILL_DIR")
mkdir -p "$CASE/tests/run"
cp "$SKILL_DIR/skill.toml" "$SKILL_DIR/SKILL.md" "$CASE/"
[[ -d $SKILL_DIR/moves ]] && cp -r "$SKILL_DIR/moves" "$CASE/"
cp "$OUT"/rec/*.jsonl "$CASE/tests/run/" 2>/dev/null || true
printf 'clock = "12:00"\nnone = true\n' >"$CASE/tests/run/expect.toml"
FIRST=$(cat "$OUT"/rec/*.jsonl 2>/dev/null | grep -o '"at_unix_ms": *[0-9]*' | grep -o '[0-9]*$' | sort -n | head -1)
replayed=$("$SKILLS" test "$CASE" 2>&1 || true)
# Passed (nothing asked) or failed only by what it asked; anything else is
# the replay's own failure, said whole.
if ! grep -qE 'cases pass|made [0-9]+ calls, 0 expected' <<<"$replayed"; then
  echo "the skill could not be replayed: $replayed" >&2
  exit 5
fi
asks=$(grep -oE "\(([0-9]+), \(\"$ASK\"" <<<"$replayed" | grep -oE '[0-9]+' || true)
decoded "$OUT/rec/voice_state.jsonl" | awk '{ print $1, ($2 ~ /"state": *"speaking"/) ? 1 : 0 }' >"$OUT/speaking"
echo "$asks" | awk -v first="${FIRST:-0}" -v t0="$T0" -v t1="$T1" -v t2="$T2" -v t3="$T3" -v ask="$ASK" -v spoke="$OUT/speaking" '
  BEGIN { while ((getline line < spoke) > 0) { split(line, f, " "); n++; at[n] = f[1]; on[n] = f[2] } }
  NF { ms = first + $1 / 1000
    p = ms < t1 ? "empty" : ms < t2 ? "track" : ms < t3 ? "over" : "after"; asked[p]++
    speaking = 0; for (i = 1; i <= n; i++) if (at[i] != "" && at[i] <= ms) speaking = on[i]
    if (speaking) while_speaking++
    when = when sprintf(" %.1fs", (ms - t0) / 1000) }
  END { printf "%s asked: empty %d, track %d, over %d; while the voice spoke %d; at%s\n",
    ask, asked["empty"], asked["track"], asked["over"], while_speaking, when ? when : " none" }'
echo "joys-scores $ROW: done; nothing moved, the recording deleted, $UNITS put back"
