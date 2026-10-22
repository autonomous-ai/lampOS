#!/usr/bin/env bash
# The real person's labelled session for "Reads your mood", the face side
# (#1671, #2132; eyes.md, "The session's plan"): both of the eyes' readers
# side by side on the person at the lamp, their own labels from a key press,
# the reads and Mood's state kept for the session alone, then the measure. A
# ten-minute run once they arrive, with their yes. No camera frame is kept or
# taken by this script; the labels and the reads never go on GitHub, only the
# counts `read` prints.
#
#   lampOS/tools/seems-session.sh on                    # on the lamp, as root
#   lampOS/tools/seems-session.sh labels > labels.jsonl  # at the keyboard the person uses
#   lampOS/tools/seems-session.sh read labels.jsonl [stretches.tsv]   # on the lamp, as root
#   lampOS/tools/seems-session.sh off                   # on the lamp, as root
#
# on: lamp-eyes from its own unit with `--seems FERPLUS --mouth-model MESH
# --seems-blendshapes BLEND --seems-log` and `LAMP_SEEMS_BROW_LOW=on` (with
# FERPLUS unset, Blendshape alone: no `--seems`), and
# lamp-mood with `LAMP_MOOD=on` and `LAMP_MOOD_SEEMS=on`, each from a `/run`
# drop-in that only adds to the unit's own command line, so a reboot undoes
# it. `mood/state` is kept in /run/seems-session (0600) for `read`. A timer
# runs `off` after an hour if nobody does. Nobody else near the lamp: the
# readers read whoever is near (eyes.md, "Only for the session").
#
# labels: the stretches in order, STRETCH seconds each (180): resting and
# looking at the lamp (neutral), smiling (positive), a long face, as when
# tired or low (negative), reading or working at the desk (neutral: drawn-down
# brows from concentrating aren't low, the first builder's P3 on #2723), and
# talking (neutral). Each is said on stderr first; then, while the person
# does it, each press of Space or Enter is one label, the stretch's own; p, n
# or u says positive, negative or neutral instead, as they feel it; s ends the
# stretch early, q the session. Prints one label a line, `{"t": unix_s,
# "says": ...}`, and writes when each stretch began and ended to STRETCHES
# (stretches.tsv): times and names only. The two machines must keep time
# (NTP).
#
# read: as root on the lamp, after `labels`. Prints, for each stretch, each
# reader's reads by kind and how many smiles and quiets Mood took (from
# `mood/state`: warmth up for a smile, energy down for a quiet), then
# `lamp-seems-measure SEEMS_LOG LABELS --delete`: for each
# reader and confidence, the reads judged and wrong, and whether each is
# under the bar (wrong at most 1 in 10). A session `on` without FERPLUS
# prints Blendshape's rows only. The measure removes the reads and
# the labels; the copy read here and Mood's state go too. Counts only.
#
# off: the drop-ins and the tap removed, the units back on their own
# command lines, and anything of the session's left removed.
#
# Environment: FERPLUS (FER+'s model, optional: it isn't shipped; without it
# `on` runs Blendshape alone),
# MESH and BLEND (/opt/lampOS/eyes/models/face_mesh_v2.onnx and
# face_blendshapes.onnx), BUS (lamp-bus), MEASURE (lamp-seems-measure),
# STRETCH (180), STRETCHES (stretches.tsv), SEEMS_LOG
# (/var/lib/lampos-eyes/seems.log), DIR (/run/seems-session), SYSTEMCTL
# (systemctl), DROPINS (/run/systemd/system).
set -euo pipefail

STRETCH=${STRETCH:-180}
STRETCHES=${STRETCHES:-stretches.tsv}
SEEMS_LOG=${SEEMS_LOG:-/var/lib/lampos-eyes/seems.log}
DIR=${DIR:-/run/seems-session}
SYSTEMCTL=${SYSTEMCTL:-systemctl}
DROPINS=${DROPINS:-/run/systemd/system}
MESH=${MESH:-/opt/lampOS/eyes/models/face_mesh_v2.onnx}
BLEND=${BLEND:-/opt/lampOS/eyes/models/face_blendshapes.onnx}
MEASURE=${MEASURE:-$(command -v lamp-seems-measure || echo /opt/lampOS/eyes/current/lamp-seems-measure)}
BUS=${BUS:-$(command -v lamp-bus || echo /opt/lampOS/bus/current/lamp-bus)}

