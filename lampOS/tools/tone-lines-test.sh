#!/usr/bin/env bash
# Tests for tone-lines.sh with no lamp and no sound: a stand-in for `say`, and
# made-up session records.
# Run: lampOS/tools/tone-lines-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/tone-lines.sh
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

# play, dry: eighteen rows in order, numbered, with the words alone.
DRY=1 "$SCRIPT" play >"$T/played.tsv"
kinds=$(cut -f2 "$T/played.tsv" | tr '\n' ' ')
check "eighteen lines" '[ "$(wc -l <"$T/played.tsv" | tr -d " ")" = 18 ]'
check "ten plain, then calm and lively three times, then glad and down" \
  '[ "$kinds" = "plain plain plain plain plain plain plain plain plain plain calm lively calm lively calm lively glad down " ]'
check "numbered from 1" '[ "$(cut -f1 "$T/played.tsv" | head -1)" = 1 ] && [ "$(cut -f1 "$T/played.tsv" | tail -1)" = 18 ]'
check "the words alone, no controls" '! grep -q "\[\[" "$T/played.tsv"'
check "calm and lively say the same words" \
  '[ "$(awk -F"\t" "\$2==\"calm\"||\$2==\"lively\"{print \$4}" "$T/played.tsv" | sort -u | wc -l | tr -d " ")" = 1 ]'

# play, spoken by a stand-in: the voice, the controls, the lamp's name.
cat >"$T/say" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$SAID"
SH
chmod +x "$T/say"
SAID="$T/said" SAY="$T/say" GAP=0 "$SCRIPT" play >/dev/null
check "each line said once" '[ "$(wc -l <"$T/said" | tr -d " ")" = 18 ]'
check "in Samantha's voice" '[ "$(grep -c "^-v Samantha " "$T/said")" = 18 ]'
check "each begun with the lamp's name" '[ "$(grep -c "Hey Lamp, " "$T/said")" = 18 ]'
check "calm slow, quiet and low" 'sed -n 11p "$T/said" | grep -q "\[\[rate 140\]\]\[\[volm 0.4\]\]\[\[pbas 38\]\]\[\[pmod 4\]\]Hey Lamp, I think"'
check "lively quick, loud and high" 'sed -n 12p "$T/said" | grep -q "\[\[rate 230\]\]\[\[volm 1.0\]\]\[\[pbas 62\]\]\[\[pmod 70\]\]Hey Lamp, I think"'
check "plain lines with no controls" 'sed -n 1p "$T/said" | grep -q "^-v Samantha Hey Lamp, What time is it?$"'

# read: made-up records for the eighteen lines, the baseline sure from line 6.
records() { # $1: lively's arousal
  python3 -I - "$1" <<'PY'
import json, sys
lively = float(sys.argv[1])
kinds = ["plain"] * 10 + ["calm", "lively"] * 3 + ["glad", "down"]
for n, kind in enumerate(kinds, 1):
    arousal = {"calm": -0.6, "lively": lively}.get(kind, 0.05)
    sure = min(1.0, (n - 1) / 5)
    print(json.dumps({"kind": "local_endpoint", "turn": n, "tone": {"whose": "room", "baseline_lines": n,
        "reading": {"arousal": arousal, "sure": sure}}}))
    print(json.dumps({"kind": "voice_tone", "turn": n, "arousal": arousal, "sure": sure,
        "valence": {"glad": 1.0, "down": -1.0}.get(kind), "words": True}))
print(json.dumps({"kind": "voice_tone_forgot", "all": False, "whose": "person", "by": "memory"}))
print("not a record")
PY
}
records 0.7 >"$T/events"
"$SCRIPT" read "$T/events" "$T/played.tsv" >"$T/read" 2>&1
code=$?
check "right signs pass" '[ $code = 0 ] && grep -q "^signs	pass$" "$T/read"'
check "each End and each telling listed" '[ "$(grep -c "^end	" "$T/read")" = 18 ] && [ "$(grep -c "^told	" "$T/read")" = 18 ]'
check "a forget listed, by whom" 'grep -q "^forgot	by=memory	whose=person	all=False$" "$T/read"'
check "glad and down by valence" 'grep -q "^glad	turn=17	valence=1.00	pass$" "$T/read" && grep -q "^down	turn=18	valence=-1.00	pass$" "$T/read"'

records -0.2 >"$T/events-wrong"
"$SCRIPT" read "$T/events-wrong" "$T/played.tsv" >"$T/read-wrong" 2>&1
code=$?
check "a lively line read calm fails" '[ $code = 1 ] && grep -q "^lively	turn=12	arousal=-0.20	sure=1.00	FAIL$" "$T/read-wrong" && grep -q "^signs	3 FAIL$" "$T/read-wrong"'

head -n 20 "$T/events" >"$T/events-short"
"$SCRIPT" read "$T/events-short" "$T/played.tsv" >"$T/read-short" 2>&1
code=$?
check "a line missed is not put side by side" '[ $code = 1 ] && grep -q "^lines	played=18	read=10" "$T/read-short"'

"$SCRIPT" read "$T/events" >"$T/read-only" 2>&1
check "without played.tsv, the records only" '[ $? = 0 ] && ! grep -q "^signs" "$T/read-only"'

"$SCRIPT" read "$T/nothing-here" >/dev/null 2>&1
check "an unreadable file is refused" '[ $? = 2 ]'
"$SCRIPT" >/dev/null 2>&1
check "no mode prints the use and fails" '[ $? = 2 ]'

if [ "$failures" -gt 0 ]; then
  echo "$failures failed"
  exit 1
fi
echo "all passed"
