#!/usr/bin/env bash
# Tests for mood-tone-run.sh with no lamp and no sound: a stand-in for `say`,
# and made-up `voice/tone` and `mood/state` taps.
# Run: lampOS/tools/mood-tone-run-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/mood-tone-run.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
failures=0
check() {
  if eval "$2"; then
    echo "ok   $1"
  else
    echo "FAIL $1"
    failures=$((failures + 1))
  fi
}

# play, dry: ten plain lines, then glad, calm and down three times, with no
# complaint and no folder left behind.
mkdir "$T/tmp"
TMPDIR="$T/tmp" DRY=1 "$SCRIPT" play >"$T/played.tsv" 2>"$T/play.err"
check "play says nothing on stderr and leaves no folder" '[ ! -s "$T/play.err" ] && [ -z "$(ls -A "$T/tmp")" ]'
kinds=$(cut -f2 "$T/played.tsv" | tr '\n' ' ')
check "nineteen lines" '[ "$(wc -l <"$T/played.tsv" | tr -d " ")" = 19 ]'
check "ten plain, then glad, calm and down three times" \
  '[ "$kinds" = "plain plain plain plain plain plain plain plain plain plain glad calm down glad calm down glad calm down " ]'
check "each glad and down line different" \
  '[ "$(awk -F"\t" "\$2==\"glad\"||\$2==\"down\"{print \$4}" "$T/played.tsv" | sort -u | wc -l | tr -d " ")" = 6 ]'
# No plain or calm line carries one of the voice's glad or down words
# (`voice_tone.rs`), so only the glad and down lines can move Mood by words.
words=$(sed -n '/^const GLAD/,/^];/p;/^const DOWN/,/^];/p' \
  "$(dirname "$SCRIPT")/../crates/voice/src/voice_tone.rs" | sed -n 's/^ *"\([a-z'"'"']*\)",$/\1/p')
check "the voice's word lists were read" '[ "$(printf "%s\n" "$words" | wc -l | tr -d " ")" -gt 20 ]'
neutral=$(awk -F'\t' '$2=="plain"||$2=="calm"{print tolower($4)}' "$T/played.tsv" | tr -c "a-z'\n" ' ')
check "no glad or down word in a plain or calm line" \
  '! printf "%s\n" $neutral | grep -qxF -f <(printf "%s\n" $words)'
