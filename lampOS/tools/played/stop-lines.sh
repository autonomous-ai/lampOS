#!/usr/bin/env bash
# The lamp check for #4710's fixed lines on the second route: a stop always gets "Okay, I'll stop there."
# (LAMP_SPOKEN_STOP_ENDS), over the lamp's answer or just after it ended, and a cut-in that ended the answer with no
# words gets "Did someone say something?" (LAMP_CUT_IN_WORDLESS_ASKS). Run from the played kit:
#
#   LAMP_HOST=<lamp> lampOS/tools/played/stop-lines.sh LABEL [ROUNDS]
#
# - Each round, four episodes, each after the lamp has been quiet for 45 s:
#   over        "Hey Lamp, tell me a long story about a lighthouse keeper.", then "Stop." about 8 s into the story;
#   over-name   the same, then "Stop Lamp.";
#   after       "Hey Lamp, tell me a two-line poem about tea.", then "Stop." about 1 s after its answer stopped
#               playing: its turn's `turn_finished`, by the lamp's clock. Not its first
#               `speech_final_sample_retired`, which ends only one playback interval (the wait sound's, or
#               the answer's first sentence before the next came; docs/playback-occurrences.md): on 06c1 on
#               2 Nov "Stop." then fell over the poem itself, 6 of 6, and a bare word over the lamp is a yield
#               that resumes, so no request came;
#   wordless    the story, then a short system sound (STOP_LINES_SOUND, Funk) about 8 s in.
# - Lines are said with this Mac's `say` at 45% output volume (put back on exit), sounds played with `afplay`.
# - What is counted, from the voice's journal since the episode began: which condition fired ("a stop said over the
#   lamp's answer", "a stop said just after the lamp's answer", "a cut-in with no words ended the lamp's answer") and
#   for its request whether "first answer audio" followed (the line played) or "its fixed line could not be said".
#   Otherwise what the cut-in's request became: answered by the model, held, heard 0 words, or never begun (for the
#   wordless sound: a yield that resumed, which should stay silent).
# - What passes: over, over-name and after each fire their condition and play; wordless either fires and plays, or the
#   yield resumed and the story went on (no 0-word request left the lamp quiet).
#
# STOP_LINES_ENV, optional: space-separated NAME=VALUE pairs for lamp-voice (for example LAMP_CUT_IN_WORDLESS_ASKS=on),
# put on through a /run drop-in (zz-stop-lines-check.conf) and the voice restarted; on any exit the drop-in is removed
# and the voice restarted without it. Without it nothing on the lamp is changed: it measures the voice as it runs.
#
# It plays sound in the room, about 2 minutes an episode. Not between 12:00 and 14:00 on weekdays. Settings: as the
# kit's (README.md); LAMP_SSH and STOP_LINES_QUIET_GATE replace this kit's lamp_ssh and quiet_gate.sh, STOP_LINES_SAY
# and STOP_LINES_PLAY replace `say` and `afplay`, STOP_LINES_VOLUME replaces the volume's osascript (the stand-in
# test, stop-lines-test.sh); STOP_LINES_QUIET_MAX (600 s) is the longest wait for a quiet lamp, STOP_LINES_END_WAIT
# (40 s) the longest for the short answer's end.
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
cd "$HERE" || exit 9
# shellcheck source=settings.sh
. ./settings.sh
LAMP_SSH=${LAMP_SSH:-$HERE/lamp_ssh}
QUIET=${STOP_LINES_QUIET_GATE:-$HERE/quiet_gate.sh}
QUIET_MAX=${STOP_LINES_QUIET_MAX:-600}
SAY=${STOP_LINES_SAY:-/usr/bin/say}
PLAY=${STOP_LINES_PLAY:-/usr/bin/afplay}
VOLUME=${STOP_LINES_VOLUME:-/usr/bin/osascript}
SOUND=${STOP_LINES_SOUND:-/System/Library/Sounds/Funk.aiff}
RATE=${LAMP_SAY_RATE:-185}
VOICE=${STOP_LINES_VOICE:-Daniel}
OUT=$PLAYED_OUT/scenarios
DROPIN=/run/systemd/system/lamp-voice.service.d/zz-stop-lines-check.conf
LABEL=${1:-}
ROUNDS=${2:-1}
STORY="Hey Lamp, tell me a long story about a lighthouse keeper."
POEM="Hey Lamp, tell me a two-line poem about tea."

