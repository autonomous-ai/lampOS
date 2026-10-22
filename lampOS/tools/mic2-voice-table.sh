#!/usr/bin/env bash
# The voice check's two bars on the second microphone's own sound (#3125;
# the first builder at 06:49 on Queue #3126, for the owner's question 4): a
# night-kit round played from this computer, recorded on the lamp from
# `mic2/audio`, cut by when each line was played and scored by
# `same_voice_pairs` as the voice's check prints and compares them. Run on the
# computer that plays the round, from a lampOS checkout.
#
#   mic2-voice-table.sh --ssh TARGET --model CAMPP.onnx --plays ROUND.json
#                       [--lamp-bus PATH] [--seconds 1800] [--stage DIR]
#                       -- COMMAND...
#
#   --ssh       where ssh reaches the lamp (an alias, or user@host)
#   --model     the voice's CAM++ here (LAMP_SAME_VOICE_MODEL; its sha256 is
#               printed, to check against docs/same-voice.md's pin)
#   --plays     the round's log that COMMAND writes: the night kit's
#               night/<round>.json, whose rows give start, end and voice
#   --lamp-bus  lamp-bus on the lamp (lamp-bus)
#   --seconds   the recording's hard end on the lamp (1800)
#   --stage     where on the lamp each run makes a folder of its own for its
#               recording, mic2-voice-table.XXXXXX (/dev/shm: memory, so a
#               reboot clears it too; the recording is at most 512 MiB)
#   ssh asks for the lamp's password once at most: every step shares one
#   connection.
#   COMMAND     plays the round here (the night kit, with its scenes)
#
# In order:
#   1. the lamp's clock against this one's, by `date` over ssh;
#   2. a folder of this run's own made under STAGE, and `lamp-bus record`
#      of mic2/audio into it started on the lamp, and seen to grow;
#   3. COMMAND, here;
#   4. the recorder stopped and the clocks read again, their offset the mean
#      of the two readings (the recording's clock minus the player's);
#   5. the recording copied here to a folder of its own, and removed from the
#      lamp; a line cut short by the stop is left out;
#   6. same_voice_pairs with --end-silence-ms 300, then 600.
# Numbers only are printed. Nothing on the lamp changes but this run's own
# folder, which is removed at the end with its recorder stopped, as is the copy
# here. If this computer loses the lamp first, the lamp removes the folder
# itself 10 min after the recorder's hard end; if the lamp restarts before
# that, the default stage, /dev/shm, is cleared with it. Another run's folder or
# recorder is never touched, and a run that stops before it has a folder
# removes nothing there. `same_voice_pairs`
# is built here once (cargo, release); SAME_VOICE_PAIRS names another binary.
set -uo pipefail

ssh_target='' model='' plays='' lamp_bus=lamp-bus seconds=1800 stage=/dev/shm
while [[ $# -gt 0 ]]; do
  case $1 in
    --ssh) ssh_target=$2; shift 2 ;;
    --model) model=$2; shift 2 ;;
    --plays) plays=$2; shift 2 ;;
    --lamp-bus) lamp_bus=$2; shift 2 ;;
    --seconds) seconds=$2; shift 2 ;;
    --stage) stage=$2; shift 2 ;;
    --) shift; break ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