# The unit's own command line, with `extra` added: a drop-in that clears
# ExecStart and gives it again, so nothing of the unit's is lost.
dropin() { # $1: unit, $2: extra arguments, $3: environment (KEY=value ...)
  local exec
  exec=$("$SYSTEMCTL" cat "$1" | grep '^ExecStart=' | tail -1 | sed 's/^ExecStart=//')
  [ -n "$exec" ] || { echo "seems-session: $1 has no ExecStart" >&2; exit 2; }
  printf '[Service]\nExecStart=\nExecStart=%s%s\n' "$exec" "${2:+ $2}"
  [ -z "${3:-}" ] || printf 'Environment=%s\n' "$3"
}

on() {
  local model fer=
  for model in ${FERPLUS:+"$FERPLUS"} "$MESH" "$BLEND"; do
    [ -r "$model" ] || { echo "seems-session: cannot read $model" >&2; exit 2; }
  done
  install -d -m 0700 "$DIR"
  # Without FER+ (not shipped, #811), Blendshape alone; `read` then prints
  # its rows only.
  if [ -n "${FERPLUS:-}" ]; then fer="--seems $FERPLUS "; rm -f "$DIR/no-fer-plus"; else : >"$DIR/no-fer-plus"; fi
  "$SYSTEMCTL" is-active lamp-mood.service >"$DIR/mood-was" || true
  install -d "$DROPINS/lamp-eyes.service.d" "$DROPINS/lamp-mood.service.d"
  dropin lamp-eyes.service "$fer--mouth-model $MESH --seems-blendshapes $BLEND --seems-log $SEEMS_LOG" \
    "LAMP_SEEMS_BROW_LOW=on" >"$DROPINS/lamp-eyes.service.d/zz-seems-session.conf"
  dropin lamp-mood.service "" "LAMP_MOOD=on LAMP_MOOD_SEEMS=on" \
    >"$DROPINS/lamp-mood.service.d/zz-seems-session.conf"
  "$SYSTEMCTL" daemon-reload
  "$SYSTEMCTL" restart lamp-eyes.service lamp-mood.service
  systemd-run --quiet --unit=seems-session-mood --uid=lampos --gid=lampos \
    -p RuntimeMaxSec=1h -p "StandardOutput=file:$DIR/mood.txt" \
    "$BUS" --root /run/lampos-bus tap mood/state --count 1000000
  systemd-run --quiet --unit=seems-session-off --on-active=1h \
    /bin/bash "$(readlink -f "$0")" off
  echo "seems-session: on; eyes $("$SYSTEMCTL" is-active lamp-eyes.service), mood $("$SYSTEMCTL" is-active lamp-mood.service)" >&2
}

off() {
  "$SYSTEMCTL" stop seems-session-mood.service seems-session-off.timer 2>/dev/null || true
  rm -f "$DROPINS/lamp-eyes.service.d/zz-seems-session.conf" "$DROPINS/lamp-mood.service.d/zz-seems-session.conf"
  rmdir "$DROPINS/lamp-eyes.service.d" "$DROPINS/lamp-mood.service.d" 2>/dev/null || true
  "$SYSTEMCTL" daemon-reload
  "$SYSTEMCTL" restart lamp-eyes.service
  # Mood as it was: running, or stopped as on a lamp without LAMP_MOOD.
  if [ "$(cat "$DIR/mood-was" 2>/dev/null)" = active ]; then
    "$SYSTEMCTL" restart lamp-mood.service
  else
    "$SYSTEMCTL" stop lamp-mood.service
  fi
  rm -f "$SEEMS_LOG"
  rm -rf "$DIR"
  echo "seems-session: off" >&2
}

