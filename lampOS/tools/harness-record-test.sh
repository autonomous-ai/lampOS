#!/usr/bin/env bash
# Tests for harness-record.sh with no network and no Harness account: a
# stand-in harness prints canned JSON, keeps a log of its calls, and is slow
# on `show` so a stacked call would show. Checks the files, each call's
# times, the merged order, that no call of one reader overlapped another,
# the grace for a call still running at the end, watch starting again after
# it ends, Harness's error replies in the timeline, and that nothing is left
# running after the end or after a TERM.
# Run: lampOS/tools/harness-record-test.sh
# The awk programs and bash -c lines below take their $ in single quotes on
# purpose: they are expanded by awk or the inner bash, not here.
# shellcheck disable=SC2016
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/harness-record.sh
T=$(mktemp -d)
trap 'pkill -f "$T/" 2>/dev/null; rm -rf "$T"' EXIT
fails=0
check() { # check WHAT CONDITION...
  local what=$1; shift
  if "$@"; then echo "ok: $what"; else echo "FAIL: $what"; fails=$((fails + 1)); fi
}

# The stand-in: `show` takes 3 s (longer than its 2 s period) and marks a
# lock folder while it runs; a second show inside the first writes OVERLAP.
# FAKE_PS_SLEEP makes ps slow; FAKE_ERROR=1 makes watch and show answer
# with Harness's timeout at once and exit 1, as on a loaded machine.
cat >"$T/harness" <<'STAND_IN'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_DIR/calls"
timeout_reply() { echo '{"error":"TIMEOUT","detail":"Harness did not answer agents_list in time. Try again."}'; exit 1; }
case $1 in
  --help) echo "        harness ps [--machine <name>] [--json]" ;;
  ps)
    sleep "${FAKE_PS_SLEEP:-0}"
    cat <<'JSON'
{
 "machine": "test-machine",
 "harnesses": [
  { "name": "one", "state": "idle", "tools": 0 },
  { "name": "two", "state": "working" }
 ]
}
JSON
    ;;
  show)
    [[ -n ${FAKE_ERROR:-} ]] && timeout_reply
    mkdir "$FAKE_DIR/show.lock" 2>/dev/null || echo OVERLAP >>"$FAKE_DIR/overlap"
    sleep 3
    rmdir "$FAKE_DIR/show.lock"
    printf '{\n "name": "two",\n "state": "working",\n "tools": 3\n}\n'
    ;;
  watch)
    [[ -n ${FAKE_ERROR:-} ]] && timeout_reply
    echo '{"event":"started","state":"starting","name":"two","tools":0}'
    sleep 1
    echo '{"event":"working","state":"working","name":"two","tools":2}'
    sleep 30
    ;;
  *) echo "unknown verb $1" >&2; exit 2 ;;
esac
STAND_IN
chmod +x "$T/harness"
mkdir "$T/tmp"
export FAKE_DIR="$T" HARNESS_BIN="$T/harness" TMPDIR="$T/tmp"

