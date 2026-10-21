#!/usr/bin/env bash
# The voice half of "Reads your mood" (`LAMP_VOICE_TONE`, docs/voice-tone.md),
# played with nobody at the lamp (Ten's 13:41 on #1722: the person declined the
# sitting). macOS `say` speaks the same sentence calm and lively through its own
# rate, volume, pitch and pitch-movement controls, and a glad and a down line,
# after ten plain lines for the baseline. The voice's records then say whether
# the arousal and the valence read the right way.
#
#   lampOS/tools/tone-lines.sh play > played.tsv     # on a Mac beside the lamp
#   lampOS/tools/tone-lines.sh read EVENTS [played.tsv]
#
# play: each line as "Hey Lamp, ..." so it is answered, the lamp's name always
# plainly (the controls start after it, so a calm line's wake is not missed),
# GAP seconds apart (15 by default: longer than an answer, since the meter takes
# no speech while the lamp is speaking). The plain lines are questions; the
# calm, lively, glad and down lines are statements ("I think we could go to the
# park later today"), which the lamp's model could take as something to
# remember, but none asks for anything to be set, played or moved. Prints one
# tab-separated row a line: its number, its kind (plain, calm, lively, glad,
# down), when it began (UTC, to the second) and its words.
#
# read: EVENTS is the voice's session records, one JSON object a line, as the
# lamp's session readout gives them. Prints each line's End (`local_endpoint`'s
# `tone`: whose baseline, its lines, the arousal and how sure) and each telling
# (`voice_tone`: arousal, sure, valence, whether its words came), in order.
# With played.tsv, each End is put beside the line it followed, by time: the
# lamp's clock (`at_us`) and the Mac's differ by an offset, found as the one
# under which most Ends come a few seconds after a line began. So a line the
# lamp missed is shown as missed, and an End of another voice as stray, and
# neither moves the others onto the wrong kind (the reviewer-lead's P3 on
# #2504). The signs are then checked once the baseline is sure: calm under 0,
# lively over 0, glad's valence over 0, down's under 0. Counts and numbers only:
# no words from the records.
#
# The lamp: the voice on #1712's build or later, with `LAMP_VOICE_TONE=on` and
# `LAMP_BUS=on`, its speaker volume as for any played test, nobody else
# talking. Played lines are a first check only: a person's own voice is the
# row's real one ("played lines pass, real people fail").
#
# Environment: SAY (say), VOICE (Samantha), GAP (15), LAMP_WORD ("Hey Lamp,"),
# DRY (1: print the rows without speaking or waiting).
set -euo pipefail

SAY=${SAY:-say}
VOICE=${VOICE:-Samantha}
GAP=${GAP:-15}
LAMP_WORD=${LAMP_WORD:-"Hey Lamp,"}
DRY=${DRY:-0}

# The same sentence for calm and lively: no glad or down word in it, so only
# its sound differs. The controls are `say`'s own embedded commands.
SENTENCE="I think we could go to the park later today."
CALM="[[rate 140]][[volm 0.4]][[pbas 38]][[pmod 4]]"
LIVELY="[[rate 230]][[volm 1.0]][[pbas 62]][[pmod 70]]"

lines() {
  local plain
  for plain in \
    "What time is it?" \
    "What's the weather like this afternoon?" \
    "How long does it take to boil an egg?" \
    "Tell me a short fact about the moon." \
    "How do you spell necessary?" \
    "What's the capital of Canada?" \
    "How far away is the sun?" \
    "What's a good name for a cat?" \
    "How many days are there in June?" \
    "What does a koala eat?"; do
    printf 'plain||%s\n' "$plain"
  done
  local round
  for round in 1 2 3; do
    printf 'calm|%s|%s\n' "$CALM" "$SENTENCE"
    printf 'lively|%s|%s\n' "$LIVELY" "$SENTENCE"
  done
  printf 'glad||%s\n' "I'm so happy today, everything went great!"
  printf 'down||%s\n' "I'm really tired and stressed today."
}

play() {
  local n=0 kind controls words at
  # `|`, not a tab: a tab is whitespace to `read`, and an empty field would
  # collapse.
  while IFS='|' read -r kind controls words; do
    n=$((n + 1))
    at=$(date -u +%FT%TZ)
    printf '%d\t%s\t%s\t%s\n' "$n" "$kind" "$at" "$words"
    if [ "$DRY" != 1 ]; then
      "$SAY" -v "$VOICE" "$LAMP_WORD $controls$words"
      sleep "$GAP"
    fi
  done < <(lines)
}

