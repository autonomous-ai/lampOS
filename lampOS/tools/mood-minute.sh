#!/usr/bin/env bash
# Mood's lamp minute (#1670, docs/mood.md "Mood on a lamp"): the head's idle
# breath with Mood off, then with Mood on, 60 s each, and whether it keeps in
# step with Mood's one breath. Run on the lamp as root, the person away from
# the arm:
#
#   sudo MOOD_BIN=/opt/lampOS/mood/current/lamp-mood lampOS/tools/mood-minute.sh [--clap] [--stop-at S]
#
# --clap      half way through the on minute, says "clap now" for a person to clap
#             once (a sudden sound): Mood's pace should rise over 1.1 for some
#             seconds, the head quickening with it.
# --stop-at S stops Mood S seconds into the on minute: the head goes back to
#             the plain breath within Mood's 2.5 s.
#
# Mood runs as a transient unit, lamp-mood-check, as RUN_AS (the bus's user),
# with LAMP_MOOD=on; nothing is installed and on any exit it is stopped. It
# prints counts and numbers only: the head's period, its fit to the breath
# (R, from 0 to 1), mood/state's checks and each part's CPU.
#
# The numbers are lamp-voice-eval's (`mood-minute DIR`, Rust, reading the taps
# this script writes): the shell only runs the minute.
#
# Environment: MOOD_BIN (required), BUS (lamp-bus, from PATH), EVAL
# (lamp-voice-eval, from PATH), BUS_ROOT
# (/run/lampos-bus), RUN_AS (orangepi), SECONDS_EACH (60), OUT (a new
# temporary folder), CGROUP_ROOT (/sys/fs/cgroup), SETTLE (5 s for Mood to
# start); SYSTEMCTL and SYSTEMD_RUN
# for the tests' stand-ins.
set -euo pipefail

BUS=${BUS:-lamp-bus}
EVAL=${EVAL:-lamp-voice-eval}
BUS_ROOT=${BUS_ROOT:-/run/lampos-bus}
RUN_AS=${RUN_AS:-orangepi}
SECONDS_EACH=${SECONDS_EACH:-60}
CGROUP_ROOT=${CGROUP_ROOT:-/sys/fs/cgroup}
SYSTEMCTL=${SYSTEMCTL:-systemctl}
SYSTEMD_RUN=${SYSTEMD_RUN:-systemd-run}
UNIT=lamp-mood-check
CLAP=0 STOP_AT=
while [[ $# -gt 0 ]]; do
  case $1 in
    --clap) CLAP=1 ;;
    --stop-at) STOP_AT=${2:?--stop-at needs seconds}; shift ;;
    *) echo "usage: mood-minute.sh [--clap] [--stop-at S]" >&2; exit 2 ;;
  esac
  shift
done
[[ -n ${MOOD_BIN:-} && -x $MOOD_BIN ]] || { echo "MOOD_BIN must name lamp-mood's binary" >&2; exit 2; }
command -v "$EVAL" >/dev/null || { echo "EVAL must name lamp-voice-eval's binary" >&2; exit 2; }
if [[ -n $STOP_AT ]] && ! [[ $STOP_AT =~ ^[0-9]+$ && $STOP_AT -lt $SECONDS_EACH ]]; then
  echo "--stop-at is whole seconds under $SECONDS_EACH" >&2
  exit 2
fi
for part in lamp-voice lamp-expression; do
  "$SYSTEMCTL" is-active -q "$part.service" || { echo "$part is not active; nothing done" >&2; exit 3; }
done
if "$SYSTEMCTL" is-active -q "$UNIT.service" || "$SYSTEMCTL" is-active -q lamp-mood.service; then
  echo "Mood already runs; nothing done" >&2
  exit 3
fi
OUT=${OUT:-$(mktemp -d)}
mkdir -p "$OUT"

stop_mood() { "$SYSTEMCTL" stop "$UNIT.service" >/dev/null 2>&1 || true; }
trap stop_mood EXIT

cpu() { # cpu UNIT: its cgroup's usage_usec, or 0
  local group
  group=$("$SYSTEMCTL" show -p ControlGroup --value "$1.service" 2>/dev/null || true)
  awk '/^usage_usec/{print $2; found=1} END{if(!found) print 0}' "$CGROUP_ROOT$group/cpu.stat" 2>/dev/null || echo 0
}

tap() { # tap TOPIC FILE
  LAMP_BUS_ROOT=$BUS_ROOT timeout $((SECONDS_EACH + 5)) "$BUS" tap "$1" --name "mood_minute_${1//\//_}" --seconds "$SECONDS_EACH" >"$2" 2>/dev/null &
}

minute() { # minute LABEL
  local label=$1 v0 e0 m0
  v0=$(cpu lamp-voice) e0=$(cpu lamp-expression) m0=$(cpu "$UNIT")
  tap expression/body "$OUT/$label-body.jsonl"
  [[ $label == on ]] && tap mood/state "$OUT/$label-mood.jsonl"
  local waited=0
  while [[ $waited -lt $SECONDS_EACH ]]; do
    sleep 1
    waited=$((waited + 1))
    if [[ $label == on && $CLAP == 1 && $waited == $((SECONDS_EACH / 2)) ]]; then
      echo "clap now, once"
      echo "$waited" >"$OUT/on-clap-at"
    fi
    if [[ $label == on && -n $STOP_AT && $waited == "$STOP_AT" ]]; then
      stop_mood
      echo "$waited" >"$OUT/on-stopped-at"
    fi
  done
  wait
  echo "$label cpu_ms voice=$((($(cpu lamp-voice) - v0) / 1000)) expression=$((($(cpu lamp-expression) - e0) / 1000)) mood=$((($(cpu "$UNIT") - m0) / 1000))"
}

echo "mood-minute: $SECONDS_EACH s with Mood off, then $SECONDS_EACH s with Mood on"
minute off
"$SYSTEMD_RUN" --quiet --unit="$UNIT" --uid="$RUN_AS" --gid="$RUN_AS" \
  -p NoNewPrivileges=yes -p ProtectSystem=strict -p ReadWritePaths="$BUS_ROOT" \
  -E LAMP_MOOD=on "$MOOD_BIN" serve --zone system --bus-root "$BUS_ROOT"
sleep "${SETTLE:-5}"
"$SYSTEMCTL" is-active -q "$UNIT.service" || { echo "lamp-mood did not stay up" >&2; exit 4; }
minute on
stop_mood
"$EVAL" mood-minute "$OUT"
echo "mood-minute: done; Mood stopped, nothing installed"
