#!/usr/bin/env bash
# The voice half of "Reads your mood" (`LAMP_VOICE_TONE`, docs/voice-tone.md),
# played with nobody at the lamp (Ten's 13:41 on #1722: the person declined the
# sitting). macOS `say` speaks the same sentence calm and lively through its own
# rate, volume, pitch and pitch-movement controls, and a glad and a down line,
# after ten plain lines for the baseline. The voice's records then say whether
# the arousal and the valence read the right way.
#
#   lampOS/tools/tone-lines.sh play > played.tsv     # on a Mac or Linux beside the lamp
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
# lively over 0, glad's valence (the telling just after its End) over 0, down's
# under 0. Counts and numbers only: no words from the records.
#
# The lamp: the voice on #1712's build or later, with `LAMP_VOICE_TONE=on` and
# `LAMP_BUS=on`, its speaker volume as for any played test, nobody else
# talking. Played lines are a first check only: a person's own voice is the
# row's real one ("played lines pass, real people fail").
#
# On Linux, with no `say` (lamp-75c9's laptop), it speaks with speech-dispatcher's
# `spd-say` instead (or with SAY=spd-say): the lamp's name plainly, then the
# sentence with `spd-say`'s own rate, pitch, pitch range and volume, calm or
# lively as `say`'s are (Ten, #1722 15:2x). Each waits for the speech to end.
#
# With SAY=piper (PIPER_MODEL, a Piper voice's .onnx; PIPER, its binary, `piper`
# by default) each line is spoken to a file, the sentence made calm or lively by
# ffmpeg (calm: 10 dB quieter, pitch 13% lower, slower; lively: pitch 15% higher,
# quicker), joined to the plainly said name with no gap, and played with
# `pw-play` (or `aplay`). This is the Linux mode to use: on lamp-75c9 (21 Oct)
# `spd-say`'s volume setting did not reach espeak-ng, so its calm and lively came
# out equally loud (-19.3 against -19.9 dB at the laptop's mic), and the meter
# read 4 of 6 the wrong way.
#
# Environment: SAY (say, else spd-say; or piper), VOICE (Samantha; with
# spd-say, its synthesis voice, e.g. female1, and none by default), GAP (15),
# LAMP_WORD ("Hey Lamp,"), DRY (1: print the rows without speaking or waiting).
set -euo pipefail

if [ -z "${SAY:-}" ]; then
  if command -v say >/dev/null 2>&1 || ! command -v spd-say >/dev/null 2>&1; then
    SAY=say
  else
    SAY=spd-say
  fi
fi
PIPER_MODE=0
case "$(basename "$SAY")" in
  spd-say) SPD=1; VOICE=${VOICE:-} ;;
  piper) SPD=0; PIPER_MODE=1; PIPER=${PIPER:-piper}; VOICE=${VOICE:-} ;;
  *) SPD=0; VOICE=${VOICE:-Samantha} ;;
esac
GAP=${GAP:-15}
LAMP_WORD=${LAMP_WORD:-"Hey Lamp,"}
DRY=${DRY:-0}

# The same sentence for calm and lively: no glad or down word in it, so only
# its sound differs. The controls are `say`'s own embedded commands.
SENTENCE="I think we could go to the park later today."
CALM="[[rate 140]][[volm 0.4]][[pbas 38]][[pmod 4]]"
LIVELY="[[rate 230]][[volm 1.0]][[pbas 62]][[pmod 70]]"
# The same for `spd-say`, each from -100 to 100: slow, low, flat and quieter;
# quick, high, moving and full.
SPD_CALM="-r -45 -p -45 -R -90 -i -40"
SPD_LIVELY="-r 55 -p 45 -R 90 -i 100"
# The same as ffmpeg filters on Piper's speech, at the voice's own sample rate
# (its `.onnx.json`: 22050 Hz for medium and high voices, 16000 for low ones),
# so a pitch is shifted by the share it says (the reviewer-lead's P3 on #2535).
if [ "$PIPER_MODE" = 1 ]; then
  : "${PIPER_MODEL:?tone-lines: PIPER_MODEL is the Piper voice}"
  RATE=$(python3 -I -c 'import json, sys; print(int(json.load(open(sys.argv[1]))["audio"]["sample_rate"]))' \
    "$PIPER_MODEL.json" 2>/dev/null) || {
    echo "tone-lines: no sample rate in $PIPER_MODEL.json" >&2
    exit 2
  }
  PIPER_CALM="volume=-10dB,asetrate=$RATE*0.87,aresample=$RATE,atempo=0.95"
  PIPER_LIVELY="asetrate=$RATE*1.15,aresample=$RATE,atempo=1.05"
  # One working folder, removed however the run ends.
  WORK=$(mktemp -d)
  trap 'rm -rf "$WORK"' EXIT
