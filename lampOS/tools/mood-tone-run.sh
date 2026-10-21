#!/usr/bin/env bash
# The lamp run for "Reads your mood", the voice side (`LAMP_VOICE_TONE` with
# `LAMP_MOOD_TONE`, the first builder on #2602 at 23:21): played lines in an
# empty room, counting how often a calm, a glad and a down line move Mood the
# right way. `tone-lines.sh` says the lines; this gives it the list and reads
# what the lamp did.
#
#   lampOS/tools/mood-tone-run.sh play > played.tsv   # on a Mac or Linux beside the lamp
#   lampOS/tools/mood-tone-run.sh prompt > said.tsv   # at the keyboard of the person who says them
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
# prompt: the person's own session (the first builder on #2602, 22 Oct 06:43):
# the same rows, said by the person at the lamp in their own voice. Each line
# is shown on stderr with how to say it; they press Enter as they begin it, and
# its row is printed then; q stops. The ten plain lines GAP seconds apart, then
# ROUNDS rounds of glad, calm, down, lively (the calm sentence, said lively)
# and raised (said to the lamp, loud and excited but not upset, as at a match),
# then ROOM rounds (3) of:
#   - quoted: someone else's low word, or a glad word said sarcastically,
#     said to the lamp: shown with what Mood did, never judged (words that
#     don't mean it are a known miss of a word list);
#   - aside: after a plain line to the lamp, as soon as it has answered, "I'm
#     so tired today" said to the other person in the room, not to the lamp.
# A calm line and an aside, which should leave Mood as it was, are followed
# by STILL_GAP seconds (45: past the 40 s Mood is read over), the other kinds
# by KIND_GAP. About 45 minutes with three rounds of each.
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
#     FACE=near, energy down at least 0.05 (the quiet: 0.20 at rest);
#   - lively: energy up at least 0.03 (half a smile's 0.10, no warmth);
#   - raised: never quieted (energy down under 0.05): lively, glad or
#     nothing, since a raised voice isn't upset unless its words say so;
#   - aside: as calm, since it wasn't said to the lamp;
#   - quoted: "noted", with the move printed, never right or wrong.
# A move the other way (glad or lively down, down up, calm or an aside
# brightened or quieted, a raised line quieted) is "wrong way", any other
# "not moved"; no `mood/state` in the window is Mood
# as it was. The voice's own reading is printed
# beside each, so a line the meter read wrong is told from one Mood took
# wrong. Mood's time and the player's are wall clocks: the two machines must
# keep time (NTP). Counts and numbers only: no words from the lamp.
#
# The bar (lamp-runs.md): glad, calm and down, and lively in prompt's
# session, each right in at least ROUNDS - 1 of its lines; every raised line
# and aside right; and no line moving Mood the wrong way. A session with any
# of prompt's own kinds needs all of them, ROOM of the room's, or it isn't
# judged. Exits 0 when it passes.
#
# The lamp: the voice with `LAMP_VOICE_TONE=on` and `LAMP_BUS=on` on #2670's
# build or later, Mood with `LAMP_MOOD=on` and `LAMP_MOOD_TONE=on`, nobody in
# the room and a quiet hour. Played lines are a first check only: a person's
# own voice is the row's real one.
#
# Environment: ROUNDS (3), ROOM (3), GAP (15), KIND_GAP (150), STILL_GAP (45),
# FACE (none or near), WITHIN (40), TELL_WITHIN (20); tone-lines.sh's own (SAY, VOICE,
# PIPER, PIPER_MODEL, LAMP_WORD, DRY).
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROUNDS=${ROUNDS:-3}
ROOM=${ROOM:-3}
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
# Raised but not upset: no glad or down word, so only the voice can move Mood.
RAISED=(
  "Come on, that was a goal!"
  "Look at that, he just kept on running!"
  "No way, they did it in the last minute!"
)
# Words that don't mean it: another's low word, or a glad one said sarcastically.
QUOTED=(
  "My sister said she was sad about the move."
  "Oh great, the bus is late again."
  "He told me the film was awful, but I liked it."
)
ASIDE="I'm so tired today."

list() { # $1: "person" for prompt's kinds too
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
    if [ "${1:-}" = person ]; then
      printf 'lively|%s\n' "$CALM_SENTENCE"
      printf 'raised|%s\n' "${RAISED[$((round % ${#RAISED[@]}))]}"
    fi
  done
  [ "${1:-}" = person ] || return 0
  for round in $(seq 0 $((ROOM - 1))); do
    printf 'quoted|%s\n' "${QUOTED[$((round % ${#QUOTED[@]}))]}"
    printf 'aside|%s\n' "$ASIDE"
  done
}

