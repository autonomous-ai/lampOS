#!/usr/bin/env bash
# score-test.sh: runs score.sh on a synthetic OUT_DIR (real ids from lines.txt) and checks key numbers.
# Pass 1: q01 to_lamp: answer, voice played by request. q09 desk_talk: look_up, voice held by request.
# q11 desk_talk: no attention decision, voice none from plays.tsv. q02 to_lamp (held): answer without a
# request, voice played by the time fallback. q10 desk_talk (held): failed, voice held from plays.tsv.
# Pass 2: q01 again but in_conversation (not clean). q09 stay_quiet (Jev flips). q02 seen in a busy room
# (not clean). q12 desk_talk (held): Jev answers, the voice holds back (a new false answer for Jev).
# q11 also has the talk before it: a decision (request 30) made after q11 began on words that ended
# before it, and its verdict; neither is q11's. The log's `failed` is a kind, as lamp-policy writes it.
# A missing or failed decision is wrong on a "not" line. Pass 2 f01 (held follow_up) is in_conversation
# and still clean. Pass 2 b02 (busy) read room/state quiet before it: scored as background talk, clean.
# The same pass-1 rows in the old 4-column plays.tsv must score the same.
set -u
cd "$(dirname "$0")" || exit 1
T=$(mktemp -d) || exit 1
trap 'rm -rf "$T"' EXIT
B=1761720000000
p() { printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$1" "$2" $((B + $3)) $((B + $3 + 2000)) "$4" 30 "${5:-quiet}"; }
{ printf 'pass\tid\tlamp_start_ms\tlamp_end_ms\tvoice_seen\tmacvol\troom_read\n'
  p 1 q01 0 played; p 1 q09 60000 held; p 1 q11 120000 none; p 1 q02 180000 played; p 1 q10 240000 held
  p 2 q01 300000 played; p 2 q09 360000 held; p 2 q02 420000 played; p 2 q12 480000 held
  p 2 f01 540000 played; p 2 b02 600000 played quiet; } > "$T/plays.tsv"
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
  d 483000 attention ',"request":24,"judgement":{"probability":0.6,"choice":"answer"},"took_ms":700'
  d 543000 attention ',"request":25,"judgement":{"probability":0.85,"choice":"answer"},"took_ms":500' quiet true
  d 603000 attention ',"request":26,"judgement":{"probability":0.1,"choice":"stay_quiet"},"took_ms":500' busy
  d 650000 hello ',"judgement":{"probability":0.2,"choice":"wait"}'; } > "$T/decisions.jsonl"
v() { echo "$(( (B + $1) / 1000 )).$(( (B + $1) % 1000 ))000 lamp-52e6 lamp-voice[812]: lamp-voice provider: request $2: its answer is $3"; }
{ v 5000 17 'played: its name was heard close to the lamp'
  v 66000 18 'held back: nobody said its name'
  v 122000 30 'played: its name was heard close to the lamp'
  v 190000 99 'played: its name was heard close to the lamp'
  v 305000 21 'played: its name was heard close to the lamp'
  v 366000 22 'held back: nobody said its name'
  v 426000 23 'played: its name was heard close to the lamp'
  v 486000 24 'held back: nobody said its name'
  v 546000 25 'played: a follow-up in the conversation'
  v 606000 26 'played: its name was heard close to the lamp'; } > "$T/voice.txt"

out=$(bash score.sh "$T") || { echo "FAIL score.sh exited non-zero"; exit 1; }
fail=0
want() { printf '%s\n' "$out" | grep -qF -- "$1" || { echo "FAIL missing: $1"; fail=1; }; }
want '| held | all | 6 | 3/6 | 6/6 | 1 | 1 | 1 | 0 | 2 |'
want '| held | clean | 5 | 2/5 | 5/5 | 1 | 1 | 1 | 0 | 2 |'
want '| all | all | 11 | 6/11 | 11/11 | 1 | 1 | 2 | 0 | 5 |'
want '| all | clean | 9 | 4/9 | 9/9 | 1 | 1 | 2 | 0 | 5 |'
want 'In conversation (lamp spoke within 20 s): 2 trials. Room seen not the label room: 1 trials. Clean: 9/11. Background talk: the room as the lamp read it; read busy before 0 of them.'
want '| to_lamp | background talk | 1 | lamp | 0 | 0 | 1 | 0 | 0 | 0/1 | 1 | 0 | 0 | 1/1 |'
want '| follow_up | quiet | 1 | lamp | 1 | 0 | 0 | 0 | 0 | 1/1 | 1 | 0 | 0 | 1/1 |'
want '- hello: 1 decisions: wait 1'
want '- progress: 0 decisions (none asked: thin, nothing to score)'
want 'Jev take flipped on 1/3 ids, voice on 0/3 ids.'
want '| desk_talk | quiet | 5 | not | 1 | 1 | 1 | 1 | 1 | 1/5 | 0 | 4 | 1 | 5/5 |'
want '| decision after line end | 1000 | 1000 | 2000 | 10 |'
want '| took_ms | 500 | 700 | 800 | 10 |'
want '| 1 | q01 | tune | quiet | to_lamp | lamp | answer | 0.62 | played | 17 | false | quiet |'
want '| 1 | q11 | tune | quiet | desk_talk | not | missing | - | none | - | - | - |'
want '| 2 | q01 | tune | quiet | to_lamp | lamp | answer | 0.80 | played | 21 | true | quiet |'
printf '%s\n' "$out" | grep -qF '| q02 |' && { echo "FAIL the default per-line table shows a held line"; fail=1; }
want '- Jev correct: 2/5 (0.40, 95% 0.12-0.77)'
want '- voice correct: 5/5 (1.00, 95% 0.57-1.00)'
want '- margin (Jev - voice): -3 trials'
want '- desk talk false answers new for Jev (Jev took, voice held back): 1'
want 'Jev beats the voice: not shown'
printf '%s\n' "$out" | grep -q SECRET && { echo "FAIL heard text in the output"; fail=1; }
held=$(bash score.sh "$T" --split held) || fail=1
printf '%s\n' "$held" | grep -qF '| q01 |' && { echo "FAIL --split held shows a tune line"; fail=1; }
for w in '| 1 | q02 | held | quiet | to_lamp | lamp | answer | 0.70 | played | - | false | quiet |' \
         '| 1 | q10 | held | quiet | desk_talk | not | failed | - | held | 20 | false | quiet |' \
         '| 2 | q02 | held | quiet | to_lamp | lamp | answer | 0.90 | played | 23 | false | busy |'; do
  printf '%s\n' "$held" | grep -qF -- "$w" || { echo "FAIL --split held missing: $w"; fail=1; }; done
mkdir "$T/old" && cp "$T/decisions.jsonl" "$T/voice.txt" "$T/old/" || exit 1
awk -F'\t' -v OFS='\t' 'NR == 1 { print "id", "lamp_start_ms", "lamp_end_ms", "voice_seen"; next } $1 == 1 { print $2, $3, $4, $5 }' \
  "$T/plays.tsv" > "$T/old/plays.tsv"
bash score.sh "$T/old" | grep -qF '| all | all | 5 | 2/5 | 5/5 | 1 | 0 | 1 | 0 | 3 |' || { echo "FAIL old plays.tsv"; fail=1; }
[ $fail = 0 ] && echo PASS || { printf '%s\n' "$out"; echo FAIL; exit 1; }
