#!/usr/bin/env bash
# Tests for arm-hour.sh with no lamp: a stand-in ARM_LAMP records each command
# it is given (and fails one that says FAILME, prints PROOF-OUT for one that
# says PROOF), the rows come from a file, and the person's keys are scripted.
# Run: lampOS/tools/arm-hour-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/arm-hour.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

cat >"$T/lamp" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$1" >>"$FAKE/calls"
echo $$ >"$FAKE/ssh.pid"
[[ $1 == *FAILME* ]] && exit 1
[[ $1 == *PROOF* ]] && echo "PROOF-OUT"
[[ $1 == *SLOW* ]] && sleep 5
# trials.sh's check at the start: there unless $FAKE/no-trials.
[[ $1 == "test -x "* ]] && { [[ -e $FAKE/no-trials ]] && exit 1; exit 0; }
# A trial that keeps running on the lamp after its ssh is killed, as on
# lamp-4ace at 03:29 (#2034): a process of its own, ended only by TERM, its
# trap putting back.
if [[ $1 == "sudo "*trials.sh* && -e $FAKE/trial-hangs ]]; then
  ( trap 'echo "trial put back" >>"$FAKE/calls"; rm -f "$FAKE/trial.pid"; exit 130' TERM
    while :; do sleep 0.1; done ) &
  echo $! >"$FAKE/trial.pid"
  wait
  exit 130
fi
# The tool's end of a trial: TERM to it, then wait for it to be gone.
if [[ $1 == *"pkill -TERM -f"* ]]; then
  [[ -e $FAKE/trial.pid ]] && kill -TERM "$(cat "$FAKE/trial.pid")" 2>/dev/null
  for _ in $(seq 1 50); do [[ -e $FAKE/trial.pid ]] || exit 0; sleep 0.1; done
  exit 1
fi
exit 0
EOF
chmod +x "$T/lamp"

cat >"$T/rows" <<'EOF'
row | 1 | A still row | still | 3 min
do | Sit at the desk.
watch | The light warms.
sheet | #1 (test)
lamp | setup one
mac | echo mac-ran >>"$FAKE/calls"
say | Hey Lamp, hello.
wait | 0
proof | show PROOF
back | back one
back | back two
end
row | 2 | A moving row | moves | 5 min
do | Hand near the arm.
watch | The arm folds.
lamp | move the arm
back | rest the arm
end
row | 3 | A row by hand | still | 2 min
do | Do it by hand.
watch | It happens.
end
row | 4 | A failing row | still | 1 min
do | Nothing.
watch | Nothing.
lamp | first step
lamp | FAILME step
lamp | never run
back | put back after a failure
end
row | 5 | A trial row | still | 3 min
do | Nothing.
watch | The trial passes.
trial | nudges
end
EOF

export FAKE=$T ARM_LAMP=$T/lamp ARM_ROWS_FILE=$T/rows ARM_TRIALS=/x/trials.sh

pass=0 fail=0
ok() { pass=$((pass + 1)); }
bad() { fail=$((fail + 1)); echo "FAIL: $*"; }
check() { if [[ $2 == "$3" ]]; then ok; else bad "$1 (expected exit $2, got $3)"; sed 's/^/    | /' "$T/out"; fi; }
has() { if grep -qF -- "$3" "$2"; then ok; else bad "$1 (no '$3' in $2)"; sed 's/^/    | /' "$2"; fi; }
hasnt() { if grep -qF -- "$3" "$2" 2>/dev/null; then bad "$1 ('$3' in $2)"; else ok; fi; }
reset() { rm -f "$T/calls" "$T/log"; : >"$T/calls"; export ARM_LOG=$T/log; }
# run KEYS ARGS...: the person's keys, one a line.
run() { local keys=$1; shift; printf '%b' "$keys" | "$SCRIPT" "$@" >"$T/out" 2>&1; }
order() { # FILE A B: A's line comes before B's
  local a b
  a=$(grep -nF -- "$2" "$1" | head -1 | cut -d: -f1); b=$(grep -nF -- "$3" "$1" | head -1 | cut -d: -f1)
  [[ -n $a && -n $b && $a -lt $b ]]
}