# How to say each kind, for the person.
how() {
  case "$1" in
    plain) echo "to the lamp, as you'd ask it" ;;
    glad) echo "to the lamp, as you mean it" ;;
    calm) echo "to the lamp, quietly and slowly" ;;
    down) echo "to the lamp, as when tired or low" ;;
    lively) echo "to the lamp, the same sentence lively and quick" ;;
    raised) echo "to the lamp, loud and excited but not upset, as at a match" ;;
    quoted) echo "to the lamp, in your ordinary voice" ;;
    aside) echo "first say \"$LAMP_WORD what time is it?\" to the lamp; as soon as it has answered, turn to the other person and say this to them, not to the lamp" ;;
  esac
}

prompt() {
  local number=0 kind words key wait
  LAMP_WORD=${LAMP_WORD:-"Hey Lamp,"}
  # The list on its own descriptor: the keys are stdin's.
  while IFS='|' read -r -u 3 kind words; do
    number=$((number + 1))
    if [ "$kind" = aside ]; then
      echo "mood-tone-run: $number ($kind), $(how "$kind"): \"$words\". Enter as you begin it, q to stop." >&2
    else
      echo "mood-tone-run: $number ($kind), $(how "$kind"): \"$LAMP_WORD $words\". Enter as you begin it, q to stop." >&2
    fi
    key=
    IFS= read -r key || key=q
    [ "$key" != q ] || return 0
    printf '%s\t%s\t%s\t%s\n' "$number" "$kind" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$words"
    case "$kind" in
      plain) wait=${GAP:-15} ;;
      calm | aside) wait=${STILL_GAP:-45} ;;
      *) wait=${KIND_GAP:-150} ;;
    esac
    [ "$wait" -eq 0 ] || echo "mood-tone-run: the next in $wait s" >&2
    sleep "$wait"
  done 3< <(list person)
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
  python3 -I - "$1" "$2" "$3" "$ROUNDS" "$FACE" "${WITHIN:-40}" "${TELL_WITHIN:-20}" "$ROOM" <<'PY'
import json, sys
from datetime import datetime, timezone
tone_path, mood_path, played_path = sys.argv[1:4]
rounds, face = int(sys.argv[4]), sys.argv[5]
within, tell_within = float(sys.argv[6]), float(sys.argv[7])
room = int(sys.argv[8])
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
kinds = ("glad", "calm", "down", "lively", "raised", "quoted", "aside")
right = dict.fromkeys(kinds, 0)
played = dict.fromkeys(kinds, 0)
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
    if kind == "quoted":
        verdict = "noted"
    # A quiet inside the window is the wrong way, whatever brightening came
    # too (reviewer-two's P2 on #2758).
    elif kind == "glad":
        verdict = "wrong way" if down <= -0.05 else ("right" if up >= 0.05 and warmer >= 0.02 else "not moved")
    elif kind == "lively":
        verdict = "wrong way" if down <= -0.05 else ("right" if up >= 0.03 else "not moved")
    elif kind == "raised":
        verdict = "wrong way" if down <= -0.05 else "right"
    elif kind in ("calm", "aside") or face == "none":
        verdict = "right" if still else "wrong way"
    else:
        verdict = "right" if down <= -0.05 else ("wrong way" if up >= 0.05 else "not moved")
    right[kind] += verdict == "right"
    wrong += verdict == "wrong way"
    print(f"{number}\t{kind}\t{reading}\tenergy={e0:.2f}{up:+.2f}/{down:+.2f}\twarmth={w0:.2f}{warmer:+.2f}\t{verdict}")
# glad, calm and down always; with any of the person's own kinds, it's
# prompt's session, and every one of them is needed (reviewer-two's P2 on
# #2758: a session stopped before the room passed without it).
person = any(played[k] for k in ("lively", "raised", "quoted", "aside"))
judged = [k for k in kinds if k != "quoted" and (k in ("glad", "calm", "down") or person)]
for kind in judged:
    print(f"{kind}\tright={right[kind]}\tof={played[kind]}")
if person:
    print(f"quoted\tnoted={played['quoted']}")
print(f"wrong_way\t{wrong}")
need = lambda k: room if k in ("quoted", "aside") else rounds
short = [k for k in kinds if (k in judged or person) and played[k] < need(k)]
if short:
    print(f"bar\tnot judged: too few {', '.join(short)} lines")
    sys.exit(1)
# Every raised line and aside right; the others in all but one round.
passed = wrong == 0 and all(
    right[k] >= (played[k] if k in ("raised", "aside") else rounds - 1) for k in judged)
print("bar\tpass" if passed else "bar\tFAIL")
sys.exit(0 if passed else 1)
PY
}

case "${1:-}" in
  play) play ;;
  prompt) prompt ;;
  read) shift; read_run "${1:?mood-tone-run: read TONE MOOD played.tsv}" "${2:?}" "${3:?}" ;;
  *) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 2 ;;
esac