# No process whose command line names PATTERN: pgrep, or /proc where it is missing.
none_running() { # none_running PATTERN
  if command -v pgrep >/dev/null 2>&1; then ! pgrep -f "$1" >/dev/null; return; fi
  local f cmd
  for f in /proc/[0-9]*/cmdline; do
    cmd=$(tr "\\0" " " <"$f" 2>/dev/null) || continue
    [[ $cmd == *"$1"* ]] && return 1
  done
  return 0
}
now_s() { date +%s; }
now_ms() { # as the recorder counts: bash 5, GNU date, else perl
  if [[ -n ${EPOCHREALTIME:-} ]]; then local t=${EPOCHREALTIME/,/.}; local f=${t#*.}000; echo "${t%.*}${f:0:3}"
  elif [[ $(date +%3N) =~ ^[0-9]{3}$ ]]; then date +%s%3N
  else perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'; fi
}
# Every line has END, START and END - START.
times_add_up() { awk -F'\t' 'NF < 4 || $1 - $2 != $3 || length($1) != 13 { bad = 1 } END { exit bad }' "$1"; }
P='[0-9]{13}	[0-9]{13}	[0-9]+	' # END START MS

# 1. A 7 s recording with an agent. The show made at 6 s answers at 9 s,
# inside the grace, and is kept.
t0=$(now_s)
t0_ms=$(now_ms)
"$SCRIPT" record run1 7 --machine test-machine --agent two --out "$T/out" >"$T/stdout" 2>"$T/stderr"
rc=$?
took=$(($(now_s) - t0))
D="$T/out/run1"
check "record exits 0" test "$rc" -eq 0
check "says where it wrote" grep -q "$D" "$T/stdout"
check "watch.jsonl, show.jsonl, ps.jsonl, timeline.txt" \
  test -s "$D/watch.jsonl" -a -s "$D/show.jsonl" -a -s "$D/ps.jsonl" -a -s "$D/timeline.txt"
check "no events.jsonl without an events verb" test ! -e "$D/events.jsonl"
check "watch keeps both lines whole, stamped" \
  test "$(grep -cE "^$P"'\{"event":"(started|working)",.*\}$' "$D/watch.jsonl")" -eq 2
check "watch run 1 started, then stopped at end" \
  bash -c 'head -1 "$0" | grep -q "!watch run 1 started$" && tail -1 "$0" | grep -q "!stopped at end$"' "$D/watch.jsonl"
check "ps is one stamped JSON line per call" \
  test "$(grep -cvE "^$P"'\{.*"harnesses".*\}$' "$D/ps.jsonl")" -eq 0
n_ps=$(($(wc -l <"$D/ps.jsonl")))
check "ps every 5 s: 2 calls in 7 s (got $n_ps)" test "$n_ps" -eq 2
n_show=$(($(wc -l <"$D/show.jsonl")))
check "a 3 s show is not stacked: 2 or 3 shows in 7 s (got $n_show)" test "$n_show" -ge 2 -a "$n_show" -le 3
check "every show made was answered, the last in the grace" test "$(grep -c '^show' "$T/calls")" -eq "$n_show"
check "the last show answered after the end" test "$(tail -1 "$D/show.jsonl" | cut -f1)" -gt $((t0_ms + 7000))
check "no show started after the end" awk -F'\t' -v end=$((t0_ms + 7300)) '$2 > end { bad = 1 } END { exit bad }' "$D/show.jsonl"
check "the show in flight at the end was not killed" bash -c '! grep -q killed "$0"' "$D/show.jsonl"
check "the grace held the end only as long as the call (took ${took}s)" test "$took" -ge 8 -a "$took" -le 14
check "show joined onto one line" grep -qE "^$P"'\{.*"tools": ?3 ?\}$' "$D/show.jsonl"
check "each line's END - START is its MS" times_add_up "$D/show.jsonl"
check "times add up in ps and watch too" bash -c "$(declare -f times_add_up); times_add_up \"\$0\" && times_add_up \"\$1\"" "$D/ps.jsonl" "$D/watch.jsonl"
check "a show's MS is how long it took (~3000)" \
  awk -F'\t' '$3 < 2900 || $3 > 6000 { bad = 1 } END { exit bad }' "$D/show.jsonl"
check "never two shows at once" test ! -e "$T/overlap"
check "the machine and agent passed on" grep -q '^show --machine test-machine two --json$' "$T/calls"
check "watch called as watch --machine M AGENT --json" grep -q '^watch --machine test-machine two --json$' "$T/calls"
sleep 0.3
check "nothing left running after the end" none_running "$T/harness"
check "no work folder left" test -z "$(ls "$T/tmp")"

# The ms stamp of each timeline line (its only 13-digit word).
stamps() { awk '{ for (i = 1; i <= NF; i++) if ($i ~ /^[0-9]+$/ && length($i) == 13) { print $i; break } }' "$1"; }

# The timeline: sorted by time, every reading in it.
cut -f1 "$D"/*.jsonl | sort -n >"$T/all-ms"
check "timeline has every line" test "$(wc -l <"$D/timeline.txt")" -eq "$(wc -l <"$T/all-ms")"
stamps "$D/timeline.txt" >"$T/tl-ms"
check "timeline in time order" cmp -s "$T/tl-ms" "$T/all-ms"
if command -v jq >/dev/null 2>&1; then
  check "timeline sums up ps, with its time" grep -qE 'ps	2 harnesses: idle=1 working=1  \(took [0-9]+ ms\)$' "$D/timeline.txt"
  check "timeline sums up show, with its time" grep -qE 'show	state=working tools=3  \(took [0-9]{4} ms\)$' "$D/timeline.txt"
  check "timeline sums up watch" grep -qE 'watch	working state=working tools=2$' "$D/timeline.txt"
fi

# 2. The timeline again, without jq: the plain summaries.
mkdir "$T/nojq"
for tool in awk sort cut sed tr rm cat; do ln -s "$(command -v "$tool")" "$T/nojq/$tool"; done
PATH="$T/nojq" "$(command -v bash)" "$SCRIPT" timeline "$D" >/dev/null
check "timeline without jq" grep -qE 'show	state=working tools=3  \(took [0-9]{4} ms\)$' "$D/timeline.txt"
stamps "$D/timeline.txt" >"$T/tl-ms"
check "timeline without jq is in time order" cmp -s "$T/tl-ms" "$T/all-ms"

# 3. A call still running past the grace is killed and says so.
t0=$(now_s)
FAKE_PS_SLEEP=20 HARNESS_RECORD_GRACE=1 "$SCRIPT" record slow 2 --out "$T/out" >/dev/null 2>&1
took=$(($(now_s) - t0))
check "a slow call holds the end only for the grace (took ${took}s)" test "$took" -le 8
check "the cut-off call is a !killed at end line" grep -qE "^$P"'!killed at end$' "$T/out/slow/ps.jsonl"
check "its MS is the time it ran (~3000)" \
  awk -F'\t' '$3 < 2500 || $3 > 6000 { bad = 1 } END { exit bad }' "$T/out/slow/ps.jsonl"
sleep 0.3
check "nothing left of the slow call" none_running "$T/harness"

# 4. Harness's timeout: watch ends at once and starts again; the timeline
# names the error.
FAKE_ERROR=1 "$SCRIPT" record busy 5 --agent two --out "$T/out" >/dev/null 2>&1
W="$T/out/busy/watch.jsonl"
check "watch ending at once starts again" grep -q "!watch run 2 started$" "$W"
check "each end is kept" test "$(grep -c '!rc=1 watch ended$' "$W")" -ge 2
check "watch keeps the error line whole" grep -qE "^$P"'\{"error":"TIMEOUT",.*\}$' "$W"
check "a run starts 2 s after the last began" \
  bash -c 'grep "watch run" "$0" | cut -f1 | awk "NR > 1 && (\$1 - p < 1900 || \$1 - p > 4000) { bad = 1 } { p = \$1 } END { exit bad }"' "$W"
if command -v jq >/dev/null 2>&1; then
  check "the timeline says error=TIMEOUT for watch" grep -qE 'watch	error=TIMEOUT$' "$T/out/busy/timeline.txt"
  check "and !rc=1 error=TIMEOUT for show" grep -qE 'show	!rc=1 error=TIMEOUT  \(took [0-9]+ ms\)$' "$T/out/busy/timeline.txt"
fi
PATH="$T/nojq" "$(command -v bash)" "$SCRIPT" timeline "$T/out/busy" >/dev/null
check "without jq too: error=TIMEOUT" grep -qE 'watch	error=TIMEOUT$' "$T/out/busy/timeline.txt"
check "without jq too: !rc=1 error=TIMEOUT" grep -qE 'show	!rc=1 error=TIMEOUT  \(took [0-9]+ ms\)$' "$T/out/busy/timeline.txt"

# 5. TERM partway: every call is killed at once, the timeline still built.
"$SCRIPT" record run2 60 --agent two --out "$T/out" >/dev/null 2>&1 &
pid=$!
sleep 2.5
t0=$(now_s)
kill -TERM "$pid"
wait "$pid"
rc=$?
check "TERM ends it with 143 (got $rc)" test "$rc" -eq 143
check "TERM does not wait for the grace" test $(($(now_s) - t0)) -le 3
check "the show cut off by TERM is kept as killed" grep -qE "^$P"'!killed at end$' "$T/out/run2/show.jsonl"
check "timeline built after TERM" test -s "$T/out/run2/timeline.txt"
sleep 0.3
check "nothing left running after TERM" none_running "$T/harness"
check "no machine flag when none given" grep -q '^ps --json$' "$T/calls"

# 6. A failing harness is kept as a "!" line; no agent means no watch or show.
cat >"$T/broken" <<'STAND_IN'
#!/usr/bin/env bash
echo "not signed in" >&2
exit 5
STAND_IN
chmod +x "$T/broken"
HARNESS_BIN="$T/broken" "$SCRIPT" record run3 1 --out "$T/out" >/dev/null 2>&1
check "a failed call is a !rc line" grep -qE "^$P"'!rc=5 not signed in$' "$T/out/run3/ps.jsonl"
check "no watch or show without an agent" test ! -e "$T/out/run3/watch.jsonl" -a ! -e "$T/out/run3/show.jsonl"

# 7. Usage.
"$SCRIPT" record ../x 5 >/dev/null 2>&1
check "a name with a slash is refused" test $? -eq 2
"$SCRIPT" record x soon >/dev/null 2>&1
check "seconds must be a number" test $? -eq 2

# 8. An events verb, when the binary's help names one.
cat >"$T/later" <<'STAND_IN'
#!/usr/bin/env bash
case $1 in
  --help) echo "        harness events [--machine <name>] [<name>] [--json]" ;;
  events) echo '{"event":"tool","state":"working","tools":1}'; sleep 30 ;;
  *) echo '{"harnesses":[]}' ;;
esac
STAND_IN
chmod +x "$T/later"
HARNESS_BIN="$T/later" "$SCRIPT" record run4 2 --out "$T/out" >/dev/null 2>&1
check "events.jsonl when Harness has events" grep -q '"event":"tool"' "$T/out/run4/events.jsonl"
sleep 0.3
check "nothing left of events" none_running "$T/later"

if command -v shellcheck >/dev/null 2>&1; then
  check "shellcheck" shellcheck "$SCRIPT" "$0"
fi

if ((fails)); then echo "$fails failed"; exit 1; fi
echo "all passed"