# --dry: every row's lines and steps, nothing run, no ARM_LAMP needed.
reset
ARM_LAMP= run "" --dry; check "--dry" 0 $?
has "--dry: row 1's title" "$T/out" "Row 1: A still row (3 min)"
has "--dry: its two lines" "$T/out" "Watch: The light warms."
has "--dry: a step" "$T/out" "lamp | setup one"
has "--dry: the put-back" "$T/out" "back | back two"
has "--dry: the moving row's question" "$T/out" "ask: Hand near the arm? (y)"
has "--dry: the row by hand" "$T/out" "By hand: the builder runs this row from its run sheet"
[[ ! -s $T/calls ]] && ok || bad "--dry ran: $(cat "$T/calls")"

# No ARM_LAMP: refused.
reset
ARM_LAMP= run "" --only 1; check "no ARM_LAMP" 2 $?

# A still row: its steps in order, the proof shown, the answer logged, then
# every put-back, in order.
reset
run "\n\ny\n" --only 1; check "row 1" 0 $?
[[ $(cat "$T/calls" | tr '\n' ',') == "setup one,mac-ran,show PROOF,back one,back two," ]] && ok || bad "row 1's order: $(cat "$T/calls" | tr '\n' ',')"
has "row 1: the line to say" "$T/out" 'SAY: "Hey Lamp, hello."'
has "row 1: the proof shown" "$T/out" "PROOF-OUT"
has "row 1: passed, logged" "$T/log" "row 1 passed"
has "row 1: its run sheet named" "$T/out" "Run sheet: #1 (test)"

# A moving row without y: nothing runs, nothing to put back.
reset
run "\nn\n" --only 2; check "row 2 without y" 0 $?
[[ ! -s $T/calls ]] && ok || bad "row 2 moved without y: $(cat "$T/calls")"
has "row 2: said why" "$T/out" "not started: nothing moves without y"
has "row 2: logged" "$T/log" "row 2 not started: no hand at the arm"
# With y: it moves, then rests.
reset
run "\ny\ny\n" --only 2; check "row 2 with y" 0 $?
[[ $(cat "$T/calls" | tr '\n' ',') == "move the arm,rest the arm," ]] && ok || bad "row 2 with y: $(cat "$T/calls" | tr '\n' ',')"

# Skipped: nothing runs, logged.
reset
run "s\n" --only 1; check "row 1 skipped" 0 $?
[[ ! -s $T/calls ]] && ok || bad "a skipped row ran: $(cat "$T/calls")"
has "skipped, logged" "$T/log" "row 1 skipped"

# q before a row: nothing runs, stopped.
reset
run "q\n" --only 1; check "q before row 1" 0 $?
[[ ! -s $T/calls ]] && ok || bad "q before a row ran: $(cat "$T/calls")"
has "stopped said" "$T/out" "Stopped at row 1"
# q at the line to say: what ran is put back.
reset
run "\nq\n" --only 1; check "q at the line" 0 $?
[[ $(cat "$T/calls" | tr '\n' ',') == "setup one,mac-ran,back one,back two," ]] && ok || bad "q at the line: $(cat "$T/calls" | tr '\n' ',')"
# The input ending is q, never a pass.
reset
run "\n" --only 1; check "input ended" 0 $?
hasnt "no pass without an answer" "$T/log" "row 1 passed"
has "put back when the input ended" "$T/calls" "back two"

# A failed step: the rest not run, put back, logged.
reset
run "\n\n" --only 4; check "row 4" 0 $?
[[ $(cat "$T/calls" | tr '\n' ',') == "first step,FAILME step,put back after a failure," ]] && ok || bad "row 4: $(cat "$T/calls" | tr '\n' ',')"
has "row 4: the failed step said" "$T/out" "FAILED at: lamp | FAILME step"
has "row 4: logged" "$T/log" "row 4 step failed: lamp | FAILME step"

