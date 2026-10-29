#!/usr/bin/env bash
# score-test.sh: runs score.sh on a synthetic OUT_DIR (real ids from lines.txt) and checks key numbers.
# q01 to_lamp: answer, voice played by request. q09 desk_talk: look_up, voice held by request.
# q11 desk_talk: no attention decision, voice none from plays.tsv. q02 to_lamp (held): answer without a
# request, voice played by the time fallback. q10 desk_talk (held): failed, voice held from plays.tsv.
set -u
cd "$(dirname "$0")" || exit 1
T=$(mktemp -d) || exit 1
trap 'rm -rf "$T"' EXIT
B=1761720000000
p() { printf '%s\t%s\t%s\t%s\n' "$1" $((B + $2)) $((B + $2 + 2000)) "$3"; }
{ printf 'id\tlamp_start_ms\tlamp_end_ms\tvoice_seen\n'
  p q01 0 played; p q09 60000 held; p q11 120000 none; p q02 180000 played; p q10 240000 held; } > "$T/plays.tsv"
d() { echo "{\"at_unix_ms\":$((B + $1)),\"about\":\"$2\",\"shadow\":true$3,\"context\":{\"heard\":\"SECRET WORDS\"}}"; }
{ d 2655 attention ',"request":17,"judgement":{"probability":0.62,"choice":"answer","from_rule":false},"took_ms":655'
  d 63000 attention ',"request":18,"judgement":{"probability":0.4,"choice":"look_up","from_rule":false},"took_ms":500'
  d 122500 room ',"request":19,"judgement":{"probability":0.9,"choice":"answer"}'
  d 184000 attention ',"request":null,"judgement":{"probability":0.7,"choice":"answer","from_rule":true},"took_ms":800'
  d 243000 attention ',"request":20,"failed":true,"took_ms":100'; } > "$T/decisions.jsonl"
v() { echo "$(( (B + $1) / 1000 )).$(( (B + $1) % 1000 ))000 lamp-52e6 lamp-voice[812]: lamp-voice provider: request $2: its answer is $3"; }
{ v 5000 17 'played: its name was heard close to the lamp'
  v 66000 18 'held back: nobody said its name'
  v 190000 99 'played: its name was heard close to the lamp'; } > "$T/voice.txt"

out=$(bash score.sh "$T") || { echo "FAIL score.sh exited non-zero"; exit 1; }
fail=0
want() { printf '%s\n' "$out" | grep -qF -- "$1" || { echo "FAIL missing: $1"; fail=1; }; }
want '| held | 2 | 2/2 | 2/2 | 1 | 0 | 0 | 0 | 1 |'
want '| all | 5 | 4/5 | 5/5 | 1 | 0 | 1 | 0 | 3 |'
want '| desk_talk | quiet | 3 | not | 0 | 1 | 0 | 1 | 1 | 2/3 | 0 | 2 | 1 | 3/3 |'
want '| decision after line end | 1000 | 2000 | 2000 | 4 |'
want '| took_ms | 500 | 800 | 800 | 4 |'
want '| q01 | tune | quiet | to_lamp | lamp | answer | 0.62 | played | 17 |'
want '| q09 | tune | quiet | desk_talk | not | look_up | 0.40 | held | 18 |'
want '| q11 | tune | quiet | desk_talk | not | missing | - | none | - |'
want '| q02 | held | quiet | to_lamp | lamp | answer | 0.70 | played | - |'
want '| q10 | held | quiet | desk_talk | not | failed | - | held | 20 |'
printf '%s\n' "$out" | grep -q SECRET && { echo "FAIL heard text in the output"; fail=1; }
held=$(bash score.sh "$T" --split held) || fail=1
printf '%s\n' "$held" | grep -qF '| q01 |' && { echo "FAIL --split held shows a tune line"; fail=1; }
[ $fail = 0 ] && echo PASS || { printf '%s\n' "$out"; echo FAIL; exit 1; }
