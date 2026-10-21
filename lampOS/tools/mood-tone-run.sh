#!/usr/bin/env bash
# The lamp run for "Reads your mood", the voice side (`LAMP_VOICE_TONE` with
# `LAMP_MOOD_TONE`, the first builder on #2602 at 23:21): played lines in an
# empty room, counting how often a calm, a glad and a down line move Mood the
# right way. `tone-lines.sh` says the lines; this gives it the list and reads
# what the lamp did.
#
#   lampOS/tools/mood-tone-run.sh play > played.tsv   # on a Mac or Linux beside the lamp
#   lampOS/tools/mood-tone-run.sh read TONE MOOD played.tsv
#
# play: ten plain lines for the voice's baseline, GAP seconds apart (15), then
# ROUNDS rounds (3) of a glad line, a calm one and a down one, each followed by
# KIND_GAP seconds (150: past Mood's 120 s smile cooldown and a down line's
# 120 s quiet, so no line's effect is still there at the next). None of the
# plain lines has a glad or down word in it. Prints tone-lines.sh's rows: the
# line's number, its kind, when it began (UTC, to the second) and its words.
# About 25 minutes with three rounds.
#
# read: TONE and MOOD are `lamp-bus tap voice/tone` and `lamp-bus tap
# mood/state` from the lamp for the whole run, one message a line (a header,
# then the body); other lines are skipped. Each glad, calm and down line is
# put beside the first `voice/tone` within TELL_WITHIN seconds after it began
# (20: its End, and up to 1.5 s more since #2670) and Mood's energy and warmth
# from just before it began to WITHIN seconds after (40). Its kind moved Mood
# the right way when:
#   - glad: energy up at least 0.05 and warmth up at least 0.02 (a smile's
#     brightening: 0.10 and 0.03 at rest);
#   - calm: energy and warmth both within 0.03 of before (calm isn't sad);
#   - down, with FACE=none (the room empty, the default): the same as calm,
#     since a down line quiets the lamp only with a face near (#2638); with
#     FACE=near, energy down at least 0.05 (the quiet: 0.20 at rest).
# A move the other way (glad down, down up, calm brightened or quieted) is
# "wrong way", any other "not moved"; no `mood/state` in the window is Mood
# as it was. The voice's own reading is printed
# beside each, so a line the meter read wrong is told from one Mood took
# wrong. Mood's time and the player's are wall clocks: the two machines must
# keep time (NTP). Counts and numbers only: no words from the lamp.
#
# The bar (lamp-runs.md): every kind right in at least ROUNDS - 1 of its
# lines, and no line moving Mood the wrong way. Exits 0 when it passes.
#
# The lamp: the voice with `LAMP_VOICE_TONE=on` and `LAMP_BUS=on` on #2670's
# build or later, Mood with `LAMP_MOOD=on` and `LAMP_MOOD_TONE=on`, nobody in
# the room and a quiet hour. Played lines are a first check only: a person's
# own voice is the row's real one.
#
# Environment: ROUNDS (3), GAP (15), KIND_GAP (150), FACE (none or near),
# WITHIN (40), TELL_WITHIN (20); tone-lines.sh's own (SAY, VOICE, PIPER,
# PIPER_MODEL, LAMP_WORD, DRY).
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROUNDS=${ROUNDS:-3}
FACE=${FACE:-none}

# One sentence for calm, with no glad or down word, and a glad and a down line
# a round, each different, every one with a word the voice's list knows.
CALM_SENTENCE="I think we could go to the park later today."
GLAD=(
  "I'm so happy today, everything went great!"
  "That was wonderful, I'm really glad it worked."
  "I love this, what a lovely evening."
)
DOWN=(
  "I'm really tired and stressed today."
  "I feel sad and a bit lonely tonight."
  "That was awful, I'm so upset about it."
)

list() {
  local plain round
  for plain in \
    "What time is it?" \
    "What's the weather like this afternoon?" \
    "How long does it take to boil an egg?" \
    "Tell me a short fact about the moon." \
    "How do you spell necessary?" \
    "What's the capital of Canada?" \
    "How far away is the sun?" \
    "What should I call a cat?" \
    "How many days are there in June?" \
    "What does a koala eat?"; do
    printf 'plain|%s\n' "$plain"
  done
  for round in $(seq 0 $((ROUNDS - 1))); do
    printf 'glad|%s\n' "${GLAD[$((round % ${#GLAD[@]}))]}"
    printf 'calm|%s\n' "$CALM_SENTENCE"
    printf 'down|%s\n' "${DOWN[$((round % ${#DOWN[@]}))]}"
  done
}

play() {
  # One working folder, removed however the run ends.
  WORK=$(mktemp -d)
  trap 'rm -rf "$WORK"' EXIT
  list >"$WORK/lines"
  TONE_LINES="$WORK/lines" GAP="${GAP:-15}" KIND_GAP="${KIND_GAP:-150}" \
    "$HERE/tone-lines.sh" play
}