# A row by hand: its two lines, a wait for the try, the question; nothing run.
reset
run "\n\nit drooped well\n" --only 3; check "row 3" 0 $?
[[ ! -s $T/calls ]] && ok || bad "a row by hand ran: $(cat "$T/calls")"
has "row 3: by hand said" "$T/out" "By hand: the builder runs this row from its run sheet"
has "row 3: waits for the try" "$T/out" "Enter once the try is done"
has "row 3: the words logged" "$T/log" "row 3: it drooped well"
# A row by hand with a proof: the proof only after the try.
cat >"$T/rows4" <<'EOF'
row | 1 | By hand, with a proof | still | 1 min
do | x
watch | y
proof | show PROOF
end
EOF
reset
printf 'q\n' | ARM_ROWS_FILE=$T/rows4 "$SCRIPT" >/dev/null 2>&1
printf '\nq\n' | ARM_ROWS_FILE=$T/rows4 "$SCRIPT" >"$T/out" 2>&1
[[ ! -s $T/calls ]] && ok || bad "a proof ran before the try was done: $(cat "$T/calls")"
reset
printf '\n\ny\n' | ARM_ROWS_FILE=$T/rows4 "$SCRIPT" >"$T/out" 2>&1
has "the proof after the try" "$T/out" "PROOF-OUT"

# A trials.sh row runs on the lamp as root, from ARM_TRIALS.
reset
run "\ny\n" --only 5; check "row 5" 0 $?
has "row 5: trials.sh run" "$T/calls" "sudo /x/trials.sh nudges"

# Every row in order, and --from.
reset
run "s\ns\ns\ns\ns\n"; check "all rows" 0 $?
for n in 1 2 3 4 5; do has "row $n shown" "$T/out" "Row $n: "; done
order "$T/out" "Row 1: " "Row 5: " && ok || bad "rows out of order"
has "done said" "$T/out" "Done. The log:"
reset
run "s\ns\n" --from 4; check "--from 4" 0 $?
hasnt "--from 4 skips row 3" "$T/out" "Row 3: "
has "--from 4 shows row 4" "$T/out" "Row 4: "

# A put-back that fails is said, and the next row waits for Enter.
reset
cat >"$T/rows2" <<'EOF'
row | 1 | Back fails | still | 1 min
do | x
watch | y
lamp | step
back | FAILME back
end
row | 2 | Next | still | 1 min
do | x
watch | y
end
EOF
printf '\ny\nq\n' | ARM_ROWS_FILE=$T/rows2 "$SCRIPT" >"$T/out" 2>&1; check "a failed put back" 0 $?
has "PUT BACK FAILED said" "$T/out" "PUT BACK FAILED"
hasnt "the next row didn't start" "$T/out" "Row 2: "

# A tell: what the person does now, then Enter; q there stops and puts back.
cat >"$T/rows6" <<'EOF'
row | 1 | A tell | still | 1 min
do | x
watch | y
lamp | before
tell | Sit down now.
lamp | after
back | back it
end
EOF
reset
printf '\n\n\ny\n' | ARM_ROWS_FILE=$T/rows6 "$SCRIPT" >"$T/out" 2>&1; check "a tell" 0 $?
has "the tell shown" "$T/out" "NOW: Sit down now."
[[ $(cat "$T/calls" | tr '\n' ',') == "before,after,back it," ]] && ok || bad "a tell: $(cat "$T/calls" | tr '\n' ',')"
reset
printf '\nq\n' | ARM_ROWS_FILE=$T/rows6 "$SCRIPT" >"$T/out" 2>&1
[[ $(cat "$T/calls" | tr '\n' ',') == "before,back it," ]] && ok || bad "q at a tell: $(cat "$T/calls" | tr '\n' ',')"

# A put-back on the Mac ("back | mac: ..."): run there, not on the lamp.
cat >"$T/rows5" <<'EOF'
row | 1 | A Mac put-back | still | 1 min
do | x
watch | y
mac | echo mac-step >>"$FAKE/calls"
back | mac: echo mac-back >>"$FAKE/calls"
back | lamp back
end
EOF
reset
printf '\n\ny\n' | ARM_ROWS_FILE=$T/rows5 "$SCRIPT" >"$T/out" 2>&1; check "a Mac put-back" 0 $?
[[ $(cat "$T/calls" | tr '\n' ',') == "mac-step,mac-back,lamp back," ]] && ok || bad "a Mac put-back: $(cat "$T/calls" | tr '\n' ',')"

