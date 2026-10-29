#!/usr/bin/env bash
# slot-test.sh: runs slot.sh against stubs (lock, replay, ssh, gh; no lamp, no post): a slot whose lock is
# taken (PLAYING, the run, QUIET with the result, the lock released) and one whose lock is never free by
# LAST (loans.txt "aborted, nothing ran", exit 4, nothing posted). Prints PASS or FAIL lines.
set -u
HERE=$(cd "$(dirname "$0")" && pwd) || exit 1
T=$(mktemp -d) || exit 1; trap 'rm -rf "$T"' EXIT
mkdir "$T/bin"
printf '#!/bin/sh\necho "ssh $*" >> "%s/log"\n' "$T" > "$T/bin/ssh"
printf '#!/bin/sh\necho "gh $*" >> "%s/log"\n' "$T" > "$T/bin/gh"
printf '#!/bin/sh\necho "lock $2" >> "%s/log"; [ "$2" = try ] && [ -n "${LOCKFAIL:-}" ] && exit 1; exit 0\n' "$T" > "$T/lock"
printf '#!/bin/sh\necho "replay $* DEADLINE=$DEADLINE" >> "%s/log"\nprintf "loans.txt: start line written\\npulled: 42 lines played\\n${NOTBACK:-}" > "$2/run.txt"\n' "$T" > "$T/replay"
chmod +x "$T/bin/ssh" "$T/bin/gh" "$T/lock" "$T/replay"
DAY=$(TZ=Asia/Ho_Chi_Minh date +%F) NOW=$(TZ=Asia/Ho_Chi_Minh date +%H:%M)
END=$(TZ=Asia/Ho_Chi_Minh date -v+30M +%H:%M)
te=$(TZ=Asia/Ho_Chi_Minh date -j -f '%Y-%m-%d %H:%M:%S' "$DAY $END:00" +%s)
fails=0; want() { grep -qF -- "$1" "$T/log" || { echo "FAIL $2: no '$1'"; fails=$((fails+1)); }; }
run() { PATH=$T/bin:$PATH LOCK=$T/lock REPLAY=$T/replay SESS=s1 "$HERE/slot.sh" "$DAY" "$NOW" "$NOW" "$END" "$T/out" >/dev/null 2>&1; }
run; rc=$?
[ $rc = 0 ] || { echo "FAIL taken: exit $rc"; fails=$((fails+1)); }
want 'lock try' taken; want 'PLAYING' taken; want "DEADLINE=$(( te - 240 ))" taken
want 'QUIET' taken; want 'pulled: 42 lines played' taken; want 'lock release' taken
grep -q 'loans.txt' "$T/log" && { echo "FAIL taken: slot.sh wrote loans.txt (replay.sh does)"; fails=$((fails+1)); }
[ "$(grep -n PLAYING "$T/log" | cut -d: -f1)" -lt "$(grep -n '^replay' "$T/log" | cut -d: -f1)" ] || { echo "FAIL PLAYING after the run"; fails=$((fails+1)); }
: > "$T/log"; LOCKFAIL=1 run; rc=$?
[ $rc = 4 ] || { echo "FAIL not free: exit $rc"; fails=$((fails+1)); }
want 'aborted, nothing ran' not-free; want 'loans.txt' not-free
grep -qE 'PLAYING|QUIET|^replay' "$T/log" && { echo "FAIL not free: posted or ran"; fails=$((fails+1)); }
: > "$T/log"; NOTBACK='LAMP-POLICY NOT PUT BACK' run; want 'LAMP-POLICY NOT PUT BACK: its /run drop-in' not-back
[ $fails = 0 ] && echo PASS || { cat "$T/log"; exit 1; }