fi

# Piper speaks `$2` into the file `$1`; a Piper that fails says why.
piper_wav() {
  if ! printf '%s\n' "$2" | "$PIPER" -m "$PIPER_MODEL" -f "$1" >/dev/null 2>"$WORK/piper.err"; then
    echo "tone-lines: piper failed:" >&2
    cat "$WORK/piper.err" >&2
    exit 1
  fi
}

# With Piper, the line made ready in `$WORK/line.wav` before it is played, so
# its time is taken just before the sound (the reviewer-lead's P3 on #2535).
piper_line() {
  local kind=$1 words=$2 filter
  piper_wav "$WORK/name.wav" "$LAMP_WORD"
  piper_wav "$WORK/words.wav" "$words"
  case "$kind" in
    calm) filter=$PIPER_CALM ;;
    lively) filter=$PIPER_LIVELY ;;
    *) filter=anull ;;
  esac
  # Nothing here may read stdin: it is `play`'s list of lines.
  ffmpeg -nostdin -hide_banner -loglevel error -y -i "$WORK/name.wav" -i "$WORK/words.wav" \
    -filter_complex "[1:a]$filter,aresample=$RATE[w];[0:a][w]concat=n=2:v=0:a=1" "$WORK/line.wav"
}

# Says one line: with `say`, the controls embedded after the lamp's name; with
# `spd-say`, the name plainly, then the words with the kind's own settings.
speak() {
  local kind=$1 controls=$2 words=$3 voice=() flags=()
  if [ "$PIPER_MODE" = 1 ]; then
    # Made ready by `piper_line` already.
    if command -v pw-play >/dev/null 2>&1; then
      pw-play "$WORK/line.wav" </dev/null
    else
      aplay -q "$WORK/line.wav" </dev/null
    fi
  elif [ "$SPD" = 1 ]; then
    [ -n "$VOICE" ] && voice=(-t "$VOICE")
    case "$kind" in
      calm) read -r -a flags <<<"$SPD_CALM" ;;
      lively) read -r -a flags <<<"$SPD_LIVELY" ;;
    esac
    # `${a[@]+"${a[@]}"}`: an empty array under `set -u` aborts macOS's bash
    # 3.2 (the reviewer-lead's P3 on #2535).
    "$SAY" -w ${voice[@]+"${voice[@]}"} "$LAMP_WORD"
    "$SAY" -w ${voice[@]+"${voice[@]}"} ${flags[@]+"${flags[@]}"} "$words"
  else
    "$SAY" -v "$VOICE" "$LAMP_WORD $controls$words"
  fi
}

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
    if [ "$DRY" != 1 ] && [ "$PIPER_MODE" = 1 ]; then
      piper_line "$kind" "$words"
    fi
    at=$(date -u +%FT%TZ)
    printf '%d\t%s\t%s\t%s\n' "$n" "$kind" "$at" "$words"
    if [ "$DRY" != 1 ]; then
      speak "$kind" "$controls" "$words"
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
                     record.get("valence"), record.get("words"), record.get("at_us")))
    elif kind == "voice_tone_forgot":
        print(f"forgot\tby={record.get('by')}\twhose={record.get('whose')}\tall={record.get('all')}")
fmt = lambda v: "-" if v is None else (f"{v:.2f}" if isinstance(v, float) else str(v))
for turn, whose, lines, arousal, sure, _ in ends:
    print(f"end\tturn={fmt(turn)}\twhose={fmt(whose)}\tlines={fmt(lines)}\tarousal={fmt(arousal)}\tsure={fmt(sure)}")
for turn, arousal, sure, value, words, _ in told:
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

# An End's valence is the telling just after it, by the lamp's clock: the voice
# numbers its turns again from 1 in each conversation, so one session's records
# can hold one turn number twice (lamp-75c9, 21 Oct 21:01). Records without a
# time fall back to the turn.
TOLD_WITHIN = 3.0

def valence_of(end):
    turn, at = end[0], end[5]
    if isinstance(at, (int, float)):
        after = [t for t in told if isinstance(t[5], (int, float)) and 0 <= (t[5] - at) / 1e6 <= TOLD_WITHIN]
        if after:
            return min(after, key=lambda t: t[5])[3]
    for t in reversed(told):
        if t[0] == turn and not isinstance(t[5], (int, float)):
            return t[3]
    return None

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
        value = valence_of(end)
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