check "a glad word in every glad line, a down word in every down line" '
  ok=1
  while IFS="	" read -r _ kind _ said; do
    case $kind in glad|down) ;; *) continue ;; esac
    printf "%s\n" $(printf "%s" "$said" | tr "A-Z" "a-z" | tr -c "a-z'"'"'\n" " ") | grep -qxF -f <(printf "%s\n" $words) || ok=0
  done <"$T/played.tsv"
  [ $ok = 1 ]'

# play, said by a stand-in: every line once, the lamp's name first; the gaps
# come from GAP and KIND_GAP.
cat >"$T/say" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$SAID"
SH
chmod +x "$T/say"
SAID="$T/said" SAY="$T/say" GAP=0 KIND_GAP=0 "$SCRIPT" play >/dev/null
check "each line said once, begun with the lamp's name" \
  '[ "$(wc -l <"$T/said" | tr -d " ")" = 19 ] && [ "$(grep -c "Hey Lamp, " "$T/said")" = 19 ]'
# The waits: GAP after each plain line, KIND_GAP after the others, read from
# a stand-in `sleep`.
mkdir "$T/bin"
cat >"$T/bin/sleep" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$1" >>"$SLEPT"
SH
chmod +x "$T/bin/sleep"
SLEPT="$T/slept" PATH="$T/bin:$PATH" SAID="$T/said2" SAY="$T/say" GAP=1 KIND_GAP=7 "$SCRIPT" play >/dev/null
check "GAP after each plain line, KIND_GAP after each other" \
  '[ "$(grep -cx 1 "$T/slept")" = 10 ] && [ "$(grep -cx 7 "$T/slept")" = 9 ] && [ "$(wc -l <"$T/slept" | tr -d " ")" = 19 ]'
check "calm said with say's calm controls, glad and down plainly" \
  '[ "$(grep -c "\[\[rate 140\]\]" "$T/said")" = 3 ] && ! grep "so happy\|tired and" "$T/said" | grep -q "\[\["'

# read: each judged line 150 s apart from 07:00:00 UTC; Mood at rest at 0.30
# energy and 0.60 warmth, sampled every second; `voice/tone` 3 s after each.
taps() { # $1: what Mood does: "right", "calm-up", "down-quiet", "raised-quiet", "aside-quiet" or "dip-then-up"; $2: "person" for prompt's kinds
  python3 -I - "$1" "$T/played-run.tsv" "$T/tone" "$T/mood" "${2:-}" <<'PY'
import json, sys
from datetime import datetime, timezone
shape, played, tone, mood, who = sys.argv[1:6]
start = datetime(2026, 10, 21, 7, 0, 0, tzinfo=timezone.utc).timestamp()
kinds = ["glad", "calm", "down"] * 3
# The person's session is with a face near, so a down line quiets the lamp.
if who == "person":
    kinds = ["glad", "calm", "down", "lively", "raised"] * 3 + ["quoted", "aside"] * 3
with open(played, "w") as out:
    for n in range(10):
        at = datetime.fromtimestamp(start - 200 + n * 15, timezone.utc)
        out.write(f"{n + 1}\tplain\t{at:%Y-%m-%dT%H:%M:%SZ}\twords\n")
    for n, kind in enumerate(kinds):
        at = datetime.fromtimestamp(start + n * 150, timezone.utc)
        out.write(f"{n + 11}\t{kind}\t{at:%Y-%m-%dT%H:%M:%SZ}\twords\n")
header = lambda topic, t: json.dumps({"topic": topic, "kind": "latest", "at_unix_ms": int(t * 1000)})
with open(tone, "w") as out:
    out.write("Started ten-tap-tone.service\n")
    for n, kind in enumerate(kinds):
        t = start + n * 150 + 3
        body = {"request": n + 11, "arousal": -0.4 if kind == "calm" else 0.1, "sure": 1.0}
        if kind != "calm":
            body["valence"] = 1.0 if kind == "glad" else -1.0
        out.write(header("voice/tone", t) + " " + json.dumps(body) + "\n")
with open(mood, "w") as out:
    out.write("not a message\n")
    for second in range(-210, len(kinds) * 150):
        t = start + second
        n, since = divmod(second, 150) if second >= 0 else (None, None)
        energy, warmth = 0.30, 0.60
        if n is not None and since >= 4 and since < 60:
            kind = kinds[n]
            if kind == "glad":
                energy, warmth = 0.30 + 0.10 * 0.5 ** ((since - 4) / 15), 0.63
            elif kind == "calm" and shape == "calm-up":
                energy = 0.35
            elif kind == "down" and (shape == "down-quiet" or who == "person"):
                energy = 0.20
            elif kind in ("lively", "raised") and not (kind == "raised" and shape == "raised-quiet"):
                energy = 0.30 + 0.05 * 0.5 ** ((since - 4) / 15)
            elif kind in ("raised", "aside") and shape == kind + "-quiet":
                energy = 0.20
            elif kind == "quoted":
                energy = 0.20
            # A quiet first, then the brightening, inside the window.
            if kind in ("lively", "glad") and shape == "dip-then-up" and since < 10:
                energy = 0.20
        out.write(header("mood/state", t) + " " + json.dumps({"energy": energy, "warmth": warmth}) + "\n")
PY
}

taps right
"$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read" 2>&1
code=$?
check "Mood moved right: the bar passes" '[ $code = 0 ] && grep -q "^bar	pass$" "$T/read"'
check "each kind counted" 'grep -q "^glad	right=3	of=3$" "$T/read" && grep -q "^calm	right=3	of=3$" "$T/read" && grep -q "^down	right=3	of=3$" "$T/read"'
check "plain lines not judged" '[ "$(grep -c "	right$" "$T/read")" = 9 ] && ! grep -q "	plain	" "$T/read"'
check "the voice's reading beside each line" 'grep -q "^11	glad	arousal=+0.10	sure=+1.00	valence=+1.00	energy=0.30+0.10/+0.00	warmth=0.60+0.03	right$" "$T/read"'
check "no wrong way" 'grep -q "^wrong_way	0$" "$T/read"'

taps calm-up
"$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read-up" 2>&1
code=$?
check "a calm line that brightens Mood is the wrong way, and fails" \
  '[ $code = 1 ] && grep -q "^wrong_way	3$" "$T/read-up" && grep -q "^bar	FAIL$" "$T/read-up"'

# With nobody near, a down line that quiets Mood is wrong (#2638: only with a
# face near); with FACE=near it's right, and no quiet is "not moved".
taps down-quiet
"$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read-quiet" 2>&1
check "a quiet with nobody near is the wrong way" 'grep -q "^down	right=0	of=3$" "$T/read-quiet"'
FACE=near "$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read-near" 2>&1
code=$?
check "with a face near, the quiet is right" '[ $code = 0 ] && grep -q "^down	right=3	of=3$" "$T/read-near"'
taps right
FACE=near "$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read-near-still" 2>&1
code=$?
check "with a face near, no quiet is not moved, and fails" \
  '[ $code = 1 ] && [ "$(grep -c "	down	.*not moved$" "$T/read-near-still")" = 3 ]'

# Mood saying nothing new after a line is Mood as it was (the reviewer-lead's
# P3 on #2685): with only the messages before 07:00:00 kept, the calm and down
# lines are right, and the glad ones not moved.
grep -v '"mood/state"' "$T/mood" >"$T/mood-quiet"
python3 -I - "$T/mood" "$T/mood-quiet" <<'PY'
import json, sys
src, dst = sys.argv[1], sys.argv[2]
with open(dst, "a") as out:
    for line in open(src):
        if '"mood/state"' in line and json.loads(line.split("} ", 1)[0] + "}")["at_unix_ms"] < 1792566000000:
            out.write(line)
PY
"$SCRIPT" read "$T/tone" "$T/mood-quiet" "$T/played-run.tsv" >"$T/read-quiet-mood" 2>&1
check "no Mood message after a line is Mood unchanged" \
  'grep -q "^calm	right=3	of=3$" "$T/read-quiet-mood" && grep -q "^down	right=3	of=3$" "$T/read-quiet-mood" && [ "$(grep -c "	glad	.*not moved$" "$T/read-quiet-mood")" = 3 ] && ! grep -q "not judged" "$T/read-quiet-mood"'

# The person's own session (the first builder on #2602, 22 Oct 06:43): prompt
# shows each line, Enter as it begins prints its row, q stops.
mkdir -p "$T/nosleep"
printf '#!/usr/bin/env bash\n' >"$T/nosleep/sleep"
chmod +x "$T/nosleep/sleep"
yes "" | head -60 | PATH="$T/nosleep:$PATH" "$SCRIPT" prompt >"$T/said.tsv" 2>"$T/said.err"
check "prompt: ten plain, three rounds of five kinds, then three of quoted and aside" \
  '[ "$(cut -f2 "$T/said.tsv" | tr "\n" " ")" = "$(printf "plain %.0s" $(seq 10))$(printf "glad calm down lively raised %.0s" 1 2 3)$(printf "quoted aside %.0s" 1 2 3)" ]'
check "prompt: each row numbered and timed" \
  'awk -F"\t" "\$1 != NR || \$3 !~ /^20[0-9-]*T[0-9:]*Z\$/ { bad = 1 } END { exit bad }" "$T/said.tsv"'
check "prompt: each line said first, to the lamp with its name, the aside without" \
  '[ "$(grep -c "^mood-tone-run: [0-9]* (" "$T/said.err")" = 31 ] && grep "(aside)" "$T/said.err" | grep -q ": \"I.m so tired today.\"" && grep "(raised)" "$T/said.err" | grep -q "\"Hey Lamp, "'
check "prompt: lively is the calm sentence; no glad or down word in a raised line" \
  '[ "$(awk -F"\t" "\$2==\"lively\"||\$2==\"calm\"{print \$4}" "$T/said.tsv" | sort -u | wc -l | tr -d " ")" = 1 ] && ! awk -F"\t" "\$2==\"raised\"{print tolower(\$4)}" "$T/said.tsv" | tr -c "a-z'"'"'\n" " " | xargs -n1 | grep -qxF -f <(printf "%s\n" $words)'
cat >"$T/nosleep/sleep" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$1" >>"$SLEPT"
SH
yes "" | head -60 | SLEPT="$T/slept-prompt" PATH="$T/nosleep:$PATH" GAP=1 KIND_GAP=7 STILL_GAP=3 "$SCRIPT" prompt >/dev/null 2>&1
check "prompt: GAP after plain, STILL_GAP after calm and aside, KIND_GAP after the rest" \
  '[ "$(sort "$T/slept-prompt" | uniq -c | tr -s " " | tr "\n" "|")" = " 10 1| 6 3| 15 7|" ]'
printf 'q' | PATH="$T/nosleep:$PATH" "$SCRIPT" prompt >"$T/said-q.tsv" 2>/dev/null
check "prompt: q stops before the line" '[ ! -s "$T/said-q.tsv" ]'

taps right person
FACE=near "$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read-person" 2>&1
code=$?
check "the person's kinds right: the bar passes" '[ $code = 0 ] && grep -q "^bar	pass$" "$T/read-person"'
check "lively, raised and aside counted, quoted noted" \
  'grep -q "^lively	right=3	of=3$" "$T/read-person" && grep -q "^raised	right=3	of=3$" "$T/read-person" && grep -q "^aside	right=3	of=3$" "$T/read-person" && grep -q "^quoted	noted=3$" "$T/read-person" && [ "$(grep -c "	quoted	.*noted$" "$T/read-person")" = 3 ]'
taps raised-quiet person
FACE=near "$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read-raised" 2>&1
code=$?
check "a raised line that quiets the lamp is the wrong way, and fails" \
  '[ $code = 1 ] && grep -q "^wrong_way	3$" "$T/read-raised" && grep -q "^raised	right=0	of=3$" "$T/read-raised"'
taps aside-quiet person
FACE=near "$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read-aside" 2>&1
code=$?
check "an aside that moves Mood is the wrong way, and fails" \
  '[ $code = 1 ] && grep -q "^aside	right=0	of=3$" "$T/read-aside" && grep -q "^bar	FAIL$" "$T/read-aside"'
# A glad or lively line that quiets Mood before it brightens is the wrong
# way, not right (reviewer-two's P2 on #2758).
taps dip-then-up person
FACE=near "$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read-dip" 2>&1
code=$?
check "a quiet before a brightening is the wrong way, glad and lively both" \
  '[ $code = 1 ] && grep -q "^glad	right=0	of=3$" "$T/read-dip" && grep -q "^lively	right=0	of=3$" "$T/read-dip" && grep -q "^wrong_way	6$" "$T/read-dip"'
# A session stopped before the room isn't judged (reviewer-two's P2 on #2758).
taps right person
head -25 "$T/played-run.tsv" >"$T/played-stopped.tsv"
FACE=near "$SCRIPT" read "$T/tone" "$T/mood" "$T/played-stopped.tsv" >"$T/read-stopped" 2>&1
code=$?
check "prompt's session stopped before the room is not judged" \
  '[ $code = 1 ] && grep -q "^bar	not judged: too few quoted, aside lines$" "$T/read-stopped" && grep -q "^aside	right=0	of=0$" "$T/read-stopped"'
ROOM=4 FACE=near "$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read-room" 2>&1
check "fewer room lines than ROOM are not judged" 'grep -q "^bar	not judged: too few quoted, aside lines$" "$T/read-room"'
taps right

ROUNDS=4 "$SCRIPT" read "$T/tone" "$T/mood" "$T/played-run.tsv" >"$T/read-few" 2>&1
code=$?
check "fewer lines than the rounds asked for are not judged" '[ $code = 1 ] && grep -q "^bar	not judged" "$T/read-few"'
"$SCRIPT" read "$T/nothing-here" "$T/mood" "$T/played-run.tsv" >/dev/null 2>&1
check "an unreadable file is refused" '[ $? = 2 ]'
"$SCRIPT" >/dev/null 2>&1
check "no mode prints the use and fails" '[ $? = 2 ]'

if [ "$failures" -gt 0 ]; then
  echo "$failures failed"
  exit 1
fi
echo "all passed"
