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
# play: each line as "Hey Lamp, ..." so it is answered, GAP seconds apart (15 by
# default: longer than an answer, since the meter takes no speech while the
# lamp is speaking). Questions only, so nothing is set, played or moved on the
# lamp. Prints one tab-separated row a line: its number, its kind (plain, calm,
# lively, glad, down), when it was played (UTC) and its words.
#
# read: EVENTS is the voice's session records, one JSON object a line, as the
# lamp's session readout gives them. Prints each line's End (`local_endpoint`'s
# `tone`: whose baseline, its lines, the arousal and how sure) and each telling
# (`voice_tone`: arousal, sure, valence, whether its words came), in order.
# With played.tsv, and as many Ends read as lines played, each End is put
# beside its kind, and the signs are checked once the baseline is sure:
# calm under 0, lively over 0, glad's valence over 0, down's under 0. Counts
# and numbers only: no words from the records.
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
      "$SAY" -v "$VOICE" "$controls$LAMP_WORD $words"
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
                     reading.get("arousal"), reading.get("sure")))
    elif kind == "voice_tone":
        told.append((record.get("turn"), record.get("arousal"), record.get("sure"),
                     record.get("valence"), record.get("words")))
    elif kind == "voice_tone_forgot":
        print(f"forgot\tby={record.get('by')}\twhose={record.get('whose')}\tall={record.get('all')}")
fmt = lambda v: "-" if v is None else (f"{v:.2f}" if isinstance(v, float) else str(v))
for turn, whose, lines, arousal, sure in ends:
    print(f"end\tturn={fmt(turn)}\twhose={fmt(whose)}\tlines={fmt(lines)}\tarousal={fmt(arousal)}\tsure={fmt(sure)}")
valence = {turn: (value, words) for turn, _, _, value, words in told}
for turn, arousal, sure, value, words in told:
    print(f"told\tturn={fmt(turn)}\tarousal={fmt(arousal)}\tsure={fmt(sure)}\tvalence={fmt(value)}\twords={fmt(words)}")
if not played:
    sys.exit(0)
kinds = [row.split("\t")[1] for row in open(played, encoding="utf-8").read().splitlines() if row.strip()]
if len(kinds) != len(ends):
    print(f"lines\tplayed={len(kinds)}\tread={len(ends)}: not put side by side (a line missed, or another voice)")
    sys.exit(1)
failed = 0
for kind, (turn, _, _, arousal, sure) in zip(kinds, ends):
    if kind in ("calm", "lively"):
        if sure is None or sure < 0.5:
            print(f"{kind}\tturn={fmt(turn)}\tarousal={fmt(arousal)}\tsure={fmt(sure)}\tnot sure yet")
            continue
        ok = arousal is not None and (arousal < 0 if kind == "calm" else arousal > 0)
    elif kind in ("glad", "down"):
        value = valence.get(turn, (None, None))[0]
        ok = value is not None and (value > 0 if kind == "glad" else value < 0)
        print(f"{kind}\tturn={fmt(turn)}\tvalence={fmt(value)}\t{'pass' if ok else 'FAIL'}")
        failed += not ok
        continue
    else:
        continue
    print(f"{kind}\tturn={fmt(turn)}\tarousal={fmt(arousal)}\tsure={fmt(sure)}\t{'pass' if ok else 'FAIL'}")
    failed += not ok
print(f"signs\t{'pass' if failed == 0 else f'{failed} FAIL'}")
sys.exit(1 if failed else 0)
PY
}

case "${1:-}" in
  play) play ;;
  read) shift; read_events "${1:?tone-lines: read EVENTS [played.tsv]}" "${2:-}" ;;
  *) sed -n '2,33p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
