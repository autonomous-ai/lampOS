#!/usr/bin/env bash
# Tests for sound-shape.sh on a made-up tap: a loud voice (0.7 to 0.8), a
# stressed word at 1.0 every 1.1 s and a half-second pause every 3 s.
# Run: lampOS/tools/sound-shape-test.sh
set -uo pipefail
SHAPE=$(cd "$(dirname "$0")" && pwd)/sound-shape.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; failures=$((failures + 1)); fi
}
t=0
for i in $(seq 0 599); do
  into=$(( (i * 20) % 3000 ))
  if (( into >= 2500 )); then l=0.0
  elif (( into % 1100 < 120 )); then l=1.0
  else l=$(( i % 3 == 0 ? 7 : 8 )); l=0.$l
  fi
  # Said 100 ms ahead, but every tenth piece 400 ms ahead.
  ahead=$(( i % 10 == 0 ? 400000 : 100000 ))
  printf '{"topic":"voice/sound","seq":%d,"source":"voice"} {"stamp":{"clock_id":"boot","at_us":%d,"sequence":%d},"session_id":"s","turn":1,"request":11,"start_us":%d,"end_us":%d,"level":%s,"audio_generation":1}\n' "$i" $((t + 1000000 - ahead)) "$i" $((t + 1000000)) $((t + 1020000)) "$l"
  t=$((t + 20000))
done >"$T/on.sound"
"$SHAPE" "$T/on.sound" >"$T/out" 2>&1; status=$?
check "it reads lamp-bus tap's lines, past their header" '[[ $status == 0 ]] && grep -q "^pieces 600, with sound 500, piece 20 ms$" "$T/out"'
check "the levels' percentiles over pieces with sound" 'grep -q "^level p10 0.70 p25 0.70 p50 0.80 p75 0.80 p90 1.00 p99 1.00$" "$T/out"'
check "the rises, as a ratio and as a step, fewer at x1.5 than at +0.10" 'line=$(grep "^pieces over the recent level:" "$T/out") && x=$(sed -E "s/.*x1.5 ([0-9]+),.*/\1/" <<<"$line") && a=$(sed -E "s/.*\+0.10 ([0-9]+),.*/\1/" <<<"$line") && (( x < a ))'
check "how far ahead each piece was said, and how many 300 ms or more" 'grep -q "^said ahead (ms): p10 100 p50 100 p90 400; 300 ms or more 60 of 600$" "$T/out"'
check "the half-second pauses, under every quiet line" '[[ $(grep -c "^quiet under .*: 3 pauses of 350 ms or more, longest 500 ms$" "$T/out") == 4 ]]'
: >"$T/empty"
"$SHAPE" "$T/empty" >"$T/out" 2>&1
check "a tap with no sound says so" 'grep -q "^no sound$" "$T/out"'
echo "$failures failed"
[[ $failures == 0 ]]
