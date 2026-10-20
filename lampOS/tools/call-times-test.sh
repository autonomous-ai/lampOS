#!/usr/bin/env bash
# Tests for call-times.sh with no lamp: a stand-in journalctl gives a made-up
# window before an install and one after, and the table is checked.
# Run: lampOS/tools/call-times-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/call-times.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
fails=0
check() { # check WHAT CONDITION...
  local what=$1; shift
  if "$@"; then echo "ok: $what"; else echo "FAIL: $what"; fails=$((fails + 1)); fi
}

# journalctl: the window that starts at 13:00 is before the install, 14:00
# after; its arguments are kept to be checked.
cat >"$T/journalctl" <<'STAND_IN'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_DIR/journalctl.args"
since=
while [[ $# -gt 0 ]]; do [[ $1 == --since ]] && since=$2; shift; done
voice() { echo "$1 lamp-4ace lamp-voice[812]: lamp-voice $2: request $3 asked for $4; $5 in $6 ms; the answer was sent"; }
actions() { echo "$1 lamp-4ace lamp-actions[640]: lamp-actions: $2: done in $3 ms"; }
case $since in
  13:00)
    t=1760940000
    for pair in 54:3 52:2 60:4 51:2 55:3; do
      actions "$t.100000" light_colour "${pair#*:}"
      voice "$t.150000" second $t light_colour "the executive answered" "${pair%:*}"
      t=$((t + 10))
    done
    for ms in 49 50 48; do voice "$t.000000" provider $t use_skill "the executive answered" $ms; t=$((t + 10)); done
    voice "$t.000000" second $t remember "the memory answered" 30
    voice "$t.500000" second $((t + 1)) set_timer "the executive did not answer" 1500
    echo "$t.600000 lamp-4ace lamp-voice[812]: lamp-voice second: heard request 9: zebrafish words"
    echo "$t.700000 lamp-4ace lamp-actions[640]: lamp-actions: a caller was dropped after 3 ms: broken pipe"
    ;;
  14:00)
    t=1760943600
    for pair in 4:2 3:2 5:3 4:2 6:3; do
      voice "$t.150000" second $t light_colour "the executive answered" "${pair%:*}"
      actions "$t.151000" light_colour "${pair#*:}"
      t=$((t + 10))
    done
    for ms in 1 1 2; do voice "$t.000000" provider $t use_skill "the executive answered" $ms; t=$((t + 10)); done
    # A light call lamp-actions answered two seconds apart is not matched.
    voice "$t.000000" second $t light_level "the executive answered" 9
    actions "$((t + 2)).000000" light_level 4
    ;;
esac
STAND_IN
chmod +x "$T/journalctl"
export FAKE_DIR=$T

out=$(JOURNALCTL="$T/journalctl" "$SCRIPT" 13:00 13:30 14:00 14:30 2>&1)
status=$?
echo "$out"
check "it exits 0" test "$status" -eq 0
has() { grep -qF -- "$1" <<<"$out"; }
check "the header names both windows" has "| call | | before (13:00 to 13:30) | after (14:00 to 14:30) |"
check "a light call's wait, before and after" has '| `light_colour` | waited | 54 (51–60), 5 calls | 4 (3–6), 5 calls |'
check "lamp-actions' own time, matched by name" has '| `light_colour` | lamp-actions | 3 (2–4), 5 calls | 2 (2–3), 5 calls |'
check "the rest: the wait less lamp-actions' time" has '| `light_colour` | the rest | 51 (49–56), 5 calls | 2 (1–3), 5 calls |'
check "a skill's call, the voice's wait only" has '| `use_skill` | waited | 49 (48–50), 3 calls | 1 (1–2), 3 calls |'
check "no lamp-actions row for a skill's call" bash -c '! grep -qF "| \`use_skill\` | lamp-actions" <<<"$1"' _ "$out"
check "another part's call is shown, in one window" has '| `remember` | waited | 30 (30–30), 1 call | none |'
check "a call not answered is counted apart" has '| `set_timer` | not answered | 1500 (1500–1500), 1 call | none |'
check "an answer two seconds from its call is not matched" has '| `light_level` | waited | none | 9 (9–9), 1 call |'
check "nor shown as lamp-actions' time" bash -c '! grep -qF "| \`light_level\` | lamp-actions" <<<"$1"' _ "$out"
check "the rows of a call go waited, lamp-actions, the rest" bash -c '[[ $(grep -F "\`light_colour\`" <<<"$1" | cut -d"|" -f3 | tr -d " " | tr "\n" ,) == waited,lamp-actions,therest, ]]' _ "$out"
check "no word heard reaches the table" bash -c '! grep -q zebrafish <<<"$1"' _ "$out"
check "a dropped caller is no call" bash -c '! grep -q "a caller" <<<"$1"' _ "$out"
check "journalctl is asked for the two parts, short-unix, each window" \
  bash -c '[[ $(grep -c -- "--output=short-unix --unit lamp-voice --unit lamp-actions --since 1[34]:00 --until 1[34]:30" "$1") == 2 ]]' _ "$T/journalctl.args"

out=$("$SCRIPT" 13:00 13:30 2>&1)
status=$?
check "two times is a usage error" test "$status" -eq 2
check "it says how to use it" grep -q usage <<<"$out"

echo "$fails failed"
[[ $fails -eq 0 ]]
