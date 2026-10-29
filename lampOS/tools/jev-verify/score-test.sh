#!/usr/bin/env bash
# score-test.sh: runs score.sh on a synthetic OUT_DIR (real ids from lines.txt) and checks key numbers.
# Pass 1: q01 to_lamp: answer, voice played by request. q09 desk_talk: look_up, voice held by request.
# q11 desk_talk: no attention decision, voice none from plays.tsv. q02 to_lamp (held): answer without a
# request, voice played by the time fallback. q10 desk_talk (held): failed, voice held from plays.tsv.
# Pass 2: q01 again but in_conversation (not clean). q09 stay_quiet (Jev flips). q02 seen in a busy room
# (not clean). q12 desk_talk (held): Jev answers, the voice holds back (a new false answer for Jev).
# q11 also has the talk before it: a decision (request 30) made after q11 began on words that ended
# before it, and its verdict; neither is q11's. The log's `failed` is a kind, as lamp-policy writes it.
# The same pass-1 rows in the old 4-column plays.tsv must score the same.
set -u
cd "$(dirname "$0")" || exit 1
T=$(mktemp -d) || exit 1
trap 'rm -rf "$T"' EXIT
B=1761720000000
p() { printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$1" "$2" $((B + $3)) $((B + $3 + 2000)) "$4" 30; }
{ printf 'pass\tid\tlamp_start_ms\tlamp_end_ms\tvoice_seen\tmacvol\n'
  p 1 q01 0 played; p 1 q09 60000 held; p 1 q11 120000 none; p 1 q02 180000 played; p 1 q10 240000 held
  p 2 q01 300000 played; p 2 q09 360000 held; p 2 q02 420000 played; p 2 q12 480000 held; } > "$T/plays.tsv"
# d AT ABOUT EXTRA [ROOM] [IN_CONVERSATION]
d() { echo "{\"at_unix_ms\":$((B + $1)),\"about\":\"$2\",\"shadow\":true$3,\"context\":{\"heard\":\"SECRET WORDS\",\"room\":\"${4:-quiet}\"},\"in_conversation\":${5:-false}}"; }
{ d 2655 attention ',"request":17,"judgement":{"probability":0.62,"choice":"answer","from_rule":false},"took_ms":655'
  d 63000 attention ',"request":18,"judgement":{"probability":0.4,"choice":"look_up","from_rule":false},"took_ms":500'
  d 122500 room ',"request":19,"judgement":{"probability":0.9,"choice":"answer"}'
  d 121000 attention ',"request":30,"judgement":{"probability":0.9,"choice":"answer"},"took_ms":1500,"waited_ms":600'
  d 184000 attention ',"request":null,"judgement":{"probability":0.7,"choice":"answer","from_rule":true},"took_ms":800'
  d 243000 attention ',"request":20,"failed":"timed_out","took_ms":100'
  d 302500 attention ',"request":21,"judgement":{"probability":0.8,"choice":"answer"},"took_ms":300' quiet true
  d 363000 attention ',"request":22,"judgement":{"probability":0.1,"choice":"stay_quiet"},"took_ms":400'
  d 423000 attention ',"request":23,"judgement":{"probability":0.9,"choice":"answer"},"took_ms":600' busy
  d 483000 attention ',"request":24,"judgement":{"probability":0.6,"choice":"answer"},"took_ms":700'; } > "$T/decisions.jsonl"
v() { echo "$(( (B + $1) / 1000 )).$(( (B + $1) % 1000 ))000 lamp-52e6 lamp-voice[812]: lamp-voice provider: request $2: its answer is $3"; }
{ v 5000 17 'played: its name was heard close to the lamp'
  v 66000 18 'held back: nobody said its name'
  v 122000 30 'played: its name was heard close to the lamp'
  v 190000 99 'played: its name was heard close to the lamp'
  v 305000 21 'played: its name was heard close to the lamp'
  v 366000 22 'held back: nobody said its name'
  v 426000 23 'played: its name was heard close to the lamp'
  v 486000 24 'held back: nobody said its name'; } > "$T/voice.txt"

out=$(bash score.sh "$T") || { echo "FAIL score.sh exited non-zero"; exit 1; }
fail=0
want() { printf '%s\n' "$out" | grep -qF -- "$1" || { echo "FAIL missing: $1"; fail=1; }; }
want '| held | all | 4 | 3/4 | 4/4 | 1 | 1 | 1 | 0 | 2 |'
want '| held | clean | 3 | 2/3 | 3/3 | 1 | 1 | 1 | 0 | 2 |'
want '| all | all | 9 | 7/9 | 9/9 | 1 | 1 | 2 | 0 | 5 |'
want '| all | clean | 7 | 5/7 | 7/7 | 1 | 1 | 2 | 0 | 5 |'
want 'In conversation (lamp spoke within 20 s): 1 trials. Room seen not the label room: 1 trials. Clean: 7/9.'
want 'Jev take flipped on 1/3 ids, voice on 0/3 ids.'
want '| desk_talk | quiet | 5 | not | 1 | 1 | 1 | 1 | 1 | 3/5 | 0 | 4 | 1 | 5/5 |'
want '| decision after line end | 1000 | 2000 | 2000 | 8 |'
want '| took_ms | 500 | 800 | 800 | 8 |'
want '| 1 | q01 | tune | quiet | to_lamp | lamp | answer | 0.62 | played | 17 | false | quiet |'
want '| 1 | q11 | tune | quiet | desk_talk | not | missing | - | none | - | - | - |'
want '| 1 | q02 | held | quiet | to_lamp | lamp | answer | 0.70 | played | - | false | quiet |'
want '| 1 | q10 | held | quiet | desk_talk | not | failed | - | held | 20 | false | quiet |'
want '| 2 | q01 | tune | quiet | to_lamp | lamp | answer | 0.80 | played | 21 | true | quiet |'
want '| 2 | q02 | held | quiet | to_lamp | lamp | answer | 0.90 | played | 23 | false | busy |'
want '- Jev correct: 2/3 (0.67, 95% 0.21-0.94)'
want '- voice correct: 3/3 (1.00, 95% 0.44-1.00)'
want '- margin (Jev - voice): -1 trials'
want '- desk talk false answers new for Jev (Jev took, voice held back): 1'
want 'Jev beats the voice: not shown'
printf '%s\n' "$out" | grep -q SECRET && { echo "FAIL heard text in the output"; fail=1; }
held=$(bash score.sh "$T" --split held) || fail=1
printf '%s\n' "$held" | grep -qF '| q01 |' && { echo "FAIL --split held shows a tune line"; fail=1; }
mkdir "$T/old" && cp "$T/decisions.jsonl" "$T/voice.txt" "$T/old/" || exit 1
awk -F'\t' -v OFS='\t' 'NR == 1 { print "id", "lamp_start_ms", "lamp_end_ms", "voice_seen"; next } $1 == 1 { print $2, $3, $4, $5 }' \
  "$T/plays.tsv" > "$T/old/plays.tsv"
bash score.sh "$T/old" | grep -qF '| all | all | 5 | 4/5 | 5/5 | 1 | 0 | 1 | 0 | 3 |' || { echo "FAIL old plays.tsv"; fail=1; }
[ $fail = 0 ] && echo PASS || { printf '%s\n' "$out"; echo FAIL; exit 1; }