if [[ -z $LABEL || ! $ROUNDS =~ ^[1-9]$ ]]; then
  echo "usage: stop-lines.sh LABEL [ROUNDS, 1 to 9]" >&2
  exit 2
fi
for pair in ${STOP_LINES_ENV:-}; do
  [[ $pair =~ ^LAMP_[A-Z0-9_]+=[A-Za-z0-9._-]+$ ]] || { echo "stop-lines.sh: STOP_LINES_ENV: '$pair' is not LAMP_NAME=value" >&2; exit 2; }
done
if [[ $(date +%u) -le 5 && $((10#$(date +%H))) -ge 12 && $((10#$(date +%H))) -lt 14 && ${LAMP_PLAY_AT_LUNCH:-} != yes ]]; then
  echo "stop-lines.sh: not between 12:00 and 14:00 local, when the office rests" >&2
  exit 2
fi
[[ -f $SOUND ]] || { echo "stop-lines.sh: no sound at $SOUND" >&2; exit 2; }

lamp() { "$LAMP_SSH" "$@" </dev/null; }

unit=$(lamp 'systemctl show -p ActiveState -p Environment -p ExecStart lamp-voice') \
  || { echo "stop-lines.sh: no answer from the lamp" >&2; exit 3; }
[[ $unit == *ActiveState=active* ]] || { echo "stop-lines.sh: lamp-voice is not active; nothing changed" >&2; exit 3; }
bin=$(sed -n 's/.*ExecStart={ path=\([^ ;]*\).*/\1/p' <<<"$unit" | head -n 1)

mkdir -p "$OUT"
work=$(mktemp -d)
changed=no
volume_before=$("$VOLUME" -e 'output volume of (get volume settings)' 2>/dev/null)
put_back() { # on every exit: the Mac's volume, then the drop-in; a failed put-back makes a clean exit 6
  local rc=$?
  rm -rf "$work"
  [[ -n $volume_before ]] && "$VOLUME" -e "set volume output volume $volume_before" >/dev/null 2>&1
  if [[ $changed == yes ]]; then
    if lamp "sudo -n rm -f $DROPIN && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-voice"; then
      echo "stop-lines.sh: drop-in removed, lamp-voice restarted without it"
    else
      echo "stop-lines.sh: putting lamp-voice back FAILED: remove $DROPIN, daemon-reload and restart lamp-voice by hand" >&2
      ((rc)) || rc=6
    fi
  fi
  exit "$rc"
}
trap put_back EXIT
trap 'exit 130' INT TERM HUP

if [[ -n ${STOP_LINES_ENV:-} ]]; then
  changed=yes
  lines=$(for pair in $STOP_LINES_ENV; do printf 'Environment=%s\\n' "$pair"; done)
  lamp "sudo -n mkdir -p ${DROPIN%/*} && printf '[Service]\\n$lines' | sudo -n tee $DROPIN >/dev/null && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-voice" \
    || { echo "stop-lines.sh: the drop-in could not be put on (sudo -n)" >&2; exit 4; }
  echo "stop-lines.sh: lamp-voice restarted with $STOP_LINES_ENV"
  unit=$(lamp 'systemctl show -p ActiveState -p Environment -p ExecStart lamp-voice') || unit=""
fi
"$VOLUME" -e 'set volume output volume 45' >/dev/null 2>&1 || echo "stop-lines.sh: the volume could not be set to 45%" >&2

report=$OUT/stop-lines-$LABEL.txt
tally=$work/tally.tsv
: >"$report"
: >"$tally"
{
  echo "stop-lines $LABEL  $(date -u +%Y-%m-%dT%H:%M:%SZ)  lamp $LAMP_HOST"
  echo "build: ${bin:-binary not found} sha256 $(lamp "sudo -n sha256sum ${bin:-/nonexistent} 2>/dev/null" | cut -c1-16)"
  echo "flags: $(grep -o 'LAMP_\(SPOKEN_STOP_ENDS\|CUT_IN_WORDLESS_ASKS\|VOICE_ROUTE\)=[^ ]*' <<<"$unit" | sort -u | tr '\n' ' ')"
} | tee -a "$report"

quiet() { # until the lamp has heard and said nothing for 45 s (a voice just restarted counts as busy)
  local waited=0
  until "$QUIET" 45 >/dev/null 2>&1; do
    ((waited >= QUIET_MAX)) && { echo "stop-lines.sh: the lamp was not quiet for 45 s within ${QUIET_MAX} s; stopped" >&2; exit 5; }
    sleep 10
    waited=$((waited + 10))
  done
}

say_line() { # TEXT: said from this Mac's speaker, made once and kept
  local f
  f="$PLAYED_OUT/prompts/generated/stop-lines-$(printf '%s' "$VOICE|$RATE|$1" | shasum -a 256 | cut -c1-16).aiff"
  [[ -s $f ]] || "$SAY" -v "$VOICE" -r "$RATE" -o "$f" "$1"
  echo "  $(date +%H:%M:%S) PLAY $1" >>"$report"
  "$PLAY" "$f"
}

# The session's first writes and turns' ends, then the lamp's monotonic clock as "now US".
# shellcheck disable=SC2016 # expanded on the lamp, not here
ENDS='S=$(sudo -n ls -t /var/lib/lampOS/voice/sessions | head -1); sudo -n sh -c "grep -h -e speaker_first_write -e turn_finished /var/lib/lampOS/voice/sessions/$S/metadata/events-*.jsonl"; python3 -c "import time; print(\"now\", time.monotonic_ns()//1000)"'
now_us() { lamp "python3 -c 'import time; print(time.monotonic_ns()//1000)'"; }

answer_ended() { # SINCE_US SECONDS: until the first turn answered after SINCE_US has finished (its turn_finished:
  # every sample played and the answer done), then prints how long ago that was, in µs, by the lamp's clock
  local until=$(($(date +%s) + $2)) ago
  while (($(date +%s) < until)); do
    ago=$(lamp "$ENDS" | python3 -c '
import json, sys
since = int(sys.argv[1]); first = None; finished = None; now = None
for line in sys.stdin:
    if line.startswith("now "):
        now = int(line.split()[1]); continue
    try: r = json.loads(line)
    except ValueError: continue
    at = r.get("at_us", 0)
    if at < since: continue
    turn = (r.get("owner") or {}).get("turn", r.get("turn"))
    if r.get("kind") == "speaker_first_write" and first is None: first = turn
    if r.get("kind") == "turn_finished" and first is not None and turn == first and finished is None: finished = at
if finished is None or now is None: sys.exit(1)
print(max(0, now - finished))' "$1") && { echo "$ago"; return 0; }
    sleep 0.5
  done
  return 1
}

episode() { # NAME: one episode played and counted
  local name=$1 began since
  quiet
  began=$(date +%s)
  since=$(now_us)
  echo "$name:" >>"$report"
  case $name in
    over) say_line "$STORY"; sleep 11; say_line "Stop." ;;
    over-name) say_line "$STORY"; sleep 11; say_line "Stop Lamp." ;;
    wordless) say_line "$STORY"; sleep 11; echo "  $(date +%H:%M:%S) PLAY sound ${SOUND##*/}" >>"$report"; "$PLAY" "$SOUND" ;;
    after)
      say_line "$POEM"
      if ago=$(answer_ended "$since" "${STOP_LINES_END_WAIT:-40}"); then
        # About 1 s after the answer's end, however long the lamp took to say it ended.
        ((ago < 1000000)) && sleep "$(awk -v ago="$ago" 'BEGIN { printf "%.3f", (1000000 - ago) / 1e6 }')"
        echo "  the poem ended $((ago / 1000)) ms before it was seen" >>"$report"
        say_line "Stop."
      else
        echo "  the poem's end was never seen; not played" >>"$report"
      fi ;;
  esac
  sleep 12
  lamp "sudo -n journalctl -u lamp-voice --since @$((began - 1)) --no-pager -o short-iso" >"$work/journal.txt"
  python3 - "$name" "$work/journal.txt" "$tally" <<'PY' | tee -a "$report"
import re, sys
name, journal, tally = sys.argv[1:4]
lines = [l.rstrip('\n') for l in open(journal, errors='replace')]
fired = {'over': 'a stop said over the lamp\'s answer: answered', 'over-name': 'a stop said over the lamp\'s answer: answered',
         'after': 'a stop said just after the lamp\'s answer: answered', 'wordless': 'a cut-in with no words ended the lamp\'s answer: answered'}
any_fixed = re.compile(r'request (\d+): (a stop said (over|just after) the lamp\'s answer|a cut-in with no words ended the lamp\'s answer): answered')
def say(text, ok):
    print('  %s: %s%s' % (name, text, '  PASS' if ok else '  FAIL'))
    with open(tally, 'a') as f: f.write('%s\t%s\n' % (name, 'pass' if ok else 'fail'))
hit = None
for line in lines:
    m = any_fixed.search(line)
    if m: hit = (m.group(1), m.group(2), line); break
if hit:
    request, condition, _ = hit
    played = any(re.search(r'request %s first answer audio' % request, l) for l in lines)
    unsaid = any(re.search(r'request %s: its fixed line could not be said' % request, l) for l in lines)
    right = fired[name].startswith(condition) or (name == 'after' and 'over' in condition)
    how = 'played' if played else ('could not be said' if unsaid else 'NOT PLAYED (no first answer audio)')
    say('request %s: %s; %s' % (request, condition, how), played and right)
    sys.exit()
# None fired: what the cut-in became.
words = [l for l in lines if re.search(r'request \d+ heard \d+ words', l)]
last = words[-1] if words else ''
m = re.search(r'request (\d+) heard (\d+) words', last)
if not m:
    say('no fixed line, and no request after the ask (a yield that resumed?)', name == 'wordless'); sys.exit()
request, count = m.group(1), int(m.group(2))
asked = any(re.search(r'request %s chat round 1' % request, l) for l in lines)
held = any(re.search(r'request %s: its answer is held back' % request, l) for l in lines)
if len(words) < 2:
    say('no fixed line, and no request after the ask (a yield that resumed?)', name == 'wordless'); sys.exit()
say('no fixed line: request %s heard %d words, %s' % (request, count, 'asked of the model' if asked else ('held' if held else 'left quiet')), False)
PY
}

for round in $(seq 1 "$ROUNDS"); do
  echo "round $round" | tee -a "$report"
  for name in over over-name after wordless; do
    episode "$name"
  done
done

python3 - "$tally" <<'PY' | tee -a "$report"
import sys
rows = [l.rstrip('\n').split('\t') for l in open(sys.argv[1])]
for name in ('over', 'over-name', 'after', 'wordless'):
    mine = [r for r in rows if r[0] == name]
    print('%s: %d of %d pass' % (name, sum(r[1] == 'pass' for r in mine), len(mine)))
PY
echo "stop-lines.sh: the report is $report"
grep -q FAIL "$report" && exit 1
exit 0
