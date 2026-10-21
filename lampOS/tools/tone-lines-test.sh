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
check "calm slow, quiet and low, after the lamp's name said plainly" 'sed -n 11p "$T/said" | grep -q "^-v Samantha Hey Lamp, \[\[rate 140\]\]\[\[volm 0.4\]\]\[\[pbas 38\]\]\[\[pmod 4\]\]I think"'
check "lively quick, loud and high, after the lamp's name said plainly" 'sed -n 12p "$T/said" | grep -q "^-v Samantha Hey Lamp, \[\[rate 230\]\]\[\[volm 1.0\]\]\[\[pbas 62\]\]\[\[pmod 70\]\]I think"'
check "plain lines with no controls" 'sed -n 1p "$T/said" | grep -q "^-v Samantha Hey Lamp, What time is it?$"'

# play, spoken by a stand-in `spd-say` (Linux): the lamp's name plainly, then
# the words with the kind's own settings, each waited for.
mkdir -p "$T/spd"
cat >"$T/spd/spd-say" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$SAID"
SH
chmod +x "$T/spd/spd-say"
SAID="$T/spd-said" SAY="$T/spd/spd-say" GAP=0 "$SCRIPT" play >/dev/null
check "spd-say: the name, then the words, for each line" '[ "$(wc -l <"$T/spd-said" | tr -d " ")" = 36 ]'
check "spd-say: the lamp's name plainly, waited for" '[ "$(grep -cx -- "-w Hey Lamp," "$T/spd-said")" = 18 ]'
check "spd-say: calm slow, low, flat and quieter" 'sed -n 22p "$T/spd-said" | grep -qx -- "-w -r -45 -p -45 -R -90 -i -40 I think we could go to the park later today."'
check "spd-say: lively quick, high, moving and full" 'sed -n 24p "$T/spd-said" | grep -qx -- "-w -r 55 -p 45 -R 90 -i 100 I think we could go to the park later today."'
check "spd-say: plain lines with no settings, no Mac voice" 'sed -n 2p "$T/spd-said" | grep -qx -- "-w What time is it?"'
SAID="$T/spd-voice" SAY="$T/spd/spd-say" VOICE=female1 GAP=0 "$SCRIPT" play >/dev/null
check "spd-say: a voice given is its synthesis voice" '[ "$(grep -c -- "^-w -t female1 " "$T/spd-voice")" = 36 ]'

# play, through Piper (the Linux mode to use): a stand-in Piper writes a steady
# tone for any text, a stand-in pw-play keeps each line it is given, and ffmpeg
# makes the sentence calm or lively. Skipped without ffmpeg.
if command -v ffmpeg >/dev/null 2>&1; then
  mkdir -p "$T/piper" "$T/kept"
  cat >"$T/piper/piper" <<'SH'