labels() {
  local name says key now ended stretch status
  : >"$STRETCHES"
  # The list on its own descriptor: the keys are stdin's.
  while IFS='|' read -r -u 3 name says; do
    echo "seems-session: next, for $STRETCH s: $name. Press Space or Enter while you do it (p, n or u for how you feel instead; s next, q stop)." >&2
    now=$(date +%s)
    ended=$((now + STRETCH))
    stretch=$name
    while [ "$(date +%s)" -lt "$ended" ]; do
      key=
      status=0
      IFS= read -rsn1 -t 1 key || status=$?
      # A second with no key; at the input's end, wait out the stretch.
      if [ "$status" -gt 128 ]; then continue; fi
      if [ "$status" -ne 0 ]; then sleep 1; continue; fi
      case "$key" in
        "" | " ") printf '{"t": %s, "says": "%s"}\n' "$(date +%s)" "$says" ;;
        p) printf '{"t": %s, "says": "positive"}\n' "$(date +%s)" ;;
        n) printf '{"t": %s, "says": "negative"}\n' "$(date +%s)" ;;
        u) printf '{"t": %s, "says": "neutral"}\n' "$(date +%s)" ;;
        s) break ;;
        q) printf '%s\t%s\t%s\n' "$stretch" "$now" "$(date +%s)" >>"$STRETCHES"; return 0 ;;
      esac
    done
    printf '%s\t%s\t%s\n' "$stretch" "$now" "$(date +%s)" >>"$STRETCHES"
  done 3<<'LIST'
resting, looking at the lamp|neutral
smiling|positive
a long face, as when tired or low|negative
reading or working at the desk, concentrating|neutral
talking, as to someone at the desk|neutral
LIST
}

read_session() {
  local labels=$1 stretches=${2:-$STRETCHES} readers=fer_plus,blendshapes
  [ ! -e "$DIR/no-fer-plus" ] || readers=blendshapes
  for file in "$labels" "$SEEMS_LOG"; do
    [ -r "$file" ] || { echo "seems-session: cannot read $file" >&2; exit 2; }
  done
  install -d -m 0700 "$DIR"
  cp "$SEEMS_LOG" "$DIR/reads.jsonl"
  python3 -I - "$DIR/reads.jsonl" "$DIR/mood.txt" "$stretches" "$readers" <<'PY'
import json, os, sys
reads_path, mood_path, stretches_path, readers = sys.argv[1:5]
reads = []
for line in open(reads_path, encoding="utf-8", errors="replace"):
    try:
        read = json.loads(line)
    except ValueError:
        continue
    reads.append((read.get("t"), read.get("from", "fer_plus"), read.get("kind")))
moods = []
if os.path.exists(mood_path):
    decoder = json.JSONDecoder()
    for line in open(mood_path, encoding="utf-8", errors="replace"):
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            header, end = decoder.raw_decode(line)
            body, _ = decoder.raw_decode(line[end:].lstrip())
        except ValueError:
            continue
        if isinstance(header.get("at_unix_ms"), (int, float)) and all(
            isinstance(body.get(key), (int, float)) for key in ("energy", "warmth")
        ):
            moods.append((header["at_unix_ms"] / 1000.0, body["energy"], body["warmth"]))
stretches = []
if os.path.exists(stretches_path):
    for row in open(stretches_path, encoding="utf-8").read().splitlines():
        fields = row.split("\t")
        if len(fields) == 3:
            stretches.append((fields[0], float(fields[1]), float(fields[2])))
print("stretch\treader\tpositive\tnegative\tunsure\tmood_smiles\tmood_quiets")
for name, begun, ended in stretches:
    # Mood's smiles and quiets from one state to the next: a smile adds
    # warmth (0.03 at rest) and a quiet takes energy (0.10); the energy
    # coming back after a quiet is neither.
    smiles = quiets = 0
    within = [(energy, warmth) for t, energy, warmth in moods if begun <= t <= ended]
    for (energy_was, warmth_was), (energy, warmth) in zip(within, within[1:]):
        smiles += warmth - warmth_was >= 0.02
        quiets += energy_was - energy >= 0.05
    for reader in readers.split(","):
        kinds = [kind for t, by, kind in reads if by == reader and isinstance(t, (int, float)) and begun <= t <= ended]
        print(f"{name}\t{reader}\t{kinds.count('positive')}\t{kinds.count('negative')}\t{kinds.count('unsure')}\t{smiles}\t{quiets}")
PY
  rm -f "$DIR/reads.jsonl" "$DIR/mood.txt" "$DIR/no-fer-plus"
  # The measure always prints both readers; FER+'s rows are dropped when it
  # didn't run.
  if [ "$readers" = blendshapes ]; then
    "$MEASURE" "$SEEMS_LOG" "$labels" --delete | grep -v '^fer_plus'
  else
    "$MEASURE" "$SEEMS_LOG" "$labels" --delete
  fi
}

case "${1:-}" in
  on) on ;;
  off) off ;;
  labels) labels ;;
  read) shift; read_session "${1:?seems-session: read LABELS [stretches.tsv]}" "${2:-}" ;;
  *) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 2 ;;
esac
