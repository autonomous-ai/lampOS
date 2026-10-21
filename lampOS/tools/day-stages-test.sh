#!/usr/bin/env bash
# Tests for day-stages.sh with no lamp: three made-up sessions of timing
# records, as the voice writes them (coordinator.rs), and the tables checked.
# Run: lampOS/tools/day-stages-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/day-stages.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
fails=0
check() { # check WHAT CONDITION...
  local what=$1; shift
  if "$@"; then echo "ok: $what"; else echo "FAIL: $what"; fails=$((fails + 1)); fi
}

# Session A, its records over two files, as the service numbers them.
A=$T/sessions/a
mkdir -p "$A/metadata"
cat >"$A/metadata/events-0001.jsonl" <<'EVENTS'
{"kind":"listening_ready","at_us":1000000}
{"kind":"speaker_first_write","turn":1,"at_us":2000000,"owner":{"generation":1},"playback_sequence":1}
{"kind":"local_endpoint","owner":{"generation":1},"turn":2,"at_us":10000000,"last_block_host_read_us":9990000,"includes_silence_wait_ms":600,"acoustic_speech_end":null}
{"kind":"transcript","turn":2,"at_us":10100000,"text":"zebrafish words"}
not a json line
{"kind":"speaker_first_write","turn":2,"at_us":11200000}
{"kind":"local_endpoint","owner":{"generation":1},"turn":3,"at_us":15000000,"last_block_host_read_us":14990000,"includes_silence_wait_ms":600,"acoustic_speech_end":null,"not_speech":{"speech_blocks":9,"fewer_than":30}}
EVENTS
cat >"$A/metadata/events-0002.jsonl" <<'EVENTS'
{"kind":"speaker_first_write","turn":2,"at_us":10400000}
{"kind":"answer_first_audio","turn":2,"at_us":11105000,"after_decision_ms":1105}
{"kind":"answer_stages","turn":2,"at_us":12000000,"from":"line_end","decided_at_us":10000000,"words_ms":300,"cue_ms":null,"chat_first_ms":700,"first_answer_audio_ms":1100}
{"kind":"local_endpoint","owner":{"generation":1},"turn":4,"at_us":20000000,"last_block_host_read_us":19995000,"includes_silence_wait_ms":300,"acoustic_speech_end":null,"early_end":{"at_us":19800000,"last_block_host_read_us":19795000,"before_this_ms":200}}
{"kind":"answer_first_audio","turn":4,"at_us":21300000,"after_decision_ms":1300}
{"kind":"speaker_first_write","turn":4,"at_us":21000000}
{"kind":"answer_stages","turn":4,"at_us":22000000,"from":"line_end","decided_at_us":20000000,"words_ms":400,"cue_ms":null,"chat_first_ms":900,"first_answer_audio_ms":1500}
{"kind":"local_endpoint","owner":{"generation":1},"turn":5,"at_us":30000000,"last_block_host_read_us":29990000,"includes_silence_wait_ms":1000,"acoustic_speech_end":null}
{"kind":"answer_first_audio","turn":5,"at_us":30900000,"after_decision_ms":900}
{"kind":"speaker_first_write","turn":5,"at_us":30950000}
EVENTS

# Session B: the same turns, slower; given as its metadata folder.
B=$T/sessions/b
mkdir -p "$B/metadata"
cat >"$B/metadata/events-0001.jsonl" <<'EVENTS'
{"kind":"local_endpoint","turn":2,"at_us":10000000,"last_block_host_read_us":9990000,"includes_silence_wait_ms":1000}
{"kind":"speaker_first_write","turn":2,"at_us":11400000}
{"kind":"answer_stages","turn":2,"from":"line_end","decided_at_us":10000000,"words_ms":300,"cue_ms":250,"chat_first_ms":1200,"first_answer_audio_ms":1600,"at_us":12000000}
{"kind":"answer_first_audio","turn":2,"at_us":11600000,"after_decision_ms":1600}
{"kind":"local_endpoint","turn":4,"at_us":20000000,"last_block_host_read_us":19990000,"includes_silence_wait_ms":600}
{"kind":"speaker_first_write","turn":4,"at_us":21400000}
{"kind":"answer_stages","turn":4,"from":"line_end","decided_at_us":20000000,"words_ms":500,"cue_ms":null,"chat_first_ms":1000,"first_answer_audio_ms":1400,"at_us":22000000}
{"kind":"answer_first_audio","turn":4,"at_us":21400000,"after_decision_ms":1400}
EVENTS