read_run() {
  local file
  for file in "$1" "$2" "$3"; do
    [ -r "$file" ] || { echo "mood-tone-run: cannot read $file" >&2; exit 2; }
  done
  python3 -I - "$1" "$2" "$3" "$ROUNDS" "$FACE" "${WITHIN:-40}" "${TELL_WITHIN:-20}" <<'PY'
import json, sys
from datetime import datetime, timezone
tone_path, mood_path, played_path = sys.argv[1:4]
rounds, face = int(sys.argv[4]), sys.argv[5]
within, tell_within = float(sys.argv[6]), float(sys.argv[7])
if face not in ("none", "near"):
    sys.exit("mood-tone-run: FACE is none or near")

def messages(path):
    # A tap's line: its header, a space, its body; anything else is skipped.
    out = []
    for line in open(path, encoding="utf-8", errors="replace"):
        line = line.strip()
        if not line.startswith("{"):
            continue
        decoder = json.JSONDecoder()
        try:
            header, end = decoder.raw_decode(line)
            body, _ = decoder.raw_decode(line[end:].lstrip())
        except ValueError:
            continue
        at = header.get("at_unix_ms")
        if isinstance(at, (int, float)) and isinstance(body, dict):
            out.append((at / 1000.0, body))
    return sorted(out, key=lambda m: m[0])

tones, moods = messages(tone_path), messages(mood_path)
fmt = lambda v: "-" if v is None else (f"{v:+.2f}" if isinstance(v, float) else str(v))
right = {"glad": 0, "calm": 0, "down": 0}
played = {"glad": 0, "calm": 0, "down": 0}
wrong = 0
for row in open(played_path, encoding="utf-8").read().splitlines():
    fields = row.split("\t")
    if len(fields) < 3 or fields[1] not in right:
        continue
    number, kind = fields[0], fields[1]
    start = datetime.strptime(fields[2], "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc).timestamp()
    played[kind] += 1
    told = next((b for t, b in tones if start <= t <= start + tell_within), None)
    reading = "told=none" if told is None else (
        f"arousal={fmt(told.get('arousal'))}\tsure={fmt(told.get('sure'))}\tvalence={fmt(told.get('valence'))}")
    before = [b for t, b in moods if t <= start]
    # Nothing new from Mood in the window is Mood as it was (the
    # reviewer-lead's P3 on #2685): Mood says its state about once a second,
    # but a reader shouldn't count on that.
    after = [b for t, b in moods if start < t <= start + within] or before[-1:]
    if not before:
        print(f"{number}\t{kind}\t{reading}\tmood=none\tnot judged")
        continue
    e0, w0 = before[-1].get("energy"), before[-1].get("warmth")
    energies = [b.get("energy") for b in after if isinstance(b.get("energy"), (int, float))]
    warmths = [b.get("warmth") for b in after if isinstance(b.get("warmth"), (int, float))]
    if not isinstance(e0, (int, float)) or not isinstance(w0, (int, float)) or not energies or not warmths:
        print(f"{number}\t{kind}\t{reading}\tmood=none\tnot judged")
        continue
    up, down = max(energies) - e0, min(energies) - e0
    warmer = max(warmths) - w0
    still = abs(up) < 0.03 and abs(down) < 0.03 and abs(warmer) < 0.03 and abs(min(warmths) - w0) < 0.03
    if kind == "glad":
        verdict = "right" if up >= 0.05 and warmer >= 0.02 else ("wrong way" if down <= -0.05 else "not moved")
    elif kind == "calm" or face == "none":
        verdict = "right" if still else "wrong way"
    else:
        verdict = "right" if down <= -0.05 else ("wrong way" if up >= 0.05 else "not moved")
    right[kind] += verdict == "right"
    wrong += verdict == "wrong way"
    print(f"{number}\t{kind}\t{reading}\tenergy={e0:.2f}{up:+.2f}/{down:+.2f}\twarmth={w0:.2f}{warmer:+.2f}\t{verdict}")
for kind in ("glad", "calm", "down"):
    print(f"{kind}\tright={right[kind]}\tof={played[kind]}")
print(f"wrong_way\t{wrong}")
short = [k for k in right if played[k] < rounds]
if short:
    print(f"bar\tnot judged: too few {', '.join(short)} lines")
    sys.exit(1)
passed = wrong == 0 and all(right[k] >= rounds - 1 for k in right)
print("bar\tpass" if passed else "bar\tFAIL")
sys.exit(0 if passed else 1)
PY
}

case "${1:-}" in
  play) play ;;
  read) shift; read_run "${1:?mood-tone-run: read TONE MOOD played.tsv}" "${2:?}" "${3:?}" ;;
  *) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 2 ;;
esac