if [[ -z $ssh_target || -z $model || -z $plays || $# -eq 0 ]]; then
  echo "usage: mic2-voice-table.sh --ssh TARGET --model CAMPP.onnx --plays ROUND.json [--lamp-bus PATH] [--seconds 1800] [--stage DIR] -- COMMAND..." >&2
  exit 2
fi
[[ -f $model ]] || { echo "no model at $model" >&2; exit 2; }
[[ $seconds =~ ^[0-9]+$ ]] || { echo "--seconds $seconds: a whole number" >&2; exit 2; }

here=$(cd "$(dirname "$0")/.." && pwd)
copy=$(mktemp -d)
# One ssh connection for every step, so a lamp reached by password asks for
# it once. Its socket lives in a short folder of its own: a Unix socket's
# address holds about 104 bytes, and a Mac's $TMPDIR is long.
control=$(mktemp -d /tmp/m2v.XXXXXX)
run_stage=''
ssh_options=(-o ControlMaster=auto -o "ControlPath=$control/%C" -o ControlPersist=120)
lamp() { ssh "${ssh_options[@]}" "$ssh_target" "$1"; }
cleanup() {
  # Only this run's own folder, once it has one: never another run's
  # recorder or folder (reviewer-two's P2 on #3154).
  if [[ -n $run_stage ]]; then
    lamp "[ -f '$run_stage/pid' ] && kill \$(cat '$run_stage/pid') 2>/dev/null; rm -rf '$run_stage'" >/dev/null 2>&1
  fi
  ssh "${ssh_options[@]}" -O exit "$ssh_target" >/dev/null 2>&1
  rm -rf "$copy" "$control"
}
trap cleanup EXIT INT TERM

now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time() * 1000'; }
# The lamp's clock minus this one's, from one ssh round trip.
offset_ms() {
  local before at after
  before=$(now_ms)
  at=$(lamp 'date +%s%3N') || return 1
  after=$(now_ms)
  echo $((at - (before + after) / 2))
}
sha() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

pairs=${SAME_VOICE_PAIRS:-}
if [[ -z $pairs ]]; then
  (cd "$here" && cargo build -q --release -p lamp-voice-eval --example same_voice_pairs) || exit 1
  pairs="$here/target/release/examples/same_voice_pairs"
fi
echo "model sha256 $(sha "$model")"

# The connection opens first, so a password typed is not timed as the clock.
lamp true || { echo "the lamp could not be reached" >&2; exit 1; }
first=$(offset_ms) || { echo "the lamp's clock could not be read" >&2; exit 1; }
run_stage=$(lamp "mktemp -d '$stage/mic2-voice-table.XXXXXX'") && [[ -n $run_stage ]] \
  || { run_stage=''; echo "no folder for the recording under $stage on the lamp" >&2; exit 1; }
# If this computer loses the lamp before its cleanup, the lamp removes the
# run's folder itself, 10 min after the recorder's own hard end (reviewer-
# three's P3 on #3154): no recording outlives the run by more than that.
lamp "nohup sh -c 'sleep $((seconds + 600)); rm -rf \"$run_stage\"' </dev/null >/dev/null 2>&1 &" \
  || { echo "the lamp's own removal could not be set" >&2; exit 1; }
lamp "set -e; nohup '$lamp_bus' record '$run_stage' mic2/audio --seconds $seconds --max-mb 512 </dev/null >'$run_stage/record.log' 2>&1 & echo \$! >'$run_stage/pid'" \
  || { echo "the recorder did not start" >&2; exit 1; }
sleep 3
lamp "test -s '$run_stage/mic2_audio.jsonl'" \
  || { echo "nothing recorded from mic2/audio after 3 s:" >&2; lamp "cat '$run_stage/record.log'" >&2; exit 1; }
echo "recording on the lamp; playing the round"

"$@"
played=$?
lamp "kill \$(cat '$run_stage/pid') 2>/dev/null; sleep 1"
second=$(offset_ms) || second=$first
offset=$(((first + second) / 2))
echo "round exit $played; offset ${offset} ms (readings $first and $second)"

scp -q "${ssh_options[@]}" "$ssh_target:$run_stage/mic2_audio.jsonl" "$copy/" || { echo "the recording could not be copied" >&2; exit 1; }
# Ownership ends only once the folder is gone; else cleanup tries again.
lamp "rm -rf '$run_stage'" && run_stage=''
recording="$copy/mic2_audio.jsonl"
# A kill can stop the recorder mid-line: leave out a last line without its end.
if [[ -n $(tail -c1 "$recording") ]]; then
  sed '$d' "$recording" >"$recording.whole" && mv "$recording.whole" "$recording"
  echo "a last line cut short by the stop is left out"
fi
[[ -f $plays ]] || { echo "no round log at $plays" >&2; exit 1; }

status=0
for silence in 300 600; do
  echo "--- end silence $silence ms"
  "$pairs" "$model" --bus "$recording" --plays "$plays" --offset-ms "$offset" --end-silence-ms "$silence" || status=1
done
exit $status