has() { grep -qF -- "$1" <<<"$out"; }
hasnt() { ! grep -qF -- "$1" <<<"$out"; }
norow() { ! grep -qE "^\| $1( e)? \|" <<<"$out"; }

out=$("$SCRIPT" "$A" 2>&1)
status=$?
echo "$out"
check "it exits 0" test "$status" -eq 0
check "the header" has "| turn | wait | words | cue | chat | audio | sound |"
# Turn 2: speech ended 9.99 s less 600 ms = 9.39 s; decided at 10 s; the
# first sound the earlier of two writes, in the other file, 10.4 s.
check "a second-route turn, joined over two files" has "| 2 | 610 | 300 | - | 700 | 1105 | 1010 |"
# Turn 4: the line went 200 ms early, so the route's count is less 200.
check "an early line's stages are after the decision, marked e" has "| 4 e | 305 | 200 | - | 700 | 1300 | 1305 |"
check "a Live turn: no stages, its first audio and sound" has "| 5 | 1010 | - | - | - | 900 | 1960 |"
check "the lamp's own line is no row" norow 1
check "a sound thrown out as not speech is no row" norow 3
check "medians" has "| median | 610 | 250 | - | 700 | 1105 | 1305 |"
check "p90, nearest rank" has "| p90 | 1010 | 300 | - | 700 | 1300 | 1960 |"
check "counts" has "| turns | 3 | 2 | 0 | 2 | 3 | 3 |"
check "no word heard reaches the table" hasnt zebrafish

out=$("$SCRIPT" --turns 2,5 "$A" 2>&1)
check "--turns keeps the listed turns" bash -c 'grep -c "^| [0-9]" <<<"$1" | grep -qx 2' _ "$out"
check "--turns: turn 5 is kept" has "| 5 | 1010 |"
check "--turns: turn 4 is not" norow 4
check "--turns: medians of those" has "| median | 810 | 300 | - | 700 | 1002.5 | 1485 |"

out=$("$SCRIPT" --last 1 "$A" 2>&1)
check "--last 1 keeps the last turn" bash -c 'grep "^| [0-9]" <<<"$1" | grep -q "^| 5 |" && [[ $(grep -c "^| [0-9]" <<<"$1") == 1 ]]' _ "$out"

out=$("$SCRIPT" --turns 2,4 --last 1 "$A" 2>&1)
check "--last after --turns" bash -c '[[ $(grep "^| [0-9]" <<<"$1") == "| 4 e |"* && $(grep -c "^| [0-9]" <<<"$1") == 1 ]]' _ "$out"

out=$("$SCRIPT" "$A/metadata/events-0002.jsonl" 2>&1)
check "one file alone: turn 2 has no line end there, so no row" norow 2
check "one file alone: turn 4 is read" has "| 4 e | 305 |"

out=$("$SCRIPT" "$A" "$B/metadata" 2>&1)
status=$?
echo "$out"
check "two sessions exit 0" test "$status" -eq 0
check "a table for each" bash -c '[[ $(grep -c "^### " <<<"$1") == 3 ]]' _ "$out"
check "the second's rows" has "| 2 | 1010 | 300 | 250 | 1200 | 1600 | 2410 |"
check "the second's medians" has "| median | 810 | 400 | 250 | 1100 | 1500 | 2210 |"
check "the median difference per stage" has "| +200 | +150 | - | +400 | +395 | +905 |"