read_events() {
  local events=$1 played=${2:-}
  [ -r "$events" ] || { echo "tone-lines: cannot read $events" >&2; exit 2; }
  python3 -I - "$events" "$played" <<'PY'
import json, sys
events, played = sys.argv[1], sys.argv[2]
from datetime import datetime, timezone
ends, told = [], []
for line in open(events, encoding="utf-8", errors="replace"):
    try:
        record = json.loads(line)
    except ValueError:
        continue
    kind = record.get("kind")
    if kind == "local_endpoint" and isinstance(record.get("tone"), dict):
        tone = record["tone"]
        reading = tone.get("reading") or {}
        ends.append((record.get("turn"), tone.get("whose"), tone.get("baseline_lines"),
                     reading.get("arousal"), reading.get("sure"), record.get("at_us")))
    elif kind == "voice_tone":
        told.append((record.get("turn"), record.get("arousal"), record.get("sure"),
                     record.get("valence"), record.get("words")))
    elif kind == "voice_tone_forgot":
        print(f"forgot\tby={record.get('by')}\twhose={record.get('whose')}\tall={record.get('all')}")
fmt = lambda v: "-" if v is None else (f"{v:.2f}" if isinstance(v, float) else str(v))
for turn, whose, lines, arousal, sure, _ in ends:
    print(f"end\tturn={fmt(turn)}\twhose={fmt(whose)}\tlines={fmt(lines)}\tarousal={fmt(arousal)}\tsure={fmt(sure)}")
valence = {turn: value for turn, _, _, value, _ in told}
for turn, arousal, sure, value, words in told:
    print(f"told\tturn={fmt(turn)}\tarousal={fmt(arousal)}\tsure={fmt(sure)}\tvalence={fmt(value)}\twords={fmt(words)}")
if not played:
    sys.exit(0)

# Each played line's start, in the Mac's seconds.
lines = []
for row in open(played, encoding="utf-8").read().splitlines():
    fields = row.split("\t")
    if len(fields) < 3:
        continue
    at = datetime.strptime(fields[2], "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
    lines.append((fields[0], fields[1], at.timestamp()))
timed = [(k, end) for k, end in enumerate(ends) if isinstance(end[5], (int, float))]
# An End comes when the line's speech ends and the pause after it is decided:
# a line of 2 to 4 s and its 0.6 s, so about 3 s after it began, and never
# outside 0.5 to 8 s. The second the Mac gives is a second wide.
TYPICAL, EARLIEST, LATEST = 3.0, 0.5, 8.0

def pair(offset):
    used, matched, error = set(), {}, 0.0
    for n, (_, _, start) in enumerate(lines):
        for k, end in timed:
            late = end[5] / 1e6 - offset - start
            if k not in used and EARLIEST - 1 <= late <= LATEST + 1:
                used.add(k)
                matched[n] = end
                error += abs(late - TYPICAL)
                break
    return len(matched), -error, matched

best = (0, 0.0, {})
for _, _, start in lines:
    for _, end in timed:
        found = pair(end[5] / 1e6 - start - TYPICAL)
        if found[:2] > best[:2]:
            best = found
matched = best[2]
failed = judged = 0
for n, (number, kind, _) in enumerate(lines):
    end = matched.get(n)
    if end is None:
        print(f"{number}\t{kind}\tmissed")
        continue
    turn, _, _, arousal, sure, _ = end
    if kind in ("calm", "lively"):
        if sure is None or sure < 0.5:
            print(f"{number}\t{kind}\tturn={fmt(turn)}\tarousal={fmt(arousal)}\tsure={fmt(sure)}\tnot sure yet")
            continue
        ok = arousal is not None and (arousal < 0 if kind == "calm" else arousal > 0)
        print(f"{number}\t{kind}\tturn={fmt(turn)}\tarousal={fmt(arousal)}\tsure={fmt(sure)}\t{'pass' if ok else 'FAIL'}")
    elif kind in ("glad", "down"):
        value = valence.get(turn)
        ok = value is not None and (value > 0 if kind == "glad" else value < 0)
        print(f"{number}\t{kind}\tturn={fmt(turn)}\tvalence={fmt(value)}\t{'pass' if ok else 'FAIL'}")
    else:
        print(f"{number}\t{kind}\tturn={fmt(turn)}")
        continue
    judged += 1
    failed += not ok
missed = len(lines) - len(matched)
stray = len(timed) - len(matched)
print(f"lines\tplayed={len(lines)}\tmatched={len(matched)}\tmissed={missed}\tstray={stray}")
if failed:
    print(f"signs\t{failed} FAIL")
elif judged < 4:
    print(f"signs\ttoo few to judge ({judged})")
else:
    print("signs\tpass")
sys.exit(1 if failed or judged < 4 else 0)
PY
}

case "${1:-}" in
  play) play ;;
  read) shift; read_events "${1:?tone-lines: read EVENTS [played.tsv]}" "${2:-}" ;;
  *) sed -n '2,33p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