#!/usr/bin/env bash
while [ $# -gt 0 ]; do case "$1" in -f) out=$2; shift ;; esac; shift; done
cat >/dev/null
python3 -I -c "
import math, struct, sys, wave
w = wave.open(sys.argv[1], 'wb'); w.setnchannels(1); w.setsampwidth(2); w.setframerate(22050)
w.writeframes(b''.join(struct.pack('<h', int(8000 * math.sin(2 * math.pi * 220 * n / 22050))) for n in range(22050)))
" "$out"
SH
  cat >"$T/piper/pw-play" <<'SH'
#!/usr/bin/env bash
n=$(ls "$KEPT" | wc -l | tr -d " ")
cp "$1" "$KEPT/$((n + 1)).wav"
SH
  chmod +x "$T/piper/piper" "$T/piper/pw-play"
  echo '{"audio": {"sample_rate": 22050}}' >"$T/voice.onnx.json"
  KEPT="$T/kept" PATH="$T/piper:$PATH" SAY=piper PIPER_MODEL="$T/voice.onnx" GAP=0 "$SCRIPT" play </dev/null >"$T/piper-played.tsv"
  # The words only: the stand-in's name is the line's first second.
  mean() { ffmpeg -nostdin -hide_banner -ss 1.1 -i "$1" -af volumedetect -f null - 2>&1 | sed -n 's/.*mean_volume: \([-0-9.]*\) dB/\1/p'; }
  check "piper: every line played, the list read whole" '[ "$(ls "$T/kept" | wc -l | tr -d " ")" = 18 ] && [ "$(cut -f2 "$T/piper-played.tsv" | tr "\n" " ")" = "$kinds" ]'
  calm=$(mean "$T/kept/11.wav"); lively=$(mean "$T/kept/12.wav"); plain=$(mean "$T/kept/1.wav")
  check "piper: calm's words are quieter than lively's (${calm} against ${lively} dB)" 'awk -v c="$calm" -v l="$lively" "BEGIN{exit !(l - c > 6)}"'
  check "piper: a plain line's words are as loud as lively's" 'awk -v p="$plain" -v l="$lively" "BEGIN{d=p-l; exit !(d < 1.5 && d > -1.5)}"'
  # A voice with no sample rate is refused before anything is said, and a
  # Piper that fails says why; no working folder is left behind.
  before=$(ls -d "${TMPDIR:-/tmp}"/tmp.* 2>/dev/null | wc -l | tr -d " ")
  echo '{}' >"$T/bare.onnx.json"
  SAY=piper PIPER_MODEL="$T/bare.onnx" PATH="$T/piper:$PATH" GAP=0 "$SCRIPT" play </dev/null >/dev/null 2>"$T/bare.err"
  check "piper: a voice with no sample rate is refused" '[ $? = 2 ] && grep -q "no sample rate" "$T/bare.err"'
  printf '#!/usr/bin/env bash\necho "model is broken" >&2\nexit 3\n' >"$T/piper-bad"
  chmod +x "$T/piper-bad"
  SAY=piper PIPER="$T/piper-bad" PIPER_MODEL="$T/voice.onnx" PATH="$T/piper:$PATH" GAP=0 "$SCRIPT" play </dev/null >/dev/null 2>"$T/bad.err"
  check "piper: a Piper that fails says why" 'grep -q "model is broken" "$T/bad.err"'
  after=$(ls -d "${TMPDIR:-/tmp}"/tmp.* 2>/dev/null | wc -l | tr -d " ")
  check "piper: no working folder left behind" '[ "$after" -le "$before" ]'
else
  echo "skip piper: no ffmpeg"
fi

# read: made-up records and played times for the eighteen lines, 18 s apart,
# each End 3 s after its line began, on a lamp clock 5000 s from the Mac's.
# The baseline is sure from line 6.
records() { # $1: lively's arousal; $2: a line the lamp missed (0: none); $3: 1 for a stray End
  python3 -I - "$1" "$2" "$3" "$T/played-timed.tsv" <<'PY'
import json, sys
lively, missed, stray, played = float(sys.argv[1]), int(sys.argv[2]), sys.argv[3] == "1", sys.argv[4]
kinds = ["plain"] * 10 + ["calm", "lively"] * 3 + ["glad", "down"]
with open(played, "w") as out:
    for n, kind in enumerate(kinds, 1):
        second = (n - 1) * 18
        out.write(f"{n}\t{kind}\t2026-10-21T07:{second // 60:02d}:{second % 60:02d}Z\twords\n")
for n, kind in enumerate(kinds, 1):
    if n == missed:
        continue
    arousal = {"calm": -0.6, "lively": lively}.get(kind, 0.05)
    sure = min(1.0, (n - 1) / 5)
    at_us = int((5000 + (n - 1) * 18 + 3) * 1e6)
    print(json.dumps({"kind": "local_endpoint", "turn": n, "at_us": at_us, "tone": {"whose": "room",
        "baseline_lines": n, "reading": {"arousal": arousal, "sure": sure}}}))
    print(json.dumps({"kind": "voice_tone", "turn": n, "arousal": arousal, "sure": sure,
        "valence": {"glad": 1.0, "down": -1.0}.get(kind), "words": True}))
if stray:
    # Another voice, 9.5 s after line 13 began: no line's End.
    print(json.dumps({"kind": "local_endpoint", "turn": 99, "at_us": int((5000 + 12 * 18 + 9.5) * 1e6),
        "tone": {"whose": "room", "baseline_lines": 99, "reading": {"arousal": 0.9, "sure": 1.0}}}))
print(json.dumps({"kind": "voice_tone_forgot", "all": False, "whose": "person", "by": "memory"}))
print("not a record")
PY
}
records 0.7 0 0 >"$T/events"
"$SCRIPT" read "$T/events" "$T/played-timed.tsv" >"$T/read" 2>&1
code=$?
check "right signs pass" '[ $code = 0 ] && grep -q "^signs	pass$" "$T/read"'
check "every line matched by time" 'grep -q "^lines	played=18	matched=18	missed=0	stray=0$" "$T/read"'
check "each End and each telling listed" '[ "$(grep -c "^end	" "$T/read")" = 18 ] && [ "$(grep -c "^told	" "$T/read")" = 18 ]'
check "a forget listed, by whom" 'grep -q "^forgot	by=memory	whose=person	all=False$" "$T/read"'
check "glad and down by valence" 'grep -q "^17	glad	turn=17	valence=1.00	pass$" "$T/read" && grep -q "^18	down	turn=18	valence=-1.00	pass$" "$T/read"'

records -0.2 0 0 >"$T/events-wrong"
"$SCRIPT" read "$T/events-wrong" "$T/played-timed.tsv" >"$T/read-wrong" 2>&1
code=$?
check "a lively line read calm fails" '[ $code = 1 ] && grep -q "^12	lively	turn=12	arousal=-0.20	sure=1.00	FAIL$" "$T/read-wrong" && grep -q "^signs	3 FAIL$" "$T/read-wrong"'

# The reviewer-lead's P3 on #2504: a missed line and a stray End keep the
# counts equal; paired by time, nothing shifts onto the wrong kind.
records 0.7 12 1 >"$T/events-shifted"
"$SCRIPT" read "$T/events-shifted" "$T/played-timed.tsv" >"$T/read-shifted" 2>&1
code=$?
check "a missed line is missed, a stray End is stray" 'grep -q "^12	lively	missed$" "$T/read-shifted" && grep -q "^lines	played=18	matched=17	missed=1	stray=1$" "$T/read-shifted"'
check "the lines after it keep their own Ends" '[ $code = 0 ] && grep -q "^13	calm	turn=13	arousal=-0.60	sure=1.00	pass$" "$T/read-shifted" && grep -q "^18	down	turn=18" "$T/read-shifted"'

grep -v at_us "$T/events" >"$T/events-untimed"
"$SCRIPT" read "$T/events-untimed" "$T/played-timed.tsv" >"$T/read-untimed" 2>&1
code=$?
check "Ends with no time can't be judged" '[ $code = 1 ] && grep -q "^signs	too few to judge (0)$" "$T/read-untimed"'

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