# Session C, for --lines: turns 1 and 2 one line said with a 400 ms pause
# (turn 1 cut on silence_fallback, unanswered), turn 3 a line of its own
# 10 s later. Turn 2's records sit in two segments, as the service rolls
# them; it was admitted twice (again after an early end), the earliest is
# its speech start. Turn 3 has no prefix read, so its admission counts.
C=$T/sessions/c
mkdir -p "$C/metadata"
cat >"$C/metadata/events-0.jsonl" <<'EVENTS'
{"kind":"input_admitted","turn":1,"at_us":1300000,"prefix_first_host_read_us":1000000,"admission_basis":"vad"}
{"kind":"local_endpoint","turn":1,"at_us":2700000,"last_block_host_read_us":2600000,"includes_silence_wait_ms":600,"endpoint":{"basis":"silence_fallback","silence_ms":600}}
{"kind":"transcript","turn":1,"at_us":2800000,"text":"zebrafish half"}
{"kind":"input_admitted","turn":2,"at_us":2600000,"prefix_first_host_read_us":2400000}
EVENTS
cat >"$C/metadata/events-1.jsonl" <<'EVENTS'
{"kind":"input_admitted","turn":2,"at_us":3900000,"prefix_first_host_read_us":3800000,"admission_basis":"continued_after_early_end"}
{"kind":"local_endpoint","turn":2,"at_us":4700000,"last_block_host_read_us":4600000,"includes_silence_wait_ms":600,"endpoint":{"basis":"model_complete","silence_ms":600}}
{"kind":"answer_first_audio","turn":2,"at_us":5500000,"after_decision_ms":800}
{"kind":"input_admitted","turn":3,"at_us":14000000,"prefix_first_host_read_us":null}
{"kind":"local_endpoint","turn":3,"at_us":15600000,"last_block_host_read_us":15500000,"includes_silence_wait_ms":500,"endpoint":{"basis":"silence","silence_ms":500}}
{"kind":"answer_first_audio","turn":3,"at_us":16500000,"after_decision_ms":900}
EVENTS

out=$("$SCRIPT" --lines "$C" 2>&1)
status=$?
echo "$out"
check "--lines exits 0, the session not taken as its gap" test "$status" -eq 0
check "--lines: the stage table still comes first" has "| 2 | 700 | - | - | - | 800 | - |"
check "--lines: the default gap" has "#### heard lines (gap 2000 ms)"
check "--lines: two turns 400 ms apart are one line, over two segments" has "| 1 | 1,2 | silence_fallback,model_complete | -,y |"
check "--lines: a turn 10 s later is its own line" has "| 3 | 3 | silence | y |"
check "--lines: how many lines" has "lines 2"
check "--lines: heard as 2+ turns" has "heard as 2+ turns 1 (turns in them 2)"
check "--lines: the fragment's ending basis" has "fragment ends: silence_fallback 1"
check "--lines: every turn's ending basis" has "turn ends: model_complete 1, silence 1, silence_fallback 1"
check "--lines: no word heard" hasnt zebrafish

out=$("$SCRIPT" --lines 300 "$C" 2>&1)
check "a 300 ms gap splits them" has "heard as 2+ turns 0 (turns in them 0)"
check "a 300 ms gap: three lines" has "lines 3"
check "a 300 ms gap: the gap shown" has "#### heard lines (gap 300 ms)"
check "a 300 ms gap: no fragment" has "fragment ends: none"
out=$("$SCRIPT" --lines=300 "$C" 2>&1)
check "--lines=300 is the same" has "lines 3"

out=$("$SCRIPT" --turns 1,3 --lines "$C" 2>&1)
check "--lines groups only the kept turns" has "heard as 2+ turns 0 (turns in them 0)"
check "--lines with --turns: turn 1 alone" has "| 1 | 1 | silence_fallback | - |"

out=$("$SCRIPT" --lines "$A" "$C" 2>&1)
status=$?
check "--lines, two sessions exit 0" test "$status" -eq 0
check "--lines, two sessions: a lines table each" bash -c '[[ $(grep -c "^#### heard lines" <<<"$1") == 2 ]]' _ "$out"
check "--lines, two sessions: no speech start, no joining" has "| 4 | 4 | - | y |"
check "--lines, two sessions: the comparison" has "heard as 2+ turns: first 0, second 1 (+1)"

out=$("$SCRIPT" --lines 0 "$C" 2>&1)
check "--lines 0 is a usage error" test $? -eq 2
out=$("$SCRIPT" --lines=x "$C" 2>&1)
check "--lines=x is a usage error" test $? -eq 2

out=$("$SCRIPT" 2>&1)
check "no session is a usage error" test $? -eq 2
out=$("$SCRIPT" --last 0 "$A" 2>&1)
check "--last 0 is a usage error" test $? -eq 2
out=$("$SCRIPT" --turns x "$A" 2>&1)
check "--turns x is a usage error" test $? -eq 2
out=$("$SCRIPT" "$T/none" 2>&1)
check "a folder with no records fails" test $? -eq 1

[[ $fails -eq 0 ]] && echo "all passed" || { echo "$fails failed"; exit 1; }