# trials.sh missing on the lamp: said at the start, before any row.
reset
touch "$T/no-trials"
run "\n" --only 5; check "no trials.sh on the lamp" 2 $?
rm -f "$T/no-trials"
has "how to get it there said" "$T/out" "copy the morning build's lampOS/tools/trials.sh"
hasnt "no row started" "$T/out" "Row 5: "
# A row without a trial needs none: no check.
reset
touch "$T/no-trials"
run "s\n" --only 1; check "a row without a trial, no trials.sh" 0 $?
rm -f "$T/no-trials"

# ^C in a trial (#2034's P2): the trial is ended on the lamp, its own trap
# puts back, and only then "Stopped".
reset
touch "$T/trial-hangs"
( printf '\n'; sleep 30 ) | "$SCRIPT" --only 5 >"$T/out" 2>&1 &
pid=$!
for _ in $(seq 1 100); do [[ -e $T/trial.pid ]] && break; sleep 0.1; done
# ^C reaches the whole foreground group: the tool and its ssh.
kill -TERM $pid "$(cat "$T/ssh.pid")" 2>/dev/null
for _ in $(seq 1 100); do grep -q "Stopped at row" "$T/out" 2>/dev/null && break; sleep 0.1; done
kill $pid 2>/dev/null; wait $pid 2>/dev/null
rm -f "$T/trial-hangs"
has "the tool ended the trial on the lamp" "$T/calls" "pkill -TERM -f '[t]rials\.sh '"
has "the trial's own put-back ran" "$T/calls" "trial put back"
has "the trial's end said" "$T/out" "the trial has ended"
order "$T/out" "the trial has ended" "Stopped at row 5" && ok || bad "Stopped said before the trial had ended"
[[ ! -e $T/trial.pid ]] && ok || { bad "the trial still runs"; kill "$(cat "$T/trial.pid")" 2>/dev/null; }

# Stopped (TERM) in the middle of a step: put back.
reset
cat >"$T/rows3" <<'EOF'
row | 1 | A slow row | still | 1 min
do | x
watch | y
lamp | SLOW step
back | back after a stop
end
EOF
( printf '\n'; sleep 30 ) | ARM_ROWS_FILE=$T/rows3 "$SCRIPT" >"$T/out" 2>&1 &
pid=$!
for _ in $(seq 1 50); do grep -q "SLOW step" "$T/calls" 2>/dev/null && break; sleep 0.1; done
kill -TERM $pid 2>/dev/null
for _ in $(seq 1 100); do grep -q "back after a stop" "$T/calls" 2>/dev/null && break; sleep 0.1; done
has "put back after TERM" "$T/calls" "back after a stop"
kill $pid 2>/dev/null; wait $pid 2>/dev/null

# The real table, the morning's 19 rows: all there, in order, each with its
# two lines and its run sheet, and every row that moves asks for the hand.
reset
ARM_ROWS_FILE= ARM_LAMP= "$SCRIPT" --dry >"$T/out" 2>&1; check "the real table --dry" 0 $?
[[ $(grep -c '^Row ' "$T/out") == 19 ]] && ok || bad "the real table has $(grep -c '^Row ' "$T/out") rows, not 19"
[[ $(grep '^Row ' "$T/out" | sed 's/^Row \([0-9]*\):.*/\1/' | tr '\n' ' ') == "1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 " ]] && ok || bad "the real table's rows out of order"
[[ $(grep -c '^  Do:    .' "$T/out") == 19 && $(grep -c '^  Watch: .' "$T/out") == 19 && $(grep -c '^  Run sheet: ' "$T/out") == 19 ]] && ok ||
  bad "a real row without its two lines or run sheet"
[[ $(grep -c 'ask: Hand near the arm' "$T/out") -ge 10 ]] && ok || bad "the moving rows don't all ask for the hand"

echo "arm-hour-test: $pass passed, $fail failed"
[[ $fail == 0 ]]
